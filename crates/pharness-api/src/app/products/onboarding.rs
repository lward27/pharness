use super::model::{find_product, normalize_key, product_model_json, validate_required};
use super::onboarding_policy::{
    onboarding_environment_profile_descriptors, onboarding_patch_paths, validate_binding_scope,
    validate_onboarding_contract_compatibility,
};
use super::onboarding_state::{find_onboarding, onboarding_operator_response, onboarding_response};
use super::registration::{github_external_id, resolve_public_github_repository};
use super::types::{
    CreateRepositoryOnboardingRequest, ExecuteRepositoryOnboardingActionRequest,
    PutRepositoryOnboardingProposalRequest, RepositoryOnboardingResponse,
};
use crate::app::hashing::canonical_material_hash;
use crate::app::identifiers::{is_git_sha, new_prefixed_id};
use crate::app::repository_readiness::ensure_repo_mode_enabled;
use crate::app::{ApiError, AppState};
use crate::dispatch::{
    OnboardingContractValidationRequest, OnboardingPatchRequest, RepositoryDiscoveryRequest,
    SourceDeliveryExecutionRequest, SourceDeliveryObservationRequest,
};
use axum::extract::{Path, State};
use axum::Json;
use pharness_core::{
    AgentEvent, EventId, EventKind, RunBudgetConsumption, RunId, RunScope, SessionId,
};
use pharness_store::{
    ApproveRepositoryOnboardingProposal, ApprovedOnboardingProductModelChange,
    ApprovedOnboardingService, CreateRepositoryOnboarding, CreateRepositoryOnboardingProposal,
    CreateRun, CreateSession, StoredRepositoryOnboarding, StoredRepositoryOnboardingProposal,
    StoredService,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(super) async fn create_repository_onboarding(
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
    let actor = request.actor.trim().to_string();
    let reason = request.reason.trim().to_string();
    let onboarding = state
        .store
        .create_repository_onboarding(CreateRepositoryOnboarding {
            id: new_prefixed_id("ronb"),
            product_id: request.product_id,
            repository_id,
            binding_id: binding.id,
            onboarding_kind: "refresh".into(),
            registered_commit: request.source_commit.to_ascii_lowercase(),
            actor: actor.clone(),
            reason: reason.clone(),
        })
        .await?;
    if state.inference.enabled {
        let profile = state
            .compiled_agent_profiles(
                state
                    .worker
                    .config_json()
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("unconfigured"),
            )
            .into_iter()
            .find(|profile| profile.id == "repository-onboarding-proposer")
            .ok_or_else(|| {
                ApiError::internal("compiled onboarding proposer profile is unavailable")
            })?;
        crate::app::inference::create_planned_selection(
            &state,
            crate::app::inference::PlannedSelectionRequest {
                subject_kind: "repository_onboarding",
                subject_id: &onboarding.id,
                stage: pharness_core::InferenceStage::Onboarding,
                profile: &serde_json::to_value(profile)
                    .map_err(|error| ApiError::internal(error.to_string()))?,
                requested: request.proposer_inference_policy.as_ref(),
                actor: &actor,
                reason: &reason,
                state_hash: &onboarding_response(onboarding.clone())?.state_hash,
            },
        )
        .await?;
    }
    Ok(Json(
        onboarding_operator_response(&state, onboarding).await?,
    ))
}

pub(super) async fn get_repository_onboarding(
    State(state): State<AppState>,
    Path(onboarding_id): Path<String>,
) -> Result<Json<RepositoryOnboardingResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let onboarding = find_onboarding(&state, &onboarding_id).await?;
    Ok(Json(
        onboarding_operator_response(&state, onboarding).await?,
    ))
}

pub(super) async fn get_repository_onboarding_flow(
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
    let response = onboarding_operator_response(&state, onboarding).await?;
    Ok(Json(json!({
        "onboarding": response,
        "discovery": discovery,
        "proposal": proposal,
        "proposer_run": proposer_run,
        "source_delivery_intent": source_delivery_intent,
        "readiness": readiness,
    })))
}

pub(super) async fn put_repository_onboarding_proposal(
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
    let preview = onboarding_operator_response(&state, onboarding.clone()).await?;
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
    let repository = state
        .store
        .get_repository(&onboarding.repository_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository", &onboarding.repository_id))?;
    let inventory = discovery
        .inventory_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("deterministic discovery inventory is unavailable"))?;
    validate_onboarding_contract_compatibility(
        &state.environment_profiles,
        &repository.canonical_url,
        inventory,
        &contract,
    )?;
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

pub(super) async fn execute_repository_onboarding_action(
    State(state): State<AppState>,
    Path((onboarding_id, action_id)): Path<(String, String)>,
    Json(request): Json<ExecuteRepositoryOnboardingActionRequest>,
) -> Result<Json<RepositoryOnboardingResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    validate_required(&request.actor, "actor", 200)?;
    validate_required(&request.reason, "reason", 1_000)?;
    let onboarding = find_onboarding(&state, &onboarding_id).await?;
    let preview = onboarding_operator_response(&state, onboarding.clone()).await?;
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
            let contract: pharness_core::RepositoryContract =
                serde_json::from_value(typed.candidate_contract.clone()).map_err(|error| {
                    ApiError::conflict(format!("stored candidate contract is invalid: {error}"))
                })?;
            let discovery = state
                .store
                .get_repository_discovery(&proposal.discovery_id)
                .await?
                .filter(|discovery| {
                    discovery.status == "succeeded"
                        && discovery.content_hash.as_deref()
                            == Some(proposal.discovery_hash.as_str())
                })
                .ok_or_else(|| ApiError::conflict("proposal discovery is unavailable"))?;
            let repository = state
                .store
                .get_repository(&onboarding.repository_id)
                .await?
                .ok_or_else(|| ApiError::not_found("repository", &onboarding.repository_id))?;
            validate_onboarding_contract_compatibility(
                &state.environment_profiles,
                &repository.canonical_url,
                discovery.inventory_json.as_ref().ok_or_else(|| {
                    ApiError::conflict("proposal discovery inventory is unavailable")
                })?,
                &contract,
            )?;
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
    Ok(Json(onboarding_operator_response(&state, updated).await?))
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
    let mut profile = state
        .compiled_agent_profiles(&model)
        .into_iter()
        .find(|profile| profile.id == "repository-onboarding-proposer")
        .ok_or_else(|| ApiError::internal("compiled onboarding proposer profile is unavailable"))?;
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
    let compatible_environment_profiles = onboarding_environment_profile_descriptors(
        &state.environment_profiles,
        &repository.canonical_url,
        inventory,
    );
    if compatible_environment_profiles.is_empty() {
        return Err(ApiError::conflict(
            "repository onboarding proposer has no compatible active EnvironmentProfile; configure a profile whose repository allowlist and accepted lock kind match discovery, then start a fresh onboarding",
        ));
    }
    let active_environment_profile_ids = compatible_environment_profiles
        .iter()
        .filter_map(|profile| profile.get("id").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(", ");
    let product = state
        .store
        .get_product(&onboarding.product_id)
        .await?
        .ok_or_else(|| ApiError::not_found("product", &onboarding.product_id))?;
    let product_snapshot = state
        .store
        .get_product_model_snapshot(&product.current_model_snapshot_id)
        .await?
        .ok_or_else(|| {
            ApiError::not_found("product_model_snapshot", &product.current_model_snapshot_id)
        })?;
    let context = json!({
        "schema_version":pharness_core::AGENT_CONTEXT_SCHEMA,
        "subject":{"kind":"repository_onboarding","id":onboarding.id},
        "intent":"Propose the canonical RepositoryContract and bounded instructions from deterministic discovery and exact read-only repository evidence.",
        "pinned_repository":{"id":repository.id,"url":repository.canonical_url,"default_branch":repository.default_branch,"source_commit":onboarding.registered_commit},
        "discovery":bounded_discovery,
        "product_model":{
            "snapshot_id":product_snapshot.id,
            "content_hash":product_snapshot.content_hash,
            "model":product_snapshot.model_json,
            "rule":"Reuse existing Product Services and bindings unless discovery proves a distinct reviewed component; do not invent a duplicate Service for the Repository name."
        },
        "contract_constraints":{
            "compatible_environment_profiles":compatible_environment_profiles,
            "environment_profile_rule":"candidate_contract.environment_profile and dependency_lock.kind must exactly match one compatible descriptor; generic language names, shortened aliases, and unsupported lock kinds are invalid",
        },
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
    let (inference_marker, resolved_profile) = if state.inference.enabled {
        let selection = crate::app::inference::latest_planned_selection(
            state,
            "repository_onboarding",
            &onboarding.id,
            "onboarding",
        )
        .await?
        .ok_or_else(|| {
            ApiError::conflict("Onboarding inference selection was not pinned at creation")
        })?;
        (
            crate::app::inference::execution_marker_for_selection(state, &selection),
            Some((
                selection.resolved_binding.agent_profile_hash.clone(),
                selection.resolved_binding.target.upstream_model.clone(),
            )),
        )
    } else {
        (crate::app::inference::execution_marker(state, None), None)
    };
    if let Some((profile_hash, model)) = resolved_profile {
        profile.profile_hash = profile_hash;
        profile.model = model;
    }
    let run = state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: format!(
                "Submit one bounded Repository onboarding proposal. Treat discovery and Product-model facts as authoritative, reuse existing Services, inspect only what is needed, and do not modify the checkout. candidate_contract.environment_profile must exactly equal one of these compatible IDs: {active_environment_profile_ids}."
            ),
            cwd: cwd.clone(),
            max_turns: profile.budget.initial_turns,
            initial_status: "queued".into(),
            execution_target_json: json!({
                "kind":state.worker.execution_target_kind(),
                "inference":inference_marker,
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
    let repository = state
        .store
        .get_repository(&onboarding.repository_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository", &onboarding.repository_id))?;
    validate_onboarding_contract_compatibility(
        &state.environment_profiles,
        &repository.canonical_url,
        discovery
            .inventory_json
            .as_ref()
            .ok_or_else(|| ApiError::conflict("onboarding discovery inventory is unavailable"))?,
        &contract,
    )?;
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
