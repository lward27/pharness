use super::hashing::canonical_material_hash;
use super::identifiers::{is_git_sha, new_prefixed_id};
use super::{ApiError, AppState};
use crate::dispatch::RepositoryDiscoveryRequest;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pharness_store::{
    CreateProductAggregate, CreateRepositoryOnboarding, RegisterRepositoryAggregate,
    RegisteredRepositoryAggregate, StoredProduct, StoredProductModelSnapshot, StoredRepository,
    StoredRepositoryBinding, StoredRepositoryDraft, StoredRepositoryOnboarding, StoredService,
    UpdateProductAggregate,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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
            "/api/repository-onboardings/:onboarding_id/actions/:action_id/execute",
            post(execute_repository_onboarding_action),
        )
}

#[derive(Debug, Serialize)]
struct OrganizationResponse {
    id: String,
    organization_key: String,
    display_name: String,
    repo_mode_v1_enabled: bool,
}

#[derive(Debug, Serialize)]
struct OrganizationOverviewResponse {
    organization: OrganizationResponse,
    products: usize,
    repositories: usize,
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
struct ExecuteRepositoryOnboardingActionRequest {
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
    status: String,
    effect_class: String,
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
        .ensure_bootstrap_organization(&state.repo_mode.organization)
        .await?;
    Ok(Json(OrganizationResponse {
        id: organization.id,
        organization_key: organization.organization_key,
        display_name: organization.display_name,
        repo_mode_v1_enabled: state.repo_mode.enabled,
    }))
}

async fn organization_overview(
    State(state): State<AppState>,
) -> Result<Json<OrganizationOverviewResponse>, ApiError> {
    let organization = state
        .store
        .ensure_bootstrap_organization(&state.repo_mode.organization)
        .await?;
    let products = state.store.list_products(&organization.id).await?;
    let mut repository_count = 0usize;
    for product in &products {
        repository_count += state
            .store
            .list_product_repositories(&product.id)
            .await?
            .len();
    }
    Ok(Json(OrganizationOverviewResponse {
        organization: OrganizationResponse {
            id: organization.id,
            organization_key: organization.organization_key,
            display_name: organization.display_name,
            repo_mode_v1_enabled: state.repo_mode.enabled,
        },
        products: products.len(),
        repositories: repository_count,
    }))
}

async fn list_products(State(state): State<AppState>) -> Result<Json<ProductsResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    state
        .store
        .ensure_bootstrap_organization(&state.repo_mode.organization)
        .await?;
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
    let response = onboarding_response(onboarding)?;
    Ok(Json(json!({
        "onboarding": response,
        "discovery": discovery,
        "proposal": null,
        "source_delivery_intent": null,
        "readiness": null,
    })))
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
        _ => {
            return Err(ApiError::bad_request(
                "unsupported repository onboarding action",
            ))
        }
    }
    let updated = find_onboarding(&state, &onboarding_id).await?;
    Ok(Json(onboarding_response(updated)?))
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
    }))?;
    let action = match onboarding.status.as_str() {
        "registered" => Some(("start_discovery", Vec::new())),
        "discovery_failed" => Some(("retry_discovery", Vec::new())),
        _ => None,
    };
    let actions = action
        .into_iter()
        .map(|(id, blockers)| RepositoryOnboardingActionResponse {
            id: id.into(),
            status: if blockers.is_empty() {
                "available".into()
            } else {
                "blocked".into()
            },
            effect_class: "isolated_read".into(),
            requires_confirmation: false,
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
        state_version: onboarding.state_version,
        state_hash,
        blockers: onboarding.blockers,
        actions,
        created_at: onboarding.created_at,
        updated_at: onboarding.updated_at,
    })
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
    let repository_url = format!("https://api.github.com/repos/{external_id}");
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
    if repository.full_name.to_ascii_lowercase() != external_id.to_ascii_lowercase() {
        return Err(ApiError::conflict(
            "GitHub resolved the repository URL to a different provider identity",
        ));
    }
    let commit_url = format!("https://api.github.com/repos/{external_id}/commits/{source_commit}");
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
    use super::{normalize_key, parse_github_repository_url};

    #[test]
    fn product_keys_are_stable_and_bounded() {
        assert_eq!(normalize_key(" Orion Platform ").unwrap(), "orion-platform");
        assert_eq!(normalize_key("API / Core").unwrap(), "api-core");
        assert!(normalize_key("---").is_err());
        assert!(normalize_key(&"a".repeat(65)).is_err());
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
}
