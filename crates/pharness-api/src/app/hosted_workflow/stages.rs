use crate::app::{ApiError, AppState};
use pharness_core::{AgentProfile, InferencePolicyRef, InferenceStage};
use pharness_store::StoredRepoWorkItemMetadata;
use serde_json::{json, Value};

fn stage_for_profile(profile_id: &str) -> Result<(&'static str, InferenceStage), ApiError> {
    match profile_id {
        "repo-planner" => Ok(("plan", InferenceStage::Plan)),
        "repo-builder" => Ok(("implement", InferenceStage::Implement)),
        // Reliability V2 qualifies repair as a bounded implementation pass.
        "repo-repair" => Ok(("repair", InferenceStage::Implement)),
        "repo-test-diagnoser" => Ok(("test_diagnosis", InferenceStage::Test)),
        "repo-verifier" => Ok(("verify", InferenceStage::Verify)),
        _ => Err(ApiError::conflict("unsupported hosted engineering profile")),
    }
}

pub(in crate::app) fn validate_runtime(
    state: &AppState,
    metadata: &StoredRepoWorkItemMetadata,
) -> Result<(), ApiError> {
    let Some(policy) = &metadata.workflow_policy else {
        return Ok(());
    };
    policy.validate().map_err(ApiError::conflict)?;
    if metadata.workflow_policy_hash.as_deref()
        != Some(crate::app::hashing::canonical_material_hash(&json!(policy))?.as_str())
    {
        return Err(ApiError::conflict(
            "hosted workflow policy hash is inconsistent",
        ));
    }
    // The creation switch is intentionally absent: disabling new submissions
    // must not silently change the contract of already authorized work.
    if !state.inference.enabled || !state.repo_mode.coding_reliability_v2_enabled {
        return Err(ApiError::conflict("hosted work requires its recorded gateway and Coding Reliability V2; execution cannot fall back"));
    }
    Ok(())
}

pub(in crate::app) fn pinned_profile(
    metadata: &StoredRepoWorkItemMetadata,
    profile_id: &str,
) -> Result<Option<AgentProfile>, ApiError> {
    metadata
        .workflow_policy
        .as_ref()
        .map(|policy| {
            policy
                .agent_profiles
                .iter()
                .find(|profile| profile.id == profile_id)
                .cloned()
                .ok_or_else(|| {
                    ApiError::conflict("hosted AgentProfile is missing from the authorization")
                })
        })
        .transpose()
}

pub(in crate::app) fn pinned_policy_ref(
    metadata: &StoredRepoWorkItemMetadata,
    profile_id: &str,
) -> Result<Option<InferencePolicyRef>, ApiError> {
    let Some(policy) = &metadata.workflow_policy else {
        return Ok(None);
    };
    let (key, _) = stage_for_profile(profile_id)?;
    serde_json::from_value(policy.stage_inference[key]["policy"].clone())
        .map(Some)
        .map_err(|_| ApiError::conflict("hosted inference policy reference is invalid"))
}

pub(in crate::app) async fn validate_preview(
    state: &AppState,
    metadata: &StoredRepoWorkItemMetadata,
    profile_id: &str,
    requested: Option<&InferencePolicyRef>,
) -> Result<(), ApiError> {
    validate_runtime(state, metadata)?;
    let Some(policy) = &metadata.workflow_policy else {
        return Ok(());
    };
    let profile = pinned_profile(metadata, profile_id)?.unwrap();
    let reference = pinned_policy_ref(metadata, profile_id)?.unwrap();
    if requested.is_some_and(|requested| requested != &reference) {
        return Err(ApiError::conflict(
            "hosted stage policy cannot override the recorded authorization",
        ));
    }
    if !state
        .compiled_agent_profiles(&profile.model)
        .contains(&profile)
    {
        return Err(ApiError::conflict("hosted profile implementation changed; the saved authorization requires compatible execution"));
    }
    let (key, stage) = stage_for_profile(profile_id)?;
    let preview =
        crate::app::inference::preview_selection(state, stage, &json!(profile), Some(&reference))
            .await?;
    for field in [
        "policy",
        "policy_hash",
        "target",
        "target_hash",
        "binding_hash",
        "base_agent_profile_hash",
        "agent_profile_hash",
    ] {
        if policy.stage_inference[key].get(field).is_none()
            || preview[field] != policy.stage_inference[key][field]
        {
            return Err(ApiError::conflict(format!(
                "hosted {key} {field} changed since authorization"
            )));
        }
    }
    Ok(())
}

pub(in crate::app) async fn validate_planned(
    state: &AppState,
    metadata: &StoredRepoWorkItemMetadata,
    profile_id: &str,
) -> Result<(), ApiError> {
    validate_preview(state, metadata, profile_id, None).await?;
    let Some(policy) = &metadata.workflow_policy else {
        return Ok(());
    };
    let (key, _) = stage_for_profile(profile_id)?;
    let stored_stage = match profile_id {
        "repo-test-diagnoser" => "test",
        "repo-repair" => "implement",
        _ => key,
    };
    let native_stage = if profile_id == "repo-planner" {
        "plan"
    } else {
        profile_id
    };
    if crate::app::agent_hosts::latest_planned_execution_selection(
        state,
        "work_item",
        &metadata.work_item_id,
        native_stage,
    )
    .await?
    .is_some()
    {
        return Err(ApiError::conflict(
            "hosted work cannot execute a native-host selection",
        ));
    }
    let selection = crate::app::inference::latest_planned_selection_for_profile(
        state,
        "work_item",
        &metadata.work_item_id,
        stored_stage,
        profile_id,
    )
    .await?
    .ok_or_else(|| ApiError::conflict("hosted planned inference selection is unavailable"))?;
    if policy.stage_inference[key]["binding_hash"] != selection.binding_hash {
        return Err(ApiError::conflict(
            "planned inference selection does not match the hosted authorization",
        ));
    }
    Ok(())
}

pub(in crate::app) fn context_policy(
    metadata: &StoredRepoWorkItemMetadata,
    mut policy: Value,
) -> Value {
    if metadata.workflow_policy.is_some() {
        policy["source_only"] = json!(false);
        policy["manual_merge"] = json!(false);
        policy["automatic_source_delivery"] = json!(true);
        policy["delivery_actor"] = json!("controller");
        policy["production_approval"] = json!("before_gitops_merge");
        policy["workflow_policy_hash"] = json!(metadata.workflow_policy_hash);
    }
    policy
}

pub(in crate::app) fn bind_run(
    metadata: &StoredRepoWorkItemMetadata,
    profile_id: &str,
    mut execution: Value,
) -> Result<Value, ApiError> {
    if metadata.workflow_policy.is_some() {
        execution["hosted_workflow_policy_hash"] = json!(metadata.workflow_policy_hash);
        if profile_id != "controller-deterministic-test" {
            let profile = pinned_profile(metadata, profile_id)?.unwrap();
            execution["agent_profile"]["base_profile_hash"] = json!(profile.profile_hash);
        }
    }
    Ok(execution)
}

/// Recheck persisted authority when a prepared or resumed Run asks for model
/// access. A restart must not turn the gateway off into a direct-provider run.
pub(in crate::app) async fn validate_run(
    state: &AppState,
    run: &pharness_store::StoredRun,
) -> Result<(), ApiError> {
    let Some(work_item_id) = run
        .execution_target_json
        .pointer("/run_scope/work_item_id")
        .and_then(Value::as_str)
    else {
        return Ok(());
    };
    let Some(metadata) = state
        .store
        .get_repo_work_item_metadata(work_item_id)
        .await?
    else {
        return Ok(());
    };
    if metadata.workflow_policy.is_none() {
        return Ok(());
    }
    validate_runtime(state, &metadata)?;
    if run.execution_target_json["hosted_workflow_policy_hash"]
        != json!(metadata.workflow_policy_hash)
    {
        return Err(ApiError::conflict(
            "Run does not carry its hosted workflow authorization",
        ));
    }
    let profile_id = run
        .execution_target_json
        .pointer("/agent_profile/id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("hosted Run profile is unavailable"))?;
    if profile_id == "controller-deterministic-test" {
        if run.execution_target_json["repo_mode"]["deterministic_test"] != true
            || run.execution_target_json["inference"]["mode"] != "not_selected"
        {
            return Err(ApiError::conflict(
                "hosted deterministic Test cannot request model access",
            ));
        }
        return Ok(());
    }
    validate_planned(state, &metadata, profile_id).await?;
    let profile = pinned_profile(&metadata, profile_id)?.unwrap();
    let expected_budget = if profile_id == "repo-builder" {
        &metadata.workflow_policy.as_ref().unwrap().builder_budget
    } else {
        &profile.budget
    };
    if &run.run_budget != expected_budget
        || run.execution_target_json["run_budget"] != json!(expected_budget)
    {
        return Err(ApiError::conflict(
            "hosted Run limits differ from the recorded authorization",
        ));
    }
    let actual = &run.execution_target_json["agent_profile"];
    if actual["base_profile_hash"] != profile.profile_hash
        || actual["tools"] != json!(profile.tools)
        || actual["budget"] != json!(profile.budget)
        || run.execution_target_json["inference"]["mode"] != "gateway"
    {
        return Err(ApiError::conflict(
            "hosted Run profile or backend differs from its authorization",
        ));
    }
    let selection_id = run
        .execution_target_json
        .pointer("/inference/planned_selection_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("hosted Run has no pinned gateway selection"))?;
    let selection = state
        .store
        .get_stage_inference_selection(selection_id)
        .await?
        .ok_or_else(|| ApiError::conflict("hosted Run gateway selection is unavailable"))?;
    let (key, _) = stage_for_profile(profile_id)?;
    if selection.subject_kind != "work_item"
        || selection.subject_id != metadata.work_item_id
        || metadata.workflow_policy.as_ref().unwrap().stage_inference[key]["binding_hash"]
            != selection.binding_hash
    {
        return Err(ApiError::conflict(
            "hosted Run references a different gateway authorization",
        ));
    }
    Ok(())
}
