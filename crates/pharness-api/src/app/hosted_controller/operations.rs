use super::state::{condition, continuation_candidate, now, Condition, Snapshot};
use crate::app::hashing::canonical_material_hash;
use crate::app::hosted_workflow::stages as hosted;
use crate::app::repo_mode::{self, RepoWorkItemActionExecutionRequest};
use crate::app::{ApiError, AppState};
use pharness_store::{
    BeginWorkflowOperation, StoredWorkflowOperation, StoredWorkflowReconciliation,
};
use serde_json::{json, Value};

pub(super) async fn execute_operation(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    snapshot: &Snapshot,
    operation: StoredWorkflowOperation,
    refs: Value,
) -> Result<Condition, ApiError> {
    if operation.action == super::build::ACTION {
        return super::build::reconcile(state, claim, snapshot, &operation, false).await;
    }
    if matches!(
        operation.action.as_str(),
        "approve_work_plan" | "approve_change_set"
    ) {
        super::approval::validate(
            state,
            &claim.work_item_id,
            &operation.action,
            refs["action_resource"].as_str().unwrap_or_default(),
        )
        .await?;
    }
    // Persist the pre-dispatch resource set. A replacement owner can distinguish
    // a never-attempted operation from a lost acknowledgement or partial startup.
    let operation = state
        .store
        .record_workflow_operation(
            claim,
            &operation.id,
            "running",
            &refs,
            "Execution boundary recorded before calling the existing stage executor",
            now(),
        )
        .await?;
    let result = if operation.action == "continue_stage" {
        let predecessor = snapshot
            .runs
            .iter()
            .find(|r| refs["action_resource"] == r.id.as_str())
            .ok_or_else(|| ApiError::conflict("the recorded predecessor Run is unavailable"))?;
        repo_mode::continue_repo_stage_chain(state, predecessor)
            .await
            .map(|r| r.unwrap_or(Value::Null))
    } else {
        repo_mode::execute_repo_work_item_action(
            state,
            &claim.work_item_id,
            &operation.action,
            RepoWorkItemActionExecutionRequest {
                actor: "controller:hosted-workflow".into(),
                reason: format!(
                    "Automatic {} under saved workflow {}",
                    operation.action,
                    snapshot
                        .metadata
                        .workflow_policy_hash
                        .as_deref()
                        .unwrap_or_default()
                ),
                state_hash: operation.input_hash.clone(),
                inference_policies: None,
                execution_policies: None,
            },
        )
        .await
    };
    if let Err(error) = result {
        state
            .store
            .record_workflow_operation(
                claim,
                &operation.id,
                "blocked",
                &refs,
                &error.message,
                now(),
            )
            .await?;
        return Ok(condition("blocked", error.message));
    }
    let updated = Snapshot::load(state, &claim.work_item_id).await?;
    if operation.action == "continue_stage" && result.as_ref().is_ok_and(Value::is_null) {
        state
            .store
            .record_workflow_operation(
                claim,
                &operation.id,
                "succeeded",
                &refs,
                "Continuation examined sealed evidence and dispatched no further Run",
                now(),
            )
            .await?;
        return Ok(condition(
            "waiting",
            "The stage executor found no authorized continuation.",
        ));
    }
    reconcile_operation(state, claim, &updated, operation, false, false).await
}

pub(super) async fn reconcile_operation(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    snapshot: &Snapshot,
    operation: StoredWorkflowOperation,
    expired: bool,
    redispatch: bool,
) -> Result<Condition, ApiError> {
    if operation.action == super::build::ACTION {
        return super::build::reconcile(state, claim, snapshot, &operation, expired).await;
    }
    if operation.status == "pending" {
        if claim.control != "active" || expired {
            state
                .store
                .release_pending_workflow_locks(claim, &operation.id, now())
                .await?;
            return Ok(condition(
                &claim.control,
                "The recorded operation has not been dispatched.",
            ));
        }
        // A pending record was created before any executor call. Recover the
        // same identity only if its original action is still eligible.
        let action = snapshot.actions.iter().find(|a| {
            a.id == operation.action && a.state_hash == operation.input_hash && a.status == "ready"
        });
        let resource = if let Some(action) = action {
            action.resource.clone()
        } else if operation.action == "continue_stage" {
            continuation_candidate(snapshot)
                .filter(|run| {
                    canonical_material_hash(&json!({"run":run.id,"result":run.result_json}))
                        .ok()
                        .as_deref()
                        == Some(&operation.input_hash)
                })
                .map(|run| run.id.to_string())
                .ok_or_else(|| ApiError::conflict("the pending continuation is stale"))?
        } else {
            return Err(ApiError::conflict("the pending action is stale"));
        };
        let refs = json!({"action_resource":resource,"before_run_ids":snapshot.runs.iter().map(|r| r.id.as_str()).collect::<Vec<_>>()});
        let keys = operation
            .resource_keys
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        state
            .store
            .begin_workflow_operation(
                claim,
                BeginWorkflowOperation {
                    id: &operation.id,
                    action: &operation.action,
                    input_hash: &operation.input_hash,
                    effect: &operation.effect,
                    resource_keys: &keys,
                },
                now(),
            )
            .await?;
        // Box only this restart branch to keep the async state machine finite.
        return Box::pin(execute_operation(state, claim, snapshot, operation, refs)).await;
    }
    let mut refs = operation.resource_refs.clone();
    if operation.action == "authorize_source_delivery" {
        return super::source::reconcile(state, claim, snapshot, &operation, expired).await;
    }
    if matches!(
        operation.action.as_str(),
        "approve_work_plan" | "approve_change_set"
    ) {
        let plan = state
            .store
            .get_work_plan_by_work_item(&claim.work_item_id)
            .await?;
        let approved = if operation.action == "approve_work_plan" {
            plan.is_some_and(|p| refs["action_resource"] == p.id && p.status == "approved")
        } else if let Some(plan) = plan {
            state
                .store
                .get_change_set_by_work_plan(&plan.id)
                .await?
                .is_some_and(|c| refs["action_resource"] == c.id && c.status == "approved")
        } else {
            false
        };
        if approved {
            state
                .store
                .record_workflow_operation(
                    claim,
                    &operation.id,
                    "succeeded",
                    &refs,
                    "Existing approval matches the recorded workflow operation",
                    now(),
                )
                .await?;
            return Ok(condition(
                "progressing",
                "The saved authorization was applied to the recorded evidence.",
            ));
        }
        return Ok(condition(
            "blocked",
            "The approval operation has incomplete state; no duplicate decision was applied.",
        ));
    }
    let candidates: Vec<_> = snapshot
        .runs
        .iter()
        .filter(|r| {
            refs.get("run_id").map_or_else(
                || {
                    refs["before_run_ids"]
                        .as_array()
                        .is_some_and(|ids| !ids.contains(&json!(r.id)))
                },
                |id| id == r.id.as_str(),
            )
        })
        .collect();
    if candidates.len() != 1 {
        return Ok(condition("blocked", "The dispatched operation has no unique durable Run. Its resource locks are retained for recovery."));
    }
    let run = candidates[0];
    let execution = snapshot
        .stages
        .iter()
        .find(|s| s.run_id.as_ref() == Some(&run.id))
        .ok_or_else(|| {
            ApiError::conflict("partial Run startup has no stage execution; dispatch is withheld")
        })?;
    if execution.context_pack_id.is_none()
        || run.execution_target_json["hosted_workflow_policy_hash"]
            != json!(snapshot.metadata.workflow_policy_hash)
    {
        return Err(ApiError::conflict(
            "partial or conflicting Run startup cannot be dispatched",
        ));
    }
    refs["run_id"] = json!(run.id);
    refs["stage_execution_id"] = json!(execution.id);
    if matches!(run.status.as_str(), "completed" | "failed" | "cancelled") {
        crate::worker::reconcile_terminal_hosted_run(&state.store, run)
            .await
            .map_err(|e| ApiError::conflict(e.to_string()))?;
        refs["terminal_run_status"] = json!(run.status);
        state
            .store
            .record_workflow_operation(
                claim,
                &operation.id,
                "succeeded",
                &refs,
                "Run termination reconciled; its sealed stage outcome still determines progression",
                now(),
            )
            .await?;
        return Ok(condition(
            "progressing",
            "The existing Run is terminal; stage evidence will be evaluated next.",
        ));
    }
    state
        .store
        .record_workflow_operation(
            claim,
            &operation.id,
            "running",
            &refs,
            "Observed the existing Run; resource locks remain held",
            now(),
        )
        .await?;
    if run.status == "preparing" {
        return super::preparation::reconcile(state, claim, run, expired).await;
    }
    if redispatch && run.status == "queued" && claim.control == "active" && !expired {
        if run.budget_consumption.turns_used != 0 || run.budget_consumption.tokens_used != 0 {
            return Err(ApiError::conflict(
                "a queued Run with consumed execution cannot be replayed",
            ));
        }
        hosted::validate_run(state, run).await?;
        // The adapter observes the exact named/hash-bound Job before creation.
        // No new Run, budget or workspace is allocated by this retry.
        state.worker.spawn_run(run.clone(), run.cwd.clone());
    }
    Ok(condition(
        if expired { "wait_expired" } else { "waiting" },
        if expired {
            "The bounded wait expired; existing execution is still observed and no new dispatch is attempted."
        } else {
            "Observing the existing Run. New stages wait for its sealed evidence and active workflow control."
        },
    ))
}
