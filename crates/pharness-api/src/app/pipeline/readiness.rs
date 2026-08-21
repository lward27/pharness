use super::super::ApiError;
use super::evidence::pipeline_intent_attached_evidence_status;
use super::state::pipeline_intent_is_deployment_eligible;
use pharness_store::StoredPipelineIntent;
use serde_json::Value;

pub(in crate::app) fn ensure_pipeline_intent_ready_for_deployment(
    intent: &StoredPipelineIntent,
) -> Result<(), ApiError> {
    if pipeline_intent_is_deployment_eligible(&intent.status) {
        return Ok(());
    }
    Err(ApiError::conflict(format!(
        "pipeline_intent {} must be approved with successful execution evidence before proposing deployment",
        intent.id
    )))
}

pub(in crate::app) fn ensure_pipeline_evidence_ready_for_deployment(
    pipeline_intent: &StoredPipelineIntent,
) -> Result<(), ApiError> {
    if pipeline_intent_attached_evidence_status(pipeline_intent) != Some("satisfied") {
        return Err(ApiError::conflict(format!(
            "pipeline_intent {} needs satisfied PipelineRunAnalysis evidence before approving deployment",
            pipeline_intent.id
        )));
    }
    let expected_namespace = pipeline_intent
        .intent_json
        .pointer("/execution_evidence/pipeline_run/namespace")
        .and_then(Value::as_str);
    let expected_name = pipeline_intent
        .intent_json
        .pointer("/execution_evidence/pipeline_run/name")
        .and_then(Value::as_str);
    let evidence_namespace = pipeline_intent
        .intent_json
        .pointer("/evidence/resource/namespace")
        .and_then(Value::as_str);
    let evidence_name = pipeline_intent
        .intent_json
        .pointer("/evidence/resource/name")
        .and_then(Value::as_str);
    if expected_namespace.is_some_and(|value| evidence_namespace != Some(value))
        || expected_name.is_some_and(|value| evidence_name != Some(value))
    {
        return Err(ApiError::conflict(format!(
            "pipeline_intent {} evidence does not match the executed PipelineRun",
            pipeline_intent.id
        )));
    }
    Ok(())
}
