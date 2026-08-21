use super::*;
use axum::routing::{get, post};
use axum::Router;

pub(super) fn router(state: AppState) -> Router<AppState> {
    runs::internal_router()
        .route(
            "/api/internal/pipeline-intents/:pipeline_intent_id/execution-outcome",
            post(internal_pipeline_intent_execution_outcome),
        )
        .route(
            "/api/internal/deployment-intents/:deployment_intent_id/argo-sync-context",
            get(internal_argo_sync_context),
        )
        .route(
            "/api/internal/deployment-intents/:deployment_intent_id/argo-sync-control",
            get(internal_argo_sync_control),
        )
        .route(
            "/api/internal/deployment-intents/:deployment_intent_id/argo-sync-outcome",
            post(internal_argo_sync_outcome),
        )
        .route(
            "/api/internal/change-sets/:change_set_id/git-delivery-context",
            get(internal_git_delivery_context),
        )
        .route(
            "/api/internal/change-sets/:change_set_id/git-delivery-outcome",
            post(internal_git_delivery_outcome),
        )
        .route(
            "/api/internal/change-sets/:change_set_id/git-delivery-observation-context",
            get(internal_git_delivery_observation_context),
        )
        .route(
            "/api/internal/change-sets/:change_set_id/git-delivery-observation-outcome",
            post(internal_git_delivery_observation_outcome),
        )
        .route(
            "/api/internal/gitops-change-sets/:gitops_change_set_id/base-revision-context",
            get(internal_gitops_base_revision_context),
        )
        .route(
            "/api/internal/gitops-change-sets/:gitops_change_set_id/base-revision-outcome",
            post(internal_gitops_base_revision_outcome),
        )
        .route(
            "/api/internal/gitops-change-sets/:gitops_change_set_id/delivery-context",
            get(internal_gitops_delivery_context),
        )
        .route(
            "/api/internal/gitops-change-sets/:gitops_change_set_id/delivery-outcome",
            post(internal_gitops_delivery_outcome),
        )
        .route(
            "/api/internal/gitops-change-sets/:gitops_change_set_id/delivery-observation-context",
            get(internal_gitops_delivery_observation_context),
        )
        .route(
            "/api/internal/gitops-change-sets/:gitops_change_set_id/delivery-observation-outcome",
            post(internal_gitops_delivery_observation_outcome),
        )
        .route_layer(middleware::from_fn_with_state(state, require_worker_token))
}
