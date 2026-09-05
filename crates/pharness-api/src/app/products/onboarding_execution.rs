use super::model::validate_required;
use super::onboarding_policy::{
    onboarding_patch_paths, valid_prefixed_sha256, validate_onboarding_contract_compatibility,
};
use super::onboarding_state::{find_onboarding, onboarding_response};
use super::types::{
    InternalOnboardingContractValidationQuery, InternalOnboardingPatchQuery,
    OnboardingContractValidationContextResponse, OnboardingContractValidationOutcomeRequest,
    OnboardingPatchContextResponse, OnboardingPatchOutcomeRequest,
    RepositoryDiscoveryContextResponse, RepositoryDiscoveryOutcomeRequest,
};
use crate::app::identifiers::{is_git_sha, new_prefixed_id};
use crate::app::{ApiError, AppState};
use axum::extract::{Path, Query, State};
use axum::Json;
use pharness_core::RunId;
use pharness_store::{
    CreateArtifact, CreateRepositoryContractVersion, StoredRepositoryOnboarding,
    StoredRepositoryOnboardingProposal,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

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
    let contract: pharness_core::RepositoryContract =
        serde_json::from_value(typed.candidate_contract.clone()).map_err(|error| {
            ApiError::conflict(format!("approved candidate contract is invalid: {error}"))
        })?;
    validate_onboarding_contract_compatibility(
        &state.environment_profiles,
        &repository.canonical_url,
        discovery.inventory_json.as_ref().ok_or_else(|| {
            ApiError::conflict("approved proposal discovery inventory is unavailable")
        })?,
        &contract,
    )?;
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
        "unchanged" => {
            let patch = request.patch.unwrap_or_default();
            let patch_hash = request.patch_hash.ok_or_else(|| {
                ApiError::bad_request("unchanged onboarding outcome needs a hash")
            })?;
            let empty_hash = format!("sha256:{:x}", Sha256::digest([]));
            if !patch.is_empty()
                || !request.changed_paths.is_empty()
                || patch_hash != empty_hash
                || onboarding.source_delivery_intent_id.is_some()
            {
                return Err(ApiError::conflict(
                    "unchanged onboarding evidence must contain an empty patch and no source delivery",
                ));
            }
            let run_id = RunId::new(onboarding.proposer_run_id.clone().ok_or_else(|| {
                ApiError::conflict("onboarding no-change evidence has no proposer Run provenance")
            })?);
            let run = state
                .store
                .get_run(&run_id)
                .await?
                .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
            let proposal_hash = onboarding.approved_proposal_hash.clone().ok_or_else(|| {
                ApiError::conflict("onboarding no-change evidence has no approved proposal")
            })?;
            let artifact = state
                .store
                .create_artifact(CreateArtifact {
                    id: new_prefixed_id("art"),
                    session_id: run.session_id,
                    run_id: Some(run.id),
                    kind: "repository_onboarding_no_change".into(),
                    label: format!("No-change onboarding evidence for {onboarding_id}"),
                    mime_type: Some("application/json".into()),
                    path: None,
                    content_text: None,
                    content_json: Some(json!({
                        "schema_version":"pharness.dev/repository-onboarding-no-change/v1alpha1",
                        "onboarding_id":onboarding_id,
                        "execution_id":execution_id,
                        "proposal_hash":proposal_hash,
                        "source_commit":onboarding.registered_commit,
                        "empty_patch_hash":patch_hash,
                        "changed_paths":[],
                    })),
                })
                .await?;
            let updated = state
                .store
                .finish_repository_onboarding_patch_unchanged(
                    &onboarding_id,
                    &execution_id,
                    &artifact.id,
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
            "onboarding patch outcome status must be succeeded, unchanged, or failed",
        )),
    }
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
    let proposal = state
        .store
        .get_current_repository_onboarding_proposal(&onboarding.id)
        .await?
        .filter(|proposal| {
            proposal.status == "approved"
                && onboarding.approved_proposal_hash.as_deref()
                    == Some(proposal.content_hash.as_str())
        })
        .ok_or_else(|| ApiError::conflict("approved onboarding proposal is unavailable"))?;
    resolve_onboarding_contract_provenance(&state, &onboarding, &proposal, &source_commit).await?;
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
            let repository = state
                .store
                .get_repository(&onboarding.repository_id)
                .await?
                .ok_or_else(|| ApiError::not_found("repository", &onboarding.repository_id))?;
            let profile = state
                .environment_profiles
                .iter()
                .find(|profile| profile.active && profile.id == contract.environment_profile)
                .ok_or_else(|| {
                    ApiError::conflict(
                        "validated RepositoryContract selects no active EnvironmentProfile",
                    )
                })?;
            contract
                .validate_for_profile(profile)
                .map_err(|error| ApiError::conflict(error.to_string()))?;
            if !profile
                .repository_allowlist
                .contains(&repository.canonical_url)
            {
                return Err(ApiError::conflict(
                    "validated RepositoryContract profile does not allow this repository",
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
            let mut merge_provenance = resolve_onboarding_contract_provenance(
                &state,
                &onboarding,
                &proposal,
                &source_commit,
            )
            .await?;
            let provenance = merge_provenance.as_object_mut().ok_or_else(|| {
                ApiError::internal("resolved onboarding provenance is not an object")
            })?;
            provenance.insert(
                "proposal".into(),
                json!({"id":proposal.id,"hash":proposal.content_hash}),
            );
            provenance.insert("validation_execution_id".into(), json!(execution_id));
            provenance.insert("contract_source".into(), json!(request.contract_source));
            provenance.insert("warnings".into(), json!(request.warnings));
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

async fn resolve_onboarding_contract_provenance(
    state: &AppState,
    onboarding: &StoredRepositoryOnboarding,
    proposal: &StoredRepositoryOnboardingProposal,
    source_commit: &str,
) -> Result<Value, ApiError> {
    if let Some(intent_id) = onboarding.source_delivery_intent_id.as_deref() {
        let intent = state
            .store
            .get_source_delivery_intent(intent_id)
            .await?
            .filter(|intent| {
                intent.status == "merged"
                    && intent.subject_kind == "repository_onboarding_proposal"
                    && intent.subject_id == proposal.id
                    && intent
                        .merge_provenance
                        .as_ref()
                        .and_then(|value| value.get("merge_commit_sha"))
                        .and_then(Value::as_str)
                        == Some(source_commit)
            })
            .ok_or_else(|| ApiError::conflict("onboarding merge provenance is unavailable"))?;
        return Ok(json!({
            "source_delivery_intent_id":intent.id,
            "source_delivery":intent.merge_provenance,
        }));
    }

    let artifact_id = onboarding.patch_artifact_id.as_deref().ok_or_else(|| {
        ApiError::conflict("onboarding source or no-change provenance is unavailable")
    })?;
    let artifact = state
        .store
        .get_artifact(artifact_id)
        .await?
        .filter(|artifact| {
            artifact.kind == "repository_onboarding_no_change"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("schema_version").and_then(Value::as_str)
                        == Some("pharness.dev/repository-onboarding-no-change/v1alpha1")
                        && content.get("onboarding_id").and_then(Value::as_str)
                            == Some(onboarding.id.as_str())
                        && content.get("execution_id").and_then(Value::as_str)
                            == onboarding.patch_execution_id.as_deref()
                        && content.get("proposal_hash").and_then(Value::as_str)
                            == Some(proposal.content_hash.as_str())
                        && content.get("source_commit").and_then(Value::as_str)
                            == Some(source_commit)
                        && content
                            .get("changed_paths")
                            .and_then(Value::as_array)
                            .is_some_and(Vec::is_empty)
                })
        })
        .ok_or_else(|| ApiError::conflict("onboarding no-change provenance is unavailable"))?;
    Ok(json!({
        "source_delivery_intent_id":Value::Null,
        "no_change_artifact_id":artifact.id,
        "no_change":artifact.content_json,
    }))
}
