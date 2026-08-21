use super::super::audit::append_controller_wait_audit_event;
use super::super::clock::{current_millis, unique_suffix};
use super::super::sessions::root_session_for_request;
use super::super::{ApiError, AppState, CONTROLLER_WAIT_INTERVAL_MS, CONTROLLER_WAIT_MAX_CHECKS};
use super::reconcile_model::WorkItemReconcileAction;
use pharness_store::{CreateControllerWait, StoredControllerWait, StoredWorkItem};
use serde_json::json;

/// Schedule a bounded, durable wait for an external controller dependency.
/// This records intent to observe later; it does not run a poller or mutate the
/// external system. A future controller worker owns due-wait execution.
pub(in crate::app) async fn schedule_controller_wait(
    state: &AppState,
    work_item: &StoredWorkItem,
    action: WorkItemReconcileAction,
    actor: Option<String>,
) -> Result<(StoredControllerWait, bool), ApiError> {
    let wait_kind = action
        .controller_wait_kind()
        .expect("only controller wait actions may schedule waits");
    if let Some(active) = state
        .store
        .get_active_controller_wait_for_work_item(&work_item.id)
        .await?
    {
        if active.wait_kind == wait_kind
            && active.subject_kind == "work_item"
            && active.subject_id == work_item.id
        {
            return Ok((active, false));
        }
        let reason = format!("controller moved to {}", action.as_str());
        let superseded = state
            .store
            .supersede_controller_wait(&active.id, reason.clone())
            .await?;
        append_controller_wait_audit_event(
            &state.store,
            &superseded,
            "controller_wait.superseded",
            actor.clone(),
            Some(reason),
        )
        .await?;
    }

    let work_plan = state
        .store
        .get_work_plan_by_work_item(&work_item.id)
        .await?;
    let (session_id, run_id) = match work_plan {
        Some(plan) => (
            plan.session_id,
            plan.run_id.or_else(|| work_item.current_run_id.clone()),
        ),
        None => {
            root_session_for_request(
                &state.store,
                None,
                work_item.current_run_id.clone(),
                "controller wait",
            )
            .await?
        }
    };
    let now = current_millis();
    let work_item_budget_ms = u128::from(work_item.max_elapsed_seconds).saturating_mul(1_000);
    let controller_budget_ms =
        CONTROLLER_WAIT_INTERVAL_MS.saturating_mul(u128::from(CONTROLLER_WAIT_MAX_CHECKS));
    let deadline_at = now.saturating_add(work_item_budget_ms.min(controller_budget_ms));
    let wait = state
        .store
        .create_controller_wait(CreateControllerWait {
            id: format!("cwait_{}", unique_suffix()),
            work_item_id: work_item.id.clone(),
            session_id,
            run_id,
            status: "active".to_string(),
            wait_kind: wait_kind.to_string(),
            subject_kind: "work_item".to_string(),
            subject_id: work_item.id.clone(),
            next_check_at: now.saturating_add(CONTROLLER_WAIT_INTERVAL_MS).to_string(),
            deadline_at: deadline_at.to_string(),
            max_checks: CONTROLLER_WAIT_MAX_CHECKS,
            data_json: json!({
                "source": "work_item.reconcile",
                "controller_action": action.as_str(),
                "work_item_id": work_item.id,
                "source_provenance": {
                    "repo": work_item.source_repo,
                    "ref": work_item.source_ref,
                },
                "target": {
                    "environment": work_item.target_environment,
                    "namespace": work_item.target_namespace,
                    "argo_application": work_item.argo_application,
                    "production_impacting": work_item.production_impacting,
                },
                "automatic_execution": false,
                "automatic_retry": false,
                "automatic_rollback": false,
            }),
        })
        .await?;
    append_controller_wait_audit_event(
        &state.store,
        &wait,
        "controller_wait.scheduled",
        actor,
        None,
    )
    .await?;
    Ok((wait, true))
}

pub(in crate::app) async fn supersede_active_controller_wait_if_present(
    state: &AppState,
    work_item_id: &str,
    reason: String,
    actor: Option<String>,
) -> Result<Option<StoredControllerWait>, ApiError> {
    let Some(active) = state
        .store
        .get_active_controller_wait_for_work_item(work_item_id)
        .await?
    else {
        return Ok(None);
    };
    let wait = state
        .store
        .supersede_controller_wait(&active.id, reason.clone())
        .await?;
    append_controller_wait_audit_event(
        &state.store,
        &wait,
        "controller_wait.superseded",
        actor,
        Some(reason),
    )
    .await?;
    Ok(Some(wait))
}
