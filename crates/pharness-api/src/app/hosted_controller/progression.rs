use super::operations::{execute_operation, reconcile_operation};
use super::state::{condition, continuation_candidate, now, Condition, Snapshot};
use crate::app::hashing::canonical_material_hash;
use crate::app::hosted_workflow::stages as hosted;
use crate::app::{ApiError, AppState};
use pharness_core::hosted_sdlc::HostedAutomaticAction;
use pharness_store::{BeginWorkflowOperation, StoredWorkflowReconciliation};
use serde_json::json;

pub(super) async fn advance(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    snapshot: &Snapshot,
    expired: bool,
) -> Result<Condition, ApiError> {
    // Always inspect the existing operation first, including while paused.
    if let Some(operation) = state
        .store
        .active_workflow_operation(&claim.work_item_id)
        .await?
    {
        return reconcile_operation(state, claim, snapshot, operation, expired, true).await;
    }
    if snapshot.metadata.closed_at.is_some() {
        return Ok(condition(
            "closed",
            "The WorkItem is closed; retained evidence is unchanged.",
        ));
    }
    if claim.control != "active" {
        return Ok(condition(&claim.control, "New development and promotion are stopped. Observation and authorized recovery continue."));
    }
    if expired {
        return Ok(condition("wait_expired", "The existing bounded wait expired without new evidence. No additional work or budget was created."));
    }
    let candidate = snapshot.actions.iter().find(|a| {
        a.status == "ready"
            && matches!(
                a.id.as_str(),
                "start_planner"
                    | "approve_work_plan"
                    | "authorize_stage_chain"
                    | "approve_change_set"
                    | "authorize_source_delivery"
            )
    });
    let (action, input_hash, resource) = if let Some(action) = candidate {
        let permission = match action.id.as_str() {
            "start_planner" | "approve_work_plan" => HostedAutomaticAction::Plan,
            "authorize_stage_chain" => HostedAutomaticAction::Implement,
            "authorize_source_delivery" => HostedAutomaticAction::SourceDelivery,
            _ => HostedAutomaticAction::Verify,
        };
        require_authority(snapshot, permission)?;
        if matches!(
            action.id.as_str(),
            "approve_work_plan" | "approve_change_set"
        ) {
            super::approval::validate(state, &claim.work_item_id, &action.id, &action.resource)
                .await?;
        }
        if action.id == "start_planner" {
            hosted::validate_planned(state, &snapshot.metadata, "repo-planner").await?;
        } else if action.id == "authorize_stage_chain" {
            for profile in [
                "repo-builder",
                "repo-repair",
                "repo-test-diagnoser",
                "repo-verifier",
            ] {
                hosted::validate_preview(state, &snapshot.metadata, profile, None).await?;
            }
        }
        (
            action.id.clone(),
            action.state_hash.clone(),
            action.resource.clone(),
        )
    } else if let Some(run) = continuation_candidate(snapshot) {
        require_authority(snapshot, HostedAutomaticAction::Test)?;
        require_authority(snapshot, HostedAutomaticAction::Verify)?;
        require_authority(snapshot, HostedAutomaticAction::Implement)?;
        (
            "continue_stage".into(),
            canonical_material_hash(&json!({"run":run.id,"result":run.result_json}))?,
            run.id.to_string(),
        )
    } else {
        return Ok(condition(
            "waiting",
            snapshot
                .actions
                .first()
                .map(|a| a.external_effect_summary.clone())
                .unwrap_or_else(|| {
                    "Waiting for stage evidence; no eligible automatic action is available.".into()
                }),
        ));
    };
    let identity = canonical_material_hash(&json!([claim.work_item_id, action, input_hash]))?;
    let id = format!("workflowop_{}", identity.trim_start_matches("sha256:"));
    if state
        .store
        .get_workflow_operation(&id)
        .await?
        .is_some_and(|o| o.status == "succeeded")
    {
        return Ok(condition("blocked", "This operation is already reconciled. Its stage evidence does not permit further progression."));
    }
    let runs_worker = matches!(
        action.as_str(),
        "start_planner" | "authorize_stage_chain" | "continue_stage"
    );
    let repo_lock = format!("repository:{}", snapshot.metadata.repository_id);
    let keys = if runs_worker {
        vec!["coding", repo_lock.as_str()]
    } else if action == "authorize_source_delivery" {
        vec![repo_lock.as_str()]
    } else {
        Vec::new()
    };
    let operation = state
        .store
        .begin_workflow_operation(
            claim,
            BeginWorkflowOperation {
                id: &id,
                action: &action,
                input_hash: &input_hash,
                effect: "development",
                resource_keys: &keys,
            },
            now(),
        )
        .await?;
    let refs = json!({"action_resource":resource,"before_run_ids":snapshot.runs.iter().map(|r| r.id.as_str()).collect::<Vec<_>>()});
    execute_operation(state, claim, snapshot, operation, refs).await
}

fn require_authority(snapshot: &Snapshot, action: HostedAutomaticAction) -> Result<(), ApiError> {
    if !snapshot
        .metadata
        .workflow_policy
        .as_ref()
        .unwrap()
        .automatic_actions
        .contains(&action)
    {
        return Err(ApiError::conflict(
            "the saved workflow does not authorize this automatic action",
        ));
    }
    Ok(())
}
