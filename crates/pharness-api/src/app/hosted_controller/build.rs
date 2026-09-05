use super::state::{condition, now, Condition, Snapshot};
use crate::app::hashing::canonical_material_hash as hash;
use crate::app::pipeline::hosted;
use crate::app::{ApiError, AppState, CONTROLLER_WAIT_INTERVAL_MS, CONTROLLER_WAIT_MAX_CHECKS};
use pharness_store::{
    BeginWorkflowOperation, StoredWorkflowOperation, StoredWorkflowReconciliation,
};
use serde_json::{json, Value};

mod outcomes;
mod preparation;
pub(in crate::app) use outcomes::{internal_build_attempt, internal_build_outcome};
pub(super) const ACTION: &str = "build_verified_source";

fn deadline(operation: &StoredWorkflowOperation) -> i64 {
    operation.created_at.saturating_add(
        (CONTROLLER_WAIT_INTERVAL_MS as i64) * i64::from(CONTROLLER_WAIT_MAX_CHECKS),
    )
}
fn attempt_id(execution: &str) -> String {
    format!("build_attempt_{execution}")
}
fn terminal_id(execution: &str) -> String {
    format!("build_terminal_{execution}")
}

async fn source(state: &AppState, item_id: &str) -> Result<Value, ApiError> {
    let plan = state
        .store
        .get_work_plan_by_work_item(item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("hosted build has no WorkPlan"))?;
    let change = state
        .store
        .get_change_set_by_work_plan(&plan.id)
        .await?
        .ok_or_else(|| ApiError::conflict("hosted build has no approved ChangeSet"))?;
    hosted::source_provenance(&state.store, &change)
        .await?
        .ok_or_else(|| ApiError::conflict("source-only work cannot enter hosted build progression"))
}

pub(super) async fn candidate(
    state: &AppState,
    snapshot: &Snapshot,
) -> Result<Option<(String, String)>, ApiError> {
    if !snapshot
        .stages
        .iter()
        .any(|s| s.stage_key == "source_delivery" && s.status == "succeeded")
    {
        return Ok(None);
    }
    let provenance = source(state, &snapshot.metadata.work_item_id).await?;
    let plan = state
        .store
        .get_work_plan_by_work_item(&snapshot.metadata.work_item_id)
        .await?
        .unwrap();
    let change = state
        .store
        .get_change_set_by_work_plan(&plan.id)
        .await?
        .unwrap();
    let input_hash = hash(
        &json!({"change_set_id":change.id,"source_provenance":provenance,"workflow_policy_hash":snapshot.metadata.workflow_policy_hash}),
    )?;
    let identity = hash(&json!([snapshot.metadata.work_item_id, ACTION, input_hash]))?;
    if let Some(previous) = state
        .store
        .get_workflow_operation(&format!(
            "workflowop_{}",
            identity.trim_start_matches("sha256:")
        ))
        .await?
    {
        if previous.status == "succeeded" {
            if previous.resource_refs["build_result"]["status"] == "verified" {
                return Ok(None);
            }
            return Err(ApiError::conflict("The recorded build failed or lacks verified source/image results. No new build or deployment was started."));
        }
    }
    Ok(Some((input_hash, change.id)))
}

pub(super) async fn reconcile(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    snapshot: &Snapshot,
    operation: &StoredWorkflowOperation,
    expired: bool,
) -> Result<Condition, ApiError> {
    let expired = expired || now() >= deadline(operation);
    let mut operation = operation.clone();
    if operation.resource_refs.get("build_dispatch").is_none() {
        if claim.control != "active" || expired {
            if operation.status == "pending" {
                state
                    .store
                    .release_pending_workflow_locks(claim, &operation.id, now())
                    .await?;
            }
            return Ok(condition(
                if expired {
                    "wait_expired"
                } else {
                    &claim.control
                },
                "The original build is recorded; new build preparation is stopped.",
            ));
        }
        let Some((expected, resource)) = candidate(state, snapshot).await? else {
            return Err(ApiError::conflict(
                "the recorded build no longer has eligible source evidence",
            ));
        };
        if expected != operation.input_hash {
            return Err(ApiError::conflict(
                "the recorded build source or policy changed before dispatch",
            ));
        }
        if operation.status == "pending" {
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
                        action: ACTION,
                        input_hash: &operation.input_hash,
                        effect: &operation.effect,
                        resource_keys: &keys,
                    },
                    now(),
                )
                .await?;
        }
        let mut refs = operation.resource_refs.clone();
        refs["action_resource"] = json!(resource);
        operation = state
            .store
            .record_workflow_operation(
                claim,
                &operation.id,
                "running",
                &refs,
                "Recover the original build preparation without changing its authority",
                now(),
            )
            .await?;
        operation = preparation::prepare(state, claim, &operation).await?;
    }
    let d = &operation.resource_refs["build_dispatch"];
    let id = operation.resource_refs["pipeline_intent_id"]
        .as_str()
        .ok_or_else(|| ApiError::conflict("hosted build intent is not recorded"))?;
    let intent = state
        .store
        .get_pipeline_intent(id)
        .await?
        .ok_or_else(|| ApiError::conflict("recorded hosted build intent is unavailable"))?;
    // A pause may occur after preparation but before execution-state admission.
    if intent.intent_json.get("execution_state").is_none() && (claim.control != "active" || expired)
    {
        return Ok(condition(
            if expired {
                "wait_expired"
            } else {
                &claim.control
            },
            "No executor is dispatched while new build work is stopped.",
        ));
    }
    let intent = preparation::mark_executing(state, &operation).await?;
    let intent = outcomes::settle(state, &intent).await?;
    let execution_id = d["execution_id"]
        .as_str()
        .ok_or_else(|| ApiError::conflict("recorded build execution is unavailable"))?;
    if state
        .store
        .get_artifact(&terminal_id(execution_id))
        .await?
        .is_some()
    {
        let verified = intent.intent_json["execution_state"]["state"] == "pipeline_run_succeeded"
            && intent.intent_json["build_output"]["status"] == "verified"
            && hosted::validate_observed_intent(state, &intent)
                .await
                .is_ok();
        let mut refs = operation.resource_refs.clone();
        refs["build_result"] = json!({"status":if verified {"verified"}else{"blocked"},"terminal_artifact_id":terminal_id(execution_id),"build_output":intent.intent_json.get("build_output"),"pipeline_run_uid":intent.intent_json["execution_state"]["pipeline_run_uid"]});
        state
            .store
            .record_workflow_operation(
                claim,
                &operation.id,
                "succeeded",
                &refs,
                "Recorded build outcome reconciled; staging and production remain separate",
                now(),
            )
            .await?;
        return Ok(condition(
            if verified { "progressing" } else { "blocked" },
            if verified {
                "The merged source has a verified build result. Staging, production approval and runtime verification remain."
            } else {
                "The build failed, lacks declared source/image evidence, or its source authority changed. Delivery is stopped."
            },
        ));
    }
    let admitted = state
        .store
        .get_artifact(&attempt_id(execution_id))
        .await?
        .is_some();
    let recover = !admitted && claim.control == "active" && !expired;
    if recover {
        preparation::saved(state, id, execution_id, true).await?;
    }
    let status = state
        .worker
        .reconcile_hosted_build_job(&d["executor_job_manifest"], recover)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    if matches!(status, "missing" | "failed" | "succeeded") && admitted {
        // The one recorded observer has no creation permission. A lost executor
        // acknowledgement can never allocate a second PipelineRun or workspace.
        let dispatch_observer = !expired
            && operation
                .resource_refs
                .get("build_observer_dispatch")
                .is_none();
        if dispatch_observer {
            // Fence before the external call. After an interrupted or uncertain
            // create, only observe this identity; expiry/TTL cannot renew it.
            let mut refs = operation.resource_refs.clone();
            refs["build_observer_dispatch"] = json!({
                "job_name":d["observer_job_manifest"]["metadata"]["name"],
                "admitted_at_ms":now()
            });
            state
                .store
                .record_workflow_operation(
                    claim,
                    &operation.id,
                    "running",
                    &refs,
                    "One read-only build recovery Job admitted within the original deadline",
                    now(),
                )
                .await?;
        }
        let observed = state
            .worker
            .reconcile_hosted_build_job(&d["observer_job_manifest"], dispatch_observer)
            .await
            .map_err(|e| ApiError::conflict(e.to_string()))?;
        if matches!(observed, "failed" | "succeeded") || (observed == "missing" && !expired) {
            return Ok(condition("blocked","The one bounded build observer is unavailable or ended without an accepted terminal result. Retained evidence requires intervention; no replacement observer was started."));
        }
    } else if matches!(status, "failed" | "succeeded") {
        return Ok(condition("blocked","The build executor ended before admission. Its original identity and grant are retained; no replacement build was started."));
    }
    Ok(condition(
        if expired { "wait_expired" } else { "waiting" },
        if expired {
            "The original build wait expired. No new build or observation Job is created; late outcomes remain recordable."
        } else {
            "Observing the original build execution. Source, image and deployment results remain separate."
        },
    ))
}
