use crate::dto::{
    ApprovalDecision, ApprovalGateResponse, ApprovalGateSummaryResponse, ApprovalGatesResponse,
    ApprovalSummaryResponse, ApprovalsResponse, ArtifactResponse, ArtifactsResponse,
    AuditEventsResponse, ChangeSetResponse, ChangeSetsResponse, CreateChangeSetRequest,
    CreateChangeSetResponse, CreatePermissionGrantRequest, CreateRunRequest,
    CreateTrustedEnvelopeRequest, CreateWorkPlanFromRemediationPlanRequest, CreateWorkPlanResponse,
    DecideApprovalGateRequest, DecideApprovalGateResponse, DecideApprovalRequest,
    DecideApprovalResponse, EventsResponse, ExecuteCapabilityRequest, ExecuteCapabilityResponse,
    FileChangeResponse, IncidentResponse, IncidentsResponse, ObservationResponse,
    ObservationsResponse, PermissionGrantResponse, PermissionGrantsResponse,
    RemediationPlanResponse, RemediationPlansResponse, ReviewApprovalRequest,
    ReviseChangeSetRequest, ReviseChangeSetResponse, ReviseWorkPlanRequest, ReviseWorkPlanResponse,
    RevokePermissionGrantRequest, RunDiffResponse, RunResponse, RunSummaryResponse, RunsResponse,
    TransitionChangeSetRequest, TransitionChangeSetResponse, TransitionWorkPlanRequest,
    TransitionWorkPlanResponse, TrustedEnvelopeResponse, WorkPlanResponse, WorkPlansResponse,
};
use crate::worker::LocalWorker;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::stream::{self, Stream};
use pharness_core::{
    AgentAction, AgentEvent, EventId, EventKind, PermissionGrant, PermissionGrantPolicy,
    PermissionGrantScope, PolicyDecision, PolicyMode, ReadOnlyClusterTools, RunId, SafetyPolicy,
    SessionId, ToolExecutor, ToolResult,
};
use pharness_store::{
    ApprovalGateListFilter, ApprovalGateSummaryFilter, ApprovalListFilter, ApprovalSummaryFilter,
    ChangeSetListFilter, IncidentListFilter, ObservationListFilter, RemediationPlanListFilter,
    RunListFilter, RunSummaryFilter, StoredApprovalGate, StoredChangeSet, StoredPermissionGrant,
    StoredRemediationPlan, StoredWorkPlan, UpdateChangeSetRevision, UpdateWorkPlanRevision,
    WorkPlanListFilter,
};
use pharness_store::{
    CreateAuditEvent, CreateChangeSet, CreatePermissionGrant, CreateRun, CreateSession,
    CreateWorkPlan, SqliteStore, StoreError,
};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::timeout;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

const DEFAULT_DIRECT_CAPABILITY_TIMEOUT_MS: u64 = 60_000;
const MAX_DIRECT_CAPABILITY_TIMEOUT_MS: u64 = 300_000;
const DEFAULT_POLICY_SUBJECT: &str = "agent:local-worker";
const DEFAULT_TRUSTED_ENVELOPE_ENVIRONMENT: &str = "local";

#[derive(Clone)]
pub struct AppState {
    store: Arc<SqliteStore>,
    worker: Option<LocalWorker>,
    cluster_tools: ReadOnlyClusterTools,
    policy: SafetyPolicy,
}

pub fn router(
    store: Arc<SqliteStore>,
    worker: Option<LocalWorker>,
    cluster_tools: ReadOnlyClusterTools,
    policy: SafetyPolicy,
) -> Router {
    let state = AppState {
        store,
        worker,
        cluster_tools,
        policy,
    };

    Router::new()
        .route("/health", get(health))
        .route("/api/config/effective", get(config_effective))
        .route("/api/capabilities/execute", post(execute_capability))
        .route("/api/runs", get(list_runs).post(create_run))
        .route("/api/runs/summary", get(run_summary))
        .route("/api/runs/:run_id", get(get_run))
        .route("/api/runs/:run_id/events", get(get_run_events))
        .route("/api/runs/:run_id/events/stream", get(stream_run_events))
        .route("/api/runs/:run_id/diff", get(get_run_diff))
        .route("/api/runs/:run_id/artifacts", get(list_run_artifacts))
        .route("/api/runs/:run_id/observations", get(list_run_observations))
        .route("/api/runs/:run_id/cancel", post(cancel_run))
        .route("/api/runs/:run_id/approvals", post(decide_run_approval))
        .route("/api/artifacts/:artifact_id", get(get_artifact))
        .route("/api/observations", get(list_observations))
        .route("/api/observations/:observation_id", get(get_observation))
        .route("/api/incidents", get(list_incidents))
        .route("/api/incidents/:incident_id", get(get_incident))
        .route("/api/remediation-plans", get(list_remediation_plans))
        .route("/api/remediation-plans/:plan_id", get(get_remediation_plan))
        .route(
            "/api/work-plans/from-remediation-plan",
            post(create_work_plan_from_remediation_plan),
        )
        .route("/api/work-plans", get(list_work_plans))
        .route("/api/work-plans/:work_plan_id", get(get_work_plan))
        .route(
            "/api/work-plans/:work_plan_id/revise",
            post(revise_work_plan),
        )
        .route(
            "/api/work-plans/:work_plan_id/transition",
            post(transition_work_plan),
        )
        .route(
            "/api/work-plans/:work_plan_id/trusted-envelope",
            post(create_work_plan_trusted_envelope),
        )
        .route(
            "/api/change-sets",
            get(list_change_sets).post(create_change_set),
        )
        .route("/api/change-sets/:change_set_id", get(get_change_set))
        .route(
            "/api/change-sets/:change_set_id/revise",
            post(revise_change_set),
        )
        .route(
            "/api/change-sets/:change_set_id/transition",
            post(transition_change_set),
        )
        .route(
            "/api/change-sets/:change_set_id/trusted-envelope",
            post(create_change_set_trusted_envelope),
        )
        .route("/api/approval-gates", get(list_approval_gates))
        .route("/api/approval-gates/summary", get(approval_gate_summary))
        .route("/api/approval-gates/:gate_id", get(get_approval_gate))
        .route(
            "/api/approval-gates/:gate_id/satisfy",
            post(satisfy_approval_gate),
        )
        .route(
            "/api/approval-gates/:gate_id/waive",
            post(waive_approval_gate),
        )
        .route(
            "/api/approval-gates/:gate_id/reject",
            post(reject_approval_gate),
        )
        .route("/api/audit-events", get(list_audit_events))
        .route("/api/approvals", get(list_approvals))
        .route("/api/approvals/summary", get(approval_summary))
        .route("/api/approvals/:approval_id", get(get_approval))
        .route(
            "/api/approvals/:approval_id/approve",
            post(approve_approval),
        )
        .route("/api/approvals/:approval_id/deny", post(deny_approval))
        .route(
            "/api/permission-grants",
            get(list_permission_grants).post(create_permission_grant),
        )
        .route(
            "/api/permission-grants/:grant_id",
            get(get_permission_grant),
        )
        .route(
            "/api/permission-grants/:grant_id/revoke",
            post(revoke_permission_grant),
        )
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true }))
}

async fn config_effective(State(state): State<AppState>) -> Json<serde_json::Value> {
    let worker = state.worker.as_ref().map(|worker| {
        let config = worker.config();
        json!({
            "enabled": config.enabled,
            "provider": config.provider,
            "model": config.model,
            "base_url": config.base_url,
        })
    });

    Json(json!({
        "api": {
            "name": "pharness-api",
        },
        "cluster": {
            "kubectl_bin": state.cluster_tools.kubectl_bin(),
            "argocd_namespace": state.cluster_tools.argocd_namespace(),
            "prometheus_configured": state.cluster_tools.prometheus_configured(),
            "loki_configured": state.cluster_tools.loki_configured(),
            "registry_alias_count": state.cluster_tools.registry_alias_count(),
        },
        "policy": policy_json(&state.policy),
        "worker": worker.unwrap_or_else(|| {
            json!({
                "enabled": false,
                "provider": null,
                "model": null,
                "base_url": null,
            })
        }),
    }))
}

async fn execute_capability(
    State(state): State<AppState>,
    Json(request): Json<ExecuteCapabilityRequest>,
) -> Result<Json<ExecuteCapabilityResponse>, ApiError> {
    let action = request.action;
    let timeout_ms = direct_capability_timeout_ms(request.timeout_ms);
    if !is_direct_capability_action(&action) {
        return Err(ApiError::bad_request(format!(
            "{} is not exposed through direct capability execution",
            action.kind_name()
        )));
    }

    let decision = state.policy.evaluate_action(&action);
    let response = match &decision {
        PolicyDecision::Allow { .. } => {
            let action_name = action.kind_name().to_string();
            match timeout(
                Duration::from_millis(timeout_ms),
                state.cluster_tools.execute(&action),
            )
            .await
            {
                Ok(Ok(result)) => {
                    append_direct_capability_audit_event(
                        &state.store,
                        DirectCapabilityAuditInput {
                            kind: "direct_capability.executed",
                            action: &action,
                            decision: &decision,
                            executed: true,
                            cancelled: false,
                            timeout_ms,
                            result: Some(&result),
                            error: None,
                        },
                    )
                    .await?;
                    ExecuteCapabilityResponse {
                        status: "ok".to_string(),
                        action: action_name,
                        decision: decision.clone(),
                        executed: true,
                        cancelled: false,
                        timeout_ms,
                        result: Some(result),
                        error: None,
                    }
                }
                Ok(Err(error)) => {
                    let error = error.to_string();
                    append_direct_capability_audit_event(
                        &state.store,
                        DirectCapabilityAuditInput {
                            kind: "direct_capability.failed",
                            action: &action,
                            decision: &decision,
                            executed: true,
                            cancelled: false,
                            timeout_ms,
                            result: None,
                            error: Some(&error),
                        },
                    )
                    .await?;
                    ExecuteCapabilityResponse {
                        status: "tool_error".to_string(),
                        action: action_name,
                        decision: decision.clone(),
                        executed: true,
                        cancelled: false,
                        timeout_ms,
                        result: None,
                        error: Some(error),
                    }
                }
                Err(_) => {
                    let error = format!("capability execution cancelled after {timeout_ms} ms");
                    append_direct_capability_audit_event(
                        &state.store,
                        DirectCapabilityAuditInput {
                            kind: "direct_capability.cancelled",
                            action: &action,
                            decision: &decision,
                            executed: true,
                            cancelled: true,
                            timeout_ms,
                            result: None,
                            error: Some(&error),
                        },
                    )
                    .await?;
                    ExecuteCapabilityResponse {
                        status: "cancelled".to_string(),
                        action: action_name,
                        decision: decision.clone(),
                        executed: true,
                        cancelled: true,
                        timeout_ms,
                        result: None,
                        error: Some(error),
                    }
                }
            }
        }
        PolicyDecision::Ask { .. } => ExecuteCapabilityResponse {
            status: "approval_required".to_string(),
            action: action.kind_name().to_string(),
            decision: decision.clone(),
            executed: false,
            cancelled: false,
            timeout_ms,
            result: None,
            error: None,
        },
        PolicyDecision::Deny { summary, .. } => ExecuteCapabilityResponse {
            status: "denied".to_string(),
            action: action.kind_name().to_string(),
            decision: decision.clone(),
            executed: false,
            cancelled: false,
            timeout_ms,
            result: None,
            error: Some(summary.clone()),
        },
    };
    if matches!(decision, PolicyDecision::Deny { .. }) {
        append_direct_capability_audit_event(
            &state.store,
            DirectCapabilityAuditInput {
                kind: "direct_capability.denied",
                action: &action,
                decision: &decision,
                executed: false,
                cancelled: false,
                timeout_ms,
                result: None,
                error: None,
            },
        )
        .await?;
    }

    Ok(Json(response))
}

fn direct_capability_timeout_ms(requested: Option<u64>) -> u64 {
    requested
        .unwrap_or(DEFAULT_DIRECT_CAPABILITY_TIMEOUT_MS)
        .clamp(1, MAX_DIRECT_CAPABILITY_TIMEOUT_MS)
}

fn is_direct_capability_action(action: &AgentAction) -> bool {
    matches!(
        action,
        AgentAction::KubernetesGet { .. }
            | AgentAction::ArgoGetApp { .. }
            | AgentAction::PrometheusQuery { .. }
            | AgentAction::PrometheusInventory { .. }
            | AgentAction::LokiLogSummary { .. }
            | AgentAction::TektonGetPipelineRuns { .. }
            | AgentAction::TektonGetTaskRuns { .. }
            | AgentAction::TektonAnalyzePipelineRun { .. }
    )
}

fn run_policy(default: &SafetyPolicy, override_mode: Option<PolicyMode>) -> SafetyPolicy {
    let mut policy = default.clone();
    if let Some(mode) = override_mode {
        policy.mode = mode;
    }
    policy
}

fn policy_json(policy: &SafetyPolicy) -> serde_json::Value {
    json!({
        "subject": &policy.subject,
        "environment": &policy.environment,
        "mode": policy.mode,
        "allow_read_only_shell": policy.allow_read_only_shell,
        "require_approval_for_writes": policy.require_approval_for_writes,
        "require_approval_for_network": policy.require_approval_for_network,
        "require_approval_for_destructive": policy.require_approval_for_destructive,
        "deny_privileged": policy.deny_privileged,
        "deny_secret_access": policy.deny_secret_access,
        "permission_grant_count": policy.permission_grants.len(),
    })
}

async fn active_permission_grants(store: &SqliteStore) -> Result<Vec<PermissionGrant>, ApiError> {
    let now = current_millis();
    let grants = store
        .list_permission_grants(Some("active"), 200)
        .await?
        .into_iter()
        .filter(|grant| grant_is_unexpired(grant, now))
        .map(permission_grant_snapshot)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(grants)
}

fn permission_grant_snapshot(grant: StoredPermissionGrant) -> Result<PermissionGrant, ApiError> {
    let scope =
        serde_json::from_value::<PermissionGrantScope>(grant.scope_json).map_err(|error| {
            ApiError::internal(format!(
                "permission grant {} has invalid scope: {error}",
                grant.id
            ))
        })?;
    let policy =
        serde_json::from_value::<PermissionGrantPolicy>(grant.policy_json).map_err(|error| {
            ApiError::internal(format!(
                "permission grant {} has invalid policy: {error}",
                grant.id
            ))
        })?;

    Ok(PermissionGrant {
        id: grant.id,
        subject: grant.subject,
        scope,
        policy,
        expires_at: grant.expires_at,
    })
}

fn grant_is_unexpired(grant: &StoredPermissionGrant, now_millis: u128) -> bool {
    grant
        .expires_at
        .as_deref()
        .map(|expires_at| {
            expires_at
                .parse::<u128>()
                .map(|expires_at| expires_at > now_millis)
                .unwrap_or(false)
        })
        .unwrap_or(true)
}

async fn create_run(
    State(state): State<AppState>,
    Json(request): Json<CreateRunRequest>,
) -> Result<Json<RunResponse>, ApiError> {
    let run_id = RunId::new(format!("run_{}", unique_suffix()));
    let session_id = SessionId::new(format!("ses_{}", run_id.as_str()));
    let cwd = request.cwd.unwrap_or_else(|| ".".to_string());
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
                "kind": "local_process",
                "policy": &policy,
                "run_scope": &run_scope_json,
            }),
        })
        .await?;

    let queue_payload = state.worker.as_ref().map_or_else(
        || {
            json!({
                "source": "api",
                "worker": "disabled",
                "policy_mode": policy.mode,
                "policy_environment": &policy.environment,
                "run_scope": &run_scope_json,
            })
        },
        |worker| {
            let config = worker.config();
            json!({
                "source": "api",
                "worker": "local",
                "provider": config.provider,
                "model": config.model,
                "policy_mode": policy.mode,
                "policy_environment": &policy.environment,
                "run_scope": &run_scope_json,
            })
        },
    );

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

    if let Some(worker) = &state.worker {
        worker.spawn_run(run.clone(), cwd);
    }

    Ok(Json(run.into()))
}

#[derive(Debug, Default, serde::Deserialize)]
struct ListRunsQuery {
    status: Option<String>,
    namespace: Option<String>,
    repo: Option<String>,
    branch: Option<String>,
    production_impacting: Option<bool>,
    started_after_ms: Option<i64>,
    started_before_ms: Option<i64>,
    limit: Option<u32>,
    offset: Option<u32>,
}

async fn list_runs(
    State(state): State<AppState>,
    Query(query): Query<ListRunsQuery>,
) -> Result<Json<RunsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let runs = state
        .store
        .list_runs(RunListFilter {
            status: clean_optional_text(query.status),
            namespace: clean_optional_text(query.namespace),
            repo: clean_optional_text(query.repo),
            branch: clean_optional_text(query.branch),
            production_impacting: query.production_impacting,
            started_after_ms: query.started_after_ms,
            started_before_ms: query.started_before_ms,
            limit,
            offset,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let count = runs.len();

    Ok(Json(RunsResponse {
        runs,
        count,
        limit,
        offset,
    }))
}

async fn run_summary(
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

async fn get_run(
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

async fn get_run_events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<EventsResponse>, ApiError> {
    let events = state.store.list_events(&RunId::new(run_id)).await?;
    Ok(Json(EventsResponse { events }))
}

async fn get_run_diff(
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

async fn list_run_artifacts(
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

async fn get_artifact(
    State(state): State<AppState>,
    Path(artifact_id): Path<String>,
) -> Result<Json<ArtifactResponse>, ApiError> {
    let artifact = state
        .store
        .get_artifact(&artifact_id)
        .await?
        .ok_or_else(|| ApiError::not_found("artifact", &artifact_id))?;

    Ok(Json(artifact.into()))
}

async fn list_run_observations(
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
        .collect::<Vec<_>>();
    let count = observations.len();

    Ok(Json(ObservationsResponse {
        observations,
        count,
        limit: None,
        offset: None,
    }))
}

#[derive(Debug, Default, serde::Deserialize)]
struct ListObservationsQuery {
    run_id: Option<String>,
    source: Option<String>,
    kind: Option<String>,
    subject: Option<String>,
    resource_namespace: Option<String>,
    resource_kind: Option<String>,
    resource_name: Option<String>,
    observed_after_ms: Option<i64>,
    observed_before_ms: Option<i64>,
    limit: Option<u32>,
    offset: Option<u32>,
}

async fn list_observations(
    State(state): State<AppState>,
    Query(query): Query<ListObservationsQuery>,
) -> Result<Json<ObservationsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let observations = state
        .store
        .list_observations(ObservationListFilter {
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            source: clean_optional_text(query.source),
            kind: clean_optional_text(query.kind),
            subject: clean_optional_text(query.subject),
            resource_namespace: clean_optional_text(query.resource_namespace),
            resource_kind: clean_optional_text(query.resource_kind),
            resource_name: clean_optional_text(query.resource_name),
            observed_after_ms: query.observed_after_ms,
            observed_before_ms: query.observed_before_ms,
            limit,
            offset,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let count = observations.len();

    Ok(Json(ObservationsResponse {
        observations,
        count,
        limit: Some(limit),
        offset: Some(offset),
    }))
}

async fn get_observation(
    State(state): State<AppState>,
    Path(observation_id): Path<String>,
) -> Result<Json<ObservationResponse>, ApiError> {
    let observation = state
        .store
        .get_observation(&observation_id)
        .await?
        .ok_or_else(|| ApiError::not_found("observation", &observation_id))?;

    Ok(Json(observation.into()))
}

#[derive(Debug, Default, serde::Deserialize)]
struct ListIncidentsQuery {
    run_id: Option<String>,
    status: Option<String>,
    severity: Option<String>,
    resource_namespace: Option<String>,
    resource_kind: Option<String>,
    resource_name: Option<String>,
    created_after_ms: Option<i64>,
    created_before_ms: Option<i64>,
    limit: Option<u32>,
    offset: Option<u32>,
}

async fn list_incidents(
    State(state): State<AppState>,
    Query(query): Query<ListIncidentsQuery>,
) -> Result<Json<IncidentsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let incidents = state
        .store
        .list_incidents(IncidentListFilter {
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            status: clean_optional_text(query.status),
            severity: clean_optional_text(query.severity),
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
    let count = incidents.len();

    Ok(Json(IncidentsResponse {
        incidents,
        count,
        limit,
        offset,
    }))
}

async fn get_incident(
    State(state): State<AppState>,
    Path(incident_id): Path<String>,
) -> Result<Json<IncidentResponse>, ApiError> {
    let incident = state
        .store
        .get_incident(&incident_id)
        .await?
        .ok_or_else(|| ApiError::not_found("incident", &incident_id))?;

    Ok(Json(incident.into()))
}

#[derive(Debug, Default, serde::Deserialize)]
struct ListRemediationPlansQuery {
    incident_id: Option<String>,
    run_id: Option<String>,
    status: Option<String>,
    risk_level: Option<String>,
    resource_namespace: Option<String>,
    resource_kind: Option<String>,
    resource_name: Option<String>,
    created_after_ms: Option<i64>,
    created_before_ms: Option<i64>,
    limit: Option<u32>,
    offset: Option<u32>,
}

async fn list_remediation_plans(
    State(state): State<AppState>,
    Query(query): Query<ListRemediationPlansQuery>,
) -> Result<Json<RemediationPlansResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let remediation_plans = state
        .store
        .list_remediation_plans(RemediationPlanListFilter {
            incident_id: clean_optional_text(query.incident_id),
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            status: clean_optional_text(query.status),
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
    let count = remediation_plans.len();

    Ok(Json(RemediationPlansResponse {
        remediation_plans,
        count,
        limit,
        offset,
    }))
}

async fn get_remediation_plan(
    State(state): State<AppState>,
    Path(plan_id): Path<String>,
) -> Result<Json<RemediationPlanResponse>, ApiError> {
    let plan = state
        .store
        .get_remediation_plan(&plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("remediation_plan", &plan_id))?;

    Ok(Json(plan.into()))
}

#[derive(Debug, Default, serde::Deserialize)]
struct ListWorkPlansQuery {
    remediation_plan_id: Option<String>,
    incident_id: Option<String>,
    run_id: Option<String>,
    status: Option<String>,
    risk_level: Option<String>,
    resource_namespace: Option<String>,
    resource_kind: Option<String>,
    resource_name: Option<String>,
    created_after_ms: Option<i64>,
    created_before_ms: Option<i64>,
    limit: Option<u32>,
    offset: Option<u32>,
}

async fn list_work_plans(
    State(state): State<AppState>,
    Query(query): Query<ListWorkPlansQuery>,
) -> Result<Json<WorkPlansResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let work_plans = state
        .store
        .list_work_plans(WorkPlanListFilter {
            remediation_plan_id: clean_optional_text(query.remediation_plan_id),
            incident_id: clean_optional_text(query.incident_id),
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            status: clean_optional_text(query.status),
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
    let count = work_plans.len();

    Ok(Json(WorkPlansResponse {
        work_plans,
        count,
        limit,
        offset,
    }))
}

async fn get_work_plan(
    State(state): State<AppState>,
    Path(work_plan_id): Path<String>,
) -> Result<Json<WorkPlanResponse>, ApiError> {
    let work_plan = state
        .store
        .get_work_plan(&work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &work_plan_id))?;

    Ok(Json(work_plan.into()))
}

async fn transition_work_plan(
    State(state): State<AppState>,
    Path(work_plan_id): Path<String>,
    Json(request): Json<TransitionWorkPlanRequest>,
) -> Result<Json<TransitionWorkPlanResponse>, ApiError> {
    let current = state
        .store
        .get_work_plan(&work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &work_plan_id))?;
    let target = WorkPlanStatus::parse(&request.target_status)?;
    let current_status = WorkPlanStatus::parse(&current.status)?;
    current_status.ensure_can_transition_to(target)?;

    let work_plan = state
        .store
        .update_work_plan_status(
            &work_plan_id,
            target.as_str(),
            clean_optional_text(request.actor.clone()),
            clean_optional_text(request.reason.clone()),
        )
        .await?;
    append_work_plan_audit_event(
        &state.store,
        &work_plan,
        &format!("work_plan.{}", target.as_str()),
        clean_optional_text(request.actor),
        clean_optional_text(request.reason),
        json!({
            "previous_status": current.status,
            "target_status": target.as_str(),
        }),
    )
    .await?;

    Ok(Json(TransitionWorkPlanResponse {
        work_plan: work_plan.into(),
    }))
}

async fn revise_work_plan(
    State(state): State<AppState>,
    Path(work_plan_id): Path<String>,
    Json(request): Json<ReviseWorkPlanRequest>,
) -> Result<Json<ReviseWorkPlanResponse>, ApiError> {
    let current = state
        .store
        .get_work_plan(&work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &work_plan_id))?;
    if current.status == "completed" {
        return Err(ApiError::conflict("completed work plans cannot be revised"));
    }

    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let work_plan = state
        .store
        .revise_work_plan(
            &work_plan_id,
            UpdateWorkPlanRevision {
                title: clean_optional_text(request.title),
                summary: clean_optional_text(request.summary),
                risk_level: clean_optional_text(request.risk_level),
                requires_approval: request.requires_approval,
                work_plan_json: request.work_plan_json,
                actor: actor.clone(),
                reason: reason.clone(),
            },
        )
        .await?;
    let invalidated_gates = if request.material_change {
        state
            .store
            .stale_approval_gates_for_remediation_plan(
                &work_plan.remediation_plan_id,
                actor.clone(),
                reason.clone().or_else(|| {
                    Some(format!(
                        "work plan {} revised from revision {} to {}",
                        work_plan.id, current.revision, work_plan.revision
                    ))
                }),
            )
            .await?
    } else {
        Vec::new()
    };
    for gate in &invalidated_gates {
        append_approval_gate_audit_event(&state.store, gate, "approval_gate.stale", "stale")
            .await?;
    }
    let invalidated_change_set = if request.material_change {
        stale_change_set_for_work_plan(
            &state.store,
            &work_plan.id,
            actor.clone(),
            reason.clone().or_else(|| {
                Some(format!(
                    "work plan {} revised from revision {} to {}",
                    work_plan.id, current.revision, work_plan.revision
                ))
            }),
        )
        .await?
    } else {
        None
    };
    if let Some(change_set) = &invalidated_change_set {
        append_change_set_audit_event(
            &state.store,
            change_set,
            "change_set.stale",
            actor.clone(),
            reason.clone(),
            json!({
                "source": "work_plan_revision",
                "work_plan_id": work_plan.id,
                "work_plan_revision": work_plan.revision,
            }),
        )
        .await?;
    }
    append_work_plan_audit_event(
        &state.store,
        &work_plan,
        "work_plan.revised",
        actor,
        reason,
        json!({
            "previous_revision": current.revision,
            "revision": work_plan.revision,
            "material_change": request.material_change,
            "invalidated_gate_ids": invalidated_gates
                .iter()
                .map(|gate| gate.id.clone())
                .collect::<Vec<_>>(),
            "invalidated_change_set_id": invalidated_change_set
                .as_ref()
                .map(|change_set| change_set.id.clone()),
        }),
    )
    .await?;

    Ok(Json(ReviseWorkPlanResponse {
        work_plan: work_plan.into(),
        invalidated_gates: invalidated_gates.into_iter().map(Into::into).collect(),
        invalidated_change_set: invalidated_change_set.map(Into::into),
    }))
}

async fn stale_change_set_for_work_plan(
    store: &SqliteStore,
    work_plan_id: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<Option<StoredChangeSet>, StoreError> {
    let Some(change_set) = store.get_change_set_by_work_plan(work_plan_id).await? else {
        return Ok(None);
    };
    if !matches!(
        change_set.status.as_str(),
        "draft" | "proposed" | "approved"
    ) {
        return Ok(None);
    }

    store
        .update_change_set_status(&change_set.id, "stale", actor, reason)
        .await
        .map(Some)
}

async fn create_work_plan_from_remediation_plan(
    State(state): State<AppState>,
    Json(request): Json<CreateWorkPlanFromRemediationPlanRequest>,
) -> Result<Json<CreateWorkPlanResponse>, ApiError> {
    let remediation_plan_id = clean_optional_text(Some(request.remediation_plan_id))
        .ok_or_else(|| ApiError::bad_request("remediation_plan_id is required"))?;
    if let Some(existing) = state
        .store
        .get_work_plan_by_remediation_plan(&remediation_plan_id)
        .await?
    {
        return Ok(Json(CreateWorkPlanResponse {
            work_plan: existing.into(),
            created: false,
        }));
    }

    let remediation_plan = state
        .store
        .get_remediation_plan(&remediation_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("remediation_plan", &remediation_plan_id))?;
    let work_plan = state
        .store
        .create_work_plan(work_plan_from_remediation_plan(
            &remediation_plan,
            format!("wplan_{}", unique_suffix()),
        ))
        .await?;

    Ok(Json(CreateWorkPlanResponse {
        work_plan: work_plan.into(),
        created: true,
    }))
}

fn work_plan_from_remediation_plan(plan: &StoredRemediationPlan, id: String) -> CreateWorkPlan {
    let steps = plan
        .plan_json
        .get("steps")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let approval_gates = plan
        .plan_json
        .get("approval_gates")
        .cloned()
        .unwrap_or_else(|| json!([]));
    CreateWorkPlan {
        id,
        remediation_plan_id: plan.id.clone(),
        incident_id: plan.incident_id.clone(),
        session_id: plan.session_id.clone(),
        run_id: plan.run_id.clone(),
        status: "draft".to_string(),
        title: format!("WorkPlan: {}", plan.title),
        summary: plan.summary.clone(),
        risk_level: plan.risk_level.clone(),
        requires_approval: plan.requires_approval,
        resource_namespace: plan.resource_namespace.clone(),
        resource_kind: plan.resource_kind.clone(),
        resource_name: plan.resource_name.clone(),
        work_plan_json: json!({
            "source": {
                "kind": "remediation_plan",
                "id": plan.id.clone(),
                "incident_id": plan.incident_id.clone(),
            },
            "status": "draft",
            "execution": {
                "enabled": false,
                "reason": "work plan execution is not implemented",
            },
            "approval_gates": approval_gates,
            "steps": steps,
            "remediation_plan": plan.plan_json.clone(),
        }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkPlanStatus {
    Draft,
    Proposed,
    Approved,
    Executing,
    Blocked,
    Completed,
    Rejected,
}

impl WorkPlanStatus {
    fn parse(value: &str) -> Result<Self, ApiError> {
        match value {
            "draft" => Ok(Self::Draft),
            "proposed" => Ok(Self::Proposed),
            "approved" => Ok(Self::Approved),
            "executing" => Ok(Self::Executing),
            "blocked" => Ok(Self::Blocked),
            "completed" => Ok(Self::Completed),
            "rejected" => Ok(Self::Rejected),
            other => Err(ApiError::bad_request(format!(
                "unsupported work plan status: {other}"
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Proposed => "proposed",
            Self::Approved => "approved",
            Self::Executing => "executing",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::Rejected => "rejected",
        }
    }

    fn ensure_can_transition_to(self, target: Self) -> Result<(), ApiError> {
        let allowed = match self {
            Self::Draft => matches!(target, Self::Proposed | Self::Rejected),
            Self::Proposed => matches!(target, Self::Approved | Self::Rejected | Self::Draft),
            Self::Approved => matches!(target, Self::Executing | Self::Rejected | Self::Draft),
            Self::Executing => matches!(target, Self::Blocked | Self::Completed),
            Self::Blocked => matches!(target, Self::Executing | Self::Rejected | Self::Draft),
            Self::Completed | Self::Rejected => false,
        };
        if allowed {
            Ok(())
        } else {
            Err(ApiError::conflict(format!(
                "cannot transition work plan from {} to {}",
                self.as_str(),
                target.as_str()
            )))
        }
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct ListChangeSetsQuery {
    work_plan_id: Option<String>,
    remediation_plan_id: Option<String>,
    incident_id: Option<String>,
    run_id: Option<String>,
    status: Option<String>,
    risk_level: Option<String>,
    resource_namespace: Option<String>,
    resource_kind: Option<String>,
    resource_name: Option<String>,
    created_after_ms: Option<i64>,
    created_before_ms: Option<i64>,
    limit: Option<u32>,
    offset: Option<u32>,
}

async fn list_change_sets(
    State(state): State<AppState>,
    Query(query): Query<ListChangeSetsQuery>,
) -> Result<Json<ChangeSetsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let change_sets = state
        .store
        .list_change_sets(ChangeSetListFilter {
            work_plan_id: clean_optional_text(query.work_plan_id),
            remediation_plan_id: clean_optional_text(query.remediation_plan_id),
            incident_id: clean_optional_text(query.incident_id),
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            status: clean_optional_text(query.status),
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
    let count = change_sets.len();

    Ok(Json(ChangeSetsResponse {
        change_sets,
        count,
        limit,
        offset,
    }))
}

async fn get_change_set(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
) -> Result<Json<ChangeSetResponse>, ApiError> {
    let change_set = state
        .store
        .get_change_set(&change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &change_set_id))?;

    Ok(Json(change_set.into()))
}

async fn create_change_set(
    State(state): State<AppState>,
    Json(request): Json<CreateChangeSetRequest>,
) -> Result<Json<CreateChangeSetResponse>, ApiError> {
    ensure_json_object(&request.change_set_json, "change_set_json")?;
    let work_plan_id = clean_optional_text(Some(request.work_plan_id))
        .ok_or_else(|| ApiError::bad_request("work_plan_id is required"))?;
    if let Some(existing) = state
        .store
        .get_change_set_by_work_plan(&work_plan_id)
        .await?
    {
        return Ok(Json(CreateChangeSetResponse {
            change_set: existing.into(),
            created: false,
        }));
    }
    let work_plan = state
        .store
        .get_work_plan(&work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &work_plan_id))?;
    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let material_hash = material_hash(&request.change_set_json)?;
    let change_set = state
        .store
        .create_change_set(CreateChangeSet {
            id: format!("cset_{}", unique_suffix()),
            work_plan_id: work_plan.id.clone(),
            remediation_plan_id: work_plan.remediation_plan_id.clone(),
            incident_id: work_plan.incident_id.clone(),
            session_id: work_plan.session_id.clone(),
            run_id: work_plan.run_id.clone(),
            status: "draft".to_string(),
            title: clean_optional_text(request.title)
                .unwrap_or_else(|| format!("ChangeSet: {}", work_plan.title)),
            summary: clean_optional_text(request.summary).unwrap_or(work_plan.summary),
            risk_level: clean_optional_text(request.risk_level).unwrap_or(work_plan.risk_level),
            material_hash,
            resource_namespace: work_plan.resource_namespace,
            resource_kind: work_plan.resource_kind,
            resource_name: work_plan.resource_name,
            change_set_json: request.change_set_json,
        })
        .await?;
    append_change_set_audit_event(
        &state.store,
        &change_set,
        "change_set.created",
        actor,
        reason,
        json!({ "created": true }),
    )
    .await?;

    Ok(Json(CreateChangeSetResponse {
        change_set: change_set.into(),
        created: true,
    }))
}

async fn transition_change_set(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
    Json(request): Json<TransitionChangeSetRequest>,
) -> Result<Json<TransitionChangeSetResponse>, ApiError> {
    let current = state
        .store
        .get_change_set(&change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &change_set_id))?;
    let target = ChangeSetStatus::parse(&request.target_status)?;
    let current_status = ChangeSetStatus::parse(&current.status)?;
    current_status.ensure_can_transition_to(target)?;

    let change_set = state
        .store
        .update_change_set_status(
            &change_set_id,
            target.as_str(),
            clean_optional_text(request.actor.clone()),
            clean_optional_text(request.reason.clone()),
        )
        .await?;
    append_change_set_audit_event(
        &state.store,
        &change_set,
        &format!("change_set.{}", target.as_str()),
        clean_optional_text(request.actor),
        clean_optional_text(request.reason),
        json!({
            "previous_status": current.status,
            "target_status": target.as_str(),
        }),
    )
    .await?;

    Ok(Json(TransitionChangeSetResponse {
        change_set: change_set.into(),
    }))
}

async fn revise_change_set(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
    Json(request): Json<ReviseChangeSetRequest>,
) -> Result<Json<ReviseChangeSetResponse>, ApiError> {
    ensure_json_object(&request.change_set_json, "change_set_json")?;
    let current = state
        .store
        .get_change_set(&change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &change_set_id))?;
    if current.status == "applied" {
        return Err(ApiError::conflict("applied change sets cannot be revised"));
    }

    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let material_hash = material_hash(&request.change_set_json)?;
    let material_hash_changed = current.material_hash != material_hash;
    let change_set = state
        .store
        .revise_change_set(
            &change_set_id,
            UpdateChangeSetRevision {
                title: clean_optional_text(request.title),
                summary: clean_optional_text(request.summary),
                risk_level: clean_optional_text(request.risk_level),
                material_hash,
                change_set_json: request.change_set_json,
                actor: actor.clone(),
                reason: reason.clone(),
            },
        )
        .await?;
    let invalidated_gates = if request.material_change && material_hash_changed {
        state
            .store
            .stale_approval_gates_for_remediation_plan(
                &change_set.remediation_plan_id,
                actor.clone(),
                reason.clone().or_else(|| {
                    Some(format!(
                        "change set {} revised from revision {} to {}",
                        change_set.id, current.revision, change_set.revision
                    ))
                }),
            )
            .await?
    } else {
        Vec::new()
    };
    for gate in &invalidated_gates {
        append_approval_gate_audit_event(&state.store, gate, "approval_gate.stale", "stale")
            .await?;
    }
    append_change_set_audit_event(
        &state.store,
        &change_set,
        "change_set.revised",
        actor,
        reason,
        json!({
            "previous_revision": current.revision,
            "revision": change_set.revision,
            "previous_material_hash": current.material_hash,
            "material_hash": change_set.material_hash,
            "material_hash_changed": material_hash_changed,
            "material_change": request.material_change,
            "invalidated_gate_ids": invalidated_gates
                .iter()
                .map(|gate| gate.id.clone())
                .collect::<Vec<_>>(),
        }),
    )
    .await?;

    Ok(Json(ReviseChangeSetResponse {
        change_set: change_set.into(),
        material_hash_changed,
        invalidated_gates: invalidated_gates.into_iter().map(Into::into).collect(),
    }))
}

async fn create_work_plan_trusted_envelope(
    State(state): State<AppState>,
    Path(work_plan_id): Path<String>,
    Json(request): Json<CreateTrustedEnvelopeRequest>,
) -> Result<Json<TrustedEnvelopeResponse>, ApiError> {
    let work_plan = state
        .store
        .get_work_plan(&work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &work_plan_id))?;
    let grant_request = trusted_envelope_grant_request(&work_plan.id, None, &request)?;
    let actor = clean_optional_text(request.created_by.clone());
    let reason = clean_optional_text(Some(request.reason.clone()));
    let grant = create_permission_grant_record(&state.store, grant_request).await?;
    append_work_plan_audit_event(
        &state.store,
        &work_plan,
        "work_plan.trusted_envelope_created",
        actor,
        reason,
        json!({
            "permission_grant_id": grant.id,
            "work_plan_id": work_plan.id,
        }),
    )
    .await?;

    Ok(Json(TrustedEnvelopeResponse {
        grant: grant.into(),
    }))
}

async fn create_change_set_trusted_envelope(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
    Json(request): Json<CreateTrustedEnvelopeRequest>,
) -> Result<Json<TrustedEnvelopeResponse>, ApiError> {
    let change_set = state
        .store
        .get_change_set(&change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &change_set_id))?;
    let grant_request =
        trusted_envelope_grant_request(&change_set.work_plan_id, Some(&change_set.id), &request)?;
    let actor = clean_optional_text(request.created_by.clone());
    let reason = clean_optional_text(Some(request.reason.clone()));
    let grant = create_permission_grant_record(&state.store, grant_request).await?;
    append_change_set_audit_event(
        &state.store,
        &change_set,
        "change_set.trusted_envelope_created",
        actor,
        reason,
        json!({
            "permission_grant_id": grant.id,
            "work_plan_id": change_set.work_plan_id,
            "change_set_id": change_set.id,
        }),
    )
    .await?;

    Ok(Json(TrustedEnvelopeResponse {
        grant: grant.into(),
    }))
}

fn trusted_envelope_grant_request(
    work_plan_id: &str,
    change_set_id: Option<&str>,
    request: &CreateTrustedEnvelopeRequest,
) -> Result<CreatePermissionGrantRequest, ApiError> {
    let reason = clean_optional_text(Some(request.reason.clone()))
        .ok_or_else(|| ApiError::bad_request("trusted envelope reason is required"))?;
    let subject = clean_optional_text(request.subject.clone())
        .unwrap_or_else(|| DEFAULT_POLICY_SUBJECT.to_string());
    let environment = clean_optional_text(request.environment.clone())
        .unwrap_or_else(|| DEFAULT_TRUSTED_ENVELOPE_ENVIRONMENT.to_string());
    let mut scope = Map::new();
    scope.insert("environment".to_string(), json!(environment));
    scope.insert("capability_kinds".to_string(), json!(["filesystem"]));
    scope.insert("actions".to_string(), json!(["write_file", "patch_file"]));
    scope.insert("max_risk".to_string(), json!("medium"));
    scope.insert("work_plan_ids".to_string(), json!([work_plan_id]));
    if let Some(change_set_id) = change_set_id {
        scope.insert("change_set_ids".to_string(), json!([change_set_id]));
    }
    insert_optional_scope_array(&mut scope, "namespaces", request.namespace.clone());
    insert_optional_scope_array(&mut scope, "repos", request.repo.clone());
    insert_optional_scope_array(&mut scope, "branches", request.branch.clone());
    scope.insert(
        "production_impacting".to_string(),
        json!(request.production_impacting.unwrap_or(false)),
    );

    Ok(CreatePermissionGrantRequest {
        subject,
        created_by: clean_optional_text(request.created_by.clone()),
        reason,
        scope: Value::Object(scope),
        policy: json!({ "policy_mode": "trusted_writes" }),
        expires_at: request.expires_at.clone(),
    })
}

fn insert_optional_scope_array(scope: &mut Map<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = clean_optional_text(value) {
        scope.insert(key.to_string(), json!([value]));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChangeSetStatus {
    Draft,
    Proposed,
    Approved,
    Applied,
    Rejected,
    Stale,
}

impl ChangeSetStatus {
    fn parse(value: &str) -> Result<Self, ApiError> {
        match value {
            "draft" => Ok(Self::Draft),
            "proposed" => Ok(Self::Proposed),
            "approved" => Ok(Self::Approved),
            "applied" => Ok(Self::Applied),
            "rejected" => Ok(Self::Rejected),
            "stale" => Ok(Self::Stale),
            other => Err(ApiError::bad_request(format!(
                "unsupported change set status: {other}"
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Proposed => "proposed",
            Self::Approved => "approved",
            Self::Applied => "applied",
            Self::Rejected => "rejected",
            Self::Stale => "stale",
        }
    }

    fn ensure_can_transition_to(self, target: Self) -> Result<(), ApiError> {
        let allowed = match self {
            Self::Draft => matches!(target, Self::Proposed | Self::Rejected),
            Self::Proposed => matches!(target, Self::Approved | Self::Rejected | Self::Draft),
            Self::Approved => matches!(target, Self::Applied | Self::Rejected | Self::Draft),
            Self::Applied | Self::Rejected | Self::Stale => false,
        };
        if allowed {
            Ok(())
        } else {
            Err(ApiError::conflict(format!(
                "cannot transition change set from {} to {}",
                self.as_str(),
                target.as_str()
            )))
        }
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct ListApprovalGatesQuery {
    remediation_plan_id: Option<String>,
    incident_id: Option<String>,
    run_id: Option<String>,
    status: Option<String>,
    gate_kind: Option<String>,
    risk_level: Option<String>,
    resource_namespace: Option<String>,
    resource_kind: Option<String>,
    resource_name: Option<String>,
    created_after_ms: Option<i64>,
    created_before_ms: Option<i64>,
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct ApprovalGateSummaryQuery {
    remediation_plan_id: Option<String>,
    incident_id: Option<String>,
    run_id: Option<String>,
    status: Option<String>,
    gate_kind: Option<String>,
    risk_level: Option<String>,
    resource_namespace: Option<String>,
    resource_kind: Option<String>,
    resource_name: Option<String>,
    created_after_ms: Option<i64>,
    created_before_ms: Option<i64>,
}

async fn list_approval_gates(
    State(state): State<AppState>,
    Query(query): Query<ListApprovalGatesQuery>,
) -> Result<Json<ApprovalGatesResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let approval_gates = state
        .store
        .list_approval_gates(ApprovalGateListFilter {
            remediation_plan_id: clean_optional_text(query.remediation_plan_id),
            incident_id: clean_optional_text(query.incident_id),
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            status: clean_optional_text(query.status),
            gate_kind: clean_optional_text(query.gate_kind),
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
    let count = approval_gates.len();

    Ok(Json(ApprovalGatesResponse {
        approval_gates,
        count,
        limit,
        offset,
    }))
}

async fn approval_gate_summary(
    State(state): State<AppState>,
    Query(query): Query<ApprovalGateSummaryQuery>,
) -> Result<Json<ApprovalGateSummaryResponse>, ApiError> {
    let summary = state
        .store
        .approval_gate_summary(ApprovalGateSummaryFilter {
            remediation_plan_id: clean_optional_text(query.remediation_plan_id),
            incident_id: clean_optional_text(query.incident_id),
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            status: clean_optional_text(query.status),
            gate_kind: clean_optional_text(query.gate_kind),
            risk_level: clean_optional_text(query.risk_level),
            resource_namespace: clean_optional_text(query.resource_namespace),
            resource_kind: clean_optional_text(query.resource_kind),
            resource_name: clean_optional_text(query.resource_name),
            created_after_ms: query.created_after_ms,
            created_before_ms: query.created_before_ms,
        })
        .await?;

    Ok(Json(ApprovalGateSummaryResponse { summary }))
}

async fn get_approval_gate(
    State(state): State<AppState>,
    Path(gate_id): Path<String>,
) -> Result<Json<ApprovalGateResponse>, ApiError> {
    let gate = state
        .store
        .get_approval_gate(&gate_id)
        .await?
        .ok_or_else(|| ApiError::not_found("approval_gate", &gate_id))?;

    Ok(Json(gate.into()))
}

async fn satisfy_approval_gate(
    State(state): State<AppState>,
    Path(gate_id): Path<String>,
    Json(request): Json<DecideApprovalGateRequest>,
) -> Result<Json<DecideApprovalGateResponse>, ApiError> {
    decide_approval_gate(state, gate_id, "satisfied", request).await
}

async fn waive_approval_gate(
    State(state): State<AppState>,
    Path(gate_id): Path<String>,
    Json(request): Json<DecideApprovalGateRequest>,
) -> Result<Json<DecideApprovalGateResponse>, ApiError> {
    decide_approval_gate(state, gate_id, "waived", request).await
}

async fn reject_approval_gate(
    State(state): State<AppState>,
    Path(gate_id): Path<String>,
    Json(request): Json<DecideApprovalGateRequest>,
) -> Result<Json<DecideApprovalGateResponse>, ApiError> {
    decide_approval_gate(state, gate_id, "rejected", request).await
}

async fn decide_approval_gate(
    state: AppState,
    gate_id: String,
    status: &str,
    request: DecideApprovalGateRequest,
) -> Result<Json<DecideApprovalGateResponse>, ApiError> {
    let current = state
        .store
        .get_approval_gate(&gate_id)
        .await?
        .ok_or_else(|| ApiError::not_found("approval_gate", &gate_id))?;
    if current.status != "pending" {
        return Err(ApiError::conflict("approval gate is not pending"));
    }

    let gate = state
        .store
        .decide_approval_gate(
            &gate_id,
            status,
            clean_optional_text(request.decided_by.clone()),
            clean_optional_text(request.reason.clone()),
        )
        .await?;
    append_approval_gate_audit_event(
        &state.store,
        &gate,
        &format!("approval_gate.{status}"),
        status,
    )
    .await?;

    Ok(Json(DecideApprovalGateResponse {
        approval_gate: gate.into(),
    }))
}

async fn stream_run_events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let run_id = RunId::new(run_id);
    state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;

    let stream = event_stream(state.store, run_id, last_event_seq(&headers));
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

fn event_stream(
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

struct EventStreamState {
    store: Arc<SqliteStore>,
    run_id: RunId,
    last_seq: u64,
}

async fn next_event(
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

async fn run_is_terminal(store: &SqliteStore, run_id: &RunId) -> bool {
    store
        .get_run(run_id)
        .await
        .ok()
        .flatten()
        .is_some_and(|run| matches!(run.status.as_str(), "completed" | "failed" | "cancelled"))
}

fn last_event_seq(headers: &HeaderMap) -> u64 {
    headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(parse_last_event_id)
        .unwrap_or(0)
}

fn parse_last_event_id(value: &str) -> Option<u64> {
    value
        .parse()
        .ok()
        .or_else(|| value.rsplit_once('_')?.1.parse().ok())
}

fn sse_event(event: AgentEvent) -> Event {
    let event_id = event.event_id.to_string();
    let event_kind = event.kind.as_str();
    Event::default()
        .id(event_id)
        .event(event_kind)
        .json_data(event)
        .unwrap_or_else(sse_error_event)
}

fn sse_error_event(error: impl std::fmt::Display) -> Event {
    Event::default()
        .event("stream.error")
        .data(json!({ "error": error.to_string() }).to_string())
}

#[derive(Debug, Default, serde::Deserialize)]
struct ListApprovalsQuery {
    status: Option<String>,
    namespace: Option<String>,
    repo: Option<String>,
    branch: Option<String>,
    production_impacting: Option<bool>,
    requested_after_ms: Option<i64>,
    requested_before_ms: Option<i64>,
    limit: Option<u32>,
    offset: Option<u32>,
}

async fn list_approvals(
    State(state): State<AppState>,
    Query(query): Query<ListApprovalsQuery>,
) -> Result<Json<ApprovalsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let approvals = state
        .store
        .list_approvals(ApprovalListFilter {
            status: clean_optional_text(query.status),
            namespace: clean_optional_text(query.namespace),
            repo: clean_optional_text(query.repo),
            branch: clean_optional_text(query.branch),
            production_impacting: query.production_impacting,
            requested_after_ms: query.requested_after_ms,
            requested_before_ms: query.requested_before_ms,
            limit,
            offset,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let count = approvals.len();
    Ok(Json(ApprovalsResponse {
        approvals,
        count,
        limit,
        offset,
    }))
}

async fn approval_summary(
    State(state): State<AppState>,
    Query(query): Query<ApprovalSummaryQuery>,
) -> Result<Json<ApprovalSummaryResponse>, ApiError> {
    let summary = state
        .store
        .approval_summary(ApprovalSummaryFilter {
            status: clean_optional_text(query.status),
            namespace: clean_optional_text(query.namespace),
            repo: clean_optional_text(query.repo),
            branch: clean_optional_text(query.branch),
            production_impacting: query.production_impacting,
            requested_after_ms: query.requested_after_ms,
            requested_before_ms: query.requested_before_ms,
        })
        .await?;

    Ok(Json(ApprovalSummaryResponse { summary }))
}

async fn get_approval(
    State(state): State<AppState>,
    Path(approval_id): Path<String>,
) -> Result<Json<crate::dto::ApprovalResponse>, ApiError> {
    let approval = state
        .store
        .get_approval(&approval_id)
        .await?
        .ok_or_else(|| ApiError::not_found("approval", &approval_id))?;

    Ok(Json(approval.into()))
}

#[derive(Debug, Default, serde::Deserialize)]
struct ApprovalSummaryQuery {
    status: Option<String>,
    namespace: Option<String>,
    repo: Option<String>,
    branch: Option<String>,
    production_impacting: Option<bool>,
    requested_after_ms: Option<i64>,
    requested_before_ms: Option<i64>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct ListPermissionGrantsQuery {
    status: Option<String>,
    limit: Option<u32>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct ListAuditEventsQuery {
    resource_kind: Option<String>,
    resource_id: Option<String>,
    run_id: Option<String>,
    limit: Option<u32>,
}

async fn list_audit_events(
    State(state): State<AppState>,
    Query(query): Query<ListAuditEventsQuery>,
) -> Result<Json<AuditEventsResponse>, ApiError> {
    let run_id = query.run_id.as_deref().map(RunId::new);
    let events = state
        .store
        .list_audit_events(
            query.resource_kind.as_deref(),
            query.resource_id.as_deref(),
            run_id.as_ref(),
            query.limit.unwrap_or(50),
        )
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(AuditEventsResponse { events }))
}

async fn list_permission_grants(
    State(state): State<AppState>,
    Query(query): Query<ListPermissionGrantsQuery>,
) -> Result<Json<PermissionGrantsResponse>, ApiError> {
    let grants = state
        .store
        .list_permission_grants(query.status.as_deref(), query.limit.unwrap_or(50))
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(PermissionGrantsResponse { grants }))
}

async fn get_permission_grant(
    State(state): State<AppState>,
    Path(grant_id): Path<String>,
) -> Result<Json<PermissionGrantResponse>, ApiError> {
    let grant = state
        .store
        .get_permission_grant(&grant_id)
        .await?
        .ok_or_else(|| ApiError::not_found("permission grant", &grant_id))?;

    Ok(Json(grant.into()))
}

async fn create_permission_grant(
    State(state): State<AppState>,
    Json(request): Json<CreatePermissionGrantRequest>,
) -> Result<Json<PermissionGrantResponse>, ApiError> {
    let grant = create_permission_grant_record(&state.store, request).await?;

    Ok(Json(grant.into()))
}

async fn create_permission_grant_record(
    store: &SqliteStore,
    request: CreatePermissionGrantRequest,
) -> Result<StoredPermissionGrant, ApiError> {
    validate_permission_grant_request(&request)?;
    let created_by = clean_optional_text(request.created_by.clone());
    let grant = store
        .create_permission_grant(CreatePermissionGrant {
            id: format!("pgrant_{}", unique_suffix()),
            subject: request.subject,
            reason: request.reason,
            scope_json: request.scope,
            policy_json: request.policy,
            expires_at: request.expires_at,
        })
        .await?;
    append_permission_grant_audit_event(store, "permission_grant.created", &grant, created_by)
        .await?;

    Ok(grant)
}

async fn revoke_permission_grant(
    State(state): State<AppState>,
    Path(grant_id): Path<String>,
    Json(request): Json<RevokePermissionGrantRequest>,
) -> Result<Json<PermissionGrantResponse>, ApiError> {
    let grant = state
        .store
        .revoke_permission_grant(&grant_id, request.revoked_by.clone(), request.reason)
        .await?;
    append_permission_grant_audit_event(
        &state.store,
        "permission_grant.revoked",
        &grant,
        request.revoked_by,
    )
    .await?;

    Ok(Json(grant.into()))
}

async fn append_permission_grant_audit_event(
    store: &SqliteStore,
    kind: &str,
    grant: &StoredPermissionGrant,
    actor: Option<String>,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", grant.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "permission_grant".to_string(),
            resource_id: grant.id.clone(),
            run_id: None,
            payload_json: json!({
                "grant_id": grant.id,
                "subject": grant.subject,
                "status": grant.status,
                "reason": grant.reason,
                "scope": grant.scope_json,
                "policy": grant.policy_json,
                "expires_at": grant.expires_at,
                "revoked_at": grant.revoked_at,
                "revoked_by": grant.revoked_by,
                "revoke_reason": grant.revoke_reason,
            }),
        })
        .await
        .map(|_| ())
}

async fn append_change_set_audit_event(
    store: &SqliteStore,
    change_set: &StoredChangeSet,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
    extra: serde_json::Value,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", change_set.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "change_set".to_string(),
            resource_id: change_set.id.clone(),
            run_id: change_set.run_id.clone(),
            payload_json: json!({
                "change_set_id": change_set.id,
                "work_plan_id": change_set.work_plan_id,
                "remediation_plan_id": change_set.remediation_plan_id,
                "incident_id": change_set.incident_id,
                "run_id": change_set.run_id.as_ref().map(RunId::as_str),
                "status": change_set.status,
                "revision": change_set.revision,
                "material_hash": change_set.material_hash,
                "risk_level": change_set.risk_level,
                "summary": change_set.summary,
                "reason": reason,
                "resource": {
                    "namespace": change_set.resource_namespace,
                    "kind": change_set.resource_kind,
                    "name": change_set.resource_name,
                },
                "extra": extra,
            }),
        })
        .await
        .map(|_| ())
}

async fn append_work_plan_audit_event(
    store: &SqliteStore,
    plan: &StoredWorkPlan,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
    extra: serde_json::Value,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", plan.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "work_plan".to_string(),
            resource_id: plan.id.clone(),
            run_id: plan.run_id.clone(),
            payload_json: json!({
                "work_plan_id": plan.id,
                "remediation_plan_id": plan.remediation_plan_id,
                "incident_id": plan.incident_id,
                "run_id": plan.run_id.as_ref().map(RunId::as_str),
                "status": plan.status,
                "revision": plan.revision,
                "risk_level": plan.risk_level,
                "requires_approval": plan.requires_approval,
                "summary": plan.summary,
                "reason": reason,
                "resource": {
                    "namespace": plan.resource_namespace,
                    "kind": plan.resource_kind,
                    "name": plan.resource_name,
                },
                "extra": extra,
            }),
        })
        .await
        .map(|_| ())
}

async fn append_approval_gate_audit_event(
    store: &SqliteStore,
    gate: &StoredApprovalGate,
    kind: &str,
    decision: &str,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", gate.id, unique_suffix()),
            kind: kind.to_string(),
            actor: gate
                .stale_by
                .clone()
                .or_else(|| gate.decided_by.clone())
                .or_else(|| Some("api".to_string())),
            resource_kind: "approval_gate".to_string(),
            resource_id: gate.id.clone(),
            run_id: gate.run_id.clone(),
            payload_json: json!({
                "approval_gate_id": gate.id,
                "remediation_plan_id": gate.remediation_plan_id,
                "incident_id": gate.incident_id,
                "run_id": gate.run_id.as_ref().map(RunId::as_str),
                "status": gate.status,
                "decision": decision,
                "gate_kind": gate.gate_kind,
                "gate_order": gate.gate_order,
                "risk_level": gate.risk_level,
                "summary": gate.summary,
                "resource": {
                    "namespace": gate.resource_namespace,
                    "kind": gate.resource_kind,
                    "name": gate.resource_name,
                },
                "decided_at": gate.decided_at,
                "decided_by": gate.decided_by,
                "reason": gate.decision_reason,
                "stale_at": gate.stale_at,
                "stale_by": gate.stale_by,
                "stale_reason": gate.stale_reason,
            }),
        })
        .await
        .map(|_| ())
}

fn validate_permission_grant_request(
    request: &CreatePermissionGrantRequest,
) -> Result<(), ApiError> {
    if request.subject.trim().is_empty() {
        return Err(ApiError::bad_request(
            "permission grant subject is required",
        ));
    }
    if request.reason.trim().is_empty() {
        return Err(ApiError::bad_request("permission grant reason is required"));
    }
    if request
        .created_by
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(ApiError::bad_request(
            "permission grant created_by cannot be blank",
        ));
    }
    if !request.scope.is_object() {
        return Err(ApiError::bad_request(
            "permission grant scope must be a JSON object",
        ));
    }
    if !request.policy.is_object() {
        return Err(ApiError::bad_request(
            "permission grant policy must be a JSON object",
        ));
    }
    let scope =
        serde_json::from_value::<PermissionGrantScope>(request.scope.clone()).map_err(|error| {
            ApiError::bad_request(format!("permission grant scope is invalid: {error}"))
        })?;
    if scope
        .environment
        .as_deref()
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        return Err(ApiError::bad_request(
            "permission grant scope.environment is required",
        ));
    }
    serde_json::from_value::<PermissionGrantPolicy>(request.policy.clone()).map_err(|error| {
        ApiError::bad_request(format!("permission grant policy is invalid: {error}"))
    })?;
    if let Some(expires_at) = &request.expires_at {
        expires_at.parse::<u128>().map_err(|_| {
            ApiError::bad_request("permission grant expires_at must be unix milliseconds")
        })?;
    }

    Ok(())
}

fn clean_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn ensure_json_object(value: &serde_json::Value, field: &str) -> Result<(), ApiError> {
    if value.is_object() {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "{field} must be a JSON object"
        )))
    }
}

fn material_hash(value: &serde_json::Value) -> Result<String, ApiError> {
    let encoded = serde_json::to_vec(value)
        .map_err(|error| ApiError::internal(format!("failed to encode material hash: {error}")))?;
    let digest = Sha256::digest(encoded);
    Ok(format!("sha256:{digest:x}"))
}

async fn cancel_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<RunResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    if let Some(worker) = &state.worker {
        worker.cancel(&run_id);
    }
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

async fn decide_run_approval(
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

async fn approve_approval(
    State(state): State<AppState>,
    Path(approval_id): Path<String>,
    Json(request): Json<ReviewApprovalRequest>,
) -> Result<Json<DecideApprovalResponse>, ApiError> {
    decide_approval_by_id(
        state,
        approval_id,
        ApprovalDecisionInput {
            decision: ApprovalDecision::Approve,
            decided_by: request.decided_by,
            reason: request.reason,
        },
    )
    .await
}

async fn deny_approval(
    State(state): State<AppState>,
    Path(approval_id): Path<String>,
    Json(request): Json<ReviewApprovalRequest>,
) -> Result<Json<DecideApprovalResponse>, ApiError> {
    decide_approval_by_id(
        state,
        approval_id,
        ApprovalDecisionInput {
            decision: ApprovalDecision::Deny,
            decided_by: request.decided_by,
            reason: request.reason,
        },
    )
    .await
}

async fn decide_approval_by_id(
    state: AppState,
    approval_id: String,
    input: ApprovalDecisionInput,
) -> Result<Json<DecideApprovalResponse>, ApiError> {
    let approval = state
        .store
        .get_approval(&approval_id)
        .await?
        .ok_or_else(|| ApiError::not_found("approval", &approval_id))?;
    if approval.status != "pending" {
        return Err(ApiError::conflict("approval is not pending"));
    }

    let run_id = approval.run_id.clone();
    decide_current_run_approval(state, run_id, input, Some(approval_id.as_str())).await
}

struct ApprovalDecisionInput {
    decision: ApprovalDecision,
    decided_by: Option<String>,
    reason: Option<String>,
}

async fn decide_current_run_approval(
    state: AppState,
    run_id: RunId,
    input: ApprovalDecisionInput,
    expected_approval_id: Option<&str>,
) -> Result<Json<DecideApprovalResponse>, ApiError> {
    let pending = state
        .store
        .pending_approval_for_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::conflict("run has no pending approval"))?;
    if let Some(expected_approval_id) = expected_approval_id {
        if pending.id != expected_approval_id {
            return Err(ApiError::conflict(
                "approval is not the current pending approval for its run",
            ));
        }
    }

    let decided_by = input.decided_by;
    let reason = input.reason;

    match input.decision {
        ApprovalDecision::Deny => {
            let approval = state
                .store
                .decide_pending_approval(&run_id, "denied", decided_by.clone(), reason.clone())
                .await?;
            append_approval_decided_event(&state.store, &approval, "denied").await?;
            append_approval_decision_audit_event(
                &state.store,
                &approval,
                "approval.denied",
                "denied",
                decided_by,
                reason,
            )
            .await?;
            let run = state
                .store
                .complete_run(
                    &run_id,
                    "failed",
                    json!({
                        "status": "failed",
                        "turns": approval.turns_completed,
                        "summary": approval.summary,
                        "error": "approval denied",
                        "approval_id": approval.id,
                        "run_scope": approval.run_scope_json,
                    }),
                    Some("approval denied".to_string()),
                )
                .await?;

            Ok(Json(DecideApprovalResponse {
                approval: approval.into(),
                run: run.into(),
            }))
        }
        ApprovalDecision::Approve => {
            if pending.action_json.is_none() {
                return Err(ApiError::conflict(
                    "pending approval has no reviewed action to resume",
                ));
            }
            let Some(worker) = &state.worker else {
                return Err(ApiError::conflict(
                    "cannot approve without an enabled local worker",
                ));
            };

            let approval = state
                .store
                .decide_pending_approval(&run_id, "approved", decided_by.clone(), reason.clone())
                .await?;
            append_approval_decided_event(&state.store, &approval, "approved").await?;
            append_approval_decision_audit_event(
                &state.store,
                &approval,
                "approval.approved",
                "approved",
                decided_by,
                reason,
            )
            .await?;
            let run = state.store.mark_run_running(&run_id).await?;
            worker.resume_run(run.clone(), approval.clone());

            Ok(Json(DecideApprovalResponse {
                approval: approval.into(),
                run: run.into(),
            }))
        }
    }
}

struct DirectCapabilityAuditInput<'a> {
    kind: &'a str,
    action: &'a AgentAction,
    decision: &'a PolicyDecision,
    executed: bool,
    cancelled: bool,
    timeout_ms: u64,
    result: Option<&'a ToolResult>,
    error: Option<&'a str>,
}

async fn append_direct_capability_audit_event(
    store: &SqliteStore,
    input: DirectCapabilityAuditInput<'_>,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!(
                "aud_direct_{}_{}",
                input.action.id().as_str(),
                unique_suffix()
            ),
            kind: input.kind.to_string(),
            actor: Some("api".to_string()),
            resource_kind: "capability".to_string(),
            resource_id: input.action.kind_name().to_string(),
            run_id: None,
            payload_json: json!({
                "action": input.action.kind_name(),
                "action_id": input.action.id().as_str(),
                "decision": input.decision,
                "executed": input.executed,
                "cancelled": input.cancelled,
                "timeout_ms": input.timeout_ms,
                "result": input.result.map(direct_capability_result_summary),
                "error": input.error.map(|value| truncate_audit_text(value, 512)),
            }),
        })
        .await
        .map(|_| ())
}

fn direct_capability_result_summary(result: &ToolResult) -> Value {
    let mut summary = Map::new();
    summary.insert("tool_status".to_string(), json!(result.status));
    summary.insert(
        "summary".to_string(),
        Value::String(truncate_audit_text(&result.summary, 256)),
    );
    insert_cloned(&mut summary, "source", result.content.get("source"));
    insert_cloned(&mut summary, "resource", result.content.get("resource"));
    insert_cloned(
        &mut summary,
        "stdout_truncated",
        result.content.get("stdout_truncated"),
    );
    insert_object_if_not_empty(
        &mut summary,
        "output",
        select_json_paths(
            &result.content,
            &[
                ("kind", "/output/kind"),
                ("name", "/output/metadata/name"),
                ("namespace", "/output/metadata/namespace"),
                ("item_count", "/output/item_count"),
            ],
        ),
    );
    insert_object_if_not_empty(
        &mut summary,
        "response",
        select_json_paths(
            &result.content,
            &[
                ("result_count", "/response/data/result_count"),
                ("results_truncated", "/response/data/results_truncated"),
                ("stream_count", "/response/data/stream_count"),
                ("streams_truncated", "/response/data/streams_truncated"),
                ("entry_count", "/response/data/entry_count"),
                ("entries_truncated", "/response/data/entries_truncated"),
            ],
        ),
    );
    insert_object_if_not_empty(
        &mut summary,
        "inventory",
        select_json_paths(
            &result.content,
            &[
                ("active_targets", "/inventory/targets/active_count"),
                ("unhealthy_targets", "/inventory/targets/unhealthy_count"),
                ("rules", "/inventory/rules/rule_count"),
                ("problem_rules", "/inventory/rules/problem_rule_count"),
                ("alerts", "/inventory/alerts/alert_count"),
            ],
        ),
    );
    insert_object_if_not_empty(
        &mut summary,
        "analysis",
        select_json_paths(
            &result.content,
            &[
                ("status", "/analysis/summary/status"),
                ("task_run_count", "/analysis/summary/task_run_count"),
                (
                    "succeeded_task_runs",
                    "/analysis/summary/succeeded_task_runs",
                ),
                ("failed_task_runs", "/analysis/summary/failed_task_runs"),
                ("deployment_status", "/analysis/deployment/status"),
                ("argo_sync_status", "/analysis/argo_application/sync_status"),
                (
                    "argo_health_status",
                    "/analysis/argo_application/health_status",
                ),
                (
                    "image_alignment_status",
                    "/analysis/summary/image_alignment/status",
                ),
            ],
        ),
    );

    Value::Object(summary)
}

fn select_json_paths(source: &Value, paths: &[(&str, &str)]) -> Map<String, Value> {
    let mut selected = Map::new();
    for (key, pointer) in paths {
        insert_cloned(&mut selected, key, source.pointer(pointer));
    }
    selected
}

fn insert_cloned(target: &mut Map<String, Value>, key: &str, value: Option<&Value>) {
    if let Some(value) = value {
        target.insert(key.to_string(), value.clone());
    }
}

fn insert_object_if_not_empty(
    target: &mut Map<String, Value>,
    key: &str,
    value: Map<String, Value>,
) {
    if !value.is_empty() {
        target.insert(key.to_string(), Value::Object(value));
    }
}

fn truncate_audit_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }

    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }

    format!("{}...[truncated]", &value[..end])
}

async fn append_approval_decision_audit_event(
    store: &SqliteStore,
    approval: &pharness_store::StoredApproval,
    kind: &str,
    decision: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", approval.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.clone().or_else(|| Some("api".to_string())),
            resource_kind: "approval".to_string(),
            resource_id: approval.id.clone(),
            run_id: Some(approval.run_id.clone()),
            payload_json: json!({
                "approval_id": approval.id,
                "run_id": approval.run_id.as_str(),
                "decision": decision,
                "kind": approval.kind,
                "summary": approval.summary,
                "risk_level": approval.risk_level,
                "turns_completed": approval.turns_completed,
                "action": approval_action_kind(approval),
                "run_scope": approval.run_scope_json,
                "decided_by": actor,
                "reason": reason,
            }),
        })
        .await
        .map(|_| ())
}

fn approval_action_kind(approval: &pharness_store::StoredApproval) -> Option<&str> {
    approval
        .action_json
        .as_ref()
        .and_then(|action| action.get("action"))
        .and_then(serde_json::Value::as_str)
}

async fn append_approval_decided_event(
    store: &SqliteStore,
    approval: &pharness_store::StoredApproval,
    decision: &str,
) -> Result<(), StoreError> {
    let seq = store.list_events(&approval.run_id).await?.len() as u64 + 1;
    store
        .append_event(&AgentEvent {
            event_id: EventId::new(format!("evt_{}_{}", approval.run_id.as_str(), seq)),
            session_id: approval.session_id.clone(),
            run_id: approval.run_id.clone(),
            seq,
            kind: EventKind::ApprovalDecided,
            payload: json!({
                "approval_id": approval.id,
                "decision": decision,
                "kind": approval.kind,
                "run_scope": approval.run_scope_json,
            }),
        })
        .await
}

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn current_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found(entity: &str, id: &str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: format!("{entity} not found: {id}"),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::NotFound { entity, id } => Self::not_found(&entity, &id),
            other => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: other.to_string(),
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": self.message,
            })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        approval_gate_summary, approval_summary, cancel_run, config_effective, create_change_set,
        create_change_set_trusted_envelope, create_run, create_work_plan_from_remediation_plan,
        create_work_plan_trusted_envelope, decide_run_approval, deny_approval, execute_capability,
        get_approval, get_approval_gate, get_artifact, get_incident, get_observation,
        get_permission_grant, get_remediation_plan, get_run, get_run_diff, get_run_events,
        get_work_plan, last_event_seq, list_approval_gates, list_approvals, list_audit_events,
        list_change_sets, list_incidents, list_observations, list_permission_grants,
        list_remediation_plans, list_run_artifacts, list_run_observations, list_runs,
        list_work_plans, parse_last_event_id, policy_json, revise_change_set, revise_work_plan,
        revoke_permission_grant, router, run_policy, run_summary, satisfy_approval_gate,
        transition_change_set, transition_work_plan, unique_suffix,
        validate_permission_grant_request, AppState, ApprovalGateSummaryQuery,
        ApprovalSummaryQuery, ListApprovalGatesQuery, ListApprovalsQuery, ListAuditEventsQuery,
        ListChangeSetsQuery, ListIncidentsQuery, ListObservationsQuery, ListPermissionGrantsQuery,
        ListRemediationPlansQuery, ListRunsQuery, ListWorkPlansQuery,
    };
    use crate::dto::{
        ApprovalDecision, CreateChangeSetRequest, CreatePermissionGrantRequest, CreateRunRequest,
        CreateTrustedEnvelopeRequest, CreateWorkPlanFromRemediationPlanRequest,
        DecideApprovalGateRequest, DecideApprovalRequest, ExecuteCapabilityRequest,
        ReviewApprovalRequest, ReviseChangeSetRequest, ReviseWorkPlanRequest,
        RevokePermissionGrantRequest, TransitionChangeSetRequest, TransitionWorkPlanRequest,
    };
    use axum::extract::{Path, Query, State};
    use axum::http::{HeaderMap, HeaderValue, StatusCode};
    use axum::Json;
    use pharness_core::{AgentAction, PolicyMode, ReadOnlyClusterTools, RunScope, SafetyPolicy};
    use pharness_store::{
        CreateApproval, CreateApprovalGate, CreateArtifact, CreateFileChange, CreateIncident,
        CreateObservation, CreateRemediationPlan, SqliteStore,
    };
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::Arc;

    async fn test_state() -> AppState {
        AppState {
            store: Arc::new(SqliteStore::connect_in_memory().await.unwrap()),
            worker: None,
            cluster_tools: ReadOnlyClusterTools::default(),
            policy: SafetyPolicy::default(),
        }
    }

    async fn test_state_with_cluster_tools(cluster_tools: ReadOnlyClusterTools) -> AppState {
        AppState {
            store: Arc::new(SqliteStore::connect_in_memory().await.unwrap()),
            worker: None,
            cluster_tools,
            policy: SafetyPolicy::default(),
        }
    }

    #[tokio::test]
    async fn router_mounts_static_and_dynamic_run_routes() {
        let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());

        let _app = router(
            store,
            None,
            ReadOnlyClusterTools::default(),
            SafetyPolicy::default(),
        );
    }

    fn fake_kubectl_script() -> PathBuf {
        let path = std::env::temp_dir().join(format!("pharness-fake-kubectl-{}", unique_suffix()));
        fs::write(
            &path,
            r#"#!/bin/sh
printf '%s\n' '{"apiVersion":"v1","kind":"List","items":[]}'
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }

    fn slow_fake_kubectl_script() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("pharness-slow-fake-kubectl-{}", unique_suffix()));
        fs::write(
            &path,
            r#"#!/bin/sh
sleep 2
printf '%s\n' '{"apiVersion":"v1","kind":"List","items":[]}'
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }

    #[tokio::test]
    async fn creates_gets_lists_events_and_cancels_run() {
        let state = test_state().await;

        let Json(created) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "inspect app".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
                policy_mode: None,
                scope: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(created.status, "queued");
        assert_eq!(created.max_turns, 12);

        let Json(fetched) = get_run(State(state.clone()), Path(created.id.to_string()))
            .await
            .unwrap();
        assert_eq!(fetched.id, created.id);

        let Json(events) = get_run_events(State(state.clone()), Path(created.id.to_string()))
            .await
            .unwrap();
        assert_eq!(events.events.len(), 1);

        let Json(cancelled) = cancel_run(State(state.clone()), Path(created.id.to_string()))
            .await
            .unwrap();
        assert_eq!(cancelled.status, "cancelled");
    }

    #[tokio::test]
    async fn create_run_persists_requested_policy_mode() {
        let state = test_state().await;

        let Json(created) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "write file".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
                policy_mode: Some(PolicyMode::TrustedWrites),
                scope: None,
            }),
        )
        .await
        .unwrap();
        let stored = state.store.get_run(&created.id).await.unwrap().unwrap();
        let Json(events) = get_run_events(State(state.clone()), Path(created.id.to_string()))
            .await
            .unwrap();

        assert_eq!(
            stored.execution_target_json["policy"]["mode"],
            "trusted_writes"
        );
        assert_eq!(
            stored.execution_target_json["policy"]["environment"],
            "local"
        );
        assert_eq!(
            events.events[0].payload["policy_mode"],
            serde_json::json!("trusted_writes")
        );
        assert_eq!(
            events.events[0].payload["policy_environment"],
            serde_json::json!("local")
        );
    }

    #[tokio::test]
    async fn create_run_normalizes_empty_run_scope() {
        let state = test_state().await;

        let Json(created) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "inspect app".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
                policy_mode: None,
                scope: None,
            }),
        )
        .await
        .unwrap();
        let stored = state.store.get_run(&created.id).await.unwrap().unwrap();
        let Json(fetched) = get_run(State(state.clone()), Path(created.id.to_string()))
            .await
            .unwrap();
        let Json(events) = get_run_events(State(state.clone()), Path(created.id.to_string()))
            .await
            .unwrap();

        assert!(stored.execution_target_json["run_scope"].is_null());
        assert!(fetched.scope.is_none());
        assert!(events.events[0].payload["run_scope"].is_null());
    }

    #[tokio::test]
    async fn create_run_persists_run_scope_metadata() {
        let state = test_state().await;
        let scope = RunScope {
            namespace: Some("apps-dev".to_string()),
            repo: Some("git@example.test/team/app.git".to_string()),
            branch: Some("feature/pharness".to_string()),
            work_plan_id: Some("wplan_scope".to_string()),
            change_set_id: Some("cset_scope".to_string()),
            production_impacting: false,
        };

        let Json(created) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "inspect app".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
                policy_mode: None,
                scope: Some(scope.clone()),
            }),
        )
        .await
        .unwrap();
        let stored = state.store.get_run(&created.id).await.unwrap().unwrap();
        let Json(fetched) = get_run(State(state.clone()), Path(created.id.to_string()))
            .await
            .unwrap();
        let Json(events) = get_run_events(State(state.clone()), Path(created.id.to_string()))
            .await
            .unwrap();

        assert_eq!(
            stored.execution_target_json["run_scope"]["namespace"],
            "apps-dev"
        );
        assert_eq!(fetched.scope.as_ref(), Some(&scope));
        assert_eq!(
            events.events[0].payload["run_scope"]["branch"],
            "feature/pharness"
        );

        let Json(listed) = list_runs(
            State(state.clone()),
            Query(ListRunsQuery {
                status: Some("queued".to_string()),
                namespace: Some("apps-dev".to_string()),
                repo: Some("git@example.test/team/app.git".to_string()),
                branch: Some("feature/pharness".to_string()),
                production_impacting: Some(false),
                started_after_ms: Some(0),
                started_before_ms: None,
                limit: Some(10),
                offset: Some(0),
            }),
        )
        .await
        .unwrap();

        assert_eq!(listed.count, 1);
        assert_eq!(listed.runs[0].id, created.id);
        assert_eq!(listed.runs[0].started_at, fetched.started_at);

        let Json(summary) = run_summary(
            State(state),
            Query(ListRunsQuery {
                status: Some("queued".to_string()),
                namespace: Some("apps-dev".to_string()),
                repo: Some("git@example.test/team/app.git".to_string()),
                branch: Some("feature/pharness".to_string()),
                production_impacting: Some(false),
                started_after_ms: Some(0),
                started_before_ms: None,
                limit: None,
                offset: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(summary.summary.total, 1);
        assert_eq!(
            summary.summary.by_status[0].value.as_deref(),
            Some("queued")
        );
    }

    #[tokio::test]
    async fn create_run_snapshots_active_permission_grants() {
        let state = test_state().await;

        let Json(grant) = super::create_permission_grant(
            State(state.clone()),
            Json(CreatePermissionGrantRequest {
                subject: "agent:local-worker".to_string(),
                created_by: None,
                reason: "trusted local write smoke".to_string(),
                scope: serde_json::json!({
                    "environment": "local",
                    "capability_kinds": ["filesystem"],
                    "actions": ["write_file"],
                    "max_risk": "medium"
                }),
                policy: serde_json::json!({
                    "policy_mode": "trusted_writes"
                }),
                expires_at: None,
            }),
        )
        .await
        .unwrap();
        let Json(created) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "write file".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
                policy_mode: None,
                scope: None,
            }),
        )
        .await
        .unwrap();
        let stored = state.store.get_run(&created.id).await.unwrap().unwrap();

        assert_eq!(
            stored.execution_target_json["policy"]["permission_grants"][0]["id"],
            grant.id
        );
    }

    #[tokio::test]
    async fn reports_disabled_worker_config() {
        let state = test_state().await;

        let Json(config) = config_effective(State(state)).await;

        assert_eq!(config["worker"]["enabled"], false);
        assert!(config["worker"]["model"].is_null());
        assert_eq!(config["cluster"]["argocd_namespace"], "argocd");
        assert_eq!(config["cluster"]["loki_configured"], false);
        assert_eq!(config["policy"]["mode"], "default");
        assert_eq!(config["policy"]["environment"], "local");
    }

    #[test]
    fn run_policy_applies_mode_override_without_mutating_defaults() {
        let default = SafetyPolicy::default();
        let policy = run_policy(&default, Some(PolicyMode::TrustedWrites));

        assert_eq!(policy.mode, PolicyMode::TrustedWrites);
        assert_eq!(default.mode, PolicyMode::Default);
    }

    #[test]
    fn policy_json_exposes_decision_flags_without_secrets() {
        let policy = SafetyPolicy {
            mode: PolicyMode::Plan,
            ..SafetyPolicy::default()
        };
        let json = policy_json(&policy);

        assert_eq!(json["mode"], "plan");
        assert_eq!(json["subject"], "agent:local-worker");
        assert_eq!(json["environment"], "local");
        assert_eq!(json["permission_grant_count"], 0);
        assert_eq!(json["deny_secret_access"], true);
    }

    #[tokio::test]
    async fn direct_capability_execution_denies_secret_reads() {
        let state = test_state().await;
        let Json(response) = execute_capability(
            State(state.clone()),
            Json(ExecuteCapabilityRequest {
                action: AgentAction::KubernetesGet {
                    id: "act_secret".into(),
                    reason: "read secret".to_string(),
                    resource: "secrets".to_string(),
                    namespace: Some("argocd".to_string()),
                    name: None,
                    all_namespaces: false,
                    label_selector: None,
                },
                timeout_ms: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status, "denied");
        assert_eq!(response.action, "kubernetes_get");
        assert!(!response.executed);
        assert!(response.result.is_none());
        let Json(audit_events) = list_audit_events(
            State(state),
            Query(ListAuditEventsQuery {
                resource_kind: Some("capability".to_string()),
                resource_id: Some("kubernetes_get".to_string()),
                run_id: None,
                limit: Some(50),
            }),
        )
        .await
        .unwrap();
        assert!(audit_events.events.iter().any(|event| {
            event.kind == "direct_capability.denied"
                && event.payload["action"] == "kubernetes_get"
                && event.payload["executed"] == false
        }));
    }

    #[tokio::test]
    async fn direct_capability_execution_audits_success_summary() {
        let fake_kubectl = fake_kubectl_script();
        let state = test_state_with_cluster_tools(
            ReadOnlyClusterTools::default().with_kubectl_bin(fake_kubectl.display().to_string()),
        )
        .await;
        let Json(response) = execute_capability(
            State(state.clone()),
            Json(ExecuteCapabilityRequest {
                action: AgentAction::KubernetesGet {
                    id: "act_pods".into(),
                    reason: "read pods".to_string(),
                    resource: "pods".to_string(),
                    namespace: Some("argocd".to_string()),
                    name: None,
                    all_namespaces: false,
                    label_selector: None,
                },
                timeout_ms: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status, "ok");
        assert_eq!(response.action, "kubernetes_get");
        assert!(response.executed);
        let Json(audit_events) = list_audit_events(
            State(state),
            Query(ListAuditEventsQuery {
                resource_kind: Some("capability".to_string()),
                resource_id: Some("kubernetes_get".to_string()),
                run_id: None,
                limit: Some(50),
            }),
        )
        .await
        .unwrap();
        let event = audit_events
            .events
            .iter()
            .find(|event| event.kind == "direct_capability.executed")
            .unwrap();

        assert_eq!(event.payload["executed"], true);
        assert_eq!(event.payload["result"]["source"], "kubernetes");
        assert_eq!(event.payload["result"]["output"]["item_count"], 0);
        assert!(!event.payload.to_string().contains("PodList"));
        let _ = fs::remove_file(fake_kubectl);
    }

    #[tokio::test]
    async fn direct_capability_execution_can_be_cancelled_by_timeout() {
        let fake_kubectl = slow_fake_kubectl_script();
        let state = test_state_with_cluster_tools(
            ReadOnlyClusterTools::default()
                .with_kubectl_bin(fake_kubectl.display().to_string())
                .with_timeout_ms(5_000),
        )
        .await;
        let Json(response) = execute_capability(
            State(state.clone()),
            Json(ExecuteCapabilityRequest {
                action: AgentAction::KubernetesGet {
                    id: "act_cancel".into(),
                    reason: "read pods".to_string(),
                    resource: "pods".to_string(),
                    namespace: Some("argocd".to_string()),
                    name: None,
                    all_namespaces: false,
                    label_selector: None,
                },
                timeout_ms: Some(10),
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status, "cancelled");
        assert_eq!(response.action, "kubernetes_get");
        assert!(response.executed);
        assert!(response.cancelled);
        assert_eq!(response.timeout_ms, 10);
        assert!(response.result.is_none());
        let Json(audit_events) = list_audit_events(
            State(state),
            Query(ListAuditEventsQuery {
                resource_kind: Some("capability".to_string()),
                resource_id: Some("kubernetes_get".to_string()),
                run_id: None,
                limit: Some(50),
            }),
        )
        .await
        .unwrap();
        assert!(audit_events.events.iter().any(|event| {
            event.kind == "direct_capability.cancelled"
                && event.payload["executed"] == true
                && event.payload["cancelled"] == true
                && event.payload["timeout_ms"] == 10
        }));
        let _ = fs::remove_file(fake_kubectl);
    }

    #[tokio::test]
    async fn direct_capability_execution_denies_secret_shaped_tekton_reads() {
        let Json(response) = execute_capability(
            State(test_state().await),
            Json(ExecuteCapabilityRequest {
                action: AgentAction::TektonGetPipelineRuns {
                    id: "act_tekton_secret".into(),
                    reason: "read pipeline runs".to_string(),
                    namespace: Some("token-store".to_string()),
                    name: None,
                    all_namespaces: false,
                    label_selector: None,
                },
                timeout_ms: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status, "denied");
        assert_eq!(response.action, "tekton_get_pipeline_runs");
        assert!(!response.executed);
        assert!(response.result.is_none());
    }

    #[tokio::test]
    async fn direct_capability_execution_denies_secret_shaped_tekton_task_reads() {
        let Json(response) = execute_capability(
            State(test_state().await),
            Json(ExecuteCapabilityRequest {
                action: AgentAction::TektonGetTaskRuns {
                    id: "act_tekton_task_secret".into(),
                    reason: "read task runs".to_string(),
                    namespace: Some("token-store".to_string()),
                    name: None,
                    all_namespaces: false,
                    label_selector: None,
                },
                timeout_ms: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status, "denied");
        assert_eq!(response.action, "tekton_get_task_runs");
        assert!(!response.executed);
        assert!(response.result.is_none());
    }

    #[tokio::test]
    async fn direct_capability_execution_denies_secret_shaped_tekton_analysis() {
        let Json(response) = execute_capability(
            State(test_state().await),
            Json(ExecuteCapabilityRequest {
                action: AgentAction::TektonAnalyzePipelineRun {
                    id: "act_tekton_analysis_secret".into(),
                    reason: "analyze pipeline run".to_string(),
                    namespace: "ci".to_string(),
                    name: "token-build".to_string(),
                },
                timeout_ms: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status, "denied");
        assert_eq!(response.action, "tekton_analyze_pipeline_run");
        assert!(!response.executed);
        assert!(response.result.is_none());
    }

    #[tokio::test]
    async fn direct_capability_execution_returns_tool_errors_as_json() {
        let state = test_state().await;
        let Json(response) = execute_capability(
            State(state.clone()),
            Json(ExecuteCapabilityRequest {
                action: AgentAction::PrometheusQuery {
                    id: "act_prom".into(),
                    reason: "query".to_string(),
                    query: "up".to_string(),
                },
                timeout_ms: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status, "tool_error");
        assert_eq!(response.action, "prometheus_query");
        assert!(response.executed);
        assert!(response
            .error
            .as_deref()
            .unwrap()
            .contains("not configured"));
        let Json(audit_events) = list_audit_events(
            State(state),
            Query(ListAuditEventsQuery {
                resource_kind: Some("capability".to_string()),
                resource_id: Some("prometheus_query".to_string()),
                run_id: None,
                limit: Some(50),
            }),
        )
        .await
        .unwrap();
        assert!(audit_events.events.iter().any(|event| {
            event.kind == "direct_capability.failed"
                && event.payload["executed"] == true
                && event.payload["error"]
                    .as_str()
                    .unwrap()
                    .contains("not configured")
        }));
    }

    #[tokio::test]
    async fn direct_capability_execution_accepts_prometheus_inventory() {
        let Json(response) = execute_capability(
            State(test_state().await),
            Json(ExecuteCapabilityRequest {
                action: AgentAction::PrometheusInventory {
                    id: "act_prom_inventory".into(),
                    reason: "inventory".to_string(),
                },
                timeout_ms: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status, "tool_error");
        assert_eq!(response.action, "prometheus_inventory");
        assert!(response.executed);
        assert!(response
            .error
            .as_deref()
            .unwrap()
            .contains("not configured"));
    }

    #[tokio::test]
    async fn direct_capability_execution_accepts_loki_log_summary() {
        let Json(response) = execute_capability(
            State(test_state().await),
            Json(ExecuteCapabilityRequest {
                action: AgentAction::LokiLogSummary {
                    id: "act_loki".into(),
                    reason: "logs".to_string(),
                    query: r#"{namespace="apps-dev"}"#.to_string(),
                    since_seconds: Some(900),
                    limit: Some(25),
                },
                timeout_ms: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status, "tool_error");
        assert_eq!(response.action, "loki_log_summary");
        assert!(response.executed);
        assert!(response
            .error
            .as_deref()
            .unwrap()
            .contains("not configured"));
    }

    #[tokio::test]
    async fn direct_capability_execution_rejects_non_cluster_actions() {
        let error = execute_capability(
            State(test_state().await),
            Json(ExecuteCapabilityRequest {
                action: AgentAction::ListDir {
                    id: "act_list".into(),
                    reason: "list".to_string(),
                    path: ".".into(),
                    depth: 1,
                },
                timeout_ms: None,
            }),
        )
        .await
        .unwrap_err();

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn parses_sse_last_event_ids() {
        assert_eq!(parse_last_event_id("7"), Some(7));
        assert_eq!(
            parse_last_event_id("evt_run_1778887440941720000_12"),
            Some(12)
        );
        assert_eq!(parse_last_event_id("nonsense"), None);
    }

    #[test]
    fn reads_last_event_id_header() {
        let mut headers = HeaderMap::new();
        headers.insert("last-event-id", HeaderValue::from_static("evt_run_test_4"));

        assert_eq!(last_event_seq(&headers), 4);
    }

    #[tokio::test]
    async fn lists_pending_approvals() {
        let state = test_state().await;

        let Json(created) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "write file".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
                policy_mode: None,
                scope: None,
            }),
        )
        .await
        .unwrap();
        state
            .store
            .create_approval(CreateApproval {
                id: "appr_list".to_string(),
                session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
                run_id: created.id.clone(),
                status: "pending".to_string(),
                kind: "file_write".to_string(),
                summary: "write README.md".to_string(),
                risk_level: "medium".to_string(),
                run_scope_json: None,
                action_json: None,
                preview_json: None,
                resume_messages_json: None,
                turns_completed: 1,
            })
            .await
            .unwrap();

        let Json(response) = list_approvals(
            State(state.clone()),
            Query(ListApprovalsQuery {
                status: Some("pending".to_string()),
                namespace: None,
                repo: None,
                branch: None,
                production_impacting: None,
                requested_after_ms: None,
                requested_before_ms: None,
                limit: Some(50),
                offset: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.approvals.len(), 1);
        assert_eq!(response.count, 1);
        assert_eq!(response.limit, 50);
        assert_eq!(response.offset, 0);
        assert_eq!(response.approvals[0].id, "appr_list");

        state
            .store
            .create_approval(CreateApproval {
                id: "appr_scoped".to_string(),
                session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
                run_id: created.id,
                status: "pending".to_string(),
                kind: "file_write".to_string(),
                summary: "write scoped file".to_string(),
                risk_level: "medium".to_string(),
                run_scope_json: Some(serde_json::json!({
                    "namespace": "apps-dev",
                    "repo": "git@example.test/team/pharness.git",
                    "branch": "feature/approval-filter",
                    "production_impacting": false
                })),
                action_json: None,
                preview_json: None,
                resume_messages_json: None,
                turns_completed: 1,
            })
            .await
            .unwrap();
        let Json(scoped) = list_approvals(
            State(state.clone()),
            Query(ListApprovalsQuery {
                status: Some("pending".to_string()),
                namespace: Some("apps-dev".to_string()),
                repo: Some("git@example.test/team/pharness.git".to_string()),
                branch: Some("feature/approval-filter".to_string()),
                production_impacting: Some(false),
                requested_after_ms: Some(0),
                requested_before_ms: None,
                limit: Some(10),
                offset: Some(0),
            }),
        )
        .await
        .unwrap();

        assert_eq!(scoped.approvals.len(), 1);
        assert_eq!(scoped.approvals[0].id, "appr_scoped");

        let Json(summary) = approval_summary(
            State(state),
            Query(ApprovalSummaryQuery {
                status: Some("pending".to_string()),
                namespace: Some("apps-dev".to_string()),
                repo: Some("git@example.test/team/pharness.git".to_string()),
                branch: Some("feature/approval-filter".to_string()),
                production_impacting: Some(false),
                requested_after_ms: Some(0),
                requested_before_ms: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(summary.summary.total, 1);
        assert_eq!(
            summary.summary.by_status[0].value.as_deref(),
            Some("pending")
        );
        assert_eq!(
            summary.summary.by_namespace[0].value.as_deref(),
            Some("apps-dev")
        );
        assert_eq!(
            summary.summary.by_age_bucket[0].value.as_deref(),
            Some("lt_5m")
        );
    }

    #[tokio::test]
    async fn gets_and_denies_approval_by_id() {
        let state = test_state().await;

        let Json(created) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "write file".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
                policy_mode: None,
                scope: None,
            }),
        )
        .await
        .unwrap();
        state
            .store
            .create_approval(CreateApproval {
                id: "appr_by_id".to_string(),
                session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
                run_id: created.id.clone(),
                status: "pending".to_string(),
                kind: "file_write".to_string(),
                summary: "write README.md".to_string(),
                risk_level: "medium".to_string(),
                run_scope_json: Some(serde_json::json!({
                    "namespace": "apps-dev",
                    "repo": "git@example.test/team/app.git",
                    "branch": "feature/pharness",
                    "production_impacting": false
                })),
                action_json: Some(
                    serde_json::to_value(AgentAction::WriteFile {
                        id: "act_write".into(),
                        reason: "test".to_string(),
                        path: "README.md".into(),
                        content: "hello".to_string(),
                    })
                    .unwrap(),
                ),
                preview_json: Some(serde_json::json!({
                    "kind": "file_write",
                    "action": "write_file",
                    "status": "ok",
                    "path": "README.md"
                })),
                resume_messages_json: Some(serde_json::json!([])),
                turns_completed: 1,
            })
            .await
            .unwrap();
        state
            .store
            .mark_run_approval_required(
                &created.id,
                serde_json::json!({
                    "status": "approval_required",
                    "approval_id": "appr_by_id"
                }),
            )
            .await
            .unwrap();

        let Json(fetched) = get_approval(State(state.clone()), Path("appr_by_id".to_string()))
            .await
            .unwrap();
        let Json(decided) = deny_approval(
            State(state.clone()),
            Path("appr_by_id".to_string()),
            Json(ReviewApprovalRequest {
                decided_by: Some("operator".to_string()),
                reason: Some("not aligned".to_string()),
            }),
        )
        .await
        .unwrap();

        assert_eq!(fetched.status, "pending");
        assert_eq!(fetched.preview.as_ref().unwrap()["path"], "README.md");
        assert_eq!(decided.approval.status, "denied");
        assert_eq!(decided.run.status, "failed");
        let Json(audit_events) = list_audit_events(
            State(state),
            Query(ListAuditEventsQuery {
                resource_kind: Some("approval".to_string()),
                resource_id: Some("appr_by_id".to_string()),
                run_id: None,
                limit: Some(50),
            }),
        )
        .await
        .unwrap();
        assert!(audit_events.events.iter().any(|event| {
            event.kind == "approval.denied"
                && event.actor.as_deref() == Some("operator")
                && event.payload["approval_id"] == "appr_by_id"
        }));
    }

    #[tokio::test]
    async fn approval_by_id_refuses_non_current_pending_approval() {
        let state = test_state().await;

        let Json(created) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "write file".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
                policy_mode: None,
                scope: None,
            }),
        )
        .await
        .unwrap();
        for approval_id in ["appr_old", "appr_current"] {
            state
                .store
                .create_approval(CreateApproval {
                    id: approval_id.to_string(),
                    session_id: pharness_core::SessionId::new(format!(
                        "ses_{}",
                        created.id.as_str()
                    )),
                    run_id: created.id.clone(),
                    status: "pending".to_string(),
                    kind: "file_write".to_string(),
                    summary: format!("write from {approval_id}"),
                    risk_level: "medium".to_string(),
                    run_scope_json: None,
                    action_json: Some(
                        serde_json::to_value(AgentAction::WriteFile {
                            id: format!("act_{approval_id}").into(),
                            reason: "test".to_string(),
                            path: "README.md".into(),
                            content: "hello".to_string(),
                        })
                        .unwrap(),
                    ),
                    preview_json: None,
                    resume_messages_json: Some(serde_json::json!([])),
                    turns_completed: 1,
                })
                .await
                .unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }

        let error = deny_approval(
            State(state),
            Path("appr_old".to_string()),
            Json(ReviewApprovalRequest {
                decided_by: Some("operator".to_string()),
                reason: Some("stale".to_string()),
            }),
        )
        .await
        .unwrap_err();

        assert_eq!(error.status, StatusCode::CONFLICT);
        assert!(error.message.contains("current pending approval"));
    }

    #[tokio::test]
    async fn creates_lists_gets_and_revokes_permission_grants() {
        let state = test_state().await;

        let Json(created) = super::create_permission_grant(
            State(state.clone()),
            Json(CreatePermissionGrantRequest {
                subject: "agent:local-worker".to_string(),
                created_by: Some("lucas".to_string()),
                reason: "trusted local write smoke".to_string(),
                scope: serde_json::json!({
                    "environment": "local",
                    "capability_kinds": ["filesystem"]
                }),
                policy: serde_json::json!({
                    "policy_mode": "trusted_writes"
                }),
                expires_at: Some("9999999999999".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(listed) = list_permission_grants(
            State(state.clone()),
            Query(ListPermissionGrantsQuery {
                status: Some("active".to_string()),
                limit: Some(50),
            }),
        )
        .await
        .unwrap();
        let Json(fetched) = get_permission_grant(State(state.clone()), Path(created.id.clone()))
            .await
            .unwrap();
        let Json(revoked) = revoke_permission_grant(
            State(state.clone()),
            Path(created.id.clone()),
            Json(RevokePermissionGrantRequest {
                revoked_by: Some("tester".to_string()),
                reason: Some("done".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(audit_events) = list_audit_events(
            State(state),
            Query(ListAuditEventsQuery {
                resource_kind: Some("permission_grant".to_string()),
                resource_id: Some(created.id.clone()),
                run_id: None,
                limit: Some(50),
            }),
        )
        .await
        .unwrap();

        assert_eq!(created.status, "active");
        assert_eq!(listed.grants.len(), 1);
        assert_eq!(fetched.id, created.id);
        assert_eq!(revoked.status, "revoked");
        assert_eq!(revoked.revoked_by.as_deref(), Some("tester"));
        assert_eq!(audit_events.events.len(), 2);
        assert!(audit_events
            .events
            .iter()
            .any(|event| event.kind == "permission_grant.created"));
        assert!(audit_events
            .events
            .iter()
            .any(|event| event.kind == "permission_grant.created"
                && event.actor.as_deref() == Some("lucas")));
        assert!(audit_events
            .events
            .iter()
            .any(|event| event.kind == "permission_grant.revoked"
                && event.actor.as_deref() == Some("tester")));
    }

    #[test]
    fn rejects_invalid_permission_grant_shape() {
        let error = validate_permission_grant_request(&CreatePermissionGrantRequest {
            subject: "".to_string(),
            created_by: None,
            reason: "test".to_string(),
            scope: serde_json::json!({}),
            policy: serde_json::json!({}),
            expires_at: None,
        })
        .unwrap_err();

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn rejects_permission_grant_without_environment_scope() {
        let error = validate_permission_grant_request(&CreatePermissionGrantRequest {
            subject: "agent:local-worker".to_string(),
            created_by: None,
            reason: "test".to_string(),
            scope: serde_json::json!({
                "capability_kinds": ["filesystem"],
            }),
            policy: serde_json::json!({
                "policy_mode": "trusted_writes"
            }),
            expires_at: None,
        })
        .unwrap_err();

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert!(error.message.contains("scope.environment"));
    }

    #[tokio::test]
    async fn returns_run_diff() {
        let state = test_state().await;

        let Json(created) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "write file".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
                policy_mode: None,
                scope: None,
            }),
        )
        .await
        .unwrap();
        state
            .store
            .create_file_change(CreateFileChange {
                id: "chg_test".to_string(),
                session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
                run_id: created.id.clone(),
                path: "README.md".to_string(),
                before_hash: None,
                after_hash: None,
                diff: "--- before\n+++ after".to_string(),
            })
            .await
            .unwrap();

        let Json(response) = get_run_diff(State(state), Path(created.id.to_string()))
            .await
            .unwrap();

        assert_eq!(response.changes.len(), 1);
        assert!(response.diff.contains("+++ after"));
    }

    #[tokio::test]
    async fn returns_run_artifacts_and_single_artifact() {
        let state = test_state().await;

        let Json(created) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "observe".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
                policy_mode: None,
                scope: None,
            }),
        )
        .await
        .unwrap();
        state
            .store
            .create_artifact(CreateArtifact {
                id: "art_test".to_string(),
                session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
                run_id: Some(created.id.clone()),
                kind: "tool_result".to_string(),
                label: "Prometheus query".to_string(),
                mime_type: Some("application/json".to_string()),
                path: None,
                content_text: None,
                content_json: Some(serde_json::json!({"result_count": 33})),
            })
            .await
            .unwrap();

        let Json(listed) = list_run_artifacts(State(state.clone()), Path(created.id.to_string()))
            .await
            .unwrap();
        let Json(fetched) = get_artifact(State(state), Path("art_test".to_string()))
            .await
            .unwrap();

        assert_eq!(listed.artifacts.len(), 1);
        assert_eq!(listed.artifacts[0].id, "art_test");
        assert_eq!(fetched.content_json.unwrap()["result_count"], 33);
    }

    #[tokio::test]
    async fn returns_run_observations_and_single_observation() {
        let state = test_state().await;

        let Json(created) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "observe".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
                policy_mode: None,
                scope: None,
            }),
        )
        .await
        .unwrap();
        state
            .store
            .create_observation(CreateObservation {
                id: "obs_test".to_string(),
                session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
                run_id: Some(created.id.clone()),
                source: "prometheus".to_string(),
                kind: "query".to_string(),
                subject: "up".to_string(),
                summary: "read Prometheus instant query".to_string(),
                resource_namespace: None,
                resource_kind: Some("query".to_string()),
                resource_name: Some("up".to_string()),
                resource_ref_json: Some(serde_json::json!({
                    "provider": "prometheus",
                    "kind": "query",
                    "name": "up"
                })),
                artifact_id: None,
                data_json: serde_json::json!({"result_count": 33}),
            })
            .await
            .unwrap();

        let Json(listed) =
            list_run_observations(State(state.clone()), Path(created.id.to_string()))
                .await
                .unwrap();
        let Json(filtered) = list_observations(
            State(state.clone()),
            Query(ListObservationsQuery {
                run_id: Some(created.id.to_string()),
                source: Some("prometheus".to_string()),
                kind: Some("query".to_string()),
                subject: Some("up".to_string()),
                resource_namespace: None,
                resource_kind: Some("query".to_string()),
                resource_name: Some("up".to_string()),
                observed_after_ms: Some(0),
                observed_before_ms: None,
                limit: Some(10),
                offset: Some(0),
            }),
        )
        .await
        .unwrap();
        let Json(fetched) = get_observation(State(state), Path("obs_test".to_string()))
            .await
            .unwrap();

        assert_eq!(listed.observations.len(), 1);
        assert_eq!(listed.count, 1);
        assert_eq!(listed.observations[0].id, "obs_test");
        assert_eq!(
            listed.observations[0].resource_kind.as_deref(),
            Some("query")
        );
        assert_eq!(listed.observations[0].resource_name.as_deref(), Some("up"));
        assert_eq!(filtered.observations.len(), 1);
        assert_eq!(filtered.count, 1);
        assert_eq!(filtered.limit, Some(10));
        assert_eq!(filtered.offset, Some(0));
        assert_eq!(filtered.observations[0].id, "obs_test");
        assert_eq!(fetched.data_json["result_count"], 33);
    }

    #[tokio::test]
    async fn returns_filtered_incidents_and_single_incident() {
        let state = test_state().await;

        let Json(created) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "observe incident".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
                policy_mode: None,
                scope: None,
            }),
        )
        .await
        .unwrap();
        state
            .store
            .create_observation(CreateObservation {
                id: "obs_incident".to_string(),
                session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
                run_id: Some(created.id.clone()),
                source: "tekton".to_string(),
                kind: "pipeline_run_analysis".to_string(),
                subject: "build-app".to_string(),
                summary: "analyzed Tekton PipelineRun ci/build-app".to_string(),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                resource_ref_json: None,
                artifact_id: None,
                data_json: serde_json::json!({"status":"failed"}),
            })
            .await
            .unwrap();
        state
            .store
            .create_incident(CreateIncident {
                id: "inc_test".to_string(),
                observation_id: "obs_incident".to_string(),
                session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
                run_id: Some(created.id.clone()),
                status: "candidate".to_string(),
                severity: "high".to_string(),
                title: "Tekton PipelineRun issue: ci/build-app".to_string(),
                summary: "PipelineRun status is failed".to_string(),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                data_json: serde_json::json!({"reasons":["PipelineRun status is failed"]}),
            })
            .await
            .unwrap();
        state
            .store
            .create_remediation_plan(CreateRemediationPlan {
                id: "rplan_test".to_string(),
                incident_id: "inc_test".to_string(),
                session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
                run_id: Some(created.id.clone()),
                status: "draft".to_string(),
                title: "Draft remediation for ci/build-app".to_string(),
                summary: "Review Tekton evidence before proposing a mutation".to_string(),
                risk_level: "high".to_string(),
                requires_approval: true,
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                plan_json: serde_json::json!({
                    "mode": "read_only_draft",
                    "approval_gates": ["pipeline_mutation", "cluster_mutation"],
                }),
            })
            .await
            .unwrap();
        state
            .store
            .create_approval_gate(CreateApprovalGate {
                id: "agate_test".to_string(),
                remediation_plan_id: "rplan_test".to_string(),
                incident_id: "inc_test".to_string(),
                session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
                run_id: Some(created.id.clone()),
                status: "pending".to_string(),
                gate_kind: "pipeline_mutation".to_string(),
                gate_order: 1,
                title: "Approve pipeline mutation".to_string(),
                summary: "Require approval before rerunning Tekton resources".to_string(),
                risk_level: "high".to_string(),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                gate_json: serde_json::json!({
                    "required_before": "rerunning PipelineRun",
                }),
            })
            .await
            .unwrap();

        let Json(listed) = list_incidents(
            State(state.clone()),
            Query(ListIncidentsQuery {
                run_id: Some(created.id.to_string()),
                status: Some("candidate".to_string()),
                severity: Some("high".to_string()),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                created_after_ms: Some(0),
                created_before_ms: None,
                limit: Some(10),
                offset: Some(0),
            }),
        )
        .await
        .unwrap();
        let Json(fetched) = get_incident(State(state.clone()), Path("inc_test".to_string()))
            .await
            .unwrap();
        let Json(listed_plans) = list_remediation_plans(
            State(state.clone()),
            Query(ListRemediationPlansQuery {
                incident_id: Some("inc_test".to_string()),
                run_id: Some(created.id.to_string()),
                status: Some("draft".to_string()),
                risk_level: Some("high".to_string()),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                created_after_ms: Some(0),
                created_before_ms: None,
                limit: Some(10),
                offset: Some(0),
            }),
        )
        .await
        .unwrap();
        let Json(fetched_plan) =
            get_remediation_plan(State(state.clone()), Path("rplan_test".to_string()))
                .await
                .unwrap();
        let Json(created_work_plan) = create_work_plan_from_remediation_plan(
            State(state.clone()),
            Json(CreateWorkPlanFromRemediationPlanRequest {
                remediation_plan_id: "rplan_test".to_string(),
            }),
        )
        .await
        .unwrap();
        let Json(existing_work_plan) = create_work_plan_from_remediation_plan(
            State(state.clone()),
            Json(CreateWorkPlanFromRemediationPlanRequest {
                remediation_plan_id: "rplan_test".to_string(),
            }),
        )
        .await
        .unwrap();
        let Json(listed_work_plans) = list_work_plans(
            State(state.clone()),
            Query(ListWorkPlansQuery {
                remediation_plan_id: Some("rplan_test".to_string()),
                incident_id: Some("inc_test".to_string()),
                run_id: Some(created.id.to_string()),
                status: Some("draft".to_string()),
                risk_level: Some("high".to_string()),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                created_after_ms: Some(0),
                created_before_ms: None,
                limit: Some(10),
                offset: Some(0),
            }),
        )
        .await
        .unwrap();
        let Json(fetched_work_plan) = get_work_plan(
            State(state.clone()),
            Path(created_work_plan.work_plan.id.clone()),
        )
        .await
        .unwrap();
        let Json(listed_gates) = list_approval_gates(
            State(state.clone()),
            Query(ListApprovalGatesQuery {
                remediation_plan_id: Some("rplan_test".to_string()),
                incident_id: Some("inc_test".to_string()),
                run_id: Some(created.id.to_string()),
                status: Some("pending".to_string()),
                gate_kind: Some("pipeline_mutation".to_string()),
                risk_level: Some("high".to_string()),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                created_after_ms: Some(0),
                created_before_ms: None,
                limit: Some(10),
                offset: Some(0),
            }),
        )
        .await
        .unwrap();
        let Json(fetched_gate) =
            get_approval_gate(State(state.clone()), Path("agate_test".to_string()))
                .await
                .unwrap();
        let Json(gate_summary) = approval_gate_summary(
            State(state.clone()),
            Query(ApprovalGateSummaryQuery {
                remediation_plan_id: Some("rplan_test".to_string()),
                incident_id: Some("inc_test".to_string()),
                run_id: Some(created.id.to_string()),
                status: Some("pending".to_string()),
                gate_kind: Some("pipeline_mutation".to_string()),
                risk_level: Some("high".to_string()),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                created_after_ms: Some(0),
                created_before_ms: None,
            }),
        )
        .await
        .unwrap();
        let Json(decided_gate) = satisfy_approval_gate(
            State(state.clone()),
            Path("agate_test".to_string()),
            Json(DecideApprovalGateRequest {
                decided_by: Some("lucas".to_string()),
                reason: Some("reviewed remediation smoke".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(gate_audit_events) = list_audit_events(
            State(state),
            Query(ListAuditEventsQuery {
                resource_kind: Some("approval_gate".to_string()),
                resource_id: Some("agate_test".to_string()),
                run_id: None,
                limit: Some(50),
            }),
        )
        .await
        .unwrap();

        assert_eq!(listed.count, 1);
        assert_eq!(listed.limit, 10);
        assert_eq!(listed.offset, 0);
        assert_eq!(listed.incidents[0].id, "inc_test");
        assert_eq!(fetched.observation_id, "obs_incident");
        assert_eq!(fetched.severity, "high");
        assert_eq!(listed_plans.count, 1);
        assert_eq!(listed_plans.limit, 10);
        assert_eq!(listed_plans.offset, 0);
        assert_eq!(listed_plans.remediation_plans[0].id, "rplan_test");
        assert_eq!(fetched_plan.incident_id, "inc_test");
        assert!(fetched_plan.requires_approval);
        assert_eq!(fetched_plan.plan_json["mode"], "read_only_draft");
        assert!(created_work_plan.created);
        assert!(!existing_work_plan.created);
        assert_eq!(
            created_work_plan.work_plan.remediation_plan_id,
            "rplan_test"
        );
        assert_eq!(
            existing_work_plan.work_plan.id,
            created_work_plan.work_plan.id
        );
        assert_eq!(listed_work_plans.count, 1);
        assert_eq!(
            listed_work_plans.work_plans[0].id,
            created_work_plan.work_plan.id
        );
        assert_eq!(fetched_work_plan.incident_id, "inc_test");
        assert!(!fetched_work_plan.work_plan_json["execution"]["enabled"]
            .as_bool()
            .unwrap());
        assert_eq!(listed_gates.count, 1);
        assert_eq!(listed_gates.limit, 10);
        assert_eq!(listed_gates.offset, 0);
        assert_eq!(listed_gates.approval_gates[0].id, "agate_test");
        assert_eq!(fetched_gate.remediation_plan_id, "rplan_test");
        assert_eq!(fetched_gate.gate_kind, "pipeline_mutation");
        assert_eq!(gate_summary.summary.total, 1);
        assert_eq!(
            gate_summary.summary.by_status[0].value.as_deref(),
            Some("pending")
        );
        assert_eq!(
            gate_summary.summary.by_gate_kind[0].value.as_deref(),
            Some("pipeline_mutation")
        );
        assert_eq!(
            gate_summary.summary.by_resource_namespace[0]
                .value
                .as_deref(),
            Some("ci")
        );
        assert_eq!(decided_gate.approval_gate.status, "satisfied");
        assert_eq!(
            decided_gate.approval_gate.decided_by.as_deref(),
            Some("lucas")
        );
        assert!(gate_audit_events
            .events
            .iter()
            .any(|event| event.kind == "approval_gate.satisfied"));
    }

    #[tokio::test]
    async fn transitions_and_revisions_stale_work_plan_gates() {
        let state = test_state().await;

        let Json(created) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "plan lifecycle".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
                policy_mode: None,
                scope: None,
            }),
        )
        .await
        .unwrap();
        let session_id = pharness_core::SessionId::new(format!("ses_{}", created.id.as_str()));
        state
            .store
            .create_observation(CreateObservation {
                id: "obs_plan_lifecycle".to_string(),
                session_id: session_id.clone(),
                run_id: Some(created.id.clone()),
                source: "tekton".to_string(),
                kind: "pipeline_run_analysis".to_string(),
                subject: "build-api".to_string(),
                summary: "PipelineRun needs operator review".to_string(),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-api".to_string()),
                resource_ref_json: None,
                artifact_id: None,
                data_json: serde_json::json!({"status":"failed"}),
            })
            .await
            .unwrap();
        state
            .store
            .create_incident(CreateIncident {
                id: "inc_plan_lifecycle".to_string(),
                observation_id: "obs_plan_lifecycle".to_string(),
                session_id: session_id.clone(),
                run_id: Some(created.id.clone()),
                status: "candidate".to_string(),
                severity: "high".to_string(),
                title: "Tekton PipelineRun issue: ci/build-api".to_string(),
                summary: "PipelineRun status is failed".to_string(),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-api".to_string()),
                data_json: serde_json::json!({}),
            })
            .await
            .unwrap();
        state
            .store
            .create_remediation_plan(CreateRemediationPlan {
                id: "rplan_lifecycle".to_string(),
                incident_id: "inc_plan_lifecycle".to_string(),
                session_id: session_id.clone(),
                run_id: Some(created.id.clone()),
                status: "draft".to_string(),
                title: "Draft remediation for ci/build-api".to_string(),
                summary: "Review evidence before proposing mutation".to_string(),
                risk_level: "high".to_string(),
                requires_approval: true,
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-api".to_string()),
                plan_json: serde_json::json!({
                    "steps": [{"id": "inspect"}],
                    "approval_gates": ["pipeline_mutation"],
                }),
            })
            .await
            .unwrap();
        state
            .store
            .create_approval_gate(CreateApprovalGate {
                id: "agate_lifecycle".to_string(),
                remediation_plan_id: "rplan_lifecycle".to_string(),
                incident_id: "inc_plan_lifecycle".to_string(),
                session_id,
                run_id: Some(created.id.clone()),
                status: "pending".to_string(),
                gate_kind: "pipeline_mutation".to_string(),
                gate_order: 1,
                title: "Approve pipeline mutation".to_string(),
                summary: "Require approval before changing pipeline state".to_string(),
                risk_level: "high".to_string(),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-api".to_string()),
                gate_json: serde_json::json!({}),
            })
            .await
            .unwrap();

        let Json(created_work_plan) = create_work_plan_from_remediation_plan(
            State(state.clone()),
            Json(CreateWorkPlanFromRemediationPlanRequest {
                remediation_plan_id: "rplan_lifecycle".to_string(),
            }),
        )
        .await
        .unwrap();
        let work_plan_id = created_work_plan.work_plan.id.clone();
        let Json(proposed) = transition_work_plan(
            State(state.clone()),
            Path(work_plan_id.clone()),
            Json(TransitionWorkPlanRequest {
                target_status: "proposed".to_string(),
                actor: Some("lucas".to_string()),
                reason: Some("ready for review".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(approved) = transition_work_plan(
            State(state.clone()),
            Path(work_plan_id.clone()),
            Json(TransitionWorkPlanRequest {
                target_status: "approved".to_string(),
                actor: Some("lucas".to_string()),
                reason: Some("bounded plan approved".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(work_plan_envelope) = create_work_plan_trusted_envelope(
            State(state.clone()),
            Path(work_plan_id.clone()),
            Json(CreateTrustedEnvelopeRequest {
                subject: None,
                created_by: Some("lucas".to_string()),
                reason: "bounded WorkPlan approved".to_string(),
                environment: Some("local".to_string()),
                namespace: Some("apps-dev".to_string()),
                repo: Some("git@example.test/team/app.git".to_string()),
                branch: Some("feature/pharness".to_string()),
                production_impacting: Some(false),
                expires_at: None,
            }),
        )
        .await
        .unwrap();
        let Json(satisfied_gate) = satisfy_approval_gate(
            State(state.clone()),
            Path("agate_lifecycle".to_string()),
            Json(DecideApprovalGateRequest {
                decided_by: Some("lucas".to_string()),
                reason: Some("pipeline mutation reviewed".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(revised) = revise_work_plan(
            State(state.clone()),
            Path(work_plan_id.clone()),
            Json(ReviseWorkPlanRequest {
                title: None,
                summary: Some("Revised after new evidence".to_string()),
                risk_level: None,
                requires_approval: None,
                work_plan_json: serde_json::json!({
                    "steps": [{"id": "inspect"}, {"id": "prepare_changeset"}],
                }),
                actor: Some("lucas".to_string()),
                reason: Some("new evidence changed execution plan".to_string()),
                material_change: true,
            }),
        )
        .await
        .unwrap();
        let Json(work_plan_audit_events) = list_audit_events(
            State(state.clone()),
            Query(ListAuditEventsQuery {
                resource_kind: Some("work_plan".to_string()),
                resource_id: Some(work_plan_id),
                run_id: None,
                limit: Some(50),
            }),
        )
        .await
        .unwrap();
        let Json(gate_audit_events) = list_audit_events(
            State(state),
            Query(ListAuditEventsQuery {
                resource_kind: Some("approval_gate".to_string()),
                resource_id: Some("agate_lifecycle".to_string()),
                run_id: None,
                limit: Some(50),
            }),
        )
        .await
        .unwrap();

        assert_eq!(proposed.work_plan.status, "proposed");
        assert_eq!(approved.work_plan.status, "approved");
        assert_eq!(
            work_plan_envelope.grant.scope["work_plan_ids"][0],
            serde_json::json!(approved.work_plan.id.clone())
        );
        assert!(work_plan_envelope.grant.scope["change_set_ids"].is_null());
        assert_eq!(satisfied_gate.approval_gate.status, "satisfied");
        assert_eq!(revised.work_plan.status, "draft");
        assert_eq!(revised.work_plan.revision, 2);
        assert_eq!(revised.invalidated_gates.len(), 1);
        assert_eq!(revised.invalidated_gates[0].status, "stale");
        assert_eq!(
            revised.invalidated_gates[0].stale_reason.as_deref(),
            Some("new evidence changed execution plan")
        );
        assert!(work_plan_audit_events
            .events
            .iter()
            .any(|event| event.kind == "work_plan.revised"));
        assert!(work_plan_audit_events
            .events
            .iter()
            .any(|event| event.kind == "work_plan.trusted_envelope_created"));
        assert!(gate_audit_events
            .events
            .iter()
            .any(|event| event.kind == "approval_gate.stale"));
    }

    #[tokio::test]
    async fn creates_transitions_and_revisions_stale_change_set_gates() {
        let state = test_state().await;

        let Json(created) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "change set lifecycle".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
                policy_mode: None,
                scope: None,
            }),
        )
        .await
        .unwrap();
        let session_id = pharness_core::SessionId::new(format!("ses_{}", created.id.as_str()));
        state
            .store
            .create_observation(CreateObservation {
                id: "obs_changeset_lifecycle".to_string(),
                session_id: session_id.clone(),
                run_id: Some(created.id.clone()),
                source: "tekton".to_string(),
                kind: "pipeline_run_analysis".to_string(),
                subject: "build-api".to_string(),
                summary: "PipelineRun needs code change review".to_string(),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-api".to_string()),
                resource_ref_json: None,
                artifact_id: None,
                data_json: serde_json::json!({"status":"failed"}),
            })
            .await
            .unwrap();
        state
            .store
            .create_incident(CreateIncident {
                id: "inc_changeset_lifecycle".to_string(),
                observation_id: "obs_changeset_lifecycle".to_string(),
                session_id: session_id.clone(),
                run_id: Some(created.id.clone()),
                status: "candidate".to_string(),
                severity: "high".to_string(),
                title: "Tekton PipelineRun issue: ci/build-api".to_string(),
                summary: "PipelineRun status is failed".to_string(),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-api".to_string()),
                data_json: serde_json::json!({}),
            })
            .await
            .unwrap();
        state
            .store
            .create_remediation_plan(CreateRemediationPlan {
                id: "rplan_changeset".to_string(),
                incident_id: "inc_changeset_lifecycle".to_string(),
                session_id: session_id.clone(),
                run_id: Some(created.id.clone()),
                status: "draft".to_string(),
                title: "Draft remediation for ci/build-api".to_string(),
                summary: "Prepare a bounded source change".to_string(),
                risk_level: "high".to_string(),
                requires_approval: true,
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-api".to_string()),
                plan_json: serde_json::json!({
                    "steps": [{"id": "prepare_changeset"}],
                    "approval_gates": ["source_change"],
                }),
            })
            .await
            .unwrap();
        state
            .store
            .create_approval_gate(CreateApprovalGate {
                id: "agate_changeset".to_string(),
                remediation_plan_id: "rplan_changeset".to_string(),
                incident_id: "inc_changeset_lifecycle".to_string(),
                session_id,
                run_id: Some(created.id.clone()),
                status: "pending".to_string(),
                gate_kind: "source_change".to_string(),
                gate_order: 1,
                title: "Approve source change".to_string(),
                summary: "Require approval before applying proposed source changes".to_string(),
                risk_level: "high".to_string(),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-api".to_string()),
                gate_json: serde_json::json!({}),
            })
            .await
            .unwrap();

        let Json(created_work_plan) = create_work_plan_from_remediation_plan(
            State(state.clone()),
            Json(CreateWorkPlanFromRemediationPlanRequest {
                remediation_plan_id: "rplan_changeset".to_string(),
            }),
        )
        .await
        .unwrap();
        let Json(created_change_set) = create_change_set(
            State(state.clone()),
            Json(CreateChangeSetRequest {
                work_plan_id: created_work_plan.work_plan.id.clone(),
                title: Some("ChangeSet: fix build config".to_string()),
                summary: Some("Update build config for checkout-api".to_string()),
                risk_level: Some("medium".to_string()),
                change_set_json: serde_json::json!({
                    "changes": [{
                        "path": "build/checkout-api.yaml",
                        "diff": "--- before\n+++ after\n-retries: 1\n+retries: 2",
                    }],
                    "rollback": "restore previous build config",
                }),
                actor: Some("lucas".to_string()),
                reason: Some("prepare bounded source change".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(existing_change_set) = create_change_set(
            State(state.clone()),
            Json(CreateChangeSetRequest {
                work_plan_id: created_work_plan.work_plan.id.clone(),
                title: None,
                summary: None,
                risk_level: None,
                change_set_json: serde_json::json!({"changes":[]}),
                actor: None,
                reason: None,
            }),
        )
        .await
        .unwrap();
        let change_set_id = created_change_set.change_set.id.clone();
        let original_hash = created_change_set.change_set.material_hash.clone();
        let Json(listed_change_sets) = list_change_sets(
            State(state.clone()),
            Query(ListChangeSetsQuery {
                work_plan_id: Some(created_work_plan.work_plan.id.clone()),
                remediation_plan_id: Some("rplan_changeset".to_string()),
                incident_id: Some("inc_changeset_lifecycle".to_string()),
                run_id: Some(created.id.to_string()),
                status: Some("draft".to_string()),
                risk_level: Some("medium".to_string()),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-api".to_string()),
                created_after_ms: Some(0),
                created_before_ms: None,
                limit: Some(10),
                offset: Some(0),
            }),
        )
        .await
        .unwrap();
        let Json(proposed) = transition_change_set(
            State(state.clone()),
            Path(change_set_id.clone()),
            Json(TransitionChangeSetRequest {
                target_status: "proposed".to_string(),
                actor: Some("lucas".to_string()),
                reason: Some("ready for source review".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(approved) = transition_change_set(
            State(state.clone()),
            Path(change_set_id.clone()),
            Json(TransitionChangeSetRequest {
                target_status: "approved".to_string(),
                actor: Some("lucas".to_string()),
                reason: Some("source change approved".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(change_set_envelope) = create_change_set_trusted_envelope(
            State(state.clone()),
            Path(change_set_id.clone()),
            Json(CreateTrustedEnvelopeRequest {
                subject: None,
                created_by: Some("lucas".to_string()),
                reason: "bounded ChangeSet approved".to_string(),
                environment: Some("local".to_string()),
                namespace: Some("apps-dev".to_string()),
                repo: Some("git@example.test/team/app.git".to_string()),
                branch: Some("feature/pharness".to_string()),
                production_impacting: Some(false),
                expires_at: None,
            }),
        )
        .await
        .unwrap();
        let Json(satisfied_gate) = satisfy_approval_gate(
            State(state.clone()),
            Path("agate_changeset".to_string()),
            Json(DecideApprovalGateRequest {
                decided_by: Some("lucas".to_string()),
                reason: Some("source change reviewed".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(revised) = revise_change_set(
            State(state.clone()),
            Path(change_set_id.clone()),
            Json(ReviseChangeSetRequest {
                title: None,
                summary: Some("Update build config and timeout".to_string()),
                risk_level: None,
                change_set_json: serde_json::json!({
                    "changes": [{
                        "path": "build/checkout-api.yaml",
                        "diff": "--- before\n+++ after\n-retries: 1\n+retries: 2\n-timeout: 60\n+timeout: 90",
                    }],
                    "rollback": "restore previous build config",
                }),
                actor: Some("lucas".to_string()),
                reason: Some("source change payload changed".to_string()),
                material_change: true,
            }),
        )
        .await
        .unwrap();
        let Json(revised_work_plan) = revise_work_plan(
            State(state.clone()),
            Path(created_work_plan.work_plan.id.clone()),
            Json(ReviseWorkPlanRequest {
                title: None,
                summary: Some("Plan changed after source review".to_string()),
                risk_level: None,
                requires_approval: None,
                work_plan_json: serde_json::json!({
                    "steps": [{"id": "prepare_changeset"}, {"id": "rerun_tests"}],
                }),
                actor: Some("lucas".to_string()),
                reason: Some("plan changed after source review".to_string()),
                material_change: true,
            }),
        )
        .await
        .unwrap();
        let Json(change_set_audit_events) = list_audit_events(
            State(state.clone()),
            Query(ListAuditEventsQuery {
                resource_kind: Some("change_set".to_string()),
                resource_id: Some(change_set_id.clone()),
                run_id: None,
                limit: Some(50),
            }),
        )
        .await
        .unwrap();
        let Json(gate_audit_events) = list_audit_events(
            State(state),
            Query(ListAuditEventsQuery {
                resource_kind: Some("approval_gate".to_string()),
                resource_id: Some("agate_changeset".to_string()),
                run_id: None,
                limit: Some(50),
            }),
        )
        .await
        .unwrap();

        assert!(created_change_set.created);
        assert!(!existing_change_set.created);
        assert_eq!(listed_change_sets.count, 1);
        assert_eq!(listed_change_sets.change_sets[0].revision, 1);
        assert_eq!(proposed.change_set.status, "proposed");
        assert_eq!(approved.change_set.status, "approved");
        assert_eq!(
            change_set_envelope.grant.scope["work_plan_ids"][0],
            serde_json::json!(created_work_plan.work_plan.id.clone())
        );
        assert_eq!(
            change_set_envelope.grant.scope["change_set_ids"][0],
            serde_json::json!(change_set_id.clone())
        );
        assert_eq!(satisfied_gate.approval_gate.status, "satisfied");
        assert_eq!(revised.change_set.status, "draft");
        assert_eq!(revised.change_set.revision, 2);
        assert!(revised.material_hash_changed);
        assert_ne!(revised.change_set.material_hash, original_hash);
        assert_eq!(revised.invalidated_gates.len(), 1);
        assert_eq!(revised.invalidated_gates[0].status, "stale");
        assert_eq!(
            revised.invalidated_gates[0].stale_reason.as_deref(),
            Some("source change payload changed")
        );
        let invalidated_change_set = revised_work_plan.invalidated_change_set.unwrap();
        assert_eq!(invalidated_change_set.id, change_set_id);
        assert_eq!(invalidated_change_set.status, "stale");
        assert!(change_set_audit_events
            .events
            .iter()
            .any(|event| event.kind == "change_set.revised"));
        assert!(change_set_audit_events
            .events
            .iter()
            .any(|event| event.kind == "change_set.trusted_envelope_created"));
        assert!(gate_audit_events
            .events
            .iter()
            .any(|event| event.kind == "approval_gate.stale"));
    }

    #[tokio::test]
    async fn denial_decides_pending_approval_and_blocks_run() {
        let state = test_state().await;

        let Json(created) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "write file".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
                policy_mode: None,
                scope: None,
            }),
        )
        .await
        .unwrap();

        state
            .store
            .create_approval(CreateApproval {
                id: "appr_test".to_string(),
                session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
                run_id: created.id.clone(),
                status: "pending".to_string(),
                kind: "file_write".to_string(),
                summary: "write README.md".to_string(),
                risk_level: "medium".to_string(),
                run_scope_json: Some(serde_json::json!({
                    "namespace": "apps-dev",
                    "repo": "git@example.test/team/app.git",
                    "branch": "feature/pharness",
                    "production_impacting": false
                })),
                action_json: Some(
                    serde_json::to_value(AgentAction::WriteFile {
                        id: "act_write".into(),
                        reason: "test".to_string(),
                        path: "README.md".into(),
                        content: "hello".to_string(),
                    })
                    .unwrap(),
                ),
                preview_json: None,
                resume_messages_json: Some(serde_json::json!([])),
                turns_completed: 1,
            })
            .await
            .unwrap();
        state
            .store
            .mark_run_approval_required(
                &created.id,
                serde_json::json!({
                    "status": "approval_required",
                    "approval_id": "appr_test"
                }),
            )
            .await
            .unwrap();

        let Json(response) = decide_run_approval(
            State(state.clone()),
            Path(created.id.to_string()),
            Json(DecideApprovalRequest {
                decision: ApprovalDecision::Deny,
                decided_by: Some("test".to_string()),
                reason: Some("not now".to_string()),
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.approval.status, "denied");
        assert_eq!(
            response
                .approval
                .scope
                .as_ref()
                .unwrap()
                .namespace
                .as_deref(),
            Some("apps-dev")
        );
        assert_eq!(response.run.status, "failed");
        let events = state.store.list_events(&created.id).await.unwrap();
        assert!(events.iter().any(|event| {
            event.kind == pharness_core::EventKind::ApprovalDecided
                && event.payload["run_scope"]["namespace"] == "apps-dev"
        }));
        let Json(audit_events) = list_audit_events(
            State(state),
            Query(ListAuditEventsQuery {
                resource_kind: Some("approval".to_string()),
                resource_id: Some("appr_test".to_string()),
                run_id: None,
                limit: Some(50),
            }),
        )
        .await
        .unwrap();
        assert!(audit_events.events.iter().any(|event| {
            event.kind == "approval.denied"
                && event.actor.as_deref() == Some("test")
                && event.payload["run_scope"]["namespace"] == "apps-dev"
                && event.payload["action"] == "write_file"
        }));
    }
}
