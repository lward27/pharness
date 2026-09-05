use super::types::{
    ApplyProductModelChangeRequest, CreateProductRequest, NormalizedProductModelBinding,
    NormalizedProductModelChange, NormalizedProductModelScope, NormalizedProductModelService,
    OrganizationResponse, ProductModelBindingInput, ProductModelChangePreflightRequest,
    ProductModelChangePreflightResponse, ProductModelServiceInput, ProductModelSnapshotResponse,
    ProductResponse, ProductsResponse, ServiceResponse, UpdateProductRequest,
};
use crate::app::hashing::canonical_material_hash;
use crate::app::identifiers::new_prefixed_id;
use crate::app::repository_readiness::ensure_repo_mode_enabled;
use crate::app::{ApiError, AppState};
use axum::extract::{Path, State};
use axum::Json;
use pharness_store::{
    ApplyProductModelRevision, CreateProductAggregate, ProductModelBindingRevision,
    ProductModelServiceRevision, RepositoryBindingScope, StoredProduct, StoredProductModelSnapshot,
    StoredRepository, StoredRepositoryBinding, StoredService, UpdateProductAggregate,
};
use serde_json::{json, Value};

pub(super) async fn get_organization(
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

pub(super) async fn list_agent_profiles(
    State(state): State<AppState>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let config = state.worker.config_json();
    let model = config
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unconfigured");
    let profiles = state.compiled_agent_profiles(model);
    Ok(Json(
        json!({"agent_profiles": profiles, "count": profiles.len()}),
    ))
}

pub(super) async fn organization_overview(
    State(state): State<AppState>,
) -> Result<Json<Value>, ApiError> {
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
        crate::app::operator_experience::organization_overview_value(&state, &organization).await?,
    ))
}

pub(super) async fn list_products(
    State(state): State<AppState>,
) -> Result<Json<ProductsResponse>, ApiError> {
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

pub(super) async fn create_product(
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

pub(super) async fn get_product(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
) -> Result<Json<ProductResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let product = find_product(&state, &product_id).await?;
    Ok(Json(product_response(&state, product).await?))
}

pub(super) async fn update_product(
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

pub(super) async fn get_product_model_snapshot(
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

pub(super) async fn get_product_model(
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

pub(super) async fn preflight_product_model_change(
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

pub(super) async fn apply_product_model_change(
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

pub(super) async fn list_product_services(
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

pub(super) fn validate_repository_binding_scope(
    path_glob: &str,
    role: &str,
) -> Result<(), ApiError> {
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

pub(super) async fn find_product(state: &AppState, id: &str) -> Result<StoredProduct, ApiError> {
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

pub(in crate::app) fn normalize_key(value: &str) -> Result<String, ApiError> {
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

pub(in crate::app) fn validate_required(
    value: &str,
    field: &str,
    max_len: usize,
) -> Result<(), ApiError> {
    let value = value.trim();
    if value.is_empty() || value.len() > max_len {
        return Err(ApiError::bad_request(format!(
            "{field} must be between 1 and {max_len} characters"
        )));
    }
    Ok(())
}
