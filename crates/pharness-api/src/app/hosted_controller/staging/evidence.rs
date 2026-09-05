use super::{now, ApiError, AppState};
use crate::app::hashing::canonical_material_hash as hash;
use crate::app::pipeline::hosted;
use pharness_core::hosted_sdlc::HostedAutomaticAction;
use pharness_store::{
    CreateArtifact, StoredArtifact, StoredPipelineIntent, StoredRepoWorkItemMetadata,
};
use serde_json::{json, Value};

pub(super) async fn build(
    state: &AppState,
    item_id: &str,
) -> Result<Option<(StoredPipelineIntent, StoredRepoWorkItemMetadata, String)>, ApiError> {
    let metadata = state
        .store
        .get_repo_work_item_metadata(item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("staging WorkItem is unavailable"))?;
    let Some(plan) = state.store.get_work_plan_by_work_item(item_id).await? else {
        return Ok(None);
    };
    let Some(change) = state.store.get_change_set_by_work_plan(&plan.id).await? else {
        return Ok(None);
    };
    let Some(intent) = state
        .store
        .get_pipeline_intent_by_change_set(&change.id)
        .await?
    else {
        return Ok(None);
    };
    let Some(operation_id) = intent
        .intent_json
        .pointer("/hosted_build/operation_id")
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };
    let operation = state
        .store
        .get_workflow_operation(operation_id)
        .await?
        .ok_or_else(|| ApiError::conflict("staging build operation is unavailable"))?;
    if operation.status != "succeeded" {
        return Ok(None);
    }
    if operation.action != super::super::build::ACTION
        || operation.work_item_id != item_id
        || operation.resource_refs["pipeline_intent_id"] != intent.id
        || operation.resource_refs["build_result"]["status"] != "verified"
        || intent.intent_json["execution_state"]["state"] != "pipeline_run_succeeded"
        || intent.intent_json["build_output"]["status"] != "verified"
        || !hosted::validate_observed_intent(state, &intent).await?
    {
        return Err(ApiError::conflict(
            "staging requires the original autonomous, verified build and source evidence",
        ));
    }
    let policy = metadata
        .workflow_policy
        .as_ref()
        .ok_or_else(|| ApiError::conflict("source-only work has no staging authority"))?;
    policy.validate().map_err(ApiError::conflict)?;
    let contract = state
        .store
        .get_deployment_contract(&policy.delivery_binding.staging.deployment_contract_id)
        .await?
        .ok_or_else(|| ApiError::conflict("saved staging DeploymentContract is unavailable"))?;
    if !policy
        .automatic_actions
        .contains(&HostedAutomaticAction::StagingDelivery)
        || !policy
            .automatic_actions
            .contains(&HostedAutomaticAction::Observe)
        || metadata.workflow_policy_hash.as_deref() != Some(hash(&json!(policy))?.as_str())
        || contract.status != "active"
        || json!(contract) != policy.staging_contract
    {
        return Err(ApiError::conflict(
            "staging authorization or deployment configuration changed",
        ));
    }
    let execution = &intent.intent_json["execution_state"]["execution_id"];
    let terminal_id = format!("build_terminal_{}", execution.as_str().unwrap_or_default());
    let terminal = state
        .store
        .get_artifact(&terminal_id)
        .await?
        .ok_or_else(|| {
            ApiError::conflict("staging requires the original terminal build receipt")
        })?;
    let output_id = intent.intent_json["build_output"]["artifact_id"]
        .as_str()
        .ok_or_else(|| ApiError::conflict("staging build output artifact is missing"))?;
    let output = state
        .store
        .get_artifact(output_id)
        .await?
        .ok_or_else(|| ApiError::conflict("staging build output artifact is unavailable"))?;
    let t = terminal
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("build receipt has no content"))?;
    let o = output
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("build output has no content"))?;
    let declared = &t["analysis"]["outputs"]["declared_results"];
    let source = &intent.intent_json["source_provenance"]["merge_commit_sha"];
    if terminal.kind != "hosted_build_observation"
        || output.kind != "pipeline_build_output"
        || terminal.session_id != intent.session_id
        || output.session_id != intent.session_id
        || terminal.run_id != intent.run_id
        || output.run_id != intent.run_id
        || t["execution_id"] != *execution
        || o["execution_id"] != *execution
        || t["status"] != "completed"
        || o["status"] != "verified"
        || o["pipeline_intent_id"] != intent.id
        || operation.resource_refs["build_dispatch"]["execution_id"] != *execution
        || operation.resource_refs["build_result"]["terminal_artifact_id"] != terminal_id
        || t["manifest_hash"] != operation.resource_refs["build_dispatch"]["manifest_hash"]
        || t["pipeline_run_uid"] != intent.intent_json["execution_state"]["pipeline_run_uid"]
        || t["pipeline_run_uid"].as_str().map_or(true, str::is_empty)
        || declared["SOURCE_COMMIT"] != *source
        || o["source"]["commit"] != *source
        || o["source"]["expected_merge_commit"] != *source
        || declared["IMAGE_DIGEST"] != intent.intent_json["build_output"]["image_digest"]
        || declared["IMAGE_DIGEST"] != o["image"]["digest"]
        || declared["IMAGE_URL"] != o["image"]["url"]
        || declared["IMAGE_URL"]
            != format!(
                "{}:git-{}",
                policy.delivery_binding.image_name,
                source.as_str().unwrap_or_default()
            )
    {
        return Err(ApiError::conflict(
            "staging build receipt, declared source and image digest disagree",
        ));
    }
    let evidence_hash = hash(
        &json!({"pipeline_intent_id":intent.id,"source_provenance":intent.intent_json["source_provenance"],"terminal":t,"output":o,"workflow_policy_hash":metadata.workflow_policy_hash}),
    )?;
    Ok(Some((intent, metadata, evidence_hash)))
}

pub(super) async fn artifact(
    state: &AppState,
    intent: &StoredPipelineIntent,
    id: &str,
    kind: &str,
    body: Value,
) -> Result<StoredArtifact, ApiError> {
    if let Some(existing) = state.store.get_artifact(id).await? {
        if existing.kind != kind
            || existing.session_id != intent.session_id
            || existing.run_id != intent.run_id
            || existing.content_json.as_ref() != Some(&body)
        {
            return Err(ApiError::conflict(
                "different staging evidence is already recorded",
            ));
        }
        return Ok(existing);
    }
    state
        .store
        .create_artifact(CreateArtifact {
            id: id.into(),
            session_id: intent.session_id.clone(),
            run_id: intent.run_id.clone(),
            kind: kind.into(),
            label: "Hosted staging evidence".into(),
            mime_type: Some("application/json".into()),
            path: None,
            content_text: None,
            content_json: Some(body),
        })
        .await
        .map_err(Into::into)
}

pub(super) async fn may_advance(state: &AppState, item: &str) -> Result<bool, ApiError> {
    Ok(state
        .store
        .get_workflow_reconciliation(item)
        .await?
        .is_some_and(|c| c.control == "active")
        && crate::app::OperationalMode::from_env() == crate::app::OperationalMode::Normal)
}

pub(super) fn validate_observation(
    authority: &pharness_core::hosted_sdlc::staging::HostedStagingAuthority,
    plan: &pharness_core::hosted_sdlc::staging::StagingGitOpsPlan,
    observation: &Value,
    checked: i64,
) -> Result<Value, ApiError> {
    let fields = [
        "gitops_commit_sha",
        "base_commit_sha",
        "current_gitops_revision",
        "file_blob_sha",
        "path",
        "image_ref",
        "authority_hash",
        "plan_hash",
        "parent_count",
        "changed_file_count",
        "current_file_matches",
        "observation_attempts",
    ];
    if observation.as_object().map_or(true, |o| {
        o.len() != fields.len() || o.keys().any(|k| !fields.contains(&k.as_str()))
    }) || [
        "gitops_commit_sha",
        "current_gitops_revision",
        "file_blob_sha",
    ]
    .iter()
    .any(|key| {
        !observation[*key]
            .as_str()
            .is_some_and(crate::app::identifiers::is_git_sha)
    }) || observation["gitops_commit_sha"] == plan.base_commit_sha
        || observation["file_blob_sha"] == plan.original_blob_sha
        || observation["base_commit_sha"] != plan.base_commit_sha
        || observation["authority_hash"] != plan.authority_hash
        || observation["plan_hash"] != plan.material_hash().map_err(ApiError::conflict)?
        || observation["path"] != authority.coordinates().map_err(ApiError::conflict)?.0
        || observation["image_ref"] != authority.image_ref().map_err(ApiError::conflict)?
        || observation["parent_count"] != 1
        || observation["changed_file_count"] != 1
        || observation["current_file_matches"] != true
        || !observation["observation_attempts"]
            .as_u64()
            .is_some_and(|n| (1..=12).contains(&n))
        || checked < authority.created_at_ms
        || checked > now().saturating_add(30_000)
    {
        return Err(ApiError::conflict(
            "staging observation does not prove the admitted one-file digest change",
        ));
    }
    // Independent re-observation may see a newer unrelated GitOps revision. The
    // admitted commit, base, blob and image can never change across callbacks.
    let mut identity = observation.clone();
    for key in ["current_gitops_revision", "observation_attempts"] {
        identity.as_object_mut().unwrap().remove(key);
    }
    Ok(identity)
}
