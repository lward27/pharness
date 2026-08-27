use super::clock::{current_millis, unique_suffix};
use super::hashing::canonical_material_hash;
use super::repo_mode::repo_work_item_state_hash;
use super::{ApiError, AppState};
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pharness_store::{
    CreateArchiveRecord, CreateRetentionHold, CreateRetentionPreview, DeleteArchiveRecord,
    RETENTION_POLICY_VERSION,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

const PREVIEW_TTL_MILLIS: u128 = 15 * 60 * 1_000;
const MAX_HOLD_MILLIS: u128 = 365 * 24 * 60 * 60 * 1_000;

pub(super) fn spawn_retention_scheduler(state: AppState) {
    if super::OperationalMode::from_env() != super::OperationalMode::Normal
        || !env_bool("PHARNESS_RETENTION_SCHEDULED_ENABLED", false)
    {
        return;
    }
    let interval_seconds = std::env::var("PHARNESS_RETENTION_INTERVAL_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(86_400)
        .clamp(3_600, 604_800);
    tokio::spawn(async move {
        // Do not let API startup unexpectedly mutate lifecycle state. The first
        // scheduled preview occurs after one full configured interval.
        loop {
            tokio::time::sleep(Duration::from_secs(interval_seconds)).await;
            if let Err(error) = scheduled_retention_pass(&state).await {
                tracing::warn!(?error, "scheduled retention pass failed");
            }
        }
    });
}

async fn scheduled_retention_pass(state: &AppState) -> Result<(), ApiError> {
    let preview = create_retention_preview_record(
        state,
        "system:retention-scheduler",
        "Scheduled retention policy review",
    )
    .await?;
    if env_bool("PHARNESS_RETENTION_PREVIEW_ONLY", true)
        || !env_bool("PHARNESS_RETENTION_AUTOMATIC_EXECUTION_ENABLED", false)
    {
        return Ok(());
    }
    execute_preview_record(
        state,
        &preview,
        "system:retention-scheduler",
        "Automatic execution of exact scheduled retention preview",
    )
    .await?;
    Ok(())
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/system/data-inventory", get(data_inventory))
        .route(
            "/api/system/archives",
            get(list_archives).post(create_archive_record),
        )
        .route(
            "/api/system/archives/:archive_id/delete",
            post(delete_archive),
        )
        .route(
            "/api/system/retention/previews",
            get(list_retention_previews).post(create_retention_preview),
        )
        .route(
            "/api/system/retention/previews/:preview_id/execute",
            post(execute_retention_preview),
        )
        .route(
            "/api/system/retention/receipts",
            get(list_retention_receipts),
        )
        .route(
            "/api/work-items/:work_item_id/retention-holds",
            post(create_work_item_retention_hold),
        )
        .route(
            "/api/retention-holds/:hold_id/release",
            post(release_retention_hold),
        )
}

async fn data_inventory(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let inventory = state.store.data_inventory().await?;
    let holds = state.store.list_retention_holds().await?;
    Ok(Json(json!({
        "inventory":inventory,
        "policy":{
            "schema_version":RETENTION_POLICY_VERSION,
            "workspace_days":7,
            "raw_run_payload_days":30,
            "capability_verification_days":7,
            "evidence_retention":"indefinite",
            "automatic_execution":std::env::var("PHARNESS_RETENTION_AUTOMATIC_EXECUTION_ENABLED").ok().is_some_and(|value| value == "true"),
            "preview_only":std::env::var("PHARNESS_RETENTION_PREVIEW_ONLY").ok().map_or(true,|value| value != "false"),
        },
        "holds":holds,
        "as_of":current_millis().to_string(),
    })))
}

async fn list_archives(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let archives = state.store.list_archive_records().await?;
    let holds = state.store.list_retention_holds().await?;
    let generation = state.store.get_database_generation().await?;
    let values = archives
        .iter()
        .map(|archive| archive_with_deletion_action(archive, generation.as_ref(), &holds))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({"archives":values,"count":archives.len()})))
}

fn archive_with_deletion_action(
    archive: &pharness_store::StoredArchiveRecord,
    generation: Option<&pharness_store::DatabaseGeneration>,
    holds: &[pharness_store::StoredRetentionHold],
) -> Result<Value, ApiError> {
    let now = current_millis();
    let mut blockers = Vec::new();
    if archive.status != "retained" || archive.deleted_at.is_some() {
        blockers.push(
            json!({"code":"archive_not_retained","summary":"The archive is no longer retained."}),
        );
    }
    if archive
        .deletion_eligible_at
        .parse::<u128>()
        .unwrap_or(u128::MAX)
        > now
    {
        blockers.push(json!({"code":"retention_window_active","summary":format!("Archive deletion is unavailable until {}",archive.deletion_eligible_at)}));
    }
    if generation.map(|value| value.id.as_str()) != Some(archive.database_generation_id.as_str()) {
        blockers.push(json!({"code":"database_generation_mismatch","summary":"The ArchiveRecord does not belong to the active database generation."}));
    }
    if !env_bool("PHARNESS_DATABASE_GENERATION_ACCEPTED", false) {
        blockers.push(json!({"code":"database_generation_not_accepted","summary":"The clean database generation has not been explicitly accepted as healthy."}));
    }
    if holds.iter().any(|hold| {
        hold.subject_kind == "archive"
            && hold.subject_id == archive.id
            && hold.released_at.is_none()
            && hold
                .expires_at
                .as_deref()
                .and_then(|value| value.parse::<u128>().ok())
                .map_or(true, |expires| expires > now)
    }) {
        blockers.push(json!({"code":"retention_hold_active","summary":"The ArchiveRecord has an active retention hold."}));
    }
    let state_hash = canonical_material_hash(&json!({
        "archive_id":archive.id,
        "status":archive.status,
        "database_generation_id":archive.database_generation_id,
        "archived_generation_id":archive.archived_generation_id,
        "database_claim":archive.database_claim,
        "archive_claim":archive.archive_claim,
        "database_sha256":archive.database_sha256,
        "manifest_sha256":archive.manifest_sha256,
        "deletion_eligible_at":archive.deletion_eligible_at,
        "generation_accepted":env_bool("PHARNESS_DATABASE_GENERATION_ACCEPTED", false),
        "blockers":blockers,
    }))?;
    let mut value =
        serde_json::to_value(archive).map_err(|error| ApiError::internal(error.to_string()))?;
    value["deletion_action"] = json!({
        "id":"delete_archive",
        "status":if blockers.is_empty() { "ready" } else { "blocked" },
        "effect_class":"destructive_external",
        "blockers":blockers,
        "state_hash":state_hash,
        "confirmation":format!("DELETE ARCHIVE {}",archive.id),
        "external_effect_summary":format!("Delete retained PVCs {} and {} in the configured PHarness namespace",archive.database_claim,archive.archive_claim),
    });
    Ok(value)
}

#[derive(Debug, Deserialize)]
struct DeleteArchiveRequest {
    actor: String,
    reason: String,
    state_hash: String,
    confirmation: String,
}

async fn delete_archive(
    State(state): State<AppState>,
    Path(archive_id): Path<String>,
    Json(request): Json<DeleteArchiveRequest>,
) -> Result<Json<Value>, ApiError> {
    validate_actor_reason(&request.actor, &request.reason)?;
    let archive = state
        .store
        .get_archive_record(&archive_id)
        .await?
        .ok_or_else(|| ApiError::not_found("archive_record", &archive_id))?;
    let holds = state.store.list_retention_holds().await?;
    let generation = state.store.get_database_generation().await?;
    let preview = archive_with_deletion_action(&archive, generation.as_ref(), &holds)?;
    let action = &preview["deletion_action"];
    if action["state_hash"].as_str() != Some(request.state_hash.as_str()) {
        return Err(ApiError::conflict(
            "ArchiveRecord changed after deletion review; refresh before retrying",
        ));
    }
    if action["status"] != "ready" {
        return Err(ApiError::conflict(
            "ArchiveRecord is not eligible for deletion",
        ));
    }
    let expected_confirmation = format!("DELETE ARCHIVE {archive_id}");
    if request.confirmation != expected_confirmation {
        return Err(ApiError::bad_request(format!(
            "confirmation must exactly match {expected_confirmation:?}"
        )));
    }
    let resources = state
        .worker
        .delete_archive_claims(
            &archive.id,
            &archive.archived_generation_id,
            &archive.database_claim,
            &archive.archive_claim,
        )
        .await
        .map_err(|error| ApiError::conflict(format!("archive deletion blocked: {error}")))?;
    let deleted = state
        .store
        .mark_archive_deleted(DeleteArchiveRecord {
            archive_id: archive.id.clone(),
            preview_id: format!("retpreview_{}", unique_suffix()),
            receipt_id: format!("retreceipt_{}", unique_suffix()),
            state_hash: request.state_hash,
            actor: request.actor,
            reason: request.reason,
            deleted_at: current_millis().to_string(),
        })
        .await?;
    Ok(Json(json!({"archive":deleted,"resources":resources})))
}

#[derive(Debug, Deserialize)]
struct CreateArchiveRequest {
    archived_generation_id: String,
    database_claim: String,
    archive_claim: String,
    database_sha256: String,
    manifest_sha256: String,
    archive: Value,
    deletion_eligible_at: String,
    actor: String,
    reason: String,
}

async fn create_archive_record(
    State(state): State<AppState>,
    Json(request): Json<CreateArchiveRequest>,
) -> Result<Json<Value>, ApiError> {
    validate_actor_reason(&request.actor, &request.reason)?;
    validate_kubernetes_resource_name(&request.database_claim)?;
    validate_kubernetes_resource_name(&request.archive_claim)?;
    validate_sha256(&request.database_sha256)?;
    validate_sha256(&request.manifest_sha256)?;
    let generation = state
        .store
        .get_database_generation()
        .await?
        .ok_or_else(|| ApiError::conflict("database generation is not initialized"))?;
    let eligible = request
        .deletion_eligible_at
        .parse::<u128>()
        .map_err(|_| ApiError::bad_request("deletion_eligible_at must be unix milliseconds"))?;
    if eligible < current_millis() + 14 * 24 * 60 * 60 * 1_000 {
        return Err(ApiError::bad_request(
            "archive deletion eligibility must be at least 14 days in the future",
        ));
    }
    let archive = state
        .store
        .create_archive_record(CreateArchiveRecord {
            id: format!("archive_{}", unique_suffix()),
            database_generation_id: generation.id,
            archived_generation_id: request.archived_generation_id,
            database_claim: request.database_claim,
            archive_claim: request.archive_claim,
            database_sha256: request.database_sha256,
            manifest_sha256: request.manifest_sha256,
            archive: json!({
                "manifest":request.archive,
                "recorded_by":request.actor,
                "record_reason":request.reason,
            }),
            deletion_eligible_at: request.deletion_eligible_at,
        })
        .await?;
    Ok(Json(json!({"archive":archive})))
}

#[derive(Debug, Deserialize)]
struct ActorReasonRequest {
    actor: String,
    reason: String,
}

async fn create_retention_preview(
    State(state): State<AppState>,
    Json(request): Json<ActorReasonRequest>,
) -> Result<Json<Value>, ApiError> {
    validate_actor_reason(&request.actor, &request.reason)?;
    let preview = create_retention_preview_record(&state, &request.actor, &request.reason).await?;
    Ok(Json(json!({"preview":preview})))
}

async fn create_retention_preview_record(
    state: &AppState,
    actor: &str,
    reason: &str,
) -> Result<pharness_store::StoredRetentionPreview, ApiError> {
    let generation = state
        .store
        .get_database_generation()
        .await?
        .ok_or_else(|| ApiError::conflict("database generation is not initialized"))?;
    let now = current_millis();
    let candidates = state.store.retention_candidates(now).await?;
    let material = json!({
        "database_generation_id":generation.id,
        "policy_version":RETENTION_POLICY_VERSION,
        "candidates":candidates,
    });
    let content_hash = canonical_material_hash(&material)?;
    let state_hash = canonical_material_hash(&json!({
        "content_hash":content_hash,
        "created_at_boundary":now,
    }))?;
    let preview = state
        .store
        .create_retention_preview(CreateRetentionPreview {
            id: format!("retpreview_{}", unique_suffix()),
            database_generation_id: generation.id,
            preview: material["candidates"].clone(),
            content_hash,
            state_hash,
            actor: actor.into(),
            reason: reason.into(),
            expires_at: (now + PREVIEW_TTL_MILLIS).to_string(),
        })
        .await?;
    Ok(preview)
}

async fn list_retention_previews(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let previews = state.store.list_retention_previews().await?;
    Ok(Json(json!({"previews":previews,"count":previews.len()})))
}

#[derive(Debug, Deserialize)]
struct ExecuteRetentionRequest {
    actor: String,
    reason: String,
    state_hash: String,
    confirmation: String,
}

async fn execute_retention_preview(
    State(state): State<AppState>,
    Path(preview_id): Path<String>,
    Json(request): Json<ExecuteRetentionRequest>,
) -> Result<Json<Value>, ApiError> {
    validate_actor_reason(&request.actor, &request.reason)?;
    let preview = state
        .store
        .get_retention_preview(&preview_id)
        .await?
        .ok_or_else(|| ApiError::not_found("retention preview", &preview_id))?;
    if preview.state_hash != request.state_hash {
        return Err(ApiError::conflict(
            "retention preview changed after review; create a new preview",
        ));
    }
    let expected_confirmation = format!("EXECUTE RETENTION {preview_id}");
    if request.confirmation != expected_confirmation {
        return Err(ApiError::bad_request(format!(
            "confirmation must exactly match {expected_confirmation:?}"
        )));
    }
    let generation = state
        .store
        .get_database_generation()
        .await?
        .ok_or_else(|| ApiError::conflict("database generation is not initialized"))?;
    if generation.id != preview.database_generation_id {
        return Err(ApiError::conflict(
            "retention preview belongs to a different database generation",
        ));
    }
    let receipt = execute_preview_record(&state, &preview, &request.actor, &request.reason).await?;
    Ok(Json(json!({"receipt":receipt})))
}

async fn execute_preview_record(
    state: &AppState,
    preview: &pharness_store::StoredRetentionPreview,
    actor: &str,
    reason: &str,
) -> Result<pharness_store::StoredRetentionReceipt, ApiError> {
    let generation = state
        .store
        .get_database_generation()
        .await?
        .ok_or_else(|| ApiError::conflict("database generation is not initialized"))?;
    if generation.id != preview.database_generation_id {
        return Err(ApiError::conflict(
            "retention preview belongs to a different database generation",
        ));
    }
    let current_candidates = state.store.retention_candidates(current_millis()).await?;
    let current_hash = canonical_material_hash(&json!({
        "database_generation_id":generation.id,
        "policy_version":RETENTION_POLICY_VERSION,
        "candidates":current_candidates,
    }))?;
    if current_hash != preview.content_hash {
        return Err(ApiError::conflict(
            "retention eligibility changed after preview; create a new preview",
        ));
    }
    let workspace_resources = preview
        .preview
        .get("workspaces")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    state
        .worker
        .cleanup_retention_resources(&workspace_resources)
        .await
        .map_err(|error| {
            ApiError::conflict(format!(
                "retention Kubernetes resource preconditions failed: {error}"
            ))
        })?;
    state
        .store
        .execute_retention_preview(
            preview,
            &format!("retreceipt_{}", unique_suffix()),
            actor,
            reason,
            current_millis(),
        )
        .await
        .map_err(Into::into)
}

async fn list_retention_receipts(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let receipts = state.store.list_retention_receipts().await?;
    Ok(Json(json!({"receipts":receipts,"count":receipts.len()})))
}

#[derive(Debug, Deserialize)]
struct CreateHoldRequest {
    actor: String,
    reason: String,
    state_hash: String,
    expires_at: Option<String>,
}

async fn create_work_item_retention_hold(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
    Json(request): Json<CreateHoldRequest>,
) -> Result<Json<Value>, ApiError> {
    validate_actor_reason(&request.actor, &request.reason)?;
    let metadata = state
        .store
        .get_repo_work_item_metadata(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Repo WorkItem", &work_item_id))?;
    let expected = repo_work_item_state_hash(&metadata)?;
    if request.state_hash != expected {
        return Err(ApiError::conflict(
            "WorkItem changed after retention review; refresh before creating the hold",
        ));
    }
    if let Some(expires_at) = &request.expires_at {
        let expires = expires_at
            .parse::<u128>()
            .map_err(|_| ApiError::bad_request("expires_at must be unix milliseconds"))?;
        let now = current_millis();
        if expires <= now || expires > now + MAX_HOLD_MILLIS {
            return Err(ApiError::bad_request(
                "retention hold must expire within the next 365 days",
            ));
        }
    }
    let id = format!("rethold_{}", unique_suffix());
    let hold_state_hash = canonical_material_hash(&json!({
        "id":id,
        "subject_kind":"work_item",
        "subject_id":work_item_id,
        "reason":request.reason,
        "expires_at":request.expires_at,
    }))?;
    let hold = state
        .store
        .create_retention_hold(CreateRetentionHold {
            id,
            subject_kind: "work_item".into(),
            subject_id: work_item_id,
            reason: request.reason,
            actor: request.actor,
            expires_at: request.expires_at,
            state_hash: hold_state_hash,
        })
        .await?;
    Ok(Json(json!({"hold":hold})))
}

#[derive(Debug, Deserialize)]
struct ReleaseHoldRequest {
    actor: String,
    reason: String,
    state_hash: String,
}

async fn release_retention_hold(
    State(state): State<AppState>,
    Path(hold_id): Path<String>,
    Json(request): Json<ReleaseHoldRequest>,
) -> Result<Json<Value>, ApiError> {
    validate_actor_reason(&request.actor, &request.reason)?;
    let hold = state
        .store
        .release_retention_hold(
            &hold_id,
            &request.actor,
            &request.reason,
            &request.state_hash,
        )
        .await?;
    Ok(Json(json!({"hold":hold})))
}

fn validate_actor_reason(actor: &str, reason: &str) -> Result<(), ApiError> {
    if actor.trim().is_empty() || actor.len() > 200 {
        return Err(ApiError::bad_request(
            "actor is required and must be at most 200 characters",
        ));
    }
    if reason.trim().is_empty() || reason.len() > 1_000 {
        return Err(ApiError::bad_request(
            "reason is required and must be at most 1,000 characters",
        ));
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), ApiError> {
    if !value.strip_prefix("sha256:").is_some_and(|hash| {
        hash.len() == 64
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) {
        return Err(ApiError::bad_request(
            "archive checksum must be sha256:<64 lowercase hex>",
        ));
    }
    Ok(())
}

fn validate_kubernetes_resource_name(value: &str) -> Result<(), ApiError> {
    if value.is_empty()
        || value.len() > 63
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || value.starts_with('-')
        || value.ends_with('-')
    {
        return Err(ApiError::bad_request(
            "archive claim name is not a normalized Kubernetes resource name",
        ));
    }
    Ok(())
}

fn env_bool(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(default)
}
