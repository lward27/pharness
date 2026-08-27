use super::hashing::canonical_material_hash;
use super::identifiers::{is_git_sha, new_prefixed_id};
use super::repo_mode::current_readiness_mismatches;
use super::{ApiError, AppState};
use crate::dispatch::{
    OnboardingContractValidationRequest, OnboardingPatchRequest, RepositoryDiscoveryRequest,
    RepositoryReadinessExecutionRequest, SourceDeliveryExecutionRequest,
    SourceDeliveryObservationRequest,
};
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pharness_core::{
    AgentEvent, EventId, EventKind, RunBudgetConsumption, RunId, RunScope, SessionId,
};
use pharness_store::{
    ApplyProductModelRevision, ApproveRepositoryOnboardingProposal,
    ApprovedOnboardingProductModelChange, ApprovedOnboardingService,
    CompleteSubjectEnvironmentPreparation, CreateArtifact, CreateProductAggregate,
    CreateRepositoryContractVersion, CreateRepositoryOnboarding,
    CreateRepositoryOnboardingProposal, CreateRepositoryReadinessAssessment, CreateRun,
    CreateSession, CreateSubjectEnvironmentPreparation, CreateSubjectWorkspace,
    ProductModelBindingRevision, ProductModelServiceRevision, RegisterRepositoryAggregate,
    RegisteredRepositoryAggregate, RepositoryBindingScope, StoredProduct,
    StoredProductModelSnapshot, StoredRepository, StoredRepositoryBinding, StoredRepositoryDraft,
    StoredRepositoryOnboarding, StoredRepositoryOnboardingProposal, StoredService,
    UpdateProductAggregate,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

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

#[derive(Debug, Serialize)]
struct OrganizationResponse {
    id: String,
    organization_key: String,
    display_name: String,
    repo_mode_v1_enabled: bool,
    repo_mode_v1_ui_enabled: bool,
}

#[derive(Debug, Deserialize)]
struct CreateProductRequest {
    display_name: String,
    description: String,
    owner_principal: String,
    actor: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct UpdateProductRequest {
    display_name: Option<String>,
    description: Option<String>,
    owner_principal: Option<String>,
    actor: String,
    reason: String,
    state_hash: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ProductModelServiceInput {
    #[serde(default)]
    id: Option<String>,
    service_key: String,
    display_name: String,
    description: String,
    #[serde(default = "default_active_status")]
    status: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ProductModelScopeInput {
    path_glob: String,
    role: String,
    #[serde(default)]
    service_id: Option<String>,
    #[serde(default)]
    service_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ProductModelBindingInput {
    repository_id: String,
    #[serde(default = "default_active_status")]
    status: String,
    scopes: Vec<ProductModelScopeInput>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProductModelChangePreflightRequest {
    services: Vec<ProductModelServiceInput>,
    bindings: Vec<ProductModelBindingInput>,
    actor: String,
    reason: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct NormalizedProductModelService {
    id: String,
    service_key: String,
    display_name: String,
    description: String,
    status: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct NormalizedProductModelScope {
    id: String,
    path_glob: String,
    role: String,
    service_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct NormalizedProductModelBinding {
    binding_id: String,
    repository_id: String,
    revision_id: String,
    status: String,
    scopes: Vec<NormalizedProductModelScope>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct NormalizedProductModelChange {
    services: Vec<NormalizedProductModelService>,
    bindings: Vec<NormalizedProductModelBinding>,
}

#[derive(Debug, Serialize)]
struct ProductModelChangePreflightResponse {
    product_id: String,
    state_hash: String,
    normalized_change: NormalizedProductModelChange,
    resulting_snapshot: Value,
    resulting_snapshot_hash: String,
    preflight_hash: String,
    predicted_mutations: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ApplyProductModelChangeRequest {
    normalized_change: NormalizedProductModelChange,
    state_hash: String,
    preflight_hash: String,
    actor: String,
    reason: String,
}

fn default_active_status() -> String {
    "active".into()
}

#[derive(Debug, Clone, Deserialize)]
struct RepositoryRegistrationPreflightRequest {
    repository_url: String,
    source_commit: String,
}

#[derive(Debug, Deserialize)]
struct RegisterRepositoryRequest {
    repository_url: String,
    source_commit: String,
    preflight_hash: String,
    actor: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct CreateRepositoryOnboardingRequest {
    product_id: String,
    source_commit: String,
    actor: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct RepositoryReadinessQuery {
    source_commit: String,
}

#[derive(Debug, Deserialize)]
struct CreateRepositoryReadinessRequest {
    source_commit: String,
    actor: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct ExecuteRepositoryOnboardingActionRequest {
    actor: String,
    reason: String,
    state_hash: String,
}

#[derive(Debug, Deserialize)]
struct PutRepositoryOnboardingProposalRequest {
    proposal: pharness_core::RepositoryOnboardingProposal,
    actor: String,
    reason: String,
    state_hash: String,
}

#[derive(Debug, Serialize)]
pub(in crate::app) struct RepositoryDiscoveryContextResponse {
    discovery_id: String,
    onboarding_id: String,
    repository_id: String,
    provider: String,
    canonical_url: String,
    default_branch: String,
    source_commit: String,
    limits: pharness_core::RepositoryDiscoveryLimits,
}

#[derive(Debug, Deserialize)]
pub(in crate::app) struct RepositoryDiscoveryOutcomeRequest {
    status: String,
    #[serde(default)]
    discovery: Option<pharness_core::RepositoryDiscovery>,
    #[serde(default)]
    error_code: Option<String>,
    #[serde(default)]
    error_summary: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(in crate::app) struct InternalOnboardingPatchQuery {
    execution_id: String,
}

#[derive(Debug, Serialize)]
pub(in crate::app) struct OnboardingPatchContextResponse {
    onboarding_id: String,
    execution_id: String,
    repository_id: String,
    provider: String,
    canonical_url: String,
    default_branch: String,
    source_commit: String,
    proposal_id: String,
    proposal_hash: String,
    candidate_contract: Value,
    instructions: String,
    remove_alias: bool,
}

#[derive(Debug, Deserialize)]
pub(in crate::app) struct OnboardingPatchOutcomeRequest {
    status: String,
    #[serde(default)]
    patch: Option<String>,
    #[serde(default)]
    patch_hash: Option<String>,
    #[serde(default)]
    changed_paths: Vec<String>,
    #[serde(default)]
    error_code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(in crate::app) struct InternalOnboardingContractValidationQuery {
    execution_id: String,
}

#[derive(Debug, Serialize)]
pub(in crate::app) struct OnboardingContractValidationContextResponse {
    onboarding_id: String,
    execution_id: String,
    repository_id: String,
    provider: String,
    canonical_url: String,
    source_commit: String,
    proposal_id: String,
    proposal_hash: String,
    expected_contract: Value,
}

#[derive(Debug, Deserialize)]
pub(in crate::app) struct OnboardingContractValidationOutcomeRequest {
    status: String,
    #[serde(default)]
    contract: Option<Value>,
    #[serde(default)]
    contract_content_hash: Option<String>,
    #[serde(default)]
    contract_source: Option<String>,
    #[serde(default)]
    warnings: Vec<String>,
    #[serde(default)]
    error_code: Option<String>,
}

#[derive(Debug, Serialize)]
pub(in crate::app) struct RepositoryReadinessPreparationContextResponse {
    preparation_id: String,
    workspace_id: String,
    repository_id: String,
    provider: String,
    canonical_url: String,
    default_branch: String,
    source_commit: String,
    contract_version_id: String,
    contract_content_hash: String,
    contract: Value,
    environment_profile_id: String,
}

#[derive(Debug, Deserialize)]
pub(in crate::app) struct RepositoryReadinessPreparationOutcomeRequest {
    status: String,
    #[serde(default)]
    resolved_commit: Option<String>,
    #[serde(default)]
    repository_contract: Option<Value>,
    #[serde(default)]
    repository_contract_hash: Option<String>,
    #[serde(default)]
    environment_snapshot: Option<Value>,
    #[serde(default)]
    snapshot_signature: Option<String>,
    #[serde(default)]
    acceptance_results: Value,
    #[serde(default)]
    logs: Value,
    #[serde(default)]
    error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RepositoryRegistrationPreflightResponse {
    product_id: String,
    provider: String,
    provider_repository_id: String,
    external_id: String,
    canonical_url: String,
    default_branch: String,
    source_commit: String,
    commit_verified: bool,
    already_registered_globally: bool,
    already_bound_to_product: bool,
    predicted_mutations: Vec<String>,
    blockers: Vec<String>,
    preflight_hash: String,
}

#[derive(Debug, Serialize)]
struct ProductResponse {
    id: String,
    organization_id: String,
    product_key: String,
    display_name: String,
    description: String,
    owner_principal: String,
    state_version: u64,
    state_hash: String,
    current_model_snapshot_id: String,
    current_model_snapshot_hash: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct ProductsResponse {
    products: Vec<ProductResponse>,
    count: usize,
}

#[derive(Debug, Serialize)]
struct ProductModelSnapshotResponse {
    id: String,
    product_id: String,
    version: u64,
    model: Value,
    content_hash: String,
    created_by: String,
    creation_reason: String,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct ServiceResponse {
    id: String,
    product_id: String,
    service_key: String,
    display_name: String,
    description: String,
    status: String,
}

#[derive(Debug, Clone, Serialize)]
struct RepositoryResponse {
    id: String,
    provider: String,
    provider_repository_id: String,
    external_id: String,
    canonical_url: String,
    default_branch: String,
    registered_commit: String,
    state_version: u64,
    binding_id: Option<String>,
    binding_revision_id: Option<String>,
    onboarding_id: Option<String>,
    onboarding_status: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct RepositoriesResponse {
    repositories: Vec<RepositoryResponse>,
    count: usize,
}

#[derive(Debug, Serialize)]
struct RepositoryOnboardingResponse {
    id: String,
    product_id: String,
    repository_id: String,
    binding_id: String,
    onboarding_kind: String,
    status: String,
    registered_commit: String,
    resolved_commit: Option<String>,
    current_discovery_id: Option<String>,
    current_proposal_revision: u64,
    approved_proposal_hash: Option<String>,
    source_delivery_intent_id: Option<String>,
    contract_version_id: Option<String>,
    readiness_assessment_id: Option<String>,
    proposer_run_id: Option<String>,
    proposer_profile_hash: Option<String>,
    proposer_stop_reason: Option<String>,
    patch_execution_id: Option<String>,
    patch_artifact_id: Option<String>,
    patch_hash: Option<String>,
    validation_execution_id: Option<String>,
    validation_stop_reason: Option<String>,
    state_version: u64,
    state_hash: String,
    blockers: Vec<Value>,
    actions: Vec<RepositoryOnboardingActionResponse>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
struct RepositoryOnboardingActionResponse {
    id: String,
    lifecycle_stage: String,
    resource: Value,
    status: String,
    effect_class: String,
    external_effect_summary: String,
    approval_requirements: Vec<String>,
    expected_result: String,
    requires_confirmation: bool,
    blockers: Vec<String>,
    state_hash: String,
}

#[derive(Debug, Deserialize)]
struct GitHubRepositoryResponse {
    id: u64,
    full_name: String,
    html_url: String,
    default_branch: String,
}

#[derive(Debug, Deserialize)]
struct GitHubCommitResponse {
    sha: String,
}

async fn get_organization(
    State(state): State<AppState>,
) -> Result<Json<OrganizationResponse>, ApiError> {
    let organization = state
        .store
        .get_organization(&state.repo_mode.organization.id)
        .await?;
    Ok(Json(OrganizationResponse {
        id: organization
            .as_ref()
            .map(|value| value.id.clone())
            .unwrap_or_else(|| state.repo_mode.organization.id.clone()),
        organization_key: organization
            .as_ref()
            .map(|value| value.organization_key.clone())
            .unwrap_or_else(|| state.repo_mode.organization.organization_key.clone()),
        display_name: organization
            .as_ref()
            .map(|value| value.display_name.clone())
            .unwrap_or_else(|| state.repo_mode.organization.display_name.clone()),
        repo_mode_v1_enabled: state.repo_mode.enabled,
        repo_mode_v1_ui_enabled: state.repo_mode.ui_enabled,
    }))
}

async fn list_agent_profiles(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let config = state.worker.config_json();
    let model = config
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unconfigured");
    let profiles =
        pharness_core::compiled_agent_profiles(model, pharness_runhost::SYSTEM_PROMPT_VERSION);
    Ok(Json(
        json!({"agent_profiles": profiles, "count": profiles.len()}),
    ))
}

async fn organization_overview(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let organization = state
        .store
        .get_organization(&state.repo_mode.organization.id)
        .await?
        .unwrap_or_else(|| pharness_store::StoredOrganization {
            id: state.repo_mode.organization.id.clone(),
            organization_key: state.repo_mode.organization.organization_key.clone(),
            display_name: state.repo_mode.organization.display_name.clone(),
            created_at: String::new(),
            updated_at: String::new(),
        });
    Ok(Json(
        super::operator_experience::organization_overview_value(&state, &organization).await?,
    ))
}

async fn list_products(State(state): State<AppState>) -> Result<Json<ProductsResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let products = state
        .store
        .list_products(&state.repo_mode.organization.id)
        .await?;
    let mut responses = Vec::with_capacity(products.len());
    for product in products {
        responses.push(product_response(&state, product).await?);
    }
    let count = responses.len();
    Ok(Json(ProductsResponse {
        products: responses,
        count,
    }))
}

async fn create_product(
    State(state): State<AppState>,
    Json(request): Json<CreateProductRequest>,
) -> Result<Json<ProductResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    validate_required(&request.display_name, "display_name", 120)?;
    validate_required(&request.description, "description", 2_000)?;
    validate_required(&request.owner_principal, "owner_principal", 200)?;
    validate_required(&request.actor, "actor", 200)?;
    validate_required(&request.reason, "reason", 1_000)?;
    state
        .store
        .ensure_bootstrap_organization(&state.repo_mode.organization)
        .await?;

    let product_id = new_prefixed_id("prod");
    let snapshot_id = new_prefixed_id("pmodel");
    let product_key = normalize_key(&request.display_name)?;
    let model = product_model_json(
        &product_id,
        &state.repo_mode.organization.id,
        &product_key,
        request.display_name.trim(),
        request.description.trim(),
        request.owner_principal.trim(),
        &[],
        &[],
        &[],
    );
    let snapshot_hash = canonical_material_hash(&model)?;
    let product = state
        .store
        .create_product(CreateProductAggregate {
            id: product_id,
            organization_id: state.repo_mode.organization.id.clone(),
            product_key,
            display_name: request.display_name.trim().into(),
            description: request.description.trim().into(),
            owner_principal: request.owner_principal.trim().into(),
            snapshot_id,
            snapshot_json: model,
            snapshot_hash,
            actor: request.actor.trim().into(),
            reason: request.reason.trim().into(),
        })
        .await?;
    Ok(Json(product_response(&state, product).await?))
}

async fn get_product(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
) -> Result<Json<ProductResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let product = find_product(&state, &product_id).await?;
    Ok(Json(product_response(&state, product).await?))
}

async fn update_product(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
    Json(request): Json<UpdateProductRequest>,
) -> Result<Json<ProductResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    validate_required(&request.actor, "actor", 200)?;
    validate_required(&request.reason, "reason", 1_000)?;
    let current = find_product(&state, &product_id).await?;
    let current_response = product_response(&state, current.clone()).await?;
    if request.state_hash != current_response.state_hash {
        return Err(ApiError::conflict(
            "product changed after the operator preview; refresh and retry",
        ));
    }
    let display_name = request
        .display_name
        .as_deref()
        .unwrap_or(&current.display_name)
        .trim()
        .to_string();
    let description = request
        .description
        .as_deref()
        .unwrap_or(&current.description)
        .trim()
        .to_string();
    let owner_principal = request
        .owner_principal
        .as_deref()
        .unwrap_or(&current.owner_principal)
        .trim()
        .to_string();
    validate_required(&display_name, "display_name", 120)?;
    validate_required(&description, "description", 2_000)?;
    validate_required(&owner_principal, "owner_principal", 200)?;
    let product_key = normalize_key(&display_name)?;
    let repositories = state.store.list_product_repositories(&product_id).await?;
    let services = state.store.list_product_services(&product_id).await?;
    let bindings = state
        .store
        .list_product_repository_bindings(&product_id)
        .await?;
    let model = product_model_json(
        &product_id,
        &current.organization_id,
        &product_key,
        &display_name,
        &description,
        &owner_principal,
        &services,
        &repositories,
        &bindings,
    );
    let updated = state
        .store
        .update_product(UpdateProductAggregate {
            id: product_id,
            expected_state_version: current.state_version,
            product_key,
            display_name,
            description,
            owner_principal,
            snapshot_id: new_prefixed_id("pmodel"),
            snapshot_hash: canonical_material_hash(&model)?,
            snapshot_json: model,
            actor: request.actor.trim().into(),
            reason: request.reason.trim().into(),
        })
        .await?;
    Ok(Json(product_response(&state, updated).await?))
}

async fn get_product_model_snapshot(
    State(state): State<AppState>,
    Path((product_id, snapshot_id)): Path<(String, String)>,
) -> Result<Json<ProductModelSnapshotResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let snapshot = state
        .store
        .get_product_model_snapshot(&snapshot_id)
        .await?
        .ok_or_else(|| ApiError::not_found("product_model_snapshot", &snapshot_id))?;
    if snapshot.product_id != product_id {
        return Err(ApiError::not_found("product_model_snapshot", &snapshot_id));
    }
    Ok(Json(snapshot_response(snapshot)))
}

async fn get_product_model(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let product = find_product(&state, &product_id).await?;
    let snapshot = state
        .store
        .get_product_model_snapshot(&product.current_model_snapshot_id)
        .await?
        .ok_or_else(|| {
            ApiError::not_found("product_model_snapshot", &product.current_model_snapshot_id)
        })?;
    let services = state.store.list_product_services(&product_id).await?;
    let repositories = state.store.list_product_repositories(&product_id).await?;
    let bindings = state
        .store
        .list_product_repository_bindings(&product_id)
        .await?;
    let mut binding_models = Vec::with_capacity(bindings.len());
    for binding in bindings {
        let revision = state
            .store
            .get_repository_binding_revision(&binding.current_revision_id)
            .await?
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "binding {} references missing revision {}",
                    binding.id, binding.current_revision_id
                ))
            })?;
        let typed_scopes = state
            .store
            .list_repository_binding_scopes(&revision.id)
            .await?;
        binding_models.push(json!({
            "id": binding.id,
            "repository_id": binding.repository_id,
            "status": binding.status,
            "current_revision": revision,
            "typed_scopes": typed_scopes,
            "scope_model": if typed_scopes.is_empty() { "legacy" } else { "typed" },
        }));
    }
    Ok(Json(json!({
        "product": product_response(&state, product).await?,
        "snapshot": snapshot_response(snapshot),
        "services": services,
        "repositories": repositories,
        "bindings": binding_models,
        "database_generation_id": state.store.get_database_generation().await?.map(|value| value.id),
    })))
}

async fn preflight_product_model_change(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
    Json(request): Json<ProductModelChangePreflightRequest>,
) -> Result<Json<ProductModelChangePreflightResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    validate_required(&request.actor, "actor", 200)?;
    validate_required(&request.reason, "reason", 1_000)?;
    let product = find_product(&state, &product_id).await?;
    let response =
        build_product_model_change_preflight(&state, &product, request.services, request.bindings)
            .await?;
    Ok(Json(response))
}

async fn apply_product_model_change(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
    Json(request): Json<ApplyProductModelChangeRequest>,
) -> Result<Json<ProductModelSnapshotResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    validate_required(&request.actor, "actor", 200)?;
    validate_required(&request.reason, "reason", 1_000)?;
    let product = find_product(&state, &product_id).await?;
    let current = product_response(&state, product.clone()).await?;
    if request.state_hash != current.state_hash {
        return Err(ApiError::conflict(
            "product changed after the topology preview; refresh and review again",
        ));
    }
    validate_normalized_product_model_change(&state, &product, &request.normalized_change).await?;
    let snapshot =
        product_model_v1alpha2_json(&state, &product, &request.normalized_change).await?;
    let snapshot_hash = canonical_material_hash(&snapshot)?;
    let expected_preflight_hash = canonical_material_hash(&json!({
        "product_id": product.id,
        "state_hash": current.state_hash,
        "normalized_change": request.normalized_change,
        "resulting_snapshot_hash": snapshot_hash,
    }))?;
    if request.preflight_hash != expected_preflight_hash {
        return Err(ApiError::conflict(
            "product topology differs from the reviewed preflight",
        ));
    }
    let mut binding_revisions = Vec::with_capacity(request.normalized_change.bindings.len());
    for binding in &request.normalized_change.bindings {
        binding_revisions.push(ProductModelBindingRevision {
            binding_id: binding.binding_id.clone(),
            repository_id: binding.repository_id.clone(),
            revision_id: binding.revision_id.clone(),
            status: binding.status.clone(),
            scopes: binding
                .scopes
                .iter()
                .map(|scope| RepositoryBindingScope {
                    id: scope.id.clone(),
                    binding_revision_id: binding.revision_id.clone(),
                    path_glob: scope.path_glob.clone(),
                    role: scope.role.clone(),
                    service_id: scope.service_id.clone(),
                    created_at: String::new(),
                })
                .collect(),
            evidence_json: json!({
                "kind": "operator_product_model_revision",
                "preflight_hash": request.preflight_hash,
            }),
            content_hash: canonical_material_hash(&json!({
                "binding_id": binding.binding_id,
                "repository_id": binding.repository_id,
                "status": binding.status,
                "scopes": binding.scopes,
            }))?,
        });
    }
    let model_revision = ApplyProductModelRevision {
        product_id: product.id.clone(),
        expected_state_version: product.state_version,
        services: request
            .normalized_change
            .services
            .iter()
            .map(|service| ProductModelServiceRevision {
                id: service.id.clone(),
                service_key: service.service_key.clone(),
                display_name: service.display_name.clone(),
                description: service.description.clone(),
                status: service.status.clone(),
            })
            .collect(),
        bindings: binding_revisions,
        snapshot_id: new_prefixed_id("pmodel"),
        snapshot_json: snapshot,
        snapshot_hash,
        actor: request.actor.trim().into(),
        reason: request.reason.trim().into(),
    };
    let snapshot = state
        .store
        .apply_product_model_revision(model_revision)
        .await?;
    Ok(Json(snapshot_response(snapshot)))
}

async fn list_product_services(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    find_product(&state, &product_id).await?;
    let services = state.store.list_product_services(&product_id).await?;
    let responses = services
        .into_iter()
        .map(|service| ServiceResponse {
            id: service.id,
            product_id: service.product_id,
            service_key: service.service_key,
            display_name: service.display_name,
            description: service.description,
            status: service.status,
        })
        .collect::<Vec<_>>();
    Ok(Json(
        json!({"services": responses, "count": responses.len()}),
    ))
}

async fn preflight_repository_registration(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
    Json(request): Json<RepositoryRegistrationPreflightRequest>,
) -> Result<Json<RepositoryRegistrationPreflightResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let product = find_product(&state, &product_id).await?;
    let response = repository_registration_preflight(&state, &product, &request).await?;
    Ok(Json(response))
}

async fn register_repository(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
    Json(request): Json<RegisterRepositoryRequest>,
) -> Result<Json<RepositoryResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    validate_required(&request.actor, "actor", 200)?;
    validate_required(&request.reason, "reason", 1_000)?;
    let product = find_product(&state, &product_id).await?;
    let preflight_request = RepositoryRegistrationPreflightRequest {
        repository_url: request.repository_url,
        source_commit: request.source_commit,
    };
    let preflight = repository_registration_preflight(&state, &product, &preflight_request).await?;
    if preflight.preflight_hash != request.preflight_hash {
        return Err(ApiError::conflict(
            "repository registration preflight is stale; refresh and retry",
        ));
    }
    if !preflight.blockers.is_empty() {
        return Err(ApiError::conflict(format!(
            "repository registration is blocked: {}",
            preflight.blockers.join("; ")
        )));
    }

    let repositories = state.store.list_product_repositories(&product_id).await?;
    let services = state.store.list_product_services(&product_id).await?;
    let bindings = state
        .store
        .list_product_repository_bindings(&product_id)
        .await?;
    let repository_id = state
        .store
        .get_repository_by_provider_identity("github", &preflight.provider_repository_id)
        .await?
        .map(|repository| repository.id)
        .unwrap_or_else(|| new_prefixed_id("repo"));
    let binding_id = new_prefixed_id("rbind");
    let binding_revision_id = new_prefixed_id("rbrev");
    let onboarding_id = new_prefixed_id("ronb");
    let snapshot_id = new_prefixed_id("pmodel");
    let draft = StoredRepositoryDraft {
        id: repository_id.clone(),
        provider: "github".into(),
        external_id: preflight.provider_repository_id.clone(),
        canonical_url: preflight.canonical_url.clone(),
        default_branch: preflight.default_branch.clone(),
        registered_commit: preflight.source_commit.clone(),
    };
    let evidence = json!({
        "schema_version": "pharness.dev/repository-registration/v1alpha1",
        "provider": "github",
        "provider_repository_id": preflight.provider_repository_id,
        "external_id": preflight.external_id,
        "canonical_url": preflight.canonical_url,
        "default_branch": preflight.default_branch,
        "source_commit": preflight.source_commit,
        "commit_verified": true,
        "preflight_hash": preflight.preflight_hash,
    });
    let binding_hash = canonical_material_hash(&json!({
        "schema_version": "pharness.dev/repository-binding/v1alpha1",
        "product_id": product_id,
        "repository_id": repository_id,
        "service_ids": [],
        "scopes": ["**"],
        "status": "reviewed",
        "evidence": evidence,
    }))?;
    let model = product_model_with_registration(
        &product,
        &services,
        &repositories,
        &bindings,
        &draft,
        &binding_id,
        &binding_revision_id,
    );
    let aggregate = state
        .store
        .register_repository(RegisterRepositoryAggregate {
            repository: draft,
            binding_id,
            binding_revision_id,
            onboarding_id,
            binding_content_hash: binding_hash,
            evidence_json: evidence,
            product_id,
            expected_product_state_version: product.state_version,
            snapshot_id,
            snapshot_hash: canonical_material_hash(&model)?,
            snapshot_json: model,
            actor: request.actor.trim().into(),
            reason: request.reason.trim().into(),
        })
        .await?;
    Ok(Json(registered_repository_response(aggregate)))
}

async fn list_product_repositories(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
) -> Result<Json<RepositoriesResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    find_product(&state, &product_id).await?;
    let repositories = state.store.list_product_repositories(&product_id).await?;
    let mut responses = Vec::with_capacity(repositories.len());
    for repository in repositories {
        let binding = state
            .store
            .get_repository_binding(&product_id, &repository.id)
            .await?;
        let onboarding = state
            .store
            .list_repository_onboardings(&repository.id)
            .await?
            .into_iter()
            .find(|onboarding| onboarding.product_id == product_id);
        responses.push(repository_response(repository, binding, onboarding));
    }
    let count = responses.len();
    Ok(Json(RepositoriesResponse {
        repositories: responses,
        count,
    }))
}

async fn get_repository(
    State(state): State<AppState>,
    Path(repository_id): Path<String>,
) -> Result<Json<RepositoryResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let repository = state
        .store
        .get_repository(&repository_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository", &repository_id))?;
    Ok(Json(repository_response(repository, None, None)))
}

async fn get_repository_readiness(
    State(state): State<AppState>,
    Path(repository_id): Path<String>,
    Query(query): Query<RepositoryReadinessQuery>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    if !is_git_sha(&query.source_commit) {
        return Err(ApiError::bad_request(
            "source_commit must be a full 40-character Git object ID",
        ));
    }
    let repository = state
        .store
        .get_repository(&repository_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository", &repository_id))?;
    let assessment = state
        .store
        .latest_repository_readiness_assessment(
            &repository_id,
            &query.source_commit.to_ascii_lowercase(),
        )
        .await?;
    let source_commit = query.source_commit.to_ascii_lowercase();
    let mismatches = match assessment.as_ref() {
        Some(assessment) => {
            let version = state
                .store
                .latest_repository_contract_version(&repository_id, &source_commit)
                .await?;
            match version {
                Some(version) => {
                    let contract: pharness_core::RepositoryContract =
                        serde_json::from_value(version.contract.clone()).map_err(|error| {
                            ApiError::internal(format!(
                                "stored RepositoryContract is invalid: {error}"
                            ))
                        })?;
                    current_readiness_mismatches(
                        &state,
                        &repository,
                        &source_commit,
                        &version,
                        &contract,
                        assessment,
                    )
                    .await?
                }
                None => vec!["canonical_contract_version_missing".into()],
            }
        }
        None => vec!["assessment_missing".into()],
    };
    Ok(Json(readiness_response(
        &state,
        &repository,
        &source_commit,
        assessment,
        mismatches,
    )))
}

async fn create_repository_readiness_assessment(
    State(state): State<AppState>,
    Path(repository_id): Path<String>,
    Json(request): Json<CreateRepositoryReadinessRequest>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    validate_required(&request.actor, "actor", 200)?;
    validate_required(&request.reason, "reason", 1_000)?;
    if !is_git_sha(&request.source_commit) {
        return Err(ApiError::bad_request(
            "source_commit must be a full 40-character Git object ID",
        ));
    }
    let source_commit = request.source_commit.to_ascii_lowercase();
    let repository = state
        .store
        .get_repository(&repository_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository", &repository_id))?;
    if repository.registered_commit != source_commit {
        return Err(ApiError::conflict(
            "readiness source must match the Repository's registered immutable revision",
        ));
    }
    let version = state
        .store
        .latest_repository_contract_version(&repository_id, &source_commit)
        .await?
        .ok_or_else(|| {
            ApiError::conflict(
                "no validated canonical RepositoryContract exists at this exact revision",
            )
        })?;
    let contract: pharness_core::RepositoryContract =
        serde_json::from_value(version.contract.clone()).map_err(|error| {
            ApiError::internal(format!("stored RepositoryContract is invalid: {error}"))
        })?;
    contract
        .validate_candidate()
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let profile = state
        .environment_profiles
        .iter()
        .find(|profile| {
            profile.active
                && profile.id == contract.environment_profile
                && profile.repository_allowlist.contains(&repository.canonical_url)
        })
        .cloned()
        .ok_or_else(|| {
            ApiError::conflict(
                "RepositoryContract EnvironmentProfile is inactive or does not allow this repository",
            )
        })?;
    if !state.worker.source_reader_available()
        || !state
            .worker
            .source_reader_allows_repository(&repository.canonical_url)
    {
        return Err(ApiError::conflict(
            "repository readiness requires a verified isolated source-reader allowlist",
        ));
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .as_millis();
    let source_reader_verification = state
        .store
        .latest_capability_verification_for_repository(
            "source_reader",
            &repository.canonical_url,
        )
        .await?
        .filter(|verification| {
            verification.status == "available"
                && verification.repository.as_deref() == Some(repository.canonical_url.as_str())
                && verification
                    .expires_at
                    .parse::<u128>()
                    .is_ok_and(|expiry| expiry > now)
        })
        .ok_or_else(|| {
            ApiError::conflict(
                "repository readiness requires a fresh passing source-reader verification for this exact repository",
            )
        })?;
    let profile_capability = format!("environment_profile:{}", profile.id);
    let profile_verification = state
        .store
        .latest_capability_verification(&profile_capability)
        .await?
        .filter(|verification| {
            verification.status == "available"
                && verification
                    .expires_at
                    .parse::<u128>()
                    .is_ok_and(|expiry| expiry > now)
        })
        .ok_or_else(|| {
            ApiError::conflict(
                "repository readiness requires a fresh passing runner-profile verification",
            )
        })?;
    let input = json!({
        "schema_version": "pharness.dev/repository-readiness-input/v1alpha1",
        "repository_id": repository_id,
        "source_commit": source_commit,
        "contract_version_id": version.id,
        "contract_hash": version.content_hash,
        "dependency_lock_hash": contract.dependency_lock.sha256,
        "environment_profile_id": profile.id,
        "environment_profile_revision": profile.revision,
        "runner_image": profile.image,
        "validation_policy_version": "repo-mode-v1",
        "required_executables":profile.required_executables,
        "acceptance_commands":contract.acceptance_commands,
        "capability_evidence":{
            "source_reader":{"id":source_reader_verification.id,"verified_at":source_reader_verification.verified_at,"expires_at":source_reader_verification.expires_at},
            "environment_profile":{"id":profile_verification.id,"verified_at":profile_verification.verified_at,"expires_at":profile_verification.expires_at},
        },
    });
    let input_hash = canonical_material_hash(&input)?;
    if let Some(existing) = state
        .store
        .latest_repository_readiness_assessment(&repository_id, &source_commit)
        .await?
        .filter(|assessment| assessment.input_hash == input_hash)
    {
        return Ok(Json(readiness_response(
            &state,
            &repository,
            &source_commit,
            Some(existing),
            Vec::new(),
        )));
    }
    let subject_id = format!("{repository_id}:{source_commit}");
    if let Some(current) = state
        .store
        .latest_subject_environment_preparation("repository_readiness", &subject_id)
        .await?
        .filter(|preparation| {
            preparation.input_hash == input_hash
                && matches!(preparation.status.as_str(), "queued" | "running")
        })
    {
        return Ok(Json(json!({
            "repository_id":repository.id,
            "source_commit":source_commit,
            "status":"assessment_running",
            "preparation":current,
        })));
    }
    let workspace = state
        .store
        .create_subject_workspace(CreateSubjectWorkspace {
            id: new_prefixed_id("sws"),
            subject_kind: "repository_readiness".into(),
            subject_id: subject_id.clone(),
            run_id: None,
            status: "provisioning".into(),
            source_repo: repository.canonical_url.clone(),
            source_ref: repository.default_branch.clone(),
            source_commit: source_commit.clone(),
            branch: None,
            retention_status: "ephemeral".into(),
            actor: request.actor.trim().into(),
            reason: request.reason.trim().into(),
        })
        .await?;
    let preparation = state
        .store
        .create_subject_environment_preparation(CreateSubjectEnvironmentPreparation {
            id: new_prefixed_id("sprep"),
            subject_kind: "repository_readiness".into(),
            subject_id,
            workspace_id: workspace.id.clone(),
            run_id: None,
            status: "queued".into(),
            environment_profile_id: profile.id.clone(),
            source_commit: source_commit.clone(),
            input_hash,
            input,
        })
        .await?;
    let dispatch = state
        .worker
        .dispatch_repository_readiness(RepositoryReadinessExecutionRequest {
            preparation_id: preparation.id.clone(),
            profile,
        })
        .await;
    match dispatch {
        Ok(receipt) => {
            tracing::info!(repository_id, preparation_id=%preparation.id, job=%receipt.job_name, actor=%request.actor, reason=%request.reason, "repository readiness dispatched");
            Ok(Json(json!({
                "repository_id":repository.id,
                "source_commit":source_commit,
                "status":"assessment_running",
                "workspace":workspace,
                "preparation":preparation,
                "job_name":receipt.job_name,
            })))
        }
        Err(error) => {
            state
                .store
                .complete_subject_environment_preparation(CompleteSubjectEnvironmentPreparation {
                    id: preparation.id,
                    status: "failed".into(),
                    resolved_commit: None,
                    repository_contract: None,
                    repository_contract_hash: None,
                    environment_snapshot: None,
                    acceptance_results: json!([]),
                    logs: json!([{"step":"dispatch","status":"failed"}]),
                    error_code: Some("readiness_dispatch_failed".into()),
                })
                .await?;
            Err(ApiError::unavailable(format!(
                "repository readiness dispatch failed: {error}"
            )))
        }
    }
}

fn readiness_response(
    state: &AppState,
    repository: &StoredRepository,
    source_commit: &str,
    assessment: Option<pharness_store::StoredRepositoryReadinessAssessment>,
    mismatches: Vec<String>,
) -> Value {
    let writer = state.worker.git_writer_settings();
    let observer = state.worker.git_observer_settings();
    let (current, status, current_blockers) =
        readiness_current_state(assessment.is_some(), &mismatches);
    json!({
        "repository_id": repository.id,
        "source_commit": source_commit,
        "current": current,
        "status": status,
        "mismatches": mismatches,
        "blockers": current_blockers,
        "assessment": assessment,
        "capabilities": {
            "source_reader": {
                "availability": if state.worker.source_reader_available() { "configured_unverified" } else { "unavailable" },
                "trust_policy": if state.worker.source_reader_allows_repository(&repository.canonical_url) { "allowed" } else { "denied" },
                "authorization": "not_required_for_read_only_assessment",
            },
            "source_writer": {
                "availability": if writer.is_some() { "configured_unverified" } else { "unavailable" },
                "trust_policy": if writer.as_ref().is_some_and(|settings| settings.allowed_repos.contains(&repository.canonical_url)) { "allowed" } else { "denied" },
                "authorization": "not_granted",
            },
            "provider_observer": {
                "availability": if observer.is_some() { "configured_unverified" } else { "unavailable" },
                "trust_policy": if observer.as_ref().is_some_and(|settings| settings.allowed_repos.contains(&repository.canonical_url)) { "allowed" } else { "denied" },
                "authorization": "not_required_for_read_only_observation",
            },
        },
    })
}

fn readiness_current_state(
    assessment_present: bool,
    mismatches: &[String],
) -> (bool, &'static str, Vec<Value>) {
    let current = assessment_present && mismatches.is_empty();
    let status = if !assessment_present {
        "missing"
    } else if current {
        "ready"
    } else {
        "stale"
    };
    let blockers = mismatches
        .iter()
        .map(|code| {
            json!({
                "code": code,
                "summary": readiness_mismatch_summary(code),
            })
        })
        .collect();
    (current, status, blockers)
}

fn readiness_mismatch_summary(code: &str) -> &'static str {
    match code {
        "assessment_missing" => {
            "no immutable readiness assessment exists for the exact source commit"
        }
        "canonical_contract_version_missing" => {
            "the exact source commit no longer has a canonical RepositoryContract version"
        }
        "assessment_not_ready" => {
            "the immutable assessment did not prove both contract and coding readiness"
        }
        "contract_or_policy_tuple_changed" => {
            "the contract, dependency lock, or validation-policy tuple changed"
        }
        "environment_profile_unavailable" => {
            "the contract-selected EnvironmentProfile is unavailable for this repository"
        }
        "environment_profile_tuple_changed" => {
            "the EnvironmentProfile revision or immutable runner digest changed"
        }
        "assessment_expired" => "the immutable readiness assessment expired",
        "source_reader_evidence_stale" => {
            "the bound isolated source-reader verification is missing, expired, or superseded"
        }
        "runner_profile_evidence_stale" => {
            "the bound isolated runner-profile verification is missing, expired, or superseded"
        }
        "readiness_input_hash_mismatch" => {
            "the recomputed readiness input hash does not match the immutable assessment"
        }
        _ => "the immutable readiness assessment no longer matches current controller inputs",
    }
}

async fn create_repository_onboarding(
    State(state): State<AppState>,
    Path(repository_id): Path<String>,
    Json(request): Json<CreateRepositoryOnboardingRequest>,
) -> Result<Json<RepositoryOnboardingResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    validate_required(&request.actor, "actor", 200)?;
    validate_required(&request.reason, "reason", 1_000)?;
    if !is_git_sha(&request.source_commit) {
        return Err(ApiError::bad_request(
            "source_commit must be a full 40-character Git object ID",
        ));
    }
    let repository = state
        .store
        .get_repository(&repository_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository", &repository_id))?;
    find_product(&state, &request.product_id).await?;
    let binding = state
        .store
        .get_repository_binding(&request.product_id, &repository_id)
        .await?
        .ok_or_else(|| ApiError::conflict("repository is not actively bound to this Product"))?;
    // Reverify the immutable provider object before creating durable onboarding state.
    let external_id = github_external_id(&repository.canonical_url);
    let _ = resolve_public_github_repository(&external_id, &request.source_commit).await?;
    let onboarding = state
        .store
        .create_repository_onboarding(CreateRepositoryOnboarding {
            id: new_prefixed_id("ronb"),
            product_id: request.product_id,
            repository_id,
            binding_id: binding.id,
            onboarding_kind: "refresh".into(),
            registered_commit: request.source_commit.to_ascii_lowercase(),
            actor: request.actor.trim().into(),
            reason: request.reason.trim().into(),
        })
        .await?;
    Ok(Json(onboarding_response(onboarding)?))
}

async fn get_repository_onboarding(
    State(state): State<AppState>,
    Path(onboarding_id): Path<String>,
) -> Result<Json<RepositoryOnboardingResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let onboarding = find_onboarding(&state, &onboarding_id).await?;
    Ok(Json(onboarding_response(onboarding)?))
}

async fn get_repository_onboarding_flow(
    State(state): State<AppState>,
    Path(onboarding_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let onboarding = find_onboarding(&state, &onboarding_id).await?;
    let discovery = match onboarding.current_discovery_id.as_deref() {
        Some(id) => state.store.get_repository_discovery(id).await?,
        None => None,
    };
    let proposal = state
        .store
        .get_current_repository_onboarding_proposal(&onboarding.id)
        .await?;
    let proposer_run = match onboarding.proposer_run_id.as_deref() {
        Some(run_id) => state.store.get_run(&RunId::new(run_id)).await?,
        None => None,
    };
    let source_delivery_intent = match onboarding.source_delivery_intent_id.as_deref() {
        Some(intent_id) => state.store.get_source_delivery_intent(intent_id).await?,
        None => None,
    };
    let readiness = match onboarding.readiness_assessment_id.as_deref() {
        Some(assessment_id) => {
            state
                .store
                .get_repository_readiness_assessment(assessment_id)
                .await?
        }
        None => {
            let source_commit = onboarding
                .resolved_commit
                .as_deref()
                .unwrap_or(onboarding.registered_commit.as_str());
            state
                .store
                .latest_repository_readiness_assessment(&onboarding.repository_id, source_commit)
                .await?
        }
    };
    let response = onboarding_response(onboarding)?;
    Ok(Json(json!({
        "onboarding": response,
        "discovery": discovery,
        "proposal": proposal,
        "proposer_run": proposer_run,
        "source_delivery_intent": source_delivery_intent,
        "readiness": readiness,
    })))
}

async fn put_repository_onboarding_proposal(
    State(state): State<AppState>,
    Path(onboarding_id): Path<String>,
    Json(request): Json<PutRepositoryOnboardingProposalRequest>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    validate_required(&request.actor, "actor", 200)?;
    validate_required(&request.reason, "reason", 1_000)?;
    if request.proposal.schema_version != pharness_core::ONBOARDING_PROPOSAL_SCHEMA {
        return Err(ApiError::bad_request(
            "proposal schema_version must be pharness.dev/repository-onboarding-proposal/v1alpha1",
        ));
    }
    if request.proposal.instructions.len() > 32 * 1024 {
        return Err(ApiError::bad_request(
            "repository instructions must not exceed 32 KiB",
        ));
    }
    let onboarding = find_onboarding(&state, &onboarding_id).await?;
    let preview = onboarding_response(onboarding.clone())?;
    if preview.state_hash != request.state_hash {
        return Err(ApiError::conflict(
            "repository onboarding changed after proposal preview; refresh and retry",
        ));
    }
    let discovery_id = onboarding
        .current_discovery_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("onboarding has no current discovery"))?;
    let discovery = state
        .store
        .get_repository_discovery(discovery_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository_discovery", discovery_id))?;
    if discovery.status != "succeeded"
        || request.proposal.discovery_id != discovery.id
        || request.proposal.discovery_hash != discovery.content_hash.clone().unwrap_or_default()
    {
        return Err(ApiError::conflict(
            "proposal must reference the exact current successful discovery",
        ));
    }
    let contract: pharness_core::RepositoryContract =
        serde_json::from_value(request.proposal.candidate_contract.clone()).map_err(|error| {
            ApiError::bad_request(format!("candidate contract is invalid: {error}"))
        })?;
    contract
        .validate_candidate()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    if !state
        .environment_profiles
        .iter()
        .any(|profile| profile.active && profile.id == contract.environment_profile)
    {
        return Err(ApiError::conflict(
            "candidate contract selects an unavailable EnvironmentProfile",
        ));
    }
    validate_onboarding_product_proposals(&state, &onboarding, &request.proposal).await?;
    let proposal_value = serde_json::to_value(&request.proposal)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let content_hash = canonical_material_hash(&proposal_value)?;
    let stored = state
        .store
        .create_repository_onboarding_proposal(CreateRepositoryOnboardingProposal {
            id: new_prefixed_id("rprop"),
            onboarding_id,
            expected_state_version: onboarding.state_version,
            proposal: proposal_value,
            content_hash,
            discovery_id: discovery.id,
            discovery_hash: discovery.content_hash.unwrap_or_default(),
            actor: request.actor.trim().into(),
            origin: "operator".into(),
        })
        .await?;
    Ok(Json(json!({"proposal": stored})))
}

async fn execute_repository_onboarding_action(
    State(state): State<AppState>,
    Path((onboarding_id, action_id)): Path<(String, String)>,
    Json(request): Json<ExecuteRepositoryOnboardingActionRequest>,
) -> Result<Json<RepositoryOnboardingResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    validate_required(&request.actor, "actor", 200)?;
    validate_required(&request.reason, "reason", 1_000)?;
    let onboarding = find_onboarding(&state, &onboarding_id).await?;
    let preview = onboarding_response(onboarding.clone())?;
    if request.state_hash != preview.state_hash {
        return Err(ApiError::conflict(
            "repository onboarding changed after action preview; refresh and retry",
        ));
    }
    let action = preview
        .actions
        .iter()
        .find(|action| action.id == action_id)
        .ok_or_else(|| ApiError::not_found("repository_onboarding_action", &action_id))?;
    if action.status != "available" {
        return Err(ApiError::conflict(format!(
            "repository onboarding action is unavailable: {}",
            action.blockers.join("; ")
        )));
    }
    match action_id.as_str() {
        "start_discovery" | "retry_discovery" => {
            let repository = state
                .store
                .get_repository(&onboarding.repository_id)
                .await?
                .ok_or_else(|| ApiError::not_found("repository", &onboarding.repository_id))?;
            if !state.worker.source_reader_available() {
                return Err(ApiError::unavailable(
                    "isolated source-reader capability is unavailable",
                ));
            }
            if !state
                .worker
                .source_reader_allows_repository(&repository.canonical_url)
            {
                return Err(ApiError::conflict(
                    "repository is outside the source-reader allowlist",
                ));
            }
            let discovery_id = new_prefixed_id("rdisc");
            state
                .store
                .create_repository_discovery(
                    &discovery_id,
                    &onboarding.id,
                    &onboarding.registered_commit,
                )
                .await?;
            let receipt = state
                .worker
                .dispatch_repository_discovery(RepositoryDiscoveryRequest {
                    discovery_id: discovery_id.clone(),
                })
                .await;
            match receipt {
                Ok(receipt) => tracing::info!(
                    discovery_id = %discovery_id,
                    job = %receipt.job_name,
                    actor = %request.actor,
                    reason = %request.reason,
                    "repository discovery action dispatched"
                ),
                Err(error) => {
                    let _ = state
                        .store
                        .fail_repository_discovery(
                            &discovery_id,
                            "discovery_dispatch_failed",
                            "isolated repository discovery could not be dispatched",
                        )
                        .await;
                    return Err(ApiError::unavailable(format!(
                        "repository discovery dispatch failed: {error}"
                    )));
                }
            }
        }
        "approve_proposal" => {
            let proposal = state
                .store
                .get_current_repository_onboarding_proposal(&onboarding.id)
                .await?
                .ok_or_else(|| ApiError::conflict("onboarding has no proposed revision"))?;
            let typed: pharness_core::RepositoryOnboardingProposal =
                serde_json::from_value(proposal.proposal.clone()).map_err(|error| {
                    ApiError::internal(format!("stored onboarding proposal is invalid: {error}"))
                })?;
            validate_onboarding_product_proposals(&state, &onboarding, &typed).await?;
            // Product topology and executable Repository onboarding are separate
            // review boundaries. This switch exists only to finish an explicitly
            // reviewed onboarding on a retained legacy generation.
            let model_change =
                if std::env::var("PHARNESS_LEGACY_ONBOARDING_PRODUCT_MODEL_APPLY_ENABLED")
                    .ok()
                    .as_deref()
                    == Some("true")
                {
                    approved_onboarding_model_change(&state, &onboarding, &proposal, &typed).await?
                } else {
                    None
                };
            state
                .store
                .approve_repository_onboarding_proposal(ApproveRepositoryOnboardingProposal {
                    onboarding_id: onboarding.id.clone(),
                    proposal_id: proposal.id.clone(),
                    proposal_hash: proposal.content_hash.clone(),
                    expected_state_version: onboarding.state_version,
                    actor: request.actor.trim().into(),
                    reason: request.reason.trim().into(),
                    model_change,
                })
                .await?;
        }
        "start_proposer" | "retry_proposer" => {
            start_repository_onboarding_proposer(
                &state,
                &onboarding,
                request.actor.trim(),
                request.reason.trim(),
            )
            .await?;
        }
        "prepare_onboarding_patch" | "retry_onboarding_patch" => {
            let execution_id = new_prefixed_id("onbpatch");
            state
                .store
                .start_repository_onboarding_patch(
                    &onboarding.id,
                    onboarding.state_version,
                    &execution_id,
                    request.actor.trim(),
                    request.reason.trim(),
                )
                .await?;
            let dispatch = state
                .worker
                .dispatch_onboarding_patch(OnboardingPatchRequest {
                    onboarding_id: onboarding.id.clone(),
                    execution_id: execution_id.clone(),
                })
                .await;
            match dispatch {
                Ok(receipt) => tracing::info!(
                    onboarding_id = %onboarding.id,
                    execution_id,
                    job = %receipt.job_name,
                    "approved onboarding patch materializer dispatched"
                ),
                Err(error) => {
                    let _ = state
                        .store
                        .fail_repository_onboarding_patch(
                            &onboarding.id,
                            &execution_id,
                            "onboarding_patch_dispatch_failed",
                        )
                        .await;
                    return Err(ApiError::unavailable(format!(
                        "onboarding patch dispatch failed: {error}"
                    )));
                }
            }
        }
        "authorize_onboarding_source_delivery" => {
            authorize_and_dispatch_onboarding_source_delivery(
                &state,
                &onboarding,
                request.actor.trim(),
                request.reason.trim(),
            )
            .await?;
        }
        "observe_onboarding_source_delivery" => {
            dispatch_onboarding_source_delivery_observation(
                &state,
                &onboarding,
                request.actor.trim(),
                request.reason.trim(),
            )
            .await?;
        }
        "validate_merged_contract" | "retry_merged_contract_validation" => {
            let execution_id = new_prefixed_id("onbvalidate");
            state
                .store
                .start_repository_onboarding_contract_validation(
                    &onboarding.id,
                    onboarding.state_version,
                    &execution_id,
                    request.actor.trim(),
                    request.reason.trim(),
                )
                .await?;
            let dispatch = state
                .worker
                .dispatch_onboarding_contract_validation(OnboardingContractValidationRequest {
                    onboarding_id: onboarding.id.clone(),
                    execution_id: execution_id.clone(),
                })
                .await;
            match dispatch {
                Ok(receipt) => {
                    tracing::info!(onboarding_id=%onboarding.id, execution_id, job=%receipt.job_name, "merged onboarding contract validation dispatched")
                }
                Err(error) => {
                    let _ = state
                        .store
                        .fail_repository_onboarding_contract_validation(
                            &onboarding.id,
                            &execution_id,
                            "merged contract validation could not be dispatched",
                        )
                        .await;
                    return Err(ApiError::unavailable(format!(
                        "merged contract validation dispatch failed: {error}"
                    )));
                }
            }
        }
        _ => {
            return Err(ApiError::bad_request(
                "unsupported repository onboarding action",
            ))
        }
    }
    let updated = find_onboarding(&state, &onboarding_id).await?;
    Ok(Json(onboarding_response(updated)?))
}

async fn validate_onboarding_product_proposals(
    state: &AppState,
    onboarding: &StoredRepositoryOnboarding,
    proposal: &pharness_core::RepositoryOnboardingProposal,
) -> Result<(), ApiError> {
    if proposal.service_proposals.len() > 32 {
        return Err(ApiError::bad_request(
            "an onboarding proposal may define at most 32 Services",
        ));
    }
    if proposal.binding_proposals.len() > 1 {
        return Err(ApiError::bad_request(
            "Repo Mode V1 permits at most one binding proposal for the onboarding Repository",
        ));
    }
    let existing = state
        .store
        .list_product_services(&onboarding.product_id)
        .await?;
    let mut service_keys = existing
        .iter()
        .map(|service| service.service_key.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let existing_keys = service_keys.clone();
    for service in &proposal.service_proposals {
        let key = normalize_key(&service.service_key)?;
        if key != service.service_key {
            return Err(ApiError::bad_request(format!(
                "Service key {} is not canonical; use {key}",
                service.service_key
            )));
        }
        validate_required(&service.display_name, "service display_name", 120)?;
        if service.description.len() > 4_000 {
            return Err(ApiError::bad_request(
                "Service description exceeds 4,000 characters",
            ));
        }
        if existing_keys.contains(&key) {
            return Err(ApiError::conflict(format!(
                "Service {key} already exists in the Product"
            )));
        }
        if !service_keys.insert(key.clone()) {
            return Err(ApiError::bad_request(format!(
                "onboarding proposal repeats Service {key}"
            )));
        }
    }
    if let Some(binding) = proposal.binding_proposals.first() {
        if binding.scopes.is_empty() || binding.scopes.len() > 64 {
            return Err(ApiError::bad_request(
                "binding scopes must contain between one and 64 repository-relative globs",
            ));
        }
        let mut seen_services = std::collections::BTreeSet::new();
        for key in &binding.service_keys {
            if !seen_services.insert(key) {
                return Err(ApiError::bad_request(format!(
                    "binding proposal repeats Service key {key}"
                )));
            }
            if !service_keys.contains(key) {
                return Err(ApiError::bad_request(format!(
                    "binding proposal references unknown Service {key}"
                )));
            }
        }
        let mut seen_scopes = std::collections::BTreeSet::new();
        for scope in &binding.scopes {
            validate_binding_scope(scope)?;
            if !seen_scopes.insert(scope) {
                return Err(ApiError::bad_request(format!(
                    "binding proposal repeats scope {scope}"
                )));
            }
        }
    }
    Ok(())
}

fn validate_binding_scope(scope: &str) -> Result<(), ApiError> {
    if scope.is_empty()
        || scope.len() > 256
        || scope.starts_with(['/', '~'])
        || scope.contains(['\\', '\n', '\r', '\0'])
        || scope
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return Err(ApiError::bad_request(format!(
            "binding scope {scope:?} is not a normalized repository-relative glob"
        )));
    }
    Ok(())
}

async fn approved_onboarding_model_change(
    state: &AppState,
    onboarding: &StoredRepositoryOnboarding,
    stored_proposal: &StoredRepositoryOnboardingProposal,
    proposal: &pharness_core::RepositoryOnboardingProposal,
) -> Result<Option<ApprovedOnboardingProductModelChange>, ApiError> {
    validate_onboarding_product_proposals(state, onboarding, proposal).await?;
    if proposal.service_proposals.is_empty() && proposal.binding_proposals.is_empty() {
        return Ok(None);
    }
    let product = find_product(state, &onboarding.product_id).await?;
    let mut services = state.store.list_product_services(&product.id).await?;
    let mut service_ids = services
        .iter()
        .map(|service| (service.service_key.clone(), service.id.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let approved_services = proposal
        .service_proposals
        .iter()
        .map(|service| ApprovedOnboardingService {
            id: new_prefixed_id("svc"),
            service_key: service.service_key.clone(),
            display_name: service.display_name.trim().into(),
            description: service.description.trim().into(),
        })
        .collect::<Vec<_>>();
    for service in &approved_services {
        service_ids.insert(service.service_key.clone(), service.id.clone());
        services.push(StoredService {
            id: service.id.clone(),
            product_id: product.id.clone(),
            service_key: service.service_key.clone(),
            display_name: service.display_name.clone(),
            description: service.description.clone(),
            status: "active".into(),
            created_at: String::new(),
            updated_at: String::new(),
        });
    }
    services.sort_by(|left, right| {
        left.service_key
            .cmp(&right.service_key)
            .then(left.id.cmp(&right.id))
    });
    let repositories = state.store.list_product_repositories(&product.id).await?;
    let mut bindings = state
        .store
        .list_product_repository_bindings(&product.id)
        .await?;
    let binding = bindings
        .iter_mut()
        .find(|binding| binding.id == onboarding.binding_id)
        .ok_or_else(|| ApiError::conflict("onboarding Repository binding is unavailable"))?;
    let binding_revision_id = proposal
        .binding_proposals
        .first()
        .map(|_| new_prefixed_id("rbrev"));
    if let Some(revision_id) = &binding_revision_id {
        binding.current_revision_id = revision_id.clone();
    }
    let snapshot_id = new_prefixed_id("pmodel");
    let snapshot = product_model_json(
        &product.id,
        &product.organization_id,
        &product.product_key,
        &product.display_name,
        &product.description,
        &product.owner_principal,
        &services,
        &repositories,
        &bindings,
    );
    let snapshot_hash = canonical_material_hash(&snapshot)?;
    let (binding_service_ids, binding_scopes, binding_evidence, binding_content_hash) =
        if let Some(binding) = proposal.binding_proposals.first() {
            let mut ids = binding
                .service_keys
                .iter()
                .map(|key| {
                    service_ids.get(key).cloned().ok_or_else(|| {
                        ApiError::internal(format!("validated Service {key} has no ID"))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            ids.sort();
            let mut scopes = binding.scopes.clone();
            scopes.sort();
            let evidence = json!({
                "schema_version":"pharness.dev/repository-binding-evidence/v1alpha1",
                "onboarding_id":onboarding.id,
                "proposal_id":stored_proposal.id,
                "proposal_hash":stored_proposal.content_hash,
                "discovery_id":stored_proposal.discovery_id,
                "discovery_hash":stored_proposal.discovery_hash,
            });
            let material = json!({
                "schema_version":"pharness.dev/repository-binding/v1alpha1",
                "binding_id":onboarding.binding_id,
                "service_ids":ids,
                "scopes":scopes,
                "evidence":evidence,
            });
            (
                ids,
                scopes,
                evidence,
                Some(canonical_material_hash(&material)?),
            )
        } else {
            (Vec::new(), Vec::new(), json!({}), None)
        };
    Ok(Some(ApprovedOnboardingProductModelChange {
        product_id: product.id,
        expected_product_state_version: product.state_version,
        services: approved_services,
        binding_id: onboarding.binding_id.clone(),
        binding_revision_id,
        binding_service_ids,
        binding_scopes,
        binding_evidence,
        binding_content_hash,
        snapshot_id,
        snapshot,
        snapshot_hash,
    }))
}

async fn authorize_and_dispatch_onboarding_source_delivery(
    state: &AppState,
    onboarding: &StoredRepositoryOnboarding,
    actor: &str,
    reason: &str,
) -> Result<(), ApiError> {
    let repository = state
        .store
        .get_repository(&onboarding.repository_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository", &onboarding.repository_id))?;
    let writer = state
        .worker
        .git_writer_settings()
        .filter(|settings| settings.allowed_repos.contains(&repository.canonical_url))
        .ok_or_else(|| {
            ApiError::conflict("repository is not allowlisted for the isolated Git writer")
        })?;
    let proposal = state
        .store
        .get_current_repository_onboarding_proposal(&onboarding.id)
        .await?
        .filter(|proposal| {
            proposal.status == "approved"
                && onboarding.approved_proposal_hash.as_deref()
                    == Some(proposal.content_hash.as_str())
        })
        .ok_or_else(|| ApiError::conflict("exact approved onboarding proposal is unavailable"))?;
    let artifact_id = onboarding
        .patch_artifact_id
        .clone()
        .ok_or_else(|| ApiError::conflict("approved onboarding patch artifact is unavailable"))?;
    let patch_hash = onboarding
        .patch_hash
        .clone()
        .ok_or_else(|| ApiError::conflict("approved onboarding patch hash is unavailable"))?;
    let artifact = state
        .store
        .get_artifact(&artifact_id)
        .await?
        .filter(|artifact| artifact.kind == "repository_onboarding_patch")
        .ok_or_else(|| ApiError::conflict("approved onboarding patch artifact is invalid"))?;
    let diff = artifact
        .content_text
        .as_deref()
        .ok_or_else(|| ApiError::conflict("approved onboarding patch artifact is empty"))?;
    if format!("sha256:{:x}", Sha256::digest(diff.as_bytes())) != patch_hash {
        return Err(ApiError::conflict(
            "approved onboarding patch artifact hash does not match",
        ));
    }
    onboarding_patch_paths(diff)?;
    let intent_id = new_prefixed_id("srcintent");
    let execution_id = new_prefixed_id("srcexec");
    let branch = format!("pharness/onboarding/{}", onboarding.id);
    let authorization = json!({
        "schema_version":"pharness.dev/source-delivery-authorization/v1alpha1",
        "actor":actor,
        "reason":reason,
        "onboarding_id":onboarding.id,
        "onboarding_state_hash":onboarding_response(onboarding.clone())?.state_hash,
        "proposal":{"id":proposal.id,"revision":proposal.revision,"hash":proposal.content_hash},
        "repository_id":repository.id,
        "source_repo":repository.canonical_url,
        "base_ref":repository.default_branch,
        "base_commit":onboarding.registered_commit,
        "head_branch":branch,
        "patch_hash":patch_hash,
        "external_effect":"create one GitHub branch, commit, and onboarding pull request; merge is not authorized",
    });
    let intent = state
        .store
        .create_source_delivery_intent(pharness_store::CreateSourceDeliveryIntent {
            id: intent_id,
            subject_kind: "repository_onboarding_proposal".into(),
            subject_id: proposal.id,
            repository_id: repository.id,
            source_repo: repository.canonical_url,
            base_ref: repository.default_branch,
            base_commit: onboarding.registered_commit.clone(),
            head_branch: branch,
            patch_artifact_id: Some(artifact.id),
            patch_hash,
            authorization,
            created_by: actor.into(),
            creation_reason: reason.into(),
        })
        .await?;
    state
        .store
        .bind_repository_onboarding_source_delivery(
            &onboarding.id,
            onboarding.state_version,
            &intent.id,
            actor,
            reason,
        )
        .await?;
    match state
        .worker
        .dispatch_source_delivery(SourceDeliveryExecutionRequest {
            source_delivery_intent_id: intent.id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => {
            state
                .store
                .update_source_delivery_intent(
                    &intent.id,
                    intent.state_version,
                    "writer_dispatched",
                    Some(&execution_id),
                    None,
                    None,
                    None,
                    None,
                    actor,
                    reason,
                )
                .await?;
            state
                .store
                .update_repository_onboarding_source_delivery(
                    &onboarding.id,
                    &intent.id,
                    "writer_dispatched",
                    None,
                    actor,
                    reason,
                )
                .await?;
            tracing::info!(onboarding_id=%onboarding.id, intent_id=%intent.id, job=%receipt.job_name, "onboarding source writer dispatched");
            let _ = writer;
            Ok(())
        }
        Err(error) => {
            state
                .store
                .update_source_delivery_intent(
                    &intent.id,
                    intent.state_version,
                    "failed",
                    Some(&execution_id),
                    None,
                    None,
                    None,
                    None,
                    "controller:repo-mode",
                    "onboarding source writer dispatch failed",
                )
                .await?;
            state
                .store
                .update_repository_onboarding_source_delivery(
                    &onboarding.id,
                    &intent.id,
                    "delivery_failed",
                    None,
                    "controller:repo-mode",
                    "onboarding source writer dispatch failed",
                )
                .await?;
            tracing::warn!(onboarding_id=%onboarding.id, intent_id=%intent.id, %error, "onboarding source writer dispatch failed");
            Err(ApiError::unavailable(
                "onboarding source writer dispatch failed",
            ))
        }
    }
}

async fn dispatch_onboarding_source_delivery_observation(
    state: &AppState,
    onboarding: &StoredRepositoryOnboarding,
    actor: &str,
    reason: &str,
) -> Result<(), ApiError> {
    let intent_id = onboarding
        .source_delivery_intent_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("onboarding source delivery intent is unavailable"))?;
    let intent = state
        .store
        .get_source_delivery_intent(intent_id)
        .await?
        .filter(|intent| {
            matches!(
                intent.status.as_str(),
                "pull_request_open" | "waiting_checks" | "waiting_merge"
            )
        })
        .ok_or_else(|| ApiError::conflict("onboarding source delivery is not observable"))?;
    state
        .worker
        .git_observer_settings()
        .filter(|settings| settings.allowed_repos.contains(&intent.source_repo))
        .ok_or_else(|| {
            ApiError::conflict("repository is not allowlisted for the isolated Git observer")
        })?;
    let execution_id = new_prefixed_id("srcobserve");
    let dispatched = state
        .store
        .update_source_delivery_intent(
            &intent.id,
            intent.state_version,
            "observer_dispatched",
            None,
            Some(&execution_id),
            None,
            None,
            None,
            actor,
            reason,
        )
        .await?;
    state
        .store
        .update_repository_onboarding_source_delivery(
            &onboarding.id,
            &intent.id,
            "observer_dispatched",
            None,
            actor,
            reason,
        )
        .await?;
    if let Err(error) = state
        .worker
        .dispatch_source_delivery_observation(SourceDeliveryObservationRequest {
            source_delivery_intent_id: intent.id.clone(),
            execution_id,
        })
        .await
    {
        state
            .store
            .update_source_delivery_intent(
                &intent.id,
                dispatched.state_version,
                &intent.status,
                None,
                None,
                None,
                None,
                None,
                "controller:repo-mode",
                "onboarding source observer dispatch failed",
            )
            .await?;
        state
            .store
            .update_repository_onboarding_source_delivery(
                &onboarding.id,
                &intent.id,
                "waiting_external",
                None,
                "controller:repo-mode",
                "onboarding source observer dispatch failed",
            )
            .await?;
        tracing::warn!(onboarding_id=%onboarding.id, intent_id=%intent.id, %error, "onboarding source observer dispatch failed");
        return Err(ApiError::unavailable(
            "onboarding source observer dispatch failed",
        ));
    }
    Ok(())
}

async fn start_repository_onboarding_proposer(
    state: &AppState,
    onboarding: &StoredRepositoryOnboarding,
    actor: &str,
    reason: &str,
) -> Result<(), ApiError> {
    if !state.worker.supports_remote_workspace() {
        return Err(ApiError::unavailable(
            "repository onboarding proposer requires kubernetes_job worker mode",
        ));
    }
    let repository = state
        .store
        .get_repository(&onboarding.repository_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository", &onboarding.repository_id))?;
    if !state.worker.source_reader_available()
        || !state
            .worker
            .source_reader_allows_repository(&repository.canonical_url)
    {
        return Err(ApiError::conflict(
            "repository onboarding proposer requires the isolated source-reader allowlist",
        ));
    }
    let discovery_id = onboarding
        .current_discovery_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("onboarding has no deterministic discovery"))?;
    let discovery = state
        .store
        .get_repository_discovery(discovery_id)
        .await?
        .filter(|discovery| {
            discovery.status == "succeeded"
                && discovery.source_commit == onboarding.registered_commit
                && discovery.resolved_commit.as_deref()
                    == Some(onboarding.registered_commit.as_str())
        })
        .ok_or_else(|| ApiError::conflict("current deterministic discovery is unavailable"))?;
    let discovery_hash = discovery
        .content_hash
        .clone()
        .ok_or_else(|| ApiError::conflict("deterministic discovery has no content hash"))?;
    let inventory = discovery
        .inventory_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("deterministic discovery inventory is unavailable"))?;
    let model = state
        .worker
        .config_json()
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unconfigured")
        .to_string();
    let profile =
        pharness_core::compiled_agent_profiles(&model, pharness_runhost::SYSTEM_PROMPT_VERSION)
            .into_iter()
            .find(|profile| profile.id == "repository-onboarding-proposer")
            .ok_or_else(|| {
                ApiError::internal("compiled onboarding proposer profile is unavailable")
            })?;
    let bounded_discovery = json!({
        "id":discovery.id,
        "hash":discovery_hash,
        "repository":inventory.get("repository"),
        "contract":inventory.get("contract"),
        "language_indicators":inventory.get("language_indicators"),
        "dependency_candidates":inventory.get("dependency_candidates"),
        "command_candidates":inventory.get("command_candidates"),
        "root_candidates":inventory.get("root_candidates"),
        "automation_references":inventory.get("automation_references"),
        "conflicts":inventory.get("conflicts"),
        "blockers":inventory.get("blockers"),
        "limits":inventory.get("limits"),
    });
    let context = json!({
        "schema_version":pharness_core::AGENT_CONTEXT_SCHEMA,
        "subject":{"kind":"repository_onboarding","id":onboarding.id},
        "intent":"Propose the canonical RepositoryContract and bounded instructions from deterministic discovery and exact read-only repository evidence.",
        "pinned_repository":{"id":repository.id,"url":repository.canonical_url,"default_branch":repository.default_branch,"source_commit":onboarding.registered_commit},
        "discovery":bounded_discovery,
        "policies":{"allowed_source_changes":[".pharness/repository.yaml",".pharness/instructions.md","remove .pharness/project.yaml"],"dependency_lock_generation":false,"agent_network":"denied"},
        "remaining_budgets":profile.budget,
    });
    let estimated_tokens = context.to_string().len() / 4;
    if estimated_tokens > 16_000 {
        return Err(ApiError::conflict(
            "mandatory onboarding context exceeds the 16,000-token context limit",
        ));
    }
    let run_id = RunId::new(new_prefixed_id("run"));
    let session_id = SessionId::new(new_prefixed_id("ses"));
    let branch = format!("pharness/onboarding/{}", onboarding.id);
    let source = pharness_runhost::WorkspaceSourceSpec {
        workspace_id: format!("onboarding-{}", onboarding.id),
        source_repo: repository.canonical_url.clone(),
        source_ref: repository.default_branch.clone(),
        source_commit: Some(onboarding.registered_commit.clone()),
        branch: branch.clone(),
        resolved_commit: Some(onboarding.registered_commit.clone()),
    };
    source
        .validate()
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    state
        .workspace
        .remote_source_allowed(&source)
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    let cwd = state.worker.effective_cwd("/workspace");
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: format!("Repository onboarding proposer: {}", repository.external_id),
            cwd: cwd.clone(),
        })
        .await?;
    let scope = RunScope {
        run_id: Some(run_id.to_string()),
        repo: Some(repository.canonical_url.clone()),
        branch: Some(branch),
        ..RunScope::default()
    };
    let run = state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: "Submit one bounded Repository onboarding proposal. Treat discovery facts as authoritative, inspect only what is needed, and do not modify the checkout.".into(),
            cwd: cwd.clone(),
            max_turns: profile.budget.initial_turns,
            initial_status: "queued".into(),
            execution_target_json: json!({
                "kind":state.worker.execution_target_kind(),
                "onboarding":{"onboarding_id":onboarding.id,"discovery_id":discovery.id,"discovery_hash":discovery_hash},
                "agent_profile":profile,
                "agent_context":context,
                "workspace_source":source,
                "run_scope":scope.to_optional_json(),
                "run_budget":profile.budget,
            }),
        })
        .await?;
    let run = state
        .store
        .set_run_budget(
            &run.id,
            &profile.budget,
            &RunBudgetConsumption {
                allowed_turns: profile.budget.initial_turns,
                allowed_tokens: profile.budget.initial_tokens,
                ..RunBudgetConsumption::default()
            },
        )
        .await?;
    let run = state.store.set_run_origin(&run.id, "controller").await?;
    let run = state
        .store
        .set_run_created_by(&run.id, Some(actor.into()))
        .await?;
    state
        .store
        .start_repository_onboarding_proposer(
            &onboarding.id,
            onboarding.state_version,
            run.id.as_str(),
            &profile.profile_hash,
            actor,
            reason,
        )
        .await?;
    state
        .store
        .append_event(&AgentEvent {
            event_id: EventId::new(new_prefixed_id("evt")),
            session_id,
            run_id: run.id.clone(),
            seq: 1,
            kind: EventKind::RunQueued,
            payload: json!({"source":"repo_mode_controller","subject":"repository_onboarding","onboarding_id":onboarding.id,"discovery_id":discovery.id,"actor":actor,"reason":reason}),
        })
        .await?;
    state.worker.spawn_run(run, cwd);
    Ok(())
}

async fn repository_registration_preflight(
    state: &AppState,
    product: &StoredProduct,
    request: &RepositoryRegistrationPreflightRequest,
) -> Result<RepositoryRegistrationPreflightResponse, ApiError> {
    if !is_git_sha(&request.source_commit) {
        return Err(ApiError::bad_request(
            "source_commit must be a full 40-character Git object ID",
        ));
    }
    let parsed = parse_github_repository_url(&request.repository_url)?;
    let provider = resolve_public_github_repository(&parsed, &request.source_commit).await?;
    let canonical_url = format!("{}.git", provider.html_url.trim_end_matches('/'));
    let existing = state
        .store
        .get_repository_by_provider_identity("github", &provider.id.to_string())
        .await?;
    let binding = if let Some(repository) = &existing {
        state
            .store
            .get_repository_binding(&product.id, &repository.id)
            .await?
    } else {
        None
    };
    let mut blockers = Vec::new();
    if binding.is_some() {
        blockers.push("repository_already_bound_to_product".into());
    }
    let predicted_mutations = if blockers.is_empty() {
        vec![
            "create_or_reuse_global_repository".into(),
            "create_reviewed_whole_repository_binding".into(),
            "create_product_model_snapshot".into(),
            "create_repository_onboarding".into(),
        ]
    } else {
        Vec::new()
    };
    let material = json!({
        "schema_version": "pharness.dev/repository-registration-preflight/v1alpha1",
        "product_id": product.id,
        "product_state_version": product.state_version,
        "product_model_snapshot_id": product.current_model_snapshot_id,
        "provider": "github",
        "provider_repository_id": provider.id.to_string(),
        "external_id": provider.full_name.to_ascii_lowercase(),
        "canonical_url": canonical_url,
        "default_branch": provider.default_branch,
        "source_commit": request.source_commit.to_ascii_lowercase(),
        "commit_verified": true,
        "already_registered_globally": existing.is_some(),
        "already_bound_to_product": binding.is_some(),
        "predicted_mutations": predicted_mutations,
        "blockers": blockers,
    });
    let preflight_hash = canonical_material_hash(&material)?;
    Ok(RepositoryRegistrationPreflightResponse {
        product_id: product.id.clone(),
        provider: "github".into(),
        provider_repository_id: provider.id.to_string(),
        external_id: provider.full_name.to_ascii_lowercase(),
        canonical_url,
        default_branch: provider.default_branch,
        source_commit: request.source_commit.to_ascii_lowercase(),
        commit_verified: true,
        already_registered_globally: existing.is_some(),
        already_bound_to_product: binding.is_some(),
        predicted_mutations,
        blockers,
        preflight_hash,
    })
}

fn product_model_with_registration(
    product: &StoredProduct,
    services: &[StoredService],
    repositories: &[StoredRepository],
    bindings: &[StoredRepositoryBinding],
    draft: &StoredRepositoryDraft,
    binding_id: &str,
    binding_revision_id: &str,
) -> Value {
    let mut model = product_model_json(
        &product.id,
        &product.organization_id,
        &product.product_key,
        &product.display_name,
        &product.description,
        &product.owner_principal,
        services,
        repositories,
        bindings,
    );
    let model_repositories = model
        .get_mut("repositories")
        .and_then(Value::as_array_mut)
        .expect("product model repository list is controller-owned");
    if !model_repositories
        .iter()
        .any(|repository| repository.get("id") == Some(&Value::String(draft.id.clone())))
    {
        model_repositories.push(json!({
            "id": draft.id,
            "provider": draft.provider,
            "provider_repository_id": draft.external_id,
            "canonical_url": draft.canonical_url,
            "default_branch": draft.default_branch,
            "registered_commit": draft.registered_commit,
            "state_version": 1,
        }));
    }
    model_repositories.sort_by(|left, right| {
        left.get("id")
            .and_then(Value::as_str)
            .cmp(&right.get("id").and_then(Value::as_str))
    });
    let model_bindings = model
        .get_mut("repository_bindings")
        .and_then(Value::as_array_mut)
        .expect("product model binding list is controller-owned");
    model_bindings.push(json!({
        "id": binding_id,
        "repository_id": draft.id,
        "status": "active",
        "current_revision_id": binding_revision_id,
    }));
    model_bindings.sort_by(|left, right| {
        left.get("id")
            .and_then(Value::as_str)
            .cmp(&right.get("id").and_then(Value::as_str))
    });
    model
}

async fn find_onboarding(
    state: &AppState,
    id: &str,
) -> Result<StoredRepositoryOnboarding, ApiError> {
    state
        .store
        .get_repository_onboarding(id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository_onboarding", id))
}

fn onboarding_response(
    onboarding: StoredRepositoryOnboarding,
) -> Result<RepositoryOnboardingResponse, ApiError> {
    let state_hash = canonical_material_hash(&json!({
        "onboarding_id": onboarding.id,
        "state_version": onboarding.state_version,
        "status": onboarding.status,
        "registered_commit": onboarding.registered_commit,
        "resolved_commit": onboarding.resolved_commit,
        "current_discovery_id": onboarding.current_discovery_id,
        "current_proposal_revision": onboarding.current_proposal_revision,
        "approved_proposal_hash": onboarding.approved_proposal_hash,
        "source_delivery_intent_id": onboarding.source_delivery_intent_id,
        "contract_version_id": onboarding.contract_version_id,
        "readiness_assessment_id": onboarding.readiness_assessment_id,
        "proposer_run_id": onboarding.proposer_run_id,
        "proposer_profile_hash": onboarding.proposer_profile_hash,
        "proposer_stop_reason": onboarding.proposer_stop_reason,
        "patch_execution_id": onboarding.patch_execution_id,
        "patch_artifact_id": onboarding.patch_artifact_id,
        "patch_hash": onboarding.patch_hash,
        "validation_execution_id": onboarding.validation_execution_id,
        "validation_stop_reason": onboarding.validation_stop_reason,
    }))?;
    let action = match onboarding.status.as_str() {
        "registered" => Some(("start_discovery", Vec::new())),
        "discovery_failed" => Some(("retry_discovery", Vec::new())),
        "discovered" => Some(("start_proposer", Vec::new())),
        "proposal_failed" => Some(("retry_proposer", Vec::new())),
        "proposal_ready" => Some(("approve_proposal", Vec::new())),
        "proposal_approved" => Some(("prepare_onboarding_patch", Vec::new())),
        "patch_failed" => Some(("retry_onboarding_patch", Vec::new())),
        "delivery_ready" => Some(("authorize_onboarding_source_delivery", Vec::new())),
        "waiting_external" | "waiting_checks" | "waiting_merge" => {
            Some(("observe_onboarding_source_delivery", Vec::new()))
        }
        "merge_observed" => Some(("validate_merged_contract", Vec::new())),
        "validation_failed" => Some(("retry_merged_contract_validation", Vec::new())),
        _ => None,
    };
    let actions = action
        .into_iter()
        .map(|(id, blockers)| RepositoryOnboardingActionResponse {
            id: id.into(),
            lifecycle_stage: onboarding_stage_for_action(id).into(),
            resource: json!({
                "kind": "repository_onboarding",
                "id": onboarding.id,
                "product_id": onboarding.product_id,
                "repository_id": onboarding.repository_id,
                "source_commit": onboarding.registered_commit,
                "proposal_revision": onboarding.current_proposal_revision,
            }),
            status: if blockers.is_empty() {
                "available".into()
            } else {
                "blocked".into()
            },
            effect_class: if id == "approve_proposal" {
                "human_review".into()
            } else if matches!(id, "start_proposer" | "retry_proposer") {
                "model_execution".into()
            } else if matches!(id, "prepare_onboarding_patch" | "retry_onboarding_patch") {
                "isolated_source_materialization".into()
            } else if id == "authorize_onboarding_source_delivery" {
                "external_source_mutation".into()
            } else if id == "observe_onboarding_source_delivery" {
                "external_observation".into()
            } else {
                "isolated_read".into()
            },
            external_effect_summary: onboarding_action_effect(id, &onboarding),
            approval_requirements: if matches!(
                id,
                "approve_proposal" | "authorize_onboarding_source_delivery"
            ) {
                vec!["actor, reason, and current state hash".into()]
            } else {
                Vec::new()
            },
            expected_result: onboarding_action_result(id).into(),
            requires_confirmation: matches!(
                id,
                "approve_proposal"
                    | "start_proposer"
                    | "retry_proposer"
                    | "prepare_onboarding_patch"
                    | "retry_onboarding_patch"
                    | "authorize_onboarding_source_delivery"
                    | "observe_onboarding_source_delivery"
                    | "validate_merged_contract"
                    | "retry_merged_contract_validation"
            ),
            blockers,
            state_hash: state_hash.clone(),
        })
        .collect();
    Ok(RepositoryOnboardingResponse {
        id: onboarding.id,
        product_id: onboarding.product_id,
        repository_id: onboarding.repository_id,
        binding_id: onboarding.binding_id,
        onboarding_kind: onboarding.onboarding_kind,
        status: onboarding.status,
        registered_commit: onboarding.registered_commit,
        resolved_commit: onboarding.resolved_commit,
        current_discovery_id: onboarding.current_discovery_id,
        current_proposal_revision: onboarding.current_proposal_revision,
        approved_proposal_hash: onboarding.approved_proposal_hash,
        source_delivery_intent_id: onboarding.source_delivery_intent_id,
        contract_version_id: onboarding.contract_version_id,
        readiness_assessment_id: onboarding.readiness_assessment_id,
        proposer_run_id: onboarding.proposer_run_id,
        proposer_profile_hash: onboarding.proposer_profile_hash,
        proposer_stop_reason: onboarding.proposer_stop_reason,
        patch_execution_id: onboarding.patch_execution_id,
        patch_artifact_id: onboarding.patch_artifact_id,
        patch_hash: onboarding.patch_hash,
        validation_execution_id: onboarding.validation_execution_id,
        validation_stop_reason: onboarding.validation_stop_reason,
        state_version: onboarding.state_version,
        state_hash,
        blockers: onboarding.blockers,
        actions,
        created_at: onboarding.created_at,
        updated_at: onboarding.updated_at,
    })
}

pub(in crate::app) fn onboarding_operator_projection(
    onboarding: StoredRepositoryOnboarding,
) -> Result<Value, ApiError> {
    serde_json::to_value(onboarding_response(onboarding)?)
        .map_err(|error| ApiError::internal(error.to_string()))
}

fn onboarding_stage_for_action(action_id: &str) -> &'static str {
    match action_id {
        "start_discovery" | "retry_discovery" => "discovery",
        "start_proposer" | "retry_proposer" | "approve_proposal" => "proposal",
        "prepare_onboarding_patch" | "retry_onboarding_patch" => "contract_change",
        "authorize_onboarding_source_delivery" | "observe_onboarding_source_delivery" => {
            "source_delivery"
        }
        "validate_merged_contract" | "retry_merged_contract_validation" => "readiness",
        _ => "onboarding",
    }
}

fn onboarding_action_effect(action_id: &str, onboarding: &StoredRepositoryOnboarding) -> String {
    match action_id {
        "approve_proposal" => format!(
            "Approve onboarding proposal revision {} for Repository {}",
            onboarding.current_proposal_revision, onboarding.repository_id
        ),
        "prepare_onboarding_patch" | "retry_onboarding_patch" => format!(
            "Materialize the reviewed .pharness configuration change for Repository {} at {}",
            onboarding.repository_id, onboarding.registered_commit
        ),
        "authorize_onboarding_source_delivery" => format!(
            "Create one reviewed onboarding pull request for Repository {} at {}",
            onboarding.repository_id, onboarding.registered_commit
        ),
        "observe_onboarding_source_delivery" => format!(
            "Read pull-request and provider-check state for Repository {}",
            onboarding.repository_id
        ),
        "start_proposer" | "retry_proposer" => {
            "Run the bounded repository-onboarding proposer".into()
        }
        _ => "Advance the isolated onboarding lifecycle without changing provider state".into(),
    }
}

fn onboarding_action_result(action_id: &str) -> &'static str {
    match action_id {
        "start_discovery" | "retry_discovery" => "Immutable discovery evidence is recorded",
        "start_proposer" | "retry_proposer" => {
            "A versioned onboarding proposal is ready for review"
        }
        "approve_proposal" => {
            "The exact executable proposal is approved; Product topology suggestions remain a separate Product-model review"
        }
        "prepare_onboarding_patch" | "retry_onboarding_patch" => {
            "A bounded onboarding patch is materialized"
        }
        "authorize_onboarding_source_delivery" => {
            "The onboarding source-delivery intent is dispatched"
        }
        "observe_onboarding_source_delivery" => {
            "Provider checks and manual-merge state are refreshed"
        }
        "validate_merged_contract" | "retry_merged_contract_validation" => {
            "The merged canonical contract is validated"
        }
        _ => "The onboarding state advances",
    }
}

pub(in crate::app) async fn finalize_repository_onboarding_proposer_run(
    state: &AppState,
    run: &pharness_store::StoredRun,
) -> Result<(), ApiError> {
    let Some(onboarding_id) = run
        .execution_target_json
        .pointer("/onboarding/onboarding_id")
        .and_then(Value::as_str)
    else {
        return Ok(());
    };
    let onboarding = find_onboarding(state, onboarding_id).await?;
    if onboarding.proposer_run_id.as_deref() != Some(run.id.as_str())
        || onboarding.status != "proposal_running"
    {
        return Ok(());
    }
    let result = validate_and_store_agent_onboarding_proposal(state, run, &onboarding).await;
    if let Err(error) = result {
        tracing::warn!(onboarding_id, run_id=%run.id, ?error, "repository onboarding proposer result was rejected");
        state
            .store
            .fail_repository_onboarding_proposer(
                onboarding_id,
                run.id.as_str(),
                "onboarding proposer did not produce a controller-valid proposal",
            )
            .await?;
    }
    Ok(())
}

async fn validate_and_store_agent_onboarding_proposal(
    state: &AppState,
    run: &pharness_store::StoredRun,
    onboarding: &StoredRepositoryOnboarding,
) -> Result<(), ApiError> {
    if run.status != "completed" {
        return Err(ApiError::conflict(
            "onboarding proposer Run did not complete successfully",
        ));
    }
    let discovery_id = onboarding
        .current_discovery_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("onboarding discovery is unavailable"))?;
    let discovery = state
        .store
        .get_repository_discovery(discovery_id)
        .await?
        .filter(|discovery| discovery.status == "succeeded")
        .ok_or_else(|| ApiError::conflict("onboarding discovery is not successful"))?;
    let discovery_hash = discovery
        .content_hash
        .clone()
        .ok_or_else(|| ApiError::conflict("onboarding discovery has no content hash"))?;
    let events = state.store.list_events(&run.id).await?;
    let submitted =
        crate::worker::structured_submission_from_events(&events, "repository_onboarding_proposal")
            .ok_or_else(|| ApiError::conflict("onboarding proposer made no typed submission"))?;
    let proposal: pharness_core::RepositoryOnboardingProposal =
        serde_json::from_value(submitted.clone()).map_err(|error| {
            ApiError::conflict(format!(
                "onboarding proposer submission is invalid: {error}"
            ))
        })?;
    if proposal.schema_version != pharness_core::ONBOARDING_PROPOSAL_SCHEMA
        || proposal.discovery_id != discovery.id
        || proposal.discovery_hash != discovery_hash
        || proposal.instructions.len() > 32 * 1024
    {
        return Err(ApiError::conflict(
            "onboarding proposal does not match its exact discovery or bounded schema",
        ));
    }
    let contract: pharness_core::RepositoryContract =
        serde_json::from_value(proposal.candidate_contract.clone()).map_err(|error| {
            ApiError::conflict(format!("candidate contract is invalid: {error}"))
        })?;
    contract
        .validate_candidate()
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    if !state
        .environment_profiles
        .iter()
        .any(|profile| profile.active && profile.id == contract.environment_profile)
    {
        return Err(ApiError::conflict(
            "candidate contract selects no active EnvironmentProfile",
        ));
    }
    let value =
        serde_json::to_value(&proposal).map_err(|error| ApiError::internal(error.to_string()))?;
    state
        .store
        .create_repository_onboarding_proposal(CreateRepositoryOnboardingProposal {
            id: new_prefixed_id("rprop"),
            onboarding_id: onboarding.id.clone(),
            expected_state_version: onboarding.state_version,
            content_hash: canonical_material_hash(&value)?,
            proposal: value,
            discovery_id: discovery.id,
            discovery_hash,
            actor: "agent:repository-onboarding-proposer".into(),
            origin: "agent".into(),
        })
        .await?;
    Ok(())
}

pub(in crate::app) async fn internal_repository_discovery_context(
    State(state): State<AppState>,
    Path(discovery_id): Path<String>,
) -> Result<Json<RepositoryDiscoveryContextResponse>, ApiError> {
    let discovery = state
        .store
        .get_repository_discovery(&discovery_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository_discovery", &discovery_id))?;
    if !matches!(discovery.status.as_str(), "queued" | "running") {
        return Err(ApiError::conflict(
            "repository discovery is already terminal",
        ));
    }
    let onboarding = find_onboarding(&state, &discovery.onboarding_id).await?;
    if onboarding.current_discovery_id.as_deref() != Some(discovery_id.as_str()) {
        return Err(ApiError::conflict(
            "repository discovery is no longer current for its onboarding",
        ));
    }
    let repository = state
        .store
        .get_repository(&onboarding.repository_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository", &onboarding.repository_id))?;
    if discovery.source_commit != onboarding.registered_commit {
        return Err(ApiError::conflict(
            "repository discovery source no longer matches onboarding provenance",
        ));
    }
    Ok(Json(RepositoryDiscoveryContextResponse {
        discovery_id,
        onboarding_id: onboarding.id,
        repository_id: repository.id,
        provider: repository.provider,
        canonical_url: repository.canonical_url,
        default_branch: repository.default_branch,
        source_commit: discovery.source_commit,
        limits: pharness_core::RepositoryDiscoveryLimits::default(),
    }))
}

pub(in crate::app) async fn internal_repository_discovery_outcome(
    State(state): State<AppState>,
    Path(discovery_id): Path<String>,
    Json(request): Json<RepositoryDiscoveryOutcomeRequest>,
) -> Result<Json<Value>, ApiError> {
    match request.status.as_str() {
        "succeeded" => {
            let discovery = request
                .discovery
                .ok_or_else(|| ApiError::bad_request("succeeded discovery requires evidence"))?;
            discovery
                .verify_content_hash()
                .map_err(|error| ApiError::bad_request(error.to_string()))?;
            let stored = state
                .store
                .get_repository_discovery(&discovery_id)
                .await?
                .ok_or_else(|| ApiError::not_found("repository_discovery", &discovery_id))?;
            if discovery.repository.registered_commit != stored.source_commit
                || discovery.repository.resolved_commit != stored.source_commit
            {
                return Err(ApiError::conflict(
                    "repository discovery evidence does not match its immutable source",
                ));
            }
            let payload = serde_json::to_value(&discovery)
                .map_err(|error| ApiError::internal(error.to_string()))?;
            let completed = state
                .store
                .finish_repository_discovery(
                    &discovery_id,
                    &discovery.repository.resolved_commit,
                    &payload,
                    &discovery.content_hash,
                )
                .await?;
            Ok(Json(json!({"discovery": completed})))
        }
        "failed" => {
            let code = request.error_code.as_deref().unwrap_or("discovery_failed");
            let summary = request
                .error_summary
                .as_deref()
                .unwrap_or("isolated repository discovery failed");
            validate_required(code, "error_code", 120)?;
            validate_required(summary, "error_summary", 1_000)?;
            let failed = state
                .store
                .fail_repository_discovery(&discovery_id, code, summary)
                .await?;
            Ok(Json(json!({"discovery": failed})))
        }
        _ => Err(ApiError::bad_request(
            "discovery outcome status must be succeeded or failed",
        )),
    }
}

pub(in crate::app) async fn internal_onboarding_patch_context(
    State(state): State<AppState>,
    Path(onboarding_id): Path<String>,
    Query(query): Query<InternalOnboardingPatchQuery>,
) -> Result<Json<OnboardingPatchContextResponse>, ApiError> {
    let onboarding = find_onboarding(&state, &onboarding_id).await?;
    if onboarding.status != "patch_queued"
        || onboarding.patch_execution_id.as_deref() != Some(query.execution_id.as_str())
    {
        return Err(ApiError::conflict(
            "onboarding patch execution is no longer current",
        ));
    }
    let proposal = state
        .store
        .get_current_repository_onboarding_proposal(&onboarding.id)
        .await?
        .filter(|proposal| {
            proposal.status == "approved"
                && onboarding.approved_proposal_hash.as_deref()
                    == Some(proposal.content_hash.as_str())
        })
        .ok_or_else(|| {
            ApiError::conflict("onboarding patch requires the exact approved proposal revision")
        })?;
    let typed: pharness_core::RepositoryOnboardingProposal =
        serde_json::from_value(proposal.proposal.clone()).map_err(|error| {
            ApiError::conflict(format!("approved proposal is invalid: {error}"))
        })?;
    if typed.discovery_id != proposal.discovery_id
        || typed.discovery_hash != proposal.discovery_hash
    {
        return Err(ApiError::conflict(
            "approved proposal discovery provenance does not match",
        ));
    }
    let discovery = state
        .store
        .get_repository_discovery(&proposal.discovery_id)
        .await?
        .filter(|discovery| {
            discovery.status == "succeeded"
                && discovery.content_hash.as_deref() == Some(proposal.discovery_hash.as_str())
                && discovery.source_commit == onboarding.registered_commit
        })
        .ok_or_else(|| ApiError::conflict("approved proposal discovery is no longer current"))?;
    let repository = state
        .store
        .get_repository(&onboarding.repository_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository", &onboarding.repository_id))?;
    if !state.worker.source_reader_available()
        || !state
            .worker
            .source_reader_allows_repository(&repository.canonical_url)
    {
        return Err(ApiError::conflict(
            "onboarding patch requires the isolated source-reader allowlist",
        ));
    }
    let remove_alias = discovery
        .inventory_json
        .as_ref()
        .and_then(|value| value.pointer("/contract/alias_present"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(Json(OnboardingPatchContextResponse {
        onboarding_id,
        execution_id: query.execution_id,
        repository_id: repository.id,
        provider: repository.provider,
        canonical_url: repository.canonical_url,
        default_branch: repository.default_branch,
        source_commit: onboarding.registered_commit,
        proposal_id: proposal.id,
        proposal_hash: proposal.content_hash,
        candidate_contract: typed.candidate_contract,
        instructions: typed.instructions,
        remove_alias,
    }))
}

pub(in crate::app) async fn internal_onboarding_patch_outcome(
    State(state): State<AppState>,
    Path(onboarding_id): Path<String>,
    Json(request): Json<OnboardingPatchOutcomeRequest>,
) -> Result<Json<Value>, ApiError> {
    let onboarding = find_onboarding(&state, &onboarding_id).await?;
    let execution_id = onboarding
        .patch_execution_id
        .clone()
        .ok_or_else(|| ApiError::conflict("onboarding has no current patch execution"))?;
    if onboarding.status != "patch_queued" {
        return Err(ApiError::conflict(
            "onboarding patch execution is already terminal",
        ));
    }
    match request.status.as_str() {
        "succeeded" => {
            let patch = request
                .patch
                .filter(|patch| !patch.is_empty() && patch.len() <= 512 * 1024)
                .ok_or_else(|| {
                    ApiError::bad_request(
                        "successful onboarding patch must be nonempty and bounded",
                    )
                })?;
            let patch_hash = request
                .patch_hash
                .ok_or_else(|| ApiError::bad_request("successful onboarding patch needs a hash"))?;
            let actual_hash = format!("sha256:{:x}", Sha256::digest(patch.as_bytes()));
            if patch_hash != actual_hash {
                return Err(ApiError::conflict(
                    "onboarding patch hash does not match its bytes",
                ));
            }
            let changed_paths = onboarding_patch_paths(&patch)?;
            let mut reported_paths = request.changed_paths;
            reported_paths.sort();
            reported_paths.dedup();
            if reported_paths != changed_paths {
                return Err(ApiError::conflict(
                    "onboarding patch changed-path evidence does not match its patch",
                ));
            }
            let run_id = RunId::new(onboarding.proposer_run_id.clone().ok_or_else(|| {
                ApiError::conflict("onboarding patch has no proposer Run provenance")
            })?);
            let run = state
                .store
                .get_run(&run_id)
                .await?
                .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
            let proposal_hash = onboarding.approved_proposal_hash.clone().ok_or_else(|| {
                ApiError::conflict("onboarding patch has no approved proposal provenance")
            })?;
            let artifact = state
                .store
                .create_artifact(CreateArtifact {
                    id: new_prefixed_id("art"),
                    session_id: run.session_id,
                    run_id: Some(run.id),
                    kind: "repository_onboarding_patch".into(),
                    label: format!("Approved onboarding patch for {onboarding_id}"),
                    mime_type: Some("text/x-diff".into()),
                    path: None,
                    content_text: Some(patch),
                    content_json: Some(json!({
                        "schema_version":"pharness.dev/repository-onboarding-patch/v1alpha1",
                        "onboarding_id":onboarding_id,
                        "execution_id":execution_id,
                        "proposal_hash":proposal_hash,
                        "source_commit":onboarding.registered_commit,
                        "patch_hash":patch_hash,
                        "changed_paths":changed_paths,
                    })),
                })
                .await?;
            let updated = state
                .store
                .finish_repository_onboarding_patch(
                    &onboarding_id,
                    &execution_id,
                    &artifact.id,
                    &patch_hash,
                )
                .await?;
            Ok(Json(
                json!({"onboarding":onboarding_response(updated)?,"artifact_id":artifact.id}),
            ))
        }
        "failed" => {
            let error_code = request
                .error_code
                .as_deref()
                .unwrap_or("onboarding_patch_failed");
            if error_code.is_empty()
                || error_code.len() > 120
                || !error_code
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
            {
                return Err(ApiError::bad_request("invalid onboarding patch error code"));
            }
            let updated = state
                .store
                .fail_repository_onboarding_patch(&onboarding_id, &execution_id, error_code)
                .await?;
            Ok(Json(json!({"onboarding":onboarding_response(updated)?})))
        }
        _ => Err(ApiError::bad_request(
            "onboarding patch outcome status must be succeeded or failed",
        )),
    }
}

fn onboarding_patch_paths(patch: &str) -> Result<Vec<String>, ApiError> {
    let allowed = [
        ".pharness/instructions.md",
        ".pharness/project.yaml",
        ".pharness/repository.yaml",
    ];
    let mut paths = Vec::new();
    for line in patch.lines() {
        let Some(header) = line.strip_prefix("diff --git a/") else {
            continue;
        };
        let (left, right) = header
            .split_once(" b/")
            .ok_or_else(|| ApiError::bad_request("onboarding patch has an invalid diff header"))?;
        if !allowed.contains(&left) || !allowed.contains(&right) {
            return Err(ApiError::conflict(
                "onboarding patch modifies a path outside the onboarding contract",
            ));
        }
        paths.push(left.to_string());
        if right != left {
            paths.push(right.to_string());
        }
    }
    paths.sort();
    paths.dedup();
    if paths.is_empty() || paths.len() > allowed.len() {
        return Err(ApiError::bad_request(
            "onboarding patch has no bounded contract changes",
        ));
    }
    Ok(paths)
}

pub(in crate::app) async fn internal_onboarding_contract_validation_context(
    State(state): State<AppState>,
    Path(onboarding_id): Path<String>,
    Query(query): Query<InternalOnboardingContractValidationQuery>,
) -> Result<Json<OnboardingContractValidationContextResponse>, ApiError> {
    let onboarding = find_onboarding(&state, &onboarding_id).await?;
    if onboarding.status != "validation_queued"
        || onboarding.validation_execution_id.as_deref() != Some(query.execution_id.as_str())
    {
        return Err(ApiError::conflict(
            "merged onboarding contract validation is no longer current",
        ));
    }
    let source_commit = onboarding
        .resolved_commit
        .clone()
        .filter(|commit| is_git_sha(commit))
        .ok_or_else(|| ApiError::conflict("onboarding merge commit is unavailable"))?;
    let intent = state
        .store
        .get_source_delivery_intent(
            onboarding
                .source_delivery_intent_id
                .as_deref()
                .ok_or_else(|| ApiError::conflict("onboarding source intent is unavailable"))?,
        )
        .await?
        .filter(|intent| {
            intent.status == "merged"
                && intent
                    .merge_provenance
                    .as_ref()
                    .and_then(|value| value.get("merge_commit_sha"))
                    .and_then(Value::as_str)
                    == Some(source_commit.as_str())
        })
        .ok_or_else(|| ApiError::conflict("onboarding merge provenance is unavailable"))?;
    let proposal = state
        .store
        .get_current_repository_onboarding_proposal(&onboarding.id)
        .await?
        .filter(|proposal| {
            proposal.status == "approved"
                && intent.subject_kind == "repository_onboarding_proposal"
                && intent.subject_id == proposal.id
                && onboarding.approved_proposal_hash.as_deref()
                    == Some(proposal.content_hash.as_str())
        })
        .ok_or_else(|| ApiError::conflict("approved onboarding proposal is unavailable"))?;
    let typed: pharness_core::RepositoryOnboardingProposal =
        serde_json::from_value(proposal.proposal.clone()).map_err(|error| {
            ApiError::conflict(format!("approved proposal is invalid: {error}"))
        })?;
    let repository = state
        .store
        .get_repository(&onboarding.repository_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository", &onboarding.repository_id))?;
    if !state.worker.source_reader_available()
        || !state
            .worker
            .source_reader_allows_repository(&repository.canonical_url)
    {
        return Err(ApiError::conflict(
            "merged contract validation requires the isolated source-reader allowlist",
        ));
    }
    Ok(Json(OnboardingContractValidationContextResponse {
        onboarding_id,
        execution_id: query.execution_id,
        repository_id: repository.id,
        provider: repository.provider,
        canonical_url: repository.canonical_url,
        source_commit,
        proposal_id: proposal.id,
        proposal_hash: proposal.content_hash,
        expected_contract: typed.candidate_contract,
    }))
}

pub(in crate::app) async fn internal_onboarding_contract_validation_outcome(
    State(state): State<AppState>,
    Path(onboarding_id): Path<String>,
    Json(request): Json<OnboardingContractValidationOutcomeRequest>,
) -> Result<Json<Value>, ApiError> {
    let onboarding = find_onboarding(&state, &onboarding_id).await?;
    let execution_id = onboarding
        .validation_execution_id
        .clone()
        .filter(|_| onboarding.status == "validation_queued")
        .ok_or_else(|| ApiError::conflict("merged contract validation is already terminal"))?;
    match request.status.as_str() {
        "succeeded" => {
            if request.warnings.len() > 20
                || request.warnings.iter().any(|warning| warning.len() > 1_000)
            {
                return Err(ApiError::bad_request(
                    "merged contract validation warnings exceed their bound",
                ));
            }
            if !matches!(
                request.contract_source.as_deref(),
                Some("canonical") | Some("canonical_with_matching_alias")
            ) {
                return Err(ApiError::conflict(
                    "Repo Mode requires a canonical RepositoryContract",
                ));
            }
            let contract_value = request
                .contract
                .ok_or_else(|| ApiError::bad_request("successful validation needs a contract"))?;
            let contract: pharness_core::RepositoryContract =
                serde_json::from_value(contract_value.clone()).map_err(|error| {
                    ApiError::bad_request(format!(
                        "validated RepositoryContract is invalid: {error}"
                    ))
                })?;
            contract
                .validate_candidate()
                .map_err(|error| ApiError::bad_request(error.to_string()))?;
            if !state
                .environment_profiles
                .iter()
                .any(|profile| profile.active && profile.id == contract.environment_profile)
            {
                return Err(ApiError::conflict(
                    "validated RepositoryContract selects no active EnvironmentProfile",
                ));
            }
            let proposal = state
                .store
                .get_current_repository_onboarding_proposal(&onboarding.id)
                .await?
                .filter(|proposal| {
                    proposal.status == "approved"
                        && onboarding.approved_proposal_hash.as_deref()
                            == Some(proposal.content_hash.as_str())
                })
                .ok_or_else(|| ApiError::conflict("approved proposal is unavailable"))?;
            let typed: pharness_core::RepositoryOnboardingProposal =
                serde_json::from_value(proposal.proposal.clone()).map_err(|error| {
                    ApiError::conflict(format!("approved proposal is invalid: {error}"))
                })?;
            if typed.candidate_contract != contract_value {
                return Err(ApiError::conflict(
                    "merged RepositoryContract differs from the approved proposal",
                ));
            }
            let content_hash = request
                .contract_content_hash
                .filter(|hash| valid_prefixed_sha256(hash))
                .ok_or_else(|| {
                    ApiError::bad_request("validated RepositoryContract needs a SHA-256 hash")
                })?;
            let source_commit = onboarding
                .resolved_commit
                .clone()
                .filter(|commit| is_git_sha(commit))
                .ok_or_else(|| ApiError::conflict("onboarding merge commit is unavailable"))?;
            let intent = state
                .store
                .get_source_delivery_intent(
                    onboarding
                        .source_delivery_intent_id
                        .as_deref()
                        .ok_or_else(|| {
                            ApiError::conflict("source delivery intent is unavailable")
                        })?,
                )
                .await?
                .filter(|intent| intent.status == "merged")
                .ok_or_else(|| ApiError::conflict("source merge provenance is unavailable"))?;
            let merge_provenance = json!({
                "source_delivery_intent_id":intent.id,
                "source_delivery":intent.merge_provenance,
                "proposal":{"id":proposal.id,"hash":proposal.content_hash},
                "validation_execution_id":execution_id,
                "contract_source":request.contract_source,
                "warnings":request.warnings,
            });
            let completed = state
                .store
                .complete_repository_onboarding_contract_validation(
                    &execution_id,
                    CreateRepositoryContractVersion {
                        id: new_prefixed_id("rcontract"),
                        repository_id: onboarding.repository_id,
                        onboarding_id: onboarding.id,
                        source_commit,
                        contract: contract_value,
                        content_hash,
                        merge_provenance,
                    },
                )
                .await?;
            Ok(Json(json!({"onboarding":onboarding_response(completed)?})))
        }
        "failed" => {
            let code = request
                .error_code
                .as_deref()
                .unwrap_or("merged_contract_validation_failed");
            if code.is_empty()
                || code.len() > 120
                || !code
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
            {
                return Err(ApiError::bad_request(
                    "invalid merged contract validation error code",
                ));
            }
            let failed = state
                .store
                .fail_repository_onboarding_contract_validation(&onboarding_id, &execution_id, code)
                .await?;
            Ok(Json(json!({"onboarding":onboarding_response(failed)?})))
        }
        _ => Err(ApiError::bad_request(
            "merged contract validation status must be succeeded or failed",
        )),
    }
}

fn valid_prefixed_sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(in crate::app) async fn internal_repository_readiness_context(
    State(state): State<AppState>,
    Path(preparation_id): Path<String>,
) -> Result<Json<RepositoryReadinessPreparationContextResponse>, ApiError> {
    let preparation = state
        .store
        .get_subject_environment_preparation(&preparation_id)
        .await?
        .filter(|preparation| {
            preparation.subject_kind == "repository_readiness"
                && matches!(preparation.status.as_str(), "queued" | "running")
        })
        .ok_or_else(|| ApiError::conflict("repository readiness preparation is not current"))?;
    let workspace = state
        .store
        .get_subject_workspace(&preparation.workspace_id)
        .await?
        .filter(|workspace| {
            workspace.subject_kind == preparation.subject_kind
                && workspace.subject_id == preparation.subject_id
                && workspace.source_commit == preparation.source_commit
        })
        .ok_or_else(|| ApiError::conflict("repository readiness workspace is unavailable"))?;
    let repository_id = preparation
        .input
        .get("repository_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("readiness input has no Repository identity"))?;
    let repository = state
        .store
        .get_repository(repository_id)
        .await?
        .filter(|repository| {
            repository.canonical_url == workspace.source_repo
                && repository.default_branch == workspace.source_ref
                && repository.registered_commit == preparation.source_commit
        })
        .ok_or_else(|| ApiError::conflict("readiness Repository provenance changed"))?;
    let contract_version_id = preparation
        .input
        .get("contract_version_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("readiness input has no contract version"))?;
    let version = state
        .store
        .get_repository_contract_version(contract_version_id)
        .await?
        .filter(|version| {
            version.repository_id == repository.id
                && version.source_commit == preparation.source_commit
                && preparation
                    .input
                    .get("contract_hash")
                    .and_then(Value::as_str)
                    == Some(version.content_hash.as_str())
        })
        .ok_or_else(|| ApiError::conflict("readiness RepositoryContract provenance changed"))?;
    let profile = state
        .environment_profiles
        .iter()
        .find(|profile| {
            profile.active
                && profile.id == preparation.environment_profile_id
                && profile
                    .repository_allowlist
                    .contains(&repository.canonical_url)
        })
        .ok_or_else(|| ApiError::conflict("readiness EnvironmentProfile is unavailable"))?;
    if preparation
        .input
        .get("environment_profile_revision")
        .and_then(Value::as_str)
        != Some(profile.revision.as_str())
        || preparation
            .input
            .get("runner_image")
            .and_then(Value::as_str)
            != Some(profile.image.as_str())
    {
        return Err(ApiError::conflict(
            "readiness EnvironmentProfile provenance changed",
        ));
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .as_millis();
    let source_reader_evidence = preparation
        .input
        .pointer("/capability_evidence/source_reader/id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("readiness input has no source-reader evidence"))?;
    state
        .store
        .latest_capability_verification_for_repository("source_reader", &repository.canonical_url)
        .await?
        .filter(|verification| {
            verification.id == source_reader_evidence
                && verification.status == "available"
                && verification.repository.as_deref() == Some(repository.canonical_url.as_str())
                && verification
                    .expires_at
                    .parse::<u128>()
                    .is_ok_and(|expiry| expiry > now)
        })
        .ok_or_else(|| {
            ApiError::conflict("readiness source-reader evidence is stale, expired, or superseded")
        })?;
    let profile_capability = format!("environment_profile:{}", profile.id);
    let profile_evidence = preparation
        .input
        .pointer("/capability_evidence/environment_profile/id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("readiness input has no runner-profile evidence"))?;
    state
        .store
        .latest_capability_verification(&profile_capability)
        .await?
        .filter(|verification| {
            verification.id == profile_evidence
                && verification.status == "available"
                && verification
                    .expires_at
                    .parse::<u128>()
                    .is_ok_and(|expiry| expiry > now)
        })
        .ok_or_else(|| {
            ApiError::conflict("readiness runner-profile evidence is stale, expired, or superseded")
        })?;
    Ok(Json(RepositoryReadinessPreparationContextResponse {
        preparation_id,
        workspace_id: workspace.id,
        repository_id: repository.id,
        provider: repository.provider,
        canonical_url: repository.canonical_url,
        default_branch: repository.default_branch,
        source_commit: preparation.source_commit,
        contract_version_id: version.id,
        contract_content_hash: version.content_hash,
        contract: version.contract,
        environment_profile_id: profile.id.clone(),
    }))
}

pub(in crate::app) async fn internal_repository_readiness_outcome(
    State(state): State<AppState>,
    Path(preparation_id): Path<String>,
    Json(request): Json<RepositoryReadinessPreparationOutcomeRequest>,
) -> Result<Json<Value>, ApiError> {
    let preparation = state
        .store
        .get_subject_environment_preparation(&preparation_id)
        .await?
        .filter(|preparation| {
            preparation.subject_kind == "repository_readiness"
                && matches!(preparation.status.as_str(), "queued" | "running")
        })
        .ok_or_else(|| {
            ApiError::conflict("repository readiness preparation is already terminal")
        })?;
    let repository_id = preparation
        .input
        .get("repository_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("readiness input has no Repository identity"))?
        .to_string();
    let repository = state
        .store
        .get_repository(&repository_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository", &repository_id))?;
    let version_id = preparation
        .input
        .get("contract_version_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("readiness input has no contract version"))?
        .to_string();
    let version = state
        .store
        .get_repository_contract_version(&version_id)
        .await?
        .filter(|version| {
            version.repository_id == repository_id
                && version.source_commit == preparation.source_commit
        })
        .ok_or_else(|| ApiError::conflict("readiness contract version is unavailable"))?;
    let profile = state
        .environment_profiles
        .iter()
        .find(|profile| profile.active && profile.id == preparation.environment_profile_id)
        .cloned()
        .ok_or_else(|| ApiError::conflict("readiness EnvironmentProfile is unavailable"))?;
    let (
        status,
        snapshot,
        contract,
        contract_hash,
        acceptance_results,
        warnings,
        blockers,
        checks,
        error_code,
    ) = if request.status == "succeeded" {
        let resolved = request
            .resolved_commit
            .as_deref()
            .filter(|commit| *commit == preparation.source_commit)
            .ok_or_else(|| ApiError::conflict("readiness checkout resolved a different commit"))?;
        let contract_value = request
            .repository_contract
            .clone()
            .ok_or_else(|| ApiError::conflict("successful readiness has no RepositoryContract"))?;
        if contract_value != version.contract
            || request.repository_contract_hash.as_deref() != Some(version.content_hash.as_str())
        {
            return Err(ApiError::conflict(
                "readiness RepositoryContract evidence changed",
            ));
        }
        let snapshot_value = request
            .environment_snapshot
            .clone()
            .ok_or_else(|| ApiError::conflict("successful readiness has no EnvironmentSnapshot"))?;
        let token = state.worker_token.as_deref().ok_or_else(|| {
            ApiError::conflict("worker token is unavailable for readiness verification")
        })?;
        if !request
            .snapshot_signature
            .as_deref()
            .is_some_and(|signature| {
                super::runs::verify_environment_snapshot(token, &snapshot_value, signature)
            })
        {
            return Err(ApiError::conflict(
                "repository readiness snapshot signature is invalid",
            ));
        }
        let typed_snapshot: pharness_core::EnvironmentSnapshot =
            serde_json::from_value(snapshot_value.clone()).map_err(|error| {
                ApiError::conflict(format!("readiness EnvironmentSnapshot is invalid: {error}"))
            })?;
        let contract: pharness_core::RepositoryContract =
            serde_json::from_value(contract_value.clone()).map_err(|error| {
                ApiError::conflict(format!("readiness RepositoryContract is invalid: {error}"))
            })?;
        if typed_snapshot.source_sha != resolved
            || typed_snapshot.manifest_sha256 != version.content_hash
            || typed_snapshot.dependency_lock_sha256 != contract.dependency_lock.sha256
            || typed_snapshot.runner_image_digest != profile.image
            || typed_snapshot.runner_revision != profile.revision
            || contract.environment_profile != profile.id
        {
            return Err(ApiError::conflict(
                "readiness snapshot does not match the pinned source, contract, and runner",
            ));
        }
        let results = request
            .acceptance_results
            .as_array()
            .filter(|results| results.len() == contract.acceptance_commands.len())
            .ok_or_else(|| {
                ApiError::conflict("readiness did not execute every declared acceptance command")
            })?;
        let mut warning_values = Vec::new();
        for command in &contract.acceptance_commands {
            let matching = results
                .iter()
                .filter(|result| {
                    result.get("name").and_then(Value::as_str) == Some(command.name.as_str())
                        && result.get("command").and_then(Value::as_str)
                            == Some(command.command.as_str())
                })
                .collect::<Vec<_>>();
            if matching.len() != 1
                || !matches!(
                    matching[0].get("status").and_then(Value::as_str),
                    Some("passed") | Some("baseline_failed")
                )
            {
                return Err(ApiError::conflict(
                    "readiness acceptance evidence is invalid",
                ));
            }
            if matching[0].get("status").and_then(Value::as_str) == Some("baseline_failed") {
                warning_values.push(json!({
                        "code":"baseline_acceptance_failed",
                        "command_name":command.name,
                        "summary":"the declared acceptance command executed structurally but the repository baseline is failing",
                    }));
            }
        }
        (
            "succeeded",
            Some(snapshot_value),
            Some(contract_value),
            Some(version.content_hash.clone()),
            request.acceptance_results.clone(),
            Value::Array(warning_values),
            json!([]),
            json!([
                {"key":"exact_checkout","status":"passed","source_commit":preparation.source_commit},
                {"key":"canonical_contract","status":"passed","contract_version_id":version.id},
                {"key":"environment_preparation","status":"passed","profile_id":profile.id,"runner_image":profile.image},
                {"key":"declared_acceptance","status":"executed"},
            ]),
            None,
        )
    } else if request.status == "failed" {
        let code = request
            .error_code
            .as_deref()
            .unwrap_or("repository_readiness_failed");
        if code.is_empty()
            || code.len() > 120
            || !code
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(ApiError::bad_request(
                "invalid repository readiness error code",
            ));
        }
        (
            "failed",
            None,
            None,
            None,
            json!([]),
            json!([]),
            json!([{"code":code,"summary":"isolated repository coding-readiness preparation failed"}]),
            json!([{"key":"environment_preparation","status":"failed"}]),
            Some(code.to_string()),
        )
    } else {
        return Err(ApiError::bad_request(
            "repository readiness outcome must be succeeded or failed",
        ));
    };
    if !request.logs.is_array() {
        return Err(ApiError::bad_request(
            "repository readiness logs must be an array",
        ));
    }
    let completed = state
        .store
        .complete_subject_environment_preparation(CompleteSubjectEnvironmentPreparation {
            id: preparation.id.clone(),
            status: status.into(),
            resolved_commit: request.resolved_commit,
            repository_contract: contract,
            repository_contract_hash: contract_hash,
            environment_snapshot: snapshot,
            acceptance_results: acceptance_results.clone(),
            logs: request.logs,
            error_code,
        })
        .await?;
    let evidence_refs = json!([
        {"kind":"repository_contract_version","id":version.id,"hash":version.content_hash},
        {"kind":"subject_environment_preparation","id":completed.id,"input_hash":completed.input_hash},
        {
            "kind":"capability_verification",
            "capability":"source_reader",
            "id":preparation.input.pointer("/capability_evidence/source_reader/id"),
            "expires_at":preparation.input.pointer("/capability_evidence/source_reader/expires_at"),
        },
        {
            "kind":"capability_verification",
            "capability":format!("environment_profile:{}", profile.id),
            "id":preparation.input.pointer("/capability_evidence/environment_profile/id"),
            "expires_at":preparation.input.pointer("/capability_evidence/environment_profile/expires_at"),
        },
    ]);
    let material = json!({
        "schema_version":"pharness.dev/repository-readiness/v1alpha1",
        "input":preparation.input,
        "contract_status":"ready",
        "coding_status":if status == "succeeded" {"ready"} else {"blocked"},
        "checks":checks,
        "blockers":blockers,
        "warnings":warnings,
        "acceptance_results":acceptance_results,
        "evidence_refs":evidence_refs,
    });
    let assessment = state
        .store
        .create_repository_readiness_assessment(CreateRepositoryReadinessAssessment {
            id: new_prefixed_id("rready"),
            repository_id: repository_id.clone(),
            source_commit: preparation.source_commit,
            contract_version_id: Some(version.id),
            contract_hash: Some(version.content_hash),
            dependency_lock_hash: preparation
                .input
                .get("dependency_lock_hash")
                .and_then(Value::as_str)
                .map(str::to_string),
            environment_profile_id: Some(profile.id),
            environment_profile_revision: Some(profile.revision),
            runner_image_digest: profile
                .image
                .split_once('@')
                .map(|(_, digest)| digest.to_string()),
            validation_policy_version: "repo-mode-v1".into(),
            contract_status: "ready".into(),
            coding_status: if status == "succeeded" {
                "ready".into()
            } else {
                "blocked".into()
            },
            checks: material["checks"].clone(),
            blockers: material["blockers"].clone(),
            warnings: material["warnings"].clone(),
            evidence_refs: material["evidence_refs"].clone(),
            input_hash: preparation.input_hash,
            content_hash: canonical_material_hash(&material)?,
            expires_at: None,
        })
        .await?;
    let assessment_source_commit = assessment.source_commit.clone();
    Ok(Json(readiness_response(
        &state,
        &repository,
        &assessment_source_commit,
        Some(assessment),
        Vec::new(),
    )))
}

fn registered_repository_response(
    registration: RegisteredRepositoryAggregate,
) -> RepositoryResponse {
    RepositoryResponse {
        id: registration.repository.id,
        provider: registration.repository.provider,
        provider_repository_id: registration.repository.external_id,
        external_id: github_external_id(&registration.repository.canonical_url),
        canonical_url: registration.repository.canonical_url,
        default_branch: registration.repository.default_branch,
        registered_commit: registration.repository.registered_commit,
        state_version: registration.repository.state_version,
        binding_id: Some(registration.binding.id),
        binding_revision_id: Some(registration.binding_revision.id),
        onboarding_id: Some(registration.onboarding.id),
        onboarding_status: Some(registration.onboarding.status),
        created_at: registration.repository.created_at,
        updated_at: registration.repository.updated_at,
    }
}

fn repository_response(
    repository: StoredRepository,
    binding: Option<StoredRepositoryBinding>,
    onboarding: Option<pharness_store::StoredRepositoryOnboarding>,
) -> RepositoryResponse {
    RepositoryResponse {
        id: repository.id,
        provider: repository.provider,
        provider_repository_id: repository.external_id,
        external_id: github_external_id(&repository.canonical_url),
        canonical_url: repository.canonical_url,
        default_branch: repository.default_branch,
        registered_commit: repository.registered_commit,
        state_version: repository.state_version,
        binding_id: binding.as_ref().map(|binding| binding.id.clone()),
        binding_revision_id: binding.map(|binding| binding.current_revision_id),
        onboarding_id: onboarding.as_ref().map(|onboarding| onboarding.id.clone()),
        onboarding_status: onboarding.map(|onboarding| onboarding.status),
        created_at: repository.created_at,
        updated_at: repository.updated_at,
    }
}

fn parse_github_repository_url(value: &str) -> Result<String, ApiError> {
    let url = url::Url::parse(value.trim())
        .map_err(|_| ApiError::bad_request("repository_url must be a valid GitHub HTTPS URL"))?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ApiError::bad_request(
            "repository_url must be a credential-free github.com HTTPS URL without query or fragment",
        ));
    }
    let parts = url
        .path()
        .trim_matches('/')
        .strip_suffix(".git")
        .unwrap_or_else(|| url.path().trim_matches('/'))
        .split('/')
        .collect::<Vec<_>>();
    if parts.len() != 2 || parts.iter().any(|part| part.is_empty()) {
        return Err(ApiError::bad_request(
            "repository_url must name exactly one GitHub owner and repository",
        ));
    }
    if !parts.iter().all(|part| {
        part.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    }) {
        return Err(ApiError::bad_request(
            "repository_url contains an invalid GitHub owner or repository name",
        ));
    }
    Ok(format!("{}/{}", parts[0], parts[1]))
}

async fn resolve_public_github_repository(
    external_id: &str,
    source_commit: &str,
) -> Result<GitHubRepositoryResponse, ApiError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("pharness-repository-registration/1")
        .build()
        .map_err(|error| ApiError::internal(format!("GitHub client setup failed: {error}")))?;
    let github_api_url = repository_registration_github_api_url()?;
    let repository_url = format!("{github_api_url}/repos/{external_id}");
    let response = client
        .get(repository_url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| {
            ApiError::unavailable(format!("GitHub repository lookup failed: {error}"))
        })?;
    if !response.status().is_success() {
        return Err(ApiError::unavailable(format!(
            "GitHub repository lookup returned {}; public repositories require anonymous reachability and private repositories require the isolated source-reader capability",
            response.status()
        )));
    }
    let repository = response
        .json::<GitHubRepositoryResponse>()
        .await
        .map_err(|error| {
            ApiError::unavailable(format!("GitHub repository response was invalid: {error}"))
        })?;
    if !repository.full_name.eq_ignore_ascii_case(external_id) {
        return Err(ApiError::conflict(
            "GitHub resolved the repository URL to a different provider identity",
        ));
    }
    let commit_url = format!("{github_api_url}/repos/{external_id}/commits/{source_commit}");
    let response = client
        .get(commit_url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| ApiError::unavailable(format!("GitHub commit lookup failed: {error}")))?;
    if !response.status().is_success() {
        return Err(ApiError::bad_request(format!(
            "source_commit was not found in the registered repository (GitHub returned {})",
            response.status()
        )));
    }
    let commit = response
        .json::<GitHubCommitResponse>()
        .await
        .map_err(|error| {
            ApiError::unavailable(format!("GitHub commit response was invalid: {error}"))
        })?;
    if !commit.sha.eq_ignore_ascii_case(source_commit) {
        return Err(ApiError::conflict(
            "GitHub resolved source_commit to a different object ID",
        ));
    }
    Ok(repository)
}

fn repository_registration_github_api_url() -> Result<String, ApiError> {
    #[cfg(feature = "ui-e2e")]
    if let Ok(value) = std::env::var("PHARNESS_UI_E2E_GITHUB_API_URL") {
        let parsed = url::Url::parse(value.trim())
            .map_err(|_| ApiError::internal("PHARNESS_UI_E2E_GITHUB_API_URL is not a valid URL"))?;
        let loopback = parsed
            .host_str()
            .is_some_and(|host| matches!(host, "127.0.0.1" | "localhost" | "::1"));
        if parsed.scheme() != "http"
            || !loopback
            || parsed.username() != ""
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(ApiError::internal(
                "the UI E2E GitHub adapter must be a credential-free loopback HTTP URL",
            ));
        }
        return Ok(value.trim().trim_end_matches('/').to_string());
    }
    Ok("https://api.github.com".into())
}

fn github_external_id(canonical_url: &str) -> String {
    let path = canonical_url
        .strip_prefix("https://github.com/")
        .unwrap_or(canonical_url);
    path.strip_suffix(".git")
        .unwrap_or(path)
        .to_ascii_lowercase()
}

pub(super) fn ensure_repo_mode_enabled(state: &AppState) -> Result<(), ApiError> {
    if state.repo_mode.enabled {
        Ok(())
    } else {
        Err(ApiError::unavailable(
            "Repo Mode V1 is disabled for this PHarness release",
        ))
    }
}

async fn build_product_model_change_preflight(
    state: &AppState,
    product: &StoredProduct,
    services: Vec<ProductModelServiceInput>,
    bindings: Vec<ProductModelBindingInput>,
) -> Result<ProductModelChangePreflightResponse, ApiError> {
    let current = product_response(state, product.clone()).await?;
    let existing_services = state.store.list_product_services(&product.id).await?;
    let service_by_key = existing_services
        .iter()
        .map(|service| (service.service_key.as_str(), service))
        .collect::<std::collections::BTreeMap<_, _>>();
    let service_by_id = existing_services
        .iter()
        .map(|service| (service.id.as_str(), service))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut normalized_services = Vec::with_capacity(services.len());
    let mut service_ids = std::collections::BTreeSet::new();
    let mut service_keys = std::collections::BTreeSet::new();
    for service in services {
        let service_key = normalize_key(&service.service_key)?;
        validate_required(&service.display_name, "service display_name", 120)?;
        validate_required(&service.description, "service description", 2_000)?;
        validate_product_model_status(&service.status, "service")?;
        if !service_keys.insert(service_key.clone()) {
            return Err(ApiError::bad_request(format!(
                "duplicate service key {service_key}"
            )));
        }
        let id = match service.id {
            Some(id) => {
                let existing = service_by_id.get(id.as_str()).ok_or_else(|| {
                    ApiError::bad_request(format!(
                        "service id {id} is not part of Product {}",
                        product.id
                    ))
                })?;
                if existing.service_key != service_key {
                    return Err(ApiError::bad_request(format!(
                        "service id {id} cannot change its stable service key"
                    )));
                }
                id
            }
            None => service_by_key
                .get(service_key.as_str())
                .map(|service| service.id.clone())
                .unwrap_or_else(|| new_prefixed_id("svc")),
        };
        if !service_ids.insert(id.clone()) {
            return Err(ApiError::bad_request(format!("duplicate service id {id}")));
        }
        normalized_services.push(NormalizedProductModelService {
            id,
            service_key,
            display_name: service.display_name.trim().into(),
            description: service.description.trim().into(),
            status: service.status,
        });
    }
    normalized_services.sort_by(|left, right| {
        left.service_key
            .cmp(&right.service_key)
            .then_with(|| left.id.cmp(&right.id))
    });

    let service_id_by_key = normalized_services
        .iter()
        .map(|service| (service.service_key.as_str(), service.id.as_str()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let existing_bindings = state
        .store
        .list_product_repository_bindings(&product.id)
        .await?;
    if bindings.len() != existing_bindings.len() {
        return Err(ApiError::bad_request(
            "product-model changes must include every registered Repository binding",
        ));
    }
    let binding_by_repository = existing_bindings
        .iter()
        .map(|binding| (binding.repository_id.as_str(), binding))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut seen_repositories = std::collections::BTreeSet::new();
    let mut normalized_bindings = Vec::with_capacity(bindings.len());
    for binding in bindings {
        validate_product_model_status(&binding.status, "binding")?;
        if !seen_repositories.insert(binding.repository_id.clone()) {
            return Err(ApiError::bad_request(format!(
                "duplicate Repository binding {}",
                binding.repository_id
            )));
        }
        let current_binding = binding_by_repository
            .get(binding.repository_id.as_str())
            .ok_or_else(|| {
                ApiError::bad_request(format!(
                    "Repository {} is not bound to Product {}",
                    binding.repository_id, product.id
                ))
            })?;
        if binding.scopes.is_empty() {
            return Err(ApiError::bad_request(format!(
                "Repository {} must have at least one reviewed scope",
                binding.repository_id
            )));
        }
        let revision_id = new_prefixed_id("rbindrev");
        let mut normalized_scopes = Vec::with_capacity(binding.scopes.len());
        let mut unique_scopes = std::collections::BTreeSet::new();
        for scope in binding.scopes {
            validate_repository_binding_scope(&scope.path_glob, &scope.role)?;
            if scope.service_id.is_some() && scope.service_key.is_some() {
                return Err(ApiError::bad_request(
                    "scope must identify a Service by id or key, not both",
                ));
            }
            let service_id = if let Some(id) = scope.service_id {
                if !service_ids.contains(&id) {
                    return Err(ApiError::bad_request(format!(
                        "scope Service {id} is not part of the resulting Product model"
                    )));
                }
                Some(id)
            } else if let Some(key) = scope.service_key {
                let key = normalize_key(&key)?;
                Some(
                    service_id_by_key
                        .get(key.as_str())
                        .ok_or_else(|| {
                            ApiError::bad_request(format!(
                                "scope Service key {key} is not part of the resulting Product model"
                            ))
                        })?
                        .to_string(),
                )
            } else {
                None
            };
            let unique = (
                scope.path_glob.clone(),
                scope.role.clone(),
                service_id.clone(),
            );
            if !unique_scopes.insert(unique) {
                return Err(ApiError::bad_request(format!(
                    "duplicate scope {} ({})",
                    scope.path_glob, scope.role
                )));
            }
            normalized_scopes.push(NormalizedProductModelScope {
                id: new_prefixed_id("rbscope"),
                path_glob: scope.path_glob,
                role: scope.role,
                service_id,
            });
        }
        normalized_scopes.sort_by(|left, right| {
            left.path_glob
                .cmp(&right.path_glob)
                .then_with(|| left.role.cmp(&right.role))
                .then_with(|| left.service_id.cmp(&right.service_id))
        });
        normalized_bindings.push(NormalizedProductModelBinding {
            binding_id: current_binding.id.clone(),
            repository_id: binding.repository_id,
            revision_id,
            status: binding.status,
            scopes: normalized_scopes,
        });
    }
    normalized_bindings.sort_by(|left, right| left.repository_id.cmp(&right.repository_id));
    let normalized_change = NormalizedProductModelChange {
        services: normalized_services,
        bindings: normalized_bindings,
    };
    let resulting_snapshot =
        product_model_v1alpha2_json(state, product, &normalized_change).await?;
    let resulting_snapshot_hash = canonical_material_hash(&resulting_snapshot)?;
    let preflight_hash = canonical_material_hash(&json!({
        "product_id": product.id,
        "state_hash": current.state_hash,
        "normalized_change": normalized_change,
        "resulting_snapshot_hash": resulting_snapshot_hash,
    }))?;
    Ok(ProductModelChangePreflightResponse {
        product_id: product.id.clone(),
        state_hash: current.state_hash,
        normalized_change,
        resulting_snapshot,
        resulting_snapshot_hash,
        preflight_hash,
        predicted_mutations: vec![
            "create or update the reviewed Service definitions".into(),
            "create immutable typed Repository binding revisions".into(),
            "create a pharness.dev/product-model/v1alpha2 snapshot".into(),
        ],
    })
}

async fn validate_normalized_product_model_change(
    state: &AppState,
    product: &StoredProduct,
    change: &NormalizedProductModelChange,
) -> Result<(), ApiError> {
    let inputs = change
        .services
        .iter()
        .map(|service| ProductModelServiceInput {
            id: Some(service.id.clone()),
            service_key: service.service_key.clone(),
            display_name: service.display_name.clone(),
            description: service.description.clone(),
            status: service.status.clone(),
        })
        .collect::<Vec<_>>();
    let existing_service_ids = state
        .store
        .list_product_services(&product.id)
        .await?
        .into_iter()
        .map(|service| service.id)
        .collect::<std::collections::BTreeSet<_>>();
    let mut service_ids = std::collections::BTreeSet::new();
    let mut service_keys = std::collections::BTreeSet::new();
    for service in &inputs {
        let id = service.id.as_deref().unwrap_or_default();
        if !id.starts_with("svc_") || id.len() < 8 || !service_ids.insert(id.to_string()) {
            return Err(ApiError::bad_request(
                "invalid or duplicate normalized Service id",
            ));
        }
        let key = normalize_key(&service.service_key)?;
        if key != service.service_key || !service_keys.insert(key) {
            return Err(ApiError::bad_request(
                "invalid or duplicate normalized Service key",
            ));
        }
        validate_required(&service.display_name, "service display_name", 120)?;
        validate_required(&service.description, "service description", 2_000)?;
        validate_product_model_status(&service.status, "service")?;
        if !existing_service_ids.contains(id) && !id.starts_with("svc_") {
            return Err(ApiError::bad_request("invalid new Service identity"));
        }
    }
    let current_bindings = state
        .store
        .list_product_repository_bindings(&product.id)
        .await?;
    if current_bindings.len() != change.bindings.len() {
        return Err(ApiError::bad_request(
            "normalized topology must include every Repository binding",
        ));
    }
    let binding_map = current_bindings
        .iter()
        .map(|binding| (binding.id.as_str(), binding.repository_id.as_str()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut seen_bindings = std::collections::BTreeSet::new();
    for binding in &change.bindings {
        if binding_map.get(binding.binding_id.as_str()).copied()
            != Some(binding.repository_id.as_str())
            || !seen_bindings.insert(binding.binding_id.clone())
            || !binding.revision_id.starts_with("rbindrev_")
        {
            return Err(ApiError::bad_request(
                "normalized Repository binding identity is invalid",
            ));
        }
        validate_product_model_status(&binding.status, "binding")?;
        if binding.scopes.is_empty() {
            return Err(ApiError::bad_request(
                "Repository binding has no typed scopes",
            ));
        }
        let mut unique = std::collections::BTreeSet::new();
        for scope in &binding.scopes {
            if !scope.id.starts_with("rbscope_") {
                return Err(ApiError::bad_request("invalid binding scope identity"));
            }
            validate_repository_binding_scope(&scope.path_glob, &scope.role)?;
            if scope
                .service_id
                .as_ref()
                .is_some_and(|id| !service_ids.contains(id))
            {
                return Err(ApiError::bad_request(
                    "binding scope references a Service outside the Product model",
                ));
            }
            if !unique.insert((
                scope.path_glob.as_str(),
                scope.role.as_str(),
                scope.service_id.as_deref(),
            )) {
                return Err(ApiError::bad_request("duplicate normalized binding scope"));
            }
        }
    }
    Ok(())
}

async fn product_model_v1alpha2_json(
    state: &AppState,
    product: &StoredProduct,
    change: &NormalizedProductModelChange,
) -> Result<Value, ApiError> {
    let mut repositories = state.store.list_product_repositories(&product.id).await?;
    repositories.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(json!({
        "schema_version": "pharness.dev/product-model/v1alpha2",
        "organization_id": product.organization_id,
        "product": {
            "id": product.id,
            "product_key": product.product_key,
            "display_name": product.display_name,
            "description": product.description,
            "owner_principal": product.owner_principal,
            "state_version": product.state_version + 1,
        },
        "services": change.services,
        "repositories": repositories.iter().map(normalized_repository_model).collect::<Vec<_>>(),
        "repository_bindings": change.bindings,
    }))
}

fn validate_product_model_status(value: &str, resource: &str) -> Result<(), ApiError> {
    if matches!(value, "active" | "retired") {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "{resource} status must be active or retired"
        )))
    }
}

fn validate_repository_binding_scope(path_glob: &str, role: &str) -> Result<(), ApiError> {
    if !matches!(
        role,
        "source" | "delivery" | "automation" | "product_integration" | "documentation"
    ) {
        return Err(ApiError::bad_request(format!(
            "unknown Repository binding scope role {role}"
        )));
    }
    if path_glob.is_empty()
        || path_glob.len() > 256
        || path_glob.starts_with('/')
        || path_glob.contains('\\')
        || path_glob.contains("//")
        || path_glob
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return Err(ApiError::bad_request(format!(
            "unsafe repository-relative scope {path_glob:?}"
        )));
    }
    globset::Glob::new(path_glob).map_err(|error| {
        ApiError::bad_request(format!(
            "malformed Repository scope glob {path_glob:?}: {error}"
        ))
    })?;
    Ok(())
}

async fn find_product(state: &AppState, id: &str) -> Result<StoredProduct, ApiError> {
    state
        .store
        .get_product(id)
        .await?
        .ok_or_else(|| ApiError::not_found("product", id))
}

async fn product_response(
    state: &AppState,
    product: StoredProduct,
) -> Result<ProductResponse, ApiError> {
    let snapshot = state
        .store
        .get_product_model_snapshot(&product.current_model_snapshot_id)
        .await?
        .ok_or_else(|| {
            ApiError::internal(format!(
                "product {} references a missing model snapshot",
                product.id
            ))
        })?;
    let state_hash = canonical_material_hash(&json!({
        "product_id": product.id,
        "state_version": product.state_version,
        "snapshot_id": snapshot.id,
        "snapshot_hash": snapshot.content_hash,
    }))?;
    Ok(ProductResponse {
        id: product.id,
        organization_id: product.organization_id,
        product_key: product.product_key,
        display_name: product.display_name,
        description: product.description,
        owner_principal: product.owner_principal,
        state_version: product.state_version,
        state_hash,
        current_model_snapshot_id: snapshot.id,
        current_model_snapshot_hash: snapshot.content_hash,
        created_at: product.created_at,
        updated_at: product.updated_at,
    })
}

fn snapshot_response(snapshot: StoredProductModelSnapshot) -> ProductModelSnapshotResponse {
    ProductModelSnapshotResponse {
        id: snapshot.id,
        product_id: snapshot.product_id,
        version: snapshot.version,
        model: snapshot.model_json,
        content_hash: snapshot.content_hash,
        created_by: snapshot.created_by,
        creation_reason: snapshot.creation_reason,
        created_at: snapshot.created_at,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn product_model_json(
    product_id: &str,
    organization_id: &str,
    product_key: &str,
    display_name: &str,
    description: &str,
    owner_principal: &str,
    services: &[StoredService],
    repositories: &[StoredRepository],
    bindings: &[StoredRepositoryBinding],
) -> Value {
    let services = services
        .iter()
        .map(|service| {
            json!({
                "id": service.id,
                "service_key": service.service_key,
                "display_name": service.display_name,
                "description": service.description,
                "status": service.status,
            })
        })
        .collect::<Vec<_>>();
    let repositories = repositories
        .iter()
        .map(normalized_repository_model)
        .collect::<Vec<_>>();
    let bindings = bindings
        .iter()
        .map(|binding| {
            json!({
                "id": binding.id,
                "repository_id": binding.repository_id,
                "status": binding.status,
                "current_revision_id": binding.current_revision_id,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "schema_version": "pharness.dev/product-model/v1alpha1",
        "organization_id": organization_id,
        "product": {
            "id": product_id,
            "product_key": product_key,
            "display_name": display_name,
            "description": description,
            "owner_principal": owner_principal,
        },
        "services": services,
        "repositories": repositories,
        "repository_bindings": bindings,
    })
}

fn normalized_repository_model(repository: &StoredRepository) -> Value {
    json!({
        "id": repository.id,
        "provider": repository.provider,
        "provider_repository_id": repository.external_id,
        "canonical_url": repository.canonical_url,
        "default_branch": repository.default_branch,
        "registered_commit": repository.registered_commit,
        "state_version": repository.state_version,
    })
}

pub(super) fn normalize_key(value: &str) -> Result<String, ApiError> {
    let mut normalized = String::new();
    let mut separator = false;
    for character in value.trim().chars() {
        if character.is_ascii_alphanumeric() {
            normalized.push(character.to_ascii_lowercase());
            separator = false;
        } else if !normalized.is_empty() && !separator {
            normalized.push('-');
            separator = true;
        }
    }
    while normalized.ends_with('-') {
        normalized.pop();
    }
    if normalized.is_empty() || normalized.len() > 64 {
        return Err(ApiError::bad_request(
            "display_name must produce a 1-64 character product key",
        ));
    }
    Ok(normalized)
}

pub(super) fn validate_required(value: &str, field: &str, max_len: usize) -> Result<(), ApiError> {
    let value = value.trim();
    if value.is_empty() || value.len() > max_len {
        return Err(ApiError::bad_request(format!(
            "{field} must be between 1 and {max_len} characters"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_key, onboarding_patch_paths, parse_github_repository_url,
        readiness_current_state, validate_binding_scope, validate_repository_binding_scope,
    };
    use serde_json::json;

    #[test]
    fn product_keys_are_stable_and_bounded() {
        assert_eq!(normalize_key(" Orion Platform ").unwrap(), "orion-platform");
        assert_eq!(normalize_key("API / Core").unwrap(), "api-core");
        assert!(normalize_key("---").is_err());
        assert!(normalize_key(&"a".repeat(65)).is_err());
    }

    #[test]
    fn onboarding_binding_scopes_are_repository_relative_and_normalized() {
        assert!(validate_binding_scope("**").is_ok());
        assert!(validate_binding_scope("src/**").is_ok());
        for invalid in ["", "/src/**", "../src/**", "src/../tests/**", "src\\**"] {
            assert!(
                validate_binding_scope(invalid).is_err(),
                "accepted {invalid}"
            );
        }
    }

    #[test]
    fn typed_product_scopes_accept_bounded_globs_and_reject_escapes() {
        for (path, role) in [
            ("**", "source"),
            ("charts/yfinance-wrapper/**", "delivery"),
            (
                "charts/root-app/templates/yfinance-wrapper.yaml",
                "product_integration",
            ),
            ("charts/root-app/templates/*.yaml", "product_integration"),
        ] {
            assert!(
                validate_repository_binding_scope(path, role).is_ok(),
                "rejected {path}"
            );
        }
        for path in [
            "",
            "/charts/**",
            "../charts/**",
            "charts/../secret",
            "charts\\**",
        ] {
            assert!(validate_repository_binding_scope(path, "delivery").is_err());
        }
        assert!(validate_repository_binding_scope("charts/**", "cluster_owner").is_err());
    }

    #[test]
    fn onboarding_product_proposals_reject_unknown_fields() {
        let proposal = json!({
            "schema_version":pharness_core::ONBOARDING_PROPOSAL_SCHEMA,
            "discovery_id":"rdisc_test",
            "discovery_hash":"sha256:discovery",
            "candidate_contract":{},
            "instructions":"",
            "service_proposals":[{
                "service_key":"api",
                "display_name":"API",
                "description":"API service",
                "unreviewed":true
            }],
            "binding_proposals":[],
            "assumptions":[],
            "conflicts":[],
            "blockers":[],
            "readiness_forecast":{}
        });
        assert!(
            serde_json::from_value::<pharness_core::RepositoryOnboardingProposal>(proposal)
                .is_err()
        );
    }

    #[test]
    fn github_registration_urls_are_canonical_and_credential_free() {
        assert_eq!(
            parse_github_repository_url("https://github.com/Example/repo.git").unwrap(),
            "Example/repo"
        );
        assert!(parse_github_repository_url("git@github.com:Example/repo.git").is_err());
        assert!(parse_github_repository_url("https://token@github.com/Example/repo.git").is_err());
        assert!(
            parse_github_repository_url("https://github.com/Example/repo.git?ref=main").is_err()
        );
        assert!(parse_github_repository_url("https://github.com/Example/repo/extra").is_err());
    }

    #[test]
    fn onboarding_patch_paths_are_controller_bounded() {
        let patch = "diff --git a/.pharness/project.yaml b/.pharness/project.yaml\n--- a/.pharness/project.yaml\n+++ /dev/null\ndiff --git a/.pharness/repository.yaml b/.pharness/repository.yaml\n--- /dev/null\n+++ b/.pharness/repository.yaml\n";
        assert_eq!(
            onboarding_patch_paths(patch).unwrap(),
            vec![
                ".pharness/project.yaml".to_string(),
                ".pharness/repository.yaml".to_string()
            ]
        );
        let escaped = "diff --git a/src/main.rs b/src/main.rs\n";
        assert!(onboarding_patch_paths(escaped).is_err());
        let rename = "diff --git a/.pharness/project.yaml b/.pharness/repository.yaml\n";
        assert_eq!(
            onboarding_patch_paths(rename).unwrap(),
            vec![
                ".pharness/project.yaml".to_string(),
                ".pharness/repository.yaml".to_string()
            ]
        );
        let escaped_rename = "diff --git a/.pharness/project.yaml b/src/repository.yaml\n";
        assert!(onboarding_patch_paths(escaped_rename).is_err());
    }

    #[test]
    fn repository_readiness_projects_missing_stale_and_current_without_client_inference() {
        assert_eq!(
            readiness_current_state(false, &["assessment_missing".into()]),
            (
                false,
                "missing",
                vec![json!({
                    "code":"assessment_missing",
                    "summary":"no immutable readiness assessment exists for the exact source commit",
                })]
            )
        );
        assert_eq!(
            readiness_current_state(true, &["environment_profile_tuple_changed".into()]),
            (
                false,
                "stale",
                vec![json!({
                    "code":"environment_profile_tuple_changed",
                    "summary":"the EnvironmentProfile revision or immutable runner digest changed",
                })]
            )
        );
        assert_eq!(
            readiness_current_state(true, &[]),
            (true, "ready", Vec::new())
        );
    }
}
