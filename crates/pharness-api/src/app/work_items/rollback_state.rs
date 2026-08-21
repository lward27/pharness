use super::super::{ApiError, AppState};
use pharness_core::RunId;
use pharness_store::{StoredArtifact, StoredWorkItem};
use serde_json::{json, Value};

pub(in crate::app) async fn latest_rollback_intent(
    state: &AppState,
    item: &StoredWorkItem,
    rollback_intent_id: Option<&str>,
) -> Result<Option<Value>, ApiError> {
    let Some(run_id) = work_item_provenance_run_id(state, item).await? else {
        return Ok(None);
    };
    let artifact = state
        .store
        .list_artifacts(&run_id)
        .await?
        .into_iter()
        .filter(|artifact| artifact.kind == "rollback_intent")
        .filter(|artifact| {
            artifact
                .content_json
                .as_ref()
                .and_then(|content| content.get("work_item_id"))
                .and_then(Value::as_str)
                == Some(item.id.as_str())
        })
        .filter(|artifact| {
            rollback_intent_id.map_or(true, |id| {
                artifact
                    .content_json
                    .as_ref()
                    .and_then(|content| content.get("rollback_intent_id"))
                    .and_then(Value::as_str)
                    == Some(id)
            })
        })
        .max_by_key(|artifact| (artifact.created_at.clone(), artifact.id.clone()));
    Ok(artifact.as_ref().map(rollback_intent_response))
}

/// Resolve the durable run that owns delivery and rollback evidence after a
/// coding attempt has finished. `current_run_id` is intentionally cleared at
/// attempt termination, so completed WorkItems must fall back to the captured
/// ChangeSet or WorkPlan provenance instead of losing their evidence graph.
pub(in crate::app) async fn work_item_provenance_run_id(
    state: &AppState,
    item: &StoredWorkItem,
) -> Result<Option<RunId>, ApiError> {
    if let Some(run_id) = item.current_run_id.clone() {
        return Ok(Some(run_id));
    }
    let Some(work_plan) = state.store.get_work_plan_by_work_item(&item.id).await? else {
        return Ok(None);
    };
    if let Some(change_set) = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?
    {
        if change_set.run_id.is_some() {
            return Ok(change_set.run_id);
        }
    }
    Ok(work_plan.run_id)
}

pub(in crate::app) fn rollback_intent_response(artifact: &StoredArtifact) -> Value {
    json!({
        "artifact_id": artifact.id,
        "created_at": artifact.created_at,
        "content": artifact.content_json,
    })
}
