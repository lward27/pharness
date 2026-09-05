use super::DISPATCH_BOUNDARY;
use crate::app::clock::current_millis;
use crate::app::hashing::canonical_material_hash;
use crate::app::{ApiError, AppState};
use crate::dto::WorkItemActionResponse;
use pharness_store::StoredWorkflowReconciliation;
use serde_json::{json, Value};

fn control_hash(state: &StoredWorkflowReconciliation) -> Result<String, ApiError> {
    canonical_material_hash(&json!({
        "work_item_id":state.work_item_id,
        "control":state.control,
        "control_version":state.control_version,
    }))
}

pub(in crate::app) fn public_state(state: &StoredWorkflowReconciliation) -> Value {
    json!({
        "control":state.control,
        "control_version":state.control_version,
        "condition":state.condition,
        "reason":state.condition_reason,
        "next_check_at":state.next_due_at.to_string(),
        "as_of":state.updated_at.to_string(),
        "observation_and_authorized_recovery_continue":true,
    })
}

pub(in crate::app) fn control_actions(
    state: &StoredWorkflowReconciliation,
    closed: bool,
) -> Result<Vec<WorkItemActionResponse>, ApiError> {
    if closed || state.control == "cancelled" {
        return Ok(Vec::new());
    }
    let actions = if state.control == "paused" {
        [("resume_workflow", "Resume new development and promotion within the saved authorization."),
         ("cancel_workflow", "Cancel future development and promotion. Existing evidence is retained; observation and already-authorized release recovery continue.")]
    } else {
        [("pause_workflow", "Pause new development and promotion. Work already dispatched may finish; observation and already-authorized release recovery continue."),
         ("cancel_workflow", "Cancel future development and promotion. Existing evidence is retained; observation and already-authorized release recovery continue.")]
    };
    let hash = control_hash(state)?;
    Ok(actions
        .into_iter()
        .map(|(id, summary)| WorkItemActionResponse {
            id: id.into(),
            lifecycle_stage: "workflow".into(),
            resource: state.work_item_id.clone(),
            status: "ready".into(),
            effect_class: "workflow_control".into(),
            blockers: Vec::new(),
            approval_required: true,
            approval_requirements: Vec::new(),
            external_effect_summary: summary.into(),
            state_hash: hash.clone(),
        })
        .collect())
}

pub(in crate::app) async fn execute_control(
    state: &AppState,
    work_item_id: &str,
    action: &str,
    expected_hash: &str,
    actor: &str,
    reason: &str,
) -> Result<Value, ApiError> {
    let control = match action {
        "pause_workflow" => "paused",
        "resume_workflow" => "active",
        "cancel_workflow" => "cancelled",
        _ => return Err(ApiError::conflict("Hosted work advances through its controller. Use pause, resume or cancel to control progression; production approval is a separate decision.")),
    };
    let _boundary = DISPATCH_BOUNDARY.lock().await;
    let current = state
        .store
        .get_workflow_reconciliation(work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("hosted controller state is unavailable"))?;
    if control_hash(&current)? != expected_hash
        || !control_actions(&current, false)?
            .iter()
            .any(|candidate| candidate.id == action)
    {
        return Err(ApiError::conflict(
            "workflow control changed; refresh before applying this decision",
        ));
    }
    let now = i64::try_from(current_millis())
        .map_err(|_| ApiError::internal("clock is outside the supported range"))?;
    let updated = state
        .store
        .set_workflow_control(
            work_item_id,
            current.control_version,
            control,
            actor,
            reason,
            now,
        )
        .await?;
    Ok(json!({"work_item_id":work_item_id,"workflow_control":public_state(&updated)}))
}
