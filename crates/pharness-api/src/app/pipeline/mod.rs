use super::AppState;
use axum::routing::{get, post};
use axum::Router;

pub(super) mod contracts;
pub(super) mod evidence;
pub(super) mod execution;
pub(super) mod handoff;
pub(super) mod intents;
pub(super) mod readiness;
pub(super) mod state;

use contracts::{
    create_pipeline_contract, get_pipeline_contract, list_pipeline_contracts,
    replace_pipeline_contract, transition_pipeline_contract,
};
use execution::execute_pipeline_intent;
use intents::{
    attach_pipeline_intent_evidence, create_gitops_update_plan,
    create_pipeline_intent_from_change_set, create_pipeline_intent_trusted_envelope,
    get_pipeline_intent, list_pipeline_intents, transition_pipeline_intent,
};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/pipeline-intents", get(list_pipeline_intents))
        .route(
            "/api/pipeline-contracts",
            get(list_pipeline_contracts).post(create_pipeline_contract),
        )
        .route(
            "/api/pipeline-contracts/:pipeline_contract_id",
            get(get_pipeline_contract),
        )
        .route(
            "/api/pipeline-contracts/:pipeline_contract_id/transition",
            post(transition_pipeline_contract),
        )
        .route(
            "/api/pipeline-contracts/:pipeline_contract_id/replace",
            post(replace_pipeline_contract),
        )
        .route(
            "/api/pipeline-intents/from-change-set",
            post(create_pipeline_intent_from_change_set),
        )
        .route(
            "/api/pipeline-intents/:pipeline_intent_id",
            get(get_pipeline_intent),
        )
        .route(
            "/api/pipeline-intents/:pipeline_intent_id/transition",
            post(transition_pipeline_intent),
        )
        .route(
            "/api/pipeline-intents/:pipeline_intent_id/evidence",
            post(attach_pipeline_intent_evidence),
        )
        .route(
            "/api/pipeline-intents/:pipeline_intent_id/trusted-envelope",
            post(create_pipeline_intent_trusted_envelope),
        )
        .route(
            "/api/pipeline-intents/:pipeline_intent_id/execute",
            post(execute_pipeline_intent),
        )
        .route(
            "/api/pipeline-intents/:pipeline_intent_id/gitops-update-plan",
            post(create_gitops_update_plan),
        )
}
