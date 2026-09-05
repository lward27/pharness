//! Hosted builds consume sealed source evidence and the saved finite policy.
//! Legacy source-only delivery keeps its original artifact contract.
use crate::app::hashing::canonical_material_hash as hash;
use crate::app::hosted_controller::approval::validate_stored;
use crate::app::identifiers::is_git_sha;
use crate::app::{ApiError, AppState};
use pharness_core::hosted_sdlc::{HostedAutomaticAction, HostedSourceMergeAuthority};
use pharness_store::{SqliteStore, StoredChangeSet, StoredPipelineIntent};
use serde_json::{json, Value};

pub(in crate::app) async fn is_hosted(
    store: &SqliteStore,
    intent: &StoredPipelineIntent,
) -> Result<bool, ApiError> {
    let plan = store
        .get_work_plan(&intent.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::conflict("pipeline WorkPlan is unavailable"))?;
    let Some(item_id) = plan.work_item_id else {
        return Ok(false);
    };
    Ok(store
        .get_repo_work_item_metadata(&item_id)
        .await?
        .is_some_and(|m| m.workflow_policy.is_some()))
}

pub(in crate::app) async fn source_provenance(
    store: &SqliteStore,
    change: &StoredChangeSet,
) -> Result<Option<Value>, ApiError> {
    let Some(item_id) = change.work_item_id.as_deref() else {
        return Ok(None);
    };
    let Some(metadata) = store.get_repo_work_item_metadata(item_id).await? else {
        return Ok(None);
    };
    let Some(policy) = metadata.workflow_policy.as_ref() else {
        return Ok(None);
    };
    policy.validate().map_err(ApiError::conflict)?;
    let item = store
        .get_work_item(item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("hosted build WorkItem is unavailable"))?;
    let plan = store
        .get_work_plan_by_work_item(item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("hosted build WorkPlan is unavailable"))?;
    validate_stored(store, item_id, "approve_change_set", &change.id).await?;
    let intent = store
        .get_source_delivery_intent_by_subject("work_item_change_set", &change.id)
        .await?
        .ok_or_else(|| ApiError::conflict("hosted build has no source delivery intent"))?;
    let outcomes = store.list_effective_stage_outcomes(item_id).await?;
    let source = outcomes
        .iter()
        .find(|o| o.stage_key == "source_delivery")
        .ok_or_else(|| {
            ApiError::conflict("hosted build requires sealed source delivery evidence")
        })?;
    let provenance = intent
        .merge_provenance
        .as_ref()
        .ok_or_else(|| ApiError::conflict("hosted build has no observed merge provenance"))?;
    let proof = &provenance["hosted_merge_proof"];
    let authority: HostedSourceMergeAuthority = serde_json::from_value(proof["authority"].clone())
        .map_err(|_| ApiError::conflict("hosted build has no bound autonomous merge authority"))?;
    let operation = store
        .workflow_operation_for_source_intent(&intent.id)
        .await?
        .ok_or_else(|| ApiError::conflict("hosted build source operation is unavailable"))?;
    authority
        .validate(operation.created_at)
        .map_err(ApiError::conflict)?;
    let attempt = store
        .get_artifact(&format!("source_merge_attempt_{}", authority.execution_id))
        .await?
        .ok_or_else(|| ApiError::conflict("hosted build source merge has no admitted attempt"))?;
    let input = &source.outcome["pinned_inputs"];
    let merged = provenance["merge_commit_sha"]
        .as_str()
        .filter(|sha| is_git_sha(sha))
        .ok_or_else(|| ApiError::conflict("hosted build merge identity is invalid"))?;
    if metadata.closed_at.is_some()
        || plan.id != change.work_plan_id
        || plan.status != "approved"
        || change.status != "approved"
        || metadata.workflow_policy_hash.as_deref() != Some(&hash(&json!(policy))?)
        || metadata.workflow_policy_hash.as_deref() != Some(&authority.workflow_policy_hash)
    {
        return Err(ApiError::conflict(
            "hosted build WorkItem, workflow policy or approved ChangeSet changed",
        ));
    }
    if intent.status != "merged"
        || provenance["status"] != "succeeded"
        || proof["accepted"] != true
        || source.status != "succeeded"
        || source.outcome["status"] != "succeeded"
        || hash(&source.outcome)? != source.content_hash
        || !source.outcome["contradictions"]
            .as_array()
            .is_some_and(Vec::is_empty)
        || input["source_delivery_intent_id"] != intent.id
        || input["subject_id"] != change.id
        || input["subject_kind"] != "work_item_change_set"
        || input["merge_commit_sha"] != merged
        || input["hosted_merge_proof"] != *proof
        || input["base_commit"] != authority.base_commit_sha
        || input["approved_head_sha"] != authority.head_commit_sha
    {
        return Err(ApiError::conflict(
            "hosted build source outcome is not a matching sealed successful merge",
        ));
    }
    if operation.id != authority.operation_id
        || operation.status != "succeeded"
        || operation.work_item_id != item_id
        || operation.action != "authorize_source_delivery"
        || operation.resource_refs["action_resource"] != change.id
        || operation.resource_refs["source_merge_authority"] != json!(authority)
        || operation.resource_refs["source_merge_authority_hash"] != hash(&json!(authority))?
        || authority.work_item_id != item_id
        || authority.source_delivery_intent_id != intent.id
        || authority.change_set_material_hash != change.material_hash
    {
        return Err(ApiError::conflict(
            "hosted build merge operation or its bound authority changed",
        ));
    }
    if authority.repository != policy.delivery_binding.source_repo
        || authority.repository != item.source_repo
        || authority.repository != intent.source_repo
        || authority.base_ref != intent.base_ref
        || authority.head_branch != intent.head_branch
        || item.source_commit.as_deref() != Some(&authority.base_commit_sha)
        || intent.base_commit != authority.base_commit_sha
        || intent.repository_id != metadata.repository_id
        || intent.authorization["workflow_policy_hash"] != authority.workflow_policy_hash
        || intent.authorization["work_item_id"] != item_id
        || intent.patch_hash
            != change.change_set_json["patch"]["hash"]
                .as_str()
                .unwrap_or_default()
        || intent.patch_artifact_id.as_deref()
            != change.change_set_json["patch"]["artifact_id"].as_str()
        || intent.pull_request.as_ref().map(|p| &p["head_sha"])
            != Some(&json!(authority.head_commit_sha))
        || provenance["head_sha"] != authority.head_commit_sha
        || proof["merge_parent_shas"]
            != json!([authority.base_commit_sha, authority.head_commit_sha])
        || !proof["merge_tree_sha"].as_str().is_some_and(is_git_sha)
    {
        return Err(ApiError::conflict(
            "hosted build source repository, patch or merge ancestry changed",
        ));
    }
    if attempt.kind != "source_merge_attempt"
        || proof["attempt_id"] != attempt.id
        || proof["attempt_hash"] != json!(attempt.content_hash)
        || attempt.content_hash.is_none()
        || attempt.content_json.as_ref().map(|v| &v["authority"]) != Some(&json!(authority))
    {
        return Err(ApiError::conflict(
            "hosted build source merge admission differs from its sealed proof",
        ));
    }
    let observation_id = input["provider_check_observation_id"]
        .as_str()
        .ok_or_else(|| ApiError::conflict("hosted source observation identity is unavailable"))?;
    let observation = store
        .get_provider_check_set_observation(observation_id)
        .await?
        .ok_or_else(|| ApiError::conflict("hosted source observation is unavailable"))?;
    if observation.status != "passing"
        || !observation.authoritative_rules_succeeded
        || observation.source_delivery_intent_id != intent.id
        || observation.head_sha != authority.head_commit_sha
        || input["provider_check_observation_hash"] != observation.content_hash
        || provenance["merge_observation_id"] != observation.id
    {
        return Err(ApiError::conflict(
            "hosted build source observation differs from its sealed evidence",
        ));
    }
    Ok(Some(json!({
        "kind":"github_merged_pull_request", "immutable":true,
        "repository":authority.repository, "base_commit":authority.base_commit_sha,
        "head_commit_sha":authority.head_commit_sha, "merge_commit_sha":merged,
        "pull_request_url":authority.pull_request_url,"pull_request_number":authority.pull_request_number,
        "hosted":{
            "workflow_policy_hash":authority.workflow_policy_hash,
            "change_set_material_hash":change.material_hash,
            "source_delivery_intent_id":intent.id,"source_operation_id":operation.id,
            "source_stage_outcome_id":source.id,"source_stage_outcome_hash":source.content_hash,
            "merge_tree_sha":proof["merge_tree_sha"],"merge_attempt_id":attempt.id,"merge_attempt_hash":attempt.content_hash
        }
    })))
}

/// Revalidate before authorization and dispatch, including changes to contract
/// rows which retain the same name/version. No production action is granted.
pub(in crate::app) async fn validate_intent(
    state: &AppState,
    intent: &StoredPipelineIntent,
) -> Result<bool, ApiError> {
    validate_intent_mode(state, intent, true).await
}

pub(in crate::app) async fn validate_observed_intent(
    state: &AppState,
    intent: &StoredPipelineIntent,
) -> Result<bool, ApiError> {
    validate_intent_mode(state, intent, false).await
}

async fn validate_intent_mode(
    state: &AppState,
    intent: &StoredPipelineIntent,
    for_write: bool,
) -> Result<bool, ApiError> {
    let change = state
        .store
        .get_change_set(&intent.change_set_id)
        .await?
        .ok_or_else(|| ApiError::conflict("pipeline ChangeSet is unavailable"))?;
    let Some(provenance) = source_provenance(&state.store, &change).await? else {
        return Ok(false);
    };
    let item_id = change.work_item_id.as_deref().unwrap();
    let metadata = state
        .store
        .get_repo_work_item_metadata(item_id)
        .await?
        .unwrap();
    let policy = metadata.workflow_policy.as_ref().unwrap();
    crate::app::hosted_workflow::delivery::validate_finance_coordinates(policy)?;
    let control = state
        .store
        .get_workflow_reconciliation(item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("hosted build controller state is unavailable"))?;
    let contract = state
        .store
        .get_pipeline_contract(&policy.delivery_binding.pipeline_contract_id)
        .await?
        .ok_or_else(|| ApiError::conflict("hosted build PipelineContract is unavailable"))?;
    let execution = super::execution::tekton_execution_spec(&intent.intent_json)?;
    if (for_write
        && (control.control != "active"
            || crate::app::OperationalMode::from_env() != crate::app::OperationalMode::Normal))
        || !policy
            .automatic_actions
            .contains(&HostedAutomaticAction::Build)
        || contract.status != "active"
        || json!(contract) != policy.pipeline_contract
        || intent.work_plan_id != change.work_plan_id
        || intent.intent_json["source_provenance"] != provenance
        || intent.intent_json["pipeline_contract"]["id"] != contract.id
        || intent.intent_json["pipeline_contract"]["version"] != contract.version
        || intent.intent_json["pipeline_contract"]["namespace"] != contract.namespace
        || intent.intent_json["pipeline_contract"]["pipeline_ref"] != contract.pipeline_ref
        || intent.intent_json.get("deployment_handoff").is_some()
        || !execution.enabled
        || execution.production_impacting
        || execution.namespace != contract.namespace
        || execution.pipeline_ref != contract.pipeline_ref
    {
        return Err(ApiError::conflict("hosted build is paused, outside its saved authorization, or has changed source or pipeline configuration"));
    }
    super::execution::execution_matches_pipeline_contract(
        &execution,
        &contract,
        provenance["merge_commit_sha"].as_str(),
    )?;
    // This also checks the finite Finance workspace, parameters and service account.
    super::execution::build_pipeline_run_manifest(intent, &execution)?;
    Ok(true)
}

pub(super) fn valid_declared_build_output(intent: &StoredPipelineIntent, analysis: &Value) -> bool {
    let Some(commit) = intent
        .intent_json
        .pointer("/source_provenance/merge_commit_sha")
        .and_then(Value::as_str)
    else {
        return false;
    };
    let image = match intent
        .intent_json
        .pointer("/execution/pipeline_ref")
        .and_then(Value::as_str)
    {
        Some("pharness-yfinance-build") => "registry.lucas.engineering/yfinance_wrapper",
        Some("pharness-finance-frontend-build") => "registry.lucas.engineering/finance-frontend",
        _ => return false,
    };
    let outputs = &analysis["outputs"];
    let declared = &outputs["declared_results"];
    is_git_sha(commit)
        && analysis["kind"] == "PipelineRunAnalysis"
        && analysis["summary"]["status"] == "succeeded"
        && analysis["pipeline_run"]["uid"]
            .as_str()
            .is_some_and(|uid| !uid.is_empty())
        && outputs["result_conflicts"]
            .as_array()
            .is_some_and(Vec::is_empty)
        && outputs["source_commit"] == commit
        && outputs["commit"] == commit
        && declared["SOURCE_COMMIT"] == commit
        && declared["IMAGE_URL"] == format!("{image}:git-{commit}")
        && outputs["image_url"] == declared["IMAGE_URL"]
        && outputs["image_digest"] == declared["IMAGE_DIGEST"]
        && declared["IMAGE_DIGEST"]
            .as_str()
            .is_some_and(crate::app::identifiers::is_sha256_digest)
}
