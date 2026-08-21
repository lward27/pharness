use pharness_store::{
    StoredDeploymentIntent, StoredObservation, StoredPipelineIntent, StoredRelease,
};
use serde_json::{json, Value};

pub(in crate::app) fn pipeline_intent_json_with_evidence(
    current: &StoredPipelineIntent,
    observation: &StoredObservation,
) -> Value {
    let mut intent_json = current.intent_json.clone();
    set_pipeline_intent_evidence(&mut intent_json, observation);
    intent_json
}

pub(in crate::app) fn set_pipeline_intent_evidence(
    intent_json: &mut Value,
    observation: &StoredObservation,
) {
    let evidence = pipeline_intent_evidence_json(observation);
    if let Some(object) = intent_json.as_object_mut() {
        object.insert("evidence".to_string(), evidence);
    }
}

pub(in crate::app) fn pipeline_intent_evidence_json(observation: &StoredObservation) -> Value {
    let analysis = observation
        .data_json
        .get("analysis")
        .cloned()
        .unwrap_or_else(|| json!({}));
    json!({
        "status": pipeline_intent_evidence_status(&analysis),
        "source": "observation",
        "observation_id": observation.id,
        "artifact_id": observation.artifact_id,
        "kind": observation.kind,
        "resource": {
            "namespace": observation.resource_namespace,
            "kind": observation.resource_kind,
            "name": observation.resource_name,
        },
        "summary": {
            "pipeline_run_status": analysis.pointer("/summary/status"),
            "pipeline_run_reason": analysis.pointer("/summary/reason"),
            "task_run_count": analysis.pointer("/summary/task_run_count"),
            "failed_task_run_count": analysis.pointer("/summary/failed_task_run_count"),
            "running_task_run_count": analysis.pointer("/summary/running_task_run_count"),
            "succeeded_task_run_count": analysis.pointer("/summary/succeeded_task_run_count"),
            "argo_sync_status": analysis.pointer("/summary/argo_sync_status"),
            "argo_health_status": analysis.pointer("/summary/argo_health_status"),
            "image_alignment_status": analysis.pointer("/summary/image_alignment/status"),
        }
    })
}

pub(in crate::app) fn pipeline_intent_evidence_status(analysis: &Value) -> &'static str {
    match analysis.pointer("/summary/status").and_then(Value::as_str) {
        Some("succeeded") => {
            let failed_tasks = analysis
                .pointer("/summary/failed_task_run_count")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            if failed_tasks != 0 || pipeline_analysis_needs_attention(analysis) {
                "attention_required"
            } else {
                "satisfied"
            }
        }
        Some("running") => "running",
        Some("failed" | "cancelled") => "failed",
        Some(_) => "attention_required",
        None => "unknown",
    }
}

pub(in crate::app) fn pipeline_analysis_needs_attention(analysis: &Value) -> bool {
    let argo_sync = analysis
        .pointer("/summary/argo_sync_status")
        .and_then(Value::as_str);
    if argo_sync.is_some_and(|status| status != "Synced") {
        return true;
    }
    let argo_health = analysis
        .pointer("/summary/argo_health_status")
        .and_then(Value::as_str);
    if argo_health.is_some_and(|status| status != "Healthy") {
        return true;
    }
    let image_alignment = analysis
        .pointer("/summary/image_alignment/status")
        .and_then(Value::as_str);
    image_alignment
        .is_some_and(|status| !matches!(status, "exact_match" | "registry_alias_match" | "unknown"))
}

pub(in crate::app) fn pipeline_intent_attached_evidence_status(
    pipeline_intent: &StoredPipelineIntent,
) -> Option<&str> {
    pipeline_intent
        .intent_json
        .pointer("/evidence/status")
        .and_then(Value::as_str)
}

pub(in crate::app) fn pipeline_execution_evidence_status(
    pipeline_intent: &StoredPipelineIntent,
) -> Option<&str> {
    pipeline_intent
        .intent_json
        .pointer("/execution_evidence/status")
        .and_then(Value::as_str)
}

pub(in crate::app) fn deployment_intent_attached_evidence_status(
    deployment_intent: &StoredDeploymentIntent,
) -> Option<&str> {
    deployment_intent
        .intent_json
        .pointer("/deployment_evidence/status")
        .and_then(Value::as_str)
}

pub(in crate::app) fn release_observability_evidence_status(
    release: &StoredRelease,
) -> Option<&str> {
    let evidence = release
        .release_json
        .pointer("/observability_evidence")
        .and_then(Value::as_array)?;
    if evidence.is_empty() {
        return None;
    }
    if evidence.iter().any(|item| {
        item.get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| status == "attention_required")
    }) {
        return Some("attention_required");
    }
    if evidence.iter().any(|item| {
        item.get("status")
            .and_then(Value::as_str)
            .map_or(true, |status| status == "unknown")
    }) {
        return Some("unknown");
    }
    Some("observed")
}
