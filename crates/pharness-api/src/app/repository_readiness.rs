use super::clock::current_millis;
use super::hashing::canonical_material_hash;
use super::{ApiError, AppState};
use serde_json::{json, Value};

pub(super) fn ensure_repo_mode_enabled(state: &AppState) -> Result<(), ApiError> {
    if state.repo_mode.enabled {
        Ok(())
    } else {
        Err(ApiError::unavailable(
            "Repo Mode V1 is disabled for this PHarness release",
        ))
    }
}

pub(in crate::app) async fn current_readiness_mismatches(
    state: &AppState,
    repository: &pharness_store::StoredRepository,
    source_commit: &str,
    version: &pharness_store::StoredRepositoryContractVersion,
    contract: &pharness_core::RepositoryContract,
    assessment: &pharness_store::StoredRepositoryReadinessAssessment,
) -> Result<Vec<String>, ApiError> {
    let mut mismatches = Vec::new();
    if assessment.contract_status != "ready" || assessment.coding_status != "ready" {
        mismatches.push("assessment_not_ready".into());
    }
    if assessment.source_commit != source_commit
        || assessment.contract_version_id.as_deref() != Some(version.id.as_str())
        || assessment.contract_hash.as_deref() != Some(version.content_hash.as_str())
        || assessment.dependency_lock_hash.as_deref()
            != Some(contract.dependency_lock.sha256.as_str())
        || assessment.validation_policy_version != "repo-mode-v1"
    {
        mismatches.push("contract_or_policy_tuple_changed".into());
    }
    let profile = state.environment_profiles.iter().find(|profile| {
        profile.active
            && profile.id == contract.environment_profile
            && profile
                .repository_allowlist
                .contains(&repository.canonical_url)
    });
    let Some(profile) = profile else {
        mismatches.push("environment_profile_unavailable".into());
        return Ok(mismatches);
    };
    if contract.validate_for_profile(profile).is_err() {
        mismatches.push("environment_profile_contract_mismatch".into());
    }
    let current_digest = profile.image.split_once('@').map(|(_, digest)| digest);
    if assessment.environment_profile_id.as_deref() != Some(profile.id.as_str())
        || assessment.environment_profile_revision.as_deref() != Some(profile.revision.as_str())
        || assessment.runner_image_digest.as_deref() != current_digest
    {
        mismatches.push("environment_profile_tuple_changed".into());
    }
    let now = current_millis();
    if assessment
        .expires_at
        .as_deref()
        .and_then(|expiry| expiry.parse::<u128>().ok())
        .is_some_and(|expiry| expiry <= now)
    {
        mismatches.push("assessment_expired".into());
    }
    let evidence = assessment
        .evidence_refs
        .as_array()
        .cloned()
        .unwrap_or_default();
    let source_evidence_id = evidence.iter().find_map(|entry| {
        (entry.get("kind").and_then(Value::as_str) == Some("capability_verification")
            && entry.get("capability").and_then(Value::as_str) == Some("source_reader"))
        .then(|| entry.get("id").and_then(Value::as_str))
        .flatten()
    });
    let profile_capability = format!("environment_profile:{}", profile.id);
    let profile_evidence_id = evidence.iter().find_map(|entry| {
        (entry.get("kind").and_then(Value::as_str) == Some("capability_verification")
            && entry.get("capability").and_then(Value::as_str) == Some(profile_capability.as_str()))
        .then(|| entry.get("id").and_then(Value::as_str))
        .flatten()
    });
    let source_verification = state
        .store
        .latest_capability_verification_for_repository("source_reader", &repository.canonical_url)
        .await?;
    let profile_verification = state
        .store
        .latest_capability_verification(&profile_capability)
        .await?;
    let source_current = source_verification.as_ref().is_some_and(|verification| {
        source_evidence_id == Some(verification.id.as_str())
            && verification.status == "available"
            && verification.repository.as_deref() == Some(repository.canonical_url.as_str())
            && verification
                .expires_at
                .parse::<u128>()
                .is_ok_and(|expiry| expiry > now)
    });
    let profile_current = profile_verification.as_ref().is_some_and(|verification| {
        profile_evidence_id == Some(verification.id.as_str())
            && verification.status == "available"
            && verification
                .expires_at
                .parse::<u128>()
                .is_ok_and(|expiry| expiry > now)
    });
    if !source_current {
        mismatches.push("source_reader_evidence_stale".into());
    }
    if !profile_current {
        mismatches.push("runner_profile_evidence_stale".into());
    }
    if let (Some(source_verification), Some(profile_verification)) =
        (source_verification, profile_verification)
    {
        let expected_input = json!({
            "schema_version":"pharness.dev/repository-readiness-input/v1alpha1",
            "repository_id":repository.id,
            "source_commit":source_commit,
            "contract_version_id":version.id,
            "contract_hash":version.content_hash,
            "dependency_lock_hash":contract.dependency_lock.sha256,
            "environment_profile_id":profile.id,
            "environment_profile_revision":profile.revision,
            "runner_image":profile.image,
            "validation_policy_version":"repo-mode-v1",
            "required_executables":profile.required_executables,
            "acceptance_commands":contract.acceptance_commands,
            "capability_evidence":{
                "source_reader":{"id":source_verification.id,"verified_at":source_verification.verified_at,"expires_at":source_verification.expires_at},
                "environment_profile":{"id":profile_verification.id,"verified_at":profile_verification.verified_at,"expires_at":profile_verification.expires_at},
            },
        });
        if canonical_material_hash(&expected_input)? != assessment.input_hash {
            mismatches.push("readiness_input_hash_mismatch".into());
        }
    }
    Ok(mismatches)
}
