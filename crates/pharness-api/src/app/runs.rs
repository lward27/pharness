use super::approvals::{
    active_permission_grants, decide_current_run_approval, ApprovalDecisionInput,
};
use super::audit::append_workspace_audit_event;
use super::auth::OperatorIdentity;
use super::clock::{current_millis, unique_suffix};
use super::environment::select_profile;
use super::operator::{all_runs_for_operator_groups, group_operator_records, run_group_resource};
use super::policy::run_policy;
use super::validation::{clean_optional_text, required_text};
use super::{ApiError, AppState};
use crate::dto::{
    ApproveBudgetExtensionRequest, ArtifactsResponse, BudgetExtensionResponse, CreateRunRequest,
    DecideApprovalRequest, DecideApprovalResponse, EnvironmentPreparationResponse, EventsResponse,
    FileChangeResponse, ObservationResponse, ObservationsResponse, RunDiffResponse,
    RunOperatorSummaryResponse, RunResponse, RunSummaryResponse, RunsResponse, WorkspaceResponse,
};
use crate::worker::{
    attempt_spec_for_run, finish_run_from_attempt, ingest_agent_event, sync_repo_stage_run,
};
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use futures::stream::{self, Stream};
use hmac::{Hmac, Mac};
use pharness_core::{
    AgentEvent, EventId, EventKind, RepositoryContract, RunId, RunScope, SessionId,
};
use pharness_runhost::AttemptOutcome;
use pharness_store::{
    ApprovalListFilter, CreateRun, CreateSession, RunListFilter, RunSummaryFilter, SqliteStore,
    StoreError, UpdateEnvironmentPreparation, UpdateWorkspaceExecution,
};
use serde_json::{json, Value};
use sha2::Sha256;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

pub(super) fn internal_router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/internal/runs/:run_id/attempt-context",
            get(internal_attempt_context),
        )
        .route(
            "/api/internal/runs/:run_id/mark-running",
            post(internal_mark_running),
        )
        .route(
            "/api/internal/runs/:run_id/workspace-provisioned",
            post(internal_workspace_provisioned),
        )
        .route(
            "/api/internal/runs/:run_id/environment-preparation",
            post(internal_environment_preparation),
        )
        .route(
            "/api/internal/runs/:run_id/events",
            post(internal_ingest_events),
        )
        .route(
            "/api/internal/runs/:run_id/outcome",
            post(internal_ingest_outcome),
        )
        .route(
            "/api/internal/runs/:run_id/control",
            get(internal_run_control),
        )
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/runs", get(list_runs).post(create_operator_run))
        .route("/api/runs/summary", get(run_summary))
        .route("/api/runs/:run_id", get(get_run))
        .route("/api/runs/:run_id/events", get(get_run_events))
        .route(
            "/api/runs/:run_id/operator-summary",
            get(get_run_operator_summary),
        )
        .route("/api/runs/:run_id/events/stream", get(stream_run_events))
        .route("/api/runs/:run_id/diff", get(get_run_diff))
        .route("/api/runs/:run_id/artifacts", get(list_run_artifacts))
        .route("/api/runs/:run_id/observations", get(list_run_observations))
        .route("/api/runs/:run_id/cancel", post(cancel_run))
        .route("/api/runs/:run_id/approvals", post(decide_run_approval))
        .route(
            "/api/runs/:run_id/environment-preparation",
            get(get_run_environment_preparation),
        )
        .route(
            "/api/runs/:run_id/budget-extensions/:extension_id/approve",
            post(approve_run_budget_extension),
        )
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct InternalEnvironmentPreparationRequest {
    status: String,
    #[serde(default)]
    project_contract: Option<serde_json::Value>,
    #[serde(default)]
    project_contract_hash: Option<String>,
    #[serde(default)]
    environment_snapshot: Option<serde_json::Value>,
    #[serde(default)]
    snapshot_signature: Option<String>,
    #[serde(default = "empty_array")]
    logs: serde_json::Value,
    #[serde(default)]
    error: Option<String>,
}

fn empty_array() -> serde_json::Value {
    serde_json::json!([])
}

pub(super) async fn internal_environment_preparation(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(request): Json<InternalEnvironmentPreparationRequest>,
) -> Result<Json<EnvironmentPreparationResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    let run = state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
    if run.status != "preparing" {
        return Err(ApiError::conflict(
            "run is not awaiting environment preparation",
        ));
    }
    let preparation = state
        .store
        .get_environment_preparation_by_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::conflict("run has no environment preparation record"))?;
    let work_item = state
        .store
        .get_work_item(&preparation.work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &preparation.work_item_id))?;

    if request.status == "failed" {
        let error = request
            .error
            .clone()
            .unwrap_or_else(|| "environment preparation failed".to_string());
        let preparation = state
            .store
            .update_environment_preparation(UpdateEnvironmentPreparation {
                id: preparation.id,
                status: "failed".to_string(),
                project_contract_json: request.project_contract,
                project_contract_hash: request.project_contract_hash,
                environment_snapshot_json: None,
                logs_json: request.logs,
                error: Some(error.clone()),
            })
            .await?;
        state
            .store
            .set_work_item_environment_snapshot(&work_item.id, "failed", None, None, None)
            .await?;
        state
            .store
            .complete_run(
                &run_id,
                "failed",
                json!({"stop_reason":"environment_preparation_failed"}),
                Some(error.clone()),
            )
            .await?;
        if run.execution_target_json.get("repo_mode").is_some() {
            sync_repo_stage_run(
                &state.store,
                &run,
                &AttemptOutcome::failed(format!("environment preparation failed: {error}")),
            )
            .await
            .map_err(|sync_error| ApiError::internal(sync_error.to_string()))?;
        } else {
            state
                .store
                .update_work_item_status(
                    &work_item.id,
                    "blocked",
                    Some("agent:environment-preparer".to_string()),
                    Some(error),
                )
                .await?;
        }
        return Ok(Json(preparation.into()));
    }
    if request.status != "succeeded" {
        return Err(ApiError::bad_request(
            "preparation status must be succeeded or failed",
        ));
    }
    let contract_json = request
        .project_contract
        .clone()
        .ok_or_else(|| ApiError::conflict("successful preparation has no project contract"))?;
    let contract =
        serde_json::from_value::<pharness_core::RepositoryContract>(contract_json.clone())
            .map_err(|error| {
                ApiError::conflict(format!("prepared project contract is invalid: {error}"))
            })?;
    let contract_hash = request
        .project_contract_hash
        .clone()
        .ok_or_else(|| ApiError::conflict("successful preparation has no contract hash"))?;
    let snapshot_json = request
        .environment_snapshot
        .clone()
        .ok_or_else(|| ApiError::conflict("successful preparation has no environment snapshot"))?;
    let snapshot =
        serde_json::from_value::<pharness_core::EnvironmentSnapshot>(snapshot_json.clone())
            .map_err(|error| {
                ApiError::conflict(format!("environment snapshot is invalid: {error}"))
            })?;
    let token = state.worker_token.as_deref().ok_or_else(|| {
        ApiError::conflict("worker token is unavailable for snapshot verification")
    })?;
    if !request
        .snapshot_signature
        .as_deref()
        .is_some_and(|signature| verify_environment_snapshot(token, &snapshot_json, signature))
    {
        return Err(ApiError::conflict(
            "environment snapshot signature is invalid",
        ));
    }
    let profile = select_profile(
        &state.environment_profiles,
        &preparation.environment_profile_id,
        &work_item.source_repo,
    )
    .map_err(ApiError::conflict)?;
    if snapshot.source_sha != preparation.source_commit
        || snapshot.manifest_sha256 != contract_hash
        || contract.environment_profile != profile.id
        || snapshot.runner_image_digest != profile.image
        || snapshot.runner_revision != profile.revision
        || work_item.repository_contract_hash.as_deref() != Some(contract_hash.as_str())
    {
        return Err(ApiError::conflict(
            "environment snapshot does not match the immutable WorkItem and runner profile",
        ));
    }
    let declared = contract
        .acceptance_commands
        .iter()
        .map(|command| command.command.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if work_item
        .acceptance_criteria
        .iter()
        .any(|command| !declared.contains(command.as_str()))
    {
        return Err(ApiError::conflict(
            "WorkItem acceptance command is not declared by prepared contract",
        ));
    }
    let preparation = state
        .store
        .update_environment_preparation(UpdateEnvironmentPreparation {
            id: preparation.id,
            status: "succeeded".to_string(),
            project_contract_json: Some(contract_json.clone()),
            project_contract_hash: Some(contract_hash.clone()),
            environment_snapshot_json: Some(snapshot_json.clone()),
            logs_json: request.logs,
            error: None,
        })
        .await?;
    state
        .store
        .set_work_item_environment_snapshot(
            &work_item.id,
            "succeeded",
            Some(preparation.id.clone()),
            Some(contract_json),
            Some(contract_hash),
        )
        .await?;
    let run = state
        .store
        .set_run_environment_snapshot(&run_id, snapshot_json)
        .await?;
    state.worker.spawn_run(run.clone(), run.cwd.clone());
    Ok(Json(preparation.into()))
}

fn verify_environment_snapshot(token: &str, payload: &serde_json::Value, signature: &str) -> bool {
    let Some(signature) = signature.strip_prefix("hmac-sha256:") else {
        return false;
    };
    let Some(signature) = decode_sha256(signature) else {
        return false;
    };
    let mut mac = Hmac::<Sha256>::new_from_slice(token.as_bytes())
        .expect("HMAC-SHA256 accepts keys of any size");
    mac.update(payload.to_string().as_bytes());
    mac.verify_slice(&signature).is_ok()
}

fn decode_sha256(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut decoded = [0_u8; 32];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = (chunk[0] as char).to_digit(16)?;
        let low = (chunk[1] as char).to_digit(16)?;
        decoded[index] = u8::try_from((high << 4) | low).ok()?;
    }
    Some(decoded)
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct InternalAttemptContextQuery {
    approval_id: Option<String>,
}

pub(super) async fn internal_attempt_context(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<InternalAttemptContextQuery>,
) -> Result<Json<pharness_runhost::AttemptSpec>, ApiError> {
    let run_id = RunId::new(run_id);
    let run = state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;

    let approval = match &query.approval_id {
        Some(approval_id) => {
            let approval = state
                .store
                .get_approval(approval_id)
                .await?
                .ok_or_else(|| ApiError::not_found("approval", approval_id))?;
            if approval.run_id != run_id {
                return Err(ApiError::conflict(
                    "approval does not belong to the requested run",
                ));
            }
            if approval.status != "approved" {
                return Err(ApiError::conflict(
                    "attempt resume requires an approved approval",
                ));
            }
            Some(approval)
        }
        None => None,
    };

    let cwd = std::path::PathBuf::from(&run.cwd);
    let spec = attempt_spec_for_run(&state.store, &run, &cwd, approval.as_ref())
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if let Some(source) = &spec.run.workspace_source {
        state
            .workspace
            .remote_source_allowed(source)
            .map_err(|error| ApiError::conflict(error.to_string()))?;
    }

    Ok(Json(spec))
}

pub(super) async fn internal_mark_running(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<RunResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    let run = state.store.mark_run_running(&run_id).await?;

    Ok(Json(run.into()))
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct InternalWorkspaceProvisionedRequest {
    pub(super) workspace_id: String,
    pub(super) resolved_commit: String,
    pub(super) branch: String,
}

pub(super) async fn internal_workspace_provisioned(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(request): Json<InternalWorkspaceProvisionedRequest>,
) -> Result<Json<WorkspaceResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    let run = state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
    let mut source = run
        .execution_target_json
        .get("workspace_source")
        .cloned()
        .ok_or_else(|| ApiError::conflict("run has no typed workspace source"))
        .and_then(|value| {
            serde_json::from_value::<pharness_runhost::WorkspaceSourceSpec>(value).map_err(
                |error| ApiError::conflict(format!("run has invalid workspace source: {error}")),
            )
        })?;
    if source.workspace_id != request.workspace_id || source.branch != request.branch {
        return Err(ApiError::conflict(
            "workspace provision report does not match the issued source contract",
        ));
    }
    source.resolved_commit = Some(request.resolved_commit);
    source
        .validate()
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    state
        .workspace
        .remote_source_allowed(&source)
        .map_err(|error| ApiError::conflict(error.to_string()))?;

    let scope = RunScope::from_execution_target(&run.execution_target_json).unwrap_or_default();
    if scope.workspace_id.as_deref() != Some(source.workspace_id.as_str()) {
        return Err(ApiError::conflict(
            "workspace provision report does not match the run scope",
        ));
    }
    let workspace = state
        .store
        .get_workspace(&source.workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("workspace", &source.workspace_id))?;
    if workspace.run_id.as_ref() != Some(&run.id)
        || scope.work_item_id.as_deref() != Some(workspace.work_item_id.as_str())
        || workspace.source_repo != source.source_repo
        || workspace.source_ref != source.source_ref
    {
        return Err(ApiError::conflict(
            "workspace provision report is not authorized for this run",
        ));
    }
    if workspace.status == "executing"
        && workspace.resolved_commit.as_deref() == source.resolved_commit.as_deref()
        && workspace.branch.as_deref() == Some(source.branch.as_str())
    {
        return Ok(Json(workspace.into()));
    }
    if workspace.status != "provisioning" {
        return Err(ApiError::conflict(
            "workspace is not awaiting source provisioning",
        ));
    }
    let workspace = state
        .store
        .update_workspace_execution(
            &workspace.id,
            UpdateWorkspaceExecution {
                run_id: Some(run.id.clone()),
                status: "executing".to_string(),
                resolved_commit: source.resolved_commit.clone(),
                branch: Some(source.branch.clone()),
                actor: Some("agent:cluster-worker".to_string()),
                reason: Some("remote source pinned by worker".to_string()),
            },
        )
        .await?;
    append_workspace_audit_event(
        &state.store,
        &workspace,
        "workspace.provisioned",
        Some("agent:cluster-worker".to_string()),
    )
    .await?;
    Ok(Json(workspace.into()))
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct InternalIngestEventsRequest {
    events: Vec<AgentEvent>,
}

pub(super) async fn internal_ingest_events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(request): Json<InternalIngestEventsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let run_id = RunId::new(run_id);
    let mut ingested = 0usize;
    for event in &request.events {
        if event.run_id != run_id {
            return Err(ApiError::conflict(
                "event run_id does not match the ingest route",
            ));
        }
        ingest_agent_event(&state.store, event).await?;
        ingested += 1;
    }

    Ok(Json(json!({ "ingested": ingested })))
}

pub(super) async fn internal_ingest_outcome(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(outcome): Json<AttemptOutcome>,
) -> Result<Json<RunResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    let run = state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;

    if matches!(run.status.as_str(), "completed" | "failed" | "cancelled") {
        return Err(ApiError::conflict(format!(
            "run is already terminal with status {}",
            run.status
        )));
    }

    finish_run_from_attempt(&state.store, &run, outcome)
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;

    let run = state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;

    super::products::finalize_repository_onboarding_proposer_run(&state, &run).await?;

    if let Err(error) = super::repo_mode::continue_repo_stage_chain(&state, &run).await {
        tracing::error!(run_id=%run.id, ?error, "authorized Repo Mode stage continuation failed");
        super::repo_mode::record_repo_chain_continuation_failure(&state, &run).await?;
    }

    Ok(Json(run.into()))
}

pub(super) async fn internal_run_control(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let run_id = RunId::new(run_id);
    let run = state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;

    let cancel_requested = run.cancel_requested_at.is_some() || run.status == "cancelled";

    Ok(Json(json!({
        "cancel_requested": cancel_requested,
        "status": run.status,
    })))
}

#[cfg(test)]
pub(super) async fn create_run(
    State(state): State<AppState>,
    Json(request): Json<CreateRunRequest>,
) -> Result<Json<RunResponse>, ApiError> {
    create_run_for_actor(state, request, None).await
}

pub(super) async fn create_operator_run(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Json(request): Json<CreateRunRequest>,
) -> Result<Json<RunResponse>, ApiError> {
    let actor = identity.map(|Extension(OperatorIdentity(name))| name);
    create_run_for_actor(state, request, actor).await
}

pub(super) async fn create_run_for_actor(
    state: AppState,
    request: CreateRunRequest,
    actor: Option<String>,
) -> Result<Json<RunResponse>, ApiError> {
    let run_id = RunId::new(format!("run_{}", unique_suffix()));
    let session_id = SessionId::new(format!("ses_{}", run_id.as_str()));
    let cwd = state
        .worker
        .effective_cwd(&request.cwd.unwrap_or_else(|| ".".to_string()));
    let max_turns = request.max_turns.unwrap_or(40);
    let run_scope = request.scope.unwrap_or_default();
    let run_scope_json = run_scope.to_optional_json();
    let mut policy = run_policy(&state.policy, request.policy_mode);
    policy.permission_grants = active_permission_grants(&state.store).await?;

    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: request.task.chars().take(80).collect(),
            cwd: cwd.clone(),
        })
        .await?;

    let run = state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: request.task,
            cwd: cwd.clone(),
            max_turns,
            initial_status: "queued".to_string(),
            execution_target_json: json!({
                "kind": state.worker.execution_target_kind(),
                "policy": &policy,
                "run_scope": &run_scope_json,
            }),
        })
        .await?;
    let run = state.store.set_run_origin(&run.id, "operator").await?;
    let run = state.store.set_run_created_by(&run.id, actor).await?;

    let worker_config = state.worker.config_json();
    let queue_payload = json!({
        "source": "api",
        "worker": state.worker.mode(),
        "provider": worker_config.get("provider"),
        "model": worker_config.get("model"),
        "policy_mode": policy.mode,
        "policy_environment": &policy.environment,
        "run_scope": &run_scope_json,
    });

    state
        .store
        .append_event(&AgentEvent {
            event_id: EventId::new(format!("evt_{}_1", run_id.as_str())),
            session_id,
            run_id,
            seq: 1,
            kind: EventKind::RunQueued,
            payload: queue_payload,
        })
        .await?;

    state.worker.spawn_run(run.clone(), cwd);

    Ok(Json(run.into()))
}

#[derive(Debug, Default, serde::Deserialize)]
pub(super) struct ListRunsQuery {
    pub(super) search: Option<String>,
    pub(super) status: Option<String>,
    pub(super) origin: Option<String>,
    pub(super) actor: Option<String>,
    pub(super) namespace: Option<String>,
    pub(super) repo: Option<String>,
    pub(super) branch: Option<String>,
    pub(super) production_impacting: Option<bool>,
    pub(super) started_after_ms: Option<i64>,
    pub(super) started_before_ms: Option<i64>,
    pub(super) limit: Option<u32>,
    pub(super) offset: Option<u32>,
}

pub(super) async fn list_runs(
    State(state): State<AppState>,
    Query(query): Query<ListRunsQuery>,
) -> Result<Json<RunsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let filter = RunListFilter {
        search: clean_optional_text(query.search),
        status: clean_optional_text(query.status),
        origin: clean_optional_text(query.origin),
        created_by: clean_optional_text(query.actor),
        namespace: clean_optional_text(query.namespace),
        repo: clean_optional_text(query.repo),
        branch: clean_optional_text(query.branch),
        production_impacting: query.production_impacting,
        started_after_ms: query.started_after_ms,
        started_before_ms: query.started_before_ms,
        limit,
        offset,
    };
    let count = state.store.count_runs(filter.clone()).await?;
    let runs = state
        .store
        .list_runs(filter.clone())
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<RunResponse>>();
    let group_runs = all_runs_for_operator_groups(state.store.as_ref(), filter).await?;
    let groups = group_operator_records(group_runs.iter().map(|run| {
        (
            run.id.to_string(),
            run.started_at.clone(),
            run.task.clone(),
            run_group_resource(run),
            run.status.clone(),
        )
    }));

    Ok(Json(RunsResponse {
        runs,
        groups,
        count,
        limit,
        offset,
    }))
}

pub(super) async fn run_summary(
    State(state): State<AppState>,
    Query(query): Query<ListRunsQuery>,
) -> Result<Json<RunSummaryResponse>, ApiError> {
    let summary = state
        .store
        .run_summary(RunSummaryFilter {
            status: clean_optional_text(query.status),
            namespace: clean_optional_text(query.namespace),
            repo: clean_optional_text(query.repo),
            branch: clean_optional_text(query.branch),
            production_impacting: query.production_impacting,
            started_after_ms: query.started_after_ms,
            started_before_ms: query.started_before_ms,
        })
        .await?;

    Ok(Json(RunSummaryResponse { summary }))
}

pub(super) async fn get_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<RunResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    let run = state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
    Ok(Json(run.into()))
}

pub(super) async fn get_run_environment_preparation(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<EnvironmentPreparationResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
    let preparation = state
        .store
        .get_environment_preparation_by_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("environment_preparation", run_id.as_str()))?;
    Ok(Json(preparation.into()))
}

pub(super) async fn approve_run_budget_extension(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path((run_id, extension_id)): Path<(String, String)>,
    Json(request): Json<ApproveBudgetExtensionRequest>,
) -> Result<Json<BudgetExtensionResponse>, ApiError> {
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .unwrap_or(request.actor);
    let actor = required_text(actor, "actor")?;
    let reason = required_text(request.reason, "reason")?;
    let run_id = RunId::new(run_id);
    let extension = state
        .store
        .get_budget_extension(&extension_id)
        .await?
        .ok_or_else(|| ApiError::not_found("budget_extension", &extension_id))?;
    if extension.run_id != run_id {
        return Err(ApiError::conflict(
            "budget extension does not belong to the requested run",
        ));
    }
    let (extension, run) = state
        .store
        .approve_budget_extension(&extension_id, &request.state_hash, &actor, &reason)
        .await?;
    state.worker.spawn_run(run.clone(), run.cwd.clone());
    Ok(Json(extension.into()))
}

pub(super) async fn get_run_events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<EventsResponse>, ApiError> {
    let events = state.store.list_events(&RunId::new(run_id)).await?;
    Ok(Json(EventsResponse { events }))
}

pub(super) async fn get_run_operator_summary(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<RunOperatorSummaryResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    let run = state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
    let events = state.store.list_events(&run_id).await?;
    let changes = state.store.list_file_changes(&run_id).await?;
    let scope = RunScope::from_execution_target(&run.execution_target_json).unwrap_or_default();
    let acceptance_commands = match scope.work_item_id.as_deref() {
        Some(work_item_id) => state
            .store
            .get_work_item(work_item_id)
            .await?
            .map(|item| item.acceptance_criteria)
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let contract = run
        .execution_target_json
        .get("repository_contract")
        .cloned()
        .and_then(|value| serde_json::from_value::<RepositoryContract>(value).ok());
    let pending_approvals = state
        .store
        .pending_approval_for_run(&run_id)
        .await?
        .into_iter()
        .map(|approval| approval.id)
        .collect();
    let mut turns = 0;
    let mut recoverable_failures = 0;
    let mut estimated_context_tokens = 0_u64;
    let mut actual_prompt_tokens = 0_u64;
    let mut actual_completion_tokens = 0_u64;
    let mut actual_total_tokens = 0_u64;
    let mut compactions = 0_u64;
    let mut truncated_tool_results = 0_u64;
    let mut tools_started = 0;
    let mut tools_completed = 0;
    let mut tools_failed = 0;
    let mut test_commands = Vec::new();
    let mut test_results = Vec::new();
    let mut awaiting_test_result: Option<String> = None;
    let mut environment_discovery_turns = 0;
    for event in &events {
        match event.kind {
            EventKind::ModelRequestStarted => {
                turns = turns.max(
                    event
                        .payload
                        .get("turn")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as u32
                        + 1,
                );
                estimated_context_tokens += event
                    .payload
                    .get("estimated_input_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                compactions += event
                    .payload
                    .get("compacted_exchanges")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                truncated_tool_results += event
                    .payload
                    .get("truncated_tool_results")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
            }
            EventKind::ModelResponseFinished => {
                actual_prompt_tokens += event
                    .payload
                    .get("prompt_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                actual_completion_tokens += event
                    .payload
                    .get("completion_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                actual_total_tokens += event
                    .payload
                    .get("total_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
            }
            EventKind::ActionProposed => {
                awaiting_test_result = None;
                if event.payload.get("action").and_then(Value::as_str) == Some("run_shell") {
                    if let Some(command) = event.payload.get("cmd").and_then(Value::as_str) {
                        if acceptance_commands
                            .iter()
                            .any(|declared| declared == command)
                        {
                            test_commands.push(command.to_string());
                            awaiting_test_result = Some(command.to_string());
                        }
                        if environment_discovery_command(command) {
                            environment_discovery_turns += 1;
                        }
                    }
                } else if event.payload.get("action").and_then(Value::as_str)
                    == Some("run_acceptance_command")
                {
                    if let Some(command) = event
                        .payload
                        .get("name")
                        .and_then(Value::as_str)
                        .and_then(|name| contract.as_ref()?.command(name))
                        .map(|command| command.command.clone())
                        .filter(|command| acceptance_commands.iter().any(|item| item == command))
                    {
                        test_commands.push(command.clone());
                        awaiting_test_result = Some(command);
                    }
                }
            }
            EventKind::ToolStarted => tools_started += 1,
            EventKind::ToolFinished => {
                tools_completed += 1;
                let failed = event.payload.get("success").and_then(Value::as_bool) == Some(false)
                    || event.payload.get("status").and_then(Value::as_str) == Some("error")
                    || event.payload.get("error").is_some()
                    || event.payload.pointer("/content/error").is_some();
                if failed {
                    tools_failed += 1;
                }
                if event
                    .payload
                    .pointer("/content/recoverable")
                    .and_then(Value::as_bool)
                    == Some(true)
                {
                    recoverable_failures += 1;
                }
                if let Some(command) = awaiting_test_result.take() {
                    test_results.push(json!({
                        "command": command,
                        "passed": !failed,
                        "result": event.payload,
                    }));
                }
            }
            _ => {}
        }
    }
    let acceptance_evidence = test_results
        .iter()
        .filter(|result| result.get("passed").and_then(Value::as_bool) == Some(true))
        .cloned()
        .collect();
    let mut seen_paths = std::collections::HashSet::new();
    let changed_paths = changes
        .iter()
        .filter(|change| seen_paths.insert(change.path.clone()))
        .map(|change| change.path.clone())
        .collect();
    let approvals = state
        .store
        .list_approvals(ApprovalListFilter {
            search: Some(run_id.to_string()),
            limit: 200,
            ..ApprovalListFilter::default()
        })
        .await?
        .into_iter()
        .filter(|approval| approval.run_id == run_id)
        .collect::<Vec<_>>();
    let now = current_millis() as u64;
    let approval_wait_ms = approvals
        .iter()
        .map(|approval| {
            let requested = approval.requested_at.parse::<u64>().unwrap_or(0);
            let decided = approval
                .decided_at
                .as_deref()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(now);
            decided.saturating_sub(requested)
        })
        .sum();
    let preparation = state
        .store
        .get_environment_preparation_by_run(&run_id)
        .await?;
    let preparation_duration_ms = preparation.and_then(|preparation| {
        let started = preparation.started_at?.parse::<u64>().ok()?;
        let finished = preparation.finished_at?.parse::<u64>().ok()?;
        Some(finished.saturating_sub(started))
    });
    let budget_extensions = state
        .store
        .list_budget_extensions_for_run(&run_id)
        .await?
        .len() as u32;
    Ok(Json(RunOperatorSummaryResponse {
        run_id: run_id.clone(),
        turns,
        recoverable_failures,
        retries: recoverable_failures,
        estimated_context_tokens,
        actual_prompt_tokens,
        actual_completion_tokens,
        actual_total_tokens,
        compactions,
        truncated_tool_results,
        tools_started,
        tools_completed,
        tools_failed,
        changed_paths,
        diff_reference: format!("/api/runs/{}/diff", run_id.as_str()),
        test_commands,
        test_results,
        acceptance_evidence,
        pending_approvals,
        environment_discovery_turns,
        approval_count: approvals.len() as u32,
        approval_wait_ms,
        preparation_duration_ms,
        budget_extensions,
        stop_reason: run.stop_reason,
    }))
}

fn environment_discovery_command(command: &str) -> bool {
    let command = command.to_ascii_lowercase();
    [
        "which python",
        "command -v python",
        "python --version",
        "python3 --version",
        "which docker",
        "command -v docker",
        "docker version",
        "apt-get",
        "apk ",
        "pip install",
        "import httpx",
        "import requests",
        "import socket",
    ]
    .iter()
    .any(|needle| command.contains(needle))
}

pub(super) async fn get_run_diff(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<RunDiffResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
    let changes: Vec<FileChangeResponse> = state
        .store
        .list_file_changes(&run_id)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    let diff = changes
        .iter()
        .map(|change| change.diff.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    Ok(Json(RunDiffResponse {
        run_id,
        changes,
        diff,
    }))
}

pub(super) async fn list_run_artifacts(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<ArtifactsResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
    let artifacts = state
        .store
        .list_artifacts(&run_id)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();

    Ok(Json(ArtifactsResponse { artifacts }))
}

pub(super) async fn list_run_observations(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<ObservationsResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
    let observations = state
        .store
        .list_run_observations(&run_id)
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<ObservationResponse>>();
    let count = observations.len();

    Ok(Json(ObservationsResponse {
        observations,
        count,
        limit: None,
        offset: None,
    }))
}

pub(super) async fn stream_run_events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<StreamRunEventsQuery>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let run_id = RunId::new(run_id);
    state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;

    let stream = event_stream(state.store, run_id, stream_start_seq(&headers, &query));
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[derive(Debug, Default, serde::Deserialize)]
pub(super) struct StreamRunEventsQuery {
    pub(super) after_seq: Option<u64>,
}

pub(super) fn event_stream(
    store: Arc<SqliteStore>,
    run_id: RunId,
    last_seq: u64,
) -> impl Stream<Item = Result<Event, Infallible>> {
    stream::unfold(
        EventStreamState {
            store,
            run_id,
            last_seq,
        },
        |mut state| async move {
            loop {
                match next_event(&state.store, &state.run_id, state.last_seq).await {
                    Ok(Some(event)) => {
                        state.last_seq = event.seq;
                        return Some((Ok(sse_event(event)), state));
                    }
                    Ok(None) if run_is_terminal(&state.store, &state.run_id).await => {
                        return None;
                    }
                    Ok(None) => {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                    Err(error) => {
                        return Some((Ok(sse_error_event(error)), state));
                    }
                }
            }
        },
    )
}

pub(super) struct EventStreamState {
    store: Arc<SqliteStore>,
    run_id: RunId,
    last_seq: u64,
}

pub(super) async fn next_event(
    store: &SqliteStore,
    run_id: &RunId,
    last_seq: u64,
) -> Result<Option<AgentEvent>, StoreError> {
    Ok(store
        .list_events(run_id)
        .await?
        .into_iter()
        .find(|event| event.seq > last_seq))
}

pub(super) async fn run_is_terminal(store: &SqliteStore, run_id: &RunId) -> bool {
    store
        .get_run(run_id)
        .await
        .ok()
        .flatten()
        .is_some_and(|run| matches!(run.status.as_str(), "completed" | "failed" | "cancelled"))
}

pub(super) fn last_event_seq(headers: &HeaderMap) -> u64 {
    headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(parse_last_event_id)
        .unwrap_or(0)
}

pub(super) fn stream_start_seq(headers: &HeaderMap, query: &StreamRunEventsQuery) -> u64 {
    query.after_seq.unwrap_or_else(|| last_event_seq(headers))
}

pub(super) fn parse_last_event_id(value: &str) -> Option<u64> {
    value
        .parse()
        .ok()
        .or_else(|| value.rsplit_once('_')?.1.parse().ok())
}

pub(super) fn sse_event(event: AgentEvent) -> Event {
    let event_id = event.event_id.to_string();
    let event_kind = event.kind.as_str();
    Event::default()
        .id(event_id)
        .event(event_kind)
        .json_data(event)
        .unwrap_or_else(sse_error_event)
}

pub(super) fn sse_error_event(error: impl std::fmt::Display) -> Event {
    Event::default()
        .event("stream.error")
        .data(json!({ "error": error.to_string() }).to_string())
}

pub(super) async fn cancel_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<RunResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    state.worker.cancel(&run_id);
    let run = state.store.cancel_run(&run_id).await?;
    let seq = state.store.list_events(&run_id).await?.len() as u64 + 1;
    state
        .store
        .append_event(&AgentEvent {
            event_id: EventId::new(format!("evt_{}_{}", run_id.as_str(), seq)),
            session_id: run.session_id.clone(),
            run_id,
            seq,
            kind: EventKind::RunCancelled,
            payload: json!({ "source": "api" }),
        })
        .await?;

    Ok(Json(run.into()))
}

pub(super) async fn decide_run_approval(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(request): Json<DecideApprovalRequest>,
) -> Result<Json<DecideApprovalResponse>, ApiError> {
    decide_current_run_approval(
        state,
        RunId::new(run_id),
        ApprovalDecisionInput {
            decision: request.decision,
            decided_by: request.decided_by,
            reason: request.reason,
        },
        None,
    )
    .await
}
