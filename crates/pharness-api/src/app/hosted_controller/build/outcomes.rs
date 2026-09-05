use super::{attempt_id, now, preparation, terminal_id, ApiError, AppState};
use crate::app::hashing::canonical_material_hash as hash;
use crate::app::pipeline::execution;
use axum::extract::{Path, State};
use axum::Json;
use pharness_store::{CreateArtifact, StoredArtifact, StoredPipelineIntent};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct BuildAttempt {
    pub execution_id: String,
    pub manifest_hash: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct BuildOutcome {
    pub execution_id: String,
    pub manifest_hash: String,
    pub pipeline_run: Option<Value>,
    pub analysis: Option<Value>,
    pub error_code: Option<String>,
    #[serde(default)]
    pub observe_only: bool,
}

pub(in crate::app) async fn internal_build_attempt(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<BuildAttempt>,
) -> Result<Json<Value>, ApiError> {
    let _boundary = super::super::DISPATCH_BOUNDARY.lock().await;
    let (intent, operation) = preparation::saved(&state, &id, &request.execution_id, true).await?;
    let d = &operation.resource_refs["build_dispatch"];
    if d["manifest_hash"] != request.manifest_hash {
        return Err(ApiError::conflict(
            "hosted build admission manifest changed",
        ));
    }
    if state
        .store
        .get_artifact(&attempt_id(&request.execution_id))
        .await?
        .is_some()
        || state
            .store
            .get_artifact(&terminal_id(&request.execution_id))
            .await?
            .is_some()
    {
        return Err(ApiError::conflict("the original build was already admitted; observe its PipelineRun without another create attempt"));
    }
    let record=artifact(&state,&intent,&attempt_id(&request.execution_id),"hosted_build_admission",json!({"operation_id":operation.id,"execution_id":request.execution_id,"manifest_hash":request.manifest_hash,"permission_grant_id":d["permission_grant_id"],"admitted_at_ms":now(),"meaning":"one PipelineRun create admitted; actual execution remains unproven"})).await?;
    Ok(Json(
        json!({"admitted":true,"admission_id":record.id,"manifest_hash":request.manifest_hash}),
    ))
}

pub(in crate::app) async fn internal_build_outcome(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<BuildOutcome>,
) -> Result<Json<Value>, ApiError> {
    let _boundary = super::super::DISPATCH_BOUNDARY.lock().await;
    let (intent, operation) = preparation::saved(&state, &id, &request.execution_id, false).await?;
    let d = &operation.resource_refs["build_dispatch"];
    if request.manifest_hash != hash(&d["pipeline_run_manifest"])? {
        return Err(ApiError::conflict("hosted build outcome manifest changed"));
    }
    let Some(run) = request.pipeline_run.as_ref() else {
        let code = request
            .error_code
            .as_deref()
            .filter(|v| {
                !v.is_empty()
                    && v.len() <= 100
                    && v.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            })
            .ok_or_else(|| {
                ApiError::bad_request("unavailable build observation requires a bounded error code")
            })?;
        if request.analysis.is_some() {
            return Err(ApiError::bad_request(
                "unavailable build observation cannot contain successful analysis",
            ));
        }
        let name = format!(
            "build_exception_{}_{}",
            request.execution_id,
            if request.observe_only {
                "observer"
            } else {
                "executor"
            }
        );
        let record=artifact(&state,&intent,&name,"hosted_build_observation_exception",json!({"execution_id":request.execution_id,"manifest_hash":request.manifest_hash,"error_code":code,"meaning":"build outcome remains unconfirmed"})).await?;
        state
            .store
            .wake_workflow(&operation.work_item_id, now())
            .await?;
        return Ok(Json(
            json!({"recorded":true,"artifact_id":record.id,"status":"unconfirmed"}),
        ));
    };
    if request.error_code.is_some()
        || serde_json::to_vec(run)
            .map_err(|_| ApiError::bad_request("invalid build observation"))?
            .len()
            > 512 * 1024
    {
        return Err(ApiError::bad_request(
            "hosted build observation is contradictory or exceeds its evidence limit",
        ));
    }
    let attempt = state
        .store
        .get_artifact(&attempt_id(&request.execution_id))
        .await?
        .ok_or_else(|| {
            ApiError::conflict("observed PipelineRun has no admitted autonomous build")
        })?;
    if attempt.kind != "hosted_build_admission"
        || attempt.content_json.as_ref().map(|v| &v["manifest_hash"])
            != Some(&json!(request.manifest_hash))
    {
        return Err(ApiError::conflict(
            "hosted build admission differs from its original manifest",
        ));
    }
    pharness_core::hosted_sdlc::build::validate_observed_run(
        &d["pipeline_run_manifest"],
        run,
        intent.intent_json["execution_state"]["pipeline_run_uid"].as_str(),
    )
    .map_err(ApiError::conflict)?;
    let succeeded = run["status"]["conditions"]
        .as_array()
        .and_then(|v| v.iter().find(|v| v["type"] == "Succeeded"))
        .and_then(|v| v["status"].as_str());
    let status = match succeeded {
        Some("True") => "completed",
        Some("False") => "failed",
        _ => "submitted",
    };
    let mut material = json!({"execution_id":request.execution_id,"manifest_hash":request.manifest_hash,"status":status,"pipeline_run_uid":run["metadata"]["uid"],"pipeline_run_namespace":run["metadata"]["namespace"],"pipeline_run_name":run["metadata"]["name"]});
    if status != "submitted" {
        let analysis = request.analysis.as_ref().ok_or_else(|| {
            ApiError::conflict("terminal hosted build requires its bounded PipelineRun analysis")
        })?;
        if serde_json::to_vec(analysis)
            .map_err(|_| ApiError::bad_request("invalid build analysis"))?
            .len()
            > 512 * 1024
            || analysis["pipeline_run"]["uid"] != run["metadata"]["uid"]
        {
            return Err(ApiError::conflict("hosted build analysis belongs to a different PipelineRun or exceeds its evidence limit"));
        }
        for name in ["SOURCE_COMMIT", "IMAGE_URL", "IMAGE_DIGEST"] {
            let actual = run["status"]["results"]
                .as_array()
                .and_then(|r| r.iter().find(|r| r["name"] == name))
                .map(|r| &r["value"])
                .unwrap_or(&Value::Null);
            if analysis["outputs"]["declared_results"][name] != *actual {
                return Err(ApiError::conflict(
                    "hosted build analysis differs from the actual declared PipelineRun results",
                ));
            }
        }
        material["analysis"] = analysis.clone();
        execution::validate_terminal_pipeline_run_analysis(&as_request(&material)?, analysis)?;
    } else if request.analysis.is_some() {
        return Err(ApiError::bad_request(
            "a running PipelineRun cannot report terminal analysis",
        ));
    }
    let terminal = terminal_id(&request.execution_id);
    if status == "submitted" && state.store.get_artifact(&terminal).await?.is_some() {
        // A delayed initial callback cannot rewind a terminal execution.
        let current = settle(&state, &intent).await?;
        return Ok(Json(
            json!({"recorded":true,"status":current.intent_json["execution_state"]["state"]}),
        ));
    }
    let receipt_id = if status == "submitted" {
        format!("build_submitted_{}", request.execution_id)
    } else {
        terminal
    };
    artifact(
        &state,
        &intent,
        &receipt_id,
        "hosted_build_observation",
        material,
    )
    .await?;
    let current = settle(&state, &intent).await?;
    state
        .store
        .wake_workflow(&operation.work_item_id, now())
        .await?;
    Ok(Json(
        json!({"recorded":true,"status":current.intent_json["execution_state"]["state"],"artifact_id":receipt_id}),
    ))
}

pub(super) async fn settle(
    state: &AppState,
    intent: &StoredPipelineIntent,
) -> Result<StoredPipelineIntent, ApiError> {
    let execution_id = intent.intent_json["execution_state"]["execution_id"]
        .as_str()
        .ok_or_else(|| ApiError::conflict("build execution is unavailable"))?;
    let terminal = state.store.get_artifact(&terminal_id(execution_id)).await?;
    let receipt = match terminal {
        Some(receipt) => Some(receipt),
        None => {
            state
                .store
                .get_artifact(&format!("build_submitted_{execution_id}"))
                .await?
        }
    };
    let Some(receipt) = receipt else {
        return Ok(intent.clone());
    };
    let body = receipt
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("hosted build observation has no body"))?;
    let request = as_request(body)?;
    let expected = match request.status.as_str() {
        "completed" => "pipeline_run_succeeded",
        "failed" => "pipeline_run_failed",
        _ => "pipeline_run_created",
    };
    if intent.intent_json["execution_state"]["state"] != expected {
        if intent.status != "executing" {
            return Err(ApiError::conflict("recorded hosted build outcome conflicts with the current intent; no execution was replayed"));
        }
        let Json(_) = execution::record_pipeline_execution_outcome(
            State(state.clone()),
            Path(intent.id.clone()),
            Json(request),
        )
        .await?;
    }
    let mut current = state.store.get_pipeline_intent(&intent.id).await?.unwrap();
    if current.intent_json["execution_state"]
        .get("pipeline_run_uid")
        .is_none()
    {
        let mut value = current.intent_json.clone();
        execution::merge_pipeline_execution_state(
            &mut value,
            json!({"pipeline_run_uid":body["pipeline_run_uid"]}),
        );
        current = state
            .store
            .update_pipeline_intent_execution(
                &intent.id,
                pharness_store::UpdatePipelineIntentExecution {
                    status: current.status.clone(),
                    intent_json: value,
                    actor: Some("observer:hosted-build".into()),
                    reason: Some("Record the observed Kubernetes PipelineRun UID".into()),
                },
            )
            .await?;
    } else if current.intent_json["execution_state"]["pipeline_run_uid"] != body["pipeline_run_uid"]
    {
        return Err(ApiError::conflict("hosted PipelineRun UID changed"));
    }
    Ok(current)
}

fn as_request(body: &Value) -> Result<crate::dto::PipelineIntentExecutionOutcomeRequest, ApiError> {
    serde_json::from_value(json!({"execution_id":body["execution_id"],"status":body["status"],"pipeline_run_namespace":body["pipeline_run_namespace"],"pipeline_run_name":body["pipeline_run_name"],"error":if body["status"]=="failed" {json!("PipelineRun completed unsuccessfully")}else{Value::Null},"pipeline_run_analysis":body.get("analysis"),"analysis_error":null})).map_err(|_|ApiError::conflict("invalid persisted build outcome"))
}
async fn artifact(
    state: &AppState,
    intent: &StoredPipelineIntent,
    id: &str,
    kind: &str,
    body: Value,
) -> Result<StoredArtifact, ApiError> {
    if let Some(existing) = state.store.get_artifact(id).await? {
        if existing.kind != kind || existing.content_json.as_ref() != Some(&body) {
            return Err(ApiError::conflict(
                "a different hosted build outcome is already recorded",
            ));
        }
        return Ok(existing);
    }
    state
        .store
        .create_artifact(CreateArtifact {
            id: id.into(),
            session_id: intent.session_id.clone(),
            run_id: intent.run_id.clone(),
            kind: kind.into(),
            label: "Hosted build evidence".into(),
            mime_type: Some("application/json".into()),
            path: None,
            content_text: None,
            content_json: Some(body),
        })
        .await
        .map_err(Into::into)
}
