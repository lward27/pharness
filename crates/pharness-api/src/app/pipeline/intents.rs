use super::super::approvals::{
    append_permission_grant_audit_event, create_permission_grant_record,
    ensure_approved_for_trusted_envelope,
};
use super::super::audit::{append_pipeline_intent_audit_event, append_work_item_audit_event};
use super::super::auth::OperatorIdentity;
use super::super::clock::{current_millis, unique_suffix};
use super::super::gitops::change_sets::safe_relative_gitops_path;
use super::super::identifiers::{is_git_sha, is_sha256_digest, safe_id_fragment};
use super::super::source::delivery_flow::{
    git_delivery_artifact_matches_plan, git_delivery_plan_matches_change_set,
};
use super::super::validation::{clean_optional_text, required_json_string, required_text};
use super::super::work_items::preflight::{
    bounded_production_grant_expiry, work_item_target_supported,
};
use super::super::{ApiError, AppState};
use super::evidence::pipeline_intent_json_with_evidence;
use super::execution::{
    execution_matches_pipeline_contract, immutable_pipeline_source_revision,
    safe_oci_image_component, tekton_execution_spec,
};
use super::state::{
    pipeline_execution_attempt, pipeline_intent_execution_state,
    pipeline_intent_is_gitops_update_eligible, MAX_PIPELINE_EXECUTION_ATTEMPTS,
};
use crate::dto::{
    AttachPipelineIntentEvidenceRequest, AttachPipelineIntentEvidenceResponse,
    CreateGitOpsUpdatePlanRequest, CreatePermissionGrantRequest,
    CreatePipelineIntentFromChangeSetRequest, CreatePipelineIntentResponse,
    CreatePipelineIntentTrustedEnvelopeRequest, CreateWorkItemPipelineIntentRequest,
    GitOpsUpdatePlanResponse, PipelineIntentResponse, PipelineIntentsResponse,
    TransitionPipelineIntentRequest, TransitionPipelineIntentResponse, TrustedEnvelopeResponse,
    WorkItemPipelineContextResponse,
};
use axum::extract::{Path, Query, State};
use axum::{Extension, Json};
use pharness_core::RunId;
use pharness_store::{
    CreateArtifact, CreatePipelineIntent, PipelineContractListFilter, PipelineIntentListFilter,
    SqliteStore, StoredArtifact, StoredChangeSet, StoredObservation, StoredPipelineIntent,
    UpdatePipelineIntentDraft, UpdatePipelineIntentEvidence,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(in crate::app) async fn create_work_item_pipeline_intent(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(work_item_id): Path<String>,
    Json(request): Json<CreateWorkItemPipelineIntentRequest>,
) -> Result<Json<CreatePipelineIntentResponse>, ApiError> {
    let work_item = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("WorkItem has no WorkPlan"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?
        .ok_or_else(|| {
            ApiError::conflict(
                "WorkItem has no captured ChangeSet; source review and immutable merge evidence are required before a PipelineIntent",
            )
        })?;
    if change_set.work_item_id.as_deref() != Some(work_item.id.as_str()) {
        return Err(ApiError::conflict(
            "WorkItem ChangeSet lineage does not match the requested WorkItem",
        ));
    }

    let source_provenance = work_item_pipeline_source_provenance(&state.store, &change_set)
        .await?
        .ok_or_else(|| {
            ApiError::conflict(
                "WorkItem PipelineIntent requires immutable Git merge provenance before a pipeline definition",
            )
        })?;
    let pipeline_contract_id = required_text(request.pipeline_contract_id, "pipeline_contract_id")?;
    let pipeline_contract = state
        .store
        .get_pipeline_contract(&pipeline_contract_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_contract", &pipeline_contract_id))?;
    if pipeline_contract.status != "active" {
        return Err(ApiError::conflict(format!(
            "WorkItem PipelineIntent requires an active PipelineContract; {} is {}",
            pipeline_contract.id, pipeline_contract.status
        )));
    }
    let mut intent_json = request.intent_json.ok_or_else(|| {
        ApiError::bad_request(
            "WorkItem PipelineIntent requires an exact enabled Tekton execution definition",
        )
    })?;
    let execution = tekton_execution_spec(&intent_json)?;
    if !execution.enabled {
        return Err(ApiError::conflict(
            "WorkItem PipelineIntent execution must be enabled before it can be reviewed against a PipelineContract",
        ));
    }
    let source_revision = required_json_string(
        source_provenance.as_object().ok_or_else(|| {
            ApiError::internal("WorkItem source provenance must have an object body")
        })?,
        "merge_commit_sha",
        "WorkItem source provenance",
    )?;
    execution_matches_pipeline_contract(&execution, &pipeline_contract, Some(&source_revision))?;
    let intent_object = intent_json.as_object_mut().ok_or_else(|| {
        ApiError::bad_request("WorkItem PipelineIntent intent_json must be a JSON object")
    })?;
    intent_object.insert(
        "pipeline_contract".to_string(),
        json!({
            "id": pipeline_contract.id,
            "version": pipeline_contract.version,
            "namespace": pipeline_contract.namespace,
            "pipeline_ref": pipeline_contract.pipeline_ref,
        }),
    );
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    let Json(response) = create_pipeline_intent_from_change_set(
        State(state.clone()),
        Json(CreatePipelineIntentFromChangeSetRequest {
            change_set_id: change_set.id.clone(),
            title: request.title,
            summary: request.summary,
            risk_level: request.risk_level,
            intent_kind: request.intent_kind,
            intent_json: Some(intent_json),
            actor: actor.clone(),
            reason: reason.clone(),
        }),
    )
    .await?;
    if response.created {
        append_work_item_audit_event(
            &state.store,
            &work_item,
            "work_item.pipeline_intent_proposed",
            actor,
            json!({
                "work_plan_id": work_plan.id,
                "change_set_id": change_set.id,
                "pipeline_intent_id": response.pipeline_intent.id,
                "pipeline_contract_id": pipeline_contract.id,
                "pipeline_contract_version": pipeline_contract.version,
                "source_provenance": source_provenance,
                "reason": reason,
            }),
        )
        .await?;
    }

    Ok(Json(response))
}

#[derive(Debug, Default, serde::Deserialize)]
pub(in crate::app) struct WorkItemPipelineContextQuery {
    pub(in crate::app) namespace: Option<String>,
    pub(in crate::app) pipeline_ref: Option<String>,
}

pub(in crate::app) async fn work_item_pipeline_intent_context(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
    Query(query): Query<WorkItemPipelineContextQuery>,
) -> Result<Json<WorkItemPipelineContextResponse>, ApiError> {
    let work_item = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("WorkItem has no WorkPlan"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?
        .ok_or_else(|| {
            ApiError::conflict(
                "WorkItem has no captured ChangeSet; immutable source provenance is unavailable",
            )
        })?;
    if change_set.work_item_id.as_deref() != Some(work_item.id.as_str()) {
        return Err(ApiError::conflict(
            "WorkItem ChangeSet lineage does not match the requested WorkItem",
        ));
    }
    let source_provenance = work_item_pipeline_source_provenance(&state.store, &change_set)
        .await?
        .ok_or_else(|| {
            ApiError::conflict("WorkItem pipeline context requires immutable Git merge provenance")
        })?;
    let contract_namespace = clean_optional_text(query.namespace);
    let contract_pipeline_ref = clean_optional_text(query.pipeline_ref);
    let active_pipeline_contracts = state
        .store
        .list_pipeline_contracts(PipelineContractListFilter {
            namespace: contract_namespace.clone(),
            pipeline_ref: contract_pipeline_ref.clone(),
            status: Some("active".to_string()),
            limit: 200,
            offset: 0,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    let pipeline_intent = state
        .store
        .get_pipeline_intent_by_change_set(&change_set.id)
        .await?
        .map(Into::into);

    Ok(Json(WorkItemPipelineContextResponse {
        work_item: work_item.into(),
        work_plan: work_plan.into(),
        change_set: change_set.into(),
        pipeline_intent,
        source_provenance,
        contract_namespace,
        contract_pipeline_ref,
        active_pipeline_contracts,
    }))
}

#[derive(Debug, Default, serde::Deserialize)]
pub(in crate::app) struct ListPipelineIntentsQuery {
    pub(in crate::app) change_set_id: Option<String>,
    pub(in crate::app) work_plan_id: Option<String>,
    pub(in crate::app) remediation_plan_id: Option<String>,
    pub(in crate::app) incident_id: Option<String>,
    pub(in crate::app) run_id: Option<String>,
    pub(in crate::app) status: Option<String>,
    pub(in crate::app) intent_kind: Option<String>,
    pub(in crate::app) risk_level: Option<String>,
    pub(in crate::app) resource_namespace: Option<String>,
    pub(in crate::app) resource_kind: Option<String>,
    pub(in crate::app) resource_name: Option<String>,
    pub(in crate::app) created_after_ms: Option<i64>,
    pub(in crate::app) created_before_ms: Option<i64>,
    pub(in crate::app) limit: Option<u32>,
    pub(in crate::app) offset: Option<u32>,
}

pub(in crate::app) async fn list_pipeline_intents(
    State(state): State<AppState>,
    Query(query): Query<ListPipelineIntentsQuery>,
) -> Result<Json<PipelineIntentsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let pipeline_intents = state
        .store
        .list_pipeline_intents(PipelineIntentListFilter {
            change_set_id: clean_optional_text(query.change_set_id),
            work_plan_id: clean_optional_text(query.work_plan_id),
            remediation_plan_id: clean_optional_text(query.remediation_plan_id),
            incident_id: clean_optional_text(query.incident_id),
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            status: clean_optional_text(query.status),
            intent_kind: clean_optional_text(query.intent_kind),
            risk_level: clean_optional_text(query.risk_level),
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
    let count = pipeline_intents.len();

    Ok(Json(PipelineIntentsResponse {
        pipeline_intents,
        count,
        limit,
        offset,
    }))
}

pub(in crate::app) async fn get_pipeline_intent(
    State(state): State<AppState>,
    Path(pipeline_intent_id): Path<String>,
) -> Result<Json<PipelineIntentResponse>, ApiError> {
    let intent = state
        .store
        .get_pipeline_intent(&pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", &pipeline_intent_id))?;

    Ok(Json(intent.into()))
}

pub(in crate::app) async fn create_pipeline_intent_from_change_set(
    State(state): State<AppState>,
    Json(request): Json<CreatePipelineIntentFromChangeSetRequest>,
) -> Result<Json<CreatePipelineIntentResponse>, ApiError> {
    let CreatePipelineIntentFromChangeSetRequest {
        change_set_id,
        title,
        summary,
        risk_level,
        intent_kind,
        intent_json,
        actor,
        reason,
    } = request;
    let change_set_id = clean_optional_text(Some(change_set_id))
        .ok_or_else(|| ApiError::bad_request("change_set_id is required"))?;
    let existing = state
        .store
        .get_pipeline_intent_by_change_set(&change_set_id)
        .await?;
    if let Some(existing) = existing
        .as_ref()
        .filter(|existing| existing.status != "stale")
    {
        return Ok(Json(CreatePipelineIntentResponse {
            pipeline_intent: existing.clone().into(),
            created: false,
        }));
    }

    let change_set = state
        .store
        .get_change_set(&change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &change_set_id))?;
    let work_plan = state
        .store
        .get_work_plan(&change_set.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &change_set.work_plan_id))?;
    ensure_approved_for_trusted_envelope("work_plan", &work_plan.id, &work_plan.status)?;
    ensure_approved_for_trusted_envelope("change_set", &change_set.id, &change_set.status)?;

    let source_provenance = work_item_pipeline_source_provenance(&state.store, &change_set).await?;
    let actor = clean_optional_text(actor);
    let reason = clean_optional_text(reason);
    let mut draft = pipeline_intent_draft(
        &change_set,
        PipelineIntentDraftRequest {
            title,
            summary,
            risk_level,
            intent_kind,
            intent_json,
            actor: actor.clone(),
            reason: reason.clone(),
        },
    )?;
    if let Some(source_provenance) = source_provenance {
        let object = draft
            .intent_json
            .as_object_mut()
            .ok_or_else(|| ApiError::internal("pipeline intent draft must have an object body"))?;
        object.insert("source_provenance".to_string(), source_provenance);
    }
    if let Some(existing) = existing {
        let previous_status = existing.status.clone();
        let pipeline_intent = state
            .store
            .revise_pipeline_intent_draft(&existing.id, draft)
            .await?;
        append_pipeline_intent_audit_event(
            &state.store,
            &pipeline_intent,
            "pipeline_intent.reproposed",
            actor,
            reason,
            json!({
                "source": "change_set",
                "previous_status": previous_status,
                "change_set_id": pipeline_intent.change_set_id,
                "work_plan_id": pipeline_intent.work_plan_id,
                "execution_enabled": false,
            }),
        )
        .await?;

        return Ok(Json(CreatePipelineIntentResponse {
            pipeline_intent: pipeline_intent.into(),
            created: false,
        }));
    }

    let pipeline_intent = state
        .store
        .create_pipeline_intent(CreatePipelineIntent {
            id: format!("pint_{}", unique_suffix()),
            change_set_id: change_set.id.clone(),
            work_plan_id: work_plan.id.clone(),
            remediation_plan_id: change_set.remediation_plan_id.clone(),
            incident_id: change_set.incident_id.clone(),
            session_id: change_set.session_id.clone(),
            run_id: change_set.run_id.clone(),
            status: "proposed".to_string(),
            title: draft.title,
            summary: draft.summary,
            risk_level: draft.risk_level,
            intent_kind: draft.intent_kind,
            resource_namespace: draft.resource_namespace,
            resource_kind: draft.resource_kind,
            resource_name: draft.resource_name,
            intent_json: draft.intent_json,
        })
        .await?;
    append_pipeline_intent_audit_event(
        &state.store,
        &pipeline_intent,
        "pipeline_intent.proposed",
        actor,
        reason,
        json!({
            "source": "change_set",
            "change_set_id": pipeline_intent.change_set_id,
            "work_plan_id": pipeline_intent.work_plan_id,
            "execution_enabled": false,
        }),
    )
    .await?;

    Ok(Json(CreatePipelineIntentResponse {
        pipeline_intent: pipeline_intent.into(),
        created: true,
    }))
}

pub(in crate::app) async fn work_item_pipeline_source_provenance(
    store: &SqliteStore,
    change_set: &StoredChangeSet,
) -> Result<Option<Value>, ApiError> {
    if change_set.work_item_id.is_none() {
        return Ok(None);
    }
    if let Some(provenance) = super::hosted::source_provenance(store, change_set).await? {
        return Ok(Some(provenance));
    }
    let run_id = change_set.run_id.as_ref().ok_or_else(|| {
        ApiError::conflict("WorkItem PipelineIntent requires coding run provenance")
    })?;
    let artifacts = store.list_artifacts(run_id).await?;
    let plan = artifacts
        .iter()
        .find(|artifact| git_delivery_plan_matches_change_set(artifact, change_set))
        .ok_or_else(|| {
            ApiError::conflict(
                "WorkItem PipelineIntent requires the current Git delivery plan before build",
            )
        })?;
    let merge = artifacts
        .iter()
        .filter(|artifact| git_delivery_artifact_matches_plan(artifact, "git_delivery_merge", &plan.id))
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .ok_or_else(|| {
            ApiError::conflict(
                "WorkItem PipelineIntent requires observed GitHub merge evidence; a mutable PR branch is not a build source",
            )
        })?;
    let merge_content = merge
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git merge evidence has no structured provenance"))?;
    let merge_commit_sha =
        required_json_string(merge_content, "merge_commit_sha", "Git merge evidence")?;
    let head_commit_sha =
        required_json_string(merge_content, "head_commit_sha", "Git merge evidence")?;
    if !is_git_sha(&merge_commit_sha) || !is_git_sha(&head_commit_sha) {
        return Err(ApiError::conflict(
            "Git merge evidence has invalid commit provenance",
        ));
    }
    Ok(Some(json!({
        "kind": "github_merged_pull_request",
        "immutable": true,
        "git_delivery_plan_artifact_id": plan.id,
        "git_delivery_merge_artifact_id": merge.id,
        "repository": plan.content_json.as_ref().and_then(|value| value.pointer("/source/repository")).and_then(Value::as_str),
        "base_commit": plan.content_json.as_ref().and_then(|value| value.pointer("/source/base_commit")).and_then(Value::as_str),
        "head_commit_sha": head_commit_sha,
        "merge_commit_sha": merge_commit_sha,
        "pull_request_url": merge_content.get("pull_request_url"),
        "pull_request_number": merge_content.get("pull_request_number"),
    })))
}

pub(in crate::app) struct PipelineIntentDraftRequest {
    title: Option<String>,
    summary: Option<String>,
    risk_level: Option<String>,
    intent_kind: Option<String>,
    intent_json: Option<serde_json::Value>,
    actor: Option<String>,
    reason: Option<String>,
}

pub(in crate::app) fn pipeline_intent_draft(
    change_set: &StoredChangeSet,
    request: PipelineIntentDraftRequest,
) -> Result<UpdatePipelineIntentDraft, ApiError> {
    let intent_kind = clean_optional_text(request.intent_kind)
        .unwrap_or_else(|| "tekton_build_test_package".to_string());
    let intent_json = pipeline_intent_json(change_set, &intent_kind, request.intent_json)?;

    Ok(UpdatePipelineIntentDraft {
        title: clean_optional_text(request.title)
            .unwrap_or_else(|| format!("PipelineIntent: {}", change_set.title)),
        summary: clean_optional_text(request.summary).unwrap_or_else(|| {
            "Propose Tekton build/test/package for approved ChangeSet".to_string()
        }),
        risk_level: clean_optional_text(request.risk_level)
            .unwrap_or_else(|| change_set.risk_level.clone()),
        intent_kind,
        resource_namespace: change_set.resource_namespace.clone(),
        resource_kind: change_set.resource_kind.clone(),
        resource_name: change_set.resource_name.clone(),
        intent_json,
        actor: request.actor,
        reason: request.reason,
    })
}

pub(in crate::app) async fn transition_pipeline_intent(
    State(state): State<AppState>,
    Path(pipeline_intent_id): Path<String>,
    Json(request): Json<TransitionPipelineIntentRequest>,
) -> Result<Json<TransitionPipelineIntentResponse>, ApiError> {
    let current = state
        .store
        .get_pipeline_intent(&pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", &pipeline_intent_id))?;
    let target = clean_optional_text(Some(request.target_status))
        .ok_or_else(|| ApiError::bad_request("target_status is required"))?;
    validate_pipeline_intent_transition(&current.status, &target)?;
    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let pipeline_intent = state
        .store
        .update_pipeline_intent_status(&pipeline_intent_id, &target, actor.clone(), reason.clone())
        .await?;
    append_pipeline_intent_audit_event(
        &state.store,
        &pipeline_intent,
        &format!("pipeline_intent.{target}"),
        actor,
        reason,
        json!({
            "previous_status": current.status,
            "status": pipeline_intent.status,
        }),
    )
    .await?;

    Ok(Json(TransitionPipelineIntentResponse {
        pipeline_intent: pipeline_intent.into(),
    }))
}

pub(in crate::app) async fn retry_failed_pipeline_intent(
    state: &AppState,
    pipeline_intent_id: &str,
    actor: String,
    reason: String,
) -> Result<PipelineIntentResponse, ApiError> {
    let current = state
        .store
        .get_pipeline_intent(pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", pipeline_intent_id))?;
    if current.status != "failed"
        || !matches!(
            pipeline_intent_execution_state(&current),
            Some("pipeline_run_failed" | "failed" | "dispatch_failed")
        )
        || current
            .intent_json
            .pointer("/execution_evidence/status")
            .and_then(Value::as_str)
            != Some("failed")
    {
        return Err(ApiError::conflict(
            "PipelineIntent retry requires durable terminal failure evidence",
        ));
    }
    let execution_attempt = pipeline_execution_attempt(&current.intent_json)?;
    if execution_attempt >= MAX_PIPELINE_EXECUTION_ATTEMPTS {
        return Err(ApiError::conflict(format!(
            "PipelineIntent has used all {MAX_PIPELINE_EXECUTION_ATTEMPTS} supervised execution attempts"
        )));
    }
    let change_set = state
        .store
        .get_change_set(&current.change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &current.change_set_id))?;
    let work_plan = state
        .store
        .get_work_plan(&current.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &current.work_plan_id))?;
    if change_set.status != "approved" || work_plan.status != "approved" {
        return Err(ApiError::conflict(
            "PipelineIntent retry requires the original approved WorkPlan and ChangeSet",
        ));
    }
    if state
        .store
        .get_deployment_intent_by_pipeline_intent(&current.id)
        .await?
        .is_some()
        || state
            .store
            .get_gitops_change_set_by_pipeline_intent(&current.id)
            .await?
            .is_some()
    {
        return Err(ApiError::conflict(
            "PipelineIntent retry is disabled after downstream delivery has started",
        ));
    }
    if change_set.work_item_id.is_some()
        && immutable_pipeline_source_revision(&current, true)?.is_none()
    {
        return Err(ApiError::conflict(
            "WorkItem PipelineIntent retry requires immutable source merge provenance",
        ));
    }

    let previous_execution_id = current
        .intent_json
        .pointer("/execution_state/execution_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("failed execution ID is unavailable"))?
        .to_string();
    let previous_pipeline_run_name = current
        .intent_json
        .pointer("/execution_state/pipeline_run_name")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("failed PipelineRun name is unavailable"))?
        .to_string();
    let failure_artifact_id = current
        .intent_json
        .pointer("/execution_evidence/artifact_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("failed execution artifact is unavailable"))?
        .to_string();
    let previous_permission_grant_id = current
        .intent_json
        .pointer("/execution_state/permission_grant_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let next_attempt = execution_attempt + 1;
    let mut intent_json = current.intent_json.clone();
    let history_entry = json!({
        "attempt": execution_attempt,
        "status": current.status,
        "execution_state": intent_json.get("execution_state"),
        "execution_evidence": intent_json.get("execution_evidence"),
        "pipeline_run_analysis": intent_json.get("evidence"),
        "build_output": intent_json.get("build_output"),
    });
    let object = intent_json
        .as_object_mut()
        .ok_or_else(|| ApiError::conflict("PipelineIntent body must be an object"))?;
    let history = object
        .entry("execution_history")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| ApiError::conflict("PipelineIntent execution_history must be an array"))?;
    history.push(history_entry);
    object.remove("execution_state");
    object.remove("execution_evidence");
    object.remove("evidence");
    object.remove("build_output");
    object.insert("execution_attempt".to_string(), json!(next_attempt));
    object.insert(
        "retry_context".to_string(),
        json!({
            "previous_attempt": execution_attempt,
            "previous_execution_id": previous_execution_id,
            "previous_pipeline_run_name": previous_pipeline_run_name,
            "failure_artifact_id": failure_artifact_id,
            "reproposed_at": current_millis(),
            "reproposed_by": actor,
            "reason": reason,
        }),
    );

    if let Some(grant_id) = previous_permission_grant_id.as_deref() {
        if let Some(grant) = state.store.get_permission_grant(grant_id).await? {
            if grant.status == "active" {
                let revoked = state
                    .store
                    .revoke_permission_grant(
                        grant_id,
                        Some(actor.clone()),
                        Some(format!(
                            "superseded by supervised PipelineIntent execution attempt {next_attempt}"
                        )),
                    )
                    .await?;
                append_permission_grant_audit_event(
                    &state.store,
                    "permission_grant.revoked",
                    &revoked,
                    Some(actor.clone()),
                )
                .await?;
            }
        }
    }

    let pipeline_intent = state
        .store
        .revise_pipeline_intent_draft(
            &current.id,
            UpdatePipelineIntentDraft {
                title: current.title.clone(),
                summary: current.summary.clone(),
                risk_level: current.risk_level.clone(),
                intent_kind: current.intent_kind.clone(),
                resource_namespace: current.resource_namespace.clone(),
                resource_kind: current.resource_kind.clone(),
                resource_name: current.resource_name.clone(),
                intent_json,
                actor: Some(actor.clone()),
                reason: Some(reason.clone()),
            },
        )
        .await?;
    append_pipeline_intent_audit_event(
        &state.store,
        &pipeline_intent,
        "pipeline_intent.retry_proposed",
        Some(actor),
        Some(reason),
        json!({
            "previous_attempt": execution_attempt,
            "execution_attempt": next_attempt,
            "previous_execution_id": previous_execution_id,
            "previous_pipeline_run_name": previous_pipeline_run_name,
            "failure_artifact_id": failure_artifact_id,
            "previous_permission_grant_id": previous_permission_grant_id,
            "automatic_execution": false,
        }),
    )
    .await?;

    Ok(pipeline_intent.into())
}

pub(in crate::app) async fn attach_pipeline_intent_evidence(
    State(state): State<AppState>,
    Path(pipeline_intent_id): Path<String>,
    Json(request): Json<AttachPipelineIntentEvidenceRequest>,
) -> Result<Json<AttachPipelineIntentEvidenceResponse>, ApiError> {
    let current = state
        .store
        .get_pipeline_intent(&pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", &pipeline_intent_id))?;
    if current.status == "stale" {
        return Err(ApiError::conflict(format!(
            "cannot attach evidence to stale pipeline intent {pipeline_intent_id}"
        )));
    }

    let observation_id = clean_optional_text(Some(request.observation_id))
        .ok_or_else(|| ApiError::bad_request("observation_id is required"))?;
    let observation = state
        .store
        .get_observation(&observation_id)
        .await?
        .ok_or_else(|| ApiError::not_found("observation", &observation_id))?;
    validate_pipeline_intent_observation(&current, &observation)?;

    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let intent_json = pipeline_intent_json_with_evidence(&current, &observation);
    let pipeline_intent = state
        .store
        .update_pipeline_intent_evidence(
            &pipeline_intent_id,
            UpdatePipelineIntentEvidence {
                intent_json,
                actor: actor.clone(),
                reason: reason.clone(),
            },
        )
        .await?;
    append_pipeline_intent_audit_event(
        &state.store,
        &pipeline_intent,
        "pipeline_intent.evidence_attached",
        actor,
        reason,
        json!({
            "observation_id": observation.id,
            "artifact_id": observation.artifact_id,
            "evidence_status": pipeline_intent.intent_json.pointer("/evidence/status"),
            "resource": {
                "namespace": observation.resource_namespace,
                "kind": observation.resource_kind,
                "name": observation.resource_name,
            },
        }),
    )
    .await?;

    Ok(Json(AttachPipelineIntentEvidenceResponse {
        pipeline_intent: pipeline_intent.into(),
        observation: observation.into(),
    }))
}

pub(in crate::app) async fn create_pipeline_intent_trusted_envelope(
    State(state): State<AppState>,
    Path(pipeline_intent_id): Path<String>,
    Json(request): Json<CreatePipelineIntentTrustedEnvelopeRequest>,
) -> Result<Json<TrustedEnvelopeResponse>, ApiError> {
    let intent = state
        .store
        .get_pipeline_intent(&pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", &pipeline_intent_id))?;
    if super::hosted::is_hosted(&state.store, &intent).await? {
        return Err(ApiError::conflict("Hosted build authority is derived from the saved workflow by its controller. A manual envelope cannot extend that authority."));
    }
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
    ensure_approved_for_trusted_envelope("pipeline_intent", &intent.id, &intent.status)?;
    let execution = tekton_execution_spec(&intent.intent_json)?;
    let work_item = match work_plan.work_item_id.as_deref() {
        Some(work_item_id) => Some(
            state
                .store
                .get_work_item(work_item_id)
                .await?
                .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?,
        ),
        None => None,
    };
    if let Some(item) = work_item.as_ref() {
        if !work_item_target_supported(item) {
            return Err(ApiError::conflict(
                "Pipeline trusted envelope target is outside the supported dev or protected-production boundary",
            ));
        }
        if item.production_impacting != execution.production_impacting {
            return Err(ApiError::conflict(
                "Pipeline production impact must exactly match its WorkItem",
            ));
        }
    }
    let reason = clean_optional_text(Some(request.reason.clone()))
        .ok_or_else(|| ApiError::bad_request("trusted envelope reason is required"))?;
    let subject =
        clean_optional_text(request.subject).unwrap_or_else(|| state.policy.subject.clone());
    let environment = work_item
        .as_ref()
        .map(|item| item.target_environment.clone())
        .unwrap_or_else(|| state.policy.environment.clone());
    let expires_at = match work_item.as_ref() {
        Some(item) => bounded_production_grant_expiry(item, request.expires_at)?,
        None => request.expires_at,
    };
    let grant = create_permission_grant_record(
        &state.store,
        CreatePermissionGrantRequest {
            subject,
            created_by: clean_optional_text(request.created_by.clone()),
            reason: reason.clone(),
            scope: json!({
                "environment": environment,
                "capability_kinds": ["tekton_start_run"],
                "actions": ["tekton_trigger_pipeline"],
                "max_risk": "high",
                "namespaces": [execution.namespace],
                "work_plan_ids": [intent.work_plan_id],
                "change_set_ids": [intent.change_set_id],
                "pipeline_intent_ids": [intent.id],
                "production_impacting": execution.production_impacting,
            }),
            policy: json!({ "policy_mode": "supervised_autonomy" }),
            expires_at,
        },
    )
    .await?;
    append_pipeline_intent_audit_event(
        &state.store,
        &intent,
        "pipeline_intent.trusted_envelope_created",
        clean_optional_text(request.created_by),
        Some(reason),
        json!({ "permission_grant_id": grant.id }),
    )
    .await?;

    Ok(Json(TrustedEnvelopeResponse {
        grant: grant.into(),
    }))
}

/// Prepare a reviewable, digest-pinned Kustomize update. This is deliberately
/// a durable plan only: a later GitOps ChangeSet/PR executor must consume this
/// exact artifact rather than treating Argo sync as source provenance.
pub(in crate::app) async fn create_gitops_update_plan(
    State(state): State<AppState>,
    Path(pipeline_intent_id): Path<String>,
    Json(request): Json<CreateGitOpsUpdatePlanRequest>,
) -> Result<Json<GitOpsUpdatePlanResponse>, ApiError> {
    let intent = state
        .store
        .get_pipeline_intent(&pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", &pipeline_intent_id))?;
    if !pipeline_intent_is_gitops_update_eligible(&intent) {
        return Err(ApiError::conflict(
            "GitOps update planning requires an eligible PipelineIntent with satisfied PipelineRunAnalysis evidence",
        ));
    }
    let work_plan = state
        .store
        .get_work_plan(&intent.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &intent.work_plan_id))?;
    let work_item_id = work_plan.work_item_id.as_deref().ok_or_else(|| {
        ApiError::conflict("GitOps update planning requires a WorkItem-backed PipelineIntent")
    })?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    if !work_item_target_supported(&work_item) {
        return Err(ApiError::conflict(
            "GitOps update planning is limited to dev or the exact protected production target",
        ));
    }
    let gitops_repo = work_item.gitops_repo.clone().ok_or_else(|| {
        ApiError::conflict("WorkItem must declare gitops_repo before GitOps update planning")
    })?;
    let gitops_ref = work_item.gitops_ref.clone().ok_or_else(|| {
        ApiError::conflict("WorkItem must declare gitops_ref before GitOps update planning")
    })?;
    let kustomization_path = required_text(request.kustomization_path, "kustomization_path")?;
    if !safe_relative_gitops_path(&kustomization_path) {
        return Err(ApiError::bad_request(
            "kustomization_path must be a safe relative repository path",
        ));
    }
    let image_name = required_text(request.image_name, "image_name")?;
    let deployment_intent = state
        .store
        .get_deployment_intent_by_pipeline_intent(&intent.id)
        .await?
        .ok_or_else(|| ApiError::conflict("PipelineIntent has no declared DeploymentIntent"))?;
    let run_id = intent.run_id.clone().ok_or_else(|| {
        ApiError::conflict("GitOps update planning requires pipeline run provenance")
    })?;
    let artifacts = state.store.list_artifacts(&run_id).await?;
    let build_output = current_pipeline_build_output(&artifacts, &intent)?;
    let requested_image_ref = clean_optional_text(request.image_ref);
    let image_ref = match (requested_image_ref, build_output.as_ref()) {
        (Some(requested), Some(output)) => {
            if requested != output.image_reference {
                return Err(ApiError::conflict(
                    "explicit GitOps image_ref does not match the verified PipelineRun build output",
                ));
            }
            requested
        }
        (Some(requested), None) => requested,
        (None, Some(output)) => output.image_reference.clone(),
        (None, None) => {
            return Err(ApiError::conflict(
                "GitOps image_ref is required until the PipelineRun records a verified digest-pinned build output",
            ))
        }
    };
    if !valid_digest_pinned_image_reference(&image_ref) {
        return Err(ApiError::bad_request(
            "GitOps image_ref must be a valid digest-pinned image with @sha256:<64 hex>",
        ));
    }
    let material_hash = format!(
        "{:x}",
        Sha256::digest(
            format!(
                "{}\n{}\n{}\n{}\n{}",
                gitops_repo, gitops_ref, kustomization_path, image_name, image_ref
            )
            .as_bytes()
        )
    );
    if let Some(existing) = artifacts.iter().find(|artifact| {
        artifact.kind == "gitops_update_plan"
            && artifact.content_json.as_ref().is_some_and(|content| {
                content.get("pipeline_intent_id").and_then(Value::as_str)
                    == Some(intent.id.as_str())
                    && content.get("material_hash").and_then(Value::as_str)
                        == Some(material_hash.as_str())
            })
    }) {
        return Ok(Json(GitOpsUpdatePlanResponse {
            artifact: existing.clone().into(),
            created: false,
        }));
    }
    let artifact = state.store.create_artifact(CreateArtifact {
        id: format!("art_{}_gitops_update_plan", unique_suffix()), session_id: intent.session_id.clone(), run_id: Some(run_id),
        kind: "gitops_update_plan".to_string(), label: format!("GitOps update plan for PipelineIntent {}", intent.id),
        mime_type: Some("application/json".to_string()), path: None, content_text: None,
        content_json: Some(json!({
            "kind": "gitops_update_plan", "version": 1, "operation": "kustomize_set_image", "material_hash": material_hash,
            "work_item_id": work_item.id, "work_plan_id": work_plan.id, "change_set_id": intent.change_set_id,
            "pipeline_intent_id": intent.id, "deployment_intent_id": deployment_intent.id,
            "gitops": { "repository": gitops_repo, "base_ref": gitops_ref, "head_branch": format!("pharness/gitops/{}/{}", safe_id_fragment(&work_item.id), safe_id_fragment(&intent.id)) },
            "build_output": build_output.as_ref().map(|output| json!({
                "artifact_id": output.artifact_id,
                "image_url": output.image_url,
                "image_digest": output.image_digest,
                "source_commit": output.source_commit,
            })),
            "update": { "kustomization_path": kustomization_path, "image_name": image_name, "new_image": image_ref },
            "execution": { "enabled": false, "reason": "requires a reviewed GitOps ChangeSet and dedicated GitOps writer" }
        })),
    }).await?;
    append_pipeline_intent_audit_event(&state.store, &intent, "pipeline_intent.gitops_update_planned", clean_optional_text(request.actor), clean_optional_text(request.reason), json!({ "artifact_id": artifact.id, "material_hash": material_hash, "deployment_intent_id": deployment_intent.id })).await?;
    Ok(Json(GitOpsUpdatePlanResponse {
        artifact: artifact.into(),
        created: true,
    }))
}

#[derive(Debug, Clone)]
pub(in crate::app) struct VerifiedPipelineBuildOutput {
    pub(in crate::app) artifact_id: String,
    pub(in crate::app) image_url: String,
    pub(in crate::app) image_digest: String,
    pub(in crate::app) image_reference: String,
    pub(in crate::app) source_commit: Option<String>,
}

pub(in crate::app) fn current_pipeline_build_output(
    artifacts: &[StoredArtifact],
    intent: &StoredPipelineIntent,
) -> Result<Option<VerifiedPipelineBuildOutput>, ApiError> {
    let Some(artifact) = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "pipeline_build_output"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("pipeline_intent_id").and_then(Value::as_str)
                        == Some(intent.id.as_str())
                })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
    else {
        return Ok(None);
    };
    let content = artifact.content_json.as_ref().ok_or_else(|| {
        ApiError::conflict("Pipeline build-output artifact has no structured provenance")
    })?;
    if content.get("status").and_then(Value::as_str) != Some("verified") {
        return Err(ApiError::conflict(
            "Pipeline build-output provenance is not trusted for GitOps planning",
        ));
    }
    let image_url = content
        .pointer("/image/url")
        .and_then(Value::as_str)
        .filter(|value| safe_oci_image_component(value))
        .map(ToOwned::to_owned)
        .ok_or_else(|| ApiError::conflict("Pipeline build-output has no valid image URL"))?;
    let image_digest = content
        .pointer("/image/digest")
        .and_then(Value::as_str)
        .filter(|value| is_sha256_digest(value))
        .map(ToOwned::to_owned)
        .ok_or_else(|| ApiError::conflict("Pipeline build-output has invalid image digest"))?;
    let image_reference = content
        .pointer("/image/reference")
        .and_then(Value::as_str)
        .filter(|value| valid_digest_pinned_image_reference(value))
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            ApiError::conflict("Pipeline build-output has invalid digest-pinned image reference")
        })?;
    let source_commit = content
        .pointer("/source/commit")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .map(ToOwned::to_owned);
    Ok(Some(VerifiedPipelineBuildOutput {
        artifact_id: artifact.id.clone(),
        image_url,
        image_digest,
        image_reference,
        source_commit,
    }))
}

pub(in crate::app) fn valid_digest_pinned_image_reference(value: &str) -> bool {
    match value.rsplit_once('@') {
        Some((repository, digest)) => {
            safe_oci_image_component(repository) && is_sha256_digest(digest)
        }
        None => false,
    }
}

pub(in crate::app) fn validate_pipeline_intent_observation(
    intent: &StoredPipelineIntent,
    observation: &StoredObservation,
) -> Result<(), ApiError> {
    if observation.source != "tekton" || observation.kind != "pipeline_run_analysis" {
        return Err(ApiError::bad_request(
            "pipeline intent evidence must be a tekton pipeline_run_analysis observation",
        ));
    }
    if observation.data_json.pointer("/analysis").is_none() {
        return Err(ApiError::bad_request(
            "pipeline intent evidence observation is missing analysis data",
        ));
    }

    let expected_namespace = intent
        .intent_json
        .pointer("/execution_evidence/pipeline_run/namespace")
        .and_then(Value::as_str);
    let expected_name = intent
        .intent_json
        .pointer("/execution_evidence/pipeline_run/name")
        .and_then(Value::as_str);
    if let Some(expected_namespace) = expected_namespace {
        if observation.resource_namespace.as_deref() != Some(expected_namespace) {
            return Err(ApiError::bad_request(
                "pipeline intent evidence must match the executor PipelineRun namespace",
            ));
        }
    }
    if let Some(expected_name) = expected_name {
        if observation.resource_name.as_deref() != Some(expected_name) {
            return Err(ApiError::bad_request(
                "pipeline intent evidence must match the executor PipelineRun name",
            ));
        }
    }

    Ok(())
}

pub(in crate::app) fn pipeline_intent_json(
    change_set: &StoredChangeSet,
    intent_kind: &str,
    intent_json: Option<serde_json::Value>,
) -> Result<serde_json::Value, ApiError> {
    if let Some(intent_json) = intent_json {
        if !intent_json.is_object() {
            return Err(ApiError::bad_request(
                "pipeline intent intent_json must be a JSON object",
            ));
        }
        return Ok(intent_json);
    }

    Ok(json!({
        "execution": {
            "enabled": false,
            "reason": "PipelineIntent is review state only in V1"
        },
        "source": {
            "change_set_id": change_set.id,
            "work_plan_id": change_set.work_plan_id,
            "material_hash": change_set.material_hash,
            "revision": change_set.revision
        },
        "pipeline": {
            "provider": "tekton",
            "intent_kind": intent_kind,
            "tasks": ["test", "build", "package"]
        }
    }))
}

pub(in crate::app) fn validate_pipeline_intent_transition(
    current: &str,
    target: &str,
) -> Result<(), ApiError> {
    match (current, target) {
        ("proposed", "approved" | "rejected") => Ok(()),
        ("approved", "rejected") => Ok(()),
        (_, "proposed") if current == target => Ok(()),
        _ => Err(ApiError::conflict(format!(
            "cannot transition pipeline intent from {current} to {target}"
        ))),
    }
}
