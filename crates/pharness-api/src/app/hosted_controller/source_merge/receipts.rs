use super::{attempt_id, now, receipt_id, saved, ApiError, AppState};
use axum::extract::{Path, State};
use axum::Json;
use pharness_store::{CreateArtifact, StoredArtifact, StoredChangeSet};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct MergeAttempt {
    pub execution_id: String,
    pub authority_hash: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct MergeOutcome {
    pub execution_id: String,
    pub authority_hash: String,
    pub checked_at_ms: i64,
    pub status: String,
    pub error_code: Option<String>,
    pub merge_http_status: Option<u16>,
    pub merge_commit_sha: Option<String>,
    pub base_commit_sha: Option<String>,
    pub head_commit_sha: Option<String>,
    pub merge_tree_sha: Option<String>,
    pub origin: Option<String>,
    pub required_checks: Option<Value>,
}

pub(in crate::app) async fn internal_source_merge_attempt(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Json(request): Json<MergeAttempt>,
) -> Result<Json<Value>, ApiError> {
    let _boundary = super::super::DISPATCH_BOUNDARY.lock().await;
    let (_, _, authority, change) = saved(&state, &intent_id, &request.execution_id, true).await?;
    if request.authority_hash != authority.material_hash().map_err(ApiError::conflict)? {
        return Err(ApiError::conflict("source merge admission hash changed"));
    }
    let id = attempt_id(&request.execution_id);
    if state.store.get_artifact(&id).await?.is_some()
        || state
            .store
            .get_artifact(&receipt_id(&request.execution_id))
            .await?
            .is_some()
    {
        return Err(ApiError::conflict("source merge was already admitted or has an outcome; observe GitHub without another merge attempt"));
    }
    let artifact = write_artifact(&state, &change, &id, "source_merge_attempt", json!({
        "authority":authority,"authority_hash":request.authority_hash,"admitted_at_ms":now(),
        "meaning":"one GitHub merge attempt admitted; acknowledgement and actual merge remain unproven",
    })).await?;
    Ok(Json(
        json!({"admitted":true,"attempt_id":artifact.id,"authority_hash":request.authority_hash}),
    ))
}

pub(in crate::app) async fn internal_source_merge_outcome(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Json(request): Json<MergeOutcome>,
) -> Result<Json<Value>, ApiError> {
    let _boundary = super::super::DISPATCH_BOUNDARY.lock().await;
    let (_, operation, authority, change) =
        saved(&state, &intent_id, &request.execution_id, false).await?;
    if request.authority_hash != authority.material_hash().map_err(ApiError::conflict)?
        || !matches!(request.status.as_str(), "merged" | "failed" | "unknown")
        || request.checked_at_ms < operation.created_at
        || request.checked_at_ms > now().saturating_add(30_000)
        || request.error_code.as_ref().is_some_and(|code| {
            code.len() > 100 || !code.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
        })
    {
        return Err(ApiError::bad_request(
            "invalid source merge outcome identity or bounded fields",
        ));
    }
    if request.status == "merged"
        && (request.error_code.is_some()
            || request.base_commit_sha.as_deref() != Some(&authority.base_commit_sha)
            || request.head_commit_sha.as_deref() != Some(&authority.head_commit_sha)
            || !request.merge_commit_sha.as_deref().is_some_and(git_sha)
            || !request.merge_tree_sha.as_deref().is_some_and(git_sha)
            || !matches!(
                request.origin.as_deref(),
                Some("api_acknowledged" | "observed_existing_merge")
            ))
    {
        return Err(ApiError::conflict(
            "source merge receipt does not bind the approved source and merge identity",
        ));
    }
    if request.status != "merged"
        && (request.origin.is_some()
            || request.merge_commit_sha.is_some()
            || request.base_commit_sha.is_some()
            || request.head_commit_sha.is_some()
            || request.merge_tree_sha.is_some())
    {
        return Err(ApiError::bad_request(
            "an unconfirmed source merge cannot contain a successful merge receipt",
        ));
    }
    let attempt = state
        .store
        .get_artifact(&attempt_id(&request.execution_id))
        .await?;
    if request.origin.as_deref() == Some("api_acknowledged")
        && (attempt.is_none() || request.merge_http_status != Some(200))
    {
        return Err(ApiError::conflict(
            "acknowledged source merge has no admitted attempt",
        ));
    }
    let material = json!({"authority_hash":request.authority_hash,"outcome":request,
        "evidence_class":"worker_receipt","provider_observation_required":true});
    if serde_json::to_vec(&material)
        .map_err(|_| ApiError::bad_request("invalid merge receipt"))?
        .len()
        > 65_536
    {
        return Err(ApiError::bad_request(
            "source merge receipt exceeds its evidence limit",
        ));
    }
    let id = receipt_id(&request.execution_id);
    let receipt = if let Some(existing) = state.store.get_artifact(&id).await? {
        if existing.kind != "source_merge_receipt"
            || existing.content_json.as_ref() != Some(&material)
        {
            return Err(ApiError::conflict(
                "a different source merge outcome is already recorded",
            ));
        }
        existing
    } else {
        write_artifact(&state, &change, &id, "source_merge_receipt", material).await?
    };
    state
        .store
        .wake_workflow(&authority.work_item_id, now())
        .await?;
    Ok(Json(
        json!({"receipt_id":receipt.id,"receipt_hash":receipt.content_hash,
        "status":"recorded","provider_observation_required":true}),
    ))
}

fn git_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

async fn write_artifact(
    state: &AppState,
    change: &StoredChangeSet,
    id: &str,
    kind: &str,
    material: Value,
) -> Result<StoredArtifact, ApiError> {
    Ok(state
        .store
        .create_artifact(CreateArtifact {
            id: id.into(),
            session_id: change.session_id.clone(),
            run_id: change.run_id.clone(),
            kind: kind.into(),
            label: "Hosted source merge evidence".into(),
            mime_type: Some("application/json".into()),
            path: None,
            content_text: None,
            content_json: Some(material),
        })
        .await?)
}
