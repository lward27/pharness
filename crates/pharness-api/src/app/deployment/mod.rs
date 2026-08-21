use super::AppState;
use axum::routing::{get, post};
use axum::Router;

pub(super) mod contracts;
pub(super) mod execution;
pub(super) mod intents;

use contracts::{
    create_deployment_contract, get_deployment_contract, list_deployment_contracts,
    transition_deployment_contract,
};
use execution::{execute_deployment_intent, preflight_deployment_intent};
use intents::{
    attach_deployment_intent_evidence, create_deployment_intent_from_pipeline_intent,
    create_deployment_intent_trusted_envelope, get_deployment_intent, list_deployment_intents,
    transition_deployment_intent,
};

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
