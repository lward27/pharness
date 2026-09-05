use super::state::{condition, now, Condition, Snapshot};
use crate::app::repo_mode::{self, RepoWorkItemActionExecutionRequest};
use crate::app::{ApiError, AppState, CONTROLLER_WAIT_INTERVAL_MS, CONTROLLER_WAIT_MAX_CHECKS};
use crate::dispatch::SourceJobKind;
use pharness_core::hosted_sdlc::HostedAutomaticAction;
use pharness_store::{
    StoredSourceDeliveryIntent, StoredWorkflowOperation, StoredWorkflowReconciliation,
};
use serde_json::json;

pub(super) async fn reconcile(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    snapshot: &Snapshot,
    operation: &StoredWorkflowOperation,
    expired: bool,
) -> Result<Condition, ApiError> {
    let subject = operation.resource_refs["action_resource"]
        .as_str()
        .ok_or_else(|| ApiError::conflict("source operation has no recorded ChangeSet"))?;
    let mut intent = state
        .store
        .get_source_delivery_intent_by_subject("work_item_change_set", subject)
        .await?
        .ok_or_else(|| {
            ApiError::conflict(
                "source publication has no durable intent; no duplicate dispatch was attempted",
            )
        })?;
    let policy = snapshot.metadata.workflow_policy.as_ref().unwrap();
    let work_item = state
        .store
        .get_work_item(&claim.work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &claim.work_item_id))?;
    if intent.repository_id != snapshot.metadata.repository_id
        || Some(intent.base_commit.as_str()) != work_item.source_commit.as_deref()
        || intent.source_repo != policy.delivery_binding.source_repo
        || intent.authorization["workflow_policy_hash"]
            != json!(snapshot.metadata.workflow_policy_hash)
        || intent.authorization["work_item_id"] != claim.work_item_id
        || !policy
            .automatic_actions
            .contains(&HostedAutomaticAction::SourceDelivery)
    {
        return Err(ApiError::conflict(
            "source intent no longer matches this WorkItem's recorded authority",
        ));
    }
    // Callbacks and fresh observation IDs must not extend an unchanged external
    // wait. Keep the existing wait allowance as an absolute operation bound.
    let deadline = operation.created_at.saturating_add(
        (CONTROLLER_WAIT_INTERVAL_MS as i64).saturating_mul(i64::from(CONTROLLER_WAIT_MAX_CHECKS)),
    );
    let expired = expired || now() >= deadline;
    let mut refs = operation.resource_refs.clone();
    refs["source_delivery_intent_id"] = json!(intent.id);
    refs["source_wait_deadline"] = json!(deadline);
    if matches!(
        intent.status.as_str(),
        "merged" | "failed" | "pull_request_closed"
    ) {
        state.store.record_workflow_operation(claim, &operation.id, "succeeded", &refs,
            "Source operation termination reconciled; the source outcome still determines delivery eligibility", now()).await?;
        return Ok(condition(
            if intent.status == "merged" {
                "progressing"
            } else {
                "blocked"
            },
            if intent.status == "merged" {
                "The exact source merge is recorded. Build, deployment, and runtime verification remain separate."
            } else {
                "Source delivery failed or its pull request was closed. No build or deployment is authorized by this result."
            },
        ));
    }
    if intent.status == "head_drift" {
        return Ok(condition(
            "blocked",
            "The pull-request head changed. Source evidence must be revalidated before delivery.",
        ));
    }
    if intent.status == "authorized" {
        if claim.control != "active" || expired {
            return Ok(condition(
                if expired {
                    "wait_expired"
                } else {
                    &claim.control
                },
                "The source intent is recorded but no new writer is being dispatched.",
            ));
        }
        let execution = intent.authorization["writer_execution_id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                ApiError::conflict("the original planned source writer identity is unavailable")
            })?;
        if intent.writer_execution_id.is_some() || intent.pull_request.is_some() {
            return Err(ApiError::conflict(
                "partial source publication has conflicting execution state",
            ));
        }
        intent = state
            .store
            .update_source_delivery_intent(
                &intent.id,
                intent.state_version,
                "writer_dispatched",
                Some(execution),
                None,
                None,
                None,
                None,
                "controller:hosted-workflow",
                "Recover the originally planned source writer identity before dispatch",
            )
            .await?;
    }
    state
        .store
        .record_workflow_operation(
            claim,
            &operation.id,
            "running",
            &refs,
            "Source delivery is still being reconciled; the repository lock remains held",
            now(),
        )
        .await?;
    if matches!(
        intent.status.as_str(),
        "writer_dispatched" | "observer_dispatched"
    ) {
        return observe_job(state, claim, &intent, expired).await;
    }
    if expired {
        return Ok(condition("wait_expired", "The bounded source wait expired. No further writer or observation Jobs are dispatched; recorded callbacks remain valid."));
    }
    if !policy
        .automatic_actions
        .contains(&HostedAutomaticAction::Observe)
    {
        return Err(ApiError::conflict(
            "the saved workflow does not authorize source observation",
        ));
    }
    let observed_at = intent
        .updated_at
        .parse::<i64>()
        .map_err(|_| ApiError::conflict("source observation freshness is unavailable"))?;
    if now().saturating_sub(observed_at) < (CONTROLLER_WAIT_INTERVAL_MS as i64) * 4 {
        return Ok(condition(
            "waiting",
            "Waiting for the next bounded source observation.",
        ));
    }
    let action = snapshot
        .actions
        .iter()
        .find(|action| action.id == "observe_source_delivery" && action.status == "ready")
        .ok_or_else(|| {
            ApiError::conflict("the source intent does not permit another observation")
        })?;
    repo_mode::execute_repo_work_item_action(
        state,
        &claim.work_item_id,
        &action.id,
        RepoWorkItemActionExecutionRequest {
            actor: "controller:hosted-workflow".into(),
            reason: "Observe the recorded source operation under its unchanged workflow authority"
                .into(),
            state_hash: action.state_hash.clone(),
            inference_policies: None,
            execution_policies: None,
        },
    )
    .await?;
    Ok(condition("waiting", "An isolated observer is checking the exact pull-request head, required checks, and merge provenance."))
}

async fn observe_job(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    intent: &StoredSourceDeliveryIntent,
    expired: bool,
) -> Result<Condition, ApiError> {
    let (kind, execution) = if intent.status == "writer_dispatched" {
        (SourceJobKind::Writer, intent.writer_execution_id.as_deref())
    } else {
        (
            SourceJobKind::Observer,
            intent.observer_execution_id.as_deref(),
        )
    };
    let execution =
        execution.ok_or_else(|| ApiError::conflict("source execution identity is unavailable"))?;
    let recover_missing =
        !expired && (kind == SourceJobKind::Observer || claim.control == "active");
    let observed = state
        .worker
        .reconcile_source_delivery_job(&intent.id, execution, kind, recover_missing)
        .await
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    if matches!(observed.status, "failed" | "succeeded") {
        // A callback can arrive while the Job is being observed. Let the next
        // pass consume it before treating a missing outcome as an exception.
        let latest = state
            .store
            .get_source_delivery_intent(&intent.id)
            .await?
            .ok_or_else(|| ApiError::conflict("the observed source intent disappeared"))?;
        if latest.state_version != intent.state_version {
            return Ok(condition("progressing", "The source callback arrived during observation; its evidence will be reconciled next."));
        }
        return Ok(condition("blocked", format!(
            "Source Job {} is {} without its outcome recorded. Its identity and repository lock are retained; it is not recreated.",
            observed.job_name, observed.status,
        )));
    }
    Ok(condition(
        if expired { "wait_expired" } else { "waiting" },
        format!(
            "Source Job {} is {}. The original execution identity is retained.",
            observed.job_name, observed.status,
        ),
    ))
}
