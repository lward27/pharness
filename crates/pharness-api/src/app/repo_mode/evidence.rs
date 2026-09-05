use super::state::{repo_metadata, repo_work_item_state_hash};
use crate::app::identifiers::new_prefixed_id;
use crate::app::repository_readiness::ensure_repo_mode_enabled;
use crate::app::validation::required_text;
use crate::app::{ApiError, AppState};
use axum::extract::{Path, State};
use axum::Json;
use pharness_store::CreateOperatorAnnotation;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
pub(super) struct CreateAnnotationRequest {
    target_kind: String,
    target_id: String,
    statement: String,
    #[serde(default = "empty_array")]
    evidence_refs: Value,
    requested_effect: String,
    actor: String,
    reason: String,
    state_hash: String,
}

fn empty_array() -> Value {
    Value::Array(Vec::new())
}

pub(super) async fn list_stage_executions(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    repo_metadata(&state, &work_item_id).await?;
    let executions = state.store.list_stage_executions(&work_item_id).await?;
    let mut views = Vec::with_capacity(executions.len());
    for execution in &executions {
        views.push(stage_execution_view(&state, execution).await?);
    }
    Ok(Json(json!({
        "stage_executions": views,
        "count": executions.len(),
    })))
}

pub(super) async fn get_stage_execution(
    State(state): State<AppState>,
    Path(stage_execution_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let execution = state
        .store
        .get_stage_execution(&stage_execution_id)
        .await?
        .ok_or_else(|| ApiError::not_found("stage_execution", &stage_execution_id))?;
    Ok(Json(
        json!({"stage_execution": stage_execution_view(&state, &execution).await?}),
    ))
}

pub(super) async fn get_stage_outcome(
    State(state): State<AppState>,
    Path(stage_execution_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let execution = state
        .store
        .get_stage_execution(&stage_execution_id)
        .await?
        .ok_or_else(|| ApiError::not_found("stage_execution", &stage_execution_id))?;
    let outcome = state
        .store
        .get_stage_outcome_for_execution(&execution.id)
        .await?;
    let agent_execution = match execution.run_id.as_ref() {
        Some(run_id) => {
            crate::app::agent_hosts::sanitized_run_agent_execution(&state, run_id).await?
        }
        None => None,
    };
    Ok(Json(json!({
        "stage_execution_id": execution.id,
        "outcome": outcome,
        "agent_execution":agent_execution,
    })))
}

async fn stage_execution_view(
    state: &AppState,
    execution: &pharness_store::StoredStageExecution,
) -> Result<Value, ApiError> {
    let mut value =
        serde_json::to_value(execution).map_err(|error| ApiError::internal(error.to_string()))?;
    if let Some(object) = value.as_object_mut() {
        let provenance = match execution.run_id.as_ref() {
            Some(run_id) => {
                crate::app::agent_hosts::sanitized_run_agent_execution(state, run_id).await?
            }
            None => None,
        };
        object.insert("agent_execution".into(), provenance.unwrap_or(Value::Null));
    }
    Ok(value)
}

pub(super) async fn get_stage_context_pack(
    State(state): State<AppState>,
    Path(stage_execution_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let execution = state
        .store
        .get_stage_execution(&stage_execution_id)
        .await?
        .ok_or_else(|| ApiError::not_found("stage_execution", &stage_execution_id))?;
    let pack = match execution.context_pack_id.as_deref() {
        Some(id) => state.store.get_agent_context_pack(id).await?,
        None => None,
    };
    Ok(Json(json!({
        "stage_execution_id": execution.id,
        "context_pack": pack,
    })))
}

pub(super) async fn list_annotations(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    repo_metadata(&state, &work_item_id).await?;
    let annotations = state.store.list_operator_annotations(&work_item_id).await?;
    let decisions = state
        .store
        .list_operator_annotation_decisions(&work_item_id)
        .await?;
    Ok(Json(json!({
        "annotations": annotations,
        "count": annotations.len(),
        "decisions":decisions,
    })))
}

pub(super) async fn list_work_item_evidence(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    repo_metadata(&state, &work_item_id).await?;
    let validations = state.store.list_evidence_validations(&work_item_id).await?;
    let mut validation_references = Vec::with_capacity(validations.len());
    for validation in &validations {
        validation_references.push(json!({
            "evidence_validation_id": validation.id,
            "typed_references": state
                .store
                .list_evidence_validation_references(&validation.id)
                .await?,
        }));
    }
    let outcomes = state
        .store
        .list_effective_stage_outcomes(&work_item_id)
        .await?;
    Ok(Json(json!({
        "work_item_id": work_item_id,
        "evidence_validations": validations,
        "validation_references": validation_references,
        "effective_stage_outcomes": outcomes,
        "count": validations.len(),
    })))
}

pub(super) async fn get_evidence_validation(
    State(state): State<AppState>,
    Path(evidence_validation_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let validation = state
        .store
        .get_evidence_validation(&evidence_validation_id)
        .await?
        .ok_or_else(|| ApiError::not_found("evidence_validation", &evidence_validation_id))?;
    let typed_references = state
        .store
        .list_evidence_validation_references(&evidence_validation_id)
        .await?;
    Ok(Json(json!({
        "evidence_validation": validation,
        "typed_references": typed_references,
    })))
}

pub(super) async fn create_annotation(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
    Json(request): Json<CreateAnnotationRequest>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let statement = required_text(request.statement, "statement")?;
    let actor = required_text(request.actor, "actor")?;
    let reason = required_text(request.reason, "reason")?;
    if statement.len() > 4_000 || actor.len() > 200 || reason.len() > 1_000 {
        return Err(ApiError::bad_request(
            "annotation statement, actor, or reason exceeds its length limit",
        ));
    }
    if !matches!(
        request.target_kind.as_str(),
        "work_item" | "stage_execution" | "stage_outcome" | "evidence_validation"
    ) {
        return Err(ApiError::bad_request("unsupported annotation target_kind"));
    }
    if !matches!(
        request.requested_effect.as_str(),
        "add_context" | "mark_evidence_stale" | "repeat_stage" | "replan"
    ) {
        return Err(ApiError::bad_request(
            "requested_effect must add context, mark evidence stale, repeat a stage, or request replan",
        ));
    }
    if !request.evidence_refs.is_array() {
        return Err(ApiError::bad_request("evidence_refs must be an array"));
    }
    let metadata = repo_metadata(&state, &work_item_id).await?;
    if metadata.closed_at.is_some()
        && matches!(request.requested_effect.as_str(), "repeat_stage" | "replan")
    {
        return Err(ApiError::conflict(
            "closed Repo Mode WorkItems retain annotations as evidence but cannot repeat or replan",
        ));
    }
    let expected_hash = repo_work_item_state_hash(&metadata)?;
    if request.state_hash != expected_hash {
        return Err(ApiError::conflict(
            "Repo WorkItem changed after annotation preview; refresh and retry",
        ));
    }
    if request.target_kind == "work_item" && request.target_id != work_item_id {
        return Err(ApiError::not_found("work_item", &request.target_id));
    }
    if request.target_kind == "stage_execution" {
        let execution = state
            .store
            .get_stage_execution(&request.target_id)
            .await?
            .ok_or_else(|| ApiError::not_found("stage_execution", &request.target_id))?;
        if execution.work_item_id != work_item_id {
            return Err(ApiError::not_found("stage_execution", &request.target_id));
        }
        if request.requested_effect == "repeat_stage"
            && (!matches!(
                execution.stage_key.as_str(),
                "implement" | "test" | "verify"
            ) || !matches!(
                execution.status.as_str(),
                "succeeded" | "failed" | "blocked" | "cancelled"
            ))
        {
            return Err(ApiError::conflict(
                "repeat_stage requires a terminal Implement, Test, or Verify StageExecution",
            ));
        }
    } else if request.requested_effect == "repeat_stage" {
        return Err(ApiError::bad_request(
            "repeat_stage requires target_kind stage_execution",
        ));
    }
    if request.target_kind == "stage_outcome" {
        let outcome = state
            .store
            .get_stage_outcome(&request.target_id)
            .await?
            .ok_or_else(|| ApiError::not_found("stage_outcome", &request.target_id))?;
        if outcome.work_item_id != work_item_id {
            return Err(ApiError::not_found("stage_outcome", &request.target_id));
        }
    }
    let annotation = state
        .store
        .create_operator_annotation(CreateOperatorAnnotation {
            id: new_prefixed_id("annot"),
            work_item_id: work_item_id.clone(),
            target_kind: request.target_kind,
            target_id: request.target_id,
            statement,
            evidence_refs: request.evidence_refs,
            requested_effect: request.requested_effect,
            actor,
            reason,
            state_hash: expected_hash,
        })
        .await?;
    let updated_metadata = repo_metadata(&state, &work_item_id).await?;
    Ok(Json(json!({
        "annotation": annotation,
        "work_item_state_hash":repo_work_item_state_hash(&updated_metadata)?,
    })))
}
