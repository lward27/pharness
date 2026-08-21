use super::*;
use axum::routing::{get, post};
use axum::Router;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/work-plans/from-remediation-plan",
            post(create_work_plan_from_remediation_plan),
        )
        .route("/api/work-plans", get(list_work_plans))
        .route("/api/work-plans/:work_plan_id", get(get_work_plan))
        .route(
            "/api/work-plans/:work_plan_id/readiness",
            get(work_plan_readiness),
        )
        .route("/api/work-plans/:work_plan_id/flow", get(work_plan_flow))
        .route(
            "/api/work-plans/:work_plan_id/revise",
            post(revise_work_plan),
        )
        .route(
            "/api/work-plans/:work_plan_id/transition",
            post(transition_work_plan),
        )
        .route(
            "/api/work-plans/:work_plan_id/trusted-envelope",
            post(create_work_plan_trusted_envelope),
        )
        .route(
            "/api/change-sets",
            get(list_change_sets).post(create_change_set),
        )
        .route("/api/change-sets/:change_set_id", get(get_change_set))
        .route(
            "/api/change-sets/:change_set_id/readiness",
            get(change_set_readiness),
        )
        .route("/api/change-sets/:change_set_id/flow", get(change_set_flow))
        .route(
            "/api/change-sets/:change_set_id/revise",
            post(revise_change_set),
        )
        .route(
            "/api/change-sets/:change_set_id/transition",
            post(transition_change_set),
        )
        .route(
            "/api/change-sets/:change_set_id/trusted-envelope",
            post(create_change_set_trusted_envelope),
        )
        .route(
            "/api/change-sets/:change_set_id/git-delivery/prepare",
            post(prepare_change_set_git_delivery),
        )
        .route(
            "/api/change-sets/:change_set_id/git-delivery/authorize",
            post(authorize_change_set_git_delivery),
        )
        .route(
            "/api/change-sets/:change_set_id/git-delivery/preflight",
            post(preflight_change_set_git_delivery),
        )
        .route(
            "/api/change-sets/:change_set_id/git-delivery/execute",
            post(execute_change_set_git_delivery),
        )
        .route(
            "/api/change-sets/:change_set_id/git-delivery/observe",
            post(observe_change_set_git_delivery),
        )
}
