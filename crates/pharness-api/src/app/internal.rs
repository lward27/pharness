use super::auth::require_worker_token;
use super::deployment::execution::{
    internal_argo_sync_context as internal_deployment_argo_sync_context,
    internal_argo_sync_control as internal_deployment_argo_sync_control,
    internal_argo_sync_outcome as internal_deployment_argo_sync_outcome, InternalArgoSyncQuery,
};
use super::gitops::delivery::{
    internal_gitops_base_revision_context, internal_gitops_base_revision_outcome,
    internal_gitops_delivery_context as internal_standard_gitops_delivery_context,
    internal_gitops_delivery_observation_context as internal_standard_gitops_delivery_observation_context,
    internal_gitops_delivery_observation_outcome as internal_standard_gitops_delivery_observation_outcome,
    internal_gitops_delivery_outcome as internal_standard_gitops_delivery_outcome,
    InternalGitOpsDeliveryQuery,
};
use super::pipeline::execution::internal_pipeline_intent_execution_outcome;
use super::products::{
    internal_onboarding_contract_validation_context,
    internal_onboarding_contract_validation_outcome, internal_onboarding_patch_context,
    internal_onboarding_patch_outcome, internal_repository_discovery_context,
    internal_repository_discovery_outcome, internal_repository_readiness_context,
    internal_repository_readiness_outcome,
};
use super::repo_mode::{
    internal_source_delivery_context, internal_source_delivery_observation_context,
    internal_source_delivery_observation_outcome, internal_source_delivery_writer_outcome,
};
use super::runs;
use super::source::git_delivery::{
    internal_git_delivery_context, internal_git_delivery_observation_context,
    internal_git_delivery_observation_outcome, internal_git_delivery_outcome,
};
use super::work_items::rollback::{
    internal_rollback_argo_sync_context, internal_rollback_argo_sync_control,
    internal_rollback_argo_sync_outcome, internal_rollback_delivery_context,
    internal_rollback_delivery_observation_context, internal_rollback_delivery_observation_outcome,
    internal_rollback_delivery_outcome,
};
use super::{ApiError, AppState};
use crate::dto::{
    ArgoSyncContextResponse, ArgoSyncControlResponse, ArgoSyncOutcomeRequest, ArtifactResponse,
    GitOpsDeliveryContextResponse, GitOpsDeliveryObservationContextResponse,
    GitOpsDeliveryObservationOutcomeRequest, GitOpsDeliveryOutcomeRequest,
};
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{middleware, Json, Router};

async fn internal_argo_sync_context(
    State(state): State<AppState>,
    Path(resource_id): Path<String>,
    Query(query): Query<InternalArgoSyncQuery>,
) -> Result<Json<ArgoSyncContextResponse>, ApiError> {
    if resource_id.starts_with("rollback_") {
        internal_rollback_argo_sync_context(&state, &resource_id, &query.execution_id).await
    } else {
        internal_deployment_argo_sync_context(State(state), Path(resource_id), Query(query)).await
    }
}

async fn internal_argo_sync_control(
    State(state): State<AppState>,
    Path(resource_id): Path<String>,
    Query(query): Query<InternalArgoSyncQuery>,
) -> Result<Json<ArgoSyncControlResponse>, ApiError> {
    if resource_id.starts_with("rollback_") {
        internal_rollback_argo_sync_control(&state, &resource_id).await
    } else {
        internal_deployment_argo_sync_control(State(state), Path(resource_id), Query(query)).await
    }
}

pub(in crate::app) async fn internal_argo_sync_outcome(
    State(state): State<AppState>,
    Path(resource_id): Path<String>,
    Json(request): Json<ArgoSyncOutcomeRequest>,
) -> Result<Json<ArtifactResponse>, ApiError> {
    if resource_id.starts_with("rollback_") {
        internal_rollback_argo_sync_outcome(&state, &resource_id, request).await
    } else {
        internal_deployment_argo_sync_outcome(State(state), Path(resource_id), Json(request)).await
    }
}

async fn internal_gitops_delivery_context(
    State(state): State<AppState>,
    Path(resource_id): Path<String>,
    Query(query): Query<InternalGitOpsDeliveryQuery>,
) -> Result<Json<GitOpsDeliveryContextResponse>, ApiError> {
    if resource_id.starts_with("rollback_") {
        internal_rollback_delivery_context(&state, &resource_id, &query.execution_id).await
    } else {
        internal_standard_gitops_delivery_context(State(state), Path(resource_id), Query(query))
            .await
    }
}

pub(in crate::app) async fn internal_gitops_delivery_outcome(
    State(state): State<AppState>,
    Path(resource_id): Path<String>,
    Json(request): Json<GitOpsDeliveryOutcomeRequest>,
) -> Result<Json<ArtifactResponse>, ApiError> {
    if resource_id.starts_with("rollback_") {
        internal_rollback_delivery_outcome(&state, &resource_id, request).await
    } else {
        internal_standard_gitops_delivery_outcome(State(state), Path(resource_id), Json(request))
            .await
    }
}

async fn internal_gitops_delivery_observation_context(
    State(state): State<AppState>,
    Path(resource_id): Path<String>,
    Query(query): Query<InternalGitOpsDeliveryQuery>,
) -> Result<Json<GitOpsDeliveryObservationContextResponse>, ApiError> {
    if resource_id.starts_with("rollback_") {
        internal_rollback_delivery_observation_context(&state, &resource_id, &query.execution_id)
            .await
    } else {
        internal_standard_gitops_delivery_observation_context(
            State(state),
            Path(resource_id),
            Query(query),
        )
        .await
    }
}

pub(in crate::app) async fn internal_gitops_delivery_observation_outcome(
    State(state): State<AppState>,
    Path(resource_id): Path<String>,
    Json(request): Json<GitOpsDeliveryObservationOutcomeRequest>,
) -> Result<Json<ArtifactResponse>, ApiError> {
    if resource_id.starts_with("rollback_") {
        internal_rollback_delivery_observation_outcome(&state, &resource_id, request).await
    } else {
        internal_standard_gitops_delivery_observation_outcome(
            State(state),
            Path(resource_id),
            Json(request),
        )
        .await
    }
}

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
            "/api/internal/source-delivery-intents/:source_delivery_intent_id/context",
            get(internal_source_delivery_context),
        )
        .route(
            "/api/internal/source-delivery-intents/:source_delivery_intent_id/writer-outcome",
            post(internal_source_delivery_writer_outcome),
        )
        .route(
            "/api/internal/source-delivery-intents/:source_delivery_intent_id/observation-context",
            get(internal_source_delivery_observation_context),
        )
        .route(
            "/api/internal/source-delivery-intents/:source_delivery_intent_id/observation-outcome",
            post(internal_source_delivery_observation_outcome),
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
        .route(
            "/api/internal/repository-discoveries/:discovery_id/context",
            get(internal_repository_discovery_context),
        )
        .route(
            "/api/internal/repository-discoveries/:discovery_id/outcome",
            post(internal_repository_discovery_outcome),
        )
        .route(
            "/api/internal/repository-onboardings/:onboarding_id/patch-context",
            get(internal_onboarding_patch_context),
        )
        .route(
            "/api/internal/repository-onboardings/:onboarding_id/patch-outcome",
            post(internal_onboarding_patch_outcome),
        )
        .route(
            "/api/internal/repository-onboardings/:onboarding_id/contract-validation-context",
            get(internal_onboarding_contract_validation_context),
        )
        .route(
            "/api/internal/repository-onboardings/:onboarding_id/contract-validation-outcome",
            post(internal_onboarding_contract_validation_outcome),
        )
        .route(
            "/api/internal/repository-readiness-preparations/:preparation_id/context",
            get(internal_repository_readiness_context),
        )
        .route(
            "/api/internal/repository-readiness-preparations/:preparation_id/outcome",
            post(internal_repository_readiness_outcome),
        )
        .route_layer(middleware::from_fn_with_state(state, require_worker_token))
}
