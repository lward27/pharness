use super::projection::{
    change_set_provenance_repair, derive_repo_actions, pending_annotation_effects,
    recoverable_repo_followup_stage_startup, recoverable_repo_stage_startup, repo_action_run_id,
    validate_change_set_outcome_binding, ChangeSetOutcomeBinding, RepoActionInputs,
};
use super::source_delivery::{
    authorize_and_dispatch_source_delivery, dispatch_source_delivery_observation,
    retry_repo_source_delivery,
};
use super::stage_authorization::authorize_repo_stage_chain;
use super::stages::{start_repo_followup_stage, start_repo_planner};
use super::state::{append_repo_audit, repo_metadata, repo_work_item_state_hash};
use crate::app::clock::current_millis;
use crate::app::hashing::canonical_material_hash;
use crate::app::identifiers::new_prefixed_id;
use crate::app::repository_readiness::ensure_repo_mode_enabled;
use crate::app::{ApiError, AppState};
use pharness_core::RunId;
use pharness_store::{
    CreateAuditEvent, CreateOperatorAnnotationDecision, CreateStageChainAuthorization, StoredRun,
    WorkspaceListFilter,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(in crate::app) struct RepoWorkItemActionExecutionRequest {
    pub actor: String,
    pub reason: String,
    pub state_hash: String,
    pub inference_policies: Option<crate::dto::StageChainInferencePolicyRequest>,
    pub execution_policies: Option<crate::dto::StageChainExecutionPolicyRequest>,
}

pub(in crate::app) async fn execute_repo_work_item_action(
    state: &AppState,
    work_item_id: &str,
    action_id: &str,
    request: RepoWorkItemActionExecutionRequest,
) -> Result<Value, ApiError> {
    let RepoWorkItemActionExecutionRequest {
        actor,
        reason,
        state_hash,
        inference_policies,
        execution_policies,
    } = request;
    ensure_repo_mode_enabled(state)?;
    let metadata = repo_metadata(state, work_item_id).await?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let work_plan = state.store.get_work_plan_by_work_item(work_item_id).await?;
    let change_set = match work_plan.as_ref() {
        Some(plan) => state.store.get_change_set_by_work_plan(&plan.id).await?,
        None => None,
    };
    let source_delivery_intent = match change_set.as_ref() {
        Some(change_set) => {
            state
                .store
                .get_source_delivery_intent_by_subject("work_item_change_set", &change_set.id)
                .await?
        }
        None => None,
    };
    let executions = state.store.list_stage_executions(work_item_id).await?;
    let chain = state
        .store
        .active_stage_chain_authorization(work_item_id)
        .await?;
    let annotations = state.store.list_operator_annotations(work_item_id).await?;
    let annotation_decisions = state
        .store
        .list_operator_annotation_decisions(work_item_id)
        .await?;
    let pending_annotation_effects =
        pending_annotation_effects(&annotations, &annotation_decisions);
    let action_run_id =
        repo_action_run_id(&metadata, &executions, work_item.current_run_id.as_ref());
    let current_run = match action_run_id {
        Some(run_id) => state.store.get_run(run_id).await?,
        None => None,
    };
    let pending_budget_extension = match current_run.as_ref() {
        Some(run) => {
            state
                .store
                .pending_budget_extension_for_run(&run.id)
                .await?
        }
        None => None,
    };
    let retryable_budget_extension = match current_run.as_ref().filter(|run| {
        run.status == "failed"
            && run
                .error
                .as_deref()
                .is_some_and(|error| error.starts_with("failed to launch worker job:"))
            && run
                .result_json
                .as_ref()
                .and_then(|result| result.get("budget_extension"))
                .is_some()
    }) {
        Some(run) => {
            state
                .store
                .latest_approved_budget_extension_for_run(&run.id)
                .await?
        }
        None => None,
    };
    let action = derive_repo_actions(
        &metadata,
        RepoActionInputs {
            attempts: (work_item.attempt_count, work_item.max_attempts),
            work_plan: work_plan.as_ref(),
            change_set: change_set.as_ref(),
            source_delivery_intent: source_delivery_intent.as_ref(),
            executions: &executions,
            chain: chain.as_ref(),
            pending_annotation_effects: &pending_annotation_effects,
            pending_budget_extension: pending_budget_extension.as_ref(),
            current_run: current_run.as_ref(),
            retryable_budget_extension: retryable_budget_extension.as_ref(),
        },
    )?
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
        "recover_stage_startup" => {
            let (run, execution) = recoverable_repo_stage_startup(
                &metadata,
                current_run.as_ref(),
                &executions,
                chain.as_ref(),
            )
            .ok_or_else(|| {
                ApiError::conflict("Repo Mode stage startup is no longer recoverable")
            })?;
            if state
                .store
                .get_environment_preparation_by_run(&run.id)
                .await?
                .is_some()
            {
                return Err(ApiError::conflict(
                    "Repo Mode stage startup already has durable preparation state",
                ));
            }
            let run_id = run.id.clone();
            let execution_id = execution.id.clone();
            state
                .store
                .refund_unstarted_work_item_attempt(
                    work_item_id,
                    &run_id,
                    Some(actor.clone()),
                    Some(reason.clone()),
                )
                .await?;
            crate::worker::fail_run_from_dispatch(
                &state.store,
                &run_id,
                "controller sealed a zero-turn Builder startup failure before preparation was created"
                    .into(),
            )
            .await?;
            append_repo_audit(
                state,
                work_item_id,
                "repo.stage_startup.recovered",
                &actor,
                &reason,
                json!({
                    "run_id":run_id,
                    "stage_execution_id":execution_id,
                    "attempt_budget_restored":true,
                    "model_turns_consumed":0,
                    "workspace_preserved":true,
                }),
            )
            .await?;
            let run = state
                .store
                .get_run(&run_id)
                .await?
                .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
            let execution = state
                .store
                .get_stage_execution(&execution_id)
                .await?
                .ok_or_else(|| ApiError::not_found("stage_execution", &execution_id))?;
            let work_item = state
                .store
                .get_work_item(work_item_id)
                .await?
                .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
            Ok(json!({
                "work_item":work_item,
                "run":run,
                "stage_execution":execution,
                "attempt_budget_restored":true,
            }))
        }
        "retry_stage_startup" => {
            let (run, execution) = recoverable_repo_followup_stage_startup(
                &metadata,
                current_run.as_ref(),
                &executions,
                chain.as_ref(),
            )
            .ok_or_else(|| {
                ApiError::conflict("Repo Mode follow-up stage startup is no longer recoverable")
            })?;
            retry_repo_followup_stage_startup(state, work_item_id, run, execution, &actor, &reason)
                .await
        }
        "apply_annotation_effect" => {
            let annotation = pending_annotation_effects
                .first()
                .copied()
                .ok_or_else(|| ApiError::conflict("annotation effect is no longer pending"))?;
            let repeats_stage = annotation.requested_effect == "repeat_stage";
            let result = if repeats_stage {
                apply_repo_stage_correction(state, work_item_id, &actor, &reason).await?
            } else {
                apply_repo_replan(state, work_item_id, &actor, &reason).await?
            };
            let decision = state
                .store
                .create_operator_annotation_decision(CreateOperatorAnnotationDecision {
                    id: new_prefixed_id("annotdec"),
                    annotation_id: annotation.id.clone(),
                    work_item_id: work_item_id.into(),
                    decision: if repeats_stage {
                        "stage_repeat_started".into()
                    } else {
                        "replan_started".into()
                    },
                    action_id: action_id.into(),
                    actor,
                    reason,
                    state_hash,
                })
                .await?;
            Ok(json!({"annotation_decision":decision,"result":result}))
        }
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
            authorize_repo_stage_chain(
                state,
                work_item_id,
                &actor,
                &reason,
                None,
                inference_policies.as_ref(),
                execution_policies.as_ref(),
            )
            .await
        }
        "approve_budget_extension" => {
            let extension = pending_budget_extension.ok_or_else(|| {
                ApiError::conflict("Repo Mode budget extension is no longer pending")
            })?;
            let (extension, run) = state
                .store
                .approve_budget_extension(&extension.id, &state_hash, &actor, &reason)
                .await?;
            state.worker.spawn_run(run.clone(), run.cwd.clone());
            Ok(json!({
                "budget_extension": crate::dto::BudgetExtensionResponse::from(extension),
            }))
        }
        "retry_budget_extension_dispatch" => {
            let extension = retryable_budget_extension.ok_or_else(|| {
                ApiError::conflict("Repo Mode budget-extension dispatch is no longer retryable")
            })?;
            let (extension, run) = state
                .store
                .retry_approved_budget_extension_dispatch(&extension.id, &extension.state_hash)
                .await?;
            state
                .store
                .create_audit_event(CreateAuditEvent {
                    id: new_prefixed_id("audit"),
                    kind: "repo_mode.budget_extension_dispatch_retried".into(),
                    actor: Some(actor.clone()),
                    resource_kind: "work_item".into(),
                    resource_id: work_item_id.into(),
                    run_id: Some(run.id.clone()),
                    payload_json: json!({
                        "reason":reason,
                        "budget_extension_id":extension.id,
                        "run_id":run.id,
                        "additional_budget_granted":false,
                    }),
                })
                .await?;
            state.worker.spawn_run(run.clone(), run.cwd.clone());
            Ok(json!({
                "budget_extension":crate::dto::BudgetExtensionResponse::from(extension),
                "run":crate::dto::RunResponse::from(run),
            }))
        }
        "correct_stage_chain" => {
            apply_repo_stage_correction(state, work_item_id, &actor, &reason).await
        }
        "replan_work_item" => apply_repo_replan(state, work_item_id, &actor, &reason).await,
        "approve_change_set" | "reject_change_set" => {
            let change_set =
                change_set.ok_or_else(|| ApiError::conflict("ChangeSet is unavailable"))?;
            if change_set.status != "proposed" {
                return Err(ApiError::conflict("ChangeSet is no longer proposed"));
            }
            let target = if action_id == "approve_change_set" {
                "approved"
            } else {
                "rejected"
            };
            let change_set = state
                .store
                .update_change_set_status(
                    &change_set.id,
                    target,
                    Some(actor.clone()),
                    Some(reason.clone()),
                )
                .await?;
            let item = state
                .store
                .update_repo_work_item_status(
                    work_item_id,
                    if target == "approved" {
                        "awaiting_approval"
                    } else {
                        "blocked"
                    },
                    &actor,
                    &format!("ChangeSet {target} by {actor}: {reason}"),
                    false,
                )
                .await?;
            Ok(json!({"change_set":change_set,"work_item":item}))
        }
        "repair_change_set_provenance" => {
            repair_approved_change_set_provenance(state, work_item_id, &actor, &reason).await
        }
        "authorize_source_delivery" => {
            authorize_and_dispatch_source_delivery(state, work_item_id, &actor, &reason).await
        }
        "retry_source_delivery" => {
            retry_repo_source_delivery(state, work_item_id, &actor, &reason).await
        }
        "observe_source_delivery" => {
            dispatch_source_delivery_observation(state, work_item_id, &actor, &reason).await
        }
        _ => Err(ApiError::conflict("unsupported Repo Mode action")),
    }
}

async fn repair_approved_change_set_provenance(
    state: &AppState,
    work_item_id: &str,
    actor: &str,
    reason: &str,
) -> Result<Value, ApiError> {
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
        .ok_or_else(|| ApiError::conflict("approved WorkPlan is required"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&plan.id)
        .await?
        .filter(|change_set| change_set.status == "approved")
        .ok_or_else(|| ApiError::conflict("approved ChangeSet is required"))?;
    if state
        .store
        .get_source_delivery_intent_by_subject("work_item_change_set", &change_set.id)
        .await?
        .is_some()
    {
        return Err(ApiError::conflict(
            "ChangeSet provenance cannot be repaired after source delivery begins",
        ));
    }
    let repair = change_set_provenance_repair(&change_set).ok_or_else(|| {
        ApiError::conflict("ChangeSet provenance is already current or cannot be repaired")
    })?;
    let material_builder_run_id = RunId::new(repair.material_builder_run_id.to_string());
    let verification_run_id = RunId::new(repair.verification_run_id.to_string());
    let builder_run = state
        .store
        .get_run(&material_builder_run_id)
        .await?
        .ok_or_else(|| {
            ApiError::conflict("ChangeSet material Builder Run provenance is unavailable")
        })?;
    let verification_run = state
        .store
        .get_run(&verification_run_id)
        .await?
        .ok_or_else(|| {
            ApiError::conflict("ChangeSet material Verifier Run provenance is unavailable")
        })?;

    if canonical_material_hash(&change_set.change_set_json)? != change_set.material_hash {
        return Err(ApiError::conflict(
            "ChangeSet material does not match its immutable hash",
        ));
    }
    if change_set.work_item_id.as_deref() != Some(work_item_id)
        || change_set
            .change_set_json
            .get("work_item_id")
            .and_then(Value::as_str)
            != Some(work_item_id)
        || change_set
            .change_set_json
            .pointer("/work_plan/id")
            .and_then(Value::as_str)
            != Some(plan.id.as_str())
        || change_set
            .change_set_json
            .pointer("/work_plan/revision")
            .and_then(Value::as_i64)
            != Some(plan.revision)
    {
        return Err(ApiError::conflict(
            "ChangeSet WorkItem or WorkPlan provenance is stale",
        ));
    }
    let source_commit = work_item
        .source_commit
        .as_deref()
        .ok_or_else(|| ApiError::conflict("immutable WorkItem source commit is unavailable"))?;
    if change_set
        .change_set_json
        .pointer("/source_provenance/source_commit")
        .and_then(Value::as_str)
        != Some(source_commit)
    {
        return Err(ApiError::conflict(
            "ChangeSet source provenance does not match the WorkItem",
        ));
    }

    let implement_stage_execution_id = change_set
        .change_set_json
        .pointer("/source_provenance/stage_execution_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("ChangeSet Implement execution is unavailable"))?;
    let implement_outcome_id = change_set
        .change_set_json
        .pointer("/source_provenance/implement_outcome_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("ChangeSet Implement outcome is unavailable"))?;
    let implement_outcome_hash = change_set
        .change_set_json
        .pointer("/source_provenance/implement_outcome_hash")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("ChangeSet Implement outcome hash is unavailable"))?;
    let verification_stage_execution_id = change_set
        .change_set_json
        .get("verification_stage_execution_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("ChangeSet Verify execution is unavailable"))?;
    let effective_outcomes = state
        .store
        .list_effective_stage_outcomes(work_item_id)
        .await?;
    if !effective_outcomes.iter().any(|outcome| {
        outcome.stage_key == "implement"
            && outcome.status == "succeeded"
            && outcome.id == implement_outcome_id
            && outcome.content_hash == implement_outcome_hash
            && outcome.stage_execution_id == implement_stage_execution_id
    }) {
        return Err(ApiError::conflict(
            "ChangeSet does not match the effective Implement outcome",
        ));
    }
    if !effective_outcomes.iter().any(|outcome| {
        outcome.stage_key == "verify"
            && outcome.status == "succeeded"
            && outcome.stage_execution_id == verification_stage_execution_id
    }) {
        return Err(ApiError::conflict(
            "ChangeSet does not match the effective Verify outcome",
        ));
    }
    let material_effective_outcomes = change_set
        .change_set_json
        .get("effective_outcomes")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::conflict("ChangeSet effective outcomes are unavailable"))?;
    let outcome_binding =
        validate_change_set_outcome_binding(material_effective_outcomes, &effective_outcomes)?;
    let historical_verifier_outcome_list_stale = match outcome_binding {
        ChangeSetOutcomeBinding::Current => false,
        ChangeSetOutcomeBinding::HistoricalVerifier { id, hash } => {
            state
                .store
                .get_stage_outcome(&id)
                .await?
                .filter(|outcome| {
                    outcome.work_item_id == work_item_id
                        && outcome.stage_key == "verify"
                        && outcome.status == "succeeded"
                        && outcome.content_hash == hash
                        && outcome.stage_execution_id != verification_stage_execution_id
                })
                .ok_or_else(|| {
                    ApiError::conflict(
                        "ChangeSet historical Verify outcome provenance is not immutable evidence",
                    )
                })?;
            true
        }
    };
    let implement_execution = state
        .store
        .get_stage_execution(implement_stage_execution_id)
        .await?
        .filter(|execution| {
            execution.work_item_id == work_item_id
                && execution.stage_key == "implement"
                && execution.status == "succeeded"
                && execution.run_id.as_ref() == Some(&material_builder_run_id)
        })
        .ok_or_else(|| {
            ApiError::conflict("ChangeSet Builder StageExecution provenance is unavailable")
        })?;
    let verification_execution = state
        .store
        .get_stage_execution(verification_stage_execution_id)
        .await?
        .filter(|execution| {
            execution.work_item_id == work_item_id
                && execution.stage_key == "verify"
                && execution.status == "succeeded"
                && execution.run_id.as_ref() == Some(&verification_run_id)
        })
        .ok_or_else(|| {
            ApiError::conflict("ChangeSet Verifier StageExecution provenance is unavailable")
        })?;
    if builder_run.id != material_builder_run_id
        || verification_run.id != verification_run_id
        || implement_execution.workspace_id.is_none()
        || verification_execution.workspace_id != implement_execution.workspace_id
    {
        return Err(ApiError::conflict(
            "ChangeSet stage run or workspace provenance is inconsistent",
        ));
    }

    let patch_artifact_id = change_set
        .change_set_json
        .pointer("/patch/artifact_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("ChangeSet patch artifact provenance is unavailable"))?;
    let patch_hash = change_set
        .change_set_json
        .pointer("/patch/hash")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("ChangeSet patch hash provenance is unavailable"))?;
    let patch = state
        .store
        .list_artifacts(&material_builder_run_id)
        .await?
        .into_iter()
        .find(|artifact| artifact.id == patch_artifact_id && artifact.kind == "workspace_git_diff")
        .ok_or_else(|| ApiError::conflict("ChangeSet patch artifact is unavailable"))?;
    let diff = patch
        .content_text
        .as_deref()
        .filter(|diff| !diff.is_empty())
        .ok_or_else(|| ApiError::conflict("ChangeSet patch artifact is empty"))?;
    if format!("sha256:{:x}", Sha256::digest(diff.as_bytes())) != patch_hash {
        return Err(ApiError::conflict(
            "ChangeSet patch artifact does not match its immutable hash",
        ));
    }

    let prior_session_id = change_set.session_id.clone();
    let prior_run_id = change_set.run_id.clone();
    let repaired = state
        .store
        .rebind_approved_change_set_provenance(
            &change_set.id,
            change_set.revision,
            &change_set.material_hash,
            &verification_run.session_id,
            &material_builder_run_id,
        )
        .await?;
    append_repo_audit(
        state,
        work_item_id,
        "repo.change_set.provenance_repaired",
        actor,
        reason,
        json!({
            "change_set_id":repaired.id,
            "revision":repaired.revision,
            "material_hash":repaired.material_hash,
            "prior_session_id":prior_session_id,
            "prior_run_id":prior_run_id,
            "session_id":repaired.session_id,
            "run_id":repaired.run_id,
            "material_unchanged":true,
            "revision_unchanged":true,
            "approval_unchanged":true,
            "external_effect":false,
            "historical_verifier_outcome_list_stale":historical_verifier_outcome_list_stale,
        }),
    )
    .await?;
    Ok(json!({
        "change_set":repaired,
        "material_unchanged":true,
        "revision_unchanged":true,
        "external_effect":false,
        "historical_verifier_outcome_list_stale":historical_verifier_outcome_list_stale,
    }))
}

async fn apply_repo_stage_correction(
    state: &AppState,
    work_item_id: &str,
    actor: &str,
    reason: &str,
) -> Result<Value, ApiError> {
    if let Some(chain) = state
        .store
        .active_stage_chain_authorization(work_item_id)
        .await?
    {
        state
            .store
            .revoke_stage_chain_authorization(
                &chain.id,
                "operator authorized a fresh correction chain",
            )
            .await?;
    }
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let workspace = state
        .store
        .list_workspaces(WorkspaceListFilter {
            work_item_id: Some(work_item_id.into()),
            limit: 100,
            ..WorkspaceListFilter::default()
        })
        .await?
        .into_iter()
        .find(|workspace| workspace.resolved_commit == work_item.source_commit)
        .ok_or_else(|| ApiError::conflict("preserved correction workspace is unavailable"))?;
    authorize_repo_stage_chain(
        state,
        work_item_id,
        actor,
        reason,
        Some(workspace),
        None,
        None,
    )
    .await
}

async fn retry_repo_followup_stage_startup(
    state: &AppState,
    work_item_id: &str,
    failed_run: &StoredRun,
    failed_execution: &pharness_store::StoredStageExecution,
    actor: &str,
    reason: &str,
) -> Result<Value, ApiError> {
    let upstream_stage = match failed_execution.stage_key.as_str() {
        "test" => "implement",
        "verify" => "test",
        _ => {
            return Err(ApiError::conflict(
                "only zero-turn Tester or Verifier startup is recoverable",
            ))
        }
    };
    let prior_chain_id = failed_run
        .execution_target_json
        .pointer("/repo_mode/chain_authorization_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("failed stage has no chain authorization"))?;
    let prior_chain = state
        .store
        .get_stage_chain_authorization(prior_chain_id)
        .await?
        .filter(|authorization| {
            authorization.work_item_id == work_item_id
                && authorization.id
                    == failed_execution
                        .input_snapshot
                        .get("chain_authorization_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
        })
        .ok_or_else(|| {
            ApiError::conflict("failed stage chain authorization provenance is unavailable")
        })?;
    if prior_chain.status == "active" {
        return Err(ApiError::conflict(
            "failed stage chain authorization is still active",
        ));
    }
    let metadata = repo_metadata(state, work_item_id).await?;
    if metadata.current_stage_execution_id.as_deref() != Some(failed_execution.id.as_str()) {
        return Err(ApiError::conflict(
            "failed stage is no longer the current WorkItem boundary",
        ));
    }
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let plan = state
        .store
        .get_work_plan_by_work_item(work_item_id)
        .await?
        .filter(|plan| {
            plan.status == "approved"
                && plan.id == prior_chain.work_plan_id
                && plan.revision == prior_chain.work_plan_revision
        })
        .ok_or_else(|| ApiError::conflict("approved WorkPlan is no longer current"))?;
    if prior_chain.product_model_snapshot_id != metadata.product_model_snapshot_id
        || prior_chain.product_model_snapshot_hash != metadata.product_model_snapshot_hash
        || prior_chain.repository_id != metadata.repository_id
        || work_item.source_commit.as_deref() != Some(prior_chain.source_commit.as_str())
        || failed_execution.workspace_id.as_deref() != Some(prior_chain.workspace_id.as_str())
    {
        return Err(ApiError::conflict(
            "failed stage authorization no longer matches the pinned WorkItem state",
        ));
    }
    let upstream_outcome = state
        .store
        .list_effective_stage_outcomes(work_item_id)
        .await?
        .into_iter()
        .find(|outcome| outcome.stage_key == upstream_stage && outcome.status == "succeeded")
        .ok_or_else(|| {
            ApiError::conflict(format!(
                "a sealed successful {upstream_stage} outcome is required"
            ))
        })?;
    let upstream_execution = state
        .store
        .get_stage_execution(&upstream_outcome.stage_execution_id)
        .await?
        .filter(|execution| {
            execution.work_item_id == work_item_id
                && execution.stage_key == upstream_stage
                && execution.status == "succeeded"
                && execution.workspace_id.as_deref() == Some(prior_chain.workspace_id.as_str())
        })
        .ok_or_else(|| ApiError::conflict("sealed upstream StageExecution is unavailable"))?;
    let upstream_run_id = upstream_execution
        .run_id
        .as_ref()
        .ok_or_else(|| ApiError::conflict("sealed upstream AgentRun is unavailable"))?;
    let upstream_run = state
        .store
        .get_run(upstream_run_id)
        .await?
        .filter(|run| run.status == "completed")
        .ok_or_else(|| ApiError::conflict("sealed upstream AgentRun is not completed"))?;
    let authorization = state
        .store
        .create_stage_chain_authorization(CreateStageChainAuthorization {
            id: new_prefixed_id("chain"),
            work_item_id: work_item_id.into(),
            work_plan_id: plan.id,
            work_plan_revision: plan.revision,
            product_model_snapshot_id: prior_chain.product_model_snapshot_id,
            product_model_snapshot_hash: prior_chain.product_model_snapshot_hash,
            repository_id: prior_chain.repository_id,
            source_commit: prior_chain.source_commit,
            workspace_id: prior_chain.workspace_id,
            writable_paths: prior_chain.writable_paths,
            profile_chain: prior_chain.profile_chain,
            budget_chain: prior_chain.budget_chain,
            state_hash: repo_work_item_state_hash(&metadata)?,
            created_by: actor.into(),
            creation_reason: reason.into(),
            expires_at: (current_millis() + 4 * 60 * 60 * 1_000).to_string(),
        })
        .await?;
    let started = match start_repo_followup_stage(
        state,
        &upstream_run,
        failed_execution.stage_key.as_str(),
        None,
    )
    .await
    {
        Ok(started) => started,
        Err(error) => {
            state
                .store
                .revoke_stage_chain_authorization(
                    &authorization.id,
                    "explicit follow-up stage startup retry could not be dispatched",
                )
                .await?;
            return Err(error);
        }
    };
    let work_item = state
        .store
        .update_repo_work_item_status(
            work_item_id,
            "executing",
            actor,
            "operator retried a zero-turn follow-up stage startup failure",
            false,
        )
        .await?;
    append_repo_audit(
        state,
        work_item_id,
        "repo.followup_stage_startup.retried",
        actor,
        reason,
        json!({
            "failed_run_id":failed_run.id,
            "failed_stage_execution_id":failed_execution.id,
            "stage":failed_execution.stage_key,
            "upstream_stage_execution_id":upstream_execution.id,
            "upstream_outcome_id":upstream_outcome.id,
            "replacement_chain_authorization_id":authorization.id,
            "attempt_budget_consumed":false,
            "model_turns_previously_consumed":0,
            "workspace_preserved":true,
        }),
    )
    .await?;
    Ok(json!({
        "work_item":work_item,
        "stage_chain_authorization":authorization,
        "retry":started,
        "attempt_budget_consumed":false,
    }))
}

async fn apply_repo_replan(
    state: &AppState,
    work_item_id: &str,
    actor: &str,
    reason: &str,
) -> Result<Value, ApiError> {
    if let Some(chain) = state
        .store
        .active_stage_chain_authorization(work_item_id)
        .await?
    {
        state
            .store
            .revoke_stage_chain_authorization(&chain.id, "operator requested full Repo Mode replan")
            .await?;
    }
    if let Some(plan) = state.store.get_work_plan_by_work_item(work_item_id).await? {
        state
            .store
            .update_work_plan_status(
                &plan.id,
                "superseded",
                Some(actor.into()),
                Some(reason.into()),
            )
            .await?;
    }
    start_repo_planner(state, work_item_id, actor, reason).await
}
