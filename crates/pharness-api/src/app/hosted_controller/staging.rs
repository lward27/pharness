use super::state::{condition, now, Condition, Snapshot};
use crate::app::hashing::canonical_material_hash as hash;
use crate::app::{ApiError, AppState, CONTROLLER_WAIT_INTERVAL_MS, CONTROLLER_WAIT_MAX_CHECKS};
use pharness_store::{
    BeginWorkflowOperation, StoredWorkflowOperation, StoredWorkflowReconciliation,
};
use serde_json::{json, Value};

mod baseline;
mod callbacks;
mod evidence;
mod preparation;
#[cfg(test)]
pub(in crate::app) use baseline::test_support::{
    seed_baseline, seed_inputs, seed_pending, seed_window,
};
pub(in crate::app) use callbacks::{
    internal_staging_attempt, internal_staging_context, internal_staging_outcome,
    internal_staging_plan,
};
pub(super) const ACTION: &str = "stage_verified_build";

fn deadline(operation: &StoredWorkflowOperation) -> i64 {
    operation.created_at.saturating_add(
        (CONTROLLER_WAIT_INTERVAL_MS as i64).saturating_mul(i64::from(CONTROLLER_WAIT_MAX_CHECKS)),
    )
}
fn artifact_id(kind: &str, execution: &str) -> String {
    format!("staging_{kind}_{execution}")
}

pub(super) async fn candidate(
    state: &AppState,
    snapshot: &Snapshot,
) -> Result<Option<(String, String)>, ApiError> {
    let Some((intent, _, evidence_hash)) =
        evidence::build(state, &snapshot.metadata.work_item_id).await?
    else {
        return Ok(None);
    };
    Ok(Some((
        hash(
            &json!({"pipeline_intent_id":intent.id,"build_evidence_hash":evidence_hash,"workflow_policy_hash":snapshot.metadata.workflow_policy_hash}),
        )?,
        intent.id,
    )))
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
    if operation.resource_refs.get("staging_dispatch").is_none() {
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
                "New staging preparation is stopped; the original operation remains recorded.",
            ));
        }
        let (expected, resource) = candidate(state, snapshot).await?.ok_or_else(|| {
            ApiError::conflict(
                "the recorded staging operation no longer has eligible build evidence",
            )
        })?;
        if expected != operation.input_hash {
            return Err(ApiError::conflict(
                "staging build or policy changed before preparation",
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
                "Recover the original staging preparation without changing its authority",
                now(),
            )
            .await?;
        operation = preparation::prepare(state, claim, &operation).await?;
    }
    let dispatch = &operation.resource_refs["staging_dispatch"];
    let id = dispatch["authority"]["deployment_intent_id"]
        .as_str()
        .ok_or_else(|| ApiError::conflict("staging deployment identity is missing"))?;
    let execution = dispatch["authority"]["execution_id"]
        .as_str()
        .ok_or_else(|| ApiError::conflict("staging execution identity is missing"))?;
    let saved = preparation::saved(state, id, execution).await?;
    if let Some(receipt) = state
        .store
        .get_artifact(&artifact_id("commit", execution))
        .await?
    {
        callbacks::settle(state, &saved, &receipt).await?;
        preparation::validate_current(state, &saved, false).await?;
        // Keep this operation and its repository/environment locks until actual
        // Argo and runtime verification finish. A commit is only one boundary.
        let mut refs = operation.resource_refs.clone();
        refs["staging_result"] = json!({"status":"gitops_committed","artifact_id":receipt.id,"gitops_commit_sha":receipt.content_json.as_ref().map(|v| &v["identity"]["gitops_commit_sha"]),"deployment_verification":"pending","runtime_verification":"pending"});
        if refs != operation.resource_refs {
            state.store.record_workflow_operation(claim, &operation.id, "running", &refs, "Staging GitOps commit observed; deployment and runtime verification remain required", now()).await?;
        }
        return Ok(condition(if expired { "wait_expired" } else { "waiting" }, "The staging digest change is committed. Argo deployment identity and application runtime verification remain pending; production is not authorized."));
    }
    let admitted = state
        .store
        .get_artifact(&artifact_id("attempt", execution))
        .await?
        .is_some();
    if !admitted {
        if let Some(condition) = baseline::reconcile(state, claim, &saved, expired).await? {
            return Ok(condition);
        }
    }
    let recover = !admitted && claim.control == "active" && !expired;
    if recover {
        preparation::validate_current(state, &saved, true).await?;
    }
    let status = state
        .worker
        .reconcile_hosted_build_job(&dispatch["executor_job_manifest"], recover)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    if matches!(status, "missing" | "failed" | "succeeded") && admitted {
        let dispatch_observer = !expired
            && operation
                .resource_refs
                .get("staging_observer_dispatch")
                .is_none();
        if dispatch_observer {
            let mut refs = operation.resource_refs.clone();
            refs["staging_observer_dispatch"] = json!({"job_name":dispatch["observer_job_manifest"]["metadata"]["name"],"admitted_at_ms":now()});
            state
                .store
                .record_workflow_operation(
                    claim,
                    &operation.id,
                    "running",
                    &refs,
                    "One bounded GitOps reader admitted to recover the original staging result",
                    now(),
                )
                .await?;
        }
        let observed = state
            .worker
            .reconcile_hosted_build_job(&dispatch["observer_job_manifest"], dispatch_observer)
            .await
            .map_err(|e| ApiError::conflict(e.to_string()))?;
        if matches!(observed, "failed" | "succeeded") || (observed == "missing" && !expired) {
            return Ok(condition("blocked", "The bounded staging observer is unavailable or ended without an accepted result. No replacement writer or observer was started."));
        }
    } else if matches!(status, "failed" | "succeeded") {
        return Ok(condition("blocked", "The staging executor ended before admission. Its original identity and evidence remain; no new GitOps write was authorized."));
    }
    Ok(condition(
        if expired { "wait_expired" } else { "waiting" },
        if expired {
            "The original staging window expired. No new write or recovery Job is started; late observations remain recordable."
        } else {
            "Observing the original staging update. A GitOps commit, deployment and runtime verification remain separate evidence."
        },
    ))
}

fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, ApiError> {
    value[key]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| ApiError::conflict(format!("staging evidence is missing {key}")))
}
