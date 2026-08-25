use super::approvals::create_permission_grant_record;
use super::clock::current_millis;
use super::hashing::canonical_material_hash;
use super::identifiers::{is_git_sha, new_prefixed_id};
use super::products::ensure_repo_mode_enabled;
use super::validation::required_text;
use super::{ApiError, AppState};
use crate::dto::{
    CreatePermissionGrantRequest, DeliverySegmentResourceResponse, DeliverySegmentResponse,
    ReconcileWorkItemResponse, WorkItemActionResponse, WorkItemFlowResponse,
};
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pharness_core::{
    AgentEvent, EventId, EventKind, RunBudgetConsumption, RunId, RunScope, SessionId,
};
use pharness_store::{
    CreateAgentContextPack, CreateEnvironmentPreparation, CreateEvidenceValidation,
    CreateOperatorAnnotation, CreateRepoWorkItem, CreateRun, CreateSession,
    CreateStageChainAuthorization, CreateStageExecution, CreateWorkspace, SealStageOutcome,
    StoredRepoWorkItemMetadata, StoredStageOutcome, UpdateEnvironmentPreparation,
    UpdateWorkspaceExecution, WorkspaceListFilter,
};
use serde::Deserialize;
use serde_json::{json, Value};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/products/:product_id/work-items/preflight",
            post(preflight_repo_work_item),
        )
        .route(
            "/api/products/:product_id/work-items",
            post(create_repo_work_item),
        )
        .route(
            "/api/work-items/:work_item_id/stage-executions",
            get(list_stage_executions),
        )
        .route(
            "/api/stage-executions/:stage_execution_id",
            get(get_stage_execution),
        )
        .route(
            "/api/stage-executions/:stage_execution_id/outcome",
            get(get_stage_outcome),
        )
        .route(
            "/api/stage-executions/:stage_execution_id/context-pack",
            get(get_stage_context_pack),
        )
        .route(
            "/api/work-items/:work_item_id/annotations",
            get(list_annotations).post(create_annotation),
        )
}

pub(in crate::app) async fn is_repo_work_item(
    state: &AppState,
    work_item_id: &str,
) -> Result<bool, ApiError> {
    Ok(state
        .store
        .get_repo_work_item_metadata(work_item_id)
        .await?
        .is_some())
}

pub(in crate::app) async fn repo_work_item_flow(
    state: &AppState,
    work_item_id: &str,
) -> Result<WorkItemFlowResponse, ApiError> {
    ensure_repo_mode_enabled(state)?;
    let metadata = repo_metadata(state, work_item_id).await?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let work_plan = state.store.get_work_plan_by_work_item(work_item_id).await?;
    let executions = state.store.list_stage_executions(work_item_id).await?;
    let outcomes = state
        .store
        .list_effective_stage_outcomes(work_item_id)
        .await?;
    let chain = state
        .store
        .active_stage_chain_authorization(work_item_id)
        .await?;
    let action_rail =
        derive_repo_actions(&metadata, work_plan.as_ref(), &executions, chain.as_ref())?;
    let workspaces = state
        .store
        .list_workspaces(WorkspaceListFilter {
            work_item_id: Some(work_item_id.into()),
            limit: 100,
            ..WorkspaceListFilter::default()
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let audit_events = state
        .store
        .list_audit_events(Some("work_item"), Some(work_item_id), None, 100)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    let first_action = action_rail.first();
    let work_item_response: crate::dto::WorkItemResponse = work_item.clone().into();
    let reconcile_preview = ReconcileWorkItemResponse {
        action: first_action
            .map(|action| action.id.clone())
            .unwrap_or_else(|| "wait".into()),
        applied: false,
        work_item: work_item_response.clone(),
        work_plan: work_plan.clone().map(Into::into),
        workspace: workspaces.last().cloned(),
        run: None,
        change_set: None,
        git_delivery_preflight: None,
        pipeline_intent: None,
        pipeline_execution_preflight: None,
        deployment_intent: None,
        deployment_execution_preflight: None,
        deployment_delivery: None,
        gitops_change_set: None,
        gitops_delivery: None,
        gitops_delivery_preflight: None,
        controller_wait: None,
        message: first_action
            .map(|action| action.external_effect_summary.clone())
            .unwrap_or_else(|| {
                "Repo Mode is waiting for the current stage or external boundary".into()
            }),
        boundary: first_action
            .map(|action| action.lifecycle_stage.clone())
            .unwrap_or_else(|| work_item.status.clone()),
        can_apply: first_action.is_some_and(|action| action.status == "ready"),
        effect_summary: first_action
            .map(|action| action.external_effect_summary.clone())
            .unwrap_or_else(|| "No controller action is currently available".into()),
        blockers: first_action
            .map(|action| action.blockers.clone())
            .unwrap_or_default(),
        authorization_checks: Vec::new(),
    };
    Ok(WorkItemFlowResponse {
        work_item: work_item_response,
        reconcile_preview,
        sdlc_flow: None,
        delivery_segments: repo_delivery_segments(&executions, &outcomes),
        workspaces,
        controller_waits: Vec::new(),
        audit_events,
        action_rail,
        delivery_configuration: json!({
            "kind":"repo_mode_source_only",
            "repository_id":metadata.repository_id,
            "source_commit":work_item.source_commit,
            "release":"inapplicable",
            "observe":"inapplicable",
        }),
        repo_mode: Some(json!({
            "metadata":metadata,
            "state_hash":repo_work_item_state_hash(&metadata)?,
            "stage_executions":executions,
            "effective_stage_outcomes":outcomes,
            "stage_chain_authorization":chain,
            "product_model_snapshot":{
                "id":metadata.product_model_snapshot_id,
                "hash":metadata.product_model_snapshot_hash,
            },
            "repository_contract_version_id":metadata.repository_contract_version_id,
        })),
    })
}

fn derive_repo_actions(
    metadata: &StoredRepoWorkItemMetadata,
    work_plan: Option<&pharness_store::StoredWorkPlan>,
    executions: &[pharness_store::StoredStageExecution],
    chain: Option<&pharness_store::StoredStageChainAuthorization>,
) -> Result<Vec<WorkItemActionResponse>, ApiError> {
    if metadata.closed_at.is_some() {
        return Ok(Vec::new());
    }
    let state_hash = repo_work_item_state_hash(metadata)?;
    let plan_execution = executions
        .iter()
        .rev()
        .find(|execution| execution.stage_key == pharness_core::RepoStageKey::Plan.as_str());
    let mut actions = Vec::new();
    if plan_execution.is_none() {
        actions.push(repo_action(
            "start_planner",
            "plan",
            &metadata.work_item_id,
            "ready",
            "model_execution",
            true,
            "Start one immutable repo-planner AgentRun from the sealed Discover evidence.",
            &state_hash,
            json!({"stage":"plan"}),
        )?);
        return Ok(actions);
    }
    if plan_execution.is_some_and(|execution| {
        matches!(execution.status.as_str(), "queued" | "running" | "paused")
    }) {
        return Ok(actions);
    }
    if let Some(plan) = work_plan.filter(|plan| plan.status == "proposed") {
        for approve in [true, false] {
            actions.push(repo_action(
                if approve { "approve_work_plan" } else { "reject_work_plan" },
                "plan",
                &plan.id,
                "ready",
                "human_review",
                true,
                if approve {
                    "Approve the exact Planner-submitted WorkPlan revision. This does not start coding."
                } else {
                    "Reject the exact Planner-submitted WorkPlan revision and block the WorkItem for correction."
                },
                &state_hash,
                json!({"work_plan_id":plan.id,"revision":plan.revision,"status":plan.status}),
            )?);
        }
        return Ok(actions);
    }
    if let Some(plan) = work_plan.filter(|plan| plan.status == "approved") {
        if chain.is_none() {
            actions.push(repo_action(
                "authorize_stage_chain",
                "implement",
                &plan.id,
                "ready",
                "model_execution",
                true,
                "Create one four-hour workspace grant and bind the Builder, Tester, and Verifier profiles to the approved WorkPlan. This does not authorize Git or provider mutation.",
                &state_hash,
                json!({"work_plan_id":plan.id,"revision":plan.revision}),
            )?);
        }
    }
    Ok(actions)
}

fn repo_action(
    id: &str,
    lifecycle_stage: &str,
    resource: &str,
    status: &str,
    effect_class: &str,
    approval_required: bool,
    summary: &str,
    work_item_state_hash: &str,
    bound_state: Value,
) -> Result<WorkItemActionResponse, ApiError> {
    Ok(WorkItemActionResponse {
        id: id.into(),
        lifecycle_stage: lifecycle_stage.into(),
        resource: resource.into(),
        status: status.into(),
        effect_class: effect_class.into(),
        blockers: Vec::new(),
        approval_required,
        approval_requirements: approval_required
            .then(|| vec![id.into()])
            .unwrap_or_default(),
        external_effect_summary: summary.into(),
        state_hash: canonical_material_hash(&json!({
            "action":id,
            "work_item_state_hash":work_item_state_hash,
            "bound_state":bound_state,
        }))?,
    })
}

fn repo_delivery_segments(
    executions: &[pharness_store::StoredStageExecution],
    outcomes: &[StoredStageOutcome],
) -> Vec<DeliverySegmentResponse> {
    [
        ("discover", "Discover"),
        ("plan", "Plan"),
        ("implement", "Implement"),
        ("test", "Test"),
        ("verify", "Verify"),
        ("source_delivery", "Source Delivery"),
    ]
    .into_iter()
    .map(|(key, label)| {
        let outcome = outcomes.iter().find(|outcome| outcome.stage_key == key);
        let execution = executions
            .iter()
            .rev()
            .find(|execution| execution.stage_key == key);
        let status = outcome
            .map(|outcome| outcome.status.as_str())
            .or_else(|| execution.map(|execution| execution.status.as_str()))
            .unwrap_or("pending");
        DeliverySegmentResponse {
            key: key.into(),
            label: label.into(),
            status: status.into(),
            summary: outcome
                .and_then(|outcome| outcome.outcome.get("stop_reason"))
                .and_then(Value::as_str)
                .unwrap_or("Awaiting its Repo Mode lifecycle boundary")
                .into(),
            stopping_reason: outcome
                .filter(|outcome| outcome.status != "succeeded")
                .and_then(|outcome| outcome.outcome.get("stop_reason"))
                .and_then(Value::as_str)
                .map(str::to_string),
            resources: execution
                .map(|execution| {
                    vec![DeliverySegmentResourceResponse {
                        kind: "stage_execution".into(),
                        id: execution.id.clone(),
                        label: format!("{} execution {}", label, execution.sequence),
                        summary: execution.stop_reason.clone(),
                    }]
                })
                .unwrap_or_default(),
        }
    })
    .collect()
}

pub(in crate::app) async fn execute_repo_work_item_action(
    state: &AppState,
    work_item_id: &str,
    action_id: &str,
    actor: String,
    reason: String,
    state_hash: String,
) -> Result<Value, ApiError> {
    ensure_repo_mode_enabled(state)?;
    let metadata = repo_metadata(state, work_item_id).await?;
    let work_plan = state.store.get_work_plan_by_work_item(work_item_id).await?;
    let executions = state.store.list_stage_executions(work_item_id).await?;
    let chain = state
        .store
        .active_stage_chain_authorization(work_item_id)
        .await?;
    let action = derive_repo_actions(&metadata, work_plan.as_ref(), &executions, chain.as_ref())?
        .into_iter()
        .find(|action| action.id == action_id)
        .ok_or_else(|| ApiError::conflict("Repo Mode action is no longer available"))?;
    if action.state_hash != state_hash {
        return Err(ApiError::conflict(
            "Repo Mode action preview is stale; refresh and retry",
        ));
    }
    if action.status != "ready" {
        return Err(ApiError::conflict("Repo Mode action is blocked"));
    }
    match action_id {
        "start_planner" => start_repo_planner(state, work_item_id, &actor, &reason).await,
        "approve_work_plan" | "reject_work_plan" => {
            let plan = work_plan.ok_or_else(|| ApiError::conflict("WorkPlan is unavailable"))?;
            if plan.status != "proposed" {
                return Err(ApiError::conflict("WorkPlan is no longer proposed"));
            }
            let target = if action_id == "approve_work_plan" {
                "approved"
            } else {
                "rejected"
            };
            let plan = state
                .store
                .update_work_plan_status(
                    &plan.id,
                    target,
                    Some(actor.clone()),
                    Some(reason.clone()),
                )
                .await?;
            let status = if target == "approved" {
                "awaiting_approval"
            } else {
                "blocked"
            };
            let item = state
                .store
                .update_repo_work_item_status(
                    work_item_id,
                    status,
                    &actor,
                    &format!("WorkPlan {} by {actor}: {reason}", target),
                    false,
                )
                .await?;
            Ok(json!({"work_plan":plan,"work_item":item}))
        }
        "authorize_stage_chain" => {
            authorize_repo_stage_chain(state, work_item_id, &actor, &reason).await
        }
        _ => Err(ApiError::conflict("unsupported Repo Mode action")),
    }
}

async fn start_repo_planner(
    state: &AppState,
    work_item_id: &str,
    actor: &str,
    reason: &str,
) -> Result<Value, ApiError> {
    if !state.worker.enabled() {
        return Err(ApiError::unavailable(
            "model execution worker is unavailable",
        ));
    }
    let metadata = repo_metadata(state, work_item_id).await?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let outcomes = state
        .store
        .list_effective_stage_outcomes(work_item_id)
        .await?;
    let model = state
        .worker
        .config_json()
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unconfigured")
        .to_string();
    let profile =
        pharness_core::compiled_agent_profiles(&model, pharness_runhost::SYSTEM_PROMPT_VERSION)
            .into_iter()
            .find(|profile| profile.id == "repo-planner")
            .ok_or_else(|| ApiError::internal("compiled repo-planner profile is unavailable"))?;
    let stage_execution_id = new_prefixed_id("stageexec");
    let context_pack_id = new_prefixed_id("context");
    let run_id = RunId::new(new_prefixed_id("run"));
    let session_id = SessionId::new(new_prefixed_id("ses"));
    let context = json!({
        "schema_version":pharness_core::AGENT_CONTEXT_SCHEMA,
        "current_intent":{"title":work_item.title,"intent":work_item.intent,"acceptance":metadata.acceptance_command_names},
        "pinned_product":{"snapshot_id":metadata.product_model_snapshot_id,"snapshot_hash":metadata.product_model_snapshot_hash},
        "pinned_repository":{"repository_id":metadata.repository_id,"source_commit":work_item.source_commit,"contract_version_id":metadata.repository_contract_version_id},
        "upstream_outcomes":outcomes.iter().map(|outcome| json!({"id":outcome.id,"stage":outcome.stage_key,"status":outcome.status,"hash":outcome.content_hash})).collect::<Vec<_>>(),
        "remaining_budgets":profile.budget,
        "policies":{"source_only":true,"manual_merge":true,"pipeline":false,"deployment":false},
        "grants":[],
        "contradictions":[],
        "risks":[],
        "operator_decisions":[],
        "evidence_catalog":outcomes.iter().map(|outcome| json!({"id":outcome.id,"kind":"stage_outcome","version":outcome.schema_version,"hash":outcome.content_hash,"stage":outcome.stage_key,"status":outcome.status})).collect::<Vec<_>>(),
    });
    let estimated_tokens = u64::try_from(context.to_string().len() / 4).unwrap_or(u64::MAX);
    if estimated_tokens > 16_000 {
        return Err(ApiError::conflict(
            "mandatory Planner context exceeds the 16,000-token context-pack limit",
        ));
    }
    let cwd = if state.worker.supports_local_workspace() {
        let path = std::env::temp_dir()
            .join("pharness-repo-mode")
            .join(run_id.as_str());
        tokio::fs::create_dir_all(&path)
            .await
            .map_err(|error| ApiError::internal(format!("planner workspace failed: {error}")))?;
        path.to_string_lossy().to_string()
    } else {
        state.worker.effective_cwd("/workspace")
    };
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: format!("Repo Planner: {}", work_item.title),
            cwd: cwd.clone(),
        })
        .await?;
    let scope = RunScope {
        run_id: Some(run_id.to_string()),
        work_item_id: Some(work_item_id.into()),
        repo: Some(work_item.source_repo.clone()),
        branch: Some(work_item.source_ref.clone()),
        ..RunScope::default()
    };
    let run = state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: format!(
                "Produce a bounded WorkPlan for this exact intent and acceptance contract: {}",
                work_item.intent
            ),
            cwd: cwd.clone(),
            max_turns: profile.budget.initial_turns,
            initial_status: "queued".into(),
            execution_target_json: json!({
                "kind":state.worker.execution_target_kind(),
                "repo_mode":{"stage_execution_id":stage_execution_id,"stage":"plan","context_pack_id":context_pack_id},
                "agent_profile":profile,
                "agent_context":context,
                "run_scope":scope.to_optional_json(),
                "run_budget":profile.budget,
            }),
        })
        .await?;
    let run = state
        .store
        .set_run_budget(
            &run.id,
            &profile.budget,
            &RunBudgetConsumption {
                allowed_turns: profile.budget.initial_turns,
                allowed_tokens: profile.budget.initial_tokens,
                ..RunBudgetConsumption::default()
            },
        )
        .await?;
    let run = state.store.set_run_origin(&run.id, "controller").await?;
    let run = state
        .store
        .set_run_created_by(&run.id, Some(actor.into()))
        .await?;
    let input_snapshot = json!({
        "context_pack_id":context_pack_id,
        "context_hash":canonical_material_hash(&context)?,
        "profile_id":profile.id,
        "profile_version":profile.version,
        "profile_hash":profile.profile_hash,
        "source_commit":work_item.source_commit,
    });
    let execution = state
        .store
        .create_stage_execution(CreateStageExecution {
            id: stage_execution_id.clone(),
            work_item_id: work_item_id.into(),
            stage_key: pharness_core::RepoStageKey::Plan.as_str().into(),
            sequence: 1,
            status: "queued".into(),
            agent_profile_id: Some(profile.id.clone()),
            agent_profile_version: Some(profile.version.clone()),
            agent_profile_hash: Some(profile.profile_hash.clone()),
            context_pack_id: None,
            run_id: Some(run.id.clone()),
            workspace_id: None,
            input_hash: canonical_material_hash(&input_snapshot)?,
            input_snapshot,
        })
        .await?;
    let pack = state
        .store
        .create_agent_context_pack(CreateAgentContextPack {
            id: context_pack_id,
            work_item_id: work_item_id.into(),
            stage_execution_id: execution.id.clone(),
            content_hash: canonical_material_hash(&context)?,
            context,
            estimated_tokens,
        })
        .await?;
    state
        .store
        .append_event(&AgentEvent {
            event_id: EventId::new(new_prefixed_id("evt")),
            session_id,
            run_id: run.id.clone(),
            seq: 1,
            kind: EventKind::RunQueued,
            payload: json!({"source":"repo_mode_controller","stage":"plan","stage_execution_id":execution.id,"actor":actor,"reason":reason}),
        })
        .await?;
    let item = state
        .store
        .update_repo_work_item_status(
            work_item_id,
            "executing",
            actor,
            "repo-planner AgentRun started",
            false,
        )
        .await?;
    state.worker.spawn_run(run.clone(), cwd);
    Ok(json!({"work_item":item,"stage_execution":execution,"context_pack":pack,"run":run}))
}

async fn authorize_repo_stage_chain(
    state: &AppState,
    work_item_id: &str,
    actor: &str,
    reason: &str,
) -> Result<Value, ApiError> {
    let metadata = repo_metadata(state, work_item_id).await?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let plan = state
        .store
        .get_work_plan_by_work_item(work_item_id)
        .await?
        .filter(|plan| plan.status == "approved")
        .ok_or_else(|| ApiError::conflict("an approved WorkPlan is required"))?;
    let contract = work_item
        .repository_contract_json
        .clone()
        .ok_or_else(|| ApiError::conflict("RepositoryContract is unavailable"))?;
    let contract: pharness_core::RepositoryContract =
        serde_json::from_value(contract).map_err(|error| {
            ApiError::internal(format!("stored RepositoryContract is invalid: {error}"))
        })?;
    let workspace = state
        .store
        .create_workspace(CreateWorkspace {
            id: new_prefixed_id("ws"),
            work_item_id: work_item_id.into(),
            run_id: None,
            status: "declared".into(),
            source_repo: work_item.source_repo.clone(),
            source_ref: work_item.source_ref.clone(),
            resolved_commit: work_item.source_commit.clone(),
            branch: Some(format!("pharness/{work_item_id}/attempt-1")),
            retention_status: "retained".into(),
            actor: Some(actor.into()),
            reason: Some(reason.into()),
        })
        .await?;
    let model = state
        .worker
        .config_json()
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unconfigured")
        .to_string();
    let profiles =
        pharness_core::compiled_agent_profiles(&model, pharness_runhost::SYSTEM_PROMPT_VERSION)
            .into_iter()
            .filter(|profile| {
                matches!(
                    profile.id.as_str(),
                    "repo-builder" | "repo-tester" | "repo-verifier"
                )
            })
            .collect::<Vec<_>>();
    if profiles.len() != 3 {
        return Err(ApiError::internal(
            "compiled Repo Mode stage chain is incomplete",
        ));
    }
    let authorization = state
        .store
        .create_stage_chain_authorization(CreateStageChainAuthorization {
            id: new_prefixed_id("chain"),
            work_item_id: work_item_id.into(),
            work_plan_id: plan.id.clone(),
            work_plan_revision: plan.revision,
            product_model_snapshot_id: metadata.product_model_snapshot_id.clone(),
            product_model_snapshot_hash: metadata.product_model_snapshot_hash.clone(),
            repository_id: metadata.repository_id.clone(),
            source_commit: work_item
                .source_commit
                .clone()
                .ok_or_else(|| ApiError::conflict("source_commit is unavailable"))?,
            workspace_id: workspace.id.clone(),
            writable_paths: serde_json::to_value(&contract.writable_paths)
                .map_err(|error| ApiError::internal(error.to_string()))?,
            profile_chain: serde_json::to_value(&profiles)
                .map_err(|error| ApiError::internal(error.to_string()))?,
            budget_chain: json!({
                "repo-builder":work_item.run_budget,
                "repo-tester":profiles.iter().find(|profile| profile.id == "repo-tester").map(|profile| &profile.budget),
                "repo-verifier":profiles.iter().find(|profile| profile.id == "repo-verifier").map(|profile| &profile.budget),
            }),
            state_hash: repo_work_item_state_hash(&metadata)?,
            created_by: actor.into(),
            creation_reason: reason.into(),
            expires_at: (current_millis() + 4 * 60 * 60 * 1_000).to_string(),
        })
        .await?;
    match start_repo_builder(
        state,
        &metadata,
        &work_item,
        &plan,
        &workspace,
        &authorization,
        &contract,
        actor,
        reason,
    )
    .await
    {
        Ok(started) => Ok(json!({
            "stage_chain_authorization":authorization,
            "workspace":workspace,
            "builder":started,
        })),
        Err(error) => {
            state
                .store
                .revoke_stage_chain_authorization(
                    &authorization.id,
                    "Builder dispatch failed before the authorized chain started",
                )
                .await?;
            Err(error)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn start_repo_builder(
    state: &AppState,
    metadata: &StoredRepoWorkItemMetadata,
    work_item: &pharness_store::StoredWorkItem,
    plan: &pharness_store::StoredWorkPlan,
    workspace: &pharness_store::StoredWorkspace,
    authorization: &pharness_store::StoredStageChainAuthorization,
    contract: &pharness_core::RepositoryContract,
    actor: &str,
    reason: &str,
) -> Result<Value, ApiError> {
    if !state.worker.enabled() {
        return Err(ApiError::unavailable(
            "model execution worker is unavailable",
        ));
    }
    let profile = authorization
        .profile_chain
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|value| serde_json::from_value::<pharness_core::AgentProfile>(value.clone()).ok())
        .filter(|profile| profile.id == "repo-builder")
        .ok_or_else(|| ApiError::conflict("chain authorization has no repo-builder profile"))?;
    let environment_profile = super::environment::select_profile(
        &state.environment_profiles,
        &contract.environment_profile,
        &work_item.source_repo,
    )
    .map_err(ApiError::conflict)?
    .clone();
    if !state.worker.supports_remote_workspace() {
        return Err(ApiError::conflict(
            "Repo Mode V1 immutable runner preparation requires kubernetes_job worker mode",
        ));
    }
    let run_id = RunId::new(new_prefixed_id("run"));
    let session_id = SessionId::new(new_prefixed_id("ses"));
    let stage_execution_id = new_prefixed_id("stageexec");
    let context_pack_id = new_prefixed_id("context");
    let branch = workspace
        .branch
        .clone()
        .ok_or_else(|| ApiError::conflict("authorized workspace has no branch"))?;
    let source_commit = work_item
        .source_commit
        .clone()
        .ok_or_else(|| ApiError::conflict("Repo Mode Builder requires source_commit"))?;
    let source = pharness_runhost::WorkspaceSourceSpec {
        workspace_id: workspace.id.clone(),
        source_repo: work_item.source_repo.clone(),
        source_ref: work_item.source_ref.clone(),
        source_commit: Some(source_commit.clone()),
        branch: branch.clone(),
        resolved_commit: None,
    };
    state
        .workspace
        .remote_source_allowed(&source)
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    let outcomes = state
        .store
        .list_effective_stage_outcomes(&work_item.id)
        .await?;
    let plan_snapshot = json!({
        "id":plan.id,
        "revision":plan.revision,
        "status":plan.status,
        "title":plan.title,
        "summary":plan.summary,
        "risk_level":plan.risk_level,
        "work_plan":plan.work_plan_json,
    });
    let plan_hash = canonical_material_hash(&plan_snapshot)?;
    let context = json!({
        "schema_version":pharness_core::AGENT_CONTEXT_SCHEMA,
        "current_intent":{"title":work_item.title,"intent":work_item.intent,"acceptance_names":metadata.acceptance_command_names,"acceptance_commands":work_item.acceptance_criteria},
        "pinned_product":{"snapshot_id":metadata.product_model_snapshot_id,"snapshot_hash":metadata.product_model_snapshot_hash},
        "pinned_repository":{"repository_id":metadata.repository_id,"source_commit":source_commit,"contract_version_id":metadata.repository_contract_version_id,"contract_hash":work_item.repository_contract_hash},
        "approved_work_plan":{"snapshot":plan_snapshot,"hash":plan_hash},
        "upstream_outcomes":outcomes.iter().map(|outcome| json!({"id":outcome.id,"stage":outcome.stage_key,"status":outcome.status,"hash":outcome.content_hash})).collect::<Vec<_>>(),
        "remaining_budgets":work_item.run_budget,
        "policies":{"source_only":true,"manual_merge":true,"agent_network":"denied","package_installation":"preparation_only"},
        "grants":[{"kind":"stage_chain","id":authorization.id,"expires_at":authorization.expires_at,"workspace_id":workspace.id,"writable_paths":contract.writable_paths}],
        "contradictions":[],
        "risks":[],
        "operator_decisions":[{"kind":"work_plan_approval","actor":actor,"reason":reason}],
        "evidence_catalog":outcomes.iter().map(|outcome| json!({"id":outcome.id,"kind":"stage_outcome","version":outcome.schema_version,"hash":outcome.content_hash,"stage":outcome.stage_key,"status":outcome.status})).collect::<Vec<_>>(),
    });
    let estimated_tokens = u64::try_from(context.to_string().len() / 4).unwrap_or(u64::MAX);
    if estimated_tokens > 16_000 {
        return Err(ApiError::conflict(
            "mandatory Builder context exceeds the 16,000-token context-pack limit",
        ));
    }
    let cwd = state.worker.effective_cwd("/workspace");
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: format!("Repo Builder: {}", work_item.title),
            cwd: cwd.clone(),
        })
        .await?;
    let scope = RunScope {
        run_id: Some(run_id.to_string()),
        repo: Some(work_item.source_repo.clone()),
        branch: Some(branch.clone()),
        work_item_id: Some(work_item.id.clone()),
        workspace_id: Some(workspace.id.clone()),
        work_plan_id: Some(plan.id.clone()),
        production_impacting: false,
        ..RunScope::default()
    };
    let grant = create_permission_grant_record(
        &state.store,
        CreatePermissionGrantRequest {
            subject: state.policy.subject.clone(),
            created_by: Some(actor.into()),
            reason: format!(
                "Repo Mode Builder grant for WorkItem {} chain {}",
                work_item.id, authorization.id
            ),
            scope: json!({
                "environment":state.policy.environment,
                "capability_kinds":["filesystem"],
                "actions":["write_file","patch_file","create_directory"],
                "max_risk":"medium",
                "repos":[work_item.source_repo],
                "branches":[branch],
                "run_ids":[run_id.to_string()],
                "workspace_ids":[workspace.id],
                "writable_path_globs":contract.writable_paths,
                "work_item_ids":[work_item.id],
                "work_plan_ids":[plan.id],
                "production_impacting":false,
            }),
            policy: json!({"policy_mode":"trusted_writes"}),
            expires_at: Some(authorization.expires_at.clone()),
        },
    )
    .await?;
    let mut policy = super::policy::run_policy(&state.policy, None);
    policy.permission_grants = super::approvals::active_permission_grants(&state.store).await?;
    let run = state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: format!(
                "Implement the approved WorkPlan for this exact Repo Mode intent: {}",
                work_item.intent
            ),
            cwd: cwd.clone(),
            max_turns: work_item.run_budget.initial_turns,
            initial_status: "preparing".into(),
            execution_target_json: json!({
                "kind":"kubernetes_workspace",
                "repo_mode":{"stage_execution_id":stage_execution_id,"stage":"implement","context_pack_id":context_pack_id,"chain_authorization_id":authorization.id},
                "agent_profile":profile,
                "agent_context":context,
                "policy":policy,
                "run_scope":scope.to_optional_json(),
                "workspace":{"base_commit":source_commit,"branch":branch},
                "workspace_source":source,
                "run_budget":work_item.run_budget,
                "environment_profile_id":work_item.environment_profile_id,
                "repository_contract":work_item.repository_contract_json,
                "selected_acceptance_commands":work_item.acceptance_criteria,
                "runner_profile":environment_profile,
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
        .set_run_created_by(&run.id, Some(actor.into()))
        .await?;
    let input_snapshot = json!({
        "chain_authorization_id":authorization.id,
        "chain_state_hash":authorization.state_hash,
        "context_pack_id":context_pack_id,
        "context_hash":canonical_material_hash(&context)?,
        "profile_id":profile.id,
        "profile_version":profile.version,
        "profile_hash":profile.profile_hash,
        "work_plan_id":plan.id,
        "work_plan_revision":plan.revision,
        "work_plan_hash":plan_hash,
        "source_commit":source_commit,
        "workspace_id":workspace.id,
    });
    let execution = state
        .store
        .create_stage_execution(CreateStageExecution {
            id: stage_execution_id,
            work_item_id: work_item.id.clone(),
            stage_key: pharness_core::RepoStageKey::Implement.as_str().into(),
            sequence: 1,
            status: "preparing".into(),
            agent_profile_id: Some(profile.id.clone()),
            agent_profile_version: Some(profile.version.clone()),
            agent_profile_hash: Some(profile.profile_hash.clone()),
            context_pack_id: None,
            run_id: Some(run.id.clone()),
            workspace_id: Some(workspace.id.clone()),
            input_hash: canonical_material_hash(&input_snapshot)?,
            input_snapshot,
        })
        .await?;
    let pack = state
        .store
        .create_agent_context_pack(CreateAgentContextPack {
            id: context_pack_id,
            work_item_id: work_item.id.clone(),
            stage_execution_id: execution.id.clone(),
            content_hash: canonical_material_hash(&context)?,
            context,
            estimated_tokens,
        })
        .await?;
    state
        .store
        .create_evidence_validation(CreateEvidenceValidation {
            id: new_prefixed_id("evalid"),
            work_item_id: work_item.id.clone(),
            stage_execution_id: Some(execution.id.clone()),
            validator_key: "approved_work_plan_snapshot".into(),
            status: "valid".into(),
            subject: json!({"work_plan_id":plan.id,"revision":plan.revision}),
            evidence_refs: json!([]),
            facts: json!({"snapshot_hash":plan_hash,"status":plan.status}),
            contradictions: json!([]),
            content_hash: canonical_material_hash(&plan_snapshot)?,
        })
        .await?;
    let workspace = state
        .store
        .update_workspace_execution(
            &workspace.id,
            UpdateWorkspaceExecution {
                run_id: Some(run.id.clone()),
                status: "preparing".into(),
                resolved_commit: Some(source_commit.clone()),
                branch: Some(branch),
                actor: Some(actor.into()),
                reason: Some(reason.into()),
            },
        )
        .await?;
    state
        .store
        .start_work_item_attempt(
            &work_item.id,
            &run.id,
            Some(actor.into()),
            Some(reason.into()),
        )
        .await?;
    let preparation = state
        .store
        .create_environment_preparation(CreateEnvironmentPreparation {
            id: new_prefixed_id("prep"),
            work_item_id: work_item.id.clone(),
            workspace_id: workspace.id.clone(),
            run_id: Some(run.id.clone()),
            status: "queued".into(),
            environment_profile_id: environment_profile.id.clone(),
            source_commit,
        })
        .await?;
    let receipt = state
        .worker
        .dispatch_environment_preparation(&run, &environment_profile)
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let preparation = state
        .store
        .update_environment_preparation(UpdateEnvironmentPreparation {
            id: preparation.id,
            status: "running".into(),
            project_contract_json: None,
            project_contract_hash: None,
            environment_snapshot_json: None,
            logs_json: json!([{"step":"dispatch","status":"succeeded","job_name":receipt.job_name}]),
            error: None,
        })
        .await?;
    Ok(json!({
        "run":run,
        "stage_execution":execution,
        "context_pack":pack,
        "workspace":workspace,
        "permission_grant":grant,
        "environment_preparation":preparation,
    }))
}

pub(in crate::app) async fn continue_repo_stage_chain(
    state: &AppState,
    completed_run: &pharness_store::StoredRun,
) -> Result<Option<Value>, ApiError> {
    let Some(stage) = completed_run
        .execution_target_json
        .pointer("/repo_mode/stage")
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };
    if completed_run.status != "completed" {
        return Ok(None);
    }
    let Some(execution_id) = completed_run
        .execution_target_json
        .pointer("/repo_mode/stage_execution_id")
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };
    let outcome = state
        .store
        .get_stage_outcome_for_execution(execution_id)
        .await?
        .ok_or_else(|| ApiError::conflict("completed Repo Mode Run has no sealed outcome"))?;
    if outcome.status != "succeeded" {
        return Ok(None);
    }
    match stage {
        "implement" => start_repo_followup_stage(state, completed_run, "test")
            .await
            .map(Some),
        "test" => start_repo_followup_stage(state, completed_run, "verify")
            .await
            .map(Some),
        _ => Ok(None),
    }
}

async fn start_repo_followup_stage(
    state: &AppState,
    completed_run: &pharness_store::StoredRun,
    stage: &str,
) -> Result<Value, ApiError> {
    let work_item_id = completed_run
        .execution_target_json
        .pointer("/run_scope/work_item_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("Repo Mode Run has no WorkItem scope"))?;
    let metadata = repo_metadata(state, work_item_id).await?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let plan = state
        .store
        .get_work_plan_by_work_item(work_item_id)
        .await?
        .filter(|plan| plan.status == "approved")
        .ok_or_else(|| ApiError::conflict("approved WorkPlan is no longer current"))?;
    let authorization = state
        .store
        .active_stage_chain_authorization(work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("stage-chain authorization is unavailable"))?;
    let now = current_millis();
    if authorization
        .expires_at
        .parse::<u128>()
        .ok()
        .is_none_or(|expires_at| expires_at <= now)
    {
        return Err(ApiError::conflict(
            "stage-chain authorization expired before the next stage",
        ));
    }
    if authorization.work_plan_id != plan.id
        || authorization.work_plan_revision != plan.revision
        || authorization.product_model_snapshot_id != metadata.product_model_snapshot_id
        || authorization.product_model_snapshot_hash != metadata.product_model_snapshot_hash
        || authorization.repository_id != metadata.repository_id
        || work_item.source_commit.as_deref() != Some(authorization.source_commit.as_str())
    {
        return Err(ApiError::conflict(
            "stage-chain authorization no longer matches the pinned WorkItem state",
        ));
    }
    let workspace = state
        .store
        .get_workspace(&authorization.workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("workspace", &authorization.workspace_id))?;
    let profile_id = match stage {
        "test" => "repo-tester",
        "verify" => "repo-verifier",
        _ => return Err(ApiError::internal("unsupported Repo Mode follow-up stage")),
    };
    let profile = authorization
        .profile_chain
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|value| serde_json::from_value::<pharness_core::AgentProfile>(value.clone()).ok())
        .filter(|profile| profile.id == profile_id)
        .ok_or_else(|| {
            ApiError::conflict(format!("chain authorization has no {profile_id} profile"))
        })?;
    let runner_profile = completed_run
        .execution_target_json
        .get("runner_profile")
        .cloned()
        .ok_or_else(|| ApiError::conflict("prepared runner profile is unavailable"))?;
    let environment_snapshot = completed_run
        .execution_target_json
        .get("environment_snapshot")
        .cloned()
        .ok_or_else(|| ApiError::conflict("prepared EnvironmentSnapshot is unavailable"))?;
    let repository_contract = work_item
        .repository_contract_json
        .clone()
        .ok_or_else(|| ApiError::conflict("RepositoryContract is unavailable"))?;
    let outcomes = state
        .store
        .list_effective_stage_outcomes(work_item_id)
        .await?;
    let context = json!({
        "schema_version":pharness_core::AGENT_CONTEXT_SCHEMA,
        "current_intent":{"title":work_item.title,"intent":work_item.intent,"acceptance_names":metadata.acceptance_command_names,"acceptance_commands":work_item.acceptance_criteria},
        "pinned_product":{"snapshot_id":metadata.product_model_snapshot_id,"snapshot_hash":metadata.product_model_snapshot_hash},
        "pinned_repository":{"repository_id":metadata.repository_id,"source_commit":authorization.source_commit,"contract_version_id":metadata.repository_contract_version_id},
        "effective_upstream_outcomes":outcomes.iter().map(|outcome| json!({"id":outcome.id,"stage":outcome.stage_key,"status":outcome.status,"hash":outcome.content_hash})).collect::<Vec<_>>(),
        "remaining_budgets":profile.budget,
        "policies":{"source_only":true,"workspace_access":if stage == "test" {"ephemeral_copy"} else {"read_only"}},
        "grants":[{"kind":"stage_chain","id":authorization.id,"expires_at":authorization.expires_at}],
        "contradictions":outcomes.iter().flat_map(|outcome| outcome.outcome.get("contradictions").and_then(Value::as_array).cloned().unwrap_or_default()).collect::<Vec<_>>(),
        "risks":outcomes.iter().flat_map(|outcome| outcome.outcome.get("risks").and_then(Value::as_array).cloned().unwrap_or_default()).collect::<Vec<_>>(),
        "operator_decisions":[{"kind":"work_plan_approval","work_plan_id":plan.id,"revision":plan.revision}],
        "evidence_catalog":outcomes.iter().map(|outcome| json!({"id":outcome.id,"kind":"stage_outcome","version":outcome.schema_version,"hash":outcome.content_hash,"stage":outcome.stage_key,"status":outcome.status})).collect::<Vec<_>>(),
    });
    let estimated_tokens = u64::try_from(context.to_string().len() / 4).unwrap_or(u64::MAX);
    if estimated_tokens > 16_000 {
        return Err(ApiError::conflict(
            "mandatory follow-up context exceeds the 16,000-token context-pack limit",
        ));
    }
    let run_id = RunId::new(new_prefixed_id("run"));
    let session_id = SessionId::new(new_prefixed_id("ses"));
    let execution_id = new_prefixed_id("stageexec");
    let context_id = new_prefixed_id("context");
    let source = pharness_runhost::WorkspaceSourceSpec {
        workspace_id: workspace.id.clone(),
        source_repo: workspace.source_repo.clone(),
        source_ref: workspace.source_ref.clone(),
        source_commit: Some(authorization.source_commit.clone()),
        branch: workspace
            .branch
            .clone()
            .ok_or_else(|| ApiError::conflict("workspace branch is unavailable"))?,
        resolved_commit: Some(authorization.source_commit.clone()),
    };
    let cwd = state.worker.effective_cwd("/workspace");
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: format!("Repo {}: {}", profile_id, work_item.title),
            cwd: cwd.clone(),
        })
        .await?;
    let scope = RunScope {
        run_id: Some(run_id.to_string()),
        repo: Some(work_item.source_repo.clone()),
        branch: workspace.branch.clone(),
        work_item_id: Some(work_item_id.into()),
        workspace_id: Some(workspace.id.clone()),
        work_plan_id: Some(plan.id.clone()),
        production_impacting: false,
        ..RunScope::default()
    };
    let run = state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: if stage == "test" {
                "Execute every selected RepositoryContract acceptance command, report exact evidence, and submit the typed Test outcome."
            } else {
                "Verify the approved plan, Builder diff, changed paths, and Test evidence; submit the typed verification decision."
            }
            .into(),
            cwd: cwd.clone(),
            max_turns: profile.budget.initial_turns,
            initial_status: "queued".into(),
            execution_target_json: json!({
                "kind":"kubernetes_workspace",
                "repo_mode":{"stage_execution_id":execution_id,"stage":stage,"context_pack_id":context_id,"chain_authorization_id":authorization.id,"workspace_access":if stage == "test" {"ephemeral_copy"} else {"read_only"}},
                "agent_profile":profile,
                "agent_context":context,
                "run_scope":scope.to_optional_json(),
                "workspace":{"base_commit":authorization.source_commit,"branch":workspace.branch},
                "workspace_source":source,
                "run_budget":profile.budget,
                "environment_profile_id":work_item.environment_profile_id,
                "repository_contract":repository_contract,
                "selected_acceptance_commands":work_item.acceptance_criteria,
                "environment_snapshot":environment_snapshot,
                "runner_profile":runner_profile,
            }),
        })
        .await?;
    let run = state
        .store
        .set_run_budget(
            &run.id,
            &profile.budget,
            &RunBudgetConsumption {
                allowed_turns: profile.budget.initial_turns,
                allowed_tokens: profile.budget.initial_tokens,
                ..RunBudgetConsumption::default()
            },
        )
        .await?;
    let run = state.store.set_run_origin(&run.id, "controller").await?;
    let run = state
        .store
        .set_run_created_by(&run.id, Some("controller:repo-mode".into()))
        .await?;
    let input = json!({
        "chain_authorization_id":authorization.id,
        "context_pack_id":context_id,
        "context_hash":canonical_material_hash(&context)?,
        "profile_id":profile.id,
        "profile_version":profile.version,
        "profile_hash":profile.profile_hash,
        "source_commit":authorization.source_commit,
        "workspace_id":workspace.id,
        "upstream_outcome_hashes":outcomes.iter().map(|outcome| &outcome.content_hash).collect::<Vec<_>>(),
    });
    let sequence = state
        .store
        .list_stage_executions(work_item_id)
        .await?
        .iter()
        .filter(|execution| execution.stage_key == stage)
        .count() as u64
        + 1;
    let execution = state
        .store
        .create_stage_execution(CreateStageExecution {
            id: execution_id,
            work_item_id: work_item_id.into(),
            stage_key: stage.into(),
            sequence,
            status: "queued".into(),
            agent_profile_id: Some(profile.id.clone()),
            agent_profile_version: Some(profile.version.clone()),
            agent_profile_hash: Some(profile.profile_hash.clone()),
            context_pack_id: None,
            run_id: Some(run.id.clone()),
            workspace_id: Some(workspace.id.clone()),
            input_hash: canonical_material_hash(&input)?,
            input_snapshot: input,
        })
        .await?;
    let pack = state
        .store
        .create_agent_context_pack(CreateAgentContextPack {
            id: context_id,
            work_item_id: work_item_id.into(),
            stage_execution_id: execution.id.clone(),
            content_hash: canonical_material_hash(&context)?,
            context,
            estimated_tokens,
        })
        .await?;
    let workspace = state
        .store
        .update_workspace_execution(
            &workspace.id,
            UpdateWorkspaceExecution {
                run_id: Some(run.id.clone()),
                status: stage.into(),
                resolved_commit: Some(authorization.source_commit.clone()),
                branch: workspace.branch.clone(),
                actor: Some("controller:repo-mode".into()),
                reason: Some(format!("automatic authorized {stage} dispatch")),
            },
        )
        .await?;
    state
        .store
        .append_event(&AgentEvent {
            event_id: EventId::new(new_prefixed_id("evt")),
            session_id,
            run_id: run.id.clone(),
            seq: 1,
            kind: EventKind::RunQueued,
            payload: json!({"source":"repo_mode_controller","stage":stage,"stage_execution_id":execution.id,"chain_authorization_id":authorization.id}),
        })
        .await?;
    state.worker.spawn_run(run.clone(), cwd);
    Ok(json!({"run":run,"stage_execution":execution,"context_pack":pack,"workspace":workspace}))
}

#[derive(Debug, Deserialize)]
struct CreateAnnotationRequest {
    target_kind: String,
    target_id: String,
    statement: String,
    #[serde(default = "empty_array")]
    evidence_refs: Value,
    requested_effect: String,
    actor: String,
    reason: String,
    state_hash: String,
}

fn empty_array() -> Value {
    Value::Array(Vec::new())
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct ContextRepositoryRequest {
    repository_id: String,
    source_commit: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepoWorkItemPreflightRequest {
    title: String,
    intent: String,
    repository_id: String,
    source_commit: String,
    acceptance_command_names: Vec<String>,
    #[serde(default)]
    context_repositories: Vec<ContextRepositoryRequest>,
    #[serde(default)]
    builder_budget: Option<pharness_core::RunBudget>,
    #[serde(default)]
    max_attempts: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRepoWorkItemRequest {
    title: String,
    intent: String,
    repository_id: String,
    source_commit: String,
    acceptance_command_names: Vec<String>,
    #[serde(default)]
    context_repositories: Vec<ContextRepositoryRequest>,
    #[serde(default)]
    builder_budget: Option<pharness_core::RunBudget>,
    #[serde(default)]
    max_attempts: Option<u32>,
    preflight_hash: String,
    actor: String,
    reason: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct RepoWorkItemPreflightResponse {
    product_id: String,
    repository_id: String,
    source_repo: String,
    source_ref: String,
    source_commit: String,
    product_model_snapshot_id: String,
    product_model_snapshot_hash: String,
    repository_contract_version_id: Option<String>,
    repository_contract_hash: Option<String>,
    environment_profile_id: Option<String>,
    selected_acceptance: Vec<Value>,
    context_repositories: Vec<Value>,
    builder_budget: pharness_core::RunBudget,
    max_attempts: u32,
    readiness_assessment_id: Option<String>,
    blockers: Vec<Value>,
    warnings: Vec<Value>,
    predicted_mutations: Vec<String>,
    preflight_hash: String,
}

async fn preflight_repo_work_item(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
    Json(request): Json<RepoWorkItemPreflightRequest>,
) -> Result<Json<RepoWorkItemPreflightResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    Ok(Json(
        build_repo_work_item_preflight(&state, &product_id, &request).await?,
    ))
}

async fn create_repo_work_item(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
    Json(request): Json<CreateRepoWorkItemRequest>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let actor = required_text(request.actor, "actor")?;
    let reason = required_text(request.reason, "reason")?;
    if actor.len() > 200 || reason.len() > 1_000 {
        return Err(ApiError::bad_request(
            "actor or reason exceeds its length limit",
        ));
    }
    let preflight_request = RepoWorkItemPreflightRequest {
        title: request.title,
        intent: request.intent,
        repository_id: request.repository_id,
        source_commit: request.source_commit,
        acceptance_command_names: request.acceptance_command_names,
        context_repositories: request.context_repositories,
        builder_budget: request.builder_budget,
        max_attempts: request.max_attempts,
    };
    let preflight = build_repo_work_item_preflight(&state, &product_id, &preflight_request).await?;
    if request.preflight_hash != preflight.preflight_hash {
        return Err(ApiError::conflict(
            "Repo WorkItem preflight is stale; refresh and retry",
        ));
    }
    if !preflight.blockers.is_empty() {
        return Err(ApiError::conflict(format!(
            "Repo WorkItem creation is blocked: {}",
            preflight
                .blockers
                .iter()
                .filter_map(|blocker| blocker.get("code").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let contract_version_id = preflight
        .repository_contract_version_id
        .clone()
        .ok_or_else(|| ApiError::conflict("current RepositoryContract version is missing"))?;
    let contract_version = state
        .store
        .get_repository_contract_version(&contract_version_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository_contract_version", &contract_version_id))?;
    let work_item_id = new_prefixed_id("witem");
    let work_item = state
        .store
        .create_repo_work_item(CreateRepoWorkItem {
            id: work_item_id.clone(),
            product_id: product_id.clone(),
            repository_id: preflight.repository_id.clone(),
            product_model_snapshot_id: preflight.product_model_snapshot_id.clone(),
            product_model_snapshot_hash: preflight.product_model_snapshot_hash.clone(),
            repository_contract_version_id: contract_version_id,
            contract_version: "pharness.dev/v1alpha1".into(),
            title: preflight_request.title.trim().into(),
            intent: preflight_request.intent.trim().into(),
            acceptance_command_names: preflight_request.acceptance_command_names,
            acceptance_commands: preflight
                .selected_acceptance
                .iter()
                .filter_map(|entry| entry.get("command").and_then(Value::as_str))
                .map(str::to_string)
                .collect(),
            context_repositories: Value::Array(preflight.context_repositories.clone()),
            source_repo: preflight.source_repo.clone(),
            source_ref: preflight.source_ref.clone(),
            source_commit: preflight.source_commit.clone(),
            environment_profile_id: preflight
                .environment_profile_id
                .clone()
                .ok_or_else(|| ApiError::conflict("EnvironmentProfile is missing"))?,
            run_budget: preflight.builder_budget.clone(),
            max_attempts: preflight.max_attempts,
            repository_contract_json: contract_version.contract.clone(),
            repository_contract_hash: preflight
                .repository_contract_hash
                .clone()
                .ok_or_else(|| ApiError::conflict("RepositoryContract hash is missing"))?,
            actor: actor.clone(),
        })
        .await?;

    let discover_execution_id = new_prefixed_id("stageexec");
    let readiness_id = preflight
        .readiness_assessment_id
        .clone()
        .ok_or_else(|| ApiError::conflict("readiness assessment is missing"))?;
    let discover_inputs = json!({
        "source_commit": preflight.source_commit,
        "product_model_snapshot_id": preflight.product_model_snapshot_id,
        "product_model_snapshot_hash": preflight.product_model_snapshot_hash,
        "repository_contract_version_id": preflight.repository_contract_version_id,
        "repository_contract_hash": preflight.repository_contract_hash,
        "readiness_assessment_id": readiness_id,
    });
    let discover_input_hash = canonical_material_hash(&discover_inputs)?;
    let execution = state
        .store
        .create_stage_execution(CreateStageExecution {
            id: discover_execution_id.clone(),
            work_item_id: work_item_id.clone(),
            stage_key: pharness_core::RepoStageKey::Discover.as_str().into(),
            sequence: 1,
            status: "succeeded".into(),
            agent_profile_id: None,
            agent_profile_version: None,
            agent_profile_hash: None,
            context_pack_id: None,
            run_id: None,
            workspace_id: None,
            input_snapshot: discover_inputs.clone(),
            input_hash: discover_input_hash,
        })
        .await?;
    let metadata = repo_metadata(&state, &work_item_id).await?;
    let outcome_document = pharness_core::StageOutcomeDocument {
        schema_version: pharness_core::STAGE_OUTCOME_SCHEMA.into(),
        work_item_id: work_item_id.clone(),
        stage_execution_id: execution.id.clone(),
        stage: pharness_core::RepoStageKey::Discover,
        status: pharness_core::StageTerminalStatus::Succeeded,
        objective: json!({"kind":"seal_current_repository_readiness"}),
        pinned_inputs: discover_inputs,
        verified_facts: vec![json!({
            "kind": "repository_readiness",
            "assessment_id": readiness_id,
            "contract_status": "ready",
            "coding_status": "ready",
        })],
        agent_claims: Vec::new(),
        outputs: vec![json!({"kind":"repository_discover_stage","status":"succeeded"})],
        acceptance: Vec::new(),
        decisions: vec![json!({"kind":"controller_seal","actor":actor,"reason":reason})],
        authorizations: Vec::new(),
        contradictions: Vec::new(),
        risks: Vec::new(),
        unavailable_capabilities: Vec::new(),
        recommendations: vec![json!({"next":"start_planner"})],
        stop_reason: "controller sealed current Repository readiness evidence".into(),
        sealed_state_version: metadata.state_version,
    };
    let outcome_value = serde_json::to_value(&outcome_document)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let outcome = state
        .store
        .seal_stage_outcome(SealStageOutcome {
            id: new_prefixed_id("stageout"),
            stage_execution_id: execution.id.clone(),
            work_item_id: work_item_id.clone(),
            stage_key: pharness_core::RepoStageKey::Discover.as_str().into(),
            status: "succeeded".into(),
            content_hash: canonical_material_hash(&outcome_value)?,
            outcome: outcome_value,
            state_version: metadata.state_version,
            supersedes_outcome_id: None,
            actor: "controller".into(),
            reason: "validated current readiness evidence".into(),
        })
        .await?;
    let metadata = repo_metadata(&state, &work_item_id).await?;
    Ok(Json(json!({
        "work_item": work_item,
        "repo_mode": metadata,
        "state_hash": repo_work_item_state_hash(&metadata)?,
        "discover_execution": execution,
        "discover_outcome": outcome,
    })))
}

async fn build_repo_work_item_preflight(
    state: &AppState,
    product_id: &str,
    request: &RepoWorkItemPreflightRequest,
) -> Result<RepoWorkItemPreflightResponse, ApiError> {
    let title = request.title.trim();
    let intent = request.intent.trim();
    if title.is_empty() || title.len() > 200 || intent.is_empty() || intent.len() > 8_000 {
        return Err(ApiError::bad_request(
            "title must be 1-200 characters and intent must be 1-8000 characters",
        ));
    }
    if !is_git_sha(&request.source_commit) {
        return Err(ApiError::bad_request(
            "source_commit must be a full 40-character Git object ID",
        ));
    }
    if request.acceptance_command_names.is_empty() {
        return Err(ApiError::bad_request(
            "at least one acceptance command name is required",
        ));
    }
    let unique_names = request
        .acceptance_command_names
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    if unique_names.len() != request.acceptance_command_names.len() {
        return Err(ApiError::bad_request(
            "acceptance command names must be unique",
        ));
    }
    if request.context_repositories.len() > 4 {
        return Err(ApiError::bad_request(
            "at most four context repositories are allowed",
        ));
    }
    let product = state
        .store
        .get_product(product_id)
        .await?
        .ok_or_else(|| ApiError::not_found("product", product_id))?;
    let product_snapshot = state
        .store
        .get_product_model_snapshot(&product.current_model_snapshot_id)
        .await?
        .ok_or_else(|| {
            ApiError::not_found("product_model_snapshot", &product.current_model_snapshot_id)
        })?;
    let repository = state
        .store
        .get_repository(&request.repository_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository", &request.repository_id))?;
    let binding = state
        .store
        .get_repository_binding(product_id, &repository.id)
        .await?;
    let source_commit = request.source_commit.to_ascii_lowercase();
    let readiness = state
        .store
        .latest_repository_readiness_assessment(&repository.id, &source_commit)
        .await?;
    let contract_version = state
        .store
        .latest_repository_contract_version(&repository.id, &source_commit)
        .await?;
    let mut blockers = Vec::<Value>::new();
    let mut warnings = Vec::<Value>::new();
    if binding.is_none() {
        blockers.push(json!({"code":"repository_not_bound_to_product","summary":"the mutable Repository is not actively bound to this Product"}));
    }
    let contract = contract_version
        .as_ref()
        .map(|version| {
            serde_json::from_value::<pharness_core::RepositoryContract>(version.contract.clone())
                .map_err(|error| {
                    ApiError::internal(format!("stored RepositoryContract is invalid: {error}"))
                })
        })
        .transpose()?;
    let mut selected_acceptance = Vec::new();
    if let Some(contract) = &contract {
        for name in &request.acceptance_command_names {
            if let Some(command) = contract.command(name) {
                selected_acceptance.push(json!({"name":command.name,"command":command.command}));
            } else {
                blockers.push(json!({"code":"acceptance_command_not_declared","summary":format!("acceptance command {name} is not declared by the active RepositoryContract"),"name":name}));
            }
        }
    } else {
        blockers.push(json!({"code":"canonical_contract_version_missing","summary":"no active canonical RepositoryContract exists for the exact source commit"}));
    }
    match &readiness {
        Some(assessment)
            if assessment.contract_status == "ready"
                && assessment.coding_status == "ready"
                && contract_version
                    .as_ref()
                    .is_some_and(|version| assessment.contract_version_id.as_deref() == Some(version.id.as_str())
                        && assessment.contract_hash.as_deref() == Some(version.content_hash.as_str())) => {}
        Some(assessment) => blockers.push(json!({
            "code":"repository_readiness_not_current",
            "summary":"the exact revision does not have a current ready contract and coding assessment",
            "assessment_id":assessment.id,
            "contract_status":assessment.contract_status,
            "coding_status":assessment.coding_status,
        })),
        None => blockers.push(json!({"code":"repository_readiness_missing","summary":"refresh readiness for this exact source commit before creating a WorkItem"})),
    }
    let budget = request.builder_budget.clone().unwrap_or_default();
    budget
        .validate()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let max_attempts = request.max_attempts.unwrap_or(2);
    if !(1..=3).contains(&max_attempts) {
        return Err(ApiError::bad_request(
            "Repo Mode max_attempts must be between one and three",
        ));
    }
    let mut context_repositories = Vec::new();
    let mut context_ids = std::collections::BTreeSet::new();
    for context in &request.context_repositories {
        if !context_ids.insert(context.repository_id.as_str())
            || context.repository_id == repository.id
            || !is_git_sha(&context.source_commit)
        {
            blockers.push(json!({"code":"invalid_context_repository","summary":"context repositories must be unique, read-only, distinct from the mutable Repository, and pinned to a full commit SHA","repository_id":context.repository_id}));
            continue;
        }
        let registered = state.store.get_repository(&context.repository_id).await?;
        let bound = state
            .store
            .get_repository_binding(product_id, &context.repository_id)
            .await?;
        let discovered = state
            .store
            .latest_successful_repository_discovery(
                &context.repository_id,
                &context.source_commit.to_ascii_lowercase(),
            )
            .await?;
        match (registered, bound, discovered) {
            (Some(registered), Some(_), Some(discovery)) => context_repositories.push(json!({
                "repository_id":registered.id,
                "canonical_url":registered.canonical_url,
                "source_commit":context.source_commit.to_ascii_lowercase(),
                "discovery_id":discovery.id,
                "discovery_hash":discovery.content_hash,
                "access":"typed_bounded_read",
            })),
            _ => blockers.push(json!({"code":"context_repository_not_ready","summary":"context repository lacks an active Product binding or deterministic discovery at the exact revision","repository_id":context.repository_id})),
        }
    }
    let writer = state.worker.git_writer_settings();
    let observer = state.worker.git_observer_settings();
    if !writer
        .as_ref()
        .is_some_and(|settings| settings.allowed_repos.contains(&repository.canonical_url))
    {
        blockers.push(json!({"code":"source_writer_unavailable","summary":"the source writer is unavailable or this repository is outside its exact allowlist"}));
    }
    if !observer
        .as_ref()
        .is_some_and(|settings| settings.allowed_repos.contains(&repository.canonical_url))
    {
        blockers.push(json!({"code":"provider_observer_unavailable","summary":"the provider observer is unavailable or this repository is outside its exact allowlist"}));
    }
    if !state
        .worker
        .source_reader_allows_repository(&repository.canonical_url)
    {
        blockers.push(json!({"code":"source_reader_unavailable","summary":"the source reader is unavailable or this repository is outside its exact allowlist"}));
    }
    warnings.extend(
        readiness
            .as_ref()
            .and_then(|assessment| assessment.warnings.as_array())
            .cloned()
            .unwrap_or_default(),
    );
    let predicted_mutations = if blockers.is_empty() {
        vec![
            "create_repo_work_item".into(),
            "seal_discover_stage_from_readiness".into(),
            "await_explicit_planner_start".into(),
        ]
    } else {
        Vec::new()
    };
    let material = json!({
        "schema_version":"pharness.dev/repo-work-item-preflight/v1alpha1",
        "product_id":product_id,
        "product_state_version":product.state_version,
        "product_model_snapshot_id":product_snapshot.id,
        "product_model_snapshot_hash":product_snapshot.content_hash,
        "repository_id":repository.id,
        "source_repo":repository.canonical_url,
        "source_ref":repository.default_branch,
        "source_commit":source_commit,
        "repository_contract_version_id":contract_version.as_ref().map(|version| &version.id),
        "repository_contract_hash":contract_version.as_ref().map(|version| &version.content_hash),
        "environment_profile_id":contract.as_ref().map(|contract| &contract.environment_profile),
        "selected_acceptance":selected_acceptance,
        "context_repositories":context_repositories,
        "builder_budget":budget,
        "max_attempts":max_attempts,
        "readiness_assessment_id":readiness.as_ref().map(|assessment| &assessment.id),
        "readiness_input_hash":readiness.as_ref().map(|assessment| &assessment.input_hash),
        "blockers":blockers,
        "warnings":warnings,
        "predicted_mutations":predicted_mutations,
    });
    let preflight_hash = canonical_material_hash(&material)?;
    Ok(RepoWorkItemPreflightResponse {
        product_id: product_id.into(),
        repository_id: repository.id,
        source_repo: repository.canonical_url,
        source_ref: repository.default_branch,
        source_commit,
        product_model_snapshot_id: product_snapshot.id,
        product_model_snapshot_hash: product_snapshot.content_hash,
        repository_contract_version_id: contract_version.as_ref().map(|version| version.id.clone()),
        repository_contract_hash: contract_version
            .as_ref()
            .map(|version| version.content_hash.clone()),
        environment_profile_id: contract.map(|contract| contract.environment_profile),
        selected_acceptance,
        context_repositories,
        builder_budget: budget,
        max_attempts,
        readiness_assessment_id: readiness.map(|assessment| assessment.id),
        blockers,
        warnings,
        predicted_mutations,
        preflight_hash,
    })
}

async fn list_stage_executions(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    repo_metadata(&state, &work_item_id).await?;
    let executions = state.store.list_stage_executions(&work_item_id).await?;
    Ok(Json(json!({
        "stage_executions": executions,
        "count": executions.len(),
    })))
}

async fn get_stage_execution(
    State(state): State<AppState>,
    Path(stage_execution_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let execution = state
        .store
        .get_stage_execution(&stage_execution_id)
        .await?
        .ok_or_else(|| ApiError::not_found("stage_execution", &stage_execution_id))?;
    Ok(Json(json!({"stage_execution": execution})))
}

async fn get_stage_outcome(
    State(state): State<AppState>,
    Path(stage_execution_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let execution = state
        .store
        .get_stage_execution(&stage_execution_id)
        .await?
        .ok_or_else(|| ApiError::not_found("stage_execution", &stage_execution_id))?;
    let outcome = state
        .store
        .get_stage_outcome_for_execution(&execution.id)
        .await?;
    Ok(Json(json!({
        "stage_execution_id": execution.id,
        "outcome": outcome,
    })))
}

async fn get_stage_context_pack(
    State(state): State<AppState>,
    Path(stage_execution_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let execution = state
        .store
        .get_stage_execution(&stage_execution_id)
        .await?
        .ok_or_else(|| ApiError::not_found("stage_execution", &stage_execution_id))?;
    let pack = match execution.context_pack_id.as_deref() {
        Some(id) => state.store.get_agent_context_pack(id).await?,
        None => None,
    };
    Ok(Json(json!({
        "stage_execution_id": execution.id,
        "context_pack": pack,
    })))
}

async fn list_annotations(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    repo_metadata(&state, &work_item_id).await?;
    let annotations = state.store.list_operator_annotations(&work_item_id).await?;
    Ok(Json(json!({
        "annotations": annotations,
        "count": annotations.len(),
    })))
}

async fn create_annotation(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
    Json(request): Json<CreateAnnotationRequest>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let statement = required_text(request.statement, "statement")?;
    let actor = required_text(request.actor, "actor")?;
    let reason = required_text(request.reason, "reason")?;
    if statement.len() > 4_000 || actor.len() > 200 || reason.len() > 1_000 {
        return Err(ApiError::bad_request(
            "annotation statement, actor, or reason exceeds its length limit",
        ));
    }
    if !matches!(
        request.target_kind.as_str(),
        "work_item" | "stage_execution" | "stage_outcome" | "evidence_validation"
    ) {
        return Err(ApiError::bad_request("unsupported annotation target_kind"));
    }
    if !matches!(
        request.requested_effect.as_str(),
        "add_context" | "mark_evidence_stale" | "repeat_stage" | "replan"
    ) {
        return Err(ApiError::bad_request(
            "requested_effect must add context, mark evidence stale, repeat a stage, or request replan",
        ));
    }
    if !request.evidence_refs.is_array() {
        return Err(ApiError::bad_request("evidence_refs must be an array"));
    }
    let metadata = repo_metadata(&state, &work_item_id).await?;
    let expected_hash = repo_work_item_state_hash(&metadata)?;
    if request.state_hash != expected_hash {
        return Err(ApiError::conflict(
            "Repo WorkItem changed after annotation preview; refresh and retry",
        ));
    }
    if request.target_kind == "stage_execution" {
        let execution = state
            .store
            .get_stage_execution(&request.target_id)
            .await?
            .ok_or_else(|| ApiError::not_found("stage_execution", &request.target_id))?;
        if execution.work_item_id != work_item_id {
            return Err(ApiError::not_found("stage_execution", &request.target_id));
        }
    }
    let annotation = state
        .store
        .create_operator_annotation(CreateOperatorAnnotation {
            id: new_prefixed_id("annot"),
            work_item_id,
            target_kind: request.target_kind,
            target_id: request.target_id,
            statement,
            evidence_refs: request.evidence_refs,
            requested_effect: request.requested_effect,
            actor,
            reason,
            state_hash: expected_hash,
        })
        .await?;
    Ok(Json(json!({"annotation": annotation})))
}

async fn repo_metadata(
    state: &AppState,
    work_item_id: &str,
) -> Result<StoredRepoWorkItemMetadata, ApiError> {
    state
        .store
        .get_repo_work_item_metadata(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repo_work_item", work_item_id))
}

pub(super) fn repo_work_item_state_hash(
    metadata: &StoredRepoWorkItemMetadata,
) -> Result<String, ApiError> {
    canonical_material_hash(&json!({
        "work_item_id": metadata.work_item_id,
        "state_version": metadata.state_version,
        "product_model_snapshot_id": metadata.product_model_snapshot_id,
        "product_model_snapshot_hash": metadata.product_model_snapshot_hash,
        "repository_contract_version_id": metadata.repository_contract_version_id,
        "current_stage_execution_id": metadata.current_stage_execution_id,
        "closed_at": metadata.closed_at,
    }))
}
