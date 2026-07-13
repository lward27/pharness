use crate::dispatch::{RunDispatcher, TektonExecutionRequest};
use crate::dto::{
    ApprovalDecision, ApprovalGateResponse, ApprovalGateSummaryResponse, ApprovalGatesResponse,
    ApprovalSummaryResponse, ApprovalsResponse, ArtifactResponse, ArtifactsResponse,
    AttachDeploymentIntentEvidenceRequest, AttachDeploymentIntentEvidenceResponse,
    AttachPipelineIntentEvidenceRequest, AttachPipelineIntentEvidenceResponse,
    AttachReleaseEvidenceRequest, AttachReleaseEvidenceResponse, AuditEventsResponse,
    ChangeSetResponse, ChangeSetsResponse, CreateChangeSetRequest, CreateChangeSetResponse,
    CreateDeploymentIntentFromPipelineIntentRequest, CreateDeploymentIntentResponse,
    CreateIncidentRequest, CreateObservationRequest, CreatePermissionGrantRequest,
    CreatePipelineContractRequest, CreatePipelineIntentFromChangeSetRequest,
    CreatePipelineIntentResponse, CreatePipelineIntentTrustedEnvelopeRequest,
    CreateRegistryEvidenceFromInspectionRequest, CreateRegistryEvidenceFromInspectionResponse,
    CreateRegistryEvidenceFromReleaseRequest, CreateRegistryEvidenceResponse,
    CreateReleaseFromDeploymentIntentRequest, CreateReleaseResponse, CreateRemediationPlanRequest,
    CreateRunRequest, CreateTrustedEnvelopeRequest, CreateWorkPlanFromRemediationPlanRequest,
    CreateWorkPlanResponse, DecideApprovalGateRequest, DecideApprovalGateResponse,
    DecideApprovalRequest, DecideApprovalResponse, DeploymentIntentResponse,
    DeploymentIntentsResponse, EventsResponse, ExecuteCapabilityRequest, ExecuteCapabilityResponse,
    ExecutePipelineIntentRequest, ExecutePipelineIntentResponse, FileChangeResponse,
    IncidentResponse, IncidentsResponse, ObservationResponse, ObservationsResponse,
    PermissionGrantResponse, PermissionGrantsResponse, PipelineContractResponse,
    PipelineContractsResponse, PipelineIntentExecutionOutcomeRequest, PipelineIntentResponse,
    PipelineIntentsResponse, RegistryEvidenceListResponse, RegistryEvidenceResponse,
    ReleaseResponse, ReleasesResponse, RemediationPlanResponse, RemediationPlansResponse,
    ReplacePipelineContractRequest, ReplacePipelineContractResponse, ReviewApprovalRequest,
    ReviseChangeSetRequest, ReviseChangeSetResponse, ReviseWorkPlanRequest, ReviseWorkPlanResponse,
    RevokePermissionGrantRequest, RunDiffResponse, RunResponse, RunSummaryResponse, RunsResponse,
    SdlcFlowResponse, SdlcReadinessFinding, SdlcReadinessGateSummary, SdlcReadinessGrantSummary,
    SdlcReadinessResponse, TransitionChangeSetRequest, TransitionChangeSetResponse,
    TransitionDeploymentIntentRequest, TransitionDeploymentIntentResponse,
    TransitionPipelineContractRequest, TransitionPipelineIntentRequest,
    TransitionPipelineIntentResponse, TransitionRegistryEvidenceRequest,
    TransitionRegistryEvidenceResponse, TransitionReleaseRequest, TransitionReleaseResponse,
    TransitionWorkPlanRequest, TransitionWorkPlanResponse, TrustedEnvelopeResponse,
    WorkPlanResponse, WorkPlansResponse,
};
use crate::worker::{attempt_spec_for_run, finish_run_from_attempt, ingest_agent_event};
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use futures::stream::{self, Stream};
use pharness_core::{
    AgentAction, AgentEvent, CapabilityKind, EventId, EventKind, PermissionGrant,
    PermissionGrantPolicy, PermissionGrantScope, PolicyDecision, PolicyMode, ReadOnlyClusterTools,
    RiskLevel, RunId, SafetyPolicy, SessionId, ToolExecutor, ToolResult,
};
use pharness_runhost::AttemptOutcome;
use pharness_store::{
    ApprovalGateListFilter, ApprovalGateSummaryFilter, ApprovalListFilter, ApprovalSummaryFilter,
    AuditEventListFilter, ChangeSetListFilter, DeploymentIntentListFilter, IncidentListFilter,
    ObservationListFilter, PipelineContractListFilter, PipelineIntentListFilter,
    RegistryEvidenceListFilter, ReleaseListFilter, RemediationPlanListFilter, RunListFilter,
    RunSummaryFilter, StoredApprovalGate, StoredAuditEvent, StoredChangeSet,
    StoredDeploymentIntent, StoredIncident, StoredObservation, StoredPermissionGrant,
    StoredPipelineContract, StoredPipelineIntent, StoredRegistryEvidence, StoredRelease,
    StoredRemediationPlan, StoredWorkPlan, UpdateChangeSetRevision, UpdateDeploymentIntentDraft,
    UpdatePipelineIntentDraft, UpdatePipelineIntentExecution, UpdateRegistryEvidenceDraft,
    UpdateReleaseDraft, UpdateReleaseEvidence, UpdateWorkPlanRevision, WorkPlanListFilter,
};
use pharness_store::{
    CreateApprovalGate, CreateArtifact, CreateAuditEvent, CreateChangeSet, CreateDeploymentIntent,
    CreateIncident, CreateObservation, CreatePermissionGrant, CreatePipelineContract,
    CreatePipelineIntent, CreateRegistryEvidence, CreateRelease, CreateRemediationPlan, CreateRun,
    CreateSession, CreateWorkPlan, ReplacePipelineContract, SqliteStore, StoreError,
    UpdateDeploymentIntentEvidence, UpdatePipelineIntentEvidence,
};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
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
    worker: RunDispatcher,
    cluster_tools: ReadOnlyClusterTools,
    policy: SafetyPolicy,
    worker_token: Option<String>,
    operator_tokens: Arc<Vec<(String, String)>>,
}

pub fn router(
    store: Arc<SqliteStore>,
    worker: RunDispatcher,
    cluster_tools: ReadOnlyClusterTools,
    policy: SafetyPolicy,
    worker_token: Option<String>,
    operator_tokens: Vec<(String, String)>,
) -> Router {
    let state = AppState {
        store,
        worker,
        cluster_tools,
        policy,
        worker_token,
        operator_tokens: Arc::new(operator_tokens),
    };

    let internal = Router::new()
        .route(
            "/api/internal/runs/:run_id/attempt-context",
            get(internal_attempt_context),
        )
        .route(
            "/api/internal/runs/:run_id/mark-running",
            post(internal_mark_running),
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
        .route(
            "/api/internal/pipeline-intents/:pipeline_intent_id/execution-outcome",
            post(internal_pipeline_intent_execution_outcome),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_worker_token,
        ));

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
        .route(
            "/api/observations",
            get(list_observations).post(create_observation),
        )
        .route("/api/observations/:observation_id", get(get_observation))
        .route("/api/incidents", get(list_incidents).post(create_incident))
        .route("/api/incidents/:incident_id", get(get_incident))
        .route(
            "/api/remediation-plans",
            get(list_remediation_plans).post(create_remediation_plan),
        )
        .route("/api/remediation-plans/:plan_id", get(get_remediation_plan))
        .route(
            "/api/work-plans/from-remediation-plan",
            post(create_work_plan_from_remediation_plan),
        )
        .route("/api/work-plans", get(list_work_plans))
        .route("/api/work-plans/:work_plan_id", get(get_work_plan))
        .route(
            "/api/work-plans/:work_plan_id/readiness",
            get(work_plan_readiness),
        )
        .route("/api/work-plans/:work_plan_id/flow", get(work_plan_flow))
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
            "/api/change-sets/:change_set_id/readiness",
            get(change_set_readiness),
        )
        .route("/api/change-sets/:change_set_id/flow", get(change_set_flow))
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
        .route("/api/pipeline-intents", get(list_pipeline_intents))
        .route(
            "/api/pipeline-contracts",
            get(list_pipeline_contracts).post(create_pipeline_contract),
        )
        .route(
            "/api/pipeline-contracts/:pipeline_contract_id",
            get(get_pipeline_contract),
        )
        .route(
            "/api/pipeline-contracts/:pipeline_contract_id/transition",
            post(transition_pipeline_contract),
        )
        .route(
            "/api/pipeline-contracts/:pipeline_contract_id/replace",
            post(replace_pipeline_contract),
        )
        .route(
            "/api/pipeline-intents/from-change-set",
            post(create_pipeline_intent_from_change_set),
        )
        .route(
            "/api/pipeline-intents/:pipeline_intent_id",
            get(get_pipeline_intent),
        )
        .route(
            "/api/pipeline-intents/:pipeline_intent_id/transition",
            post(transition_pipeline_intent),
        )
        .route(
            "/api/pipeline-intents/:pipeline_intent_id/evidence",
            post(attach_pipeline_intent_evidence),
        )
        .route(
            "/api/pipeline-intents/:pipeline_intent_id/trusted-envelope",
            post(create_pipeline_intent_trusted_envelope),
        )
        .route(
            "/api/pipeline-intents/:pipeline_intent_id/execute",
            post(execute_pipeline_intent),
        )
        .route("/api/deployment-intents", get(list_deployment_intents))
        .route(
            "/api/deployment-intents/from-pipeline-intent",
            post(create_deployment_intent_from_pipeline_intent),
        )
        .route(
            "/api/deployment-intents/:deployment_intent_id",
            get(get_deployment_intent),
        )
        .route(
            "/api/deployment-intents/:deployment_intent_id/transition",
            post(transition_deployment_intent),
        )
        .route(
            "/api/deployment-intents/:deployment_intent_id/evidence",
            post(attach_deployment_intent_evidence),
        )
        .route("/api/releases", get(list_releases))
        .route(
            "/api/releases/from-deployment-intent",
            post(create_release_from_deployment_intent),
        )
        .route("/api/releases/:release_id", get(get_release))
        .route(
            "/api/releases/:release_id/transition",
            post(transition_release),
        )
        .route(
            "/api/releases/:release_id/evidence",
            post(attach_release_evidence),
        )
        .route("/api/registry-evidence", get(list_registry_evidence))
        .route(
            "/api/registry-evidence/from-release",
            post(create_registry_evidence_from_release),
        )
        .route(
            "/api/registry-evidence/from-registry-inspection",
            post(create_registry_evidence_from_registry_inspection),
        )
        .route(
            "/api/registry-evidence/:evidence_id",
            get(get_registry_evidence),
        )
        .route(
            "/api/registry-evidence/:evidence_id/transition",
            post(transition_registry_evidence),
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
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_operator_token,
        ))
        .merge(internal)
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

/// Gate `/api/internal/*` behind the configured worker token.
///
/// Worker ingest is disabled entirely when no token is configured, so a
/// loopback-only local deployment exposes no unauthenticated write surface
/// for remote workers.
async fn require_worker_token(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let Some(expected) = state.worker_token.as_deref() else {
        return ApiError::conflict("worker ingest is disabled: no worker token is configured")
            .into_response();
    };

    let provided = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    match provided {
        Some(token) if token_matches(token, expected) => next.run(request).await,
        _ => ApiError::unauthorized("invalid or missing worker token").into_response(),
    }
}

fn token_matches(provided: &str, expected: &str) -> bool {
    let provided = Sha256::digest(provided.as_bytes());
    let expected = Sha256::digest(expected.as_bytes());
    provided == expected
}

/// Authenticated operator identity resolved from the bearer token.
#[derive(Debug, Clone)]
pub struct OperatorIdentity(pub String);

/// Gate operator routes behind `PHARNESS_OPERATOR_TOKENS` when configured.
///
/// `/health` stays open for probes. With no operator tokens configured the
/// API keeps its loopback-trusting local behavior.
async fn require_operator_token(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    if state.operator_tokens.is_empty() || request.uri().path() == "/health" {
        return next.run(request).await;
    }

    let provided = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    let matched = provided.and_then(|token| {
        state
            .operator_tokens
            .iter()
            .find(|(_, expected)| token_matches(token, expected))
            .map(|(name, _)| name.clone())
    });

    match matched {
        Some(name) => {
            request.extensions_mut().insert(OperatorIdentity(name));
            next.run(request).await
        }
        None => ApiError::unauthorized("invalid or missing operator token").into_response(),
    }
}

#[derive(Debug, serde::Deserialize)]
struct InternalAttemptContextQuery {
    approval_id: Option<String>,
}

async fn internal_attempt_context(
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

    Ok(Json(spec))
}

async fn internal_mark_running(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<RunResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    let run = state.store.mark_run_running(&run_id).await?;

    Ok(Json(run.into()))
}

#[derive(Debug, serde::Deserialize)]
struct InternalIngestEventsRequest {
    events: Vec<AgentEvent>,
}

async fn internal_ingest_events(
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

async fn internal_ingest_outcome(
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

    Ok(Json(run.into()))
}

async fn internal_run_control(
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

async fn config_effective(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
) -> Json<serde_json::Value> {
    let worker = state.worker.config_json();
    let operator = json!({
        "auth_required": !state.operator_tokens.is_empty(),
        "name": identity.map(|Extension(OperatorIdentity(name))| name),
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
        "worker": worker,
        "operator": operator,
    }))
}

async fn execute_capability(
    State(state): State<AppState>,
    Json(request): Json<ExecuteCapabilityRequest>,
) -> Result<Json<ExecuteCapabilityResponse>, ApiError> {
    execute_direct_capability(&state, request.action, request.timeout_ms)
        .await
        .map(Json)
}

async fn execute_direct_capability(
    state: &AppState,
    action: AgentAction,
    requested_timeout_ms: Option<u64>,
) -> Result<ExecuteCapabilityResponse, ApiError> {
    let timeout_ms = direct_capability_timeout_ms(requested_timeout_ms);
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
                    let evidence =
                        persist_direct_capability_evidence(&state.store, &action_name, &result)
                            .await?;
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
                        artifact_id: evidence.artifact_id,
                        observation_id: evidence.observation_id,
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
                        artifact_id: None,
                        observation_id: None,
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
                        artifact_id: None,
                        observation_id: None,
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
            artifact_id: None,
            observation_id: None,
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
            artifact_id: None,
            observation_id: None,
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

    Ok(response)
}

#[derive(Debug, Default)]
struct DirectCapabilityEvidence {
    artifact_id: Option<String>,
    observation_id: Option<String>,
}

async fn persist_direct_capability_evidence(
    store: &SqliteStore,
    action_name: &str,
    result: &ToolResult,
) -> Result<DirectCapabilityEvidence, ApiError> {
    let Some(source) = direct_evidence_source(result) else {
        return Ok(DirectCapabilityEvidence::default());
    };

    let (session_id, run_id) =
        root_session_for_request(store, None, None, "direct capability evidence").await?;
    let artifact_kind = direct_artifact_kind(&result.content, source);
    let artifact_id = format!("art_direct_{}_{}", action_name, unique_suffix());
    let artifact = store
        .create_artifact(CreateArtifact {
            id: artifact_id.clone(),
            session_id: session_id.clone(),
            run_id: run_id.clone(),
            kind: artifact_kind,
            label: result.summary.clone(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(result.content.clone()),
        })
        .await?;

    let kind = direct_observation_kind(&result.content, source);
    let subject = direct_observation_subject(&result.content, source, &kind);
    let observation = store
        .create_observation(CreateObservation {
            id: format!("obs_direct_{}_{}", action_name, unique_suffix()),
            session_id,
            run_id,
            source: source.to_string(),
            kind: kind.clone(),
            subject: subject.clone(),
            summary: result.summary.clone(),
            resource_namespace: direct_observation_namespace(&result.content),
            resource_kind: direct_observation_resource_kind(&result.content, source, &kind),
            resource_name: direct_observation_resource_name(
                &result.content,
                source,
                &kind,
                &subject,
            ),
            resource_ref_json: Some(direct_observation_resource_ref(
                action_name,
                source,
                &kind,
                &subject,
            )),
            artifact_id: Some(artifact.id.clone()),
            data_json: direct_observation_data(&result.content),
        })
        .await?;
    append_observation_audit_event(
        store,
        &observation,
        "observation.created",
        Some("api".to_string()),
        Some(format!("direct capability {action_name}")),
    )
    .await?;

    Ok(DirectCapabilityEvidence {
        artifact_id: Some(artifact.id),
        observation_id: Some(observation.id),
    })
}

fn direct_evidence_source(result: &ToolResult) -> Option<&str> {
    let source = result.content.get("source")?.as_str()?;
    matches!(
        source,
        "kubernetes" | "argocd" | "prometheus" | "loki" | "tekton"
    )
    .then_some(source)
}

fn direct_artifact_kind(content: &Value, source: &str) -> String {
    if source == "tekton"
        && content.get("resource").and_then(Value::as_str) == Some("pipeline_run_analysis")
    {
        "pipeline_run_analysis".to_string()
    } else {
        format!("{source}_tool_result")
    }
}

fn direct_observation_kind(content: &Value, source: &str) -> String {
    content
        .get("resource")
        .and_then(Value::as_str)
        .or_else(|| content.get("action").and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| format!("{source}_read"))
}

fn direct_observation_subject(content: &Value, source: &str, kind: &str) -> String {
    if source == "tekton" && kind == "pipeline_run_analysis" {
        if let (Some(namespace), Some(name)) = (
            content
                .pointer("/analysis/pipeline_run/namespace")
                .and_then(Value::as_str),
            content
                .pointer("/analysis/pipeline_run/name")
                .and_then(Value::as_str),
        ) {
            return format!("{namespace}/{name}");
        }
    }
    if let Some(query) = content.get("query").and_then(Value::as_str) {
        return query.to_string();
    }
    if let Some(name) = content.get("name").and_then(Value::as_str) {
        return name.to_string();
    }
    if let Some(namespace) = content.get("namespace").and_then(Value::as_str) {
        return format!("{namespace}/{kind}");
    }
    format!("{source}/{kind}")
}

fn direct_observation_namespace(content: &Value) -> Option<String> {
    first_direct_string(&[
        content.pointer("/namespace"),
        content.pointer("/output/metadata/namespace"),
        content.pointer("/analysis/pipeline_run/namespace"),
    ])
}

fn direct_observation_resource_kind(content: &Value, source: &str, kind: &str) -> Option<String> {
    let output_kind = content.pointer("/output/kind").and_then(Value::as_str);
    if output_kind.is_some_and(|value| value != "List") {
        return output_kind.map(str::to_string);
    }
    if source == "tekton" && kind == "pipeline_run_analysis" {
        return Some("PipelineRun".to_string());
    }

    first_direct_string(&[
        content.pointer("/analysis/pipeline_run/kind"),
        content.pointer("/resource"),
    ])
    .or_else(|| match (source, kind) {
        ("argocd", _) => Some("Application".to_string()),
        ("prometheus", "inventory") => Some("inventory".to_string()),
        ("prometheus", _) => Some("query".to_string()),
        ("loki", "log_summary") => Some("log_summary".to_string()),
        (_, value) if !value.trim().is_empty() => Some(value.to_string()),
        _ => None,
    })
}

fn direct_observation_resource_name(
    content: &Value,
    source: &str,
    kind: &str,
    subject: &str,
) -> Option<String> {
    first_direct_string(&[
        content.pointer("/name"),
        content.pointer("/output/metadata/name"),
        content.pointer("/analysis/pipeline_run/name"),
    ])
    .or_else(|| match (source, kind) {
        ("prometheus", "inventory") => Some("inventory".to_string()),
        ("loki", "log_summary") => Some("log_summary".to_string()),
        _ if !subject.trim().is_empty() && !subject.contains('/') => Some(subject.to_string()),
        _ => None,
    })
}

fn first_direct_string(values: &[Option<&Value>]) -> Option<String> {
    values
        .iter()
        .filter_map(|value| value.and_then(Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

fn direct_observation_resource_ref(
    action_name: &str,
    source: &str,
    kind: &str,
    subject: &str,
) -> Value {
    json!({
        "source": source,
        "kind": kind,
        "name": subject,
        "metadata": {
            "capability": action_name,
            "direct": true,
        },
    })
}

fn direct_observation_data(content: &Value) -> Value {
    let mut data = Map::new();
    for key in [
        "source",
        "resource",
        "namespace",
        "name",
        "query",
        "output",
        "response",
        "inventory",
        "analysis",
    ] {
        if let Some(value) = content.get(key) {
            data.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(data)
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
            | AgentAction::RegistryInspectImage { .. }
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

async fn create_observation(
    State(state): State<AppState>,
    Json(request): Json<CreateObservationRequest>,
) -> Result<Json<ObservationResponse>, ApiError> {
    let source = required_text(request.source, "source")?;
    let kind = required_text(request.kind, "kind")?;
    let subject = required_text(request.subject, "subject")?;
    let summary = required_text(request.summary, "summary")?;
    let data_json = request.data_json.unwrap_or_else(|| json!({}));
    ensure_json_object(&data_json, "data_json")?;
    if let Some(resource_ref) = &request.resource_ref {
        ensure_json_object(resource_ref, "resource_ref")?;
    }
    if let Some(artifact_id) = clean_optional_text(request.artifact_id.clone()) {
        state
            .store
            .get_artifact(&artifact_id)
            .await?
            .ok_or_else(|| ApiError::not_found("artifact", &artifact_id))?;
    }

    let (session_id, run_id) = root_session_for_request(
        &state.store,
        clean_optional_text(request.session_id),
        request.run_id,
        "control-plane observation",
    )
    .await?;
    let observation = state
        .store
        .create_observation(CreateObservation {
            id: clean_optional_text(request.id)
                .unwrap_or_else(|| format!("obs_{}", unique_suffix())),
            session_id,
            run_id,
            source,
            kind,
            subject,
            summary,
            resource_namespace: clean_optional_text(request.resource_namespace),
            resource_kind: clean_optional_text(request.resource_kind),
            resource_name: clean_optional_text(request.resource_name),
            resource_ref_json: request.resource_ref,
            artifact_id: clean_optional_text(request.artifact_id),
            data_json,
        })
        .await?;
    append_observation_audit_event(
        &state.store,
        &observation,
        "observation.created",
        clean_optional_text(request.actor),
        clean_optional_text(request.reason),
    )
    .await?;

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

async fn create_incident(
    State(state): State<AppState>,
    Json(request): Json<CreateIncidentRequest>,
) -> Result<Json<IncidentResponse>, ApiError> {
    let observation_id = required_text(request.observation_id, "observation_id")?;
    let observation = state
        .store
        .get_observation(&observation_id)
        .await?
        .ok_or_else(|| ApiError::not_found("observation", &observation_id))?;
    let status = clean_optional_text(request.status).unwrap_or_else(|| "candidate".to_string());
    validate_allowed_value(
        "status",
        &status,
        &[
            "candidate",
            "open",
            "investigating",
            "mitigated",
            "resolved",
            "dismissed",
        ],
    )?;
    let severity = required_text(request.severity, "severity")?;
    validate_allowed_value(
        "severity",
        &severity,
        &["info", "low", "medium", "high", "critical"],
    )?;
    let data_json = request.data_json.unwrap_or_else(|| json!({}));
    ensure_json_object(&data_json, "data_json")?;

    let incident = state
        .store
        .create_incident(CreateIncident {
            id: clean_optional_text(request.id)
                .unwrap_or_else(|| format!("inc_{}", unique_suffix())),
            observation_id: observation.id.clone(),
            session_id: observation.session_id.clone(),
            run_id: observation.run_id.clone(),
            status,
            severity,
            title: required_text(request.title, "title")?,
            summary: required_text(request.summary, "summary")?,
            resource_namespace: clean_optional_text(request.resource_namespace)
                .or(observation.resource_namespace),
            resource_kind: clean_optional_text(request.resource_kind).or(observation.resource_kind),
            resource_name: clean_optional_text(request.resource_name).or(observation.resource_name),
            data_json,
        })
        .await?;
    append_incident_audit_event(
        &state.store,
        &incident,
        "incident.created",
        clean_optional_text(request.actor),
        clean_optional_text(request.reason),
    )
    .await?;

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

async fn create_remediation_plan(
    State(state): State<AppState>,
    Json(request): Json<CreateRemediationPlanRequest>,
) -> Result<Json<RemediationPlanResponse>, ApiError> {
    let incident_id = required_text(request.incident_id, "incident_id")?;
    let incident = state
        .store
        .get_incident(&incident_id)
        .await?
        .ok_or_else(|| ApiError::not_found("incident", &incident_id))?;
    let status = clean_optional_text(request.status).unwrap_or_else(|| "draft".to_string());
    validate_allowed_value(
        "status",
        &status,
        &[
            "draft",
            "proposed",
            "approved",
            "executing",
            "blocked",
            "completed",
            "rejected",
            "stale",
        ],
    )?;
    let risk_level = required_text(request.risk_level, "risk_level")?;
    validate_allowed_value(
        "risk_level",
        &risk_level,
        &["low", "medium", "high", "critical"],
    )?;
    let plan_json = request.plan_json.unwrap_or_else(|| json!({}));
    ensure_json_object(&plan_json, "plan_json")?;

    let plan = state
        .store
        .create_remediation_plan(CreateRemediationPlan {
            id: clean_optional_text(request.id)
                .unwrap_or_else(|| format!("rplan_{}", unique_suffix())),
            incident_id: incident.id.clone(),
            session_id: incident.session_id.clone(),
            run_id: incident.run_id.clone(),
            status,
            title: required_text(request.title, "title")?,
            summary: required_text(request.summary, "summary")?,
            risk_level,
            requires_approval: request.requires_approval.unwrap_or(true),
            resource_namespace: clean_optional_text(request.resource_namespace)
                .or(incident.resource_namespace),
            resource_kind: clean_optional_text(request.resource_kind).or(incident.resource_kind),
            resource_name: clean_optional_text(request.resource_name).or(incident.resource_name),
            plan_json,
        })
        .await?;
    append_remediation_plan_audit_event(
        &state.store,
        &plan,
        "remediation_plan.created",
        clean_optional_text(request.actor),
        clean_optional_text(request.reason),
    )
    .await?;
    for gate in approval_gates_from_remediation_plan(&plan) {
        let gate = state.store.create_approval_gate(gate).await?;
        append_approval_gate_audit_event(&state.store, &gate, "approval_gate.created", "created")
            .await?;
    }

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

async fn work_plan_readiness(
    State(state): State<AppState>,
    Path(work_plan_id): Path<String>,
) -> Result<Json<SdlcReadinessResponse>, ApiError> {
    let work_plan = state
        .store
        .get_work_plan(&work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &work_plan_id))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?;
    let resource_id = work_plan.id.clone();

    build_sdlc_readiness(
        &state.store,
        "work_plan",
        &resource_id,
        work_plan,
        change_set,
    )
    .await
    .map(Json)
}

async fn work_plan_flow(
    State(state): State<AppState>,
    Path(work_plan_id): Path<String>,
) -> Result<Json<SdlcFlowResponse>, ApiError> {
    let work_plan = state
        .store
        .get_work_plan(&work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &work_plan_id))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?;
    let resource_id = work_plan.id.clone();
    build_sdlc_flow(
        &state.store,
        "work_plan",
        &resource_id,
        work_plan,
        change_set,
    )
    .await
    .map(Json)
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
    let invalidated_trusted_envelopes = if request.material_change {
        stale_trusted_envelopes_for_work_plan(
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
        Vec::new()
    };
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
            "invalidated_permission_grant_ids": invalidated_trusted_envelopes
                .iter()
                .map(|grant| grant.id.clone())
                .collect::<Vec<_>>(),
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

async fn stale_trusted_envelopes_for_work_plan(
    store: &SqliteStore,
    work_plan_id: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<Vec<StoredPermissionGrant>, ApiError> {
    stale_trusted_envelopes_matching(store, actor, reason, |scope| {
        !scope.work_plan_ids.is_empty() && scope.work_plan_ids.iter().any(|id| id == work_plan_id)
    })
    .await
}

async fn stale_trusted_envelopes_for_change_set(
    store: &SqliteStore,
    change_set_id: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<Vec<StoredPermissionGrant>, ApiError> {
    stale_trusted_envelopes_matching(store, actor, reason, |scope| {
        !scope.change_set_ids.is_empty()
            && scope.change_set_ids.iter().any(|id| id == change_set_id)
    })
    .await
}

async fn stale_pipeline_intent_for_change_set(
    store: &SqliteStore,
    change_set_id: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<Option<StoredPipelineIntent>, ApiError> {
    let Some(intent) = store
        .get_pipeline_intent_by_change_set(change_set_id)
        .await?
    else {
        return Ok(None);
    };
    if intent.status == "stale" {
        return Ok(None);
    }

    let previous_status = intent.status.clone();
    let intent = store
        .update_pipeline_intent_status(&intent.id, "stale", actor.clone(), reason.clone())
        .await?;
    append_pipeline_intent_audit_event(
        store,
        &intent,
        "pipeline_intent.stale",
        actor,
        reason,
        json!({
            "previous_status": previous_status,
            "source": "change_set_revision",
            "change_set_id": change_set_id,
        }),
    )
    .await?;

    Ok(Some(intent))
}

async fn stale_deployment_intent_for_pipeline_intent(
    store: &SqliteStore,
    pipeline_intent_id: &str,
    actor: Option<String>,
    reason: Option<String>,
    source: &'static str,
) -> Result<Option<StoredDeploymentIntent>, ApiError> {
    let Some(intent) = store
        .get_deployment_intent_by_pipeline_intent(pipeline_intent_id)
        .await?
    else {
        return Ok(None);
    };
    if intent.status == "stale" {
        return Ok(None);
    }

    let previous_status = intent.status.clone();
    let intent = store
        .update_deployment_intent_status(&intent.id, "stale", actor.clone(), reason.clone())
        .await?;
    append_deployment_intent_audit_event(
        store,
        &intent,
        "deployment_intent.stale",
        actor,
        reason,
        json!({
            "previous_status": previous_status,
            "source": source,
            "pipeline_intent_id": pipeline_intent_id,
        }),
    )
    .await?;

    Ok(Some(intent))
}

async fn stale_release_for_deployment_intent(
    store: &SqliteStore,
    deployment_intent_id: &str,
    actor: Option<String>,
    reason: Option<String>,
    source: &'static str,
) -> Result<Option<StoredRelease>, ApiError> {
    let Some(release) = store
        .get_release_by_deployment_intent(deployment_intent_id)
        .await?
    else {
        return Ok(None);
    };
    if release.status == "stale" {
        return Ok(None);
    }

    let previous_status = release.status.clone();
    let release = store
        .update_release_status(&release.id, "stale", actor.clone(), reason.clone())
        .await?;
    append_release_audit_event(
        store,
        &release,
        "release.stale",
        actor,
        reason,
        json!({
            "previous_status": previous_status,
            "source": source,
            "deployment_intent_id": deployment_intent_id,
        }),
    )
    .await?;

    Ok(Some(release))
}

async fn stale_registry_evidence_for_release(
    store: &SqliteStore,
    release_id: &str,
    actor: Option<String>,
    reason: Option<String>,
    source: &'static str,
) -> Result<Option<StoredRegistryEvidence>, ApiError> {
    let Some(evidence) = store.get_registry_evidence_by_release(release_id).await? else {
        return Ok(None);
    };
    if evidence.status == "stale" {
        return Ok(None);
    }

    let previous_status = evidence.status.clone();
    let evidence = store
        .update_registry_evidence_status(&evidence.id, "stale", actor.clone(), reason.clone())
        .await?;
    append_registry_evidence_audit_event(
        store,
        &evidence,
        "registry_evidence.stale",
        actor,
        reason,
        json!({
            "previous_status": previous_status,
            "source": source,
            "release_id": release_id,
        }),
    )
    .await?;

    Ok(Some(evidence))
}

async fn stale_trusted_envelopes_matching(
    store: &SqliteStore,
    actor: Option<String>,
    reason: Option<String>,
    matches_scope: impl Fn(&PermissionGrantScope) -> bool,
) -> Result<Vec<StoredPermissionGrant>, ApiError> {
    let active_grants = store.list_permission_grants(Some("active"), 200).await?;
    let mut staled = Vec::new();
    for grant in active_grants {
        let scope = serde_json::from_value::<PermissionGrantScope>(grant.scope_json.clone())
            .map_err(|error| {
                ApiError::internal(format!(
                    "permission grant {} has invalid scope: {error}",
                    grant.id
                ))
            })?;
        if !matches_scope(&scope) {
            continue;
        }

        let grant = store
            .stale_permission_grant(&grant.id, actor.clone(), reason.clone())
            .await?;
        append_permission_grant_audit_event(store, "permission_grant.stale", &grant, actor.clone())
            .await?;
        staled.push(grant);
    }

    Ok(staled)
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

#[derive(Debug, Default, serde::Deserialize)]
struct ListPipelineIntentsQuery {
    change_set_id: Option<String>,
    work_plan_id: Option<String>,
    remediation_plan_id: Option<String>,
    incident_id: Option<String>,
    run_id: Option<String>,
    status: Option<String>,
    intent_kind: Option<String>,
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
struct ListDeploymentIntentsQuery {
    pipeline_intent_id: Option<String>,
    change_set_id: Option<String>,
    work_plan_id: Option<String>,
    remediation_plan_id: Option<String>,
    incident_id: Option<String>,
    run_id: Option<String>,
    status: Option<String>,
    intent_kind: Option<String>,
    risk_level: Option<String>,
    target_environment: Option<String>,
    target_namespace: Option<String>,
    argo_application: Option<String>,
    resource_namespace: Option<String>,
    resource_kind: Option<String>,
    resource_name: Option<String>,
    created_after_ms: Option<i64>,
    created_before_ms: Option<i64>,
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct ListReleasesQuery {
    deployment_intent_id: Option<String>,
    pipeline_intent_id: Option<String>,
    change_set_id: Option<String>,
    work_plan_id: Option<String>,
    remediation_plan_id: Option<String>,
    incident_id: Option<String>,
    run_id: Option<String>,
    status: Option<String>,
    release_kind: Option<String>,
    risk_level: Option<String>,
    target_environment: Option<String>,
    target_namespace: Option<String>,
    argo_application: Option<String>,
    version: Option<String>,
    commit_sha: Option<String>,
    image_digest: Option<String>,
    created_after_ms: Option<i64>,
    created_before_ms: Option<i64>,
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct ListRegistryEvidenceQuery {
    release_id: Option<String>,
    deployment_intent_id: Option<String>,
    pipeline_intent_id: Option<String>,
    change_set_id: Option<String>,
    work_plan_id: Option<String>,
    remediation_plan_id: Option<String>,
    incident_id: Option<String>,
    run_id: Option<String>,
    status: Option<String>,
    risk_level: Option<String>,
    registry: Option<String>,
    repository: Option<String>,
    image_ref: Option<String>,
    image_digest: Option<String>,
    tag: Option<String>,
    source: Option<String>,
    verification_status: Option<String>,
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

async fn change_set_readiness(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
) -> Result<Json<SdlcReadinessResponse>, ApiError> {
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
    let resource_id = change_set.id.clone();

    build_sdlc_readiness(
        &state.store,
        "change_set",
        &resource_id,
        work_plan,
        Some(change_set),
    )
    .await
    .map(Json)
}

async fn change_set_flow(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
) -> Result<Json<SdlcFlowResponse>, ApiError> {
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
    let resource_id = change_set.id.clone();
    build_sdlc_flow(
        &state.store,
        "change_set",
        &resource_id,
        work_plan,
        Some(change_set),
    )
    .await
    .map(Json)
}

async fn build_sdlc_flow(
    store: &SqliteStore,
    resource_kind: &str,
    resource_id: &str,
    work_plan: StoredWorkPlan,
    change_set: Option<StoredChangeSet>,
) -> Result<SdlcFlowResponse, ApiError> {
    let pipeline_intent = if let Some(change_set) = &change_set {
        store
            .get_pipeline_intent_by_change_set(&change_set.id)
            .await?
    } else {
        None
    };
    let deployment_intent = if let Some(pipeline_intent) = &pipeline_intent {
        store
            .get_deployment_intent_by_pipeline_intent(&pipeline_intent.id)
            .await?
    } else {
        None
    };
    let release = if let Some(deployment_intent) = &deployment_intent {
        store
            .get_release_by_deployment_intent(&deployment_intent.id)
            .await?
    } else {
        None
    };
    let registry_evidence = if let Some(release) = &release {
        store.get_registry_evidence_by_release(&release.id).await?
    } else {
        None
    };
    let readiness = build_sdlc_readiness(
        store,
        resource_kind,
        resource_id,
        work_plan.clone(),
        change_set.clone(),
    )
    .await?;
    let incidents =
        collect_sdlc_flow_incidents(store, &work_plan.incident_id, release.as_ref()).await?;
    let remediation_plans =
        collect_sdlc_flow_remediation_plans(store, &work_plan, &incidents).await?;
    let approval_gates = collect_sdlc_flow_approval_gates(store, &remediation_plans).await?;
    let audit_events = collect_sdlc_flow_audit_events(
        store,
        &work_plan,
        change_set.as_ref(),
        pipeline_intent.as_ref(),
        deployment_intent.as_ref(),
        release.as_ref(),
        registry_evidence.as_ref(),
        &incidents,
        &remediation_plans,
        &approval_gates,
    )
    .await?;

    Ok(SdlcFlowResponse {
        resource_kind: resource_kind.to_string(),
        resource_id: resource_id.to_string(),
        readiness,
        work_plan: work_plan.into(),
        change_set: change_set.map(Into::into),
        pipeline_intent: pipeline_intent.map(Into::into),
        deployment_intent: deployment_intent.map(Into::into),
        release: release.map(Into::into),
        registry_evidence: registry_evidence.map(Into::into),
        incidents: incidents.into_iter().map(Into::into).collect(),
        remediation_plans: remediation_plans.into_iter().map(Into::into).collect(),
        approval_gates: approval_gates.into_iter().map(Into::into).collect(),
        audit_events: audit_events.into_iter().map(Into::into).collect(),
    })
}

async fn collect_sdlc_flow_incidents(
    store: &SqliteStore,
    root_incident_id: &str,
    release: Option<&StoredRelease>,
) -> Result<Vec<StoredIncident>, ApiError> {
    let mut incident_ids = BTreeSet::new();
    incident_ids.insert(root_incident_id.to_string());

    if let Some(release) = release {
        if let Some(evidence) = release
            .release_json
            .get("observability_evidence")
            .and_then(Value::as_array)
        {
            for item in evidence {
                let Some(observation_id) = item.get("observation_id").and_then(Value::as_str)
                else {
                    continue;
                };
                incident_ids.insert(release_observability_incident_id_for_ids(
                    &release.id,
                    observation_id,
                ));
            }
        }
    }

    let mut incidents = Vec::new();
    for incident_id in incident_ids {
        if let Some(incident) = store.get_incident(&incident_id).await? {
            incidents.push(incident);
        }
    }
    Ok(incidents)
}

async fn collect_sdlc_flow_remediation_plans(
    store: &SqliteStore,
    work_plan: &StoredWorkPlan,
    incidents: &[StoredIncident],
) -> Result<Vec<StoredRemediationPlan>, ApiError> {
    let mut plan_ids = BTreeSet::new();
    plan_ids.insert(work_plan.remediation_plan_id.clone());
    for incident in incidents {
        for plan in store
            .list_remediation_plans(RemediationPlanListFilter {
                incident_id: Some(incident.id.clone()),
                limit: 50,
                ..RemediationPlanListFilter::default()
            })
            .await?
        {
            plan_ids.insert(plan.id);
        }
    }

    let mut plans = Vec::new();
    for plan_id in plan_ids {
        if let Some(plan) = store.get_remediation_plan(&plan_id).await? {
            plans.push(plan);
        }
    }
    Ok(plans)
}

async fn collect_sdlc_flow_approval_gates(
    store: &SqliteStore,
    remediation_plans: &[StoredRemediationPlan],
) -> Result<Vec<StoredApprovalGate>, ApiError> {
    let mut gates = Vec::new();
    let mut seen_gate_ids = BTreeSet::new();
    for plan in remediation_plans {
        for gate in store
            .list_approval_gates(ApprovalGateListFilter {
                remediation_plan_id: Some(plan.id.clone()),
                limit: 100,
                ..ApprovalGateListFilter::default()
            })
            .await?
        {
            if seen_gate_ids.insert(gate.id.clone()) {
                gates.push(gate);
            }
        }
    }
    Ok(gates)
}

#[allow(clippy::too_many_arguments)]
async fn collect_sdlc_flow_audit_events(
    store: &SqliteStore,
    work_plan: &StoredWorkPlan,
    change_set: Option<&StoredChangeSet>,
    pipeline_intent: Option<&StoredPipelineIntent>,
    deployment_intent: Option<&StoredDeploymentIntent>,
    release: Option<&StoredRelease>,
    registry_evidence: Option<&StoredRegistryEvidence>,
    incidents: &[StoredIncident],
    remediation_plans: &[StoredRemediationPlan],
    approval_gates: &[StoredApprovalGate],
) -> Result<Vec<StoredAuditEvent>, ApiError> {
    let mut resources = vec![("work_plan", work_plan.id.clone())];
    if let Some(change_set) = change_set {
        resources.push(("change_set", change_set.id.clone()));
    }
    if let Some(pipeline_intent) = pipeline_intent {
        resources.push(("pipeline_intent", pipeline_intent.id.clone()));
    }
    if let Some(deployment_intent) = deployment_intent {
        resources.push(("deployment_intent", deployment_intent.id.clone()));
    }
    if let Some(release) = release {
        resources.push(("release", release.id.clone()));
    }
    if let Some(registry_evidence) = registry_evidence {
        resources.push(("registry_evidence", registry_evidence.id.clone()));
    }
    resources.extend(
        incidents
            .iter()
            .map(|incident| ("incident", incident.id.clone())),
    );
    resources.extend(
        remediation_plans
            .iter()
            .map(|plan| ("remediation_plan", plan.id.clone())),
    );
    resources.extend(
        approval_gates
            .iter()
            .map(|gate| ("approval_gate", gate.id.clone())),
    );

    let mut events = Vec::new();
    let mut seen_event_ids = BTreeSet::new();
    for (resource_kind, resource_id) in resources {
        for event in store
            .list_audit_events(Some(resource_kind), Some(&resource_id), None, 25)
            .await?
        {
            if seen_event_ids.insert(event.id.clone()) {
                events.push(event);
            }
        }
    }
    events.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    if events.len() > 200 {
        events.drain(0..events.len() - 200);
    }
    Ok(events)
}

async fn build_sdlc_readiness(
    store: &SqliteStore,
    resource_kind: &str,
    resource_id: &str,
    work_plan: StoredWorkPlan,
    change_set: Option<StoredChangeSet>,
) -> Result<SdlcReadinessResponse, ApiError> {
    let pipeline_intent = if let Some(change_set) = &change_set {
        store
            .get_pipeline_intent_by_change_set(&change_set.id)
            .await?
    } else {
        None
    };
    let deployment_intent = if let Some(pipeline_intent) = &pipeline_intent {
        store
            .get_deployment_intent_by_pipeline_intent(&pipeline_intent.id)
            .await?
    } else {
        None
    };
    let release = if let Some(deployment_intent) = &deployment_intent {
        store
            .get_release_by_deployment_intent(&deployment_intent.id)
            .await?
    } else {
        None
    };
    let registry_evidence = if let Some(release) = &release {
        store.get_registry_evidence_by_release(&release.id).await?
    } else {
        None
    };
    let gates = readiness_gate_summary(store, &work_plan.remediation_plan_id).await?;
    let grants = readiness_grant_summary(store, resource_kind, resource_id).await?;
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();

    add_status_findings(
        &mut blockers,
        &mut warnings,
        resource_kind,
        resource_id,
        &work_plan,
        change_set.as_ref(),
    );
    add_pipeline_intent_findings(&mut warnings, change_set.as_ref(), pipeline_intent.as_ref());
    add_deployment_intent_findings(
        &mut warnings,
        pipeline_intent.as_ref(),
        deployment_intent.as_ref(),
    );
    add_release_findings(&mut warnings, deployment_intent.as_ref(), release.as_ref());
    add_registry_evidence_findings(&mut warnings, release.as_ref(), registry_evidence.as_ref());
    add_gate_findings(&mut blockers, &gates);
    add_grant_findings(
        &mut blockers,
        &mut warnings,
        resource_kind,
        resource_id,
        &grants,
    );

    let ready = blockers.is_empty();
    let summary = readiness_summary(ready, blockers.len(), warnings.len());

    Ok(SdlcReadinessResponse {
        resource_kind: resource_kind.to_string(),
        resource_id: resource_id.to_string(),
        ready,
        summary,
        work_plan: work_plan.into(),
        change_set: change_set.map(Into::into),
        pipeline_intent: pipeline_intent.map(Into::into),
        deployment_intent: deployment_intent.map(Into::into),
        release: release.map(Into::into),
        registry_evidence: registry_evidence.map(Into::into),
        blockers,
        warnings,
        approval_gates: gates,
        trusted_envelopes: grants,
    })
}

fn add_status_findings(
    blockers: &mut Vec<SdlcReadinessFinding>,
    warnings: &mut Vec<SdlcReadinessFinding>,
    resource_kind: &str,
    resource_id: &str,
    work_plan: &StoredWorkPlan,
    change_set: Option<&StoredChangeSet>,
) {
    if work_plan.status != "approved" {
        blockers.push(readiness_finding(
            "work_plan_not_approved",
            format!(
                "WorkPlan {} is {}, not approved",
                work_plan.id, work_plan.status
            ),
            "work_plan",
            &work_plan.id,
        ));
    }

    match (resource_kind, change_set) {
        ("change_set", Some(change_set)) if change_set.status != "approved" => {
            blockers.push(readiness_finding(
                "change_set_not_approved",
                format!(
                    "ChangeSet {} is {}, not approved",
                    change_set.id, change_set.status
                ),
                "change_set",
                &change_set.id,
            ));
        }
        ("work_plan", Some(change_set)) if change_set.status != "approved" => {
            blockers.push(readiness_finding(
                "current_change_set_not_approved",
                format!(
                    "Current ChangeSet {} is {}, not approved",
                    change_set.id, change_set.status
                ),
                "change_set",
                &change_set.id,
            ));
        }
        ("work_plan", None) => warnings.push(readiness_finding(
            "missing_change_set",
            "No ChangeSet exists; a WorkPlan trusted envelope is broader than source-change execution",
            "work_plan",
            resource_id,
        )),
        _ => {}
    }
}

fn add_pipeline_intent_findings(
    warnings: &mut Vec<SdlcReadinessFinding>,
    change_set: Option<&StoredChangeSet>,
    pipeline_intent: Option<&StoredPipelineIntent>,
) {
    let Some(change_set) = change_set else {
        return;
    };
    match pipeline_intent {
        None => warnings.push(readiness_finding(
            "missing_pipeline_intent",
            format!("ChangeSet {} has no PipelineIntent", change_set.id),
            "change_set",
            &change_set.id,
        )),
        Some(intent) if intent.status == "stale" => warnings.push(readiness_finding(
            "stale_pipeline_intent",
            format!("PipelineIntent {} is stale after source changes", intent.id),
            "pipeline_intent",
            &intent.id,
        )),
        Some(intent) if intent.status == "executing" => warnings.push(readiness_finding(
            "pipeline_execution_running",
            format!(
                "PipelineIntent {} has a PipelineRun execution in progress",
                intent.id
            ),
            "pipeline_intent",
            &intent.id,
        )),
        Some(intent) if intent.status == "failed" => warnings.push(readiness_finding(
            "pipeline_execution_failed",
            format!(
                "PipelineIntent {} has a failed PipelineRun execution",
                intent.id
            ),
            "pipeline_intent",
            &intent.id,
        )),
        Some(intent) if !pipeline_intent_is_deployment_eligible(&intent.status) => {
            warnings.push(readiness_finding(
                "pipeline_intent_not_approved",
                format!(
                    "PipelineIntent {} is {}, not approved",
                    intent.id, intent.status
                ),
                "pipeline_intent",
                &intent.id,
            ))
        }
        Some(intent) => add_pipeline_evidence_findings(warnings, intent),
    }
}

fn add_pipeline_evidence_findings(
    warnings: &mut Vec<SdlcReadinessFinding>,
    intent: &StoredPipelineIntent,
) {
    match pipeline_execution_evidence_status(intent) {
        Some("failed") => warnings.push(readiness_finding(
            "pipeline_execution_failed",
            format!(
                "PipelineIntent {} has durable execution evidence showing a failed PipelineRun",
                intent.id
            ),
            "pipeline_intent",
            &intent.id,
        )),
        Some("succeeded") | None => {}
        Some(_) => warnings.push(readiness_finding(
            "pipeline_execution_unknown",
            format!(
                "PipelineIntent {} has execution evidence with an unknown terminal state",
                intent.id
            ),
            "pipeline_intent",
            &intent.id,
        )),
    }

    match pipeline_intent_attached_evidence_status(intent) {
        Some("satisfied") => {}
        Some("running") => warnings.push(readiness_finding(
            "pipeline_evidence_running",
            format!(
                "PipelineIntent {} has attached evidence, but the pipeline is still running",
                intent.id
            ),
            "pipeline_intent",
            &intent.id,
        )),
        Some("attention_required") => warnings.push(readiness_finding(
            "pipeline_evidence_attention_required",
            format!(
                "PipelineIntent {} has attached evidence that requires review before deployment",
                intent.id
            ),
            "pipeline_intent",
            &intent.id,
        )),
        Some("failed") => warnings.push(readiness_finding(
            "pipeline_evidence_failed",
            format!(
                "PipelineIntent {} has attached evidence from a failed pipeline",
                intent.id
            ),
            "pipeline_intent",
            &intent.id,
        )),
        Some(_) => warnings.push(readiness_finding(
            "pipeline_evidence_unknown",
            format!(
                "PipelineIntent {} has attached evidence with an unknown status",
                intent.id
            ),
            "pipeline_intent",
            &intent.id,
        )),
        None => warnings.push(readiness_finding(
            "missing_pipeline_evidence",
            format!(
                "PipelineIntent {} is approved but has no attached PipelineRunAnalysis evidence",
                intent.id
            ),
            "pipeline_intent",
            &intent.id,
        )),
    }
}

fn add_deployment_intent_findings(
    warnings: &mut Vec<SdlcReadinessFinding>,
    pipeline_intent: Option<&StoredPipelineIntent>,
    deployment_intent: Option<&StoredDeploymentIntent>,
) {
    let Some(pipeline_intent) = pipeline_intent else {
        return;
    };
    if !pipeline_intent_is_deployment_eligible(&pipeline_intent.status) {
        return;
    }

    match deployment_intent {
        None => warnings.push(readiness_finding(
            "missing_deployment_intent",
            format!(
                "PipelineIntent {} has no DeploymentIntent",
                pipeline_intent.id
            ),
            "pipeline_intent",
            &pipeline_intent.id,
        )),
        Some(intent) if intent.status == "stale" => warnings.push(readiness_finding(
            "stale_deployment_intent",
            format!(
                "DeploymentIntent {} is stale after upstream intent changes",
                intent.id
            ),
            "deployment_intent",
            &intent.id,
        )),
        Some(intent) if intent.status != "approved" => warnings.push(readiness_finding(
            "deployment_intent_not_approved",
            format!(
                "DeploymentIntent {} is {}, not approved",
                intent.id, intent.status
            ),
            "deployment_intent",
            &intent.id,
        )),
        Some(intent) => add_deployment_evidence_findings(warnings, intent),
    }
}

fn add_deployment_evidence_findings(
    warnings: &mut Vec<SdlcReadinessFinding>,
    intent: &StoredDeploymentIntent,
) {
    match deployment_intent_attached_evidence_status(intent) {
        Some("satisfied") => {}
        Some("attention_required") => warnings.push(readiness_finding(
            "deployment_evidence_attention_required",
            format!(
                "DeploymentIntent {} has attached Argo evidence that requires review before release",
                intent.id
            ),
            "deployment_intent",
            &intent.id,
        )),
        Some(_) => warnings.push(readiness_finding(
            "deployment_evidence_unknown",
            format!(
                "DeploymentIntent {} has attached Argo evidence with an unknown status",
                intent.id
            ),
            "deployment_intent",
            &intent.id,
        )),
        None => warnings.push(readiness_finding(
            "missing_deployment_evidence",
            format!(
                "DeploymentIntent {} is approved but has no attached Argo Application evidence",
                intent.id
            ),
            "deployment_intent",
            &intent.id,
        )),
    }
}

fn add_release_findings(
    warnings: &mut Vec<SdlcReadinessFinding>,
    deployment_intent: Option<&StoredDeploymentIntent>,
    release: Option<&StoredRelease>,
) {
    let Some(deployment_intent) = deployment_intent else {
        return;
    };
    if deployment_intent.status != "approved" {
        return;
    }

    match release {
        None => warnings.push(readiness_finding(
            "missing_release",
            format!("DeploymentIntent {} has no Release", deployment_intent.id),
            "deployment_intent",
            &deployment_intent.id,
        )),
        Some(release) if release.status == "stale" => warnings.push(readiness_finding(
            "stale_release",
            format!(
                "Release {} is stale after upstream deployment changes",
                release.id
            ),
            "release",
            &release.id,
        )),
        Some(release) if release.status != "approved" => warnings.push(readiness_finding(
            "release_not_approved",
            format!("Release {} is {}, not approved", release.id, release.status),
            "release",
            &release.id,
        )),
        Some(release) => add_release_observability_findings(warnings, release),
    }
}

fn add_release_observability_findings(
    warnings: &mut Vec<SdlcReadinessFinding>,
    release: &StoredRelease,
) {
    match release_observability_evidence_status(release) {
        None => warnings.push(readiness_finding(
            "missing_release_observability_evidence",
            format!(
                "Release {} has no attached Prometheus or Loki observability evidence",
                release.id
            ),
            "release",
            &release.id,
        )),
        Some("attention_required") => warnings.push(readiness_finding(
            "release_observability_attention_required",
            format!(
                "Release {} has attached observability evidence that requires review",
                release.id
            ),
            "release",
            &release.id,
        )),
        Some("unknown") => warnings.push(readiness_finding(
            "release_observability_unknown",
            format!(
                "Release {} has attached observability evidence with unknown status",
                release.id
            ),
            "release",
            &release.id,
        )),
        Some(_) => {}
    }
}

fn add_registry_evidence_findings(
    warnings: &mut Vec<SdlcReadinessFinding>,
    release: Option<&StoredRelease>,
    registry_evidence: Option<&StoredRegistryEvidence>,
) {
    let Some(release) = release else {
        return;
    };
    if release.status != "approved" {
        return;
    }

    let Some(evidence) = registry_evidence else {
        warnings.push(readiness_finding(
            "missing_registry_evidence",
            format!("Release {} has no RegistryEvidence", release.id),
            "release",
            &release.id,
        ));
        return;
    };
    if evidence.status == "stale" {
        warnings.push(readiness_finding(
            "stale_registry_evidence",
            format!(
                "RegistryEvidence {} is stale after upstream release changes",
                evidence.id
            ),
            "registry_evidence",
            &evidence.id,
        ));
        return;
    }
    if evidence.status != "verified" {
        warnings.push(readiness_finding(
            "registry_evidence_not_verified",
            format!(
                "RegistryEvidence {} is {}, not verified",
                evidence.id, evidence.status
            ),
            "registry_evidence",
            &evidence.id,
        ));
    }
    if evidence.verification_status != "verified" {
        warnings.push(readiness_finding(
            "registry_evidence_verification_not_verified",
            format!(
                "RegistryEvidence {} verification status is {}",
                evidence.id, evidence.verification_status
            ),
            "registry_evidence",
            &evidence.id,
        ));
    }
    if evidence.status == "verified"
        && evidence.verification_status == "verified"
        && registry_evidence_is_inspection_backed(evidence)
        && !registry_evidence_has_supply_chain_verification(evidence)
    {
        warnings.push(readiness_finding(
            "registry_evidence_supply_chain_not_verified",
            format!(
                "RegistryEvidence {} is verified but lacks signature, SBOM, provenance, or vulnerability evidence",
                evidence.id
            ),
            "registry_evidence",
            &evidence.id,
        ));
    }
}

fn registry_evidence_is_inspection_backed(evidence: &StoredRegistryEvidence) -> bool {
    evidence.source == "registry_inspect_image"
        || evidence
            .evidence_json
            .pointer("/execution/capability")
            .and_then(Value::as_str)
            == Some("registry_inspect_image")
}

fn registry_evidence_has_supply_chain_verification(evidence: &StoredRegistryEvidence) -> bool {
    if matches!(
        evidence.source.as_str(),
        "cosign"
            | "signature"
            | "sbom"
            | "provenance"
            | "slsa_provenance"
            | "vulnerability_scan"
            | "supply_chain"
    ) {
        return true;
    }

    if evidence
        .evidence_json
        .pointer("/verification/supply_chain_verified")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return true;
    }

    evidence
        .evidence_json
        .pointer("/verification/checks")
        .and_then(Value::as_array)
        .is_some_and(|checks| checks.iter().any(is_supply_chain_check))
}

fn is_supply_chain_check(check: &Value) -> bool {
    let name = check
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let status = check
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let supply_chain_check = [
        "signature",
        "cosign",
        "sbom",
        "provenance",
        "slsa",
        "attestation",
        "vulnerability",
        "vuln",
    ]
    .iter()
    .any(|needle| name.contains(needle));
    let verified_status = ["verified", "pass", "passed", "ok", "success"]
        .iter()
        .any(|allowed| status == *allowed);

    supply_chain_check && verified_status
}

fn add_gate_findings(blockers: &mut Vec<SdlcReadinessFinding>, gates: &SdlcReadinessGateSummary) {
    for gate in &gates.pending {
        blockers.push(readiness_finding(
            "approval_gate_pending",
            format!("ApprovalGate {} is pending", gate.id),
            "approval_gate",
            &gate.id,
        ));
    }
    for gate in &gates.stale {
        blockers.push(readiness_finding(
            "approval_gate_stale",
            format!("ApprovalGate {} is stale", gate.id),
            "approval_gate",
            &gate.id,
        ));
    }
    for gate in &gates.rejected {
        blockers.push(readiness_finding(
            "approval_gate_rejected",
            format!("ApprovalGate {} is rejected", gate.id),
            "approval_gate",
            &gate.id,
        ));
    }
}

fn add_grant_findings(
    blockers: &mut Vec<SdlcReadinessFinding>,
    warnings: &mut Vec<SdlcReadinessFinding>,
    resource_kind: &str,
    resource_id: &str,
    grants: &SdlcReadinessGrantSummary,
) {
    if grants.active.is_empty() {
        blockers.push(readiness_finding(
            "missing_active_trusted_envelope",
            format!("{resource_kind} {resource_id} has no active trusted envelope"),
            resource_kind,
            resource_id,
        ));
    }
    for grant in &grants.stale {
        warnings.push(readiness_finding(
            "stale_trusted_envelope",
            format!("PermissionGrant {} is stale", grant.id),
            "permission_grant",
            &grant.id,
        ));
    }
}

async fn readiness_gate_summary(
    store: &SqliteStore,
    remediation_plan_id: &str,
) -> Result<SdlcReadinessGateSummary, ApiError> {
    let gates = store
        .list_approval_gates(ApprovalGateListFilter {
            remediation_plan_id: Some(remediation_plan_id.to_string()),
            limit: 200,
            ..ApprovalGateListFilter::default()
        })
        .await?;
    let mut pending = Vec::new();
    let mut stale = Vec::new();
    let mut rejected = Vec::new();

    for gate in gates {
        match gate.status.as_str() {
            "pending" => pending.push(gate.into()),
            "stale" => stale.push(gate.into()),
            "rejected" => rejected.push(gate.into()),
            _ => {}
        }
    }

    Ok(SdlcReadinessGateSummary {
        pending,
        stale,
        rejected,
    })
}

async fn readiness_grant_summary(
    store: &SqliteStore,
    resource_kind: &str,
    resource_id: &str,
) -> Result<SdlcReadinessGrantSummary, ApiError> {
    let now = current_millis();
    let grants = store.list_permission_grants(None, 200).await?;
    let mut active = Vec::new();
    let mut stale = Vec::new();

    for grant in grants {
        if !trusted_envelope_matches(&grant, resource_kind, resource_id)? {
            continue;
        }

        match grant.status.as_str() {
            "active" if grant_is_unexpired(&grant, now) => active.push(grant.into()),
            "stale" => stale.push(grant.into()),
            _ => {}
        }
    }

    Ok(SdlcReadinessGrantSummary { active, stale })
}

fn trusted_envelope_matches(
    grant: &StoredPermissionGrant,
    resource_kind: &str,
    resource_id: &str,
) -> Result<bool, ApiError> {
    let scope = serde_json::from_value::<PermissionGrantScope>(grant.scope_json.clone()).map_err(
        |error| {
            ApiError::internal(format!(
                "permission grant {} has invalid scope: {error}",
                grant.id
            ))
        },
    )?;

    Ok(match resource_kind {
        "work_plan" => {
            !scope.work_plan_ids.is_empty()
                && scope.work_plan_ids.iter().any(|id| id == resource_id)
                && scope.change_set_ids.is_empty()
        }
        "change_set" => {
            !scope.change_set_ids.is_empty()
                && scope.change_set_ids.iter().any(|id| id == resource_id)
        }
        _ => false,
    })
}

fn readiness_finding(
    code: impl Into<String>,
    message: impl Into<String>,
    resource_kind: impl Into<String>,
    resource_id: impl Into<String>,
) -> SdlcReadinessFinding {
    SdlcReadinessFinding {
        code: code.into(),
        message: message.into(),
        resource_kind: resource_kind.into(),
        resource_id: resource_id.into(),
    }
}

fn readiness_summary(ready: bool, blocker_count: usize, warning_count: usize) -> String {
    if ready {
        return format!("ready with {warning_count} warning(s)");
    }

    format!("blocked by {blocker_count} blocker(s) and {warning_count} warning(s)")
}

#[derive(Debug, Default, serde::Deserialize)]
struct ListPipelineContractsQuery {
    namespace: Option<String>,
    pipeline_ref: Option<String>,
    status: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}

async fn list_pipeline_contracts(
    State(state): State<AppState>,
    Query(query): Query<ListPipelineContractsQuery>,
) -> Result<Json<PipelineContractsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let pipeline_contracts = state
        .store
        .list_pipeline_contracts(PipelineContractListFilter {
            namespace: clean_optional_text(query.namespace),
            pipeline_ref: clean_optional_text(query.pipeline_ref),
            status: clean_optional_text(query.status),
            limit,
            offset,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let count = pipeline_contracts.len();
    Ok(Json(PipelineContractsResponse {
        pipeline_contracts,
        count,
        limit,
        offset,
    }))
}

async fn get_pipeline_contract(
    State(state): State<AppState>,
    Path(pipeline_contract_id): Path<String>,
) -> Result<Json<PipelineContractResponse>, ApiError> {
    let contract = state
        .store
        .get_pipeline_contract(&pipeline_contract_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_contract", &pipeline_contract_id))?;
    Ok(Json(contract.into()))
}

async fn create_pipeline_contract(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Json(request): Json<CreatePipelineContractRequest>,
) -> Result<Json<PipelineContractResponse>, ApiError> {
    let namespace = required_text(request.namespace, "namespace")?;
    let pipeline_ref = required_text(request.pipeline_ref, "pipeline_ref")?;
    let version = clean_optional_text(request.version).unwrap_or_else(|| "v1".to_string());
    validate_kubernetes_name("namespace", &namespace)?;
    validate_kubernetes_name("pipeline_ref", &pipeline_ref)?;
    validate_kubernetes_name("version", &version)?;
    let contract = pipeline_contract_spec(&request.contract_json)?;
    validate_pipeline_contract_spec(&contract)?;
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    let contract = state
        .store
        .create_pipeline_contract(CreatePipelineContract {
            id: format!("pcontract_{}", unique_suffix()),
            status: "active".to_string(),
            namespace,
            pipeline_ref,
            version,
            contract_json: request.contract_json,
            actor: actor.clone(),
            reason: reason.clone(),
        })
        .await?;
    append_pipeline_contract_audit_event(
        &state.store,
        &contract,
        "pipeline_contract.created",
        actor,
        reason,
    )
    .await?;
    Ok(Json(contract.into()))
}

async fn transition_pipeline_contract(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(pipeline_contract_id): Path<String>,
    Json(request): Json<TransitionPipelineContractRequest>,
) -> Result<Json<PipelineContractResponse>, ApiError> {
    let current = state
        .store
        .get_pipeline_contract(&pipeline_contract_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_contract", &pipeline_contract_id))?;
    let target = required_text(request.target_status, "target_status")?;
    if current.status != "active" || target != "retired" {
        return Err(ApiError::conflict(format!(
            "PipelineContract can only transition from active to retired, not {} to {}",
            current.status, target
        )));
    }
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    let contract = state
        .store
        .update_pipeline_contract_status(&current.id, "retired", actor.clone(), reason.clone())
        .await?;
    append_pipeline_contract_audit_event(
        &state.store,
        &contract,
        "pipeline_contract.retired",
        actor,
        reason,
    )
    .await?;
    Ok(Json(contract.into()))
}

async fn replace_pipeline_contract(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(pipeline_contract_id): Path<String>,
    Json(request): Json<ReplacePipelineContractRequest>,
) -> Result<Json<ReplacePipelineContractResponse>, ApiError> {
    let current = state
        .store
        .get_pipeline_contract(&pipeline_contract_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_contract", &pipeline_contract_id))?;
    if current.status != "active" {
        return Err(ApiError::conflict(
            "only an active PipelineContract can be replaced",
        ));
    }
    let version = required_text(request.version, "version")?;
    validate_kubernetes_name("version", &version)?;
    if version == current.version {
        return Err(ApiError::conflict(
            "replacement PipelineContract version must differ from the active version",
        ));
    }
    let contract_spec = pipeline_contract_spec(&request.contract_json)?;
    validate_pipeline_contract_spec(&contract_spec)?;
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    let (retired_contract, pipeline_contract) = state
        .store
        .replace_pipeline_contract(
            &current.id,
            ReplacePipelineContract {
                id: format!("pcontract_{}", unique_suffix()),
                namespace: current.namespace.clone(),
                pipeline_ref: current.pipeline_ref.clone(),
                version,
                contract_json: request.contract_json,
                actor: actor.clone(),
                reason: reason.clone(),
            },
        )
        .await?;
    append_pipeline_contract_audit_event(
        &state.store,
        &retired_contract,
        "pipeline_contract.replaced",
        actor.clone(),
        reason.clone(),
    )
    .await?;
    append_pipeline_contract_audit_event(
        &state.store,
        &pipeline_contract,
        "pipeline_contract.created_by_replacement",
        actor,
        reason,
    )
    .await?;
    Ok(Json(ReplacePipelineContractResponse {
        retired_contract: retired_contract.into(),
        pipeline_contract: pipeline_contract.into(),
    }))
}

async fn list_pipeline_intents(
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

async fn get_pipeline_intent(
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

async fn create_pipeline_intent_from_change_set(
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

    let actor = clean_optional_text(actor);
    let reason = clean_optional_text(reason);
    let draft = pipeline_intent_draft(
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

struct PipelineIntentDraftRequest {
    title: Option<String>,
    summary: Option<String>,
    risk_level: Option<String>,
    intent_kind: Option<String>,
    intent_json: Option<serde_json::Value>,
    actor: Option<String>,
    reason: Option<String>,
}

fn pipeline_intent_draft(
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

async fn transition_pipeline_intent(
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

async fn attach_pipeline_intent_evidence(
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

async fn create_pipeline_intent_trusted_envelope(
    State(state): State<AppState>,
    Path(pipeline_intent_id): Path<String>,
    Json(request): Json<CreatePipelineIntentTrustedEnvelopeRequest>,
) -> Result<Json<TrustedEnvelopeResponse>, ApiError> {
    let intent = state
        .store
        .get_pipeline_intent(&pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", &pipeline_intent_id))?;
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
    let reason = clean_optional_text(Some(request.reason.clone()))
        .ok_or_else(|| ApiError::bad_request("trusted envelope reason is required"))?;
    let subject =
        clean_optional_text(request.subject).unwrap_or_else(|| state.policy.subject.clone());
    let grant = create_permission_grant_record(
        &state.store,
        CreatePermissionGrantRequest {
            subject,
            created_by: clean_optional_text(request.created_by.clone()),
            reason: reason.clone(),
            scope: json!({
                "environment": state.policy.environment,
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
            expires_at: request.expires_at,
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

async fn execute_pipeline_intent(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(pipeline_intent_id): Path<String>,
    Json(request): Json<ExecutePipelineIntentRequest>,
) -> Result<Json<ExecutePipelineIntentResponse>, ApiError> {
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor.clone()));
    let reason = clean_optional_text(request.reason.clone());
    let preflight = pipeline_intent_execution_preflight(&state, &pipeline_intent_id).await?;
    if !preflight.ready || request.dry_run {
        return Ok(Json(ExecutePipelineIntentResponse {
            status: if preflight.ready { "ready" } else { "blocked" }.to_string(),
            ready: preflight.ready,
            dry_run: request.dry_run,
            pipeline_intent: preflight.intent.into(),
            manifest: preflight.manifest,
            checks: preflight.checks,
            permission_grant_id: preflight.grant_id,
            execution_id: None,
            executor_job_name: None,
        }));
    }

    let execution_id = format!("pexec_{}", unique_suffix());
    let mut intent_json = preflight.intent.intent_json.clone();
    let manifest = preflight
        .manifest
        .clone()
        .ok_or_else(|| ApiError::internal("execution preflight omitted a PipelineRun manifest"))?;
    set_pipeline_execution_state(
        &mut intent_json,
        json!({
            "execution_id": execution_id,
            "state": "dispatching",
            "pipeline_run_namespace": preflight.execution.namespace,
            "pipeline_run_name": pipeline_run_name(&manifest),
            "permission_grant_id": preflight.grant_id,
        }),
    );
    let intent = state
        .store
        .update_pipeline_intent_execution(
            &preflight.intent.id,
            UpdatePipelineIntentExecution {
                status: "executing".to_string(),
                intent_json,
                actor: actor.clone(),
                reason: reason.clone(),
            },
        )
        .await?;

    let dispatch = state
        .worker
        .dispatch_tekton_execution(TektonExecutionRequest {
            pipeline_intent_id: intent.id.clone(),
            execution_id: execution_id.clone(),
            target_namespace: preflight.execution.namespace.clone(),
            pipeline_run_manifest: manifest.clone(),
        })
        .await;
    let (intent, status, executor_job_name) = match dispatch {
        Ok(receipt) => {
            let mut intent_json = intent.intent_json.clone();
            set_pipeline_execution_state(
                &mut intent_json,
                json!({
                    "execution_id": execution_id,
                    "state": "executor_job_created",
                    "executor_job_name": receipt.job_name,
                    "pipeline_run_namespace": preflight.execution.namespace,
                    "pipeline_run_name": pipeline_run_name(&manifest),
                    "permission_grant_id": preflight.grant_id,
                }),
            );
            let intent = state
                .store
                .update_pipeline_intent_execution(
                    &intent.id,
                    UpdatePipelineIntentExecution {
                        status: "executing".to_string(),
                        intent_json,
                        actor: actor.clone(),
                        reason: reason.clone(),
                    },
                )
                .await?;
            append_pipeline_intent_audit_event(
                &state.store,
                &intent,
                "pipeline_intent.execution_dispatched",
                actor.clone(),
                reason.clone(),
                json!({
                    "execution_id": execution_id,
                    "executor_job_name": receipt.job_name,
                    "permission_grant_id": preflight.grant_id,
                }),
            )
            .await?;
            (intent, "dispatched".to_string(), Some(receipt.job_name))
        }
        Err(error) => {
            let mut intent_json = intent.intent_json.clone();
            set_pipeline_execution_state(
                &mut intent_json,
                json!({
                    "execution_id": execution_id,
                    "state": "dispatch_failed",
                    "error": error.to_string(),
                    "pipeline_run_namespace": preflight.execution.namespace,
                    "pipeline_run_name": pipeline_run_name(&manifest),
                    "permission_grant_id": preflight.grant_id,
                }),
            );
            let intent = state
                .store
                .update_pipeline_intent_execution(
                    &intent.id,
                    UpdatePipelineIntentExecution {
                        status: "failed".to_string(),
                        intent_json,
                        actor: actor.clone(),
                        reason: reason.clone(),
                    },
                )
                .await?;
            append_pipeline_intent_audit_event(
                &state.store,
                &intent,
                "pipeline_intent.execution_dispatch_failed",
                actor.clone(),
                reason.clone(),
                json!({ "execution_id": execution_id, "error": error.to_string() }),
            )
            .await?;
            (intent, "failed".to_string(), None)
        }
    };

    Ok(Json(ExecutePipelineIntentResponse {
        status,
        ready: true,
        dry_run: false,
        pipeline_intent: intent.into(),
        manifest: Some(manifest),
        checks: preflight.checks,
        permission_grant_id: preflight.grant_id,
        execution_id: Some(execution_id),
        executor_job_name,
    }))
}

async fn internal_pipeline_intent_execution_outcome(
    State(state): State<AppState>,
    Path(pipeline_intent_id): Path<String>,
    Json(request): Json<PipelineIntentExecutionOutcomeRequest>,
) -> Result<Json<PipelineIntentResponse>, ApiError> {
    let intent = state
        .store
        .get_pipeline_intent(&pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", &pipeline_intent_id))?;
    if intent.status != "executing" {
        return Err(ApiError::conflict(
            "execution outcome requires a PipelineIntent in executing status",
        ));
    }
    let current_execution_id = intent
        .intent_json
        .pointer("/execution_state/execution_id")
        .and_then(Value::as_str);
    if current_execution_id != Some(request.execution_id.as_str()) {
        return Err(ApiError::conflict(
            "execution outcome does not match the current PipelineIntent execution",
        ));
    }
    let (status, event_kind, state_name) = match request.status.as_str() {
        "submitted" => (
            "executing",
            "pipeline_intent.execution_submitted",
            "pipeline_run_created",
        ),
        "completed" => (
            "approved",
            "pipeline_intent.execution_completed",
            "pipeline_run_succeeded",
        ),
        "failed" => (
            "failed",
            "pipeline_intent.execution_failed",
            if intent
                .intent_json
                .pointer("/execution_state/state")
                .and_then(Value::as_str)
                == Some("pipeline_run_created")
            {
                "pipeline_run_failed"
            } else {
                "failed"
            },
        ),
        _ => {
            return Err(ApiError::bad_request(
                "execution outcome status must be submitted, completed, or failed",
            ))
        }
    };
    let terminal_evidence = if matches!(request.status.as_str(), "completed" | "failed") {
        Some(
            persist_pipeline_execution_evidence(&state.store, &intent, &request, state_name)
                .await?,
        )
    } else {
        None
    };
    let mut intent_json = intent.intent_json.clone();
    merge_pipeline_execution_state(
        &mut intent_json,
        json!({
            "execution_id": request.execution_id,
            "state": state_name,
            "pipeline_run_namespace": request.pipeline_run_namespace,
            "pipeline_run_name": request.pipeline_run_name,
            "error": request.error,
        }),
    );
    if let Some(evidence) = terminal_evidence {
        set_pipeline_execution_evidence(&mut intent_json, evidence);
    }
    let intent = state
        .store
        .update_pipeline_intent_execution(
            &intent.id,
            UpdatePipelineIntentExecution {
                status: status.to_string(),
                intent_json,
                actor: Some("executor:tekton".to_string()),
                reason: request.error.clone(),
            },
        )
        .await?;
    append_pipeline_intent_audit_event(
        &state.store,
        &intent,
        event_kind,
        Some("executor:tekton".to_string()),
        None,
        json!({
            "execution_id": request.execution_id,
            "pipeline_run_namespace": request.pipeline_run_namespace,
            "pipeline_run_name": request.pipeline_run_name,
            "error": request.error,
        }),
    )
    .await?;
    Ok(Json(intent.into()))
}

fn validate_pipeline_intent_observation(
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

fn pipeline_intent_json_with_evidence(
    current: &StoredPipelineIntent,
    observation: &StoredObservation,
) -> Value {
    let mut intent_json = current.intent_json.clone();
    let evidence = pipeline_intent_evidence_json(observation);
    if let Some(object) = intent_json.as_object_mut() {
        object.insert("evidence".to_string(), evidence);
    }

    intent_json
}

fn pipeline_intent_evidence_json(observation: &StoredObservation) -> Value {
    let analysis = observation
        .data_json
        .get("analysis")
        .cloned()
        .unwrap_or_else(|| json!({}));
    json!({
        "status": pipeline_intent_evidence_status(&analysis),
        "source": "observation",
        "observation_id": observation.id,
        "artifact_id": observation.artifact_id,
        "kind": observation.kind,
        "resource": {
            "namespace": observation.resource_namespace,
            "kind": observation.resource_kind,
            "name": observation.resource_name,
        },
        "summary": {
            "pipeline_run_status": analysis.pointer("/summary/status"),
            "pipeline_run_reason": analysis.pointer("/summary/reason"),
            "task_run_count": analysis.pointer("/summary/task_run_count"),
            "failed_task_run_count": analysis.pointer("/summary/failed_task_run_count"),
            "running_task_run_count": analysis.pointer("/summary/running_task_run_count"),
            "succeeded_task_run_count": analysis.pointer("/summary/succeeded_task_run_count"),
            "argo_sync_status": analysis.pointer("/summary/argo_sync_status"),
            "argo_health_status": analysis.pointer("/summary/argo_health_status"),
            "image_alignment_status": analysis.pointer("/summary/image_alignment/status"),
        }
    })
}

fn pipeline_intent_evidence_status(analysis: &Value) -> &'static str {
    match analysis.pointer("/summary/status").and_then(Value::as_str) {
        Some("succeeded") => {
            let failed_tasks = analysis
                .pointer("/summary/failed_task_run_count")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            if failed_tasks != 0 || pipeline_analysis_needs_attention(analysis) {
                "attention_required"
            } else {
                "satisfied"
            }
        }
        Some("running") => "running",
        Some("failed" | "cancelled") => "failed",
        Some(_) => "attention_required",
        None => "unknown",
    }
}

fn pipeline_analysis_needs_attention(analysis: &Value) -> bool {
    let argo_sync = analysis
        .pointer("/summary/argo_sync_status")
        .and_then(Value::as_str);
    if argo_sync.is_some_and(|status| status != "Synced") {
        return true;
    }

    let argo_health = analysis
        .pointer("/summary/argo_health_status")
        .and_then(Value::as_str);
    if argo_health.is_some_and(|status| status != "Healthy") {
        return true;
    }

    let image_alignment = analysis
        .pointer("/summary/image_alignment/status")
        .and_then(Value::as_str);
    image_alignment
        .is_some_and(|status| !matches!(status, "exact_match" | "registry_alias_match" | "unknown"))
}

fn pipeline_intent_attached_evidence_status(
    pipeline_intent: &StoredPipelineIntent,
) -> Option<&str> {
    pipeline_intent
        .intent_json
        .pointer("/evidence/status")
        .and_then(Value::as_str)
}

fn pipeline_execution_evidence_status(pipeline_intent: &StoredPipelineIntent) -> Option<&str> {
    pipeline_intent
        .intent_json
        .pointer("/execution_evidence/status")
        .and_then(Value::as_str)
}

fn deployment_intent_attached_evidence_status(
    deployment_intent: &StoredDeploymentIntent,
) -> Option<&str> {
    deployment_intent
        .intent_json
        .pointer("/deployment_evidence/status")
        .and_then(Value::as_str)
}

fn release_observability_evidence_status(release: &StoredRelease) -> Option<&str> {
    let evidence = release
        .release_json
        .pointer("/observability_evidence")
        .and_then(Value::as_array)?;
    if evidence.is_empty() {
        return None;
    }
    if evidence.iter().any(|item| {
        item.get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| status == "attention_required")
    }) {
        return Some("attention_required");
    }
    if evidence.iter().any(|item| {
        item.get("status")
            .and_then(Value::as_str)
            .map_or(true, |status| status == "unknown")
    }) {
        return Some("unknown");
    }
    Some("observed")
}

fn pipeline_intent_json(
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

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TektonExecutionSpec {
    enabled: bool,
    namespace: String,
    pipeline_ref: String,
    #[serde(default)]
    production_impacting: bool,
    #[serde(default)]
    params: BTreeMap<String, Value>,
    #[serde(default)]
    workspaces: Vec<TektonWorkspaceSpec>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TektonWorkspaceSpec {
    name: String,
    #[serde(default)]
    persistent_volume_claim: Option<String>,
    #[serde(default)]
    volume_claim_template: Option<TektonVolumeClaimTemplate>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TektonVolumeClaimTemplate {
    storage: String,
    #[serde(default = "default_access_modes")]
    access_modes: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PipelineContractSpec {
    #[serde(default)]
    params: Vec<PipelineParameterContract>,
    #[serde(default)]
    workspaces: Vec<PipelineWorkspaceContract>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PipelineParameterContract {
    name: String,
    #[serde(rename = "type")]
    value_type: String,
    #[serde(default)]
    required: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PipelineWorkspaceContract {
    name: String,
    binding: String,
    #[serde(default)]
    required: bool,
}

fn default_access_modes() -> Vec<String> {
    vec!["ReadWriteOnce".to_string()]
}

struct PipelineIntentExecutionPreflight {
    ready: bool,
    intent: StoredPipelineIntent,
    execution: TektonExecutionSpec,
    manifest: Option<Value>,
    checks: Vec<Value>,
    grant_id: Option<String>,
}

async fn pipeline_intent_execution_preflight(
    state: &AppState,
    pipeline_intent_id: &str,
) -> Result<PipelineIntentExecutionPreflight, ApiError> {
    let intent = state
        .store
        .get_pipeline_intent(pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", pipeline_intent_id))?;
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
    let execution = tekton_execution_spec(&intent.intent_json)?;
    let mut checks = vec![
        execution_check(
            "pipeline_intent_approved",
            intent.status == "approved",
            format!("PipelineIntent status is {}", intent.status),
        ),
        execution_check(
            "change_set_approved",
            change_set.status == "approved",
            format!("ChangeSet status is {}", change_set.status),
        ),
        execution_check(
            "work_plan_approved",
            work_plan.status == "approved",
            format!("WorkPlan status is {}", work_plan.status),
        ),
        execution_check(
            "execution_enabled",
            execution.enabled,
            "Tekton execution is enabled",
        ),
    ];

    let contracts = state
        .store
        .list_pipeline_contracts(PipelineContractListFilter {
            namespace: Some(execution.namespace.clone()),
            pipeline_ref: Some(execution.pipeline_ref.clone()),
            status: Some("active".to_string()),
            limit: 10,
            ..PipelineContractListFilter::default()
        })
        .await?;
    let matching_contract_count = if contracts.is_empty() {
        state
            .store
            .list_pipeline_contracts(PipelineContractListFilter {
                namespace: Some(execution.namespace.clone()),
                pipeline_ref: Some(execution.pipeline_ref.clone()),
                limit: 10,
                ..PipelineContractListFilter::default()
            })
            .await?
            .len()
    } else {
        contracts.len()
    };
    let contract = match contracts.as_slice() {
        [] => {
            checks.push(execution_check(
                "active_pipeline_contract",
                false,
                if matching_contract_count == 0 {
                    format!(
                        "No PipelineContract exists for {}/{}",
                        execution.namespace, execution.pipeline_ref
                    )
                } else {
                    format!(
                        "All PipelineContracts for {}/{} are retired",
                        execution.namespace, execution.pipeline_ref
                    )
                },
            ));
            None
        }
        [contract] => {
            checks.push(execution_check(
                "active_pipeline_contract",
                true,
                format!(
                    "Active PipelineContract {} version {} matches",
                    contract.id, contract.version
                ),
            ));
            Some(contract)
        }
        _ => {
            checks.push(execution_check(
                "active_pipeline_contract",
                false,
                format!(
                    "Multiple active PipelineContracts match {}/{}; retire the older contract",
                    execution.namespace, execution.pipeline_ref
                ),
            ));
            None
        }
    };
    if let Some(contract) = contract {
        match execution_matches_pipeline_contract(&execution, contract) {
            Ok(()) => checks.push(execution_check(
                "pipeline_contract_inputs",
                true,
                format!(
                    "PipelineIntent inputs match PipelineContract {}",
                    contract.id
                ),
            )),
            Err(error) => checks.push(execution_check(
                "pipeline_contract_inputs",
                false,
                error.message,
            )),
        }
    } else {
        checks.push(execution_check(
            "pipeline_contract_inputs",
            false,
            "PipelineIntent inputs cannot be validated without one active PipelineContract",
        ));
    }

    let gates = state
        .store
        .list_approval_gates(ApprovalGateListFilter {
            remediation_plan_id: Some(intent.remediation_plan_id.clone()),
            limit: 200,
            ..ApprovalGateListFilter::default()
        })
        .await?;
    let required_kinds = if execution.production_impacting {
        ["pipeline_mutation", "cluster_mutation", "production_impact"].as_slice()
    } else {
        ["pipeline_mutation", "cluster_mutation"].as_slice()
    };
    for kind in required_kinds {
        let matching = gates
            .iter()
            .filter(|gate| gate.gate_kind == *kind)
            .collect::<Vec<_>>();
        let satisfied = !matching.is_empty()
            && matching
                .iter()
                .all(|gate| matches!(gate.status.as_str(), "satisfied" | "waived"));
        checks.push(execution_check(
            format!("approval_gate_{kind}"),
            satisfied,
            if matching.is_empty() {
                format!("Required {kind} approval gate is missing")
            } else {
                format!("{} {kind} gate(s) are satisfied or waived", matching.len())
            },
        ));
    }
    for gate in gates
        .iter()
        .filter(|gate| !required_kinds.contains(&gate.gate_kind.as_str()))
    {
        checks.push(execution_check(
            format!("approval_gate_{}", gate.id),
            matches!(gate.status.as_str(), "satisfied" | "waived"),
            format!("{} gate is {}", gate.gate_kind, gate.status),
        ));
    }

    let grant =
        matching_pipeline_execution_grant(&state.store, &state.policy, &intent, &execution).await?;
    checks.push(execution_check(
        "trusted_execution_envelope",
        grant.is_some(),
        grant
            .as_ref()
            .map(|grant| {
                format!(
                    "Active supervised-autonomy grant {} matches the PipelineIntent",
                    grant.id
                )
            })
            .unwrap_or_else(|| {
                "No active supervised-autonomy grant matches this PipelineIntent".to_string()
            }),
    ));
    let ready = checks
        .iter()
        .all(|check| check.get("passed").and_then(Value::as_bool) == Some(true));
    let manifest = ready
        .then(|| build_pipeline_run_manifest(&intent, &execution))
        .transpose()?;
    Ok(PipelineIntentExecutionPreflight {
        ready,
        intent,
        execution,
        manifest,
        checks,
        grant_id: grant.map(|grant| grant.id),
    })
}

fn execution_check(code: impl Into<String>, passed: bool, summary: impl Into<String>) -> Value {
    json!({ "code": code.into(), "passed": passed, "summary": summary.into() })
}

async fn matching_pipeline_execution_grant(
    store: &SqliteStore,
    policy: &SafetyPolicy,
    intent: &StoredPipelineIntent,
    execution: &TektonExecutionSpec,
) -> Result<Option<StoredPermissionGrant>, ApiError> {
    let now = unique_suffix();
    for grant in store.list_permission_grants(Some("active"), 200).await? {
        if !grant_is_unexpired(&grant, now) {
            continue;
        }
        let scope = serde_json::from_value::<PermissionGrantScope>(grant.scope_json.clone())
            .map_err(|error| {
                ApiError::internal(format!(
                    "permission grant {} has invalid scope: {error}",
                    grant.id
                ))
            })?;
        let grant_policy = serde_json::from_value::<PermissionGrantPolicy>(
            grant.policy_json.clone(),
        )
        .map_err(|error| {
            ApiError::internal(format!(
                "permission grant {} has invalid policy: {error}",
                grant.id
            ))
        })?;
        let matches = grant.subject == policy.subject
            && scope.environment.as_deref() == Some(policy.environment.as_str())
            && grant_policy.policy_mode == PolicyMode::SupervisedAutonomy
            && scope
                .capability_kinds
                .contains(&CapabilityKind::TektonStartRun)
            && scope
                .actions
                .iter()
                .any(|action| action == "tekton_trigger_pipeline")
            && scope
                .max_risk
                .is_some_and(|risk| risk_rank(risk) >= risk_rank(RiskLevel::High))
            && scope
                .namespaces
                .iter()
                .any(|namespace| namespace == &execution.namespace)
            && scope
                .work_plan_ids
                .iter()
                .any(|id| id == &intent.work_plan_id)
            && scope
                .change_set_ids
                .iter()
                .any(|id| id == &intent.change_set_id)
            && scope.pipeline_intent_ids.iter().any(|id| id == &intent.id)
            && scope.production_impacting == Some(execution.production_impacting);
        if matches {
            return Ok(Some(grant));
        }
    }
    Ok(None)
}

fn risk_rank(risk: RiskLevel) -> u8 {
    match risk {
        RiskLevel::Low => 1,
        RiskLevel::Medium => 2,
        RiskLevel::High => 3,
        RiskLevel::Critical => 4,
    }
}

fn tekton_execution_spec(intent_json: &Value) -> Result<TektonExecutionSpec, ApiError> {
    let execution = intent_json
        .get("execution")
        .cloned()
        .ok_or_else(|| ApiError::bad_request("pipeline intent execution is required"))?;
    let execution = serde_json::from_value::<TektonExecutionSpec>(execution).map_err(|error| {
        ApiError::bad_request(format!("pipeline intent execution is invalid: {error}"))
    })?;
    validate_tekton_execution_spec(&execution)?;
    Ok(execution)
}

fn pipeline_contract_spec(value: &Value) -> Result<PipelineContractSpec, ApiError> {
    if !value.is_object() {
        return Err(ApiError::bad_request(
            "pipeline contract contract_json must be a JSON object",
        ));
    }
    serde_json::from_value::<PipelineContractSpec>(value.clone()).map_err(|error| {
        ApiError::bad_request(format!(
            "pipeline contract contract_json is invalid: {error}"
        ))
    })
}

fn validate_pipeline_contract_spec(contract: &PipelineContractSpec) -> Result<(), ApiError> {
    let mut names = BTreeSet::new();
    for parameter in &contract.params {
        validate_kubernetes_name("pipeline contract params.name", &parameter.name)?;
        if !matches!(parameter.value_type.as_str(), "scalar" | "array") {
            return Err(ApiError::bad_request(
                "pipeline contract params.type must be scalar or array",
            ));
        }
        if !names.insert(parameter.name.as_str()) {
            return Err(ApiError::bad_request(
                "pipeline contract params must not repeat a name",
            ));
        }
    }
    let mut workspace_names = BTreeSet::new();
    for workspace in &contract.workspaces {
        validate_kubernetes_name("pipeline contract workspaces.name", &workspace.name)?;
        if !matches!(
            workspace.binding.as_str(),
            "persistent_volume_claim" | "volume_claim_template"
        ) {
            return Err(ApiError::bad_request(
                "pipeline contract workspaces.binding must be persistent_volume_claim or volume_claim_template",
            ));
        }
        if !workspace_names.insert(workspace.name.as_str()) {
            return Err(ApiError::bad_request(
                "pipeline contract workspaces must not repeat a name",
            ));
        }
    }
    Ok(())
}

fn execution_matches_pipeline_contract(
    execution: &TektonExecutionSpec,
    stored: &StoredPipelineContract,
) -> Result<(), ApiError> {
    let contract = pipeline_contract_spec(&stored.contract_json)?;
    validate_pipeline_contract_spec(&contract)?;
    for parameter in &contract.params {
        let value = execution.params.get(&parameter.name);
        if parameter.required && value.is_none() {
            return Err(ApiError::bad_request(format!(
                "PipelineIntent is missing required pipeline parameter {}",
                parameter.name
            )));
        }
        if let Some(value) = value {
            let matches = match parameter.value_type.as_str() {
                "scalar" => !value.is_array() && !value.is_object() && !value.is_null(),
                "array" => value.is_array(),
                _ => false,
            };
            if !matches {
                return Err(ApiError::bad_request(format!(
                    "PipelineIntent parameter {} does not match contract type {}",
                    parameter.name, parameter.value_type
                )));
            }
        }
    }
    if let Some(parameter) = execution
        .params
        .keys()
        .find(|name| !contract.params.iter().any(|allowed| allowed.name == **name))
    {
        return Err(ApiError::bad_request(format!(
            "PipelineIntent parameter {parameter} is not declared by the active PipelineContract"
        )));
    }
    for workspace in &contract.workspaces {
        let supplied = execution
            .workspaces
            .iter()
            .find(|candidate| candidate.name == workspace.name);
        if workspace.required && supplied.is_none() {
            return Err(ApiError::bad_request(format!(
                "PipelineIntent is missing required pipeline workspace {}",
                workspace.name
            )));
        }
        if let Some(supplied) = supplied {
            let binding = if supplied.persistent_volume_claim.is_some() {
                "persistent_volume_claim"
            } else {
                "volume_claim_template"
            };
            if binding != workspace.binding {
                return Err(ApiError::bad_request(format!(
                    "PipelineIntent workspace {} requires {} binding",
                    workspace.name, workspace.binding
                )));
            }
        }
    }
    if let Some(workspace) = execution.workspaces.iter().find(|workspace| {
        !contract
            .workspaces
            .iter()
            .any(|allowed| allowed.name == workspace.name)
    }) {
        return Err(ApiError::bad_request(format!(
            "PipelineIntent workspace {} is not declared by the active PipelineContract",
            workspace.name
        )));
    }
    Ok(())
}

fn validate_tekton_execution_spec(execution: &TektonExecutionSpec) -> Result<(), ApiError> {
    validate_kubernetes_name("execution.namespace", &execution.namespace)?;
    validate_kubernetes_name("execution.pipeline_ref", &execution.pipeline_ref)?;
    for (name, value) in &execution.params {
        validate_kubernetes_name("execution.params key", name)?;
        if !(value.is_string() || value.is_number() || value.is_boolean() || value.is_array()) {
            return Err(ApiError::bad_request(
                "execution.params values must be scalar or arrays",
            ));
        }
    }
    for workspace in &execution.workspaces {
        validate_kubernetes_name("execution.workspaces.name", &workspace.name)?;
        match (&workspace.persistent_volume_claim, &workspace.volume_claim_template) {
            (Some(pvc), None) => validate_kubernetes_name("execution.workspaces.persistent_volume_claim", pvc)?,
            (None, Some(template)) => {
                if template.storage.trim().is_empty() {
                    return Err(ApiError::bad_request("execution.workspaces.volume_claim_template.storage is required"));
                }
                if template.access_modes.is_empty() || template.access_modes.iter().any(|mode| mode != "ReadWriteOnce") {
                    return Err(ApiError::bad_request("execution workspaces support only ReadWriteOnce volume claim templates"));
                }
            }
            _ => return Err(ApiError::bad_request("each execution workspace requires exactly one persistent_volume_claim or volume_claim_template")),
        }
    }
    Ok(())
}

fn validate_kubernetes_name(field: &str, value: &str) -> Result<(), ApiError> {
    let valid = !value.is_empty()
        && value.len() <= 63
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-');
    if valid {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "{field} must be a DNS label"
        )))
    }
}

fn build_pipeline_run_manifest(
    intent: &StoredPipelineIntent,
    execution: &TektonExecutionSpec,
) -> Result<Value, ApiError> {
    let intent_label = dns_label_fragment(&intent.id);
    let change_set_label = dns_label_fragment(&intent.change_set_id);
    let name = format!("pharness-{intent_label}");
    let params = execution
        .params
        .iter()
        .map(|(name, value)| json!({ "name": name, "value": value }))
        .collect::<Vec<_>>();
    let workspaces = execution
        .workspaces
        .iter()
        .map(|workspace| {
            let mut value = Map::new();
            value.insert("name".to_string(), json!(workspace.name));
            if let Some(pvc) = &workspace.persistent_volume_claim {
                value.insert(
                    "persistentVolumeClaim".to_string(),
                    json!({ "claimName": pvc }),
                );
            }
            if let Some(template) = &workspace.volume_claim_template {
                value.insert(
                    "volumeClaimTemplate".to_string(),
                    json!({
                        "spec": {
                            "accessModes": template.access_modes,
                            "resources": { "requests": { "storage": template.storage } },
                        }
                    }),
                );
            }
            Value::Object(value)
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "apiVersion": "tekton.dev/v1",
        "kind": "PipelineRun",
        "metadata": {
            "name": name,
            "namespace": execution.namespace,
            "labels": {
                "app.kubernetes.io/part-of": "pharness",
                "pharness.lucas.engineering/pipeline-intent": intent_label,
                "pharness.lucas.engineering/change-set": change_set_label,
            },
        },
        "spec": {
            "pipelineRef": { "name": execution.pipeline_ref },
            "params": params,
            "workspaces": workspaces,
        },
    }))
}

fn dns_label_fragment(value: &str) -> String {
    let normalized = value.replace('_', "-").to_ascii_lowercase();
    normalized.chars().take(50).collect()
}

fn set_pipeline_execution_state(intent_json: &mut Value, execution_state: Value) {
    if let Some(object) = intent_json.as_object_mut() {
        object.insert("execution_state".to_string(), execution_state);
    }
}

fn merge_pipeline_execution_state(intent_json: &mut Value, update: Value) {
    let Some(intent) = intent_json.as_object_mut() else {
        return;
    };
    let mut execution_state = intent
        .get("execution_state")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(update) = update.as_object() {
        for (key, value) in update {
            execution_state.insert(key.clone(), value.clone());
        }
    }
    intent.insert(
        "execution_state".to_string(),
        Value::Object(execution_state),
    );
}

fn set_pipeline_execution_evidence(intent_json: &mut Value, evidence: Value) {
    if let Some(intent) = intent_json.as_object_mut() {
        intent.insert("execution_evidence".to_string(), evidence);
    }
}

async fn persist_pipeline_execution_evidence(
    store: &SqliteStore,
    intent: &StoredPipelineIntent,
    outcome: &PipelineIntentExecutionOutcomeRequest,
    state_name: &str,
) -> Result<Value, ApiError> {
    let artifact_id = format!("art_pipeline_execution_{}", outcome.execution_id);
    let observation_id = format!("obs_pipeline_execution_{}", outcome.execution_id);
    let evidence_status = match outcome.status.as_str() {
        "completed" => "succeeded",
        "failed" => "failed",
        _ => {
            return Err(ApiError::internal(
                "terminal execution evidence requires a terminal outcome",
            ))
        }
    };
    let pipeline_run = json!({
        "namespace": outcome.pipeline_run_namespace,
        "name": outcome.pipeline_run_name,
    });
    let error = outcome
        .error
        .as_deref()
        .map(|value| truncate_audit_text(value, 256));
    let content = json!({
        "execution_id": outcome.execution_id,
        "status": evidence_status,
        "state": state_name,
        "pipeline_run": pipeline_run.clone(),
        "error": error.clone(),
    });
    let artifact = match store.get_artifact(&artifact_id).await? {
        Some(existing) => existing,
        None => {
            store
                .create_artifact(CreateArtifact {
                    id: artifact_id.clone(),
                    session_id: intent.session_id.clone(),
                    run_id: intent.run_id.clone(),
                    kind: "tekton_pipeline_run_execution".to_string(),
                    label: format!(
                        "Tekton PipelineRun {evidence_status}: {}",
                        outcome.execution_id
                    ),
                    mime_type: Some("application/json".to_string()),
                    path: None,
                    content_text: None,
                    content_json: Some(content.clone()),
                })
                .await?
        }
    };
    let observation = match store.get_observation(&observation_id).await? {
        Some(existing) => existing,
        None => {
            let namespace = outcome.pipeline_run_namespace.clone();
            let name = outcome.pipeline_run_name.clone();
            store
                .create_observation(CreateObservation {
                    id: observation_id.clone(),
                    session_id: intent.session_id.clone(),
                    run_id: intent.run_id.clone(),
                    source: "tekton".to_string(),
                    kind: "pipeline_run_execution".to_string(),
                    subject: name.clone().unwrap_or_else(|| outcome.execution_id.clone()),
                    summary: format!(
                        "PipelineRun execution {evidence_status} for {}",
                        name.as_deref().unwrap_or(&outcome.execution_id)
                    ),
                    resource_namespace: namespace.clone(),
                    resource_kind: Some("PipelineRun".to_string()),
                    resource_name: name.clone(),
                    resource_ref_json: Some(json!({
                        "apiVersion": "tekton.dev/v1",
                        "kind": "PipelineRun",
                        "namespace": namespace,
                        "name": name,
                    })),
                    artifact_id: Some(artifact.id.clone()),
                    data_json: json!({ "execution": content }),
                })
                .await?
        }
    };

    Ok(json!({
        "status": evidence_status,
        "source": "executor",
        "execution_id": outcome.execution_id,
        "artifact_id": artifact.id,
        "observation_id": observation.id,
        "pipeline_run": pipeline_run,
        "error": error,
    }))
}

fn pipeline_run_name(manifest: &Value) -> Option<&str> {
    manifest.pointer("/metadata/name").and_then(Value::as_str)
}

fn validate_pipeline_intent_transition(current: &str, target: &str) -> Result<(), ApiError> {
    match (current, target) {
        ("proposed", "approved" | "rejected") => Ok(()),
        ("approved", "rejected") => Ok(()),
        (_, "proposed") if current == target => Ok(()),
        _ => Err(ApiError::conflict(format!(
            "cannot transition pipeline intent from {current} to {target}"
        ))),
    }
}

async fn list_deployment_intents(
    State(state): State<AppState>,
    Query(query): Query<ListDeploymentIntentsQuery>,
) -> Result<Json<DeploymentIntentsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let deployment_intents = state
        .store
        .list_deployment_intents(DeploymentIntentListFilter {
            pipeline_intent_id: clean_optional_text(query.pipeline_intent_id),
            change_set_id: clean_optional_text(query.change_set_id),
            work_plan_id: clean_optional_text(query.work_plan_id),
            remediation_plan_id: clean_optional_text(query.remediation_plan_id),
            incident_id: clean_optional_text(query.incident_id),
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            status: clean_optional_text(query.status),
            intent_kind: clean_optional_text(query.intent_kind),
            risk_level: clean_optional_text(query.risk_level),
            target_environment: clean_optional_text(query.target_environment),
            target_namespace: clean_optional_text(query.target_namespace),
            argo_application: clean_optional_text(query.argo_application),
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
    let count = deployment_intents.len();

    Ok(Json(DeploymentIntentsResponse {
        deployment_intents,
        count,
        limit,
        offset,
    }))
}

async fn get_deployment_intent(
    State(state): State<AppState>,
    Path(deployment_intent_id): Path<String>,
) -> Result<Json<DeploymentIntentResponse>, ApiError> {
    let intent = state
        .store
        .get_deployment_intent(&deployment_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_intent", &deployment_intent_id))?;

    Ok(Json(intent.into()))
}

async fn create_deployment_intent_from_pipeline_intent(
    State(state): State<AppState>,
    Json(request): Json<CreateDeploymentIntentFromPipelineIntentRequest>,
) -> Result<Json<CreateDeploymentIntentResponse>, ApiError> {
    let pipeline_intent_id = clean_optional_text(Some(request.pipeline_intent_id))
        .ok_or_else(|| ApiError::bad_request("pipeline_intent_id is required"))?;
    let pipeline_intent = state
        .store
        .get_pipeline_intent(&pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", &pipeline_intent_id))?;
    ensure_pipeline_intent_ready_for_deployment(&pipeline_intent)?;

    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let intent_kind =
        clean_optional_text(request.intent_kind).unwrap_or_else(|| "argo_sync_deploy".to_string());
    let target_environment = clean_optional_text(request.target_environment);
    let target_namespace = clean_optional_text(request.target_namespace)
        .or(pipeline_intent.resource_namespace.clone());
    let argo_application =
        clean_optional_text(request.argo_application).or(pipeline_intent.resource_name.clone());
    let intent_json = deployment_intent_json(
        &pipeline_intent,
        &intent_kind,
        target_environment.as_deref(),
        target_namespace.as_deref(),
        argo_application.as_deref(),
        request.intent_json,
    )?;
    if let Some(existing) = state
        .store
        .get_deployment_intent_by_pipeline_intent(&pipeline_intent_id)
        .await?
    {
        if existing.status == "stale" {
            let deployment_intent = state
                .store
                .revise_deployment_intent_draft(
                    &existing.id,
                    UpdateDeploymentIntentDraft {
                        title: clean_optional_text(request.title).unwrap_or_else(|| {
                            format!("DeploymentIntent: {}", pipeline_intent.title)
                        }),
                        summary: clean_optional_text(request.summary).unwrap_or_else(|| {
                            "Propose Argo CD sync/deploy after approved pipeline intent".to_string()
                        }),
                        risk_level: clean_optional_text(request.risk_level)
                            .unwrap_or_else(|| pipeline_intent.risk_level.clone()),
                        intent_kind,
                        target_environment,
                        target_namespace,
                        argo_application,
                        resource_namespace: pipeline_intent.resource_namespace,
                        resource_kind: pipeline_intent.resource_kind,
                        resource_name: pipeline_intent.resource_name,
                        intent_json,
                        actor: actor.clone(),
                        reason: reason.clone(),
                    },
                )
                .await?;
            append_deployment_intent_audit_event(
                &state.store,
                &deployment_intent,
                "deployment_intent.reproposed",
                actor,
                reason,
                json!({
                    "source": "pipeline_intent",
                    "pipeline_intent_id": deployment_intent.pipeline_intent_id,
                    "previous_status": existing.status,
                    "execution_enabled": false,
                    "pipeline_evidence_status": deployment_intent
                        .intent_json
                        .pointer("/pipeline_evidence/status"),
                    "pipeline_deploy_ready": deployment_intent
                        .intent_json
                        .pointer("/pipeline_evidence/deploy_ready"),
                }),
            )
            .await?;

            return Ok(Json(CreateDeploymentIntentResponse {
                deployment_intent: deployment_intent.into(),
                created: false,
            }));
        }

        return Ok(Json(CreateDeploymentIntentResponse {
            deployment_intent: existing.into(),
            created: false,
        }));
    }
    let deployment_intent = state
        .store
        .create_deployment_intent(CreateDeploymentIntent {
            id: format!("dint_{}", unique_suffix()),
            pipeline_intent_id: pipeline_intent.id.clone(),
            change_set_id: pipeline_intent.change_set_id.clone(),
            work_plan_id: pipeline_intent.work_plan_id.clone(),
            remediation_plan_id: pipeline_intent.remediation_plan_id.clone(),
            incident_id: pipeline_intent.incident_id.clone(),
            session_id: pipeline_intent.session_id.clone(),
            run_id: pipeline_intent.run_id.clone(),
            status: "proposed".to_string(),
            title: clean_optional_text(request.title)
                .unwrap_or_else(|| format!("DeploymentIntent: {}", pipeline_intent.title)),
            summary: clean_optional_text(request.summary).unwrap_or_else(|| {
                "Propose Argo CD sync/deploy after approved pipeline intent".to_string()
            }),
            risk_level: clean_optional_text(request.risk_level)
                .unwrap_or(pipeline_intent.risk_level),
            intent_kind,
            target_environment,
            target_namespace,
            argo_application,
            resource_namespace: pipeline_intent.resource_namespace,
            resource_kind: pipeline_intent.resource_kind,
            resource_name: pipeline_intent.resource_name,
            intent_json,
        })
        .await?;
    append_deployment_intent_audit_event(
        &state.store,
        &deployment_intent,
        "deployment_intent.proposed",
        actor,
        reason,
        json!({
            "source": "pipeline_intent",
            "pipeline_intent_id": deployment_intent.pipeline_intent_id,
            "execution_enabled": false,
            "pipeline_evidence_status": deployment_intent
                .intent_json
                .pointer("/pipeline_evidence/status"),
            "pipeline_deploy_ready": deployment_intent
                .intent_json
                .pointer("/pipeline_evidence/deploy_ready"),
        }),
    )
    .await?;

    Ok(Json(CreateDeploymentIntentResponse {
        deployment_intent: deployment_intent.into(),
        created: true,
    }))
}

async fn transition_deployment_intent(
    State(state): State<AppState>,
    Path(deployment_intent_id): Path<String>,
    Json(request): Json<TransitionDeploymentIntentRequest>,
) -> Result<Json<TransitionDeploymentIntentResponse>, ApiError> {
    let current = state
        .store
        .get_deployment_intent(&deployment_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_intent", &deployment_intent_id))?;
    let target = clean_optional_text(Some(request.target_status))
        .ok_or_else(|| ApiError::bad_request("target_status is required"))?;
    validate_deployment_intent_transition(&current.status, &target)?;
    if target == "approved" {
        let pipeline_intent = state
            .store
            .get_pipeline_intent(&current.pipeline_intent_id)
            .await?
            .ok_or_else(|| ApiError::not_found("pipeline_intent", &current.pipeline_intent_id))?;
        ensure_pipeline_evidence_ready_for_deployment(&pipeline_intent)?;
    }
    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let deployment_intent = state
        .store
        .update_deployment_intent_status(
            &deployment_intent_id,
            &target,
            actor.clone(),
            reason.clone(),
        )
        .await?;
    append_deployment_intent_audit_event(
        &state.store,
        &deployment_intent,
        &format!("deployment_intent.{target}"),
        actor,
        reason,
        json!({
            "previous_status": current.status,
            "status": deployment_intent.status,
        }),
    )
    .await?;

    Ok(Json(TransitionDeploymentIntentResponse {
        deployment_intent: deployment_intent.into(),
    }))
}

async fn attach_deployment_intent_evidence(
    State(state): State<AppState>,
    Path(deployment_intent_id): Path<String>,
    Json(request): Json<AttachDeploymentIntentEvidenceRequest>,
) -> Result<Json<AttachDeploymentIntentEvidenceResponse>, ApiError> {
    let current = state
        .store
        .get_deployment_intent(&deployment_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_intent", &deployment_intent_id))?;
    if current.status == "stale" {
        return Err(ApiError::conflict(format!(
            "cannot attach evidence to stale deployment intent {deployment_intent_id}"
        )));
    }

    let observation_id = clean_optional_text(Some(request.observation_id))
        .ok_or_else(|| ApiError::bad_request("observation_id is required"))?;
    let observation = state
        .store
        .get_observation(&observation_id)
        .await?
        .ok_or_else(|| ApiError::not_found("observation", &observation_id))?;
    validate_deployment_intent_observation(&observation)?;

    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let intent_json = deployment_intent_json_with_evidence(&current, &observation);
    let deployment_intent = state
        .store
        .update_deployment_intent_evidence(
            &deployment_intent_id,
            UpdateDeploymentIntentEvidence {
                intent_json,
                actor: actor.clone(),
                reason: reason.clone(),
            },
        )
        .await?;
    append_deployment_intent_audit_event(
        &state.store,
        &deployment_intent,
        "deployment_intent.evidence_attached",
        actor,
        reason,
        json!({
            "observation_id": observation.id,
            "artifact_id": observation.artifact_id,
            "evidence_status": deployment_intent.intent_json.pointer("/deployment_evidence/status"),
            "deploy_ready": deployment_intent.intent_json.pointer("/deployment_evidence/deploy_ready"),
            "resource": {
                "namespace": observation.resource_namespace,
                "kind": observation.resource_kind,
                "name": observation.resource_name,
            },
        }),
    )
    .await?;

    Ok(Json(AttachDeploymentIntentEvidenceResponse {
        deployment_intent: deployment_intent.into(),
        observation: observation.into(),
    }))
}

fn validate_deployment_intent_observation(observation: &StoredObservation) -> Result<(), ApiError> {
    if observation.source != "argocd" {
        return Err(ApiError::bad_request(
            "deployment intent evidence must be an argocd Application observation",
        ));
    }

    let looks_like_application = observation.kind == "applications.argoproj.io"
        || observation.resource_kind.as_deref() == Some("Application")
        || observation
            .data_json
            .pointer("/output/kind")
            .and_then(Value::as_str)
            == Some("Application");
    if !looks_like_application {
        return Err(ApiError::bad_request(
            "deployment intent evidence must describe an Argo CD Application",
        ));
    }
    if observation.data_json.pointer("/output/status").is_none() {
        return Err(ApiError::bad_request(
            "deployment intent evidence observation is missing Argo Application status",
        ));
    }

    Ok(())
}

fn deployment_intent_json_with_evidence(
    current: &StoredDeploymentIntent,
    observation: &StoredObservation,
) -> Value {
    let mut intent_json = current.intent_json.clone();
    let evidence = deployment_intent_evidence_json(observation);
    if let Some(object) = intent_json.as_object_mut() {
        object.insert("deployment_evidence".to_string(), evidence);
    }

    intent_json
}

fn deployment_intent_evidence_json(observation: &StoredObservation) -> Value {
    let output = observation
        .data_json
        .get("output")
        .cloned()
        .unwrap_or_else(|| json!({}));
    json!({
        "status": deployment_intent_evidence_status(&output),
        "source": "observation",
        "observation_id": observation.id,
        "artifact_id": observation.artifact_id,
        "kind": observation.kind,
        "deploy_ready": deployment_intent_evidence_status(&output) == "satisfied",
        "review_required": deployment_intent_evidence_status(&output) != "satisfied",
        "resource": {
            "namespace": observation.resource_namespace,
            "kind": observation.resource_kind,
            "name": observation.resource_name,
        },
        "summary": {
            "sync_status": output.pointer("/status/sync/status"),
            "health_status": output.pointer("/status/health/status"),
            "revision": output.pointer("/status/sync/revision"),
        }
    })
}

fn deployment_intent_evidence_status(output: &Value) -> &'static str {
    let sync_status = output
        .pointer("/status/sync/status")
        .and_then(Value::as_str);
    let health_status = output
        .pointer("/status/health/status")
        .and_then(Value::as_str);

    match (sync_status, health_status) {
        (Some("Synced"), Some("Healthy")) => "satisfied",
        (Some(_), Some(_)) => "attention_required",
        (Some("Synced"), None) | (None, Some("Healthy")) => "unknown",
        (Some(_), None) | (None, Some(_)) => "attention_required",
        (None, None) => "unknown",
    }
}

fn deployment_intent_json(
    pipeline_intent: &StoredPipelineIntent,
    intent_kind: &str,
    target_environment: Option<&str>,
    target_namespace: Option<&str>,
    argo_application: Option<&str>,
    intent_json: Option<serde_json::Value>,
) -> Result<serde_json::Value, ApiError> {
    if let Some(intent_json) = intent_json {
        if !intent_json.is_object() {
            return Err(ApiError::bad_request(
                "deployment intent intent_json must be a JSON object",
            ));
        }
        return Ok(intent_json);
    }

    Ok(json!({
        "execution": {
            "enabled": false,
            "reason": "DeploymentIntent is review state only in V1"
        },
        "source": {
            "pipeline_intent_id": pipeline_intent.id,
            "change_set_id": pipeline_intent.change_set_id,
            "work_plan_id": pipeline_intent.work_plan_id,
        },
        "pipeline_evidence": deployment_pipeline_evidence_json(pipeline_intent),
        "deployment": {
            "provider": "argo_cd",
            "intent_kind": intent_kind,
            "target_environment": target_environment,
            "target_namespace": target_namespace,
            "argo_application": argo_application,
            "operation": "sync"
        }
    }))
}

fn deployment_pipeline_evidence_json(pipeline_intent: &StoredPipelineIntent) -> Value {
    let Some(evidence) = pipeline_intent.intent_json.get("evidence") else {
        return json!({
            "status": "missing",
            "deploy_ready": false,
            "review_required": true,
            "source": "pipeline_intent",
            "pipeline_intent_id": pipeline_intent.id,
            "summary": "No PipelineRunAnalysis evidence is attached to the approved PipelineIntent"
        });
    };

    let status = evidence
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    json!({
        "status": status,
        "deploy_ready": status == "satisfied",
        "review_required": status != "satisfied",
        "source": "pipeline_intent.evidence",
        "pipeline_intent_id": pipeline_intent.id,
        "observation_id": evidence.get("observation_id").cloned().unwrap_or(Value::Null),
        "artifact_id": evidence.get("artifact_id").cloned().unwrap_or(Value::Null),
        "summary": evidence.get("summary").cloned().unwrap_or_else(|| json!({})),
        "evidence": evidence.clone()
    })
}

fn validate_deployment_intent_transition(current: &str, target: &str) -> Result<(), ApiError> {
    match (current, target) {
        ("proposed", "approved" | "rejected") => Ok(()),
        ("approved", "rejected") => Ok(()),
        (_, "proposed") if current == target => Ok(()),
        _ => Err(ApiError::conflict(format!(
            "cannot transition deployment intent from {current} to {target}"
        ))),
    }
}

async fn list_releases(
    State(state): State<AppState>,
    Query(query): Query<ListReleasesQuery>,
) -> Result<Json<ReleasesResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let releases = state
        .store
        .list_releases(ReleaseListFilter {
            deployment_intent_id: clean_optional_text(query.deployment_intent_id),
            pipeline_intent_id: clean_optional_text(query.pipeline_intent_id),
            change_set_id: clean_optional_text(query.change_set_id),
            work_plan_id: clean_optional_text(query.work_plan_id),
            remediation_plan_id: clean_optional_text(query.remediation_plan_id),
            incident_id: clean_optional_text(query.incident_id),
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            status: clean_optional_text(query.status),
            release_kind: clean_optional_text(query.release_kind),
            risk_level: clean_optional_text(query.risk_level),
            target_environment: clean_optional_text(query.target_environment),
            target_namespace: clean_optional_text(query.target_namespace),
            argo_application: clean_optional_text(query.argo_application),
            version: clean_optional_text(query.version),
            commit_sha: clean_optional_text(query.commit_sha),
            image_digest: clean_optional_text(query.image_digest),
            created_after_ms: query.created_after_ms,
            created_before_ms: query.created_before_ms,
            limit,
            offset,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let count = releases.len();

    Ok(Json(ReleasesResponse {
        releases,
        count,
        limit,
        offset,
    }))
}

async fn get_release(
    State(state): State<AppState>,
    Path(release_id): Path<String>,
) -> Result<Json<ReleaseResponse>, ApiError> {
    let release = state
        .store
        .get_release(&release_id)
        .await?
        .ok_or_else(|| ApiError::not_found("release", &release_id))?;

    Ok(Json(release.into()))
}

async fn create_release_from_deployment_intent(
    State(state): State<AppState>,
    Json(request): Json<CreateReleaseFromDeploymentIntentRequest>,
) -> Result<Json<CreateReleaseResponse>, ApiError> {
    let deployment_intent_id = clean_optional_text(Some(request.deployment_intent_id))
        .ok_or_else(|| ApiError::bad_request("deployment_intent_id is required"))?;
    let deployment_intent = state
        .store
        .get_deployment_intent(&deployment_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_intent", &deployment_intent_id))?;
    ensure_approved_for_trusted_envelope(
        "deployment_intent",
        &deployment_intent.id,
        &deployment_intent.status,
    )?;

    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let release_kind =
        clean_optional_text(request.release_kind).unwrap_or_else(|| "gitops_release".to_string());
    let version = clean_optional_text(request.version);
    let commit_sha = clean_optional_text(request.commit_sha);
    let image_digest = clean_optional_text(request.image_digest);
    let rollback_ref = clean_optional_text(request.rollback_ref);
    let release_json = release_json(
        &deployment_intent,
        &release_kind,
        version.as_deref(),
        commit_sha.as_deref(),
        image_digest.as_deref(),
        rollback_ref.as_deref(),
        request.release_json,
    )?;
    if let Some(existing) = state
        .store
        .get_release_by_deployment_intent(&deployment_intent_id)
        .await?
    {
        if existing.status == "stale" {
            let release = state
                .store
                .revise_release_draft(
                    &existing.id,
                    UpdateReleaseDraft {
                        title: clean_optional_text(request.title)
                            .unwrap_or_else(|| format!("Release: {}", deployment_intent.title)),
                        summary: clean_optional_text(request.summary).unwrap_or_else(|| {
                            "Propose release after approved deployment intent".to_string()
                        }),
                        risk_level: clean_optional_text(request.risk_level)
                            .unwrap_or_else(|| deployment_intent.risk_level.clone()),
                        release_kind,
                        target_environment: deployment_intent.target_environment,
                        target_namespace: deployment_intent.target_namespace,
                        argo_application: deployment_intent.argo_application,
                        version,
                        commit_sha,
                        image_digest,
                        rollback_ref,
                        release_json,
                        actor: actor.clone(),
                        reason: reason.clone(),
                    },
                )
                .await?;
            append_release_audit_event(
                &state.store,
                &release,
                "release.reproposed",
                actor,
                reason,
                json!({
                    "source": "deployment_intent",
                    "deployment_intent_id": release.deployment_intent_id,
                    "previous_status": existing.status,
                    "execution_enabled": false,
                    "deployment_evidence_status": release
                        .release_json
                        .pointer("/deployment_evidence/status"),
                    "deployment_release_ready": release
                        .release_json
                        .pointer("/deployment_evidence/release_ready"),
                }),
            )
            .await?;

            return Ok(Json(CreateReleaseResponse {
                release: release.into(),
                created: false,
            }));
        }

        return Ok(Json(CreateReleaseResponse {
            release: existing.into(),
            created: false,
        }));
    }
    let release = state
        .store
        .create_release(CreateRelease {
            id: format!("rel_{}", unique_suffix()),
            deployment_intent_id: deployment_intent.id.clone(),
            pipeline_intent_id: deployment_intent.pipeline_intent_id.clone(),
            change_set_id: deployment_intent.change_set_id.clone(),
            work_plan_id: deployment_intent.work_plan_id.clone(),
            remediation_plan_id: deployment_intent.remediation_plan_id.clone(),
            incident_id: deployment_intent.incident_id.clone(),
            session_id: deployment_intent.session_id.clone(),
            run_id: deployment_intent.run_id.clone(),
            status: "proposed".to_string(),
            title: clean_optional_text(request.title)
                .unwrap_or_else(|| format!("Release: {}", deployment_intent.title)),
            summary: clean_optional_text(request.summary)
                .unwrap_or_else(|| "Propose release after approved deployment intent".to_string()),
            risk_level: clean_optional_text(request.risk_level)
                .unwrap_or(deployment_intent.risk_level),
            release_kind,
            target_environment: deployment_intent.target_environment,
            target_namespace: deployment_intent.target_namespace,
            argo_application: deployment_intent.argo_application,
            version,
            commit_sha,
            image_digest,
            rollback_ref,
            release_json,
        })
        .await?;
    append_release_audit_event(
        &state.store,
        &release,
        "release.proposed",
        actor,
        reason,
        json!({
            "source": "deployment_intent",
            "deployment_intent_id": release.deployment_intent_id,
            "execution_enabled": false,
            "deployment_evidence_status": release
                .release_json
                .pointer("/deployment_evidence/status"),
            "deployment_release_ready": release
                .release_json
                .pointer("/deployment_evidence/release_ready"),
        }),
    )
    .await?;

    Ok(Json(CreateReleaseResponse {
        release: release.into(),
        created: true,
    }))
}

async fn transition_release(
    State(state): State<AppState>,
    Path(release_id): Path<String>,
    Json(request): Json<TransitionReleaseRequest>,
) -> Result<Json<TransitionReleaseResponse>, ApiError> {
    let current = state
        .store
        .get_release(&release_id)
        .await?
        .ok_or_else(|| ApiError::not_found("release", &release_id))?;
    let target = clean_optional_text(Some(request.target_status))
        .ok_or_else(|| ApiError::bad_request("target_status is required"))?;
    validate_release_transition(&current.status, &target)?;
    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let release = state
        .store
        .update_release_status(&release_id, &target, actor.clone(), reason.clone())
        .await?;
    append_release_audit_event(
        &state.store,
        &release,
        &format!("release.{target}"),
        actor,
        reason,
        json!({
            "previous_status": current.status,
            "status": release.status,
        }),
    )
    .await?;

    Ok(Json(TransitionReleaseResponse {
        release: release.into(),
    }))
}

async fn attach_release_evidence(
    State(state): State<AppState>,
    Path(release_id): Path<String>,
    Json(request): Json<AttachReleaseEvidenceRequest>,
) -> Result<Json<AttachReleaseEvidenceResponse>, ApiError> {
    let current = state
        .store
        .get_release(&release_id)
        .await?
        .ok_or_else(|| ApiError::not_found("release", &release_id))?;
    if matches!(current.status.as_str(), "stale" | "rejected") {
        return Err(ApiError::conflict(format!(
            "cannot attach evidence to {} release {release_id}",
            current.status
        )));
    }

    let observation_id = clean_optional_text(Some(request.observation_id))
        .ok_or_else(|| ApiError::bad_request("observation_id is required"))?;
    let observation = state
        .store
        .get_observation(&observation_id)
        .await?
        .ok_or_else(|| ApiError::not_found("observation", &observation_id))?;
    validate_release_observation(&observation)?;

    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let release_json = release_json_with_observability_evidence(&current, &observation);
    let release = state
        .store
        .update_release_evidence(
            &release_id,
            UpdateReleaseEvidence {
                release_json,
                actor: actor.clone(),
                reason: reason.clone(),
            },
        )
        .await?;
    append_release_audit_event(
        &state.store,
        &release,
        "release.evidence_attached",
        actor.clone(),
        reason.clone(),
        json!({
            "observation_id": observation.id,
            "artifact_id": observation.artifact_id,
            "evidence_status": release_observability_evidence_status(&release),
            "resource": {
                "source": observation.source,
                "kind": observation.kind,
                "namespace": observation.resource_namespace,
                "resource_kind": observation.resource_kind,
                "name": observation.resource_name,
            },
        }),
    )
    .await?;
    let incident = create_release_observability_incident(
        &state.store,
        &release,
        &observation,
        actor.clone(),
        reason.clone(),
    )
    .await?;
    let remediation_plan = match incident.as_ref() {
        Some(incident) => {
            create_release_observability_remediation_plan(
                &state.store,
                incident,
                actor.clone(),
                reason.clone(),
            )
            .await?
        }
        None => None,
    };

    Ok(Json(AttachReleaseEvidenceResponse {
        release: release.into(),
        observation: observation.into(),
        incident: incident.map(Into::into),
        remediation_plan: remediation_plan.map(Into::into),
    }))
}

fn validate_release_observation(observation: &StoredObservation) -> Result<(), ApiError> {
    match (observation.source.as_str(), observation.kind.as_str()) {
        ("prometheus", "inventory" | "prometheus_read") => Ok(()),
        ("loki", "log_summary") => Ok(()),
        _ => Err(ApiError::bad_request(
            "release evidence must be a Prometheus inventory/query or Loki log summary observation",
        )),
    }
}

fn release_json_with_observability_evidence(
    current: &StoredRelease,
    observation: &StoredObservation,
) -> Value {
    let mut release_json = current.release_json.clone();
    let evidence = release_observability_evidence_json(observation);
    if let Some(object) = release_json.as_object_mut() {
        let items = object
            .entry("observability_evidence")
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Some(items) = items.as_array_mut() {
            items.retain(|item| {
                item.get("observation_id").and_then(Value::as_str) != Some(observation.id.as_str())
            });
            items.push(evidence);
        } else {
            object.insert("observability_evidence".to_string(), json!([evidence]));
        }
    }
    release_json
}

fn release_observability_evidence_json(observation: &StoredObservation) -> Value {
    json!({
        "status": release_observability_status(observation),
        "source": "observation",
        "observation_source": observation.source,
        "observation_kind": observation.kind,
        "observation_id": observation.id,
        "artifact_id": observation.artifact_id,
        "runtime_ready": release_observability_status(observation) == "observed",
        "review_required": release_observability_status(observation) != "observed",
        "resource": {
            "namespace": observation.resource_namespace,
            "kind": observation.resource_kind,
            "name": observation.resource_name,
        },
        "summary": release_observability_summary(observation),
    })
}

fn release_observability_status(observation: &StoredObservation) -> &'static str {
    match (observation.source.as_str(), observation.kind.as_str()) {
        ("prometheus", "inventory") => {
            prometheus_inventory_observability_status(&observation.data_json)
        }
        ("prometheus", "prometheus_read") => {
            prometheus_query_observability_status(&observation.data_json)
        }
        ("loki", "log_summary") => loki_observability_status(&observation.data_json),
        _ => "unknown",
    }
}

fn prometheus_inventory_observability_status(data: &Value) -> &'static str {
    let unhealthy_targets = data
        .pointer("/inventory/targets/unhealthy_count")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let problem_rules = data
        .pointer("/inventory/rules/problem_rule_count")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let alerts = data
        .pointer("/inventory/alerts/alert_count")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if unhealthy_targets > 0 || problem_rules > 0 || alerts > 0 {
        "attention_required"
    } else if data.get("inventory").is_some() {
        "observed"
    } else {
        "unknown"
    }
}

fn prometheus_query_observability_status(data: &Value) -> &'static str {
    match data.pointer("/response/status").and_then(Value::as_str) {
        Some("success") => "observed",
        Some(_) => "attention_required",
        None => "unknown",
    }
}

fn loki_observability_status(data: &Value) -> &'static str {
    match data.pointer("/response/status").and_then(Value::as_str) {
        Some("success") => "observed",
        Some(_) => "attention_required",
        None => "unknown",
    }
}

fn release_observability_summary(observation: &StoredObservation) -> Value {
    match (observation.source.as_str(), observation.kind.as_str()) {
        ("prometheus", "inventory") => json!({
            "unhealthy_targets": observation.data_json.pointer("/inventory/targets/unhealthy_count"),
            "problem_rules": observation.data_json.pointer("/inventory/rules/problem_rule_count"),
            "alerts": observation.data_json.pointer("/inventory/alerts/alert_count"),
        }),
        ("prometheus", "prometheus_read") => json!({
            "query": observation.data_json.get("query"),
            "status": observation.data_json.pointer("/response/status"),
            "result_count": observation.data_json.pointer("/response/data/result_count"),
        }),
        ("loki", "log_summary") => json!({
            "query": observation.data_json.get("query"),
            "status": observation.data_json.pointer("/response/status"),
            "stream_count": observation.data_json.pointer("/response/data/stream_count"),
            "entry_count": observation.data_json.pointer("/response/data/entry_count"),
        }),
        _ => json!({}),
    }
}

async fn create_release_observability_incident(
    store: &SqliteStore,
    release: &StoredRelease,
    observation: &StoredObservation,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<Option<StoredIncident>, ApiError> {
    if release_observability_status(observation) != "attention_required" {
        return Ok(None);
    }

    let incident_id = release_observability_incident_id(release, observation);
    if let Some(existing) = store.get_incident(&incident_id).await? {
        return Ok(Some(existing));
    }

    let summary = release_observability_incident_summary(observation);
    let incident = store
        .create_incident(CreateIncident {
            id: incident_id,
            observation_id: observation.id.clone(),
            session_id: observation.session_id.clone(),
            run_id: observation.run_id.clone(),
            status: "candidate".to_string(),
            severity: release_observability_incident_severity(observation).to_string(),
            title: format!(
                "Release observability issue: {}",
                release_observability_resource_label(observation)
            ),
            summary: summary.clone(),
            resource_namespace: observation.resource_namespace.clone(),
            resource_kind: observation.resource_kind.clone(),
            resource_name: observation.resource_name.clone(),
            data_json: json!({
                "source": "release_observability_evidence",
                "release_id": release.id,
                "deployment_intent_id": release.deployment_intent_id,
                "pipeline_intent_id": release.pipeline_intent_id,
                "change_set_id": release.change_set_id,
                "work_plan_id": release.work_plan_id,
                "observation_id": observation.id,
                "observation_source": observation.source,
                "observation_kind": observation.kind,
                "evidence_status": "attention_required",
                "summary": release_observability_summary(observation),
            }),
        })
        .await?;
    append_incident_audit_event(
        store,
        &incident,
        "incident.created",
        actor,
        reason.or_else(|| Some("release observability evidence requires review".to_string())),
    )
    .await?;

    Ok(Some(incident))
}

async fn create_release_observability_remediation_plan(
    store: &SqliteStore,
    incident: &StoredIncident,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<Option<StoredRemediationPlan>, ApiError> {
    if incident.status != "candidate" {
        return Ok(None);
    }
    if incident.data_json.get("source").and_then(Value::as_str)
        != Some("release_observability_evidence")
    {
        return Ok(None);
    }

    let plan_id = format!("rplan_{}", incident.id);
    if let Some(existing) = store.get_remediation_plan(&plan_id).await? {
        return Ok(Some(existing));
    }

    let resource = incident_resource_label(incident);
    let plan_json = release_observability_remediation_plan_json(incident, &resource);
    let plan = store
        .create_remediation_plan(CreateRemediationPlan {
            id: plan_id,
            incident_id: incident.id.clone(),
            session_id: incident.session_id.clone(),
            run_id: incident.run_id.clone(),
            status: "draft".to_string(),
            title: format!("Draft remediation for release observability issue: {resource}"),
            summary: "Re-read bounded observability evidence, confirm release health, then require approval before any file, pipeline, or cluster mutation.".to_string(),
            risk_level: incident.severity.clone(),
            requires_approval: true,
            resource_namespace: incident.resource_namespace.clone(),
            resource_kind: incident.resource_kind.clone(),
            resource_name: incident.resource_name.clone(),
            plan_json,
        })
        .await?;
    append_remediation_plan_audit_event(
        store,
        &plan,
        "remediation_plan.created",
        actor,
        reason.or_else(|| Some("release observability incident requires review".to_string())),
    )
    .await?;

    for gate in approval_gates_from_remediation_plan(&plan) {
        let gate = store.create_approval_gate(gate).await?;
        append_approval_gate_audit_event(store, &gate, "approval_gate.created", "created").await?;
    }

    Ok(Some(plan))
}

fn release_observability_remediation_plan_json(incident: &StoredIncident, resource: &str) -> Value {
    json!({
        "mode": "read_only_draft",
        "source": "release_observability_evidence",
        "incident_id": incident.id,
        "resource": {
            "namespace": incident.resource_namespace,
            "kind": incident.resource_kind,
            "name": incident.resource_name,
            "label": resource,
        },
        "evidence": {
            "summary": incident.summary,
            "release_id": incident.data_json.get("release_id"),
            "deployment_intent_id": incident.data_json.get("deployment_intent_id"),
            "pipeline_intent_id": incident.data_json.get("pipeline_intent_id"),
            "change_set_id": incident.data_json.get("change_set_id"),
            "observation_id": incident.data_json.get("observation_id"),
            "observation_source": incident.data_json.get("observation_source"),
            "observation_kind": incident.data_json.get("observation_kind"),
            "details": incident.data_json.get("summary"),
        },
        "steps": [
            {
                "order": 1,
                "kind": "read_only",
                "capability": "prometheus_inventory",
                "summary": "Refresh bounded Prometheus inventory and compare active alerts, unhealthy targets, and problem rules against the attached evidence."
            },
            {
                "order": 2,
                "kind": "read_only",
                "capability": "loki_log_summary",
                "summary": "Inspect bounded, redacted application and controller logs for the affected namespace if Loki is configured."
            },
            {
                "order": 3,
                "kind": "read_only",
                "capability": "argocd_get_application",
                "summary": "Confirm Argo sync and health before proposing release, rollback, or rollout remediation."
            },
            {
                "order": 4,
                "kind": "proposal",
                "capability": "worktree_change",
                "summary": "If evidence points to repo configuration or application code, prepare a ChangeSet and require approval before file writes."
            },
            {
                "order": 5,
                "kind": "proposal",
                "capability": "deployment_or_pipeline_intent",
                "summary": "If evidence points to runtime or delivery state, propose a PipelineIntent or DeploymentIntent and require approval before mutation."
            }
        ],
        "approval_gates": [
            {
                "kind": "file_write",
                "required_before": "creating or patching a ChangeSet"
            },
            {
                "kind": "pipeline_mutation",
                "required_before": "rerunning or cancelling Tekton resources"
            },
            {
                "kind": "cluster_mutation",
                "required_before": "Argo sync, rollback, restart, scale, or Kubernetes write"
            },
            {
                "kind": "production_impact",
                "required_before": "any action against production-impacting scope"
            }
        ],
        "non_goals": [
            "No automatic mutation in V1",
            "No secret reads",
            "No ticket creation",
            "No notification dispatch"
        ]
    })
}

fn approval_gates_from_remediation_plan(plan: &StoredRemediationPlan) -> Vec<CreateApprovalGate> {
    let gates = plan
        .plan_json
        .get("approval_gates")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    gates
        .into_iter()
        .enumerate()
        .filter_map(|(index, gate_json)| {
            let gate_kind = approval_gate_kind(&gate_json)?;
            let gate_order = i64::try_from(index).ok()?.saturating_add(1);
            let required_before = gate_json
                .get("required_before")
                .and_then(Value::as_str)
                .unwrap_or("executing a risky action");
            Some(CreateApprovalGate {
                id: format!(
                    "agate_{}_{}_{}",
                    plan.id,
                    gate_order,
                    safe_id_fragment(&gate_kind)
                ),
                remediation_plan_id: plan.id.clone(),
                incident_id: plan.incident_id.clone(),
                session_id: plan.session_id.clone(),
                run_id: plan.run_id.clone(),
                status: "pending".to_string(),
                gate_kind: gate_kind.clone(),
                gate_order,
                title: format!("Approve {}", gate_kind.replace('_', " ")),
                summary: format!("Approval required before {required_before}."),
                risk_level: plan.risk_level.clone(),
                resource_namespace: plan.resource_namespace.clone(),
                resource_kind: plan.resource_kind.clone(),
                resource_name: plan.resource_name.clone(),
                gate_json,
            })
        })
        .collect()
}

fn approval_gate_kind(gate_json: &Value) -> Option<String> {
    gate_json
        .get("kind")
        .and_then(Value::as_str)
        .or_else(|| gate_json.as_str())
        .map(str::trim)
        .filter(|kind| !kind.is_empty())
        .map(str::to_string)
}

fn incident_resource_label(incident: &StoredIncident) -> String {
    match (
        incident.resource_namespace.as_deref(),
        incident.resource_kind.as_deref(),
        incident.resource_name.as_deref(),
    ) {
        (Some(namespace), Some(kind), Some(name)) => format!("{namespace}/{kind}/{name}"),
        (Some(namespace), _, Some(name)) => format!("{namespace}/{name}"),
        (_, Some(kind), Some(name)) => format!("{kind}/{name}"),
        (_, _, Some(name)) => name.to_string(),
        (_, Some(kind), _) => kind.to_string(),
        _ => incident.id.clone(),
    }
}

fn safe_id_fragment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn release_observability_incident_id(
    release: &StoredRelease,
    observation: &StoredObservation,
) -> String {
    release_observability_incident_id_for_ids(&release.id, &observation.id)
}

fn release_observability_incident_id_for_ids(release_id: &str, observation_id: &str) -> String {
    let digest = Sha256::digest(format!("{release_id}:{observation_id}"));
    let hash = format!("{digest:x}");
    format!("inc_relobs_{}", &hash[..16])
}

fn release_observability_incident_summary(observation: &StoredObservation) -> String {
    match (observation.source.as_str(), observation.kind.as_str()) {
        ("prometheus", "inventory") => {
            let unhealthy_targets = observation
                .data_json
                .pointer("/inventory/targets/unhealthy_count")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let problem_rules = observation
                .data_json
                .pointer("/inventory/rules/problem_rule_count")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let alerts = observation
                .data_json
                .pointer("/inventory/alerts/alert_count")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            format!(
                "Prometheus inventory reports {alerts} active alerts, {unhealthy_targets} unhealthy targets, and {problem_rules} problem rules"
            )
        }
        ("prometheus", "prometheus_read") => format!(
            "Prometheus query returned status {}",
            observation
                .data_json
                .pointer("/response/status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ),
        ("loki", "log_summary") => format!(
            "Loki log summary returned status {}",
            observation
                .data_json
                .pointer("/response/status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ),
        _ => observation.summary.clone(),
    }
}

fn release_observability_incident_severity(observation: &StoredObservation) -> &'static str {
    if observation
        .data_json
        .pointer("/inventory/alerts/alert_count")
        .and_then(Value::as_i64)
        .unwrap_or_default()
        > 0
    {
        "high"
    } else {
        "medium"
    }
}

fn release_observability_resource_label(observation: &StoredObservation) -> String {
    if let Some(namespace) = &observation.resource_namespace {
        if let Some(name) = &observation.resource_name {
            return format!("{namespace}/{name}");
        }
    }
    observation
        .resource_name
        .clone()
        .or_else(|| observation.resource_kind.clone())
        .unwrap_or_else(|| observation.subject.clone())
}

fn release_json(
    deployment_intent: &StoredDeploymentIntent,
    release_kind: &str,
    version: Option<&str>,
    commit_sha: Option<&str>,
    image_digest: Option<&str>,
    rollback_ref: Option<&str>,
    release_json: Option<serde_json::Value>,
) -> Result<serde_json::Value, ApiError> {
    if let Some(release_json) = release_json {
        if !release_json.is_object() {
            return Err(ApiError::bad_request(
                "release release_json must be a JSON object",
            ));
        }
        return Ok(release_json);
    }

    Ok(json!({
        "execution": {
            "enabled": false,
            "reason": "Release is review state only in V1"
        },
        "source": {
            "deployment_intent_id": deployment_intent.id,
            "pipeline_intent_id": deployment_intent.pipeline_intent_id,
            "change_set_id": deployment_intent.change_set_id,
            "work_plan_id": deployment_intent.work_plan_id,
        },
        "deployment_evidence": release_deployment_evidence_json(deployment_intent),
        "observability_evidence": [],
        "release": {
            "release_kind": release_kind,
            "target_environment": deployment_intent.target_environment,
            "target_namespace": deployment_intent.target_namespace,
            "argo_application": deployment_intent.argo_application,
            "version": version,
            "commit_sha": commit_sha,
            "image_digest": image_digest,
            "rollback_ref": rollback_ref,
        },
        "verification": {
            "required": ["argo_health", "lgtm_signals", "audit_event"]
        }
    }))
}

fn release_deployment_evidence_json(deployment_intent: &StoredDeploymentIntent) -> Value {
    let Some(evidence) = deployment_intent.intent_json.get("deployment_evidence") else {
        return json!({
            "status": "missing",
            "release_ready": false,
            "review_required": true,
            "source": "deployment_intent",
            "deployment_intent_id": deployment_intent.id,
            "summary": "No Argo Application evidence is attached to the approved DeploymentIntent"
        });
    };

    let status = evidence
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    json!({
        "status": status,
        "release_ready": status == "satisfied",
        "review_required": status != "satisfied",
        "source": "deployment_intent.deployment_evidence",
        "deployment_intent_id": deployment_intent.id,
        "observation_id": evidence.get("observation_id").cloned().unwrap_or(Value::Null),
        "artifact_id": evidence.get("artifact_id").cloned().unwrap_or(Value::Null),
        "summary": evidence.get("summary").cloned().unwrap_or_else(|| json!({})),
        "evidence": evidence.clone()
    })
}

fn validate_release_transition(current: &str, target: &str) -> Result<(), ApiError> {
    match (current, target) {
        ("proposed", "approved" | "rejected") => Ok(()),
        ("approved", "rejected") => Ok(()),
        (_, "proposed") if current == target => Ok(()),
        _ => Err(ApiError::conflict(format!(
            "cannot transition release from {current} to {target}"
        ))),
    }
}

async fn list_registry_evidence(
    State(state): State<AppState>,
    Query(query): Query<ListRegistryEvidenceQuery>,
) -> Result<Json<RegistryEvidenceListResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let registry_evidence = state
        .store
        .list_registry_evidence(RegistryEvidenceListFilter {
            release_id: clean_optional_text(query.release_id),
            deployment_intent_id: clean_optional_text(query.deployment_intent_id),
            pipeline_intent_id: clean_optional_text(query.pipeline_intent_id),
            change_set_id: clean_optional_text(query.change_set_id),
            work_plan_id: clean_optional_text(query.work_plan_id),
            remediation_plan_id: clean_optional_text(query.remediation_plan_id),
            incident_id: clean_optional_text(query.incident_id),
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            status: clean_optional_text(query.status),
            risk_level: clean_optional_text(query.risk_level),
            registry: clean_optional_text(query.registry),
            repository: clean_optional_text(query.repository),
            image_ref: clean_optional_text(query.image_ref),
            image_digest: clean_optional_text(query.image_digest),
            tag: clean_optional_text(query.tag),
            source: clean_optional_text(query.source),
            verification_status: clean_optional_text(query.verification_status),
            created_after_ms: query.created_after_ms,
            created_before_ms: query.created_before_ms,
            limit,
            offset,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let count = registry_evidence.len();

    Ok(Json(RegistryEvidenceListResponse {
        registry_evidence,
        count,
        limit,
        offset,
    }))
}

async fn get_registry_evidence(
    State(state): State<AppState>,
    Path(evidence_id): Path<String>,
) -> Result<Json<RegistryEvidenceResponse>, ApiError> {
    let evidence = state
        .store
        .get_registry_evidence(&evidence_id)
        .await?
        .ok_or_else(|| ApiError::not_found("registry_evidence", &evidence_id))?;

    Ok(Json(evidence.into()))
}

async fn create_registry_evidence_from_release(
    State(state): State<AppState>,
    Json(request): Json<CreateRegistryEvidenceFromReleaseRequest>,
) -> Result<Json<CreateRegistryEvidenceResponse>, ApiError> {
    let release_id = clean_optional_text(Some(request.release_id.clone()))
        .ok_or_else(|| ApiError::bad_request("release_id is required"))?;
    let release = state
        .store
        .get_release(&release_id)
        .await?
        .ok_or_else(|| ApiError::not_found("release", &release_id))?;
    ensure_approved_for_trusted_envelope("release", &release.id, &release.status)?;

    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let registry = clean_optional_text(request.registry);
    let repository = clean_optional_text(request.repository);
    let image_ref = clean_optional_text(request.image_ref);
    let image_digest = clean_optional_text(request.image_digest).or(release.image_digest.clone());
    let tag = clean_optional_text(request.tag);
    let source = clean_optional_text(request.source).unwrap_or_else(|| "manual".to_string());
    let verification_status = clean_optional_text(request.verification_status)
        .unwrap_or_else(|| "unverified".to_string());
    validate_registry_verification_status(&verification_status)?;
    let evidence_json = registry_evidence_json(
        &release,
        RegistryEvidenceJsonInput {
            registry: registry.as_deref(),
            repository: repository.as_deref(),
            image_ref: image_ref.as_deref(),
            image_digest: image_digest.as_deref(),
            tag: tag.as_deref(),
            source: &source,
            verification_status: &verification_status,
            evidence_json: request.evidence_json,
        },
    )?;
    let response = propose_registry_evidence_for_release(
        &state,
        &release,
        RegistryEvidenceDraft {
            title: clean_optional_text(request.title)
                .unwrap_or_else(|| format!("Registry evidence: {}", release.title)),
            summary: clean_optional_text(request.summary)
                .unwrap_or_else(|| "Propose registry evidence after approved release".to_string()),
            risk_level: clean_optional_text(request.risk_level)
                .unwrap_or(release.risk_level.clone()),
            registry,
            repository,
            image_ref,
            image_digest,
            tag,
            source,
            verification_status,
            evidence_json,
            actor,
            reason,
            audit_source: "release".to_string(),
            audit_execution_enabled: false,
        },
    )
    .await?;

    Ok(Json(response))
}

async fn create_registry_evidence_from_registry_inspection(
    State(state): State<AppState>,
    Json(request): Json<CreateRegistryEvidenceFromInspectionRequest>,
) -> Result<Json<CreateRegistryEvidenceFromInspectionResponse>, ApiError> {
    let release_id = clean_optional_text(Some(request.release_id.clone()))
        .ok_or_else(|| ApiError::bad_request("release_id is required"))?;
    let image_ref = clean_optional_text(Some(request.image_ref.clone()))
        .ok_or_else(|| ApiError::bad_request("image_ref is required"))?;
    let release = state
        .store
        .get_release(&release_id)
        .await?
        .ok_or_else(|| ApiError::not_found("release", &release_id))?;
    ensure_approved_for_trusted_envelope("release", &release.id, &release.status)?;

    let inspection = execute_direct_capability(
        &state,
        AgentAction::RegistryInspectImage {
            id: "api.registry_inspect_image".into(),
            reason: clean_optional_text(request.reason.clone()).unwrap_or_else(|| {
                format!("Create RegistryEvidence from registry inspection for {image_ref}")
            }),
            image_ref: image_ref.clone(),
            registry_base_url: clean_optional_text(request.registry_base_url.clone()),
        },
        request.timeout_ms,
    )
    .await?;
    if inspection.status != "ok" {
        return Ok(Json(CreateRegistryEvidenceFromInspectionResponse {
            registry_evidence: None,
            created: false,
            inspection,
        }));
    }

    let Some(result) = inspection.result.as_ref() else {
        return Ok(Json(CreateRegistryEvidenceFromInspectionResponse {
            registry_evidence: None,
            created: false,
            inspection,
        }));
    };
    let draft = registry_evidence_draft_from_inspection(&release, &request, &image_ref, result)?;
    let response = propose_registry_evidence_for_release(&state, &release, draft).await?;

    Ok(Json(CreateRegistryEvidenceFromInspectionResponse {
        registry_evidence: Some(response.registry_evidence),
        created: response.created,
        inspection,
    }))
}

struct RegistryEvidenceDraft {
    title: String,
    summary: String,
    risk_level: String,
    registry: Option<String>,
    repository: Option<String>,
    image_ref: Option<String>,
    image_digest: Option<String>,
    tag: Option<String>,
    source: String,
    verification_status: String,
    evidence_json: serde_json::Value,
    actor: Option<String>,
    reason: Option<String>,
    audit_source: String,
    audit_execution_enabled: bool,
}

async fn propose_registry_evidence_for_release(
    state: &AppState,
    release: &StoredRelease,
    draft: RegistryEvidenceDraft,
) -> Result<CreateRegistryEvidenceResponse, ApiError> {
    if let Some(existing) = state
        .store
        .get_registry_evidence_by_release(&release.id)
        .await?
    {
        if existing.status == "stale" {
            let evidence = state
                .store
                .revise_registry_evidence_draft(
                    &existing.id,
                    UpdateRegistryEvidenceDraft {
                        title: draft.title,
                        summary: draft.summary,
                        risk_level: draft.risk_level,
                        registry: draft.registry,
                        repository: draft.repository,
                        image_ref: draft.image_ref,
                        image_digest: draft.image_digest,
                        tag: draft.tag,
                        source: draft.source,
                        verification_status: draft.verification_status,
                        evidence_json: draft.evidence_json,
                        actor: draft.actor.clone(),
                        reason: draft.reason.clone(),
                    },
                )
                .await?;
            append_registry_evidence_audit_event(
                &state.store,
                &evidence,
                "registry_evidence.reproposed",
                draft.actor,
                draft.reason,
                json!({
                    "source": draft.audit_source,
                    "release_id": evidence.release_id,
                    "previous_status": existing.status,
                    "execution_enabled": draft.audit_execution_enabled,
                }),
            )
            .await?;

            return Ok(CreateRegistryEvidenceResponse {
                registry_evidence: evidence.into(),
                created: false,
            });
        }

        return Ok(CreateRegistryEvidenceResponse {
            registry_evidence: existing.into(),
            created: false,
        });
    }
    let evidence = state
        .store
        .create_registry_evidence(CreateRegistryEvidence {
            id: format!("regev_{}", unique_suffix()),
            release_id: release.id.clone(),
            deployment_intent_id: release.deployment_intent_id.clone(),
            pipeline_intent_id: release.pipeline_intent_id.clone(),
            change_set_id: release.change_set_id.clone(),
            work_plan_id: release.work_plan_id.clone(),
            remediation_plan_id: release.remediation_plan_id.clone(),
            incident_id: release.incident_id.clone(),
            session_id: release.session_id.clone(),
            run_id: release.run_id.clone(),
            status: "proposed".to_string(),
            title: draft.title,
            summary: draft.summary,
            risk_level: draft.risk_level,
            registry: draft.registry,
            repository: draft.repository,
            image_ref: draft.image_ref,
            image_digest: draft.image_digest,
            tag: draft.tag,
            source: draft.source,
            verification_status: draft.verification_status,
            evidence_json: draft.evidence_json,
        })
        .await?;
    append_registry_evidence_audit_event(
        &state.store,
        &evidence,
        "registry_evidence.proposed",
        draft.actor,
        draft.reason,
        json!({
            "source": draft.audit_source,
            "release_id": evidence.release_id,
            "execution_enabled": draft.audit_execution_enabled,
        }),
    )
    .await?;

    Ok(CreateRegistryEvidenceResponse {
        registry_evidence: evidence.into(),
        created: true,
    })
}

fn registry_evidence_draft_from_inspection(
    release: &StoredRelease,
    request: &CreateRegistryEvidenceFromInspectionRequest,
    image_ref: &str,
    result: &ToolResult,
) -> Result<RegistryEvidenceDraft, ApiError> {
    let content = &result.content;
    let registry = string_at(content, "/image/registry");
    let repository = string_at(content, "/image/repository");
    let tag = string_at(content, "/image/tag");
    let image_digest =
        string_at(content, "/image/digest").or_else(|| string_at(content, "/probe/digest"));
    let verification_status =
        string_at(content, "/verification_status").unwrap_or_else(|| "unknown".to_string());
    validate_registry_verification_status(&verification_status)?;
    let source = "registry_inspect_image".to_string();
    let evidence_json = registry_evidence_json(
        release,
        RegistryEvidenceJsonInput {
            registry: registry.as_deref(),
            repository: repository.as_deref(),
            image_ref: Some(image_ref),
            image_digest: image_digest.as_deref(),
            tag: tag.as_deref(),
            source: &source,
            verification_status: &verification_status,
            evidence_json: Some(json!({
                "execution": {
                    "enabled": true,
                    "capability": "registry_inspect_image",
                    "tool_status": result.status,
                    "summary": result.summary,
                    "manifest_body_persisted": false,
                },
                "source": {
                    "release_id": release.id,
                    "deployment_intent_id": release.deployment_intent_id,
                    "pipeline_intent_id": release.pipeline_intent_id,
                    "change_set_id": release.change_set_id,
                    "work_plan_id": release.work_plan_id,
                    "evidence_source": source,
                },
                "image": {
                    "registry": registry,
                    "repository": repository,
                    "image_ref": image_ref,
                    "image_digest": image_digest,
                    "tag": tag,
                    "requested_image_ref": content.get("requested_image_ref"),
                    "reference": content.get("reference"),
                },
                "verification": {
                    "status": verification_status,
                    "checks": [{
                        "name": "anonymous_manifest_probe",
                        "status": content.pointer("/probe/status"),
                        "accessible": content.pointer("/probe/accessible"),
                        "digest": content.pointer("/probe/digest"),
                        "content_type": content.pointer("/probe/content_type"),
                    }],
                },
            })),
        },
    )?;

    Ok(RegistryEvidenceDraft {
        title: clean_optional_text(request.title.clone())
            .unwrap_or_else(|| format!("Registry evidence: {}", release.title)),
        summary: clean_optional_text(request.summary.clone())
            .unwrap_or_else(|| result.summary.clone()),
        risk_level: clean_optional_text(request.risk_level.clone())
            .unwrap_or_else(|| release.risk_level.clone()),
        registry,
        repository,
        image_ref: Some(image_ref.to_string()),
        image_digest: image_digest.or_else(|| release.image_digest.clone()),
        tag,
        source,
        verification_status,
        evidence_json,
        actor: clean_optional_text(request.actor.clone()),
        reason: clean_optional_text(request.reason.clone()),
        audit_source: "registry_inspection".to_string(),
        audit_execution_enabled: true,
    })
}

fn string_at(source: &Value, pointer: &str) -> Option<String> {
    source
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_string)
}

async fn transition_registry_evidence(
    State(state): State<AppState>,
    Path(evidence_id): Path<String>,
    Json(request): Json<TransitionRegistryEvidenceRequest>,
) -> Result<Json<TransitionRegistryEvidenceResponse>, ApiError> {
    let current = state
        .store
        .get_registry_evidence(&evidence_id)
        .await?
        .ok_or_else(|| ApiError::not_found("registry_evidence", &evidence_id))?;
    let target = clean_optional_text(Some(request.target_status))
        .ok_or_else(|| ApiError::bad_request("target_status is required"))?;
    validate_registry_evidence_transition(&current.status, &target)?;
    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let evidence = state
        .store
        .update_registry_evidence_status(&evidence_id, &target, actor.clone(), reason.clone())
        .await?;
    append_registry_evidence_audit_event(
        &state.store,
        &evidence,
        &format!("registry_evidence.{target}"),
        actor,
        reason,
        json!({
            "previous_status": current.status,
            "status": evidence.status,
        }),
    )
    .await?;

    Ok(Json(TransitionRegistryEvidenceResponse {
        registry_evidence: evidence.into(),
    }))
}

struct RegistryEvidenceJsonInput<'a> {
    registry: Option<&'a str>,
    repository: Option<&'a str>,
    image_ref: Option<&'a str>,
    image_digest: Option<&'a str>,
    tag: Option<&'a str>,
    source: &'a str,
    verification_status: &'a str,
    evidence_json: Option<serde_json::Value>,
}

fn registry_evidence_json(
    release: &StoredRelease,
    input: RegistryEvidenceJsonInput<'_>,
) -> Result<serde_json::Value, ApiError> {
    if let Some(evidence_json) = input.evidence_json {
        ensure_json_object(&evidence_json, "evidence_json")?;
        return Ok(evidence_json);
    }

    Ok(json!({
        "execution": {
            "enabled": false,
            "reason": "RegistryEvidence is manual or API-fed evidence only in V1"
        },
        "source": {
            "release_id": release.id,
            "deployment_intent_id": release.deployment_intent_id,
            "pipeline_intent_id": release.pipeline_intent_id,
            "change_set_id": release.change_set_id,
            "work_plan_id": release.work_plan_id,
            "evidence_source": input.source,
        },
        "image": {
            "registry": input.registry,
            "repository": input.repository,
            "image_ref": input.image_ref,
            "image_digest": input.image_digest,
            "tag": input.tag,
        },
        "verification": {
            "status": input.verification_status,
            "checks": [],
        }
    }))
}

fn validate_registry_verification_status(status: &str) -> Result<(), ApiError> {
    match status {
        "verified" | "unverified" | "mismatch" | "unknown" => Ok(()),
        _ => Err(ApiError::bad_request(format!(
            "invalid registry verification status {status}"
        ))),
    }
}

fn validate_registry_evidence_transition(current: &str, target: &str) -> Result<(), ApiError> {
    match (current, target) {
        ("proposed", "verified" | "rejected") => Ok(()),
        ("verified", "rejected") => Ok(()),
        (_, "proposed") if current == target => Ok(()),
        _ => Err(ApiError::conflict(format!(
            "cannot transition registry evidence from {current} to {target}"
        ))),
    }
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
    let invalidated_trusted_envelopes = if request.material_change && material_hash_changed {
        stale_trusted_envelopes_for_change_set(
            &state.store,
            &change_set.id,
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
    let invalidated_pipeline_intent = if request.material_change && material_hash_changed {
        stale_pipeline_intent_for_change_set(
            &state.store,
            &change_set.id,
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
        None
    };
    let invalidated_deployment_intent = if let Some(intent) = &invalidated_pipeline_intent {
        stale_deployment_intent_for_pipeline_intent(
            &state.store,
            &intent.id,
            actor.clone(),
            reason.clone().or_else(|| {
                Some(format!(
                    "pipeline intent {} staled after change set {} revised",
                    intent.id, change_set.id
                ))
            }),
            "pipeline_intent_stale",
        )
        .await?
    } else {
        None
    };
    let invalidated_release = if let Some(intent) = &invalidated_deployment_intent {
        stale_release_for_deployment_intent(
            &state.store,
            &intent.id,
            actor.clone(),
            reason.clone().or_else(|| {
                Some(format!(
                    "deployment intent {} staled after change set {} revised",
                    intent.id, change_set.id
                ))
            }),
            "deployment_intent_stale",
        )
        .await?
    } else {
        None
    };
    let invalidated_registry_evidence = if let Some(release) = &invalidated_release {
        stale_registry_evidence_for_release(
            &state.store,
            &release.id,
            actor.clone(),
            reason.clone().or_else(|| {
                Some(format!(
                    "release {} staled after change set {} revised",
                    release.id, change_set.id
                ))
            }),
            "release_stale",
        )
        .await?
    } else {
        None
    };
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
            "invalidated_permission_grant_ids": invalidated_trusted_envelopes
                .iter()
                .map(|grant| grant.id.clone())
                .collect::<Vec<_>>(),
            "invalidated_pipeline_intent_id": invalidated_pipeline_intent
                .as_ref()
                .map(|intent| intent.id.clone()),
            "invalidated_deployment_intent_id": invalidated_deployment_intent
                .as_ref()
                .map(|intent| intent.id.clone()),
            "invalidated_release_id": invalidated_release
                .as_ref()
                .map(|release| release.id.clone()),
        }),
    )
    .await?;

    Ok(Json(ReviseChangeSetResponse {
        change_set: change_set.into(),
        material_hash_changed,
        invalidated_gates: invalidated_gates.into_iter().map(Into::into).collect(),
        invalidated_pipeline_intent: invalidated_pipeline_intent.map(Into::into),
        invalidated_deployment_intent: invalidated_deployment_intent.map(Into::into),
        invalidated_release: invalidated_release.map(Into::into),
        invalidated_registry_evidence: invalidated_registry_evidence.map(Into::into),
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
    ensure_approved_for_trusted_envelope("work_plan", &work_plan.id, &work_plan.status)?;
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
    let work_plan = state
        .store
        .get_work_plan(&change_set.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &change_set.work_plan_id))?;
    ensure_approved_for_trusted_envelope("work_plan", &work_plan.id, &work_plan.status)?;
    ensure_approved_for_trusted_envelope("change_set", &change_set.id, &change_set.status)?;
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

fn ensure_approved_for_trusted_envelope(
    resource_kind: &str,
    resource_id: &str,
    status: &str,
) -> Result<(), ApiError> {
    if status == "approved" {
        return Ok(());
    }

    Err(ApiError::conflict(format!(
        "{resource_kind} {resource_id} must be approved before creating a trusted envelope"
    )))
}

fn pipeline_intent_is_deployment_eligible(status: &str) -> bool {
    matches!(status, "approved" | "completed")
}

fn ensure_pipeline_intent_ready_for_deployment(
    intent: &StoredPipelineIntent,
) -> Result<(), ApiError> {
    if pipeline_intent_is_deployment_eligible(&intent.status) {
        return Ok(());
    }

    Err(ApiError::conflict(format!(
        "pipeline_intent {} must be approved with successful execution evidence before proposing deployment",
        intent.id
    )))
}

fn ensure_pipeline_evidence_ready_for_deployment(
    pipeline_intent: &StoredPipelineIntent,
) -> Result<(), ApiError> {
    if pipeline_intent_attached_evidence_status(pipeline_intent) != Some("satisfied") {
        return Err(ApiError::conflict(format!(
            "pipeline_intent {} needs satisfied PipelineRunAnalysis evidence before approving deployment",
            pipeline_intent.id
        )));
    }

    let expected_namespace = pipeline_intent
        .intent_json
        .pointer("/execution_evidence/pipeline_run/namespace")
        .and_then(Value::as_str);
    let expected_name = pipeline_intent
        .intent_json
        .pointer("/execution_evidence/pipeline_run/name")
        .and_then(Value::as_str);
    let evidence_namespace = pipeline_intent
        .intent_json
        .pointer("/evidence/resource/namespace")
        .and_then(Value::as_str);
    let evidence_name = pipeline_intent
        .intent_json
        .pointer("/evidence/resource/name")
        .and_then(Value::as_str);
    if expected_namespace.is_some_and(|value| evidence_namespace != Some(value))
        || expected_name.is_some_and(|value| evidence_name != Some(value))
    {
        return Err(ApiError::conflict(format!(
            "pipeline_intent {} evidence does not match the executed PipelineRun",
            pipeline_intent.id
        )));
    }

    Ok(())
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
struct StreamRunEventsQuery {
    after_seq: Option<u64>,
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

fn stream_start_seq(headers: &HeaderMap, query: &StreamRunEventsQuery) -> u64 {
    query.after_seq.unwrap_or_else(|| last_event_seq(headers))
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
    kind: Option<String>,
    actor: Option<String>,
    resource_kind: Option<String>,
    resource_id: Option<String>,
    run_id: Option<String>,
    namespace: Option<String>,
    repo: Option<String>,
    branch: Option<String>,
    production_impacting: Option<bool>,
    search: Option<String>,
    limit: Option<u32>,
}

async fn list_audit_events(
    State(state): State<AppState>,
    Query(query): Query<ListAuditEventsQuery>,
) -> Result<Json<AuditEventsResponse>, ApiError> {
    let events = state
        .store
        .query_audit_events(AuditEventListFilter {
            kind: clean_optional_text(query.kind),
            actor: clean_optional_text(query.actor),
            resource_kind: clean_optional_text(query.resource_kind),
            resource_id: clean_optional_text(query.resource_id),
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            namespace: clean_optional_text(query.namespace),
            repo: clean_optional_text(query.repo),
            branch: clean_optional_text(query.branch),
            production_impacting: query.production_impacting,
            search: clean_optional_text(query.search),
            limit: query.limit.unwrap_or(50),
        })
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

async fn append_observation_audit_event(
    store: &SqliteStore,
    observation: &StoredObservation,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", observation.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "observation".to_string(),
            resource_id: observation.id.clone(),
            run_id: observation.run_id.clone(),
            payload_json: json!({
                "observation_id": observation.id,
                "run_id": observation.run_id.as_ref().map(RunId::as_str),
                "source": observation.source,
                "kind": observation.kind,
                "subject": observation.subject,
                "summary": observation.summary,
                "reason": reason,
                "resource": {
                    "namespace": observation.resource_namespace,
                    "kind": observation.resource_kind,
                    "name": observation.resource_name,
                },
            }),
        })
        .await
        .map(|_| ())
}

async fn append_incident_audit_event(
    store: &SqliteStore,
    incident: &StoredIncident,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", incident.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "incident".to_string(),
            resource_id: incident.id.clone(),
            run_id: incident.run_id.clone(),
            payload_json: json!({
                "incident_id": incident.id,
                "observation_id": incident.observation_id,
                "run_id": incident.run_id.as_ref().map(RunId::as_str),
                "status": incident.status,
                "severity": incident.severity,
                "title": incident.title,
                "summary": incident.summary,
                "reason": reason,
                "resource": {
                    "namespace": incident.resource_namespace,
                    "kind": incident.resource_kind,
                    "name": incident.resource_name,
                },
            }),
        })
        .await
        .map(|_| ())
}

async fn append_remediation_plan_audit_event(
    store: &SqliteStore,
    plan: &StoredRemediationPlan,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", plan.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "remediation_plan".to_string(),
            resource_id: plan.id.clone(),
            run_id: plan.run_id.clone(),
            payload_json: json!({
                "remediation_plan_id": plan.id,
                "incident_id": plan.incident_id,
                "run_id": plan.run_id.as_ref().map(RunId::as_str),
                "status": plan.status,
                "risk_level": plan.risk_level,
                "requires_approval": plan.requires_approval,
                "title": plan.title,
                "summary": plan.summary,
                "reason": reason,
                "resource": {
                    "namespace": plan.resource_namespace,
                    "kind": plan.resource_kind,
                    "name": plan.resource_name,
                },
            }),
        })
        .await
        .map(|_| ())
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

async fn append_pipeline_intent_audit_event(
    store: &SqliteStore,
    intent: &StoredPipelineIntent,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
    extra: serde_json::Value,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", intent.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "pipeline_intent".to_string(),
            resource_id: intent.id.clone(),
            run_id: intent.run_id.clone(),
            payload_json: json!({
                "pipeline_intent_id": intent.id,
                "change_set_id": intent.change_set_id,
                "work_plan_id": intent.work_plan_id,
                "remediation_plan_id": intent.remediation_plan_id,
                "incident_id": intent.incident_id,
                "run_id": intent.run_id.as_ref().map(RunId::as_str),
                "status": intent.status,
                "intent_kind": intent.intent_kind,
                "risk_level": intent.risk_level,
                "summary": intent.summary,
                "reason": reason,
                "resource": {
                    "namespace": intent.resource_namespace,
                    "kind": intent.resource_kind,
                    "name": intent.resource_name,
                },
                "extra": extra,
            }),
        })
        .await
        .map(|_| ())
}

async fn append_pipeline_contract_audit_event(
    store: &SqliteStore,
    contract: &StoredPipelineContract,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", contract.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "pipeline_contract".to_string(),
            resource_id: contract.id.clone(),
            run_id: None,
            payload_json: json!({
                "pipeline_contract_id": contract.id,
                "status": contract.status,
                "namespace": contract.namespace,
                "pipeline_ref": contract.pipeline_ref,
                "version": contract.version,
                "reason": reason,
            }),
        })
        .await
        .map(|_| ())
}

async fn append_deployment_intent_audit_event(
    store: &SqliteStore,
    intent: &StoredDeploymentIntent,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
    extra: serde_json::Value,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", intent.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "deployment_intent".to_string(),
            resource_id: intent.id.clone(),
            run_id: intent.run_id.clone(),
            payload_json: json!({
                "deployment_intent_id": intent.id,
                "pipeline_intent_id": intent.pipeline_intent_id,
                "change_set_id": intent.change_set_id,
                "work_plan_id": intent.work_plan_id,
                "remediation_plan_id": intent.remediation_plan_id,
                "incident_id": intent.incident_id,
                "run_id": intent.run_id.as_ref().map(RunId::as_str),
                "status": intent.status,
                "intent_kind": intent.intent_kind,
                "risk_level": intent.risk_level,
                "summary": intent.summary,
                "target": {
                    "environment": intent.target_environment,
                    "namespace": intent.target_namespace,
                    "argo_application": intent.argo_application,
                },
                "reason": reason,
                "resource": {
                    "namespace": intent.resource_namespace,
                    "kind": intent.resource_kind,
                    "name": intent.resource_name,
                },
                "extra": extra,
            }),
        })
        .await
        .map(|_| ())
}

async fn append_release_audit_event(
    store: &SqliteStore,
    release: &StoredRelease,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
    extra: serde_json::Value,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", release.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "release".to_string(),
            resource_id: release.id.clone(),
            run_id: release.run_id.clone(),
            payload_json: json!({
                "release_id": release.id,
                "deployment_intent_id": release.deployment_intent_id,
                "pipeline_intent_id": release.pipeline_intent_id,
                "change_set_id": release.change_set_id,
                "work_plan_id": release.work_plan_id,
                "remediation_plan_id": release.remediation_plan_id,
                "incident_id": release.incident_id,
                "run_id": release.run_id.as_ref().map(RunId::as_str),
                "status": release.status,
                "release_kind": release.release_kind,
                "risk_level": release.risk_level,
                "summary": release.summary,
                "target": {
                    "environment": release.target_environment,
                    "namespace": release.target_namespace,
                    "argo_application": release.argo_application,
                },
                "artifacts": {
                    "version": release.version,
                    "commit_sha": release.commit_sha,
                    "image_digest": release.image_digest,
                    "rollback_ref": release.rollback_ref,
                },
                "reason": reason,
                "extra": extra,
            }),
        })
        .await
        .map(|_| ())
}

async fn append_registry_evidence_audit_event(
    store: &SqliteStore,
    evidence: &StoredRegistryEvidence,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
    extra: serde_json::Value,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", evidence.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "registry_evidence".to_string(),
            resource_id: evidence.id.clone(),
            run_id: evidence.run_id.clone(),
            payload_json: json!({
                "registry_evidence_id": evidence.id,
                "release_id": evidence.release_id,
                "deployment_intent_id": evidence.deployment_intent_id,
                "pipeline_intent_id": evidence.pipeline_intent_id,
                "change_set_id": evidence.change_set_id,
                "work_plan_id": evidence.work_plan_id,
                "remediation_plan_id": evidence.remediation_plan_id,
                "incident_id": evidence.incident_id,
                "run_id": evidence.run_id.as_ref().map(RunId::as_str),
                "status": evidence.status,
                "risk_level": evidence.risk_level,
                "summary": evidence.summary,
                "image": {
                    "registry": evidence.registry,
                    "repository": evidence.repository,
                    "image_ref": evidence.image_ref,
                    "image_digest": evidence.image_digest,
                    "tag": evidence.tag,
                },
                "source": evidence.source,
                "verification_status": evidence.verification_status,
                "reason": reason,
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

fn required_text(value: String, field: &str) -> Result<String, ApiError> {
    clean_optional_text(Some(value))
        .ok_or_else(|| ApiError::bad_request(format!("{field} is required")))
}

fn validate_allowed_value(field: &str, value: &str, allowed: &[&str]) -> Result<(), ApiError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "{field} must be one of: {}",
            allowed.join(", ")
        )))
    }
}

async fn root_session_for_request(
    store: &SqliteStore,
    requested_session_id: Option<String>,
    requested_run_id: Option<RunId>,
    title: &str,
) -> Result<(SessionId, Option<RunId>), ApiError> {
    if let Some(run_id) = requested_run_id {
        let run = store
            .get_run(&run_id)
            .await?
            .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
        return Ok((run.session_id, Some(run_id)));
    }

    let session_id = requested_session_id
        .map(SessionId::new)
        .unwrap_or_else(|| SessionId::new(format!("ses_control_{}", unique_suffix())));
    store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: title.to_string(),
            cwd: ".".to_string(),
        })
        .await?;

    Ok((session_id, None))
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
            if !state.worker.enabled() {
                return Err(ApiError::conflict(
                    "cannot approve without an enabled run worker",
                ));
            }

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
            state.worker.resume_run(run.clone(), approval.clone());

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
    insert_object_if_not_empty(
        &mut summary,
        "image",
        select_json_paths(
            &result.content,
            &[
                ("registry", "/image/registry"),
                ("repository", "/image/repository"),
                ("tag", "/image/tag"),
                ("digest", "/image/digest"),
                ("verification_status", "/verification_status"),
                ("probe_status", "/probe/status"),
                ("probe_accessible", "/probe/accessible"),
                ("probe_digest", "/probe/digest"),
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

    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
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
        approval_gate_summary, approval_summary, attach_deployment_intent_evidence,
        attach_pipeline_intent_evidence, attach_release_evidence, build_pipeline_run_manifest,
        cancel_run, change_set_flow, change_set_readiness, config_effective, create_change_set,
        create_change_set_trusted_envelope, create_deployment_intent_from_pipeline_intent,
        create_incident, create_observation, create_pipeline_intent_from_change_set,
        create_registry_evidence_from_registry_inspection, create_registry_evidence_from_release,
        create_release_from_deployment_intent, create_remediation_plan, create_run,
        create_work_plan_from_remediation_plan, create_work_plan_trusted_envelope,
        decide_run_approval, deny_approval, ensure_pipeline_evidence_ready_for_deployment,
        execute_capability, execution_matches_pipeline_contract, get_approval, get_approval_gate,
        get_artifact, get_deployment_intent, get_incident, get_observation, get_permission_grant,
        get_pipeline_intent, get_registry_evidence, get_release, get_remediation_plan, get_run,
        get_run_diff, get_run_events, get_work_plan, last_event_seq, list_approval_gates,
        list_approvals, list_audit_events, list_change_sets, list_deployment_intents,
        list_incidents, list_observations, list_permission_grants, list_pipeline_intents,
        list_registry_evidence, list_releases, list_remediation_plans, list_run_artifacts,
        list_run_observations, list_runs, list_work_plans, merge_pipeline_execution_state,
        parse_last_event_id, persist_pipeline_execution_evidence, policy_json, revise_change_set,
        revise_work_plan, revoke_permission_grant, router, run_policy, run_summary,
        satisfy_approval_gate, stream_start_seq, tekton_execution_spec, transition_change_set,
        transition_deployment_intent, transition_pipeline_intent, transition_registry_evidence,
        transition_release, transition_work_plan, unique_suffix, validate_permission_grant_request,
        work_plan_flow, work_plan_readiness, AppState, ApprovalGateSummaryQuery,
        ApprovalSummaryQuery, ListApprovalGatesQuery, ListApprovalsQuery, ListAuditEventsQuery,
        ListChangeSetsQuery, ListDeploymentIntentsQuery, ListIncidentsQuery, ListObservationsQuery,
        ListPermissionGrantsQuery, ListPipelineIntentsQuery, ListRegistryEvidenceQuery,
        ListReleasesQuery, ListRemediationPlansQuery, ListRunsQuery, ListWorkPlansQuery,
        StreamRunEventsQuery,
    };
    use crate::dispatch::RunDispatcher;
    use crate::dto::{
        ApprovalDecision, AttachDeploymentIntentEvidenceRequest,
        AttachPipelineIntentEvidenceRequest, AttachReleaseEvidenceRequest, CreateChangeSetRequest,
        CreateDeploymentIntentFromPipelineIntentRequest, CreateIncidentRequest,
        CreateObservationRequest, CreatePermissionGrantRequest,
        CreatePipelineIntentFromChangeSetRequest, CreateRegistryEvidenceFromInspectionRequest,
        CreateRegistryEvidenceFromReleaseRequest, CreateReleaseFromDeploymentIntentRequest,
        CreateRemediationPlanRequest, CreateRunRequest, CreateTrustedEnvelopeRequest,
        CreateWorkPlanFromRemediationPlanRequest, DecideApprovalGateRequest, DecideApprovalRequest,
        ExecuteCapabilityRequest, PipelineIntentExecutionOutcomeRequest, ReviewApprovalRequest,
        ReviseChangeSetRequest, ReviseWorkPlanRequest, RevokePermissionGrantRequest,
        TransitionChangeSetRequest, TransitionDeploymentIntentRequest,
        TransitionPipelineIntentRequest, TransitionRegistryEvidenceRequest,
        TransitionReleaseRequest, TransitionWorkPlanRequest,
    };
    use axum::extract::{Path, Query, State};
    use axum::http::{HeaderMap, HeaderValue, StatusCode};
    use axum::Json;
    use pharness_core::{
        AgentAction, AgentEvent, EventId, EventKind, PolicyMode, ReadOnlyClusterTools, RunId,
        RunScope, SafetyPolicy, SessionId,
    };
    use pharness_store::{
        ApprovalGateListFilter, CreateApproval, CreateApprovalGate, CreateArtifact,
        CreateChangeSet, CreateDeploymentIntent, CreateFileChange, CreateIncident,
        CreateObservation, CreatePipelineIntent, CreateRelease, CreateRemediationPlan, CreateRun,
        CreateSession, CreateWorkPlan, SqliteStore, StoredPipelineContract, StoredPipelineIntent,
    };
    use serde_json::json;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::Arc;

    async fn test_state() -> AppState {
        AppState {
            store: Arc::new(SqliteStore::connect_in_memory().await.unwrap()),
            worker: RunDispatcher::Disabled,
            cluster_tools: ReadOnlyClusterTools::default(),
            policy: SafetyPolicy::default(),
            worker_token: None,
            operator_tokens: Arc::new(Vec::new()),
        }
    }

    async fn test_state_with_cluster_tools(cluster_tools: ReadOnlyClusterTools) -> AppState {
        AppState {
            store: Arc::new(SqliteStore::connect_in_memory().await.unwrap()),
            worker: RunDispatcher::Disabled,
            cluster_tools,
            policy: SafetyPolicy::default(),
            worker_token: None,
            operator_tokens: Arc::new(Vec::new()),
        }
    }

    async fn seed_approved_release(state: &AppState) -> String {
        let session_id = SessionId::new("ses_registry_inspection");
        let run_id = RunId::new("run_registry_inspection");
        state
            .store
            .create_session(CreateSession {
                id: session_id.clone(),
                title: "registry inspection".to_string(),
                cwd: ".".to_string(),
            })
            .await
            .unwrap();
        state
            .store
            .create_run(CreateRun {
                id: run_id.clone(),
                session_id: session_id.clone(),
                user_task: "registry inspection".to_string(),
                cwd: ".".to_string(),
                max_turns: 1,
                initial_status: "completed".to_string(),
                execution_target_json: serde_json::json!({}),
            })
            .await
            .unwrap();
        state
            .store
            .create_observation(CreateObservation {
                id: "obs_registry_inspection".to_string(),
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                source: "test".to_string(),
                kind: "smoke".to_string(),
                subject: "checkout-api".to_string(),
                summary: "seed observation".to_string(),
                resource_namespace: Some("apps-dev".to_string()),
                resource_kind: Some("Deployment".to_string()),
                resource_name: Some("checkout-api".to_string()),
                resource_ref_json: None,
                artifact_id: None,
                data_json: serde_json::json!({}),
            })
            .await
            .unwrap();
        state
            .store
            .create_incident(CreateIncident {
                id: "inc_registry_inspection".to_string(),
                observation_id: "obs_registry_inspection".to_string(),
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                status: "resolved".to_string(),
                severity: "medium".to_string(),
                title: "Seed incident".to_string(),
                summary: "seed incident".to_string(),
                resource_namespace: Some("apps-dev".to_string()),
                resource_kind: Some("Deployment".to_string()),
                resource_name: Some("checkout-api".to_string()),
                data_json: serde_json::json!({}),
            })
            .await
            .unwrap();
        state
            .store
            .create_remediation_plan(CreateRemediationPlan {
                id: "rplan_registry_inspection".to_string(),
                incident_id: "inc_registry_inspection".to_string(),
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                status: "approved".to_string(),
                title: "Seed remediation".to_string(),
                summary: "seed remediation".to_string(),
                risk_level: "medium".to_string(),
                requires_approval: true,
                resource_namespace: Some("apps-dev".to_string()),
                resource_kind: Some("Deployment".to_string()),
                resource_name: Some("checkout-api".to_string()),
                plan_json: serde_json::json!({}),
            })
            .await
            .unwrap();
        state
            .store
            .create_work_plan(CreateWorkPlan {
                id: "wplan_registry_inspection".to_string(),
                remediation_plan_id: "rplan_registry_inspection".to_string(),
                incident_id: "inc_registry_inspection".to_string(),
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                status: "approved".to_string(),
                title: "Seed work".to_string(),
                summary: "seed work".to_string(),
                risk_level: "medium".to_string(),
                requires_approval: true,
                resource_namespace: Some("apps-dev".to_string()),
                resource_kind: Some("Deployment".to_string()),
                resource_name: Some("checkout-api".to_string()),
                work_plan_json: serde_json::json!({}),
            })
            .await
            .unwrap();
        state
            .store
            .create_change_set(CreateChangeSet {
                id: "cset_registry_inspection".to_string(),
                work_plan_id: "wplan_registry_inspection".to_string(),
                remediation_plan_id: "rplan_registry_inspection".to_string(),
                incident_id: "inc_registry_inspection".to_string(),
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                status: "approved".to_string(),
                title: "Seed changes".to_string(),
                summary: "seed changes".to_string(),
                risk_level: "medium".to_string(),
                material_hash: "hash_registry_inspection".to_string(),
                resource_namespace: Some("apps-dev".to_string()),
                resource_kind: Some("Deployment".to_string()),
                resource_name: Some("checkout-api".to_string()),
                change_set_json: serde_json::json!({}),
            })
            .await
            .unwrap();
        state
            .store
            .create_pipeline_intent(CreatePipelineIntent {
                id: "pint_registry_inspection".to_string(),
                change_set_id: "cset_registry_inspection".to_string(),
                work_plan_id: "wplan_registry_inspection".to_string(),
                remediation_plan_id: "rplan_registry_inspection".to_string(),
                incident_id: "inc_registry_inspection".to_string(),
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                status: "approved".to_string(),
                title: "Seed pipeline".to_string(),
                summary: "seed pipeline".to_string(),
                risk_level: "medium".to_string(),
                intent_kind: "tekton_build_test_package".to_string(),
                resource_namespace: Some("apps-dev".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("checkout-api".to_string()),
                intent_json: serde_json::json!({}),
            })
            .await
            .unwrap();
        state
            .store
            .create_deployment_intent(CreateDeploymentIntent {
                id: "dint_registry_inspection".to_string(),
                pipeline_intent_id: "pint_registry_inspection".to_string(),
                change_set_id: "cset_registry_inspection".to_string(),
                work_plan_id: "wplan_registry_inspection".to_string(),
                remediation_plan_id: "rplan_registry_inspection".to_string(),
                incident_id: "inc_registry_inspection".to_string(),
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                status: "approved".to_string(),
                title: "Seed deploy".to_string(),
                summary: "seed deploy".to_string(),
                risk_level: "medium".to_string(),
                intent_kind: "argo_sync_deploy".to_string(),
                target_environment: Some("dev".to_string()),
                target_namespace: Some("apps-dev".to_string()),
                argo_application: Some("checkout-api".to_string()),
                resource_namespace: Some("apps-dev".to_string()),
                resource_kind: Some("Application".to_string()),
                resource_name: Some("checkout-api".to_string()),
                intent_json: serde_json::json!({}),
            })
            .await
            .unwrap();
        state
            .store
            .create_release(CreateRelease {
                id: "rel_registry_inspection".to_string(),
                deployment_intent_id: "dint_registry_inspection".to_string(),
                pipeline_intent_id: "pint_registry_inspection".to_string(),
                change_set_id: "cset_registry_inspection".to_string(),
                work_plan_id: "wplan_registry_inspection".to_string(),
                remediation_plan_id: "rplan_registry_inspection".to_string(),
                incident_id: "inc_registry_inspection".to_string(),
                session_id,
                run_id: Some(run_id),
                status: "approved".to_string(),
                title: "Seed release".to_string(),
                summary: "seed release".to_string(),
                risk_level: "medium".to_string(),
                release_kind: "gitops_release".to_string(),
                target_environment: Some("dev".to_string()),
                target_namespace: Some("apps-dev".to_string()),
                argo_application: Some("checkout-api".to_string()),
                version: Some("v0.1.0-smoke".to_string()),
                commit_sha: Some("abc1234".to_string()),
                image_digest: None,
                rollback_ref: None,
                release_json: serde_json::json!({}),
            })
            .await
            .unwrap();

        "rel_registry_inspection".to_string()
    }

    #[tokio::test]
    async fn operator_auth_gates_api_routes_and_resolves_identity() {
        use tower::ServiceExt;

        let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
        let app = router(
            store,
            RunDispatcher::Disabled,
            ReadOnlyClusterTools::default(),
            SafetyPolicy::default(),
            None,
            vec![("lucas".to_string(), "op-secret".to_string())],
        );

        let health = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/health")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);

        let unauthenticated = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/runs")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

        let wrong = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/runs")
                    .header("authorization", "Bearer nope")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);

        let authed = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/config/effective")
                    .header("authorization", "Bearer op-secret")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(authed.status(), StatusCode::OK);
        let body = axum::body::to_bytes(authed.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["operator"]["auth_required"], true);
        assert_eq!(payload["operator"]["name"], "lucas");
    }

    #[tokio::test]
    async fn internal_routes_are_disabled_without_worker_token() {
        use tower::ServiceExt;

        let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
        let app = router(
            store,
            RunDispatcher::Disabled,
            ReadOnlyClusterTools::default(),
            SafetyPolicy::default(),
            None,
            Vec::new(),
        );

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/internal/runs/run_x/control")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn internal_routes_reject_missing_or_wrong_worker_token() {
        use tower::ServiceExt;

        let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
        let app = router(
            store,
            RunDispatcher::Disabled,
            ReadOnlyClusterTools::default(),
            SafetyPolicy::default(),
            Some("worker-secret".to_string()),
            Vec::new(),
        );

        let missing = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/internal/runs/run_x/control")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

        let wrong = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/internal/runs/run_x/control")
                    .header("authorization", "Bearer not-the-token")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn internal_worker_contract_marks_running_and_finishes_run() {
        use tower::ServiceExt;

        let state = test_state().await;
        let session_id = SessionId::new("ses_internal_contract");
        let run_id = RunId::new("run_internal_contract");
        state
            .store
            .create_session(CreateSession {
                id: session_id.clone(),
                title: "internal contract".to_string(),
                cwd: ".".to_string(),
            })
            .await
            .unwrap();
        state
            .store
            .create_run(CreateRun {
                id: run_id.clone(),
                session_id: session_id.clone(),
                user_task: "internal contract".to_string(),
                cwd: ".".to_string(),
                max_turns: 5,
                initial_status: "queued".to_string(),
                execution_target_json: json!({ "kind": "kubernetes_job" }),
            })
            .await
            .unwrap();

        let app = router(
            state.store.clone(),
            RunDispatcher::Disabled,
            ReadOnlyClusterTools::default(),
            SafetyPolicy::default(),
            Some("worker-secret".to_string()),
            Vec::new(),
        );

        let context = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/internal/runs/run_internal_contract/attempt-context")
                    .header("authorization", "Bearer worker-secret")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(context.status(), StatusCode::OK);

        let marked = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/internal/runs/run_internal_contract/mark-running")
                    .header("authorization", "Bearer worker-secret")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(marked.status(), StatusCode::OK);

        let event = AgentEvent {
            event_id: EventId::new("evt_run_internal_contract_2"),
            session_id: session_id.clone(),
            run_id: run_id.clone(),
            seq: 2,
            kind: EventKind::RunStarted,
            payload: json!({ "source": "worker" }),
        };
        let ingested = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/internal/runs/run_internal_contract/events")
                    .header("authorization", "Bearer worker-secret")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::to_vec(&json!({ "events": [event] })).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ingested.status(), StatusCode::OK);

        let outcome = json!({
            "status": "completed",
            "turns": 1,
            "summary": "done",
            "error": null,
            "approval": null,
        });
        let finished = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/internal/runs/run_internal_contract/outcome")
                    .header("authorization", "Bearer worker-secret")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::to_vec(&outcome).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(finished.status(), StatusCode::OK);

        let run = state.store.get_run(&run_id).await.unwrap().unwrap();
        assert_eq!(run.status, "completed");
        let events = state.store.list_events(&run_id).await.unwrap();
        assert_eq!(events.len(), 1);
    }

    #[tokio::test]
    async fn router_mounts_static_and_dynamic_run_routes() {
        let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());

        let _app = router(
            store,
            RunDispatcher::Disabled,
            ReadOnlyClusterTools::default(),
            SafetyPolicy::default(),
            None,
            Vec::new(),
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

        let Json(config) = config_effective(State(state), None).await;

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
                ..Default::default()
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
        let artifact_id = response.artifact_id.clone().unwrap();
        let observation_id = response.observation_id.clone().unwrap();
        let Json(artifact) = get_artifact(State(state.clone()), Path(artifact_id.clone()))
            .await
            .unwrap();
        assert_eq!(artifact.id, artifact_id);
        assert_eq!(artifact.kind, "kubernetes_tool_result");
        assert!(artifact.run_id.is_none());
        assert_eq!(
            artifact.content_json.as_ref().unwrap()["output"]["item_count"],
            0
        );
        let Json(observations) = list_observations(
            State(state.clone()),
            Query(ListObservationsQuery {
                run_id: None,
                source: Some("kubernetes".to_string()),
                kind: Some("pods".to_string()),
                subject: None,
                resource_namespace: Some("argocd".to_string()),
                resource_kind: Some("pods".to_string()),
                resource_name: None,
                observed_after_ms: None,
                observed_before_ms: None,
                limit: Some(50),
                offset: None,
            }),
        )
        .await
        .unwrap();
        assert_eq!(observations.count, 1);
        assert_eq!(observations.observations[0].id, observation_id);
        assert_eq!(
            observations.observations[0].artifact_id.as_deref(),
            Some(artifact_id.as_str())
        );
        let Json(audit_events) = list_audit_events(
            State(state.clone()),
            Query(ListAuditEventsQuery {
                resource_kind: Some("capability".to_string()),
                resource_id: Some("kubernetes_get".to_string()),
                run_id: None,
                limit: Some(50),
                ..Default::default()
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
        let Json(observation_audit_events) = list_audit_events(
            State(state),
            Query(ListAuditEventsQuery {
                resource_kind: Some("observation".to_string()),
                resource_id: Some(observation_id),
                run_id: None,
                limit: Some(50),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        assert!(observation_audit_events
            .events
            .iter()
            .any(|event| event.kind == "observation.created"));
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
                ..Default::default()
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
                ..Default::default()
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
    async fn direct_capability_execution_accepts_registry_inspection() {
        let state = test_state().await;
        let Json(response) = execute_capability(
            State(state.clone()),
            Json(ExecuteCapabilityRequest {
                action: AgentAction::RegistryInspectImage {
                    id: "act_registry".into(),
                    reason: "inspect image evidence".to_string(),
                    image_ref: "team/checkout-api:v1".to_string(),
                    registry_base_url: None,
                },
                timeout_ms: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status, "ok");
        assert_eq!(response.action, "registry_inspect_image");
        assert!(response.executed);
        let result = response.result.unwrap();
        assert_eq!(result.content["source"], "registry");
        assert_eq!(result.content["image"]["repository"], "team/checkout-api");
        assert_eq!(result.content["verification_status"], "unknown");

        let Json(audit_events) = list_audit_events(
            State(state),
            Query(ListAuditEventsQuery {
                resource_kind: Some("capability".to_string()),
                resource_id: Some("registry_inspect_image".to_string()),
                run_id: None,
                limit: Some(50),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        assert!(audit_events.events.iter().any(|event| {
            event.kind == "direct_capability.executed"
                && event.payload["executed"] == true
                && event.payload["result"]["image"]["repository"] == "team/checkout-api"
                && event.payload["result"]["image"]["verification_status"] == "unknown"
        }));
    }

    #[tokio::test]
    async fn registry_inspection_records_registry_evidence() {
        let state = test_state().await;
        let release_id = seed_approved_release(&state).await;
        let Json(response) = create_registry_evidence_from_registry_inspection(
            State(state.clone()),
            Json(CreateRegistryEvidenceFromInspectionRequest {
                release_id: release_id.clone(),
                image_ref: "team/checkout-api:v0.1.0-smoke".to_string(),
                registry_base_url: None,
                title: None,
                summary: None,
                risk_level: None,
                actor: Some("lucas".to_string()),
                reason: Some("registry inspection smoke".to_string()),
                timeout_ms: Some(5_000),
            }),
        )
        .await
        .unwrap();

        assert!(response.created);
        assert_eq!(response.inspection.status, "ok");
        assert!(response.inspection.executed);
        let evidence = response.registry_evidence.unwrap();
        assert_eq!(evidence.release_id, release_id);
        assert_eq!(evidence.status, "proposed");
        assert_eq!(evidence.source, "registry_inspect_image");
        assert_eq!(evidence.verification_status, "unknown");
        assert_eq!(evidence.repository.as_deref(), Some("team/checkout-api"));
        assert_eq!(
            evidence.image_ref.as_deref(),
            Some("team/checkout-api:v0.1.0-smoke")
        );
        assert_eq!(
            evidence.evidence_json["execution"]["capability"],
            "registry_inspect_image"
        );
        assert_eq!(
            evidence.evidence_json["execution"]["manifest_body_persisted"],
            false
        );

        let Json(registry_audit_events) = list_audit_events(
            State(state.clone()),
            Query(ListAuditEventsQuery {
                resource_kind: Some("registry_evidence".to_string()),
                resource_id: Some(evidence.id.clone()),
                run_id: None,
                limit: Some(50),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        assert!(registry_audit_events.events.iter().any(|event| {
            event.kind == "registry_evidence.proposed"
                && event.payload["extra"]["source"] == "registry_inspection"
                && event.payload["extra"]["execution_enabled"] == true
        }));

        let Json(capability_audit_events) = list_audit_events(
            State(state),
            Query(ListAuditEventsQuery {
                resource_kind: Some("capability".to_string()),
                resource_id: Some("registry_inspect_image".to_string()),
                run_id: None,
                limit: Some(50),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        assert!(capability_audit_events.events.iter().any(|event| {
            event.kind == "direct_capability.executed"
                && event.payload["executed"] == true
                && event.payload["result"]["image"]["repository"] == "team/checkout-api"
        }));
    }

    #[tokio::test]
    async fn readiness_distinguishes_identity_evidence_from_supply_chain_evidence() {
        let state = test_state().await;
        let release_id = seed_approved_release(&state).await;
        let Json(identity_evidence) = create_registry_evidence_from_release(
            State(state.clone()),
            Json(CreateRegistryEvidenceFromReleaseRequest {
                release_id,
                title: None,
                summary: None,
                risk_level: None,
                registry: Some("registry.example.test".to_string()),
                repository: Some("checkout-api".to_string()),
                image_ref: Some("registry.example.test/checkout-api:v0.1.0-smoke".to_string()),
                image_digest: Some("sha256:deadbeef".to_string()),
                tag: Some("v0.1.0-smoke".to_string()),
                source: Some("registry_inspect_image".to_string()),
                verification_status: Some("verified".to_string()),
                evidence_json: None,
                actor: Some("lucas".to_string()),
                reason: Some("identity evidence smoke".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(verified_identity_evidence) = transition_registry_evidence(
            State(state.clone()),
            Path(identity_evidence.registry_evidence.id.clone()),
            Json(TransitionRegistryEvidenceRequest {
                target_status: "verified".to_string(),
                actor: Some("lucas".to_string()),
                reason: Some("operator accepted identity evidence".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(identity_readiness) = change_set_readiness(
            State(state.clone()),
            Path(
                verified_identity_evidence
                    .registry_evidence
                    .change_set_id
                    .clone(),
            ),
        )
        .await
        .unwrap();

        assert!(identity_readiness
            .warnings
            .iter()
            .any(|finding| finding.code == "registry_evidence_supply_chain_not_verified"));

        let state = test_state().await;
        let release_id = seed_approved_release(&state).await;
        let Json(supply_chain_evidence) = create_registry_evidence_from_release(
            State(state.clone()),
            Json(CreateRegistryEvidenceFromReleaseRequest {
                release_id,
                title: None,
                summary: None,
                risk_level: None,
                registry: Some("registry.example.test".to_string()),
                repository: Some("checkout-api".to_string()),
                image_ref: Some("registry.example.test/checkout-api:v0.1.0-smoke".to_string()),
                image_digest: Some("sha256:deadbeef".to_string()),
                tag: Some("v0.1.0-smoke".to_string()),
                source: Some("registry_inspect_image".to_string()),
                verification_status: Some("verified".to_string()),
                evidence_json: Some(serde_json::json!({
                    "verification": {
                        "checks": [
                            {"name": "cosign_signature", "status": "verified"}
                        ]
                    }
                })),
                actor: Some("lucas".to_string()),
                reason: Some("signature evidence smoke".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(verified_supply_chain_evidence) = transition_registry_evidence(
            State(state.clone()),
            Path(supply_chain_evidence.registry_evidence.id.clone()),
            Json(TransitionRegistryEvidenceRequest {
                target_status: "verified".to_string(),
                actor: Some("lucas".to_string()),
                reason: Some("operator accepted signature evidence".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(supply_chain_readiness) = change_set_readiness(
            State(state),
            Path(
                verified_supply_chain_evidence
                    .registry_evidence
                    .change_set_id
                    .clone(),
            ),
        )
        .await
        .unwrap();

        assert!(!supply_chain_readiness
            .warnings
            .iter()
            .any(|finding| finding.code == "registry_evidence_supply_chain_not_verified"));
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

    #[test]
    fn stream_start_seq_prefers_query_cursor() {
        let mut headers = HeaderMap::new();
        headers.insert("last-event-id", HeaderValue::from_static("evt_run_test_4"));

        assert_eq!(
            stream_start_seq(&headers, &StreamRunEventsQuery { after_seq: Some(9) }),
            9
        );
        assert_eq!(
            stream_start_seq(&headers, &StreamRunEventsQuery { after_seq: None }),
            4
        );
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
                ..Default::default()
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
    async fn creates_sdlc_root_chain_and_audits_each_record() {
        let state = test_state().await;
        let Json(run) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "seed SDLC roots".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(1),
                policy_mode: None,
                scope: None,
            }),
        )
        .await
        .unwrap();

        let Json(observation) = create_observation(
            State(state.clone()),
            Json(CreateObservationRequest {
                id: Some("obs_public_create".to_string()),
                session_id: None,
                run_id: Some(run.id.clone()),
                source: "smoke".to_string(),
                kind: "pipeline_run_analysis".to_string(),
                subject: "checkout-api".to_string(),
                summary: "pipeline pending approval".to_string(),
                resource_namespace: Some("apps-dev".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("pr-smoke".to_string()),
                resource_ref: Some(serde_json::json!({
                    "apiVersion": "tekton.dev/v1",
                    "kind": "PipelineRun",
                    "namespace": "apps-dev",
                    "name": "pr-smoke"
                })),
                artifact_id: None,
                data_json: Some(serde_json::json!({ "status": "running" })),
                actor: Some("test".to_string()),
                reason: Some("root smoke".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(incident) = create_incident(
            State(state.clone()),
            Json(CreateIncidentRequest {
                id: Some("inc_public_create".to_string()),
                observation_id: observation.id.clone(),
                status: Some("candidate".to_string()),
                severity: "medium".to_string(),
                title: "Pipeline needs review".to_string(),
                summary: "Pipeline is still running".to_string(),
                resource_namespace: None,
                resource_kind: None,
                resource_name: None,
                data_json: Some(serde_json::json!({ "reason": "running" })),
                actor: Some("test".to_string()),
                reason: Some("root smoke".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(plan) = create_remediation_plan(
            State(state.clone()),
            Json(CreateRemediationPlanRequest {
                id: Some("rplan_public_create".to_string()),
                incident_id: incident.id.clone(),
                status: Some("draft".to_string()),
                title: "Review pipeline".to_string(),
                summary: "Collect read-only evidence before any mutation".to_string(),
                risk_level: "medium".to_string(),
                requires_approval: Some(true),
                resource_namespace: None,
                resource_kind: None,
                resource_name: None,
                plan_json: Some(serde_json::json!({ "steps": ["inspect pipeline"] })),
                actor: Some("test".to_string()),
                reason: Some("root smoke".to_string()),
            }),
        )
        .await
        .unwrap();

        let Json(observations) = list_observations(
            State(state.clone()),
            Query(ListObservationsQuery {
                subject: Some("checkout-api".to_string()),
                limit: Some(50),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        let Json(incidents) = list_incidents(
            State(state.clone()),
            Query(ListIncidentsQuery {
                status: Some("candidate".to_string()),
                limit: Some(50),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        let Json(plans) = list_remediation_plans(
            State(state.clone()),
            Query(ListRemediationPlansQuery {
                incident_id: Some(incident.id.clone()),
                limit: Some(50),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

        assert_eq!(observation.run_id, Some(run.id));
        assert_eq!(incident.resource_namespace.as_deref(), Some("apps-dev"));
        assert_eq!(plan.incident_id, incident.id);
        assert_eq!(observations.count, 1);
        assert_eq!(incidents.count, 1);
        assert_eq!(plans.count, 1);

        for (resource_kind, resource_id, event_kind) in [
            (
                "observation",
                observation.id.as_str(),
                "observation.created",
            ),
            ("incident", incident.id.as_str(), "incident.created"),
            (
                "remediation_plan",
                plan.id.as_str(),
                "remediation_plan.created",
            ),
        ] {
            let Json(audit_events) = list_audit_events(
                State(state.clone()),
                Query(ListAuditEventsQuery {
                    resource_kind: Some(resource_kind.to_string()),
                    resource_id: Some(resource_id.to_string()),
                    run_id: None,
                    limit: Some(50),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
            assert!(audit_events
                .events
                .iter()
                .any(|event| event.kind == event_kind && event.actor.as_deref() == Some("test")));
        }
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
                ..Default::default()
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
            State(state.clone()),
            Query(ListAuditEventsQuery {
                resource_kind: Some("approval_gate".to_string()),
                resource_id: Some("agate_test".to_string()),
                run_id: None,
                limit: Some(50),
                ..Default::default()
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
        let draft_envelope_error = create_work_plan_trusted_envelope(
            State(state.clone()),
            Path(work_plan_id.clone()),
            Json(CreateTrustedEnvelopeRequest {
                subject: None,
                created_by: Some("lucas".to_string()),
                reason: "premature WorkPlan envelope".to_string(),
                environment: Some("local".to_string()),
                namespace: Some("apps-dev".to_string()),
                repo: Some("git@example.test/team/app.git".to_string()),
                branch: Some("feature/pharness".to_string()),
                production_impacting: Some(false),
                expires_at: None,
            }),
        )
        .await
        .unwrap_err();
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
        let Json(ready_before_revision) =
            work_plan_readiness(State(state.clone()), Path(work_plan_id.clone()))
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
        let staled_grant = state
            .store
            .get_permission_grant(&work_plan_envelope.grant.id)
            .await
            .unwrap()
            .unwrap();
        let Json(future_run) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "future scoped write".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
                policy_mode: None,
                scope: Some(RunScope {
                    namespace: Some("apps-dev".to_string()),
                    repo: Some("git@example.test/team/app.git".to_string()),
                    branch: Some("feature/pharness".to_string()),
                    work_plan_id: Some(approved.work_plan.id.clone()),
                    change_set_id: None,
                    production_impacting: false,
                }),
            }),
        )
        .await
        .unwrap();
        let future_run = state.store.get_run(&future_run.id).await.unwrap().unwrap();
        let Json(blocked_after_revision) =
            work_plan_readiness(State(state.clone()), Path(approved.work_plan.id.clone()))
                .await
                .unwrap();
        let Json(work_plan_audit_events) = list_audit_events(
            State(state.clone()),
            Query(ListAuditEventsQuery {
                resource_kind: Some("work_plan".to_string()),
                resource_id: Some(work_plan_id),
                run_id: None,
                limit: Some(50),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        let Json(grant_audit_events) = list_audit_events(
            State(state.clone()),
            Query(ListAuditEventsQuery {
                resource_kind: Some("permission_grant".to_string()),
                resource_id: Some(work_plan_envelope.grant.id.clone()),
                run_id: None,
                limit: Some(50),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        let Json(gate_audit_events) = list_audit_events(
            State(state.clone()),
            Query(ListAuditEventsQuery {
                resource_kind: Some("approval_gate".to_string()),
                resource_id: Some("agate_lifecycle".to_string()),
                run_id: None,
                limit: Some(50),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

        assert_eq!(draft_envelope_error.status, StatusCode::CONFLICT);
        assert_eq!(proposed.work_plan.status, "proposed");
        assert_eq!(approved.work_plan.status, "approved");
        assert_eq!(
            work_plan_envelope.grant.scope["work_plan_ids"][0],
            serde_json::json!(approved.work_plan.id.clone())
        );
        assert!(work_plan_envelope.grant.scope["change_set_ids"].is_null());
        assert_eq!(satisfied_gate.approval_gate.status, "satisfied");
        assert!(ready_before_revision.ready);
        assert!(ready_before_revision.blockers.is_empty());
        assert_eq!(ready_before_revision.trusted_envelopes.active.len(), 1);
        assert!(ready_before_revision
            .warnings
            .iter()
            .any(|finding| finding.code == "missing_change_set"));
        assert_eq!(revised.work_plan.status, "draft");
        assert_eq!(revised.work_plan.revision, 2);
        assert_eq!(staled_grant.status, "stale");
        assert_eq!(staled_grant.revoked_by.as_deref(), Some("lucas"));
        assert_eq!(
            staled_grant.revoke_reason.as_deref(),
            Some("new evidence changed execution plan")
        );
        assert!(
            future_run.execution_target_json["policy"]["permission_grants"]
                .as_array()
                .is_none_or(Vec::is_empty)
        );
        assert!(!blocked_after_revision.ready);
        assert!(blocked_after_revision
            .blockers
            .iter()
            .any(|finding| finding.code == "work_plan_not_approved"));
        assert!(blocked_after_revision
            .blockers
            .iter()
            .any(|finding| finding.code == "missing_active_trusted_envelope"));
        assert!(blocked_after_revision
            .warnings
            .iter()
            .any(|finding| finding.code == "stale_trusted_envelope"));
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
        assert!(grant_audit_events
            .events
            .iter()
            .any(|event| event.kind == "permission_grant.stale"));
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
        let Json(proposed_work_plan) = transition_work_plan(
            State(state.clone()),
            Path(created_work_plan.work_plan.id.clone()),
            Json(TransitionWorkPlanRequest {
                target_status: "proposed".to_string(),
                actor: Some("lucas".to_string()),
                reason: Some("ready for source plan review".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(approved_work_plan) = transition_work_plan(
            State(state.clone()),
            Path(created_work_plan.work_plan.id.clone()),
            Json(TransitionWorkPlanRequest {
                target_status: "approved".to_string(),
                actor: Some("lucas".to_string()),
                reason: Some("source plan approved".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(work_plan_flow_before_change_set) = work_plan_flow(
            State(state.clone()),
            Path(created_work_plan.work_plan.id.clone()),
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
        assert_eq!(work_plan_flow_before_change_set.resource_kind, "work_plan");
        assert_eq!(
            work_plan_flow_before_change_set.resource_id,
            created_work_plan.work_plan.id
        );
        assert_eq!(
            work_plan_flow_before_change_set.work_plan.id,
            approved_work_plan.work_plan.id
        );
        assert!(work_plan_flow_before_change_set.change_set.is_none());
        assert!(work_plan_flow_before_change_set.pipeline_intent.is_none());
        assert!(work_plan_flow_before_change_set
            .readiness
            .warnings
            .iter()
            .any(|finding| finding.code == "missing_change_set"));
        assert!(work_plan_flow_before_change_set
            .incidents
            .iter()
            .any(|incident| incident.id == "inc_changeset_lifecycle"));
        assert!(work_plan_flow_before_change_set
            .remediation_plans
            .iter()
            .any(|plan| plan.id == "rplan_changeset"));
        let draft_envelope_error = create_change_set_trusted_envelope(
            State(state.clone()),
            Path(change_set_id.clone()),
            Json(CreateTrustedEnvelopeRequest {
                subject: None,
                created_by: Some("lucas".to_string()),
                reason: "premature ChangeSet envelope".to_string(),
                environment: Some("local".to_string()),
                namespace: Some("apps-dev".to_string()),
                repo: Some("git@example.test/team/app.git".to_string()),
                branch: Some("feature/pharness".to_string()),
                production_impacting: Some(false),
                expires_at: None,
            }),
        )
        .await
        .unwrap_err();
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
        let Json(proposed_pipeline_intent) = create_pipeline_intent_from_change_set(
            State(state.clone()),
            Json(CreatePipelineIntentFromChangeSetRequest {
                change_set_id: change_set_id.clone(),
                title: None,
                summary: None,
                risk_level: None,
                intent_kind: None,
                intent_json: None,
                actor: Some("lucas".to_string()),
                reason: Some("pipeline intent smoke".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(existing_pipeline_intent) = create_pipeline_intent_from_change_set(
            State(state.clone()),
            Json(CreatePipelineIntentFromChangeSetRequest {
                change_set_id: change_set_id.clone(),
                title: Some("ignored duplicate".to_string()),
                summary: None,
                risk_level: None,
                intent_kind: None,
                intent_json: None,
                actor: None,
                reason: None,
            }),
        )
        .await
        .unwrap();
        let Json(listed_pipeline_intents) = list_pipeline_intents(
            State(state.clone()),
            Query(ListPipelineIntentsQuery {
                change_set_id: Some(change_set_id.clone()),
                work_plan_id: Some(created_work_plan.work_plan.id.clone()),
                remediation_plan_id: Some("rplan_changeset".to_string()),
                incident_id: Some("inc_changeset_lifecycle".to_string()),
                run_id: Some(created.id.to_string()),
                status: Some("proposed".to_string()),
                intent_kind: Some("tekton_build_test_package".to_string()),
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
        let Json(fetched_pipeline_intent) = get_pipeline_intent(
            State(state.clone()),
            Path(proposed_pipeline_intent.pipeline_intent.id.clone()),
        )
        .await
        .unwrap();
        let Json(waiting_on_pipeline_intent) =
            change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
                .await
                .unwrap();
        let Json(approved_pipeline_intent) = transition_pipeline_intent(
            State(state.clone()),
            Path(proposed_pipeline_intent.pipeline_intent.id.clone()),
            Json(TransitionPipelineIntentRequest {
                target_status: "approved".to_string(),
                actor: Some("lucas".to_string()),
                reason: Some("pipeline intent approved".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(pipeline_observation) = create_observation(
            State(state.clone()),
            Json(CreateObservationRequest {
                id: Some("obs_pipeline_intent_evidence".to_string()),
                session_id: None,
                run_id: None,
                source: "tekton".to_string(),
                kind: "pipeline_run_analysis".to_string(),
                subject: "ci/build-api".to_string(),
                summary: "PipelineRun build-api succeeded".to_string(),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-api".to_string()),
                resource_ref: Some(json!({
                    "source": "tekton",
                    "kind": "PipelineRun",
                    "namespace": "ci",
                    "name": "build-api",
                })),
                artifact_id: None,
                data_json: Some(json!({
                    "analysis": {
                        "kind": "PipelineRunAnalysis",
                        "summary": {
                            "status": "succeeded",
                            "reason": "Succeeded",
                            "task_run_count": 3,
                            "failed_task_run_count": 0,
                            "running_task_run_count": 0,
                            "succeeded_task_run_count": 3,
                            "argo_sync_status": "Synced",
                            "argo_health_status": "Healthy",
                            "image_alignment": {
                                "status": "exact_match"
                            }
                        }
                    }
                })),
                actor: Some("lucas".to_string()),
                reason: Some("pipeline evidence fixture".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(pipeline_intent_with_evidence) = attach_pipeline_intent_evidence(
            State(state.clone()),
            Path(proposed_pipeline_intent.pipeline_intent.id.clone()),
            Json(AttachPipelineIntentEvidenceRequest {
                observation_id: pipeline_observation.id.clone(),
                actor: Some("lucas".to_string()),
                reason: Some("pipeline evidence reviewed".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(waiting_on_deployment_intent) =
            change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
                .await
                .unwrap();
        let Json(proposed_deployment_intent) = create_deployment_intent_from_pipeline_intent(
            State(state.clone()),
            Json(CreateDeploymentIntentFromPipelineIntentRequest {
                pipeline_intent_id: proposed_pipeline_intent.pipeline_intent.id.clone(),
                title: None,
                summary: None,
                risk_level: None,
                intent_kind: None,
                target_environment: Some("dev".to_string()),
                target_namespace: Some("apps-dev".to_string()),
                argo_application: Some("checkout-api".to_string()),
                intent_json: None,
                actor: Some("lucas".to_string()),
                reason: Some("deployment intent smoke".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(existing_deployment_intent) = create_deployment_intent_from_pipeline_intent(
            State(state.clone()),
            Json(CreateDeploymentIntentFromPipelineIntentRequest {
                pipeline_intent_id: proposed_pipeline_intent.pipeline_intent.id.clone(),
                title: Some("ignored duplicate".to_string()),
                summary: None,
                risk_level: None,
                intent_kind: None,
                target_environment: None,
                target_namespace: None,
                argo_application: None,
                intent_json: None,
                actor: None,
                reason: None,
            }),
        )
        .await
        .unwrap();
        let Json(listed_deployment_intents) = list_deployment_intents(
            State(state.clone()),
            Query(ListDeploymentIntentsQuery {
                pipeline_intent_id: Some(proposed_pipeline_intent.pipeline_intent.id.clone()),
                change_set_id: Some(change_set_id.clone()),
                work_plan_id: Some(created_work_plan.work_plan.id.clone()),
                remediation_plan_id: Some("rplan_changeset".to_string()),
                incident_id: Some("inc_changeset_lifecycle".to_string()),
                run_id: Some(created.id.to_string()),
                status: Some("proposed".to_string()),
                intent_kind: Some("argo_sync_deploy".to_string()),
                risk_level: Some("medium".to_string()),
                target_environment: Some("dev".to_string()),
                target_namespace: Some("apps-dev".to_string()),
                argo_application: Some("checkout-api".to_string()),
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
        let Json(fetched_deployment_intent) = get_deployment_intent(
            State(state.clone()),
            Path(proposed_deployment_intent.deployment_intent.id.clone()),
        )
        .await
        .unwrap();
        let Json(waiting_on_deployment_approval) =
            change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
                .await
                .unwrap();
        let Json(approved_deployment_intent) = transition_deployment_intent(
            State(state.clone()),
            Path(proposed_deployment_intent.deployment_intent.id.clone()),
            Json(TransitionDeploymentIntentRequest {
                target_status: "approved".to_string(),
                actor: Some("lucas".to_string()),
                reason: Some("deployment intent approved".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(deployment_observation) = create_observation(
            State(state.clone()),
            Json(CreateObservationRequest {
                id: Some("obs_deployment_intent_evidence".to_string()),
                session_id: None,
                run_id: None,
                source: "argocd".to_string(),
                kind: "applications.argoproj.io".to_string(),
                subject: "checkout-api".to_string(),
                summary: "Argo Application checkout-api is synced and healthy".to_string(),
                resource_namespace: Some("argocd".to_string()),
                resource_kind: Some("Application".to_string()),
                resource_name: Some("checkout-api".to_string()),
                resource_ref: Some(json!({
                    "source": "argocd",
                    "kind": "Application",
                    "namespace": "argocd",
                    "name": "checkout-api",
                })),
                artifact_id: None,
                data_json: Some(json!({
                    "source": "argocd",
                    "resource": "applications.argoproj.io",
                    "namespace": "argocd",
                    "name": "checkout-api",
                    "output": {
                        "apiVersion": "argoproj.io/v1alpha1",
                        "kind": "Application",
                        "metadata": {
                            "namespace": "argocd",
                            "name": "checkout-api"
                        },
                        "status": {
                            "sync": {
                                "status": "Synced",
                                "revision": "abc1234"
                            },
                            "health": {
                                "status": "Healthy"
                            }
                        }
                    }
                })),
                actor: Some("lucas".to_string()),
                reason: Some("deployment evidence fixture".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(deployment_intent_with_evidence) = attach_deployment_intent_evidence(
            State(state.clone()),
            Path(proposed_deployment_intent.deployment_intent.id.clone()),
            Json(AttachDeploymentIntentEvidenceRequest {
                observation_id: deployment_observation.id.clone(),
                actor: Some("lucas".to_string()),
                reason: Some("deployment evidence reviewed".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(waiting_on_release) =
            change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
                .await
                .unwrap();
        let Json(proposed_release) = create_release_from_deployment_intent(
            State(state.clone()),
            Json(CreateReleaseFromDeploymentIntentRequest {
                deployment_intent_id: proposed_deployment_intent.deployment_intent.id.clone(),
                title: None,
                summary: None,
                risk_level: None,
                release_kind: None,
                version: Some("v0.1.0-smoke".to_string()),
                commit_sha: Some("abc1234".to_string()),
                image_digest: Some("sha256:deadbeef".to_string()),
                rollback_ref: Some("previous-release".to_string()),
                release_json: None,
                actor: Some("lucas".to_string()),
                reason: Some("release smoke".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(existing_release) = create_release_from_deployment_intent(
            State(state.clone()),
            Json(CreateReleaseFromDeploymentIntentRequest {
                deployment_intent_id: proposed_deployment_intent.deployment_intent.id.clone(),
                title: Some("ignored duplicate".to_string()),
                summary: None,
                risk_level: None,
                release_kind: None,
                version: None,
                commit_sha: None,
                image_digest: None,
                rollback_ref: None,
                release_json: None,
                actor: None,
                reason: None,
            }),
        )
        .await
        .unwrap();
        let Json(listed_releases) = list_releases(
            State(state.clone()),
            Query(ListReleasesQuery {
                deployment_intent_id: Some(proposed_deployment_intent.deployment_intent.id.clone()),
                pipeline_intent_id: Some(proposed_pipeline_intent.pipeline_intent.id.clone()),
                change_set_id: Some(change_set_id.clone()),
                work_plan_id: Some(created_work_plan.work_plan.id.clone()),
                remediation_plan_id: Some("rplan_changeset".to_string()),
                incident_id: Some("inc_changeset_lifecycle".to_string()),
                run_id: Some(created.id.to_string()),
                status: Some("proposed".to_string()),
                release_kind: Some("gitops_release".to_string()),
                risk_level: Some("medium".to_string()),
                target_environment: Some("dev".to_string()),
                target_namespace: Some("apps-dev".to_string()),
                argo_application: Some("checkout-api".to_string()),
                version: Some("v0.1.0-smoke".to_string()),
                commit_sha: Some("abc1234".to_string()),
                image_digest: Some("sha256:deadbeef".to_string()),
                created_after_ms: Some(0),
                created_before_ms: None,
                limit: Some(10),
                offset: Some(0),
            }),
        )
        .await
        .unwrap();
        let Json(fetched_release) = get_release(
            State(state.clone()),
            Path(proposed_release.release.id.clone()),
        )
        .await
        .unwrap();
        let Json(waiting_on_release_approval) =
            change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
                .await
                .unwrap();
        let Json(approved_release) = transition_release(
            State(state.clone()),
            Path(proposed_release.release.id.clone()),
            Json(TransitionReleaseRequest {
                target_status: "approved".to_string(),
                actor: Some("lucas".to_string()),
                reason: Some("release approved".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(release_observation) = create_observation(
            State(state.clone()),
            Json(CreateObservationRequest {
                id: Some("obs_release_observability".to_string()),
                session_id: None,
                run_id: None,
                source: "prometheus".to_string(),
                kind: "inventory".to_string(),
                subject: "prometheus/inventory".to_string(),
                summary: "Prometheus inventory has no active alerts".to_string(),
                resource_namespace: None,
                resource_kind: Some("PrometheusInventory".to_string()),
                resource_name: Some("default".to_string()),
                resource_ref: Some(json!({
                    "source": "prometheus",
                    "kind": "inventory",
                })),
                artifact_id: None,
                data_json: Some(json!({
                    "source": "prometheus",
                    "resource": "inventory",
                    "inventory": {
                        "targets": {
                            "active_count": 3,
                            "unhealthy_count": 0
                        },
                        "rules": {
                            "rule_count": 2,
                            "problem_rule_count": 0
                        },
                        "alerts": {
                            "alert_count": 0
                        }
                    }
                })),
                actor: Some("lucas".to_string()),
                reason: Some("release observability fixture".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(release_with_observability) = attach_release_evidence(
            State(state.clone()),
            Path(proposed_release.release.id.clone()),
            Json(AttachReleaseEvidenceRequest {
                observation_id: release_observation.id.clone(),
                actor: Some("lucas".to_string()),
                reason: Some("release observability reviewed".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(release_alert_observation) = create_observation(
            State(state.clone()),
            Json(CreateObservationRequest {
                id: Some("obs_release_observability_alert".to_string()),
                session_id: None,
                run_id: None,
                source: "prometheus".to_string(),
                kind: "inventory".to_string(),
                subject: "prometheus/inventory".to_string(),
                summary: "Prometheus inventory has active alerts".to_string(),
                resource_namespace: None,
                resource_kind: Some("PrometheusInventory".to_string()),
                resource_name: Some("default".to_string()),
                resource_ref: Some(json!({
                    "source": "prometheus",
                    "kind": "inventory",
                })),
                artifact_id: None,
                data_json: Some(json!({
                    "source": "prometheus",
                    "resource": "inventory",
                    "inventory": {
                        "targets": {
                            "active_count": 3,
                            "unhealthy_count": 1
                        },
                        "rules": {
                            "rule_count": 2,
                            "problem_rule_count": 1
                        },
                        "alerts": {
                            "alert_count": 1
                        }
                    }
                })),
                actor: Some("lucas".to_string()),
                reason: Some("release observability alert fixture".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(release_with_observability_incident) = attach_release_evidence(
            State(state.clone()),
            Path(proposed_release.release.id.clone()),
            Json(AttachReleaseEvidenceRequest {
                observation_id: release_alert_observation.id.clone(),
                actor: Some("lucas".to_string()),
                reason: Some("release alert evidence reviewed".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(waiting_on_registry_evidence) =
            change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
                .await
                .unwrap();
        let Json(proposed_registry_evidence) = create_registry_evidence_from_release(
            State(state.clone()),
            Json(CreateRegistryEvidenceFromReleaseRequest {
                release_id: proposed_release.release.id.clone(),
                title: None,
                summary: None,
                risk_level: None,
                registry: Some("registry.example.test".to_string()),
                repository: Some("checkout-api".to_string()),
                image_ref: Some("registry.example.test/checkout-api:v0.1.0-smoke".to_string()),
                image_digest: None,
                tag: Some("v0.1.0-smoke".to_string()),
                source: Some("manual".to_string()),
                verification_status: Some("verified".to_string()),
                evidence_json: None,
                actor: Some("lucas".to_string()),
                reason: Some("registry evidence smoke".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(existing_registry_evidence) = create_registry_evidence_from_release(
            State(state.clone()),
            Json(CreateRegistryEvidenceFromReleaseRequest {
                release_id: proposed_release.release.id.clone(),
                title: Some("ignored duplicate".to_string()),
                summary: None,
                risk_level: None,
                registry: None,
                repository: None,
                image_ref: None,
                image_digest: None,
                tag: None,
                source: None,
                verification_status: None,
                evidence_json: None,
                actor: None,
                reason: None,
            }),
        )
        .await
        .unwrap();
        let Json(listed_registry_evidence) = list_registry_evidence(
            State(state.clone()),
            Query(ListRegistryEvidenceQuery {
                release_id: Some(proposed_release.release.id.clone()),
                deployment_intent_id: Some(proposed_deployment_intent.deployment_intent.id.clone()),
                pipeline_intent_id: Some(proposed_pipeline_intent.pipeline_intent.id.clone()),
                change_set_id: Some(change_set_id.clone()),
                work_plan_id: Some(created_work_plan.work_plan.id.clone()),
                remediation_plan_id: Some("rplan_changeset".to_string()),
                incident_id: Some("inc_changeset_lifecycle".to_string()),
                run_id: Some(created.id.to_string()),
                status: Some("proposed".to_string()),
                risk_level: Some("medium".to_string()),
                registry: Some("registry.example.test".to_string()),
                repository: Some("checkout-api".to_string()),
                image_ref: None,
                image_digest: Some("sha256:deadbeef".to_string()),
                tag: Some("v0.1.0-smoke".to_string()),
                source: Some("manual".to_string()),
                verification_status: Some("verified".to_string()),
                created_after_ms: Some(0),
                created_before_ms: None,
                limit: Some(10),
                offset: Some(0),
            }),
        )
        .await
        .unwrap();
        let Json(fetched_registry_evidence) = get_registry_evidence(
            State(state.clone()),
            Path(proposed_registry_evidence.registry_evidence.id.clone()),
        )
        .await
        .unwrap();
        let Json(waiting_on_registry_evidence_verification) =
            change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
                .await
                .unwrap();
        let Json(verified_registry_evidence) = transition_registry_evidence(
            State(state.clone()),
            Path(proposed_registry_evidence.registry_evidence.id.clone()),
            Json(TransitionRegistryEvidenceRequest {
                target_status: "verified".to_string(),
                actor: Some("lucas".to_string()),
                reason: Some("registry evidence verified".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(ready_before_revision) =
            change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
                .await
                .unwrap();
        let Json(flow_before_revision) =
            change_set_flow(State(state.clone()), Path(change_set_id.clone()))
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
        let staled_grant = state
            .store
            .get_permission_grant(&change_set_envelope.grant.id)
            .await
            .unwrap()
            .unwrap();
        let Json(future_run) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "future scoped changeset write".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
                policy_mode: None,
                scope: Some(RunScope {
                    namespace: Some("apps-dev".to_string()),
                    repo: Some("git@example.test/team/app.git".to_string()),
                    branch: Some("feature/pharness".to_string()),
                    work_plan_id: Some(created_work_plan.work_plan.id.clone()),
                    change_set_id: Some(change_set_id.clone()),
                    production_impacting: false,
                }),
            }),
        )
        .await
        .unwrap();
        let future_run = state.store.get_run(&future_run.id).await.unwrap().unwrap();
        let Json(blocked_after_revision) =
            change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
                .await
                .unwrap();
        let Json(_reproposed_change_set) = transition_change_set(
            State(state.clone()),
            Path(change_set_id.clone()),
            Json(TransitionChangeSetRequest {
                target_status: "proposed".to_string(),
                actor: Some("lucas".to_string()),
                reason: Some("source change ready again".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(_approved_revised_change_set) = transition_change_set(
            State(state.clone()),
            Path(change_set_id.clone()),
            Json(TransitionChangeSetRequest {
                target_status: "approved".to_string(),
                actor: Some("lucas".to_string()),
                reason: Some("revised source change approved".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(reproposed_pipeline_intent) = create_pipeline_intent_from_change_set(
            State(state.clone()),
            Json(CreatePipelineIntentFromChangeSetRequest {
                change_set_id: change_set_id.clone(),
                title: None,
                summary: None,
                risk_level: None,
                intent_kind: None,
                intent_json: None,
                actor: Some("lucas".to_string()),
                reason: Some("pipeline intent after source revision".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(waiting_on_reproposed_pipeline_intent) =
            change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
                .await
                .unwrap();
        let Json(approved_reproposed_pipeline_intent) = transition_pipeline_intent(
            State(state.clone()),
            Path(proposed_pipeline_intent.pipeline_intent.id.clone()),
            Json(TransitionPipelineIntentRequest {
                target_status: "approved".to_string(),
                actor: Some("lucas".to_string()),
                reason: Some("reproposed pipeline intent approved".to_string()),
            }),
        )
        .await
        .unwrap();
        state
            .store
            .create_observation(CreateObservation {
                id: "obs_reproposed_pipeline_evidence".to_string(),
                session_id: SessionId::new(format!("ses_{}", created.id.as_str())),
                run_id: Some(created.id.clone()),
                source: "tekton".to_string(),
                kind: "pipeline_run_analysis".to_string(),
                subject: "build-api".to_string(),
                summary: "Reproposed PipelineRun completed successfully".to_string(),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-api".to_string()),
                resource_ref_json: None,
                artifact_id: None,
                data_json: json!({
                    "analysis": {
                        "summary": {
                            "status": "succeeded",
                            "failed_task_run_count": 0,
                            "running_task_run_count": 0,
                            "succeeded_task_run_count": 1,
                            "image_alignment": { "status": "exact_match" }
                        }
                    }
                }),
            })
            .await
            .unwrap();
        let Json(_reproposed_pipeline_evidence) = attach_pipeline_intent_evidence(
            State(state.clone()),
            Path(proposed_pipeline_intent.pipeline_intent.id.clone()),
            Json(AttachPipelineIntentEvidenceRequest {
                observation_id: "obs_reproposed_pipeline_evidence".to_string(),
                actor: Some("lucas".to_string()),
                reason: Some("reproposed PipelineRun evidence reviewed".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(waiting_on_reproposed_deployment_intent) =
            change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
                .await
                .unwrap();
        let Json(reproposed_deployment_intent) = create_deployment_intent_from_pipeline_intent(
            State(state.clone()),
            Json(CreateDeploymentIntentFromPipelineIntentRequest {
                pipeline_intent_id: proposed_pipeline_intent.pipeline_intent.id.clone(),
                title: None,
                summary: None,
                risk_level: None,
                intent_kind: None,
                target_environment: Some("dev".to_string()),
                target_namespace: Some("apps-dev".to_string()),
                argo_application: Some("checkout-api".to_string()),
                intent_json: None,
                actor: Some("lucas".to_string()),
                reason: Some("deployment intent after pipeline reproposal".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(waiting_on_reproposed_deployment_approval) =
            change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
                .await
                .unwrap();
        let Json(approved_reproposed_deployment_intent) = transition_deployment_intent(
            State(state.clone()),
            Path(proposed_deployment_intent.deployment_intent.id.clone()),
            Json(TransitionDeploymentIntentRequest {
                target_status: "approved".to_string(),
                actor: Some("lucas".to_string()),
                reason: Some("reproposed deployment intent approved".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(waiting_on_reproposed_release) =
            change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
                .await
                .unwrap();
        let Json(reproposed_release) = create_release_from_deployment_intent(
            State(state.clone()),
            Json(CreateReleaseFromDeploymentIntentRequest {
                deployment_intent_id: proposed_deployment_intent.deployment_intent.id.clone(),
                title: None,
                summary: None,
                risk_level: None,
                release_kind: None,
                version: Some("v0.1.1-smoke".to_string()),
                commit_sha: Some("def5678".to_string()),
                image_digest: Some("sha256:feedface".to_string()),
                rollback_ref: Some(proposed_release.release.id.clone()),
                release_json: None,
                actor: Some("lucas".to_string()),
                reason: Some("release after deployment reproposal".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(waiting_on_reproposed_release_approval) =
            change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
                .await
                .unwrap();
        let Json(approved_reproposed_release) = transition_release(
            State(state.clone()),
            Path(proposed_release.release.id.clone()),
            Json(TransitionReleaseRequest {
                target_status: "approved".to_string(),
                actor: Some("lucas".to_string()),
                reason: Some("reproposed release approved".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(waiting_on_reproposed_registry_evidence) =
            change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
                .await
                .unwrap();
        let Json(reproposed_registry_evidence) = create_registry_evidence_from_release(
            State(state.clone()),
            Json(CreateRegistryEvidenceFromReleaseRequest {
                release_id: proposed_release.release.id.clone(),
                title: None,
                summary: None,
                risk_level: None,
                registry: Some("registry.example.test".to_string()),
                repository: Some("checkout-api".to_string()),
                image_ref: Some("registry.example.test/checkout-api:v0.1.1-smoke".to_string()),
                image_digest: None,
                tag: Some("v0.1.1-smoke".to_string()),
                source: Some("manual".to_string()),
                verification_status: Some("verified".to_string()),
                evidence_json: None,
                actor: Some("lucas".to_string()),
                reason: Some("registry evidence after release reproposal".to_string()),
            }),
        )
        .await
        .unwrap();
        let Json(verified_reproposed_registry_evidence) = transition_registry_evidence(
            State(state.clone()),
            Path(proposed_registry_evidence.registry_evidence.id.clone()),
            Json(TransitionRegistryEvidenceRequest {
                target_status: "verified".to_string(),
                actor: Some("lucas".to_string()),
                reason: Some("reproposed registry evidence verified".to_string()),
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
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        let Json(grant_audit_events) = list_audit_events(
            State(state.clone()),
            Query(ListAuditEventsQuery {
                resource_kind: Some("permission_grant".to_string()),
                resource_id: Some(change_set_envelope.grant.id.clone()),
                run_id: None,
                limit: Some(50),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        let Json(pipeline_intent_audit_events) = list_audit_events(
            State(state.clone()),
            Query(ListAuditEventsQuery {
                resource_kind: Some("pipeline_intent".to_string()),
                resource_id: Some(proposed_pipeline_intent.pipeline_intent.id.clone()),
                run_id: None,
                limit: Some(50),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        let Json(deployment_intent_audit_events) = list_audit_events(
            State(state.clone()),
            Query(ListAuditEventsQuery {
                resource_kind: Some("deployment_intent".to_string()),
                resource_id: Some(proposed_deployment_intent.deployment_intent.id.clone()),
                run_id: None,
                limit: Some(50),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        let Json(release_audit_events) = list_audit_events(
            State(state.clone()),
            Query(ListAuditEventsQuery {
                resource_kind: Some("release".to_string()),
                resource_id: Some(proposed_release.release.id.clone()),
                run_id: None,
                limit: Some(50),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        let Json(registry_evidence_audit_events) = list_audit_events(
            State(state.clone()),
            Query(ListAuditEventsQuery {
                resource_kind: Some("registry_evidence".to_string()),
                resource_id: Some(proposed_registry_evidence.registry_evidence.id.clone()),
                run_id: None,
                limit: Some(50),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        let Json(gate_audit_events) = list_audit_events(
            State(state.clone()),
            Query(ListAuditEventsQuery {
                resource_kind: Some("approval_gate".to_string()),
                resource_id: Some("agate_changeset".to_string()),
                run_id: None,
                limit: Some(50),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

        assert!(created_change_set.created);
        assert!(!existing_change_set.created);
        assert_eq!(listed_change_sets.count, 1);
        assert_eq!(listed_change_sets.change_sets[0].revision, 1);
        assert_eq!(proposed_work_plan.work_plan.status, "proposed");
        assert_eq!(approved_work_plan.work_plan.status, "approved");
        assert_eq!(draft_envelope_error.status, StatusCode::CONFLICT);
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
        assert!(proposed_pipeline_intent.created);
        assert!(!existing_pipeline_intent.created);
        assert_eq!(
            existing_pipeline_intent.pipeline_intent.id,
            proposed_pipeline_intent.pipeline_intent.id
        );
        assert_eq!(listed_pipeline_intents.count, 1);
        assert_eq!(
            fetched_pipeline_intent.id,
            proposed_pipeline_intent.pipeline_intent.id
        );
        assert_eq!(proposed_pipeline_intent.pipeline_intent.status, "proposed");
        assert_eq!(
            proposed_pipeline_intent.pipeline_intent.intent_kind,
            "tekton_build_test_package"
        );
        assert!(
            !proposed_pipeline_intent.pipeline_intent.intent_json["execution"]["enabled"]
                .as_bool()
                .unwrap()
        );
        assert!(waiting_on_pipeline_intent.ready);
        assert!(waiting_on_pipeline_intent
            .warnings
            .iter()
            .any(|finding| finding.code == "pipeline_intent_not_approved"));
        assert_eq!(approved_pipeline_intent.pipeline_intent.status, "approved");
        assert_eq!(
            pipeline_intent_with_evidence
                .pipeline_intent
                .intent_json
                .pointer("/evidence/status"),
            Some(&json!("satisfied"))
        );
        assert_eq!(
            pipeline_intent_with_evidence
                .pipeline_intent
                .intent_json
                .pointer("/evidence/observation_id"),
            Some(&json!("obs_pipeline_intent_evidence"))
        );
        assert_eq!(
            pipeline_intent_with_evidence.observation.id,
            pipeline_observation.id
        );
        assert!(waiting_on_deployment_intent.ready);
        assert!(waiting_on_deployment_intent
            .warnings
            .iter()
            .any(|finding| finding.code == "missing_deployment_intent"));
        assert!(proposed_deployment_intent.created);
        assert!(!existing_deployment_intent.created);
        assert_eq!(
            existing_deployment_intent.deployment_intent.id,
            proposed_deployment_intent.deployment_intent.id
        );
        assert_eq!(listed_deployment_intents.count, 1);
        assert_eq!(
            fetched_deployment_intent.id,
            proposed_deployment_intent.deployment_intent.id
        );
        assert_eq!(
            proposed_deployment_intent.deployment_intent.status,
            "proposed"
        );
        assert_eq!(
            proposed_deployment_intent.deployment_intent.intent_kind,
            "argo_sync_deploy"
        );
        assert_eq!(
            proposed_deployment_intent
                .deployment_intent
                .target_environment
                .as_deref(),
            Some("dev")
        );
        assert_eq!(
            proposed_deployment_intent
                .deployment_intent
                .target_namespace
                .as_deref(),
            Some("apps-dev")
        );
        assert_eq!(
            proposed_deployment_intent
                .deployment_intent
                .argo_application
                .as_deref(),
            Some("checkout-api")
        );
        assert!(
            !proposed_deployment_intent.deployment_intent.intent_json["execution"]["enabled"]
                .as_bool()
                .unwrap()
        );
        assert_eq!(
            proposed_deployment_intent
                .deployment_intent
                .intent_json
                .pointer("/pipeline_evidence/status"),
            Some(&json!("satisfied"))
        );
        assert_eq!(
            proposed_deployment_intent
                .deployment_intent
                .intent_json
                .pointer("/pipeline_evidence/deploy_ready"),
            Some(&json!(true))
        );
        assert_eq!(
            proposed_deployment_intent
                .deployment_intent
                .intent_json
                .pointer("/pipeline_evidence/observation_id"),
            Some(&json!("obs_pipeline_intent_evidence"))
        );
        assert!(waiting_on_deployment_approval
            .warnings
            .iter()
            .any(|finding| finding.code == "deployment_intent_not_approved"));
        assert_eq!(
            approved_deployment_intent.deployment_intent.status,
            "approved"
        );
        assert_eq!(
            deployment_intent_with_evidence
                .deployment_intent
                .intent_json
                .pointer("/deployment_evidence/status"),
            Some(&json!("satisfied"))
        );
        assert_eq!(
            deployment_intent_with_evidence
                .deployment_intent
                .intent_json
                .pointer("/deployment_evidence/deploy_ready"),
            Some(&json!(true))
        );
        assert_eq!(
            deployment_intent_with_evidence
                .deployment_intent
                .intent_json
                .pointer("/deployment_evidence/observation_id"),
            Some(&json!("obs_deployment_intent_evidence"))
        );
        assert_eq!(
            deployment_intent_with_evidence.observation.id,
            deployment_observation.id
        );
        assert!(waiting_on_release.ready);
        assert!(waiting_on_release
            .warnings
            .iter()
            .any(|finding| finding.code == "missing_release"));
        assert!(proposed_release.created);
        assert!(!existing_release.created);
        assert_eq!(existing_release.release.id, proposed_release.release.id);
        assert_eq!(listed_releases.count, 1);
        assert_eq!(fetched_release.id, proposed_release.release.id);
        assert_eq!(proposed_release.release.status, "proposed");
        assert_eq!(proposed_release.release.release_kind, "gitops_release");
        assert_eq!(
            proposed_release.release.target_environment.as_deref(),
            Some("dev")
        );
        assert_eq!(
            proposed_release.release.target_namespace.as_deref(),
            Some("apps-dev")
        );
        assert_eq!(
            proposed_release.release.argo_application.as_deref(),
            Some("checkout-api")
        );
        assert_eq!(
            proposed_release.release.version.as_deref(),
            Some("v0.1.0-smoke")
        );
        assert!(
            !proposed_release.release.release_json["execution"]["enabled"]
                .as_bool()
                .unwrap()
        );
        assert_eq!(
            proposed_release
                .release
                .release_json
                .pointer("/deployment_evidence/status"),
            Some(&json!("satisfied"))
        );
        assert_eq!(
            proposed_release
                .release
                .release_json
                .pointer("/deployment_evidence/release_ready"),
            Some(&json!(true))
        );
        assert_eq!(
            proposed_release
                .release
                .release_json
                .pointer("/deployment_evidence/observation_id"),
            Some(&json!("obs_deployment_intent_evidence"))
        );
        assert!(waiting_on_release_approval
            .warnings
            .iter()
            .any(|finding| finding.code == "release_not_approved"));
        assert_eq!(approved_release.release.status, "approved");
        assert_eq!(release_with_observability.release.status, "approved");
        assert_eq!(
            release_with_observability
                .release
                .release_json
                .pointer("/observability_evidence/0/observation_id"),
            Some(&json!("obs_release_observability"))
        );
        assert_eq!(
            release_with_observability
                .release
                .release_json
                .pointer("/observability_evidence/0/status"),
            Some(&json!("observed"))
        );
        assert_eq!(
            release_with_observability.observation.id,
            release_observation.id
        );
        assert!(release_with_observability.incident.is_none());
        assert!(release_with_observability.remediation_plan.is_none());
        let release_incident = release_with_observability_incident
            .incident
            .as_ref()
            .expect("attention-required release observability should create an incident");
        let release_remediation_plan = release_with_observability_incident
            .remediation_plan
            .as_ref()
            .expect("attention-required release observability should create a remediation plan");
        let release_remediation_gates = state
            .store
            .list_approval_gates(ApprovalGateListFilter {
                remediation_plan_id: Some(release_remediation_plan.id.clone()),
                incident_id: Some(release_incident.id.clone()),
                limit: 20,
                ..ApprovalGateListFilter::default()
            })
            .await
            .unwrap();
        assert_eq!(release_incident.status, "candidate");
        assert_eq!(release_incident.severity, "high");
        assert_eq!(
            release_incident.observation_id,
            "obs_release_observability_alert"
        );
        assert_eq!(release_remediation_plan.status, "draft");
        assert_eq!(release_remediation_plan.incident_id, release_incident.id);
        assert!(release_remediation_plan.requires_approval);
        assert_eq!(
            release_remediation_plan.plan_json.pointer("/source"),
            Some(&json!("release_observability_evidence"))
        );
        assert_eq!(release_remediation_gates.len(), 4);
        assert!(release_remediation_gates
            .iter()
            .any(|gate| gate.gate_kind == "cluster_mutation"));
        assert!(release_remediation_gates
            .iter()
            .all(|gate| gate.status == "pending"));
        assert_eq!(
            release_with_observability_incident
                .release
                .release_json
                .pointer("/observability_evidence/1/status"),
            Some(&json!("attention_required"))
        );
        assert!(waiting_on_registry_evidence
            .warnings
            .iter()
            .any(|finding| finding.code == "missing_registry_evidence"));
        assert!(!waiting_on_registry_evidence
            .warnings
            .iter()
            .any(|finding| finding.code == "missing_release_observability_evidence"));
        assert!(waiting_on_registry_evidence
            .warnings
            .iter()
            .any(|finding| finding.code == "release_observability_attention_required"));
        assert!(proposed_registry_evidence.created);
        assert!(!existing_registry_evidence.created);
        assert_eq!(
            existing_registry_evidence.registry_evidence.id,
            proposed_registry_evidence.registry_evidence.id
        );
        assert_eq!(listed_registry_evidence.count, 1);
        assert_eq!(
            fetched_registry_evidence.id,
            proposed_registry_evidence.registry_evidence.id
        );
        assert_eq!(
            proposed_registry_evidence.registry_evidence.status,
            "proposed"
        );
        assert_eq!(
            proposed_registry_evidence
                .registry_evidence
                .verification_status,
            "verified"
        );
        assert_eq!(
            proposed_registry_evidence
                .registry_evidence
                .image_digest
                .as_deref(),
            Some("sha256:deadbeef")
        );
        assert!(waiting_on_registry_evidence_verification
            .warnings
            .iter()
            .any(|finding| finding.code == "registry_evidence_not_verified"));
        assert_eq!(
            verified_registry_evidence.registry_evidence.status,
            "verified"
        );
        assert!(ready_before_revision.ready);
        assert!(ready_before_revision.blockers.is_empty());
        assert!(!ready_before_revision
            .warnings
            .iter()
            .any(|finding| finding.code == "pipeline_intent_not_approved"));
        assert!(!ready_before_revision
            .warnings
            .iter()
            .any(|finding| finding.code == "missing_deployment_intent"));
        assert!(!ready_before_revision
            .warnings
            .iter()
            .any(|finding| finding.code == "deployment_intent_not_approved"));
        assert!(!ready_before_revision
            .warnings
            .iter()
            .any(|finding| finding.code == "missing_release"));
        assert!(!ready_before_revision
            .warnings
            .iter()
            .any(|finding| finding.code == "release_not_approved"));
        assert!(!ready_before_revision
            .warnings
            .iter()
            .any(|finding| finding.code == "missing_registry_evidence"));
        assert!(!ready_before_revision
            .warnings
            .iter()
            .any(|finding| finding.code == "registry_evidence_not_verified"));
        assert_eq!(
            ready_before_revision
                .deployment_intent
                .as_ref()
                .map(|intent| intent.id.as_str()),
            Some(approved_deployment_intent.deployment_intent.id.as_str())
        );
        assert_eq!(
            ready_before_revision
                .release
                .as_ref()
                .map(|release| release.id.as_str()),
            Some(approved_release.release.id.as_str())
        );
        assert_eq!(
            ready_before_revision
                .registry_evidence
                .as_ref()
                .map(|evidence| evidence.id.as_str()),
            Some(verified_registry_evidence.registry_evidence.id.as_str())
        );
        assert_eq!(ready_before_revision.trusted_envelopes.active.len(), 1);
        assert_eq!(flow_before_revision.resource_kind, "change_set");
        assert_eq!(flow_before_revision.resource_id, change_set_id);
        assert!(flow_before_revision.readiness.ready);
        assert_eq!(
            flow_before_revision
                .change_set
                .as_ref()
                .map(|change_set| change_set.id.as_str()),
            Some(approved.change_set.id.as_str())
        );
        assert_eq!(
            flow_before_revision
                .pipeline_intent
                .as_ref()
                .map(|intent| intent.id.as_str()),
            Some(approved_pipeline_intent.pipeline_intent.id.as_str())
        );
        assert_eq!(
            flow_before_revision
                .release
                .as_ref()
                .map(|release| release.id.as_str()),
            Some(approved_release.release.id.as_str())
        );
        assert!(flow_before_revision
            .incidents
            .iter()
            .any(|incident| incident.id == release_incident.id));
        assert!(flow_before_revision
            .remediation_plans
            .iter()
            .any(|plan| plan.id == release_remediation_plan.id));
        assert!(flow_before_revision
            .approval_gates
            .iter()
            .any(
                |gate| gate.remediation_plan_id == release_remediation_plan.id
                    && gate.gate_kind == "cluster_mutation"
            ));
        assert!(flow_before_revision
            .audit_events
            .iter()
            .any(|event| event.kind == "remediation_plan.created"
                && event.resource_id == release_remediation_plan.id));
        assert_eq!(revised.change_set.status, "draft");
        assert_eq!(revised.change_set.revision, 2);
        assert!(revised.material_hash_changed);
        assert_ne!(revised.change_set.material_hash, original_hash);
        assert_eq!(
            revised
                .invalidated_pipeline_intent
                .as_ref()
                .map(|intent| intent.status.as_str()),
            Some("stale")
        );
        assert_eq!(
            revised
                .invalidated_deployment_intent
                .as_ref()
                .map(|intent| intent.status.as_str()),
            Some("stale")
        );
        assert_eq!(
            revised
                .invalidated_release
                .as_ref()
                .map(|release| release.status.as_str()),
            Some("stale")
        );
        assert_eq!(
            revised
                .invalidated_registry_evidence
                .as_ref()
                .map(|evidence| evidence.status.as_str()),
            Some("stale")
        );
        assert_eq!(staled_grant.status, "stale");
        assert_eq!(staled_grant.revoked_by.as_deref(), Some("lucas"));
        assert_eq!(
            staled_grant.revoke_reason.as_deref(),
            Some("source change payload changed")
        );
        assert!(
            future_run.execution_target_json["policy"]["permission_grants"]
                .as_array()
                .is_none_or(Vec::is_empty)
        );
        assert!(!blocked_after_revision.ready);
        assert!(blocked_after_revision
            .blockers
            .iter()
            .any(|finding| finding.code == "change_set_not_approved"));
        assert!(blocked_after_revision
            .blockers
            .iter()
            .any(|finding| finding.code == "missing_active_trusted_envelope"));
        assert!(blocked_after_revision
            .warnings
            .iter()
            .any(|finding| finding.code == "stale_trusted_envelope"));
        assert!(blocked_after_revision
            .warnings
            .iter()
            .any(|finding| finding.code == "stale_pipeline_intent"));
        assert!(!reproposed_pipeline_intent.created);
        assert_eq!(
            reproposed_pipeline_intent.pipeline_intent.id,
            proposed_pipeline_intent.pipeline_intent.id
        );
        assert_eq!(
            reproposed_pipeline_intent.pipeline_intent.status,
            "proposed"
        );
        assert_eq!(
            reproposed_pipeline_intent.pipeline_intent.intent_json["source"]["material_hash"],
            serde_json::json!(revised.change_set.material_hash)
        );
        assert!(waiting_on_reproposed_pipeline_intent
            .warnings
            .iter()
            .any(|finding| finding.code == "pipeline_intent_not_approved"));
        assert_eq!(
            approved_reproposed_pipeline_intent.pipeline_intent.status,
            "approved"
        );
        assert!(waiting_on_reproposed_deployment_intent
            .warnings
            .iter()
            .any(|finding| finding.code == "stale_deployment_intent"));
        assert!(!reproposed_deployment_intent.created);
        assert_eq!(
            reproposed_deployment_intent.deployment_intent.id,
            proposed_deployment_intent.deployment_intent.id
        );
        assert_eq!(
            reproposed_deployment_intent.deployment_intent.status,
            "proposed"
        );
        assert!(waiting_on_reproposed_deployment_approval
            .warnings
            .iter()
            .any(|finding| finding.code == "deployment_intent_not_approved"));
        assert_eq!(
            approved_reproposed_deployment_intent
                .deployment_intent
                .status,
            "approved"
        );
        assert!(waiting_on_reproposed_release
            .warnings
            .iter()
            .any(|finding| finding.code == "stale_release"));
        assert!(!reproposed_release.created);
        assert_eq!(reproposed_release.release.id, proposed_release.release.id);
        assert_eq!(reproposed_release.release.status, "proposed");
        assert_eq!(
            reproposed_release.release.version.as_deref(),
            Some("v0.1.1-smoke")
        );
        assert!(waiting_on_reproposed_release_approval
            .warnings
            .iter()
            .any(|finding| finding.code == "release_not_approved"));
        assert_eq!(approved_reproposed_release.release.status, "approved");
        assert!(waiting_on_reproposed_registry_evidence
            .warnings
            .iter()
            .any(|finding| finding.code == "stale_registry_evidence"));
        assert!(!reproposed_registry_evidence.created);
        assert_eq!(
            reproposed_registry_evidence.registry_evidence.id,
            proposed_registry_evidence.registry_evidence.id
        );
        assert_eq!(
            reproposed_registry_evidence
                .registry_evidence
                .image_digest
                .as_deref(),
            Some("sha256:feedface")
        );
        assert_eq!(
            verified_reproposed_registry_evidence
                .registry_evidence
                .status,
            "verified"
        );
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
        assert!(grant_audit_events
            .events
            .iter()
            .any(|event| event.kind == "permission_grant.stale"));
        assert!(pipeline_intent_audit_events
            .events
            .iter()
            .any(|event| event.kind == "pipeline_intent.proposed"));
        assert!(pipeline_intent_audit_events
            .events
            .iter()
            .any(|event| event.kind == "pipeline_intent.approved"));
        assert!(pipeline_intent_audit_events.events.iter().any(|event| {
            event.kind == "pipeline_intent.evidence_attached"
                && event.payload["extra"]["observation_id"] == "obs_pipeline_intent_evidence"
                && event.payload["extra"]["evidence_status"] == "satisfied"
        }));
        assert!(pipeline_intent_audit_events
            .events
            .iter()
            .any(|event| event.kind == "pipeline_intent.stale"));
        assert!(pipeline_intent_audit_events
            .events
            .iter()
            .any(|event| event.kind == "pipeline_intent.reproposed"));
        assert!(deployment_intent_audit_events
            .events
            .iter()
            .any(|event| event.kind == "deployment_intent.proposed"));
        assert!(deployment_intent_audit_events
            .events
            .iter()
            .any(|event| event.kind == "deployment_intent.approved"));
        assert!(deployment_intent_audit_events
            .events
            .iter()
            .any(|event| event.kind == "deployment_intent.stale"));
        assert!(deployment_intent_audit_events
            .events
            .iter()
            .any(|event| event.kind == "deployment_intent.reproposed"));
        assert!(release_audit_events
            .events
            .iter()
            .any(|event| event.kind == "release.proposed"));
        assert!(release_audit_events
            .events
            .iter()
            .any(|event| event.kind == "release.approved"));
        assert!(release_audit_events
            .events
            .iter()
            .any(|event| event.kind == "release.evidence_attached"));
        assert!(release_audit_events
            .events
            .iter()
            .any(|event| event.kind == "release.stale"));
        assert!(release_audit_events
            .events
            .iter()
            .any(|event| event.kind == "release.reproposed"));
        assert!(registry_evidence_audit_events
            .events
            .iter()
            .any(|event| event.kind == "registry_evidence.proposed"));
        assert!(registry_evidence_audit_events
            .events
            .iter()
            .any(|event| event.kind == "registry_evidence.verified"));
        assert!(registry_evidence_audit_events
            .events
            .iter()
            .any(|event| event.kind == "registry_evidence.stale"));
        assert!(registry_evidence_audit_events
            .events
            .iter()
            .any(|event| event.kind == "registry_evidence.reproposed"));
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
                ..Default::default()
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

    #[test]
    fn builds_a_constrained_tekton_pipeline_run_manifest() {
        let intent_json = json!({
            "execution": {
                "enabled": true,
                "namespace": "tekton-pipelines",
                "pipeline_ref": "clone-build-push",
                "params": { "repo-url": "https://example.test/team/app.git" },
                "workspaces": [{
                    "name": "shared-data",
                    "volume_claim_template": { "storage": "1Gi" }
                }]
            }
        });
        let execution = tekton_execution_spec(&intent_json).unwrap();
        let intent = StoredPipelineIntent {
            id: "pint_123".to_string(),
            change_set_id: "cset_456".to_string(),
            work_plan_id: "wplan_789".to_string(),
            remediation_plan_id: "rplan_1".to_string(),
            incident_id: "inc_1".to_string(),
            session_id: SessionId::new("ses_1"),
            run_id: None,
            status: "approved".to_string(),
            title: "build".to_string(),
            summary: "build".to_string(),
            risk_level: "high".to_string(),
            intent_kind: "tekton_build_test_package".to_string(),
            resource_namespace: None,
            resource_kind: None,
            resource_name: None,
            intent_json,
            created_at: "1".to_string(),
            updated_at: None,
            status_changed_at: None,
            status_changed_by: None,
            status_reason: None,
        };
        let manifest = build_pipeline_run_manifest(&intent, &execution).unwrap();

        assert_eq!(manifest["apiVersion"], "tekton.dev/v1");
        assert_eq!(manifest["kind"], "PipelineRun");
        assert_eq!(manifest["metadata"]["namespace"], "tekton-pipelines");
        assert_eq!(manifest["spec"]["pipelineRef"]["name"], "clone-build-push");
        assert_eq!(
            manifest["spec"]["workspaces"][0]["volumeClaimTemplate"]["spec"]["accessModes"][0],
            "ReadWriteOnce"
        );
        assert!(manifest
            .pointer("/spec/taskRunTemplate/serviceAccountName")
            .is_none());
    }

    #[test]
    fn pipeline_contract_rejects_unknown_or_wrongly_shaped_inputs() {
        let execution = tekton_execution_spec(&json!({
            "execution": {
                "enabled": true,
                "namespace": "tekton-pipelines",
                "pipeline_ref": "clone-build-push",
                "params": { "branches": "main", "unknown": "value" },
                "workspaces": []
            }
        }))
        .unwrap();
        let contract = StoredPipelineContract {
            id: "pcontract_1".to_string(),
            status: "active".to_string(),
            namespace: "tekton-pipelines".to_string(),
            pipeline_ref: "clone-build-push".to_string(),
            version: "v1".to_string(),
            contract_json: json!({
                "params": [{ "name": "branches", "type": "array", "required": true }],
                "workspaces": []
            }),
            created_at: "1".to_string(),
            updated_at: "1".to_string(),
            status_changed_at: "1".to_string(),
            status_changed_by: None,
            status_reason: None,
        };

        let error = execution_matches_pipeline_contract(&execution, &contract).unwrap_err();
        assert!(error.message.contains("branches"));
    }

    #[test]
    fn execution_outcome_keeps_dispatch_identity_for_reconciliation() {
        let mut intent = json!({
            "execution_state": {
                "execution_id": "exec_1",
                "executor_job_name": "pharness-tekton-exec-1",
                "permission_grant_id": "pgrant_1",
                "state": "dispatched"
            }
        });

        merge_pipeline_execution_state(
            &mut intent,
            json!({
                "execution_id": "exec_1",
                "state": "pipeline_run_created",
                "pipeline_run_namespace": "tekton-pipelines",
                "pipeline_run_name": "build-1",
                "error": null
            }),
        );

        assert_eq!(
            intent.pointer("/execution_state/executor_job_name"),
            Some(&json!("pharness-tekton-exec-1"))
        );
        assert_eq!(
            intent.pointer("/execution_state/permission_grant_id"),
            Some(&json!("pgrant_1"))
        );
        assert_eq!(
            intent.pointer("/execution_state/state"),
            Some(&json!("pipeline_run_created"))
        );
    }

    #[tokio::test]
    async fn terminal_execution_evidence_is_compact_and_idempotent() {
        let state = test_state().await;
        let session_id = SessionId::new("ses_execution_evidence");
        state
            .store
            .create_session(CreateSession {
                id: session_id.clone(),
                title: "execution evidence".to_string(),
                cwd: ".".to_string(),
            })
            .await
            .unwrap();
        let intent = StoredPipelineIntent {
            id: "pint_execution_evidence".to_string(),
            change_set_id: "cset_execution_evidence".to_string(),
            work_plan_id: "wplan_execution_evidence".to_string(),
            remediation_plan_id: "rplan_execution_evidence".to_string(),
            incident_id: "inc_execution_evidence".to_string(),
            session_id,
            run_id: None,
            status: "executing".to_string(),
            title: "execution evidence".to_string(),
            summary: "execution evidence".to_string(),
            risk_level: "high".to_string(),
            intent_kind: "tekton_build_test_package".to_string(),
            resource_namespace: None,
            resource_kind: None,
            resource_name: None,
            intent_json: json!({}),
            created_at: "1".to_string(),
            updated_at: None,
            status_changed_at: None,
            status_changed_by: None,
            status_reason: None,
        };
        let outcome = PipelineIntentExecutionOutcomeRequest {
            execution_id: "pexec_execution_evidence".to_string(),
            status: "completed".to_string(),
            pipeline_run_namespace: Some("tekton-pipelines".to_string()),
            pipeline_run_name: Some("pharness-smoke".to_string()),
            error: None,
        };

        let first = persist_pipeline_execution_evidence(
            &state.store,
            &intent,
            &outcome,
            "pipeline_run_succeeded",
        )
        .await
        .unwrap();
        let second = persist_pipeline_execution_evidence(
            &state.store,
            &intent,
            &outcome,
            "pipeline_run_succeeded",
        )
        .await
        .unwrap();

        assert_eq!(first, second);
        assert_eq!(first["status"], "succeeded");
        assert_eq!(first["pipeline_run"]["namespace"], "tekton-pipelines");
        let artifact_id = first["artifact_id"].as_str().unwrap();
        let observation_id = first["observation_id"].as_str().unwrap();
        assert_eq!(
            state
                .store
                .get_artifact(artifact_id)
                .await
                .unwrap()
                .unwrap()
                .kind,
            "tekton_pipeline_run_execution"
        );
        assert_eq!(
            state
                .store
                .get_observation(observation_id)
                .await
                .unwrap()
                .unwrap()
                .kind,
            "pipeline_run_execution"
        );
    }

    #[test]
    fn deployment_approval_requires_matching_satisfied_pipeline_evidence() {
        let mut intent = StoredPipelineIntent {
            id: "pint_deployment_evidence".to_string(),
            change_set_id: "cset_deployment_evidence".to_string(),
            work_plan_id: "wplan_deployment_evidence".to_string(),
            remediation_plan_id: "rplan_deployment_evidence".to_string(),
            incident_id: "inc_deployment_evidence".to_string(),
            session_id: SessionId::new("ses_deployment_evidence"),
            run_id: None,
            status: "approved".to_string(),
            title: "deployment evidence".to_string(),
            summary: "deployment evidence".to_string(),
            risk_level: "high".to_string(),
            intent_kind: "tekton_build_test_package".to_string(),
            resource_namespace: None,
            resource_kind: None,
            resource_name: None,
            intent_json: json!({
                "execution_evidence": {
                    "status": "succeeded",
                    "pipeline_run": { "namespace": "tekton-pipelines", "name": "build-1" }
                }
            }),
            created_at: "1".to_string(),
            updated_at: None,
            status_changed_at: None,
            status_changed_by: None,
            status_reason: None,
        };

        assert!(ensure_pipeline_evidence_ready_for_deployment(&intent).is_err());
        intent.intent_json["evidence"] = json!({
            "status": "satisfied",
            "resource": { "namespace": "tekton-pipelines", "name": "other-run" }
        });
        assert!(ensure_pipeline_evidence_ready_for_deployment(&intent).is_err());
        intent.intent_json["evidence"]["resource"]["name"] = json!("build-1");
        assert!(ensure_pipeline_evidence_ready_for_deployment(&intent).is_ok());
    }
}
