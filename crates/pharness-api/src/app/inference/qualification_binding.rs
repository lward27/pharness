use super::{
    qualification_contract_for_policy, qualification_profiles, qualification_tool_constraints,
};
use crate::app::{ApiError, AppState};
use pharness_core::{
    canonical_json_sha256, AgentProfile, InferencePolicyRef, ResolvedInferenceBinding,
    RESOLVED_INFERENCE_BINDING_SCHEMA,
};
use pharness_runhost::SYSTEM_PROMPT_VERSION;
use serde_json::json;

/// Reconstruct the frozen suite binding used to qualify a profile. Its tool
/// inputs belong to the evaluation fixtures, not a particular WorkItem.
pub(in crate::app) fn qualification_binding_for_policy(
    state: &AppState,
    policy_ref: &InferencePolicyRef,
) -> Result<(&'static str, AgentProfile, ResolvedInferenceBinding), ApiError> {
    let policy = state
        .inference
        .registry
        .policy(&policy_ref.policy_id, &policy_ref.revision)
        .ok_or_else(|| ApiError::conflict("qualification policy is unavailable"))?;
    let target = state
        .inference
        .registry
        .target(&policy.target.target_id, &policy.target.revision)
        .ok_or_else(|| ApiError::conflict("qualification target is unavailable"))?;
    if policy.policy_id == "fireworks-legacy-v1" || !policy.selectable || !target.selectable {
        return Err(ApiError::conflict(
            "hosted work requires active non-legacy qualified policies",
        ));
    }
    let (suite_id, expected_profile_id) = qualification_contract_for_policy(policy)?;
    let stage = policy
        .eligible_stages
        .first()
        .copied()
        .ok_or_else(|| ApiError::conflict("policy has no supported qualification stage"))?;
    if !policy.eligible_stages.contains(&stage)
        || !policy
            .eligible_profiles
            .iter()
            .any(|profile| profile == expected_profile_id)
    {
        return Err(ApiError::conflict(
            "qualification suite, stage, profile, and policy eligibility do not match",
        ));
    }
    let profile = qualification_profiles(policy, &target.upstream_model)
        .into_iter()
        .find(|profile| profile.id == expected_profile_id)
        .ok_or_else(|| ApiError::internal("compiled qualification AgentProfile is missing"))?;
    let tools = serde_json::to_value(&profile.tools).map_err(|error| {
        ApiError::internal(format!("failed to serialize profile tools: {error}"))
    })?;
    let budget = serde_json::to_value(&profile.budget).map_err(|error| {
        ApiError::internal(format!("failed to serialize profile budget: {error}"))
    })?;
    let reliability_v2 = policy.policy_id.ends_with("-v2");
    let stage_prompt = reliability_v2
        .then(|| pharness_runhost::stage_prompt_for_profile(&profile.id))
        .flatten()
        .map(|prompt| prompt.revision_record());
    let tool_schema_hash = if reliability_v2 {
        let (acceptance_names, evidence_ids) = qualification_tool_constraints(suite_id);
        pharness_runhost::constrained_tool_schema_hash(
            &profile.tools,
            &acceptance_names,
            &evidence_ids,
        )
        .map_err(|error| {
            ApiError::internal(format!(
                "failed to hash qualification tool schemas: {error}"
            ))
        })?
    } else {
        canonical_json_sha256(&tools).map_err(|error| {
            ApiError::internal(format!("failed to hash qualification tools: {error}"))
        })?
    };
    let context_policy_hash = if reliability_v2 {
        canonical_json_sha256(&json!({
            "schema_version":"pharness.dev/repo-context-policy/v2",
            "stage":stage,
            "max_input_tokens":policy.max_input_tokens,
            "max_output_tokens":policy.max_output_tokens,
            "controller_execution_ledger":true,
            "deterministic_checkpoints":true,
        }))
        .map_err(|error| ApiError::internal(error.to_string()))?
    } else {
        String::new()
    };
    let protocol_calibration_hash = if reliability_v2 {
        canonical_json_sha256(&json!({
            "schema_version":"pharness.dev/protocol-contract/v2",
            "target_hash":target.config_hash,
            "policy_hash":policy.policy_hash,
            "tool_choice":policy.tool_choice,
            "tool_protocol":policy.tool_protocol,
            "parallel_tool_calls":false,
        }))
        .map_err(|error| ApiError::internal(error.to_string()))?
    } else {
        String::new()
    };
    let mut binding = ResolvedInferenceBinding {
        schema_version: RESOLVED_INFERENCE_BINDING_SCHEMA.into(),
        target: target.clone(),
        policy: policy.clone(),
        prompt_version: if reliability_v2 {
            pharness_runhost::RELIABILITY_V2_PROMPT_BUNDLE_VERSION.into()
        } else {
            SYSTEM_PROMPT_VERSION.into()
        },
        stage_prompt,
        base_agent_profile_hash: profile.profile_hash.clone(),
        agent_profile_hash: String::new(),
        tool_schema_hash,
        context_policy_hash,
        protocol_calibration_hash,
        profile_budget_hash: canonical_json_sha256(&budget).map_err(|error| {
            ApiError::internal(format!("failed to hash qualification budget: {error}"))
        })?,
        binding_hash: String::new(),
    };
    binding.agent_profile_hash = binding.computed_agent_profile_hash().map_err(|error| {
        ApiError::internal(format!(
            "failed to hash qualification AgentProfile: {error}"
        ))
    })?;
    binding.binding_hash = binding.computed_hash().map_err(|error| {
        ApiError::internal(format!("failed to hash qualification binding: {error}"))
    })?;
    binding.validate().map_err(|error| {
        ApiError::internal(format!("qualification binding is invalid: {error}"))
    })?;
    Ok((suite_id, profile, binding))
}
