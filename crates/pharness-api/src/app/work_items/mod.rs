use super::pipeline::intents::{
    create_work_item_pipeline_intent, work_item_pipeline_intent_context,
};
use super::source::work_plans::create_work_plan_from_work_item;
use super::AppState;
use axum::routing::{get, post};
use axum::Router;

pub(super) mod actions;
pub(super) mod attempts;
pub(super) mod flow;
pub(super) mod lifecycle;
pub(super) mod preflight;
pub(super) mod reconcile;
pub(super) mod reconcile_model;
pub(super) mod rollback;
pub(super) mod rollback_state;
pub(super) mod wait_state;
pub(super) mod waits;

use actions::{advance_work_item, execute_work_item_action};
use attempts::{
    cancel_work_item, capture_work_item_change_set, execute_work_item, get_workspace,
    list_workspaces, replan_work_item, transition_work_item,
};
use flow::{get_work_item, list_work_items, work_item_flow};
use preflight::{create_work_item, preflight_work_item};
use reconcile::reconcile_work_item;
use rollback::{
    approve_rollback_intent, execute_rollback_intent, get_work_item_rollback_intent,
    observe_rollback_intent, preflight_rollback_intent, prepare_work_item_rollback_intent,
};
use waits::{
    list_work_item_controller_waits, list_work_item_events, reconcile_due_controller_waits,
};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/work-items",
            get(list_work_items).post(create_work_item),
        )
        .route("/api/work-items/preflight", post(preflight_work_item))
        .route("/api/work-items/:work_item_id", get(get_work_item))
        .route("/api/work-items/:work_item_id/flow", get(work_item_flow))
        .route(
            "/api/work-items/:work_item_id/events",
            get(list_work_item_events),
        )
        .route(
            "/api/work-items/:work_item_id/controller-waits",
            get(list_work_item_controller_waits),
        )
        .route(
            "/api/controller-waits/reconcile-due",
            post(reconcile_due_controller_waits),
        )
        .route(
            "/api/work-items/:work_item_id/work-plan",
            post(create_work_plan_from_work_item),
        )
        .route(
            "/api/work-items/:work_item_id/reconcile",
            post(reconcile_work_item),
        )
        .route(
            "/api/work-items/:work_item_id/actions/:action_id/execute",
            post(execute_work_item_action),
        )
        .route(
            "/api/work-items/:work_item_id/advance",
            post(advance_work_item),
        )
        .route(
            "/api/work-items/:work_item_id/rollback-intents",
            get(get_work_item_rollback_intent),
        )
        .route(
            "/api/work-items/:work_item_id/rollback-intents/prepare",
            post(prepare_work_item_rollback_intent),
        )
        .route(
            "/api/rollback-intents/:rollback_intent_id/approve",
            post(approve_rollback_intent),
        )
        .route(
            "/api/rollback-intents/:rollback_intent_id/preflight",
            post(preflight_rollback_intent),
        )
        .route(
            "/api/rollback-intents/:rollback_intent_id/execute",
            post(execute_rollback_intent),
        )
        .route(
            "/api/rollback-intents/:rollback_intent_id/observe",
            post(observe_rollback_intent),
        )
        .route(
            "/api/work-items/:work_item_id/replan",
            post(replan_work_item),
        )
        .route(
            "/api/work-items/:work_item_id/execute",
            post(execute_work_item),
        )
        .route(
            "/api/work-items/:work_item_id/capture-change-set",
            post(capture_work_item_change_set),
        )
        .route(
            "/api/work-items/:work_item_id/pipeline-intent",
            post(create_work_item_pipeline_intent),
        )
        .route(
            "/api/work-items/:work_item_id/pipeline-intent-context",
            get(work_item_pipeline_intent_context),
        )
        .route(
            "/api/work-items/:work_item_id/transition",
            post(transition_work_item),
        )
        .route(
            "/api/work-items/:work_item_id/cancel",
            post(cancel_work_item),
        )
        .route("/api/workspaces", get(list_workspaces))
        .route("/api/workspaces/:workspace_id", get(get_workspace))
}
