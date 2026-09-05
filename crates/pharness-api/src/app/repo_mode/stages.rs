use super::projection::{agent_evidence_bundle, annotation_context, annotation_contradictions};
use super::state::{append_repo_audit, repo_metadata};
use crate::app::approvals::create_permission_grant_record;
use crate::app::clock::current_millis;
use crate::app::hashing::canonical_material_hash;
use crate::app::hosted_workflow::stages as hosted;
use crate::app::identifiers::new_prefixed_id;
use crate::app::{ApiError, AppState};
use crate::dto::CreatePermissionGrantRequest;
use pharness_core::{
    AgentEvent, EventId, EventKind, RunBudgetConsumption, RunId, RunScope, SessionId,
};
use pharness_store::{
    CreateAgentContextPack, CreateEnvironmentPreparation, CreateEvidenceValidation, CreateRun,
    CreateSession, CreateStageExecution, CreateWorkspace, StoredRepoWorkItemMetadata,
    UpdateWorkspaceExecution,
};
use serde_json::{json, Value};

pub(in crate::app) async fn start_repo_planner(
    state: &AppState,
    work_item_id: &str,
    actor: &str,
    reason: &str,
) -> Result<Value, ApiError> {
    let metadata = repo_metadata(state, work_item_id).await?;
    hosted::validate_planned(state, &metadata, "repo-planner").await?;
    let planned_execution = crate::app::agent_hosts::latest_planned_execution_selection(
        state,
        "work_item",
        work_item_id,
        "plan",
    )
    .await?;
    if planned_execution.is_none() && !state.worker.supports_remote_workspace() {
        return Err(ApiError::unavailable(
            "Repo Mode planner execution requires kubernetes_job worker mode",
        ));
    }
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let outcomes = state
        .store
        .list_effective_stage_outcomes(work_item_id)
        .await?;
    let annotations = state.store.list_operator_annotations(work_item_id).await?;
    let evidence = agent_evidence_bundle(state, &metadata, &outcomes).await?;
    let model = state
        .worker
        .config_json()
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unconfigured")
        .to_string();
    let mut profile = if let Some(profile) = hosted::pinned_profile(&metadata, "repo-planner")? {
        profile
    } else {
        state
            .compiled_agent_profiles(&model)
            .into_iter()
            .find(|profile| profile.id == "repo-planner")
            .ok_or_else(|| ApiError::internal("compiled repo-planner profile is unavailable"))?
    };
    let stage_execution_id = new_prefixed_id("stageexec");
    let context_pack_id = new_prefixed_id("context");
    let run_id = RunId::new(new_prefixed_id("run"));
    let session_id = SessionId::new(new_prefixed_id("ses"));
    let plan_sequence = state
        .store
        .list_stage_executions(work_item_id)
        .await?
        .iter()
        .filter(|execution| execution.stage_key == pharness_core::RepoStageKey::Plan.as_str())
        .count() as u64
        + 1;
    let context = json!({
        "schema_version":pharness_core::AGENT_CONTEXT_SCHEMA,
        "current_intent":{"title":work_item.title,"intent":work_item.intent,"acceptance":metadata.acceptance_command_names},
        "pinned_product":{"snapshot_id":metadata.product_model_snapshot_id,"snapshot_hash":metadata.product_model_snapshot_hash},
        "pinned_repository":{"repository_id":metadata.repository_id,"source_commit":work_item.source_commit,"contract_version_id":metadata.repository_contract_version_id},
        "pinned_context_repositories":metadata.context_repositories,
        "upstream_outcomes":outcomes.iter().map(|outcome| json!({"id":outcome.id,"stage":outcome.stage_key,"status":outcome.status,"hash":outcome.content_hash})).collect::<Vec<_>>(),
        "remaining_budgets":profile.budget,
        "policies":hosted::context_policy(&metadata, json!({"source_only":true,"manual_merge":true,"pipeline":false,"deployment":false})),
        "grants":[],
        "contradictions":annotation_contradictions(&annotations),
        "risks":[],
        "operator_decisions":annotation_context(&annotations),
        "evidence_catalog":evidence.catalog,
    });
    let estimated_tokens = u64::try_from(context.to_string().len() / 4).unwrap_or(u64::MAX);
    if estimated_tokens > 16_000 {
        return Err(ApiError::conflict(
            "mandatory Planner context exceeds the 16,000-token context-pack limit",
        ));
    }
    let planner_workspace = if planned_execution.is_some() {
        Some(
            state
                .store
                .create_workspace(CreateWorkspace {
                    id: new_prefixed_id("ws"),
                    work_item_id: work_item_id.into(),
                    run_id: Some(run_id.clone()),
                    status: "provisioning".into(),
                    source_repo: work_item.source_repo.clone(),
                    source_ref: work_item.source_ref.clone(),
                    resolved_commit: work_item.source_commit.clone(),
                    branch: Some(format!("pharness/{work_item_id}/planner-{plan_sequence}")),
                    retention_status: "retained".into(),
                    actor: Some(actor.into()),
                    reason: Some(reason.into()),
                })
                .await?,
        )
    } else {
        None
    };
    let cwd = if planned_execution.is_some() {
        "/workspace".to_string()
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
        workspace_id: planner_workspace
            .as_ref()
            .map(|workspace| workspace.id.clone()),
        ..RunScope::default()
    };
    let (agent_execution_marker, inference_marker, resolved_profile) = if let Some(selection) =
        &planned_execution
    {
        (
            crate::app::agent_hosts::execution_marker(selection),
            json!({"mode":"not_selected","reason":"Planner uses codex_app_server"}),
            Some((
                selection.binding_hash.clone(),
                selection.resolved_binding.policy.model.clone(),
                selection.resolved_binding.policy.prompt_revision.clone(),
            )),
        )
    } else if state.inference.enabled {
        let selection = crate::app::inference::latest_planned_selection(
            state,
            "work_item",
            work_item_id,
            "plan",
        )
        .await?
        .ok_or_else(|| {
            ApiError::conflict("Planner inference selection was not pinned at WorkItem creation")
        })?;
        (
            Value::Null,
            crate::app::inference::execution_marker_for_selection(state, &selection),
            Some((
                selection.resolved_binding.agent_profile_hash.clone(),
                selection.resolved_binding.target.upstream_model.clone(),
                profile.prompt_version.clone(),
            )),
        )
    } else {
        (
            Value::Null,
            crate::app::inference::execution_marker(state, None),
            None,
        )
    };
    if let Some((profile_hash, model, prompt_version)) = resolved_profile {
        profile.profile_hash = profile_hash;
        profile.model = model;
        profile.prompt_version = prompt_version;
    }
    let runner_profile = if planned_execution.is_some() {
        Some(
            crate::app::environment::select_profile(
                &state.environment_profiles,
                work_item
                    .environment_profile_id
                    .as_deref()
                    .ok_or_else(|| ApiError::conflict("EnvironmentProfile is unavailable"))?,
                &work_item.source_repo,
            )
            .map_err(ApiError::conflict)?
            .clone(),
        )
    } else {
        None
    };
    let workspace_source =
        planner_workspace
            .as_ref()
            .map(|workspace| pharness_runhost::WorkspaceSourceSpec {
                workspace_id: workspace.id.clone(),
                source_repo: workspace.source_repo.clone(),
                source_ref: workspace.source_ref.clone(),
                source_commit: work_item.source_commit.clone(),
                branch: workspace.branch.clone().unwrap_or_default(),
                resolved_commit: None,
            });
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
            execution_target_json: hosted::bind_run(&metadata, "repo-planner", json!({
                "kind":if planned_execution.is_some() {"agent_host_workspace"} else {state.worker.execution_target_kind()},
                "agent_execution":agent_execution_marker,
                "inference":inference_marker,
                "repo_mode":{"stage_execution_id":stage_execution_id,"stage":"plan","context_pack_id":context_pack_id,"workspace_access":"read_only"},
                "agent_profile":profile,
                "agent_context":context,
                "agent_evidence_payloads":evidence.payloads,
                "run_scope":scope.to_optional_json(),
                "run_budget":profile.budget,
                "workspace_source":workspace_source,
                "environment_profile_id":work_item.environment_profile_id,
                "repository_contract":work_item.repository_contract_json,
                "selected_acceptance_commands":work_item.acceptance_criteria,
                "runner_profile":runner_profile,
            }))?,
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
            sequence: plan_sequence,
            status: "queued".into(),
            agent_profile_id: Some(profile.id.clone()),
            agent_profile_version: Some(profile.version.clone()),
            agent_profile_hash: Some(profile.profile_hash.clone()),
            context_pack_id: None,
            run_id: Some(run.id.clone()),
            workspace_id: planner_workspace
                .as_ref()
                .map(|workspace| workspace.id.clone()),
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
    let lease = if let (Some(planned), Some(workspace)) = (planned_execution, &planner_workspace) {
        Some(
            crate::app::agent_hosts::queue_bound_run(
                state,
                planned,
                &run,
                &execution.id,
                &workspace.id,
                None,
            )
            .await?,
        )
    } else {
        state.worker.spawn_run(run.clone(), cwd);
        None
    };
    Ok(
        json!({"work_item":item,"stage_execution":execution,"context_pack":pack,"run":run,"workspace":planner_workspace,"agent_lease":lease}),
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn start_repo_builder(
    state: &AppState,
    metadata: &StoredRepoWorkItemMetadata,
    work_item: &pharness_store::StoredWorkItem,
    plan: &pharness_store::StoredWorkPlan,
    workspace: &pharness_store::StoredWorkspace,
    authorization: &pharness_store::StoredStageChainAuthorization,
    contract: &pharness_core::RepositoryContract,
    actor: &str,
    reason: &str,
    reuse_prepared_workspace: bool,
    builder_profile_id: &str,
    correction_of: Option<&pharness_store::StoredStageOutcome>,
) -> Result<Value, ApiError> {
    hosted::validate_planned(state, metadata, builder_profile_id).await?;
    let planned_execution = crate::app::agent_hosts::latest_planned_execution_selection(
        state,
        "work_item",
        &work_item.id,
        builder_profile_id,
    )
    .await?;
    if planned_execution.is_none() && !state.worker.enabled() {
        return Err(ApiError::unavailable(
            "model execution worker is unavailable",
        ));
    }
    let mut profile = agent_profile_from_chain(&authorization.profile_chain, builder_profile_id)
        .ok_or_else(|| {
            ApiError::conflict(format!(
                "chain authorization has no {builder_profile_id} profile"
            ))
        })?;
    if hosted::pinned_profile(metadata, builder_profile_id)?.is_some_and(|saved| saved != profile) {
        return Err(ApiError::conflict(
            "stage-chain profile differs from the hosted authorization",
        ));
    }
    let effective_budget = if builder_profile_id == "repo-builder" {
        work_item.run_budget.clone()
    } else {
        profile.budget.clone()
    };
    let environment_profile = crate::app::environment::select_profile(
        &state.environment_profiles,
        &contract.environment_profile,
        &work_item.source_repo,
    )
    .map_err(ApiError::conflict)?
    .clone();
    contract
        .validate_for_profile(&environment_profile)
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    let reused_environment_snapshot = if reuse_prepared_workspace {
        latest_correction_environment_snapshot(state, work_item, workspace, &environment_profile)
            .await?
    } else {
        None
    };
    let reuse_prepared_environment = reused_environment_snapshot.is_some();
    if planned_execution.is_none() && !state.worker.supports_remote_workspace() {
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
        // A correction always keeps the existing detached-base checkout and
        // uncommitted Builder diff. Setting the resolved commit makes the
        // preparation worker verify that preserved checkout rather than
        // trying to clone over a nonempty PVC.
        resolved_commit: reuse_prepared_workspace.then(|| source_commit.clone()),
    };
    state
        .workspace
        .remote_source_allowed(&source)
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    let outcomes = state
        .store
        .list_effective_stage_outcomes(&work_item.id)
        .await?;
    let annotations = state.store.list_operator_annotations(&work_item.id).await?;
    let evidence = agent_evidence_bundle(state, metadata, &outcomes).await?;
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
    let mut operator_decisions =
        vec![json!({"kind":"work_plan_approval","actor":actor,"reason":reason})];
    operator_decisions.extend(annotation_context(&annotations));
    let context = json!({
        "schema_version":pharness_core::AGENT_CONTEXT_SCHEMA,
        "current_intent":{"title":work_item.title,"intent":work_item.intent,"acceptance_names":metadata.acceptance_command_names,"acceptance_commands":work_item.acceptance_criteria},
        "pinned_product":{"snapshot_id":metadata.product_model_snapshot_id,"snapshot_hash":metadata.product_model_snapshot_hash},
        "pinned_repository":{"repository_id":metadata.repository_id,"source_commit":source_commit,"contract_version_id":metadata.repository_contract_version_id,"contract_hash":work_item.repository_contract_hash},
        "pinned_context_repositories":metadata.context_repositories,
        "approved_work_plan":{"snapshot":plan_snapshot,"hash":plan_hash},
        "upstream_outcomes":outcomes.iter().map(|outcome| json!({"id":outcome.id,"stage":outcome.stage_key,"status":outcome.status,"hash":outcome.content_hash})).collect::<Vec<_>>(),
        "remaining_budgets":effective_budget,
        "correction":correction_of.map(|outcome| json!({
            "outcome_id":outcome.id,
            "stage":outcome.stage_key,
            "status":outcome.status,
            "content_hash":outcome.content_hash,
            "findings":outcome.outcome,
        })),
        "policies":hosted::context_policy(metadata, json!({"source_only":true,"manual_merge":true,"agent_network":"denied","package_installation":"preparation_only"})),
        "grants":[{"kind":"stage_chain","id":authorization.id,"expires_at":authorization.expires_at,"workspace_id":workspace.id,"writable_paths":contract.writable_paths}],
        "contradictions":annotation_contradictions(&annotations),
        "risks":[],
        "operator_decisions":operator_decisions,
        "evidence_catalog":evidence.catalog,
    });
    let estimated_tokens = u64::try_from(context.to_string().len() / 4).unwrap_or(u64::MAX);
    if estimated_tokens > 16_000 {
        return Err(ApiError::conflict(
            "mandatory Builder context exceeds the 16,000-token context-pack limit",
        ));
    }
    let cwd = if planned_execution.is_some() {
        "/workspace".to_string()
    } else {
        state.worker.effective_cwd("/workspace")
    };
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
                "actions":["write_file","patch_file","apply_patch","create_directory"],
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
    let mut policy = crate::app::policy::run_policy(&state.policy, None);
    policy.permission_grants =
        crate::app::approvals::active_permission_grants(&state.store).await?;
    let (agent_execution_marker, inference_marker, resolved_profile) =
        if let Some(selection) = &planned_execution {
            (
                crate::app::agent_hosts::execution_marker(selection),
                json!({"mode":"not_selected","reason":"stage uses codex_app_server"}),
                Some((
                    selection.binding_hash.clone(),
                    selection.resolved_binding.policy.model.clone(),
                    selection.resolved_binding.policy.prompt_revision.clone(),
                )),
            )
        } else if state.inference.enabled {
            let selection = crate::app::inference::latest_planned_selection_for_profile(
                state,
                "work_item",
                &work_item.id,
                "implement",
                builder_profile_id,
            )
            .await?
            .ok_or_else(|| {
                ApiError::conflict(format!(
                    "{builder_profile_id} inference selection is unavailable"
                ))
            })?;
            (
                Value::Null,
                crate::app::inference::execution_marker_for_selection(state, &selection),
                Some((
                    selection.resolved_binding.agent_profile_hash.clone(),
                    selection.resolved_binding.target.upstream_model.clone(),
                    profile.prompt_version.clone(),
                )),
            )
        } else {
            (
                Value::Null,
                crate::app::inference::execution_marker(state, None),
                None,
            )
        };
    if let Some((profile_hash, model, prompt_version)) = resolved_profile {
        profile.profile_hash = profile_hash;
        profile.model = model;
        profile.prompt_version = prompt_version;
    }
    let mut execution_target = json!({
        "kind":if planned_execution.is_some() {"agent_host_workspace"} else {"kubernetes_workspace"},
        "agent_execution":agent_execution_marker,
        "inference":inference_marker,
        "repo_mode":{"stage_execution_id":stage_execution_id,"stage":"implement","context_pack_id":context_pack_id,"chain_authorization_id":authorization.id},
        "agent_profile":profile,
        "agent_context":context,
        "agent_evidence_payloads":evidence.payloads,
        "policy":policy,
        "run_scope":scope.to_optional_json(),
        "workspace":{"base_commit":source_commit,"branch":branch},
        "workspace_source":source,
        "run_budget":effective_budget,
        "environment_profile_id":work_item.environment_profile_id,
        "repository_contract":work_item.repository_contract_json,
        "selected_acceptance_commands":work_item.acceptance_criteria,
        "runner_profile":environment_profile,
        "environment_preparation_required":!reuse_prepared_environment,
    });
    execution_target = hosted::bind_run(metadata, builder_profile_id, execution_target)?;
    if let Some(snapshot) = reused_environment_snapshot.clone() {
        execution_target["environment_snapshot"] = snapshot;
    }
    let run = state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: if builder_profile_id == "repo-repair" {
                format!(
                    "Repair the existing implementation using the exact sealed failure findings for this Repo Mode intent: {}",
                    work_item.intent
                )
            } else {
                format!(
                    "Implement the approved WorkPlan for this exact Repo Mode intent: {}",
                    work_item.intent
                )
            },
            cwd: cwd.clone(),
            max_turns: effective_budget.initial_turns,
            initial_status: if reuse_prepared_environment {
                "queued".into()
            } else {
                "preparing".into()
            },
            execution_target_json: execution_target,
        })
        .await?;
    let run = state
        .store
        .set_run_budget(
            &run.id,
            &effective_budget,
            &RunBudgetConsumption {
                allowed_turns: effective_budget.initial_turns,
                allowed_tokens: effective_budget.initial_tokens,
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
        "correction_of":correction_of.map(|outcome| json!({"outcome_id":outcome.id,"content_hash":outcome.content_hash,"stage":outcome.stage_key})),
        "source_commit":source_commit,
        "workspace_id":workspace.id,
    });
    let implement_sequence = state
        .store
        .list_stage_executions(&work_item.id)
        .await?
        .iter()
        .filter(|execution| execution.stage_key == pharness_core::RepoStageKey::Implement.as_str())
        .count() as u64
        + 1;
    let execution = state
        .store
        .create_stage_execution(CreateStageExecution {
            id: stage_execution_id,
            work_item_id: work_item.id.clone(),
            stage_key: pharness_core::RepoStageKey::Implement.as_str().into(),
            sequence: implement_sequence,
            status: if reuse_prepared_environment {
                "queued".into()
            } else {
                "preparing".into()
            },
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
                status: if reuse_prepared_environment {
                    "running".into()
                } else {
                    "preparing".into()
                },
                resolved_commit: Some(source_commit.clone()),
                branch: Some(branch),
                actor: Some(actor.into()),
                reason: Some(reason.into()),
            },
        )
        .await?;
    if correction_of.is_some() {
        state
            .store
            .start_work_item_internal_correction(
                &work_item.id,
                &run.id,
                Some(actor.into()),
                Some(reason.into()),
            )
            .await?;
    } else {
        state
            .store
            .start_work_item_attempt(
                &work_item.id,
                &run.id,
                Some(actor.into()),
                Some(reason.into()),
            )
            .await?;
    }
    if let Some(planned) = planned_execution {
        let pinned_host_id = if reuse_prepared_workspace {
            crate::app::agent_hosts::sticky_workspace_host(state, &workspace.id).await?
        } else {
            None
        };
        let lease = crate::app::agent_hosts::queue_bound_run(
            state,
            planned,
            &run,
            &execution.id,
            &workspace.id,
            pinned_host_id,
        )
        .await?;
        let preparation = if reuse_prepared_environment {
            None
        } else {
            Some(
                state
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
                    .await?,
            )
        };
        return Ok(json!({
            "run":run,
            "stage_execution":execution,
            "context_pack":pack,
            "workspace":workspace,
            "permission_grant":grant,
            "environment_preparation":preparation,
            "reused_environment_snapshot":reuse_prepared_environment,
            "agent_lease":lease,
        }));
    }
    if reuse_prepared_environment {
        state.worker.spawn_run(run.clone(), cwd);
        return Ok(json!({
            "run":run,
            "stage_execution":execution,
            "context_pack":pack,
            "workspace":workspace,
            "permission_grant":grant,
            "environment_preparation":null,
            "reused_environment_snapshot":true,
        }));
    }
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
        .mark_environment_preparation_dispatched(&preparation.id, &receipt.job_name)
        .await?;
    Ok(json!({
        "run":run,
        "stage_execution":execution,
        "context_pack":pack,
        "workspace":workspace,
        "permission_grant":grant,
        "environment_preparation":preparation,
        "refreshed_environment_snapshot":reuse_prepared_workspace,
    }))
}

pub(super) fn reusable_correction_environment_snapshot(
    snapshot: Value,
    source_commit: &str,
    repository_contract_hash: &str,
    environment_profile: &pharness_core::EnvironmentProfile,
) -> Result<Option<Value>, ApiError> {
    let typed: pharness_core::EnvironmentSnapshot = serde_json::from_value(snapshot.clone())
        .map_err(|error| {
            ApiError::conflict(format!(
                "correction EnvironmentSnapshot is invalid: {error}"
            ))
        })?;
    if typed.source_sha != source_commit || typed.manifest_sha256 != repository_contract_hash {
        return Err(ApiError::conflict(
            "correction EnvironmentSnapshot no longer matches the pinned source or contract",
        ));
    }
    if typed.runner_image_digest != environment_profile.image
        || typed.runner_revision != environment_profile.revision
    {
        // Runner provenance changed after the original attempt. Preserve the
        // exact source PVC, but require a new isolated preparation Job to
        // verify the checkout and seal a snapshot for the current runner.
        return Ok(None);
    }
    Ok(Some(snapshot))
}

pub(super) fn correction_environment_snapshot_for_reuse(
    snapshot: Option<Value>,
    source_commit: &str,
    repository_contract_hash: &str,
    environment_profile: &pharness_core::EnvironmentProfile,
) -> Result<Option<Value>, ApiError> {
    let Some(snapshot) = snapshot else {
        // A preparation failure can occur after the exact checkout is written
        // to the durable PVC but before an EnvironmentSnapshot is sealed. A
        // correction must preserve that workspace and run preparation again;
        // there is no prior environment provenance that is safe to reuse.
        return Ok(None);
    };
    reusable_correction_environment_snapshot(
        snapshot,
        source_commit,
        repository_contract_hash,
        environment_profile,
    )
}

async fn latest_correction_environment_snapshot(
    state: &AppState,
    work_item: &pharness_store::StoredWorkItem,
    workspace: &pharness_store::StoredWorkspace,
    environment_profile: &pharness_core::EnvironmentProfile,
) -> Result<Option<Value>, ApiError> {
    let executions = state.store.list_stage_executions(&work_item.id).await?;
    for execution in executions.iter().rev().filter(|execution| {
        execution.stage_key == pharness_core::RepoStageKey::Implement.as_str()
            && execution.workspace_id.as_deref() == Some(workspace.id.as_str())
    }) {
        let Some(run_id) = execution.run_id.as_ref() else {
            continue;
        };
        let Some(run) = state.store.get_run(run_id).await? else {
            continue;
        };
        let Some(snapshot) = run
            .execution_target_json
            .get("environment_snapshot")
            .filter(|snapshot| !snapshot.is_null())
            .cloned()
        else {
            continue;
        };
        return correction_environment_snapshot_for_reuse(
            Some(snapshot),
            work_item.source_commit.as_deref().unwrap_or_default(),
            work_item
                .repository_contract_hash
                .as_deref()
                .unwrap_or_default(),
            environment_profile,
        );
    }
    correction_environment_snapshot_for_reuse(
        None,
        work_item.source_commit.as_deref().unwrap_or_default(),
        work_item
            .repository_contract_hash
            .as_deref()
            .unwrap_or_default(),
        environment_profile,
    )
}

pub(super) fn agent_profile_from_chain(
    profile_chain: &Value,
    profile_id: &str,
) -> Option<pharness_core::AgentProfile> {
    profile_chain
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| {
            serde_json::from_value::<pharness_core::AgentProfile>(value.clone()).ok()
        })
        .find(|profile| profile.id == profile_id)
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
    if completed_run
        .execution_target_json
        .pointer("/repo_mode/test_diagnosis")
        .and_then(Value::as_bool)
        == Some(true)
    {
        if outcome.status != "succeeded" {
            return Ok(None);
        }
        let failed_outcome_id = completed_run
            .execution_target_json
            .pointer("/repo_mode/diagnosis_of_outcome_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::conflict("Test diagnosis has no failed-outcome binding"))?;
        let failed_outcome = state
            .store
            .get_stage_outcome(failed_outcome_id)
            .await?
            .ok_or_else(|| ApiError::conflict("diagnosed Test outcome is unavailable"))?;
        return start_repo_automatic_repair(state, completed_run, &failed_outcome)
            .await
            .map(Some);
    }
    if outcome.status != "succeeded" {
        if state.repo_mode.coding_reliability_v2_enabled
            && matches!(stage, "test" | "verify")
            && repairable_repo_stage_failure(state, completed_run, &outcome).await?
        {
            if stage == "test"
                && state.inference.enabled
                && crate::app::inference::latest_planned_selection_for_profile(
                    state,
                    "work_item",
                    &outcome.work_item_id,
                    "test",
                    "repo-test-diagnoser",
                )
                .await?
                .is_some()
            {
                return start_repo_followup_stage(state, completed_run, "test", Some(&outcome))
                    .await
                    .map(Some);
            }
            return start_repo_automatic_repair(state, completed_run, &outcome)
                .await
                .map(Some);
        }
        return Ok(None);
    }
    match stage {
        "implement" => start_repo_followup_stage(state, completed_run, "test", None)
            .await
            .map(Some),
        "test" => start_repo_followup_stage(state, completed_run, "verify", None)
            .await
            .map(Some),
        _ => Ok(None),
    }
}

async fn repairable_repo_stage_failure(
    state: &AppState,
    run: &pharness_store::StoredRun,
    outcome: &pharness_store::StoredStageOutcome,
) -> Result<bool, ApiError> {
    let executions = state
        .store
        .list_stage_executions(&outcome.work_item_id)
        .await?;
    let implement_count = executions
        .iter()
        .filter(|execution| execution.stage_key == "implement")
        .count();
    if implement_count != 1 {
        return Ok(false);
    }
    match outcome.stage_key.as_str() {
        "test" => {
            if run
                .execution_target_json
                .pointer("/repo_mode/deterministic_test")
                .and_then(Value::as_bool)
                != Some(true)
            {
                return Ok(false);
            }
            let results = state
                .store
                .list_events(&run.id)
                .await?
                .into_iter()
                .filter(|event| {
                    event.kind == EventKind::ToolFinished
                        && event
                            .payload
                            .pointer("/content/acceptance_command")
                            .and_then(Value::as_bool)
                            == Some(true)
                })
                .collect::<Vec<_>>();
            Ok(!results.is_empty()
                && results.iter().all(|event| {
                    event
                        .payload
                        .pointer("/content/exit_code")
                        .and_then(Value::as_i64)
                        .is_some_and(|code| !matches!(code, 126 | 127))
                })
                && results.iter().any(|event| {
                    event
                        .payload
                        .pointer("/content/exit_code")
                        .and_then(Value::as_i64)
                        != Some(0)
                }))
        }
        "verify" => Ok(run.status == "completed"
            && outcome
                .outcome
                .pointer("/verified_facts/0/typed_decision")
                .and_then(Value::as_str)
                .is_some_and(|decision| decision != "approved")),
        _ => Ok(false),
    }
}

async fn start_repo_automatic_repair(
    state: &AppState,
    completed_run: &pharness_store::StoredRun,
    failed_outcome: &pharness_store::StoredStageOutcome,
) -> Result<Value, ApiError> {
    let work_item_id = &failed_outcome.work_item_id;
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
    let workspace = state
        .store
        .get_workspace(&authorization.workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("workspace", &authorization.workspace_id))?;
    let contract: pharness_core::RepositoryContract = serde_json::from_value(
        work_item
            .repository_contract_json
            .clone()
            .ok_or_else(|| ApiError::conflict("RepositoryContract is unavailable"))?,
    )
    .map_err(|error| ApiError::internal(error.to_string()))?;
    append_repo_audit(
        state,
        work_item_id,
        "repo.stage_chain.automatic_repair_started",
        "controller:repo-mode",
        "one bounded repair execution after a repairable deterministic finding",
        json!({
            "trigger_run_id":completed_run.id,
            "failed_outcome_id":failed_outcome.id,
            "failed_stage_execution_id":failed_outcome.stage_execution_id,
            "failed_stage":failed_outcome.stage_key,
            "max_internal_corrections":1,
        }),
    )
    .await?;
    start_repo_builder(
        state,
        &metadata,
        &work_item,
        &plan,
        &workspace,
        &authorization,
        &contract,
        "controller:repo-mode",
        "automatic bounded repair from sealed stage findings",
        true,
        "repo-repair",
        Some(failed_outcome),
    )
    .await
}

pub(in crate::app) async fn record_repo_chain_continuation_failure(
    state: &AppState,
    completed_run: &pharness_store::StoredRun,
) -> Result<(), ApiError> {
    let Some(work_item_id) = completed_run
        .execution_target_json
        .pointer("/run_scope/work_item_id")
        .and_then(Value::as_str)
    else {
        return Ok(());
    };
    if let Some(chain) = state
        .store
        .active_stage_chain_authorization(work_item_id)
        .await?
    {
        state
            .store
            .revoke_stage_chain_authorization(
                &chain.id,
                "authorized stage continuation could not be dispatched",
            )
            .await?;
    }
    state
        .store
        .update_repo_work_item_status(
            work_item_id,
            "blocked",
            "controller:repo-mode",
            "authorized stage continuation failed and requires operator correction",
            false,
        )
        .await?;
    append_repo_audit(
        state,
        work_item_id,
        "repo.stage_chain.continuation_failed",
        "controller:repo-mode",
        "automatic dispatch failed after the previous Run was durably finalized",
        json!({"run_id":completed_run.id,"error_code":"stage_continuation_dispatch_failed"}),
    )
    .await
}

pub(super) async fn start_repo_followup_stage(
    state: &AppState,
    completed_run: &pharness_store::StoredRun,
    stage: &str,
    diagnosis_of: Option<&pharness_store::StoredStageOutcome>,
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
    hosted::validate_runtime(state, &metadata)?;
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
        .map_or(true, |expires_at| expires_at <= now)
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
    let test_diagnosis = diagnosis_of.is_some();
    let deterministic_test =
        state.repo_mode.coding_reliability_v2_enabled && stage == "test" && !test_diagnosis;
    let profile_id = match (stage, test_diagnosis) {
        ("test", true) => "repo-test-diagnoser",
        ("test", false) if deterministic_test => "controller-deterministic-test",
        ("test", false) => "repo-tester",
        ("verify", false) => "repo-verifier",
        _ => return Err(ApiError::internal("unsupported Repo Mode follow-up stage")),
    };
    if !deterministic_test {
        hosted::validate_planned(state, &metadata, profile_id).await?;
    }
    let planned_execution = if deterministic_test || test_diagnosis {
        None
    } else {
        crate::app::agent_hosts::latest_planned_execution_selection(
            state,
            "work_item",
            work_item_id,
            profile_id,
        )
        .await?
    };
    let sticky_host = crate::app::agent_hosts::sticky_workspace_host(state, &workspace.id).await?;
    if metadata.workflow_policy.is_some() && sticky_host.is_some() {
        return Err(ApiError::conflict(
            "hosted workspace cannot move to a native execution host",
        ));
    }
    let controller_test_on_agent_host = deterministic_test && sticky_host.is_some();
    let mut profile = if deterministic_test {
        let budget = pharness_core::RunBudget {
            initial_turns: 1,
            hard_turns: 1,
            initial_tokens: 1,
            hard_tokens: 1,
            active_execution_seconds: 900,
            recoverable_tool_errors: 0,
            identical_failures: 1,
            verification_reserve_turns: 0,
        };
        let material = json!({
            "id":profile_id,
            "version":"v2",
            "origin":"controller",
            "deterministic_test":true,
            "budget":budget,
        });
        pharness_core::AgentProfile {
            id: profile_id.into(),
            version: "v2".into(),
            profile_hash: canonical_material_hash(&material)?,
            prompt_version: "controller-deterministic-v1".into(),
            model: "none".into(),
            tools: Vec::new(),
            budget,
        }
    } else {
        agent_profile_from_chain(&authorization.profile_chain, profile_id).ok_or_else(|| {
            ApiError::conflict(format!("chain authorization has no {profile_id} profile"))
        })?
    };
    if !deterministic_test
        && hosted::pinned_profile(&metadata, profile_id)?.is_some_and(|saved| saved != profile)
    {
        return Err(ApiError::conflict(
            "stage-chain profile differs from the hosted authorization",
        ));
    }
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
    let annotations = state.store.list_operator_annotations(work_item_id).await?;
    let evidence = agent_evidence_bundle(state, &metadata, &outcomes).await?;
    let mut contradictions = outcomes
        .iter()
        .flat_map(|outcome| {
            outcome
                .outcome
                .get("contradictions")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    contradictions.extend(annotation_contradictions(&annotations));
    let mut operator_decisions = vec![json!({
        "kind":"work_plan_approval",
        "work_plan_id":plan.id,
        "revision":plan.revision,
    })];
    operator_decisions.extend(annotation_context(&annotations));
    let context = json!({
        "schema_version":pharness_core::AGENT_CONTEXT_SCHEMA,
        "current_intent":{"title":work_item.title,"intent":work_item.intent,"acceptance_names":metadata.acceptance_command_names,"acceptance_commands":work_item.acceptance_criteria},
        "pinned_product":{"snapshot_id":metadata.product_model_snapshot_id,"snapshot_hash":metadata.product_model_snapshot_hash},
        "pinned_repository":{"repository_id":metadata.repository_id,"source_commit":authorization.source_commit,"contract_version_id":metadata.repository_contract_version_id},
        "pinned_context_repositories":metadata.context_repositories,
        "effective_upstream_outcomes":outcomes.iter().map(|outcome| json!({"id":outcome.id,"stage":outcome.stage_key,"status":outcome.status,"hash":outcome.content_hash})).collect::<Vec<_>>(),
        "diagnosis_of":diagnosis_of.map(|outcome| json!({
            "outcome_id":outcome.id,
            "stage_execution_id":outcome.stage_execution_id,
            "content_hash":outcome.content_hash,
            "findings":outcome.outcome,
        })),
        "remaining_budgets":profile.budget,
        "policies":hosted::context_policy(&metadata, json!({"source_only":true,"workspace_access":if deterministic_test {"ephemeral_copy"} else {"read_only"},"deterministic_test":deterministic_test,"test_diagnosis":test_diagnosis})),
        "grants":[{"kind":"stage_chain","id":authorization.id,"expires_at":authorization.expires_at}],
        "contradictions":contradictions,
        "risks":outcomes.iter().flat_map(|outcome| outcome.outcome.get("risks").and_then(Value::as_array).cloned().unwrap_or_default()).collect::<Vec<_>>(),
        "operator_decisions":operator_decisions,
        "evidence_catalog":evidence.catalog,
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
    let cwd = if planned_execution.is_some() || controller_test_on_agent_host {
        "/workspace".to_string()
    } else {
        state.worker.effective_cwd("/workspace")
    };
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
    let (agent_execution_marker, inference_marker, resolved_profile) =
        if let Some(selection) = &planned_execution {
            (
                crate::app::agent_hosts::execution_marker(selection),
                json!({"mode":"not_selected","reason":"stage uses codex_app_server"}),
                Some((
                    selection.binding_hash.clone(),
                    selection.resolved_binding.policy.model.clone(),
                    selection.resolved_binding.policy.prompt_revision.clone(),
                )),
            )
        } else if deterministic_test {
            (
                if controller_test_on_agent_host {
                    json!({"mode":"controller_deterministic_test","host_pool":"sticky_workspace"})
                } else {
                    Value::Null
                },
                if metadata.workflow_policy.is_some() {
                    json!({"mode":"not_selected","reason":"controller-owned deterministic Test"})
                } else {
                    crate::app::inference::execution_marker(state, None)
                },
                None,
            )
        } else if state.inference.enabled {
            let selection = crate::app::inference::latest_planned_selection_for_profile(
                state,
                "work_item",
                work_item_id,
                stage,
                profile_id,
            )
            .await?
            .ok_or_else(|| {
                ApiError::conflict(format!("{profile_id} inference selection is unavailable"))
            })?;
            (
                Value::Null,
                crate::app::inference::execution_marker_for_selection(state, &selection),
                Some((
                    selection.resolved_binding.agent_profile_hash.clone(),
                    selection.resolved_binding.target.upstream_model.clone(),
                    profile.prompt_version.clone(),
                )),
            )
        } else {
            (
                Value::Null,
                crate::app::inference::execution_marker(state, None),
                None,
            )
        };
    if let Some((profile_hash, model, prompt_version)) = resolved_profile {
        profile.profile_hash = profile_hash;
        profile.model = model;
        profile.prompt_version = prompt_version;
    }
    let run = state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: if test_diagnosis {
                "Diagnose the exact controller-recorded deterministic Test failure without modifying source; submit a typed diagnosis."
            } else if deterministic_test {
                "Controller-owned deterministic Test execution."
            } else if stage == "test" {
                "Execute every selected RepositoryContract acceptance command, report exact evidence, and submit the typed Test outcome."
            } else {
                "Verify the approved plan, Builder diff, changed paths, and Test evidence; submit the typed verification decision."
            }
            .into(),
            cwd: cwd.clone(),
            max_turns: profile.budget.initial_turns,
            initial_status: "queued".into(),
            execution_target_json: hosted::bind_run(&metadata, profile_id, json!({
                "kind":if planned_execution.is_some() || controller_test_on_agent_host {"agent_host_workspace"} else {"kubernetes_workspace"},
                "agent_execution":agent_execution_marker,
                "inference":inference_marker,
                "repo_mode":{"stage_execution_id":execution_id,"stage":stage,"context_pack_id":context_id,"chain_authorization_id":authorization.id,"workspace_access":if deterministic_test {"ephemeral_copy"} else {"read_only"},"deterministic_test":deterministic_test,"test_diagnosis":test_diagnosis,"diagnosis_of_outcome_id":diagnosis_of.map(|outcome| outcome.id.as_str())},
                "agent_profile":profile,
                "agent_context":context,
                "agent_evidence_payloads":evidence.payloads,
                "run_scope":scope.to_optional_json(),
                "workspace":{"base_commit":authorization.source_commit,"branch":workspace.branch},
                "workspace_source":source,
                "run_budget":profile.budget,
                "environment_profile_id":work_item.environment_profile_id,
                "repository_contract":repository_contract,
                "selected_acceptance_commands":work_item.acceptance_criteria,
                "environment_snapshot":environment_snapshot,
                "runner_profile":runner_profile,
            }))?,
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
        "diagnosis_of":diagnosis_of.map(|outcome| json!({"outcome_id":outcome.id,"content_hash":outcome.content_hash})),
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
            agent_profile_id: (!deterministic_test).then(|| profile.id.clone()),
            agent_profile_version: (!deterministic_test).then(|| profile.version.clone()),
            agent_profile_hash: (!deterministic_test).then(|| profile.profile_hash.clone()),
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
                reason: Some(if test_diagnosis {
                    "automatic authorized test-diagnosis dispatch".into()
                } else {
                    format!("automatic authorized {stage} dispatch")
                }),
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
            payload: json!({"source":"repo_mode_controller","stage":stage,"test_diagnosis":test_diagnosis,"stage_execution_id":execution.id,"chain_authorization_id":authorization.id}),
        })
        .await?;
    let lease = if let Some(planned) = planned_execution {
        Some(
            crate::app::agent_hosts::queue_bound_run(
                state,
                planned,
                &run,
                &execution.id,
                &workspace.id,
                sticky_host,
            )
            .await?,
        )
    } else if controller_test_on_agent_host {
        Some(
            crate::app::agent_hosts::queue_controller_stage_on_sticky_host(
                state,
                &run,
                &execution.id,
                &workspace.id,
                stage,
            )
            .await?,
        )
    } else {
        state
            .worker
            .spawn_chained_run(run.clone(), cwd, completed_run.id.as_str());
        None
    };
    Ok(
        json!({"run":run,"stage_execution":execution,"context_pack":pack,"workspace":workspace,"agent_lease":lease}),
    )
}
