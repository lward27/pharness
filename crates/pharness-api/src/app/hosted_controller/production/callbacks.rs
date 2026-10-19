//! Worker callbacks for the production preview reader, writer and recovery
//! reader. Every handler re-derives the original authority from durable
//! records; nothing in a request body can widen what was dispatched.
use super::{artifact_id, delivery, evidence, now, proposal, ApiError, AppState};
use axum::extract::{Path, Query, State};
use axum::Json;
use pharness_core::hosted_sdlc::production::ProductionPreview;
use pharness_store::{StoredArtifact, UpdateDeploymentIntentEvidence};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct PreviewQuery {
    pub execution_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct PreviewReport {
    pub status: String,
    pub execution_id: String,
    pub authority_hash: String,
    pub preview: Option<ProductionPreview>,
    pub error_code: Option<String>,
    pub checked_at_ms: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct ProductionQuery {
    pub execution_id: String,
    #[serde(default)]
    pub observe_only: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct ProductionAttempt {
    pub execution_id: String,
    pub authority_hash: String,
    pub plan_hash: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct ProductionOutcome {
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

pub(in crate::app) async fn internal_production_preview_context(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<PreviewQuery>,
) -> Result<Json<Value>, ApiError> {
    let dispatched = proposal::dispatched(&state, &id, &query.execution_id).await?;
    let recorded = state
        .store
        .get_artifact(&artifact_id("preview", &query.execution_id))
        .await?
        .is_some();
    Ok(Json(
        json!({"authority":dispatched.authority,"authority_hash":dispatched.authority_hash,"preview_recorded":recorded}),
    ))
}

pub(in crate::app) async fn internal_production_preview(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<PreviewReport>,
) -> Result<Json<Value>, ApiError> {
    let _boundary = super::super::DISPATCH_BOUNDARY.lock().await;
    let dispatched = proposal::dispatched(&state, &id, &request.execution_id).await?;
    if request.authority_hash != dispatched.authority_hash
        || request.checked_at_ms < dispatched.authority.created_at_ms
        || request.checked_at_ms > now().saturating_add(30_000)
    {
        return Err(ApiError::conflict(
            "production preview belongs to different authority or an impossible clock",
        ));
    }
    let body = match request.status.as_str() {
        "unavailable" => {
            let code = bounded_code(
                request.error_code.as_deref(),
                "unavailable production preview requires a bounded error code",
            )?;
            if request.preview.is_some() {
                return Err(ApiError::bad_request(
                    "unavailable production preview cannot contain a file",
                ));
            }
            json!({"status":"unavailable","execution_id":request.execution_id,"authority_hash":request.authority_hash,"error_code":code,"checked_at_ms":request.checked_at_ms})
        }
        "observed" => {
            if request.error_code.is_some() {
                return Err(ApiError::bad_request(
                    "observed production preview cannot carry an error code",
                ));
            }
            let preview = request.preview.as_ref().ok_or_else(|| {
                ApiError::bad_request("observed production preview requires the file")
            })?;
            preview
                .validate(&dispatched.authority)
                .map_err(ApiError::conflict)?;
            json!({"status":"observed","execution_id":request.execution_id,"authority_hash":request.authority_hash,"preview":preview,"checked_at_ms":request.checked_at_ms})
        }
        _ => {
            return Err(ApiError::bad_request(
                "production preview status must be observed or unavailable",
            ))
        }
    };
    let preview_id = artifact_id("preview", &request.execution_id);
    let record = if let Some(existing) = state.store.get_artifact(&preview_id).await? {
        let old = existing
            .content_json
            .as_ref()
            .ok_or_else(|| ApiError::conflict("production preview has no content"))?;
        // A repeated acknowledgement differs only in its clock; the file and
        // status it reports can never change once recorded.
        if existing.kind != "hosted_production_preview"
            || old["status"] != body["status"]
            || old["preview"] != body["preview"]
            || old["error_code"] != body["error_code"]
        {
            return Err(ApiError::conflict(
                "a different production preview is already recorded",
            ));
        }
        existing
    } else {
        evidence::artifact(
            &state,
            &dispatched.pipeline,
            &preview_id,
            "hosted_production_preview",
            body,
        )
        .await?
    };
    state
        .store
        .wake_workflow(&dispatched.authority.work_item_id, now())
        .await?;
    Ok(Json(
        json!({"recorded":true,"artifact_id":record.id,"status":request.status}),
    ))
}

pub(in crate::app) async fn internal_production_context(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ProductionQuery>,
) -> Result<Json<Value>, ApiError> {
    let saved = delivery::saved(&state, &id, &query.execution_id).await?;
    let admitted = state
        .store
        .get_artifact(&artifact_id("attempt", &query.execution_id))
        .await?
        .is_some();
    if query.observe_only
        && (!admitted
            || saved
                .operation
                .resource_refs
                .get("production_observer_dispatch")
                .is_none())
    {
        return Err(ApiError::conflict(
            "the bounded production reader has not been dispatched",
        ));
    }
    let may_advance = !query.observe_only
        && !admitted
        && delivery::validate_current(&state, &saved, true)
            .await
            .is_ok();
    Ok(Json(
        json!({"authority":saved.authority,"authority_hash":saved.authority_hash,"plan":saved.plan,"plan_hash":saved.plan_hash,"may_advance":may_advance,"admission_recorded":admitted}),
    ))
}

pub(in crate::app) async fn internal_production_attempt(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<ProductionAttempt>,
) -> Result<Json<Value>, ApiError> {
    let _boundary = super::super::DISPATCH_BOUNDARY.lock().await;
    let saved = delivery::saved(&state, &id, &request.execution_id).await?;
    delivery::validate_current(&state, &saved, true).await?;
    if request.authority_hash != saved.authority_hash || request.plan_hash != saved.plan_hash {
        return Err(ApiError::conflict(
            "production admission differs from the saved authority or approved plan",
        ));
    }
    if state
        .store
        .get_artifact(&artifact_id("attempt", &request.execution_id))
        .await?
        .is_some()
        || state
            .store
            .get_artifact(&artifact_id("commit", &request.execution_id))
            .await?
            .is_some()
    {
        return Err(ApiError::conflict("the original production write was already admitted; observe GitHub without another mutation"));
    }
    let admitted_at = now();
    // The grant the writer sees is short and never outlives the authority.
    let expires_at = admitted_at
        .saturating_add(60_000)
        .min(saved.authority.expires_at_ms);
    if expires_at <= admitted_at {
        return Err(ApiError::conflict(
            "production approval window closed before admission",
        ));
    }
    let body = json!({"execution_id":request.execution_id,"operation_id":saved.operation.id,"authority_hash":request.authority_hash,"plan_hash":request.plan_hash,"approval_artifact_id":saved.authority.approval_artifact_id,"approval_hash":saved.approval_hash,"admitted_at_ms":admitted_at,"admission_expires_at_ms":expires_at,"meaning":"one expected-head GitOps commit admitted inside the open human approval; no deployment or runtime success claimed"});
    let record = evidence::artifact(
        &state,
        &saved.pipeline,
        &artifact_id("attempt", &request.execution_id),
        "hosted_production_admission",
        body,
    )
    .await?;
    Ok(Json(
        json!({"admitted":true,"admission_id":record.id,"authority_hash":request.authority_hash,"plan_hash":request.plan_hash,"approval_artifact_id":saved.authority.approval_artifact_id,"approval_hash":saved.approval_hash,"admission_expires_at_ms":expires_at}),
    ))
}

pub(in crate::app) async fn internal_production_outcome(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<ProductionOutcome>,
) -> Result<Json<Value>, ApiError> {
    let _boundary = super::super::DISPATCH_BOUNDARY.lock().await;
    let saved = delivery::saved(&state, &id, &request.execution_id).await?;
    if request.authority_hash != saved.authority_hash
        || request.checked_at_ms < saved.authority.created_at_ms
        || request.checked_at_ms > now().saturating_add(30_000)
        || (request.observe_only
            && saved
                .operation
                .resource_refs
                .get("production_observer_dispatch")
                .is_none())
    {
        return Err(ApiError::conflict(
            "production outcome belongs to different authority or an undispatched observer",
        ));
    }
    if request.status == "unconfirmed" {
        let code = bounded_code(
            request.error_code.as_deref(),
            "unconfirmed production delivery requires a bounded error code",
        )?;
        if request.observation.is_some() {
            return Err(ApiError::bad_request(
                "unconfirmed production delivery cannot contain a successful observation",
            ));
        }
        let record = evidence::artifact(&state, &saved.pipeline, &artifact_id(if request.observe_only {"observer_exception"} else {"executor_exception"}, &request.execution_id), "hosted_production_exception", json!({"execution_id":request.execution_id,"authority_hash":request.authority_hash,"error_code":code,"meaning":"production GitOps outcome remains unconfirmed"})).await?;
        state
            .store
            .wake_workflow(&saved.authority.proposal.work_item_id, now())
            .await?;
        return Ok(Json(
            json!({"recorded":true,"artifact_id":record.id,"status":"unconfirmed"}),
        ));
    }
    if request.status != "committed" || request.error_code.is_some() {
        return Err(ApiError::bad_request(
            "production outcome must be committed or unconfirmed without contradictory evidence",
        ));
    }
    let attempt = state
        .store
        .get_artifact(&artifact_id("attempt", &request.execution_id))
        .await?
        .ok_or_else(|| ApiError::conflict("production commit has no admitted write"))?;
    let a = attempt
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("production admission has no content"))?;
    if attempt.kind != "hosted_production_admission"
        || a["execution_id"] != request.execution_id
        || a["operation_id"] != saved.operation.id
        || a["authority_hash"] != request.authority_hash
        || a["plan_hash"] != saved.plan_hash
        || a["approval_artifact_id"] != saved.authority.approval_artifact_id
        || a["approval_hash"] != saved.approval_hash
        || a["admitted_at_ms"].as_i64().map_or(true, |t| {
            t < saved.authority.created_at_ms
                || t >= saved.authority.expires_at_ms
                || t > request.checked_at_ms
        })
    {
        return Err(ApiError::conflict(
            "production observation differs from its original admission",
        ));
    }
    let observation = request.observation.as_ref().ok_or_else(|| {
        ApiError::bad_request("committed production delivery requires bounded GitHub observation")
    })?;
    let identity = evidence::validate_observation(
        &saved.authority,
        &saved.plan,
        observation,
        request.checked_at_ms,
    )?;
    let receipt_id = artifact_id("commit", &request.execution_id);
    let body = json!({"execution_id":request.execution_id,"authority_hash":request.authority_hash,"identity":identity,"observation":observation,"checked_at_ms":request.checked_at_ms,"meaning":"the approved production digest change was observed; deployment and runtime verification remain pending"});
    let receipt = if let Some(existing) = state.store.get_artifact(&receipt_id).await? {
        let old = existing
            .content_json
            .as_ref()
            .ok_or_else(|| ApiError::conflict("production commit receipt has no content"))?;
        if existing.kind != "hosted_production_commit"
            || old["identity"] != identity
            || old["execution_id"] != request.execution_id
            || old["authority_hash"] != request.authority_hash
        {
            return Err(ApiError::conflict(
                "a different production commit is already recorded",
            ));
        }
        existing
    } else {
        evidence::artifact(
            &state,
            &saved.pipeline,
            &receipt_id,
            "hosted_production_commit",
            body,
        )
        .await?
    };
    settle(&state, &saved, &receipt).await?;
    state
        .store
        .wake_workflow(&saved.authority.proposal.work_item_id, now())
        .await?;
    Ok(Json(
        json!({"recorded":true,"artifact_id":receipt.id,"status":"gitops_committed","deployment_verification":"pending","runtime_verification":"pending"}),
    ))
}

pub(super) async fn settle(
    state: &AppState,
    saved: &delivery::Saved,
    receipt: &StoredArtifact,
) -> Result<(), ApiError> {
    let body = receipt
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("production commit receipt has no content"))?;
    if receipt.kind != "hosted_production_commit"
        || receipt.session_id != saved.pipeline.session_id
        || receipt.run_id != saved.pipeline.run_id
        || body["authority_hash"] != saved.authority_hash
        || body["identity"]
            != evidence::validate_observation(
                &saved.authority,
                &saved.plan,
                &body["observation"],
                body["checked_at_ms"].as_i64().unwrap_or_default(),
            )?
    {
        return Err(ApiError::conflict(
            "persisted production commit receipt is inconsistent",
        ));
    }
    // Do not recreate missing delivery records after an admitted write.
    let change = state
        .store
        .get_gitops_change_set_by_deployment_intent(&saved.deployment.id)
        .await?
        .ok_or_else(|| {
            ApiError::conflict("observed production change has no saved GitOps ChangeSet")
        })?;
    if change.id != saved.authority.gitops_change_set_id
        || change.gitops_update_plan_artifact_id
            != artifact_id("plan", &saved.authority.execution_id)
    {
        return Err(ApiError::conflict(
            "observed production ChangeSet identity changed",
        ));
    }
    let mut value = saved.deployment.intent_json.clone();
    value["hosted_production"]["gitops_observation"] = json!({"artifact_id":receipt.id,"gitops_commit_sha":body["identity"]["gitops_commit_sha"],"image_ref":body["identity"]["image_ref"],"status":"committed","deployment_verification":"pending","runtime_verification":"pending"});
    if value != saved.deployment.intent_json {
        state
            .store
            .update_deployment_intent_evidence(
                &saved.deployment.id,
                UpdateDeploymentIntentEvidence {
                    intent_json: value,
                    actor: Some("observer:hosted-production".into()),
                    reason: Some(
                        "Record the approved GitOps commit independently from actual deployment"
                            .into(),
                    ),
                },
            )
            .await?;
    }
    Ok(())
}
