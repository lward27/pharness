use super::model::{find_product, product_model_json, validate_required};
use super::onboarding_state::onboarding_response;
use super::types::{
    GitHubCommitResponse, GitHubRepositoryResponse, RegisterRepositoryRequest,
    RepositoriesResponse, RepositoryRegistrationPreflightRequest,
    RepositoryRegistrationPreflightResponse, RepositoryResponse,
};
use crate::app::hashing::canonical_material_hash;
use crate::app::identifiers::{is_git_sha, new_prefixed_id};
use crate::app::repository_readiness::ensure_repo_mode_enabled;
use crate::app::{ApiError, AppState};
use axum::extract::{Path, State};
use axum::Json;
use pharness_store::{
    RegisterRepositoryAggregate, RegisteredRepositoryAggregate, StoredProduct, StoredRepository,
    StoredRepositoryBinding, StoredRepositoryDraft, StoredService,
};
use serde_json::{json, Value};

pub(super) async fn preflight_repository_registration(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
    Json(request): Json<RepositoryRegistrationPreflightRequest>,
) -> Result<Json<RepositoryRegistrationPreflightResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let product = find_product(&state, &product_id).await?;
    let response = repository_registration_preflight(&state, &product, &request).await?;
    Ok(Json(response))
}

pub(super) async fn register_repository(
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
        proposer_inference_policy: request.proposer_inference_policy.clone(),
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
    let actor = request.actor.trim().to_string();
    let reason = request.reason.trim().to_string();
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
        let onboarding_state_hash = onboarding_response(aggregate.onboarding.clone())?.state_hash;
        crate::app::inference::create_planned_selection(
            &state,
            crate::app::inference::PlannedSelectionRequest {
                subject_kind: "repository_onboarding",
                subject_id: &aggregate.onboarding.id,
                stage: pharness_core::InferenceStage::Onboarding,
                profile: &serde_json::to_value(profile)
                    .map_err(|error| ApiError::internal(error.to_string()))?,
                requested: request.proposer_inference_policy.as_ref(),
                actor: &actor,
                reason: &reason,
                state_hash: &onboarding_state_hash,
            },
        )
        .await?;
    }
    Ok(Json(registered_repository_response(aggregate)))
}

pub(super) async fn list_product_repositories(
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

pub(super) async fn get_repository(
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
    let proposer_inference = if state.inference.enabled {
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
        Some(
            crate::app::inference::preview_selection(
                state,
                pharness_core::InferenceStage::Onboarding,
                &serde_json::to_value(profile)
                    .map_err(|error| ApiError::internal(error.to_string()))?,
                request.proposer_inference_policy.as_ref(),
            )
            .await?,
        )
    } else {
        None
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
        "proposer_inference":proposer_inference,
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
        proposer_inference,
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

pub(super) fn parse_github_repository_url(value: &str) -> Result<String, ApiError> {
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

pub(super) async fn resolve_public_github_repository(
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

pub(super) fn github_external_id(canonical_url: &str) -> String {
    let path = canonical_url
        .strip_prefix("https://github.com/")
        .unwrap_or(canonical_url);
    path.strip_suffix(".git")
        .unwrap_or(path)
        .to_ascii_lowercase()
}
