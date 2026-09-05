use super::{artifact_id, evidence, now, preparation, ApiError, AppState};
use axum::extract::{Path, State};
use axum::Json;
use pharness_store::{StoredArtifact, UpdateDeploymentIntentEvidence};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct StagingOutcome {
    pub execution_id: String,
    pub authority_hash: String,
    pub status: String,
    pub observation: Option<Value>,
    pub error_code: Option<String>,
    pub checked_at_ms: i64,
    #[serde(default)]
    pub observe_only: bool,
}

pub(in crate::app) async fn internal_staging_outcome(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<StagingOutcome>,
) -> Result<Json<Value>, ApiError> {
    let _boundary = super::super::super::DISPATCH_BOUNDARY.lock().await;
    let saved = preparation::saved(&state, &id, &request.execution_id).await?;
    if request.authority_hash
        != saved
            .authority
            .material_hash()
            .map_err(ApiError::conflict)?
        || request.checked_at_ms < saved.authority.created_at_ms
        || request.checked_at_ms > now().saturating_add(30_000)
        || (request.observe_only
            && saved
                .operation
                .resource_refs
                .get("staging_observer_dispatch")
                .is_none())
    {
        return Err(ApiError::conflict(
            "staging outcome belongs to different authority or an undispatched observer",
        ));
    }
    if request.status == "unconfirmed" {
        let code = request
            .error_code
            .as_deref()
            .filter(|c| {
                !c.is_empty()
                    && c.len() <= 100
                    && c.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            })
            .ok_or_else(|| {
                ApiError::bad_request("unconfirmed staging requires a bounded error code")
            })?;
        if request.observation.is_some() {
            return Err(ApiError::bad_request(
                "unconfirmed staging cannot contain a successful observation",
            ));
        }
        let record = evidence::artifact(&state, &saved.pipeline, &artifact_id(if request.observe_only {"observer_exception"} else {"executor_exception"}, &request.execution_id), "hosted_staging_exception", json!({"execution_id":request.execution_id,"authority_hash":request.authority_hash,"error_code":code,"meaning":"staging GitOps outcome remains unconfirmed"})).await?;
        state
            .store
            .wake_workflow(&saved.authority.work_item_id, now())
            .await?;
        return Ok(Json(
            json!({"recorded":true,"artifact_id":record.id,"status":"unconfirmed"}),
        ));
    }
    if request.status != "committed" || request.error_code.is_some() {
        return Err(ApiError::bad_request(
            "staging outcome must be committed or unconfirmed without contradictory evidence",
        ));
    }
    let plan = saved
        .plan
        .as_ref()
        .ok_or_else(|| ApiError::conflict("staging observation has no persisted plan"))?;
    let attempt = state
        .store
        .get_artifact(&artifact_id("attempt", &request.execution_id))
        .await?
        .ok_or_else(|| ApiError::conflict("staging commit has no admitted write"))?;
    let a = attempt
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("staging admission has no content"))?;
    if attempt.kind != "hosted_staging_admission"
        || a["execution_id"] != request.execution_id
        || a["operation_id"] != saved.operation.id
        || a["authority_hash"] != request.authority_hash
        || a["plan_hash"] != plan.material_hash().map_err(ApiError::conflict)?
        || a["admitted_at_ms"].as_i64().map_or(true, |t| {
            t < saved.authority.created_at_ms
                || t >= saved.authority.expires_at_ms
                || t > request.checked_at_ms
        })
    {
        return Err(ApiError::conflict(
            "staging observation differs from its original admission",
        ));
    }
    let observation = request.observation.as_ref().ok_or_else(|| {
        ApiError::bad_request("committed staging requires bounded GitHub observation")
    })?;
    let identity =
        evidence::validate_observation(&saved.authority, plan, observation, request.checked_at_ms)?;
    let receipt_id = artifact_id("commit", &request.execution_id);
    let body = json!({"execution_id":request.execution_id,"authority_hash":request.authority_hash,"identity":identity,"observation":observation,"checked_at_ms":request.checked_at_ms,"meaning":"the admitted GitOps digest change was observed; deployment and runtime verification remain pending"});
    let receipt = if let Some(existing) = state.store.get_artifact(&receipt_id).await? {
        let old = existing
            .content_json
            .as_ref()
            .ok_or_else(|| ApiError::conflict("staging commit receipt has no content"))?;
        if existing.kind != "hosted_staging_commit"
            || old["identity"] != identity
            || old["execution_id"] != request.execution_id
            || old["authority_hash"] != request.authority_hash
        {
            return Err(ApiError::conflict(
                "a different staging commit is already recorded",
            ));
        }
        existing
    } else {
        evidence::artifact(
            &state,
            &saved.pipeline,
            &receipt_id,
            "hosted_staging_commit",
            body,
        )
        .await?
    };
    // Persist the provider result first. Restart can finish the projection even
    // after authority changes, while progression independently rejects it.
    settle(&state, &saved, &receipt).await?;
    state
        .store
        .wake_workflow(&saved.authority.work_item_id, now())
        .await?;
    Ok(Json(
        json!({"recorded":true,"artifact_id":receipt.id,"status":"gitops_committed","deployment_verification":"pending","runtime_verification":"pending"}),
    ))
}

pub(in super::super) async fn settle(
    state: &AppState,
    saved: &preparation::Saved,
    receipt: &StoredArtifact,
) -> Result<(), ApiError> {
    let body = receipt
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("staging commit receipt has no content"))?;
    let plan = saved
        .plan
        .as_ref()
        .ok_or_else(|| ApiError::conflict("staging commit has no saved plan"))?;
    if receipt.kind != "hosted_staging_commit"
        || receipt.session_id != saved.pipeline.session_id
        || receipt.run_id != saved.pipeline.run_id
        || body["authority_hash"]
            != saved
                .authority
                .material_hash()
                .map_err(ApiError::conflict)?
        || body["identity"]
            != evidence::validate_observation(
                &saved.authority,
                plan,
                &body["observation"],
                body["checked_at_ms"].as_i64().unwrap_or_default(),
            )?
    {
        return Err(ApiError::conflict(
            "persisted staging commit receipt is inconsistent",
        ));
    }
    // Do not recreate missing delivery records after an admitted write.
    let change = state
        .store
        .get_gitops_change_set_by_deployment_intent(&saved.deployment.id)
        .await?
        .ok_or_else(|| {
            ApiError::conflict("observed staging change has no saved GitOps ChangeSet")
        })?;
    if change.id != saved.authority.gitops_change_set_id
        || change.gitops_update_plan_artifact_id
            != artifact_id("plan", &saved.authority.execution_id)
    {
        return Err(ApiError::conflict(
            "observed staging ChangeSet identity changed",
        ));
    }
    let mut value = saved.deployment.intent_json.clone();
    value["hosted_staging"]["gitops_observation"] = json!({"artifact_id":receipt.id,"gitops_commit_sha":body["identity"]["gitops_commit_sha"],"image_ref":body["identity"]["image_ref"],"status":"committed","deployment_verification":"pending","runtime_verification":"pending"});
    if value != saved.deployment.intent_json {
        state
            .store
            .update_deployment_intent_evidence(
                &saved.deployment.id,
                UpdateDeploymentIntentEvidence {
                    intent_json: value,
                    actor: Some("observer:hosted-staging".into()),
                    reason: Some(
                        "Record the admitted GitOps commit independently from actual deployment"
                            .into(),
                    ),
                },
            )
            .await?;
    }
    Ok(())
}
