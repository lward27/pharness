mod model;
mod onboarding;
mod onboarding_execution;
mod onboarding_policy;
mod onboarding_state;
mod readiness;
mod registration;
mod types;

#[cfg(test)]
mod tests;

pub(in crate::app) use self::onboarding::finalize_repository_onboarding_proposer_run;
pub(in crate::app) use self::onboarding_execution::{
    internal_onboarding_contract_validation_context,
    internal_onboarding_contract_validation_outcome, internal_onboarding_patch_context,
    internal_onboarding_patch_outcome, internal_repository_discovery_context,
    internal_repository_discovery_outcome,
};
pub(in crate::app) use self::onboarding_state::onboarding_operator_projection;
pub(in crate::app) use self::readiness::{
    internal_repository_readiness_context, internal_repository_readiness_outcome,
};
#[cfg(test)]
pub(in crate::app) use self::types::{
    InternalOnboardingContractValidationQuery, OnboardingContractValidationOutcomeRequest,
    OnboardingPatchOutcomeRequest,
};
pub(in crate::app) use crate::app::repository_readiness::ensure_repo_mode_enabled;

use self::model::{
    apply_product_model_change, create_product, get_organization, get_product, get_product_model,
    get_product_model_snapshot, list_agent_profiles, list_product_services, list_products,
    organization_overview, preflight_product_model_change, update_product,
};
use self::onboarding::{
    create_repository_onboarding, execute_repository_onboarding_action, get_repository_onboarding,
    get_repository_onboarding_flow, put_repository_onboarding_proposal,
};
use self::readiness::{create_repository_readiness_assessment, get_repository_readiness};
use self::registration::{
    get_repository, list_product_repositories, preflight_repository_registration,
    register_repository,
};
use crate::app::AppState;
use axum::routing::{get, post};
use axum::Router;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/organization", get(get_organization))
        .route("/api/organization/overview", get(organization_overview))
        .route("/api/products", get(list_products).post(create_product))
        .route(
            "/api/products/:product_id",
            get(get_product).patch(update_product),
        )
        .route(
            "/api/products/:product_id/model-snapshots/:snapshot_id",
            get(get_product_model_snapshot),
        )
        .route("/api/products/:product_id/model", get(get_product_model))
        .route(
            "/api/products/:product_id/model-changes/preflight",
            post(preflight_product_model_change),
        )
        .route(
            "/api/products/:product_id/model-changes",
            post(apply_product_model_change),
        )
        .route(
            "/api/products/:product_id/services",
            get(list_product_services),
        )
        .route(
            "/api/products/:product_id/repositories/preflight",
            post(preflight_repository_registration),
        )
        .route(
            "/api/products/:product_id/repositories",
            get(list_product_repositories).post(register_repository),
        )
        .route("/api/repositories/:repository_id", get(get_repository))
        .route(
            "/api/repositories/:repository_id/readiness",
            get(get_repository_readiness),
        )
        .route(
            "/api/repositories/:repository_id/readiness-assessments",
            post(create_repository_readiness_assessment),
        )
        .route(
            "/api/repositories/:repository_id/onboardings",
            post(create_repository_onboarding),
        )
        .route(
            "/api/repository-onboardings/:onboarding_id",
            get(get_repository_onboarding),
        )
        .route(
            "/api/repository-onboardings/:onboarding_id/flow",
            get(get_repository_onboarding_flow),
        )
        .route(
            "/api/repository-onboardings/:onboarding_id/proposal",
            axum::routing::put(put_repository_onboarding_proposal),
        )
        .route(
            "/api/repository-onboardings/:onboarding_id/actions/:action_id/execute",
            post(execute_repository_onboarding_action),
        )
        .route("/api/agent-profiles", get(list_agent_profiles))
}
