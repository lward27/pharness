//! Controller-sealed `release` and `observe` outcomes. Each is derived only
//! from a succeeded production operation whose runtime window verified; the
//! outcome pins that operation and result so a later reader can re-check it.
use super::super::state::Snapshot;
use super::{hash, suffix, ApiError, AppState, OBSERVE_ACTION, PROMOTE_ACTION};
use crate::app::identifiers::new_prefixed_id;
use pharness_store::{
    CreateEvidenceValidation, CreateStageExecution, SealStageOutcome, StoredStageOutcome,
    StoredWorkflowOperation,
};
use serde_json::json;

pub(super) async fn release_sealed(
    state: &AppState,
    snapshot: &Snapshot,
    promote: &StoredWorkflowOperation,
) -> Result<StoredStageOutcome, ApiError> {
    let execution = snapshot
        .stages
        .iter()
        .find(|s| s.stage_key == "release")
        .map(|s| s.id.clone())
        .ok_or_else(|| ApiError::conflict("hosted release stage execution is missing"))?;
    sealed(
        state,
        snapshot,
        promote,
        Stage {
            key: "release",
            action: PROMOTE_ACTION,
            result_key: "production_runtime_result",
            execution,
            objective: "promote_staging_verified_image_to_production",
            validator: "hosted_production_release_window",
            stop_reason:
                "approved production image observed in Argo and verified over its runtime window",
        },
    )
    .await
}

pub(super) async fn observe_sealed(
    state: &AppState,
    snapshot: &Snapshot,
    observed: &StoredWorkflowOperation,
) -> Result<StoredStageOutcome, ApiError> {
    sealed(
        state,
        snapshot,
        observed,
        Stage {
            key: "observe",
            action: OBSERVE_ACTION,
            result_key: "observe_runtime_result",
            execution: format!("stage_observe_{}", suffix(observed)),
            objective: "observe_released_production_image",
            validator: "hosted_production_observe_window",
            stop_reason:
                "released production image verified over a second independent runtime window",
        },
    )
    .await
}

struct Stage {
    key: &'static str,
    action: &'static str,
    result_key: &'static str,
    execution: String,
    objective: &'static str,
    validator: &'static str,
    stop_reason: &'static str,
}

async fn sealed(
    state: &AppState,
    snapshot: &Snapshot,
    operation: &StoredWorkflowOperation,
    stage: Stage,
) -> Result<StoredStageOutcome, ApiError> {
    let item = &snapshot.metadata.work_item_id;
    let result = &operation.resource_refs[stage.result_key];
    if operation.action != stage.action
        || operation.status != "succeeded"
        || operation.work_item_id != *item
        || result["status"] != "verified"
    {
        return Err(ApiError::conflict(format!(
            "{} cannot be sealed without a succeeded operation and verified runtime window",
            stage.key
        )));
    }
    let input = json!({
        "workflow_operation_id":operation.id,
        "deployment_intent_id":operation.resource_refs["action_resource"],
        "result_artifact_id":result["artifact_id"],
        "result_artifact_sha256":result["artifact_sha256"],
        "expected":result["expected"],
        "binding":result["binding"],
        "workflow_policy_hash":snapshot.metadata.workflow_policy_hash,
    });
    if let Some(existing) = state
        .store
        .list_effective_stage_outcomes(item)
        .await?
        .into_iter()
        .find(|o| o.stage_key == stage.key)
    {
        if existing.status != "succeeded"
            || existing.outcome["pinned_inputs"] != input
            || existing.stage_execution_id != stage.execution
        {
            return Err(ApiError::conflict(format!(
                "the sealed {} outcome belongs to different production evidence",
                stage.key
            )));
        }
        return Ok(existing);
    }
    match state.store.get_stage_execution(&stage.execution).await? {
        Some(existing) => {
            if existing.work_item_id != *item || existing.stage_key != stage.key {
                return Err(ApiError::conflict(format!(
                    "the recorded {} stage belongs to different work",
                    stage.key
                )));
            }
        }
        None if stage.key == "observe" => {
            state
                .store
                .create_stage_execution(CreateStageExecution {
                    id: stage.execution.clone(),
                    work_item_id: item.clone(),
                    stage_key: stage.key.into(),
                    sequence: 1,
                    status: "running".into(),
                    agent_profile_id: None,
                    agent_profile_version: None,
                    agent_profile_hash: None,
                    context_pack_id: None,
                    run_id: None,
                    workspace_id: None,
                    input_hash: hash(&input)?,
                    input_snapshot: input.clone(),
                })
                .await?;
        }
        None => {
            return Err(ApiError::conflict(
                "hosted release stage execution is missing",
            ))
        }
    }
    let outcome = json!({
        "schema_version":pharness_core::STAGE_OUTCOME_SCHEMA,
        "work_item_id":item,
        "stage_execution_id":stage.execution,
        "stage":stage.key,
        "origin":"controller",
        "status":"succeeded",
        "objective":{"kind":stage.objective},
        "pinned_inputs":input,
        "verified_facts":[{"kind":"finance_runtime_window","id":result["artifact_id"],"hash":result["artifact_sha256"],"status":"verified"}],
        "agent_claims":[],
        "outputs":[{"kind":"deployment_intent","id":operation.resource_refs["action_resource"]}],
        "acceptance":[],"decisions":[],
        "authorizations":[{"kind":"workflow_operation","id":operation.id,"action":operation.action}],
        "contradictions":[],"risks":[],"unavailable_capabilities":[],"recommendations":[],
        "stop_reason":stage.stop_reason,"sealed_state_version":snapshot.metadata.state_version,
    });
    state
        .store
        .create_evidence_validation(CreateEvidenceValidation {
            id: new_prefixed_id("evalid"),
            work_item_id: item.clone(),
            stage_execution_id: Some(stage.execution.clone()),
            validator_key: stage.validator.into(),
            status: "valid".into(),
            subject: json!({"workflow_operation_id":operation.id}),
            evidence_refs: json!([{"kind":"artifact","id":result["artifact_id"],"hash":result["artifact_sha256"]}]),
            facts: json!({"expected":result["expected"],"runtime_verification":result["runtime_verification"]}),
            contradictions: json!([]),
            content_hash: hash(&json!({"operation":operation.id,"result":result["artifact_sha256"]}))?,
        })
        .await?;
    state
        .store
        .seal_stage_outcome(SealStageOutcome {
            id: new_prefixed_id("stageout"),
            stage_execution_id: stage.execution,
            work_item_id: item.clone(),
            stage_key: stage.key.into(),
            status: "succeeded".into(),
            content_hash: hash(&outcome)?,
            outcome,
            state_version: snapshot.metadata.state_version,
            supersedes_outcome_id: None,
            effective: true,
            actor: "controller:hosted-production".into(),
            reason: stage.stop_reason.into(),
        })
        .await
        .map_err(Into::into)
}
