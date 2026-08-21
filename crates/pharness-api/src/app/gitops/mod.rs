use super::*;
use axum::routing::{get, post};
use axum::Router;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/gitops-change-sets",
            get(list_gitops_change_sets).post(create_gitops_change_set),
        )
        .route(
            "/api/gitops-change-sets/:gitops_change_set_id",
            get(get_gitops_change_set),
        )
        .route(
            "/api/gitops-change-sets/:gitops_change_set_id/transition",
            post(transition_gitops_change_set),
        )
        .route(
            "/api/gitops-change-sets/:gitops_change_set_id/resolve-base-revision",
            post(resolve_gitops_base_revision),
        )
        .route(
            "/api/gitops-change-sets/:gitops_change_set_id/delivery-plan",
            post(prepare_gitops_change_set_delivery),
        )
        .route(
            "/api/gitops-change-sets/:gitops_change_set_id/delivery/authorize",
            post(authorize_gitops_change_set_delivery),
        )
        .route(
            "/api/gitops-change-sets/:gitops_change_set_id/delivery/preflight",
            post(preflight_gitops_change_set_delivery),
        )
        .route(
            "/api/gitops-change-sets/:gitops_change_set_id/delivery/execute",
            post(execute_gitops_change_set_delivery),
        )
        .route(
            "/api/gitops-change-sets/:gitops_change_set_id/delivery/observe",
            post(observe_gitops_change_set_delivery),
        )
}
