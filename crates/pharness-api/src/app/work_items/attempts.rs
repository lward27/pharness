use super::super::approvals::{active_permission_grants, create_permission_grant_record};
use super::super::audit::{
    append_change_set_audit_event, append_work_item_audit_event, append_workspace_audit_event,
};
use super::super::auth::OperatorIdentity;
use super::super::clock::{current_millis, unique_suffix};
use super::super::environment::select_profile;
use super::super::event_evidence::shell_test_evidence;
use super::super::hashing::material_hash;
use super::super::policy::run_policy;
use super::super::validation::{clean_optional_text, required_text};
use super::super::{ApiError, AppState};
use super::lifecycle::WorkItemStatus;
use super::preflight::work_item_target_supported;
use crate::dto::{
    CaptureWorkItemChangeSetRequest, CreateChangeSetResponse, CreatePermissionGrantRequest,
    ExecuteWorkItemRequest, ExecuteWorkItemResponse, ReplanWorkItemRequest, ReplanWorkItemResponse,
    TransitionWorkItemRequest, WorkItemResponse, WorkspaceResponse, WorkspacesResponse,
};
use crate::workspace::collect_git_evidence;
use axum::extract::{Path, Query, State};
use axum::{Extension, Json};
use pharness_core::{
    AgentEvent, EventId, EventKind, RepositoryContract, RunBudgetConsumption, RunId, RunScope,
    SessionId,
};
use pharness_runhost::WorkspaceSourceSpec;
use pharness_store::{
    CreateArtifact, CreateChangeSet, CreateEnvironmentPreparation, CreateRun, CreateSession,
    CreateWorkspace, SqliteStore, StoredRun, StoredWorkItem, StoredWorkspace,
    UpdateEnvironmentPreparation, UpdateWorkspaceExecution, WorkspaceListFilter,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path as FsPath;

pub(in crate::app) async fn transition_work_item(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(work_item_id): Path<String>,
    Json(request): Json<TransitionWorkItemRequest>,
) -> Result<Json<WorkItemResponse>, ApiError> {
    let current = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    let target = WorkItemStatus::parse(&request.target_status)?;
    WorkItemStatus::parse(&current.status)?.ensure_can_transition_to(target)?;
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    let work_item = state
        .store
        .update_work_item_status(
            &work_item_id,
            target.as_str(),
            actor.clone(),
            reason.clone(),
        )
        .await?;
    append_work_item_audit_event(
        &state.store,
        &work_item,
        &format!("work_item.{}", target.as_str()),
        actor,
        json!({
            "previous_status": current.status,
            "status": work_item.status,
            "reason": reason,
        }),
    )
    .await?;
    Ok(Json(work_item.into()))
}

pub(in crate::app) async fn cancel_work_item(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(work_item_id): Path<String>,
    Json(request): Json<TransitionWorkItemRequest>,
) -> Result<Json<WorkItemResponse>, ApiError> {
    if request.target_status != "cancelled" {
        return Err(ApiError::bad_request(
            "work item cancel requires target_status cancelled",
        ));
    }
    transition_work_item(State(state), identity, Path(work_item_id), Json(request)).await
}

pub(in crate::app) async fn replan_work_item(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(work_item_id): Path<String>,
    Json(request): Json<ReplanWorkItemRequest>,
) -> Result<Json<ReplanWorkItemResponse>, ApiError> {
    let work_item = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    if !matches!(work_item.status.as_str(), "blocked" | "failed") {
        return Err(ApiError::conflict(
            "WorkItem replan requires blocked or failed status",
        ));
    }
    if !work_item_target_supported(&work_item) {
        return Err(ApiError::conflict(
            "WorkItem replan is limited to dev or the exact protected production target",
        ));
    }
    if work_item.attempt_count >= work_item.max_attempts {
        return Err(ApiError::conflict("WorkItem attempt budget is exhausted"));
    }
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("WorkItem has no WorkPlan to replan"))?;
    if work_plan.status != "approved" {
        return Err(ApiError::conflict(
            "WorkItem WorkPlan must remain approved before a retry can be scheduled",
        ));
    }
    if state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?
        .is_some()
    {
        return Err(ApiError::conflict(
            "WorkItem already has a ChangeSet; revise and review the WorkPlan before another coding attempt",
        ));
    }

    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = required_text(request.reason, "reason")?;
    let previous_status = work_item.status.clone();
    let work_item = state
        .store
        .finish_work_item_attempt(
            &work_item.id,
            "awaiting_approval",
            actor.clone(),
            Some(reason.clone()),
        )
        .await?;
    let attempts_remaining = work_item.max_attempts - work_item.attempt_count;
    append_work_item_audit_event(
        &state.store,
        &work_item,
        "work_item.replanned",
        actor,
        json!({
            "previous_status": previous_status,
            "reason": reason,
            "work_plan_id": work_plan.id,
            "attempt_count": work_item.attempt_count,
            "max_attempts": work_item.max_attempts,
            "attempts_remaining": attempts_remaining,
        }),
    )
    .await?;
    Ok(Json(ReplanWorkItemResponse {
        work_item: work_item.into(),
        work_plan: work_plan.into(),
        attempts_remaining,
    }))
}

pub(in crate::app) async fn execute_work_item(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(work_item_id): Path<String>,
    Json(request): Json<ExecuteWorkItemRequest>,
) -> Result<Json<ExecuteWorkItemResponse>, ApiError> {
    let local_workspace = state.worker.supports_local_workspace();
    let remote_workspace = state.worker.supports_remote_workspace();
    if !local_workspace && !remote_workspace {
        return Err(ApiError::conflict(
            "real coding alpha requires a local or Kubernetes worker",
        ));
    }
    if remote_workspace && !state.workspace.remote_configured() {
        return Err(ApiError::conflict(
            "Kubernetes coding requires PHARNESS_WORKSPACE_ALLOWED_REMOTE_REPOS",
        ));
    }
    let work_item = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    if !work_item_target_supported(&work_item) {
        return Err(ApiError::conflict(
            "coding execution is limited to dev or the exact protected production target",
        ));
    }
    let environment_profile = match work_item.environment_profile_id.as_deref() {
        Some(profile_id) => Some(
            select_profile(
                &state.environment_profiles,
                profile_id,
                &work_item.source_repo,
            )
            .map_err(ApiError::conflict)?
            .clone(),
        ),
        None if work_item.production_impacting => {
            return Err(ApiError::conflict(
                "production coding requires an immutable environment profile",
            ));
        }
        None => None,
    };
    if environment_profile.is_some() && !remote_workspace {
        return Err(ApiError::conflict(
            "immutable environment profile execution requires the Kubernetes worker",
        ));
    }
    if work_item.status != "awaiting_approval" {
        return Err(ApiError::conflict(
            "WorkItem must be awaiting_approval before an execution attempt can start",
        ));
    }
    if work_item.attempt_count >= work_item.max_attempts {
        return Err(ApiError::conflict("WorkItem attempt budget is exhausted"));
    }
    if request
        .max_turns
        .is_some_and(|requested| requested != work_item.run_budget.initial_turns)
    {
        return Err(ApiError::conflict(
            "attempt max_turns is fixed by the WorkItem RunBudget; update it through the wizard or a budget extension",
        ));
    }
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("WorkItem has no WorkPlan"))?;
    if work_plan.status != "approved" {
        return Err(ApiError::conflict(
            "WorkItem WorkPlan must be approved before source execution",
        ));
    }
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let workspace = state
        .store
        .list_workspaces(WorkspaceListFilter {
            work_item_id: Some(work_item_id.clone()),
            status: Some("declared".to_string()),
            limit: 1,
            ..Default::default()
        })
        .await?
        .into_iter()
        .next();
    let workspace = match workspace {
        Some(workspace) => workspace,
        None => {
            state
                .store
                .create_workspace(CreateWorkspace {
                    id: format!("ws_{}", unique_suffix()),
                    work_item_id: work_item.id.clone(),
                    run_id: None,
                    status: "declared".to_string(),
                    source_repo: work_item.source_repo.clone(),
                    source_ref: work_item.source_ref.clone(),
                    resolved_commit: None,
                    branch: None,
                    retention_status: "ephemeral".to_string(),
                    actor: actor.clone(),
                    reason: Some("retry attempt requires a fresh isolated workspace".to_string()),
                })
                .await?
        }
    };
    let reason = clean_optional_text(request.reason).unwrap_or_else(|| {
        if local_workspace {
            "bounded local coding attempt".to_string()
        } else {
            "bounded Kubernetes coding attempt".to_string()
        }
    });
    let attempt = work_item.attempt_count + 1;
    let (cwd, branch, base_commit, workspace_source, workspace_status, execution_kind) =
        if local_workspace {
            let provisioned = state
                .workspace
                .provision(
                    &work_item.id,
                    attempt,
                    &work_item.source_repo,
                    &work_item.source_ref,
                    work_item.source_commit.as_deref(),
                )
                .await
                .map_err(|error| ApiError::conflict(error.to_string()))?;
            (
                provisioned.cwd.to_string_lossy().to_string(),
                provisioned.branch,
                Some(provisioned.resolved_commit),
                None,
                "executing",
                "local_workspace",
            )
        } else {
            let branch = format!("pharness/{}/attempt-{attempt}", work_item.id);
            let source = WorkspaceSourceSpec {
                workspace_id: workspace.id.clone(),
                source_repo: work_item.source_repo.clone(),
                source_ref: work_item.source_ref.clone(),
                source_commit: work_item.source_commit.clone(),
                branch: branch.clone(),
                resolved_commit: None,
            };
            state
                .workspace
                .remote_source_allowed(&source)
                .map_err(|error| ApiError::conflict(error.to_string()))?;
            (
                state.worker.effective_cwd("/workspace"),
                branch,
                None,
                Some(source),
                "provisioning",
                "kubernetes_workspace",
            )
        };
    let run_id = RunId::new(format!("run_{}", unique_suffix()));
    let session_id = SessionId::new(format!("ses_{}", run_id.as_str()));
    let run_scope = RunScope {
        run_id: Some(run_id.to_string()),
        namespace: work_item.target_namespace.clone(),
        repo: Some(work_item.source_repo.clone()),
        branch: Some(branch.clone()),
        work_item_id: Some(work_item.id.clone()),
        workspace_id: Some(workspace.id.clone()),
        work_plan_id: Some(work_plan.id.clone()),
        change_set_id: None,
        production_impacting: work_item.production_impacting,
    };
    let contract = work_item
        .repository_contract_json
        .clone()
        .map(serde_json::from_value::<RepositoryContract>)
        .transpose()
        .map_err(|error| {
            ApiError::internal(format!("stored repository contract is invalid: {error}"))
        })?;
    if work_item.production_impacting && contract.is_none() {
        return Err(ApiError::conflict(
            "production coding requires a validated repository contract",
        ));
    }
    if let (Some(contract), Some(profile)) = (contract.as_ref(), environment_profile.as_ref()) {
        contract
            .validate_for_profile(profile)
            .map_err(|error| ApiError::conflict(error.to_string()))?;
    }
    if let Some(contract) = contract.as_ref() {
        create_permission_grant_record(
            &state.store,
            CreatePermissionGrantRequest {
                subject: state.policy.subject.clone(),
                created_by: actor.clone(),
                reason: format!(
                    "attempt-scoped workspace authorization for WorkItem {} run {}",
                    work_item.id, run_id
                ),
                scope: json!({
                    "environment": state.policy.environment,
                    "capability_kinds": ["filesystem"],
                    "actions": ["write_file", "patch_file", "create_directory"],
                    "max_risk": "medium",
                    "namespaces": work_item.target_namespace.iter().collect::<Vec<_>>(),
                    "repos": [work_item.source_repo],
                    "branches": [branch],
                    "run_ids": [run_id.to_string()],
                    "workspace_ids": [workspace.id],
                    "writable_path_globs": contract.writable_paths,
                    "work_item_ids": [work_item.id],
                    "work_plan_ids": [work_plan.id],
                    "production_impacting": work_item.production_impacting,
                }),
                policy: json!({ "policy_mode": "trusted_writes" }),
                expires_at: Some((current_millis() + 4 * 60 * 60 * 1_000).to_string()),
            },
        )
        .await?;
    }
    let mut policy = run_policy(&state.policy, None);
    policy.permission_grants = active_permission_grants(&state.store).await?;
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: format!("WorkItem attempt: {}", work_item.title),
            cwd: cwd.clone(),
        })
        .await?;
    let run = state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: coding_task_prompt(&work_item),
            cwd: cwd.clone(),
            max_turns: work_item.run_budget.initial_turns,
            initial_status: if environment_profile.is_some() {
                "preparing".to_string()
            } else {
                "queued".to_string()
            },
            execution_target_json: json!({
                "kind": execution_kind,
                "policy": &policy,
                "run_scope": run_scope.to_optional_json(),
                "workspace": {
                    "base_commit": base_commit,
                    "branch": branch,
                },
                "workspace_source": workspace_source,
                "run_budget": &work_item.run_budget,
                "environment_profile_id": work_item.environment_profile_id,
                "repository_contract": work_item.repository_contract_json,
                "selected_acceptance_commands": work_item.acceptance_criteria,
                "runner_profile": environment_profile.clone(),
            }),
        })
        .await?;
    let run = state
        .store
        .set_run_budget(
            &run.id,
            &work_item.run_budget,
            &RunBudgetConsumption {
                allowed_turns: work_item.run_budget.initial_turns,
                allowed_tokens: work_item.run_budget.initial_tokens,
                ..RunBudgetConsumption::default()
            },
        )
        .await?;
    let run = state.store.set_run_origin(&run.id, "controller").await?;
    let run = state
        .store
        .set_run_created_by(&run.id, actor.clone())
        .await?;
    state
        .store
        .append_event(&AgentEvent {
            event_id: EventId::new(format!("evt_{}_1", run_id.as_str())),
            session_id: session_id.clone(),
            run_id: run_id.clone(),
            seq: 1,
            kind: EventKind::RunQueued,
            payload: json!({
                "source": "work_item.execute",
                "worker": state.worker.mode(),
                "provider": state.worker.config_json().get("provider"),
                "model": state.worker.config_json().get("model"),
                "run_scope": run_scope.to_optional_json(),
                "base_commit": base_commit,
                "branch": branch,
            }),
        })
        .await?;
    let workspace = state
        .store
        .update_workspace_execution(
            &workspace.id,
            UpdateWorkspaceExecution {
                run_id: Some(run_id.clone()),
                status: workspace_status.to_string(),
                resolved_commit: base_commit.clone(),
                branch: Some(branch.clone()),
                actor: actor.clone(),
                reason: Some(reason.clone()),
            },
        )
        .await?;
    let work_item = state
        .store
        .start_work_item_attempt(&work_item.id, &run_id, actor.clone(), Some(reason))
        .await?;
    append_workspace_audit_event(
        &state.store,
        &workspace,
        if local_workspace {
            "workspace.provisioned"
        } else {
            "workspace.provisioning_requested"
        },
        actor.clone(),
    )
    .await?;
    append_work_item_audit_event(
        &state.store,
        &work_item,
        "work_item.execution_started",
        actor,
        json!({ "workspace_id": workspace.id, "run_id": run_id, "base_commit": workspace.resolved_commit, "branch": workspace.branch, "execution_kind": execution_kind }),
    )
    .await?;
    if let Some(profile) = environment_profile.as_ref() {
        let source_commit = work_item
            .source_commit
            .clone()
            .ok_or_else(|| ApiError::conflict("environment preparation requires source_commit"))?;
        let preparation = state
            .store
            .create_environment_preparation(CreateEnvironmentPreparation {
                id: format!("prep_{}", unique_suffix()),
                work_item_id: work_item.id.clone(),
                workspace_id: workspace.id.clone(),
                run_id: Some(run.id.clone()),
                status: "queued".to_string(),
                environment_profile_id: profile.id.clone(),
                source_commit,
            })
            .await?;
        let receipt = state
            .worker
            .dispatch_environment_preparation(&run, profile)
            .await
            .map_err(|error| ApiError::internal(error.to_string()))?;
        state
            .store
            .update_environment_preparation(UpdateEnvironmentPreparation {
                id: preparation.id,
                status: "running".to_string(),
                project_contract_json: None,
                project_contract_hash: None,
                environment_snapshot_json: None,
                logs_json: json!([{"step":"dispatch","status":"succeeded","job_name":receipt.job_name}]),
                error: None,
            })
            .await?;
    } else {
        state.worker.spawn_run(run.clone(), cwd);
    }
    Ok(Json(ExecuteWorkItemResponse {
        work_item: work_item.into(),
        workspace: workspace.into(),
        run: run.into(),
    }))
}

pub(in crate::app) async fn capture_work_item_change_set(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(work_item_id): Path<String>,
    Json(request): Json<CaptureWorkItemChangeSetRequest>,
) -> Result<Json<CreateChangeSetResponse>, ApiError> {
    let work_item = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    if !matches!(work_item.status.as_str(), "executing" | "verifying") {
        return Err(ApiError::conflict(
            "WorkItem has no completed coding attempt to capture",
        ));
    }
    let workspace = state
        .store
        .list_workspaces(WorkspaceListFilter {
            work_item_id: Some(work_item_id.clone()),
            status: Some("verifying".to_string()),
            limit: 1,
            ..Default::default()
        })
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| {
            ApiError::conflict("WorkItem has no completed workspace ready for capture")
        })?;
    let run_id = workspace
        .run_id
        .clone()
        .ok_or_else(|| ApiError::conflict("workspace has no run"))?;
    let run = state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
    if run.status != "completed" {
        return Err(ApiError::conflict(
            "coding run must complete before ChangeSet capture",
        ));
    }
    if state
        .store
        .get_change_set_by_work_plan(
            &state
                .store
                .get_work_plan_by_work_item(&work_item_id)
                .await?
                .ok_or_else(|| ApiError::conflict("WorkItem has no WorkPlan"))?
                .id,
        )
        .await?
        .is_some()
    {
        return Err(ApiError::conflict(
            "WorkItem already has a captured ChangeSet",
        ));
    }
    let base_commit = workspace
        .resolved_commit
        .as_deref()
        .ok_or_else(|| ApiError::conflict("workspace has no pinned base commit"))?;
    let (evidence, diff_artifact_id, status_artifact_id, test_events) = if run
        .execution_target_json
        .get("workspace_source")
        .is_some()
    {
        worker_workspace_evidence(&state.store, &run, &workspace).await?
    } else {
        state
            .workspace
            .ensure_managed(FsPath::new(&run.cwd))
            .await
            .map_err(|error| ApiError::conflict(error.to_string()))?;
        let evidence = collect_git_evidence(FsPath::new(&run.cwd), base_commit)
            .await
            .map_err(|error| ApiError::conflict(error.to_string()))?;
        let artifact = state
            .store
            .create_artifact(CreateArtifact {
                id: format!("art_{}", unique_suffix()),
                session_id: run.session_id.clone(),
                run_id: Some(run_id.clone()),
                kind: "workspace_git_diff".to_string(),
                label: format!("Git diff for {}", work_item.title),
                mime_type: Some("text/x-diff".to_string()),
                path: None,
                content_text: Some(evidence.diff.clone()),
                content_json: None,
            })
            .await?;
        let test_events = shell_test_evidence(&state.store.list_events(&run_id).await?);
        let source_status = state.store.create_artifact(CreateArtifact {
                id: format!("art_{}", unique_suffix()), session_id: run.session_id.clone(), run_id: Some(run_id.clone()),
                kind: "workspace_git_status".to_string(), label: "Captured Git status and test summaries".to_string(),
                mime_type: Some("application/json".to_string()), path: None, content_text: None,
                content_json: Some(json!({ "status": evidence.status, "changed_paths": evidence.changed_paths, "test_events": test_events })),
            }).await?;
        (evidence, artifact.id, source_status.id, test_events)
    };
    if evidence.changed_paths.is_empty() || evidence.diff.is_empty() {
        return Err(ApiError::conflict("coding run produced no source diff"));
    }
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("WorkItem has no WorkPlan"))?;
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let diff_hash = format!("{:x}", Sha256::digest(evidence.diff.as_bytes()));
    let change_set_json = json!({
        "source": { "kind": "workspace_git", "work_item_id": work_item.id, "workspace_id": workspace.id, "base_commit": base_commit, "branch": workspace.branch },
        "evidence": { "git_diff_artifact_id": diff_artifact_id, "git_status_artifact_id": status_artifact_id, "diff_sha256": diff_hash, "changed_paths": evidence.changed_paths },
        "verification": { "run_id": run_id, "test_event_count": test_events.len() }
    });
    let change_set = state
        .store
        .create_change_set(CreateChangeSet {
            id: format!("cset_{}", unique_suffix()),
            work_item_id: Some(work_item.id.clone()),
            work_plan_id: work_plan.id.clone(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: run.session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "proposed".to_string(),
            title: format!("ChangeSet: {}", work_item.title),
            summary: run
                .result_json
                .as_ref()
                .and_then(|result| result.get("summary"))
                .and_then(Value::as_str)
                .unwrap_or(&work_item.intent)
                .to_string(),
            risk_level: work_plan.risk_level.clone(),
            material_hash: material_hash(&change_set_json)?,
            resource_namespace: work_plan.resource_namespace.clone(),
            resource_kind: work_plan.resource_kind.clone(),
            resource_name: work_plan.resource_name.clone(),
            change_set_json,
        })
        .await?;
    let workspace = state
        .store
        .update_workspace_execution(
            &workspace.id,
            UpdateWorkspaceExecution {
                run_id: Some(run_id.clone()),
                status: "captured".to_string(),
                resolved_commit: Some(base_commit.to_string()),
                branch: workspace.branch.clone(),
                actor: actor.clone(),
                reason: clean_optional_text(request.reason),
            },
        )
        .await?;
    let work_item = state
        .store
        .update_work_item_status(
            &work_item.id,
            "awaiting_approval",
            actor.clone(),
            Some("real ChangeSet captured; source review required".to_string()),
        )
        .await?;
    append_change_set_audit_event(&state.store, &change_set, "change_set.captured", actor.clone(), None, json!({ "workspace_id": workspace.id, "run_id": run_id, "git_diff_artifact_id": diff_artifact_id, "git_status_artifact_id": status_artifact_id })).await?;
    append_workspace_audit_event(
        &state.store,
        &workspace,
        "workspace.change_set_captured",
        actor.clone(),
    )
    .await?;
    append_work_item_audit_event(
        &state.store,
        &work_item,
        "work_item.awaiting_approval",
        actor,
        json!({ "change_set_id": change_set.id }),
    )
    .await?;
    Ok(Json(CreateChangeSetResponse {
        change_set: change_set.into(),
        created: true,
    }))
}

pub(in crate::app) fn coding_task_prompt(work_item: &StoredWorkItem) -> String {
    let criteria = work_item
        .acceptance_criteria
        .iter()
        .map(|criterion| format!("- {criterion}"))
        .collect::<Vec<_>>()
        .join("\n");
    let target = if work_item.production_impacting {
        "protected-production source WorkItem"
    } else {
        "development WorkItem"
    };
    format!(
        "Implement this {target} in the current workspace.\n\nIntent:\n{}\n\nAcceptance criteria:\n{}\n\nBoundaries: work only in this workspace; do not read secrets; do not use network access; do not push, commit, create a pull request, deploy, or modify Git configuration other than the isolated workspace's local configuration. Run focused tests when available. Finish with a concise summary of changes and tests.",
        work_item.intent,
        if criteria.is_empty() { "- No explicit criteria were supplied." } else { &criteria }
    )
}

pub(in crate::app) async fn worker_workspace_evidence(
    store: &SqliteStore,
    run: &StoredRun,
    workspace: &StoredWorkspace,
) -> Result<(crate::workspace::GitEvidence, String, String, Vec<Value>), ApiError> {
    let artifacts = store.list_artifacts(&run.id).await?;
    let diff_artifact = artifacts
        .iter()
        .rev()
        .find(|artifact| artifact.kind == "workspace_git_diff")
        .ok_or_else(|| ApiError::conflict("worker did not retain workspace Git diff evidence"))?;
    let status_artifact = artifacts
        .iter()
        .rev()
        .find(|artifact| artifact.kind == "workspace_git_status")
        .ok_or_else(|| ApiError::conflict("worker did not retain workspace Git status evidence"))?;
    let diff = diff_artifact
        .content_text
        .clone()
        .ok_or_else(|| ApiError::conflict("workspace Git diff artifact has no text content"))?;
    let status_json = status_artifact
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("workspace Git status artifact has no JSON content"))?;
    let base_commit = status_json
        .get("base_commit")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("workspace Git status artifact has no base commit"))?;
    if workspace.resolved_commit.as_deref() != Some(base_commit) {
        return Err(ApiError::conflict(
            "workspace Git evidence does not match the pinned base commit",
        ));
    }
    let branch = status_json
        .get("branch")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("workspace Git status artifact has no branch"))?;
    if workspace.branch.as_deref() != Some(branch) {
        return Err(ApiError::conflict(
            "workspace Git evidence does not match the pinned branch",
        ));
    }
    let status = status_json
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("workspace Git status artifact has no status"))?
        .to_string();
    let changed_paths = status_json
        .get("changed_paths")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::conflict("workspace Git status artifact has no changed paths"))?
        .iter()
        .map(|path| {
            path.as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| ApiError::conflict("workspace changed path is not a string"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let test_events = status_json
        .get("test_events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok((
        crate::workspace::GitEvidence {
            status,
            diff,
            changed_paths,
        },
        diff_artifact.id.clone(),
        status_artifact.id.clone(),
        test_events,
    ))
}

#[derive(Debug, Default, serde::Deserialize)]
pub(in crate::app) struct ListWorkspacesQuery {
    pub(in crate::app) work_item_id: Option<String>,
    pub(in crate::app) run_id: Option<String>,
    pub(in crate::app) status: Option<String>,
    pub(in crate::app) limit: Option<u32>,
    pub(in crate::app) offset: Option<u32>,
}

pub(in crate::app) async fn list_workspaces(
    State(state): State<AppState>,
    Query(query): Query<ListWorkspacesQuery>,
) -> Result<Json<WorkspacesResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let workspaces = state
        .store
        .list_workspaces(WorkspaceListFilter {
            work_item_id: clean_optional_text(query.work_item_id),
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            status: clean_optional_text(query.status),
            limit,
            offset,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let count = workspaces.len();
    Ok(Json(WorkspacesResponse {
        workspaces,
        count,
        limit,
        offset,
    }))
}

pub(in crate::app) async fn get_workspace(
    State(state): State<AppState>,
    Path(workspace_id): Path<String>,
) -> Result<Json<WorkspaceResponse>, ApiError> {
    let workspace = state
        .store
        .get_workspace(&workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("workspace", &workspace_id))?;
    Ok(Json(workspace.into()))
}
