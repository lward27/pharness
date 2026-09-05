use super::model::validate_required;
use super::types::{
    CreateRepositoryReadinessRequest, RepositoryReadinessPreparationContextResponse,
    RepositoryReadinessPreparationOutcomeRequest, RepositoryReadinessQuery,
};
use crate::app::hashing::canonical_material_hash;
use crate::app::identifiers::{is_git_sha, new_prefixed_id};
use crate::app::repository_readiness::{current_readiness_mismatches, ensure_repo_mode_enabled};
use crate::app::{ApiError, AppState};
use crate::dispatch::RepositoryReadinessExecutionRequest;
use axum::extract::{Path, Query, State};
use axum::Json;
use pharness_store::{
    CompleteSubjectEnvironmentPreparation, CreateRepositoryReadinessAssessment,
    CreateSubjectEnvironmentPreparation, CreateSubjectWorkspace, StoredRepository,
};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) async fn get_repository_readiness(
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

pub(super) async fn create_repository_readiness_assessment(
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
    contract
        .validate_for_profile(&profile)
        .map_err(|error| ApiError::conflict(error.to_string()))?;
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

pub(super) fn readiness_current_state(
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
                crate::app::runs::verify_environment_snapshot(token, &snapshot_value, signature)
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
        contract
            .validate_for_profile(&profile)
            .map_err(|error| ApiError::conflict(error.to_string()))?;
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
