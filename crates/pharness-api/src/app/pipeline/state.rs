use super::super::ApiError;
use pharness_store::StoredPipelineIntent;
use serde_json::Value;

pub(in crate::app) const MAX_PIPELINE_EXECUTION_ATTEMPTS: u64 = 2;

pub(in crate::app) fn pipeline_intent_execution_state(
    intent: &StoredPipelineIntent,
) -> Option<&str> {
    intent
        .intent_json
        .pointer("/execution_state/state")
        .and_then(Value::as_str)
}

pub(in crate::app) fn pipeline_execution_attempt(intent_json: &Value) -> Result<u64, ApiError> {
    let attempt = intent_json
        .get("execution_attempt")
        .map(|value| {
            value.as_u64().ok_or_else(|| {
                ApiError::conflict("PipelineIntent execution_attempt must be a positive integer")
            })
        })
        .transpose()?
        .unwrap_or(1);
    if !(1..=MAX_PIPELINE_EXECUTION_ATTEMPTS).contains(&attempt) {
        return Err(ApiError::conflict(format!(
            "PipelineIntent execution_attempt must be between 1 and {MAX_PIPELINE_EXECUTION_ATTEMPTS}"
        )));
    }
    Ok(attempt)
}

pub(in crate::app) fn pipeline_intent_is_deployment_eligible(status: &str) -> bool {
    matches!(status, "approved" | "completed")
}

pub(in crate::app) fn pipeline_intent_is_gitops_update_eligible(
    intent: &StoredPipelineIntent,
) -> bool {
    pipeline_intent_is_deployment_eligible(&intent.status)
        && intent
            .intent_json
            .pointer("/evidence/status")
            .and_then(Value::as_str)
            == Some("satisfied")
}
