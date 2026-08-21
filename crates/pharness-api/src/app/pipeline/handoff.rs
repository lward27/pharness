use super::super::audit::append_deployment_intent_audit_event;
use super::super::clock::unique_suffix;
use super::super::deployment::intents::deployment_intent_json;
use super::super::validation::{clean_optional_text, validate_kubernetes_name};
use super::super::{ApiError, AppState};
use pharness_store::{CreateDeploymentIntent, StoredDeploymentIntent, StoredPipelineIntent};
use serde_json::{json, Value};

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct PipelineDeploymentHandoffSpec {
    pub(in crate::app) target_environment: String,
    pub(in crate::app) target_namespace: String,
    pub(in crate::app) argo_application: String,
    #[serde(default)]
    pub(in crate::app) title: Option<String>,
    #[serde(default)]
    pub(in crate::app) summary: Option<String>,
    #[serde(default)]
    pub(in crate::app) risk_level: Option<String>,
}

pub(in crate::app) async fn create_declared_deployment_handoff(
    state: &AppState,
    pipeline_intent: &StoredPipelineIntent,
) -> Result<Option<StoredDeploymentIntent>, ApiError> {
    let remediation_plan_id = pipeline_intent.remediation_plan_id.clone();
    let incident_id = pipeline_intent.incident_id.clone();
    let Some(raw_handoff) = pipeline_intent.intent_json.get("deployment_handoff") else {
        return Ok(None);
    };
    let handoff = serde_json::from_value::<PipelineDeploymentHandoffSpec>(raw_handoff.clone())
        .map_err(|error| {
            ApiError::bad_request(format!("pipeline deployment_handoff is invalid: {error}"))
        })?;
    validate_pipeline_deployment_handoff(&handoff)?;
    if pipeline_intent
        .intent_json
        .pointer("/evidence/status")
        .and_then(Value::as_str)
        != Some("satisfied")
    {
        return Err(ApiError::conflict(
            "pipeline deployment_handoff requires satisfied PipelineRunAnalysis evidence",
        ));
    }
    if state
        .store
        .get_deployment_intent_by_pipeline_intent(&pipeline_intent.id)
        .await?
        .is_some()
    {
        return Ok(None);
    }
    let title = clean_optional_text(handoff.title)
        .unwrap_or_else(|| format!("DeploymentIntent: {}", pipeline_intent.title));
    let summary = clean_optional_text(handoff.summary).unwrap_or_else(|| {
        format!(
            "Proposed Argo CD sync for {} after terminal PipelineRunAnalysis",
            handoff.argo_application
        )
    });
    let risk_level = clean_optional_text(handoff.risk_level)
        .unwrap_or_else(|| pipeline_intent.risk_level.clone());
    let intent_json = deployment_intent_json(
        pipeline_intent,
        "argo_sync_deploy",
        Some(&handoff.target_environment),
        Some(&handoff.target_namespace),
        Some(&handoff.argo_application),
        None,
    )?;
    let deployment_intent = state
        .store
        .create_deployment_intent(CreateDeploymentIntent {
            id: format!("dint_{}", unique_suffix()),
            pipeline_intent_id: pipeline_intent.id.clone(),
            change_set_id: pipeline_intent.change_set_id.clone(),
            work_plan_id: pipeline_intent.work_plan_id.clone(),
            remediation_plan_id,
            incident_id,
            session_id: pipeline_intent.session_id.clone(),
            run_id: pipeline_intent.run_id.clone(),
            status: "proposed".to_string(),
            title,
            summary,
            risk_level,
            intent_kind: "argo_sync_deploy".to_string(),
            target_environment: Some(handoff.target_environment),
            target_namespace: Some(handoff.target_namespace),
            argo_application: Some(handoff.argo_application),
            resource_namespace: pipeline_intent.resource_namespace.clone(),
            resource_kind: pipeline_intent.resource_kind.clone(),
            resource_name: pipeline_intent.resource_name.clone(),
            intent_json,
        })
        .await?;
    append_deployment_intent_audit_event(
        &state.store,
        &deployment_intent,
        "deployment_intent.auto_proposed",
        Some("executor:tekton".to_string()),
        Some("created from declared terminal PipelineIntent handoff".to_string()),
        json!({
            "source": "pipeline_intent.deployment_handoff",
            "pipeline_intent_id": pipeline_intent.id,
            "pipeline_evidence_status": pipeline_intent.intent_json.pointer("/evidence/status"),
            "execution_evidence": pipeline_intent.intent_json.get("execution_evidence"),
        }),
    )
    .await?;
    Ok(Some(deployment_intent))
}

pub(in crate::app) fn validate_pipeline_deployment_handoff(
    handoff: &PipelineDeploymentHandoffSpec,
) -> Result<(), ApiError> {
    validate_kubernetes_name(
        "deployment_handoff.target_environment",
        &handoff.target_environment,
    )?;
    validate_kubernetes_name(
        "deployment_handoff.target_namespace",
        &handoff.target_namespace,
    )?;
    validate_kubernetes_name(
        "deployment_handoff.argo_application",
        &handoff.argo_application,
    )
}
