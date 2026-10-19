//! A second, independent production window after `release` is sealed. The
//! window is bound to the sealed release outcome and starts only from identity
//! reads taken after it; the only GitOps write this step can make is the
//! automatic rollback after a failed window.
use super::super::state::{Condition, Snapshot};
use super::{
    deadline, delivery, hash, now, operation_id, proposal, required, runtime, seal, ApiError,
    AppState, PROMOTE_ACTION,
};
use pharness_core::tools::FinanceVerificationPhase;
use pharness_store::{StoredStageOutcome, StoredWorkflowOperation, StoredWorkflowReconciliation};
use serde_json::json;

pub(super) fn input_hash(
    recorded: &proposal::Recorded,
    release: &StoredStageOutcome,
) -> Result<String, ApiError> {
    hash(
        &json!({"deployment_intent_id":recorded.deployment.id,"release_outcome_id":release.id,"release_outcome_hash":release.content_hash}),
    )
}

pub(super) async fn prepare(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    snapshot: &Snapshot,
    operation: &StoredWorkflowOperation,
) -> Result<StoredWorkflowOperation, ApiError> {
    let recorded = delivery::recorded_proposal(state, snapshot).await?;
    let promote_id = operation_id(
        &snapshot.metadata.work_item_id,
        PROMOTE_ACTION,
        &delivery::input_hash(&recorded)?,
    )?;
    let promote = state
        .store
        .get_workflow_operation(&promote_id)
        .await?
        .filter(|o| o.status == "succeeded")
        .ok_or_else(|| {
            ApiError::conflict("post-release observation requires a verified production release")
        })?;
    let release = seal::release_sealed(state, snapshot, &promote).await?;
    if operation.input_hash != input_hash(&recorded, &release)? {
        return Err(ApiError::conflict(
            "post-release observation differs from the sealed release",
        ));
    }
    let execution = required(
        &promote.resource_refs["production_dispatch"]["authority"],
        "execution_id",
    )?;
    let mut refs = operation.resource_refs.clone();
    refs["observe_dispatch"] = json!({"deployment_intent_id":recorded.deployment.id,"execution_id":execution,"promote_operation_id":promote.id,"release_outcome_id":release.id,"release_outcome_hash":release.content_hash,"deadline_ms":deadline(operation)});
    state
        .store
        .record_workflow_operation(
            claim,
            &operation.id,
            "running",
            &refs,
            "Post-release observation bound to the sealed release before any native read",
            now(),
        )
        .await
        .map_err(Into::into)
}

pub(super) async fn context(
    state: &AppState,
    operation: &StoredWorkflowOperation,
) -> Result<runtime::Context, ApiError> {
    let d = &operation.resource_refs["observe_dispatch"];
    let deployment_id = required(d, "deployment_intent_id")?;
    let execution = required(d, "execution_id")?;
    let saved = delivery::saved(state, deployment_id, execution).await?;
    let result = &saved.operation.resource_refs["production_runtime_result"];
    if d["promote_operation_id"] != saved.operation.id
        || saved.operation.status != "succeeded"
        || result["status"] != "verified"
        || d["deadline_ms"] != deadline(operation)
    {
        return Err(ApiError::conflict(
            "post-release observation is not bound to a verified production release",
        ));
    }
    let release = state
        .store
        .list_effective_stage_outcomes(&saved.authority.proposal.work_item_id)
        .await?
        .into_iter()
        .find(|o| o.stage_key == "release")
        .filter(|o| {
            d["release_outcome_id"] == o.id
                && d["release_outcome_hash"] == o.content_hash
                && o.status == "succeeded"
                && o.outcome["pinned_inputs"]["workflow_operation_id"] == saved.operation.id
        })
        .ok_or_else(|| ApiError::conflict("the sealed release outcome changed or is missing"))?;
    let production_result = state
        .store
        .get_artifact(required(result, "artifact_id")?)
        .await?
        .ok_or_else(|| ApiError::conflict("production runtime result artifact is unavailable"))?;
    let body = production_result
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("production runtime result has no content"))?;
    if hash(body)? != result["artifact_sha256"] {
        return Err(ApiError::conflict(
            "production runtime result differs from the sealed projection",
        ));
    }
    let receipt = state
        .store
        .get_artifact(&super::artifact_id("commit", execution))
        .await?
        .ok_or_else(|| ApiError::conflict("production commit receipt is unavailable"))?;
    let production = delivery::runtime_context(&saved, &receipt)?;
    let completed = body["completed_at_ms"]
        .as_i64()
        .ok_or_else(|| ApiError::conflict("production runtime result has no completion clock"))?;
    Ok(runtime::Context {
        operation: operation.clone(),
        deployment: saved.deployment,
        pipeline: saved.pipeline,
        work_item_id: saved.authority.proposal.work_item_id.clone(),
        execution_id: execution.into(),
        expires_at_ms: deadline(operation),
        expected: production.expected,
        phase: FinanceVerificationPhase::Production,
        prefix: "observe",
        projection: ("hosted_production", "post_release_observation"),
        binding: json!({"authority_hash":saved.authority_hash,"release_outcome_id":release.id,"release_outcome_hash":release.content_hash,"production_result_artifact_id":production_result.id,"production_result_sha256":result["artifact_sha256"]}),
        commit_checked_at_ms: completed,
    })
}

pub(super) async fn reconcile(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    _snapshot: &Snapshot,
    operation: &StoredWorkflowOperation,
    timed_out: bool,
) -> Result<Condition, ApiError> {
    let context = context(state, operation).await?;
    runtime::reconcile(state, claim, &context, timed_out).await
}

/// Seal `observe` and complete the WorkItem. Completion is the final write and
/// the store refuses it unless every hosted stage has a succeeded outcome.
pub(super) async fn finish(
    state: &AppState,
    snapshot: &Snapshot,
    observed: &StoredWorkflowOperation,
) -> Result<(), ApiError> {
    let outcome = seal::observe_sealed(state, snapshot, observed).await?;
    if snapshot.metadata.closed_at.is_none() {
        state
            .store
            .update_repo_work_item_status(
                &snapshot.metadata.work_item_id,
                "completed",
                "controller:hosted-production",
                &format!(
                    "production release and post-release observation sealed ({})",
                    outcome.id
                ),
                true,
            )
            .await?;
    }
    Ok(())
}
