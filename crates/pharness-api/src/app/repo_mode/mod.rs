mod actions;
mod creation;
mod evidence;
mod projection;
mod source_delivery;
mod stage_authorization;
mod stages;
mod state;

#[cfg(test)]
mod tests;

pub(in crate::app) use self::actions::{
    execute_repo_work_item_action, RepoWorkItemActionExecutionRequest,
};
pub(in crate::app) use self::projection::{
    repo_controller_actions, repo_work_item_flow, validate_change_set_outcome_binding,
    ChangeSetOutcomeBinding,
};
#[cfg(test)]
pub(in crate::app) use self::source_delivery::authorize_and_dispatch_source_delivery;
pub(in crate::app) use self::source_delivery::{
    internal_source_delivery_context, internal_source_delivery_observation_context,
    internal_source_delivery_observation_outcome, internal_source_delivery_writer_outcome,
};
#[cfg(test)]
pub(in crate::app) use self::stage_authorization::authorize_repo_stage_chain;
#[cfg(test)]
pub(in crate::app) use self::stages::start_repo_planner;
pub(in crate::app) use self::stages::{
    continue_repo_stage_chain, record_repo_chain_continuation_failure,
};
#[cfg(test)]
pub(in crate::app) use self::state::seal_repo_inapplicable_tail;
pub(in crate::app) use self::state::{is_repo_work_item, repo_work_item_state_hash};

use self::creation::{create_repo_work_item, preflight_repo_work_item};
use self::evidence::{
    create_annotation, get_evidence_validation, get_stage_context_pack, get_stage_execution,
    get_stage_outcome, list_annotations, list_stage_executions, list_work_item_evidence,
};
use crate::app::AppState;
use axum::routing::{get, post};
use axum::Router;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/products/:product_id/work-items/preflight",
            post(preflight_repo_work_item),
        )
        .route(
            "/api/products/:product_id/work-items",
            post(create_repo_work_item),
        )
        .route(
            "/api/work-items/:work_item_id/stage-executions",
            get(list_stage_executions),
        )
        .route(
            "/api/stage-executions/:stage_execution_id",
            get(get_stage_execution),
        )
        .route(
            "/api/stage-executions/:stage_execution_id/outcome",
            get(get_stage_outcome),
        )
        .route(
            "/api/stage-executions/:stage_execution_id/context-pack",
            get(get_stage_context_pack),
        )
        .route(
            "/api/work-items/:work_item_id/annotations",
            get(list_annotations).post(create_annotation),
        )
        .route(
            "/api/work-items/:work_item_id/evidence",
            get(list_work_item_evidence),
        )
        .route(
            "/api/evidence-validations/:evidence_validation_id",
            get(get_evidence_validation),
        )
}
