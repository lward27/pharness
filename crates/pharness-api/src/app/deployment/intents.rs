use super::super::approvals::create_permission_grant_record;
use super::super::approvals::ensure_approved_for_trusted_envelope;
use super::super::audit::append_deployment_intent_audit_event;
use super::super::clock::unique_suffix;
use super::super::delivery_actions::ARGO_SYNC_ACTIONS;
use super::super::gitops::deployment_evidence::observed_gitops_merge_for_deployment;
use super::super::identifiers::{is_git_sha, is_sha256_digest};
use super::super::pipeline::readiness::{
    ensure_pipeline_evidence_ready_for_deployment, ensure_pipeline_intent_ready_for_deployment,
};
use super::super::principals::DEFAULT_ARGO_RUNNER_SUBJECT;
use super::super::validation::clean_optional_text;
use super::super::work_items::preflight::bounded_production_grant_expiry;
use super::super::{ApiError, AppState};
use super::target::{deployment_target, ensure_supported_deployment_target};
use crate::dto::{
    AttachDeploymentIntentEvidenceRequest, AttachDeploymentIntentEvidenceResponse,
    CreateDeploymentIntentFromPipelineIntentRequest, CreateDeploymentIntentResponse,
    CreateDeploymentIntentTrustedEnvelopeRequest, CreatePermissionGrantRequest,
    DeploymentIntentResponse, DeploymentIntentsResponse, TransitionDeploymentIntentRequest,
    TransitionDeploymentIntentResponse, TrustedEnvelopeResponse,
};
use axum::extract::{Path, Query, State};
use axum::Json;
use pharness_core::RunId;
use pharness_store::{
    CreateDeploymentIntent, DeploymentIntentListFilter, StoredDeploymentIntent, StoredObservation,
    StoredPipelineIntent, UpdateDeploymentIntentDraft, UpdateDeploymentIntentEvidence,
};
use serde_json::{json, Value};

#[derive(Debug, Default, serde::Deserialize)]
pub(in crate::app) struct ListDeploymentIntentsQuery {
    pub(in crate::app) pipeline_intent_id: Option<String>,
    pub(in crate::app) change_set_id: Option<String>,
    pub(in crate::app) work_plan_id: Option<String>,
    pub(in crate::app) remediation_plan_id: Option<String>,
    pub(in crate::app) incident_id: Option<String>,
    pub(in crate::app) run_id: Option<String>,
    pub(in crate::app) status: Option<String>,
    pub(in crate::app) intent_kind: Option<String>,
    pub(in crate::app) risk_level: Option<String>,
    pub(in crate::app) target_environment: Option<String>,
    pub(in crate::app) target_namespace: Option<String>,
    pub(in crate::app) argo_application: Option<String>,
    pub(in crate::app) resource_namespace: Option<String>,
    pub(in crate::app) resource_kind: Option<String>,
    pub(in crate::app) resource_name: Option<String>,
    pub(in crate::app) created_after_ms: Option<i64>,
    pub(in crate::app) created_before_ms: Option<i64>,
    pub(in crate::app) limit: Option<u32>,
    pub(in crate::app) offset: Option<u32>,
}

pub(in crate::app) async fn list_deployment_intents(
    State(state): State<AppState>,
    Query(query): Query<ListDeploymentIntentsQuery>,
) -> Result<Json<DeploymentIntentsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let deployment_intents = state
        .store
        .list_deployment_intents(DeploymentIntentListFilter {
            pipeline_intent_id: clean_optional_text(query.pipeline_intent_id),
            change_set_id: clean_optional_text(query.change_set_id),
            work_plan_id: clean_optional_text(query.work_plan_id),
            remediation_plan_id: clean_optional_text(query.remediation_plan_id),
            incident_id: clean_optional_text(query.incident_id),
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            status: clean_optional_text(query.status),
            intent_kind: clean_optional_text(query.intent_kind),
            risk_level: clean_optional_text(query.risk_level),
            target_environment: clean_optional_text(query.target_environment),
            target_namespace: clean_optional_text(query.target_namespace),
            argo_application: clean_optional_text(query.argo_application),
            resource_namespace: clean_optional_text(query.resource_namespace),
            resource_kind: clean_optional_text(query.resource_kind),
            resource_name: clean_optional_text(query.resource_name),
            created_after_ms: query.created_after_ms,
            created_before_ms: query.created_before_ms,
            limit,
            offset,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let count = deployment_intents.len();

    Ok(Json(DeploymentIntentsResponse {
        deployment_intents,
        count,
        limit,
        offset,
    }))
}

pub(in crate::app) async fn get_deployment_intent(
    State(state): State<AppState>,
    Path(deployment_intent_id): Path<String>,
) -> Result<Json<DeploymentIntentResponse>, ApiError> {
    let intent = state
        .store
        .get_deployment_intent(&deployment_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_intent", &deployment_intent_id))?;

    Ok(Json(intent.into()))
}

pub(in crate::app) async fn create_deployment_intent_from_pipeline_intent(
    State(state): State<AppState>,
    Json(request): Json<CreateDeploymentIntentFromPipelineIntentRequest>,
) -> Result<Json<CreateDeploymentIntentResponse>, ApiError> {
    let pipeline_intent_id = clean_optional_text(Some(request.pipeline_intent_id))
        .ok_or_else(|| ApiError::bad_request("pipeline_intent_id is required"))?;
    let pipeline_intent = state
        .store
        .get_pipeline_intent(&pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", &pipeline_intent_id))?;
    ensure_pipeline_intent_ready_for_deployment(&pipeline_intent)?;
    let remediation_plan_id = pipeline_intent.remediation_plan_id.clone();
    let incident_id = pipeline_intent.incident_id.clone();

    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let intent_kind =
        clean_optional_text(request.intent_kind).unwrap_or_else(|| "argo_sync_deploy".to_string());
    let target_environment = clean_optional_text(request.target_environment);
    let target_namespace = clean_optional_text(request.target_namespace)
        .or(pipeline_intent.resource_namespace.clone());
    let argo_application =
        clean_optional_text(request.argo_application).or(pipeline_intent.resource_name.clone());
    let intent_json = deployment_intent_json(
        &pipeline_intent,
        &intent_kind,
        target_environment.as_deref(),
        target_namespace.as_deref(),
        argo_application.as_deref(),
        request.intent_json,
    )?;
    if let Some(existing) = state
        .store
        .get_deployment_intent_by_pipeline_intent(&pipeline_intent_id)
        .await?
    {
        if existing.status == "stale" {
            let deployment_intent = state
                .store
                .revise_deployment_intent_draft(
                    &existing.id,
                    UpdateDeploymentIntentDraft {
                        title: clean_optional_text(request.title).unwrap_or_else(|| {
                            format!("DeploymentIntent: {}", pipeline_intent.title)
                        }),
                        summary: clean_optional_text(request.summary).unwrap_or_else(|| {
                            "Propose Argo CD sync/deploy after approved pipeline intent".to_string()
                        }),
                        risk_level: clean_optional_text(request.risk_level)
                            .unwrap_or_else(|| pipeline_intent.risk_level.clone()),
                        intent_kind,
                        target_environment,
                        target_namespace,
                        argo_application,
                        resource_namespace: pipeline_intent.resource_namespace,
                        resource_kind: pipeline_intent.resource_kind,
                        resource_name: pipeline_intent.resource_name,
                        intent_json,
                        actor: actor.clone(),
                        reason: reason.clone(),
                    },
                )
                .await?;
            append_deployment_intent_audit_event(
                &state.store,
                &deployment_intent,
                "deployment_intent.reproposed",
                actor,
                reason,
                json!({
                    "source": "pipeline_intent",
                    "pipeline_intent_id": deployment_intent.pipeline_intent_id,
                    "previous_status": existing.status,
                    "execution_enabled": false,
                    "pipeline_evidence_status": deployment_intent
                        .intent_json
                        .pointer("/pipeline_evidence/status"),
                    "pipeline_deploy_ready": deployment_intent
                        .intent_json
                        .pointer("/pipeline_evidence/deploy_ready"),
                }),
            )
            .await?;

            return Ok(Json(CreateDeploymentIntentResponse {
                deployment_intent: deployment_intent.into(),
                created: false,
            }));
        }

        return Ok(Json(CreateDeploymentIntentResponse {
            deployment_intent: existing.into(),
            created: false,
        }));
    }
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
            title: clean_optional_text(request.title)
                .unwrap_or_else(|| format!("DeploymentIntent: {}", pipeline_intent.title)),
            summary: clean_optional_text(request.summary).unwrap_or_else(|| {
                "Propose Argo CD sync/deploy after approved pipeline intent".to_string()
            }),
            risk_level: clean_optional_text(request.risk_level)
                .unwrap_or(pipeline_intent.risk_level),
            intent_kind,
            target_environment,
            target_namespace,
            argo_application,
            resource_namespace: pipeline_intent.resource_namespace,
            resource_kind: pipeline_intent.resource_kind,
            resource_name: pipeline_intent.resource_name,
            intent_json,
        })
        .await?;
    append_deployment_intent_audit_event(
        &state.store,
        &deployment_intent,
        "deployment_intent.proposed",
        actor,
        reason,
        json!({
            "source": "pipeline_intent",
            "pipeline_intent_id": deployment_intent.pipeline_intent_id,
            "execution_enabled": false,
            "pipeline_evidence_status": deployment_intent
                .intent_json
                .pointer("/pipeline_evidence/status"),
            "pipeline_deploy_ready": deployment_intent
                .intent_json
                .pointer("/pipeline_evidence/deploy_ready"),
        }),
    )
    .await?;

    Ok(Json(CreateDeploymentIntentResponse {
        deployment_intent: deployment_intent.into(),
        created: true,
    }))
}

pub(in crate::app) async fn create_deployment_intent_trusted_envelope(
    State(state): State<AppState>,
    Path(deployment_intent_id): Path<String>,
    Json(request): Json<CreateDeploymentIntentTrustedEnvelopeRequest>,
) -> Result<Json<TrustedEnvelopeResponse>, ApiError> {
    let intent = state
        .store
        .get_deployment_intent(&deployment_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_intent", &deployment_intent_id))?;
    let pipeline_intent = state
        .store
        .get_pipeline_intent(&intent.pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", &intent.pipeline_intent_id))?;
    let change_set = state
        .store
        .get_change_set(&intent.change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &intent.change_set_id))?;
    let work_plan = state
        .store
        .get_work_plan(&intent.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &intent.work_plan_id))?;
    ensure_approved_for_trusted_envelope("work_plan", &work_plan.id, &work_plan.status)?;
    ensure_approved_for_trusted_envelope("change_set", &change_set.id, &change_set.status)?;
    ensure_approved_for_trusted_envelope(
        "pipeline_intent",
        &pipeline_intent.id,
        &pipeline_intent.status,
    )?;
    ensure_approved_for_trusted_envelope("deployment_intent", &intent.id, &intent.status)?;

    let work_item_id = work_plan.work_item_id.as_deref().ok_or_else(|| {
        ApiError::conflict("Deployment trusted envelopes require a WorkItem-backed delivery chain")
    })?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let target = deployment_target(&intent)?;
    ensure_supported_deployment_target(&work_item, &target)?;

    let reason = clean_optional_text(Some(request.reason.clone()))
        .ok_or_else(|| ApiError::bad_request("trusted envelope reason is required"))?;
    let actor = clean_optional_text(request.created_by.clone());
    let subject = clean_optional_text(request.subject)
        .unwrap_or_else(|| DEFAULT_ARGO_RUNNER_SUBJECT.to_string());
    let expires_at = bounded_production_grant_expiry(&work_item, request.expires_at)?;
    let (source_merge_shas, gitops_merge_shas, image_digests) = if work_item.production_impacting {
        let source_merge_sha = pipeline_intent
            .intent_json
            .pointer("/source_provenance/merge_commit_sha")
            .and_then(Value::as_str)
            .filter(|value| is_git_sha(value))
            .ok_or_else(|| {
                ApiError::conflict(
                    "production deployment authorization requires immutable source merge provenance",
                )
            })?;
        let image_digest = pipeline_intent
            .intent_json
            .pointer("/build_output/image_digest")
            .and_then(Value::as_str)
            .filter(|value| is_sha256_digest(value))
            .ok_or_else(|| {
                ApiError::conflict(
                    "production deployment authorization requires a verified build image digest",
                )
            })?;
        let gitops_merge =
            observed_gitops_merge_for_deployment(&state.store, &work_item, &pipeline_intent)
                .await?
                .ok_or_else(|| {
                    ApiError::conflict(
                "production deployment authorization requires immutable GitOps merge provenance",
            )
                })?;
        let gitops_merge_sha = gitops_merge
            .content_json
            .as_ref()
            .and_then(|content| content.get("merge_commit_sha"))
            .and_then(Value::as_str)
            .filter(|value| is_git_sha(value))
            .ok_or_else(|| ApiError::conflict("GitOps merge provenance is malformed"))?;
        (
            vec![source_merge_sha.to_string()],
            vec![gitops_merge_sha.to_string()],
            vec![image_digest.to_string()],
        )
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };
    let grant = create_permission_grant_record(
        &state.store,
        CreatePermissionGrantRequest {
            subject,
            created_by: actor.clone(),
            reason: reason.clone(),
            scope: json!({
                "environment": target.environment,
                "capability_kinds": ["argo_sync"],
                "actions": ARGO_SYNC_ACTIONS,
                "max_risk": "high",
                "namespaces": [target.namespace],
                "work_item_ids": [work_item.id],
                "work_plan_ids": [work_plan.id],
                "change_set_ids": [change_set.id],
                "pipeline_intent_ids": [pipeline_intent.id],
                "deployment_intent_ids": [intent.id],
                "argo_applications": [target.application],
                "pipeline_contract_ids": work_item.pipeline_contract_id.iter().cloned().collect::<Vec<_>>(),
                "deployment_contract_ids": work_item.deployment_contract_id.iter().cloned().collect::<Vec<_>>(),
                "source_merge_shas": source_merge_shas,
                "gitops_merge_shas": gitops_merge_shas,
                "image_digests": image_digests,
                "production_impacting": work_item.production_impacting,
            }),
            policy: json!({ "policy_mode": "supervised_autonomy" }),
            expires_at,
        },
    )
    .await?;
    append_deployment_intent_audit_event(
        &state.store,
        &intent,
        "deployment_intent.trusted_envelope_created",
        actor,
        Some(reason),
        json!({
            "permission_grant_id": grant.id,
            "subject": grant.subject,
            "target": {
                "environment": target.environment,
                "namespace": target.namespace,
                "argo_application": target.application,
            },
        }),
    )
    .await?;

    Ok(Json(TrustedEnvelopeResponse {
        grant: grant.into(),
    }))
}

pub(in crate::app) async fn transition_deployment_intent(
    State(state): State<AppState>,
    Path(deployment_intent_id): Path<String>,
    Json(request): Json<TransitionDeploymentIntentRequest>,
) -> Result<Json<TransitionDeploymentIntentResponse>, ApiError> {
    let current = state
        .store
        .get_deployment_intent(&deployment_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_intent", &deployment_intent_id))?;
    let target = clean_optional_text(Some(request.target_status))
        .ok_or_else(|| ApiError::bad_request("target_status is required"))?;
    validate_deployment_intent_transition(&current.status, &target)?;
    if target == "approved" {
        let pipeline_intent = state
            .store
            .get_pipeline_intent(&current.pipeline_intent_id)
            .await?
            .ok_or_else(|| ApiError::not_found("pipeline_intent", &current.pipeline_intent_id))?;
        ensure_pipeline_evidence_ready_for_deployment(&pipeline_intent)?;
    }
    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let deployment_intent = state
        .store
        .update_deployment_intent_status(
            &deployment_intent_id,
            &target,
            actor.clone(),
            reason.clone(),
        )
        .await?;
    append_deployment_intent_audit_event(
        &state.store,
        &deployment_intent,
        &format!("deployment_intent.{target}"),
        actor,
        reason,
        json!({
            "previous_status": current.status,
            "status": deployment_intent.status,
        }),
    )
    .await?;

    Ok(Json(TransitionDeploymentIntentResponse {
        deployment_intent: deployment_intent.into(),
    }))
}

pub(in crate::app) async fn attach_deployment_intent_evidence(
    State(state): State<AppState>,
    Path(deployment_intent_id): Path<String>,
    Json(request): Json<AttachDeploymentIntentEvidenceRequest>,
) -> Result<Json<AttachDeploymentIntentEvidenceResponse>, ApiError> {
    let current = state
        .store
        .get_deployment_intent(&deployment_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_intent", &deployment_intent_id))?;
    if current.status == "stale" {
        return Err(ApiError::conflict(format!(
            "cannot attach evidence to stale deployment intent {deployment_intent_id}"
        )));
    }

    let observation_id = clean_optional_text(Some(request.observation_id))
        .ok_or_else(|| ApiError::bad_request("observation_id is required"))?;
    let observation = state
        .store
        .get_observation(&observation_id)
        .await?
        .ok_or_else(|| ApiError::not_found("observation", &observation_id))?;
    validate_deployment_intent_observation(&observation)?;

    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let intent_json = deployment_intent_json_with_evidence(&current, &observation);
    let deployment_intent = state
        .store
        .update_deployment_intent_evidence(
            &deployment_intent_id,
            UpdateDeploymentIntentEvidence {
                intent_json,
                actor: actor.clone(),
                reason: reason.clone(),
            },
        )
        .await?;
    append_deployment_intent_audit_event(
        &state.store,
        &deployment_intent,
        "deployment_intent.evidence_attached",
        actor,
        reason,
        json!({
            "observation_id": observation.id,
            "artifact_id": observation.artifact_id,
            "evidence_status": deployment_intent.intent_json.pointer("/deployment_evidence/status"),
            "deploy_ready": deployment_intent.intent_json.pointer("/deployment_evidence/deploy_ready"),
            "resource": {
                "namespace": observation.resource_namespace,
                "kind": observation.resource_kind,
                "name": observation.resource_name,
            },
        }),
    )
    .await?;

    Ok(Json(AttachDeploymentIntentEvidenceResponse {
        deployment_intent: deployment_intent.into(),
        observation: observation.into(),
    }))
}

pub(in crate::app) fn validate_deployment_intent_observation(
    observation: &StoredObservation,
) -> Result<(), ApiError> {
    if observation.source != "argocd" {
        return Err(ApiError::bad_request(
            "deployment intent evidence must be an argocd Application observation",
        ));
    }

    let looks_like_application = observation.kind == "applications.argoproj.io"
        || observation.resource_kind.as_deref() == Some("Application")
        || observation
            .data_json
            .pointer("/output/kind")
            .and_then(Value::as_str)
            == Some("Application");
    if !looks_like_application {
        return Err(ApiError::bad_request(
            "deployment intent evidence must describe an Argo CD Application",
        ));
    }
    if observation.data_json.pointer("/output/status").is_none() {
        return Err(ApiError::bad_request(
            "deployment intent evidence observation is missing Argo Application status",
        ));
    }

    Ok(())
}

pub(in crate::app) fn deployment_intent_json_with_evidence(
    current: &StoredDeploymentIntent,
    observation: &StoredObservation,
) -> Value {
    let mut intent_json = current.intent_json.clone();
    let evidence = deployment_intent_evidence_json(observation);
    if let Some(object) = intent_json.as_object_mut() {
        object.insert("deployment_evidence".to_string(), evidence);
    }

    intent_json
}

pub(in crate::app) fn deployment_intent_evidence_json(observation: &StoredObservation) -> Value {
    let output = observation
        .data_json
        .get("output")
        .cloned()
        .unwrap_or_else(|| json!({}));
    json!({
        "status": deployment_intent_evidence_status(&output),
        "source": "observation",
        "observation_id": observation.id,
        "artifact_id": observation.artifact_id,
        "kind": observation.kind,
        "deploy_ready": deployment_intent_evidence_status(&output) == "satisfied",
        "review_required": deployment_intent_evidence_status(&output) != "satisfied",
        "resource": {
            "namespace": observation.resource_namespace,
            "kind": observation.resource_kind,
            "name": observation.resource_name,
        },
        "summary": {
            "sync_status": output.pointer("/status/sync/status"),
            "health_status": output.pointer("/status/health/status"),
            "revision": output.pointer("/status/sync/revision"),
        }
    })
}

pub(in crate::app) fn deployment_intent_evidence_status(output: &Value) -> &'static str {
    let sync_status = output
        .pointer("/status/sync/status")
        .and_then(Value::as_str);
    let health_status = output
        .pointer("/status/health/status")
        .and_then(Value::as_str);

    match (sync_status, health_status) {
        (Some("Synced"), Some("Healthy")) => "satisfied",
        (Some(_), Some(_)) => "attention_required",
        (Some("Synced"), None) | (None, Some("Healthy")) => "unknown",
        (Some(_), None) | (None, Some(_)) => "attention_required",
        (None, None) => "unknown",
    }
}

pub(in crate::app) fn deployment_intent_json(
    pipeline_intent: &StoredPipelineIntent,
    intent_kind: &str,
    target_environment: Option<&str>,
    target_namespace: Option<&str>,
    argo_application: Option<&str>,
    intent_json: Option<serde_json::Value>,
) -> Result<serde_json::Value, ApiError> {
    if let Some(intent_json) = intent_json {
        if !intent_json.is_object() {
            return Err(ApiError::bad_request(
                "deployment intent intent_json must be a JSON object",
            ));
        }
        return Ok(intent_json);
    }

    Ok(json!({
        "execution": {
            "enabled": false,
            "reason": "DeploymentIntent is review state only in V1"
        },
        "source": {
            "pipeline_intent_id": pipeline_intent.id,
            "change_set_id": pipeline_intent.change_set_id,
            "work_plan_id": pipeline_intent.work_plan_id,
        },
        "pipeline_evidence": deployment_pipeline_evidence_json(pipeline_intent),
        "deployment": {
            "provider": "argo_cd",
            "intent_kind": intent_kind,
            "target_environment": target_environment,
            "target_namespace": target_namespace,
            "argo_application": argo_application,
            "operation": "sync"
        }
    }))
}

pub(in crate::app) fn deployment_pipeline_evidence_json(
    pipeline_intent: &StoredPipelineIntent,
) -> Value {
    let Some(evidence) = pipeline_intent.intent_json.get("evidence") else {
        return json!({
            "status": "missing",
            "deploy_ready": false,
            "review_required": true,
            "source": "pipeline_intent",
            "pipeline_intent_id": pipeline_intent.id,
            "summary": "No PipelineRunAnalysis evidence is attached to the approved PipelineIntent"
        });
    };

    let status = evidence
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    json!({
        "status": status,
        "deploy_ready": status == "satisfied",
        "review_required": status != "satisfied",
        "source": "pipeline_intent.evidence",
        "pipeline_intent_id": pipeline_intent.id,
        "observation_id": evidence.get("observation_id").cloned().unwrap_or(Value::Null),
        "artifact_id": evidence.get("artifact_id").cloned().unwrap_or(Value::Null),
        "summary": evidence.get("summary").cloned().unwrap_or_else(|| json!({})),
        "evidence": evidence.clone()
    })
}

pub(in crate::app) fn validate_deployment_intent_transition(
    current: &str,
    target: &str,
) -> Result<(), ApiError> {
    match (current, target) {
        ("proposed", "approved" | "rejected") => Ok(()),
        ("approved", "rejected") => Ok(()),
        (_, "proposed") if current == target => Ok(()),
        _ => Err(ApiError::conflict(format!(
            "cannot transition deployment intent from {current} to {target}"
        ))),
    }
}
