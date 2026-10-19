//! Worker callbacks for the production rollback writer. The writer derives
//! the plan from the file at the current head; the API accepts it only if it
//! replaces exactly the released candidate with the recorded previous digest.
use super::super::{artifact_id, evidence, now, ApiError, AppState};
use super::{receipt_identity, saved, validate_current};
use axum::extract::{Path, Query, State};
use axum::Json;
use pharness_core::hosted_sdlc::production::ProductionRollbackPlan;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct RollbackQuery {
    pub execution_id: String,
    #[serde(default)]
    pub observe_only: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct RollbackPlanRequest {
    pub execution_id: String,
    pub authority_hash: String,
    pub plan: ProductionRollbackPlan,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct RollbackAttempt {
    pub execution_id: String,
    pub authority_hash: String,
    pub plan_hash: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct RollbackOutcome {
    pub execution_id: String,
    pub authority_hash: String,
    pub status: String,
    pub observation: Option<Value>,
    pub error_code: Option<String>,
    pub checked_at_ms: i64,
    #[serde(default)]
    pub observe_only: bool,
}

fn bounded_code(code: Option<&str>, message: &str) -> Result<String, ApiError> {
    code.filter(|c| {
        !c.is_empty() && c.len() <= 100 && c.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
    })
    .map(str::to_owned)
    .ok_or_else(|| ApiError::bad_request(message))
}

pub(in crate::app) async fn internal_production_rollback_context(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<RollbackQuery>,
) -> Result<Json<Value>, ApiError> {
    if query.observe_only {
        return Err(ApiError::conflict(
            "the production rollback has no recovery reader",
        ));
    }
    let saved = saved(&state, &id, &query.execution_id).await?;
    let admitted = state
        .store
        .get_artifact(&artifact_id("rollback_attempt", &query.execution_id))
        .await?
        .is_some();
    let may_advance = !admitted && validate_current(&state, &saved, true).await.is_ok();
    Ok(Json(
        json!({"authority":saved.authority,"authority_hash":saved.authority_hash,"plan":saved.plan,"plan_hash":saved.plan_hash,"may_advance":may_advance,"admission_recorded":admitted}),
    ))
}

pub(in crate::app) async fn internal_production_rollback_plan(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<RollbackPlanRequest>,
) -> Result<Json<Value>, ApiError> {
    let _boundary = super::super::super::DISPATCH_BOUNDARY.lock().await;
    let saved = saved(&state, &id, &request.execution_id).await?;
    validate_current(&state, &saved, true).await?;
    if request.authority_hash != saved.authority_hash {
        return Err(ApiError::conflict(
            "production rollback plan has different authority",
        ));
    }
    request
        .plan
        .validate(&saved.authority)
        .map_err(ApiError::conflict)?;
    if saved.plan.as_ref().is_some_and(|p| p != &request.plan) {
        return Err(ApiError::conflict(
            "the original production rollback plan cannot be replaced",
        ));
    }
    if state
        .store
        .get_artifact(&artifact_id("rollback_attempt", &request.execution_id))
        .await?
        .is_some()
    {
        return Err(ApiError::conflict(
            "production rollback was admitted; its plan cannot change",
        ));
    }
    evidence::artifact(
        &state,
        &saved.delivery.pipeline,
        &artifact_id("rollback_plan", &request.execution_id),
        "hosted_production_rollback_plan",
        json!(request.plan),
    )
    .await?;
    Ok(Json(
        json!({"recorded":true,"plan_hash":request.plan.material_hash().map_err(ApiError::conflict)?}),
    ))
}

pub(in crate::app) async fn internal_production_rollback_attempt(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<RollbackAttempt>,
) -> Result<Json<Value>, ApiError> {
    let _boundary = super::super::super::DISPATCH_BOUNDARY.lock().await;
    let saved = saved(&state, &id, &request.execution_id).await?;
    validate_current(&state, &saved, true).await?;
    let plan_hash = saved
        .plan_hash
        .as_deref()
        .ok_or_else(|| ApiError::conflict("production rollback has no recorded plan"))?;
    if request.authority_hash != saved.authority_hash || request.plan_hash != plan_hash {
        return Err(ApiError::conflict(
            "production rollback admission differs from the saved authority or plan",
        ));
    }
    if state
        .store
        .get_artifact(&artifact_id("rollback_attempt", &request.execution_id))
        .await?
        .is_some()
        || state
            .store
            .get_artifact(&artifact_id("rollback_commit", &request.execution_id))
            .await?
            .is_some()
    {
        return Err(ApiError::conflict("the original production rollback was already admitted; observe GitHub without another mutation"));
    }
    let admitted_at = now();
    let expires_at = admitted_at
        .saturating_add(60_000)
        .min(saved.authority.expires_at_ms);
    if expires_at <= admitted_at {
        return Err(ApiError::conflict(
            "production rollback window closed before admission",
        ));
    }
    let a = &saved.authority;
    let body = json!({"execution_id":request.execution_id,"operation_id":saved.operation.id,"authority_hash":request.authority_hash,"plan_hash":request.plan_hash,"failed_result_artifact_id":a.failed_result_artifact_id,"failed_result_sha256":a.failed_result_sha256,"reverses_gitops_commit_sha":a.admitted_gitops_commit_sha,"admitted_at_ms":admitted_at,"admission_expires_at_ms":expires_at,"meaning":"one expected-head GitOps commit admitted to restore the recorded previous production image after a failed window; no deployment success claimed"});
    let record = evidence::artifact(
        &state,
        &saved.delivery.pipeline,
        &artifact_id("rollback_attempt", &request.execution_id),
        "hosted_production_rollback_admission",
        body,
    )
    .await?;
    Ok(Json(
        json!({"admitted":true,"admission_id":record.id,"authority_hash":request.authority_hash,"plan_hash":request.plan_hash,"failed_result_artifact_id":a.failed_result_artifact_id,"failed_result_sha256":a.failed_result_sha256,"reverses_gitops_commit_sha":a.admitted_gitops_commit_sha,"admission_expires_at_ms":expires_at}),
    ))
}

pub(in crate::app) async fn internal_production_rollback_outcome(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<RollbackOutcome>,
) -> Result<Json<Value>, ApiError> {
    let _boundary = super::super::super::DISPATCH_BOUNDARY.lock().await;
    let saved = saved(&state, &id, &request.execution_id).await?;
    if request.authority_hash != saved.authority_hash
        || request.observe_only
        || request.checked_at_ms < saved.authority.created_at_ms
        || request.checked_at_ms > now().saturating_add(30_000)
    {
        return Err(ApiError::conflict(
            "production rollback outcome belongs to different authority or an impossible clock",
        ));
    }
    let work_item = saved.authority.proposal.work_item_id.clone();
    if request.status == "unconfirmed" {
        let code = bounded_code(
            request.error_code.as_deref(),
            "unconfirmed production rollback requires a bounded error code",
        )?;
        if request.observation.is_some() {
            return Err(ApiError::bad_request(
                "unconfirmed production rollback cannot contain a successful observation",
            ));
        }
        let record = evidence::artifact(&state, &saved.delivery.pipeline, &artifact_id("rollback_exception", &request.execution_id), "hosted_production_rollback_exception", json!({"execution_id":request.execution_id,"authority_hash":request.authority_hash,"error_code":code,"meaning":"production rollback GitOps outcome remains unconfirmed"})).await?;
        state.store.wake_workflow(&work_item, now()).await?;
        return Ok(Json(
            json!({"recorded":true,"artifact_id":record.id,"status":"unconfirmed"}),
        ));
    }
    if request.status != "committed" || request.error_code.is_some() {
        return Err(ApiError::bad_request(
            "production rollback outcome must be committed or unconfirmed without contradictory evidence",
        ));
    }
    let attempt = state
        .store
        .get_artifact(&artifact_id("rollback_attempt", &request.execution_id))
        .await?
        .ok_or_else(|| ApiError::conflict("production rollback commit has no admitted write"))?;
    let a = attempt
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("production rollback admission has no content"))?;
    let plan = saved
        .plan
        .as_ref()
        .ok_or_else(|| ApiError::conflict("production rollback has no recorded plan"))?;
    if attempt.kind != "hosted_production_rollback_admission"
        || a["execution_id"] != request.execution_id
        || a["operation_id"] != saved.operation.id
        || a["authority_hash"] != request.authority_hash
        || saved
            .plan_hash
            .as_deref()
            .map_or(true, |h| a["plan_hash"] != h)
        || a["reverses_gitops_commit_sha"] != saved.authority.admitted_gitops_commit_sha
        || a["admitted_at_ms"].as_i64().map_or(true, |t| {
            t < saved.authority.created_at_ms
                || t >= saved.authority.expires_at_ms
                || t > request.checked_at_ms
        })
    {
        return Err(ApiError::conflict(
            "production rollback observation differs from its original admission",
        ));
    }
    let observation = request.observation.as_ref().ok_or_else(|| {
        ApiError::bad_request("committed production rollback requires bounded GitHub observation")
    })?;
    let identity =
        evidence::validate_observation(&saved.authority, plan, observation, request.checked_at_ms)?;
    let receipt_id = artifact_id("rollback_commit", &request.execution_id);
    let body = json!({"execution_id":request.execution_id,"authority_hash":request.authority_hash,"identity":identity,"observation":observation,"checked_at_ms":request.checked_at_ms,"meaning":"the previous production digest was restored in GitOps; Argo deployment of it remains pending"});
    let receipt = if let Some(existing) = state.store.get_artifact(&receipt_id).await? {
        let old = existing.content_json.as_ref().ok_or_else(|| {
            ApiError::conflict("production rollback commit receipt has no content")
        })?;
        if existing.kind != "hosted_production_rollback_commit"
            || old["identity"] != identity
            || old["execution_id"] != request.execution_id
            || old["authority_hash"] != request.authority_hash
        {
            return Err(ApiError::conflict(
                "a different production rollback commit is already recorded",
            ));
        }
        existing
    } else {
        evidence::artifact(
            &state,
            &saved.delivery.pipeline,
            &receipt_id,
            "hosted_production_rollback_commit",
            body,
        )
        .await?
    };
    receipt_identity(&saved, &receipt)?;
    state.store.wake_workflow(&work_item, now()).await?;
    Ok(Json(
        json!({"recorded":true,"artifact_id":receipt.id,"status":"rollback_committed","deployment_verification":"pending"}),
    ))
}
