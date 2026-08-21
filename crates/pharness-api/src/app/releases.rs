use super::*;
use axum::routing::{get, post};
use axum::Router;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/releases", get(list_releases))
        .route(
            "/api/releases/from-deployment-intent",
            post(create_release_from_deployment_intent),
        )
        .route("/api/releases/:release_id", get(get_release))
        .route(
            "/api/releases/:release_id/transition",
            post(transition_release),
        )
        .route(
            "/api/releases/:release_id/evidence",
            post(attach_release_evidence),
        )
        .route("/api/releases/:release_id/verify", post(verify_release))
        .route("/api/registry-evidence", get(list_registry_evidence))
        .route(
            "/api/registry-evidence/from-release",
            post(create_registry_evidence_from_release),
        )
        .route(
            "/api/registry-evidence/from-registry-inspection",
            post(create_registry_evidence_from_registry_inspection),
        )
        .route(
            "/api/registry-evidence/:evidence_id",
            get(get_registry_evidence),
        )
        .route(
            "/api/registry-evidence/:evidence_id/transition",
            post(transition_registry_evidence),
        )
}
