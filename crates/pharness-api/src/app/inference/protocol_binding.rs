use super::ApiError;
use pharness_core::{InferencePolicyRef, InferenceRegistry, StageInferencePolicyRevision};
use pharness_store::StoredInferenceTargetVerification;
use serde_json::{json, Value};

pub(super) fn select_policy<'a>(
    registry: &'a InferenceRegistry,
    target_id: &str,
    target_revision: &str,
    requested: Option<&InferencePolicyRef>,
) -> Result<&'a StageInferencePolicyRevision, ApiError> {
    let matches_target = |policy: &&StageInferencePolicyRevision| {
        policy.target.target_id == target_id && policy.target.revision == target_revision
    };
    if let Some(reference) = requested {
        return registry
            .policy(&reference.policy_id, &reference.revision)
            .filter(matches_target)
            .ok_or_else(|| {
                ApiError::conflict("requested protocol policy does not match this target revision")
            });
    }
    let mut candidates = registry.policies.iter().filter(matches_target);
    let policy = candidates.next().ok_or_else(|| {
        ApiError::conflict("target has no compatible configured inference policy")
    })?;
    if candidates.next().is_some() {
        return Err(ApiError::conflict(
            "this target has multiple stage policies; select a policy to verify its exact protocol settings",
        ));
    }
    Ok(policy)
}

pub(super) fn policy_identity(policy: &StageInferencePolicyRevision) -> Value {
    json!({"policy_id":policy.policy_id,"revision":policy.revision,"policy_hash":policy.policy_hash})
}

/// Store ordering is newest first. A newer failed check for the same immutable
/// binding supersedes an older pass; another policy's check cannot satisfy it.
pub(super) fn latest_verification(
    verifications: Vec<StoredInferenceTargetVerification>,
    policy: &StageInferencePolicyRevision,
    registry_hash: &str,
    runtime_revision: &str,
) -> Option<StoredInferenceTargetVerification> {
    verifications.into_iter().find(|verification| {
        verification.target_id == policy.target.target_id
            && verification.target_revision == policy.target.revision
            && verification.target_hash == policy.target_hash
            && verification.config_hash == registry_hash
            && verification
                .observed_capabilities
                .get("runtime_revision")
                .and_then(Value::as_str)
                == Some(runtime_revision)
            && verification.observed_capabilities.get("policy") == Some(&policy_identity(policy))
            && verification
                .observed_capabilities
                .get("registry_hash")
                .and_then(Value::as_str)
                == Some(registry_hash)
    })
}

pub(super) fn fresh_pass(verification: &StoredInferenceTargetVerification, now: u64) -> bool {
    verification.status == "passed"
        && verification.reachability == "reachable"
        && verification.model_visible
        && verification.streaming_compatible
        && verification.tool_compatible
        && verification.sanitized_failure.is_none()
        && verification
            .observed_capabilities
            .pointer("/protocol_calibration/passed")
            .and_then(Value::as_u64)
            == Some(30)
        && verification
            .observed_capabilities
            .pointer("/protocol_calibration/required")
            .and_then(Value::as_u64)
            == Some(30)
        && verification
            .expires_at
            .parse::<u64>()
            .ok()
            .is_some_and(|expires_at| expires_at > now)
}

#[cfg(test)]
mod tests;
