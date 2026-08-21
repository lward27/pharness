use super::*;
use axum::routing::{get, post};
use axum::Router;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/deployment-contracts",
            get(list_deployment_contracts).post(create_deployment_contract),
        )
        .route(
            "/api/deployment-contracts/:deployment_contract_id",
            get(get_deployment_contract),
        )
        .route(
            "/api/deployment-contracts/:deployment_contract_id/transition",
            post(transition_deployment_contract),
        )
        .route("/api/deployment-intents", get(list_deployment_intents))
        .route(
            "/api/deployment-intents/from-pipeline-intent",
            post(create_deployment_intent_from_pipeline_intent),
        )
        .route(
            "/api/deployment-intents/:deployment_intent_id",
            get(get_deployment_intent),
        )
        .route(
            "/api/deployment-intents/:deployment_intent_id/transition",
            post(transition_deployment_intent),
        )
        .route(
            "/api/deployment-intents/:deployment_intent_id/evidence",
            post(attach_deployment_intent_evidence),
        )
        .route(
            "/api/deployment-intents/:deployment_intent_id/trusted-envelope",
            post(create_deployment_intent_trusted_envelope),
        )
        .route(
            "/api/deployment-intents/:deployment_intent_id/preflight",
            post(preflight_deployment_intent),
        )
        .route(
            "/api/deployment-intents/:deployment_intent_id/execute",
            post(execute_deployment_intent),
        )
}
