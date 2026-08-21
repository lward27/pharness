use crate::dispatch::{
    ArgoSyncExecutionRequest, GitDeliveryExecutionRequest, GitDeliveryObservationRequest,
    GitOpsDeliveryExecutionRequest, GitOpsDeliveryObservationRequest,
    GitOpsRevisionResolutionRequest, RunDispatcher, TektonExecutionRequest,
};
use crate::dto::{
    AdvanceWorkItemRequest, AdvanceWorkItemResponse, ApprovalDecision, ApprovalGateResponse,
    ApprovalGateSummaryResponse, ApprovalGatesResponse, ApprovalResponse, ApprovalSummaryResponse,
    ApprovalsResponse, ApproveBudgetExtensionRequest, ArgoSyncContextResponse,
    ArgoSyncControlResponse, ArgoSyncOutcomeRequest, ArtifactResponse, ArtifactsResponse,
    AttachDeploymentIntentEvidenceRequest, AttachDeploymentIntentEvidenceResponse,
    AttachPipelineIntentEvidenceRequest, AttachPipelineIntentEvidenceResponse,
    AttachReleaseEvidenceRequest, AttachReleaseEvidenceResponse, AuditEventsResponse,
    BatchDecideApprovalGatesRequest, BatchDecideApprovalGatesResponse, BudgetExtensionResponse,
    CaptureWorkItemChangeSetRequest, ChangeSetResponse, ChangeSetsResponse,
    ControllerWaitTickResult, ControllerWaitsResponse, CreateChangeSetRequest,
    CreateChangeSetResponse, CreateDeploymentContractRequest,
    CreateDeploymentIntentFromPipelineIntentRequest, CreateDeploymentIntentResponse,
    CreateDeploymentIntentTrustedEnvelopeRequest, CreateGitDeliveryAuthorizationRequest,
    CreateGitOpsChangeSetRequest, CreateGitOpsChangeSetResponse,
    CreateGitOpsDeliveryAuthorizationRequest, CreateGitOpsUpdatePlanRequest, CreateIncidentRequest,
    CreateObservationRequest, CreatePermissionGrantRequest, CreatePipelineContractRequest,
    CreatePipelineIntentFromChangeSetRequest, CreatePipelineIntentResponse,
    CreatePipelineIntentTrustedEnvelopeRequest, CreateRegistryEvidenceFromInspectionRequest,
    CreateRegistryEvidenceFromInspectionResponse, CreateRegistryEvidenceFromReleaseRequest,
    CreateRegistryEvidenceResponse, CreateReleaseFromDeploymentIntentRequest,
    CreateReleaseResponse, CreateRemediationPlanRequest, CreateRunRequest,
    CreateTrustedEnvelopeRequest, CreateWorkItemPipelineIntentRequest, CreateWorkItemRequest,
    CreateWorkPlanFromRemediationPlanRequest, CreateWorkPlanResponse, DecideApprovalGateRequest,
    DecideApprovalGateResponse, DecideApprovalRequest, DecideApprovalResponse,
    DeliverySegmentResourceResponse, DeliverySegmentResponse, DeploymentContractResponse,
    DeploymentContractsResponse, DeploymentIntentDeliveryFlowResponse,
    DeploymentIntentPreflightRequest, DeploymentIntentPreflightResponse, DeploymentIntentResponse,
    DeploymentIntentsResponse, EnvironmentPreparationResponse, EventsResponse,
    ExecuteCapabilityRequest, ExecuteCapabilityResponse, ExecuteDeploymentIntentRequest,
    ExecuteDeploymentIntentResponse, ExecuteGitDeliveryRequest, ExecuteGitDeliveryResponse,
    ExecuteGitOpsDeliveryRequest, ExecuteGitOpsDeliveryResponse, ExecutePipelineIntentRequest,
    ExecutePipelineIntentResponse, ExecuteWorkItemActionRequest, ExecuteWorkItemRequest,
    ExecuteWorkItemResponse, FileChangeResponse, GitDeliveryAuthorizationResponse,
    GitDeliveryContextResponse, GitDeliveryFlowResponse, GitDeliveryObservationContextResponse,
    GitDeliveryObservationOutcomeRequest, GitDeliveryOutcomeRequest, GitDeliveryPlanResponse,
    GitDeliveryPreflightRequest, GitDeliveryPreflightResponse, GitOpsBaseRevisionContextResponse,
    GitOpsBaseRevisionOutcomeRequest, GitOpsChangeSetResponse, GitOpsChangeSetsResponse,
    GitOpsDeliveryAuthorizationResponse, GitOpsDeliveryContextResponse, GitOpsDeliveryFlowResponse,
    GitOpsDeliveryObservationContextResponse, GitOpsDeliveryObservationOutcomeRequest,
    GitOpsDeliveryOutcomeRequest, GitOpsDeliveryPlanResponse, GitOpsDeliveryPreflightRequest,
    GitOpsDeliveryPreflightResponse, GitOpsUpdatePlanResponse, IncidentResponse, IncidentsResponse,
    ObservationResponse, ObservationsResponse, ObserveGitDeliveryRequest,
    ObserveGitDeliveryResponse, ObserveGitOpsDeliveryRequest, ObserveGitOpsDeliveryResponse,
    OperatorResourceGroupMemberResponse, OperatorResourceGroupResponse, PermissionGrantResponse,
    PermissionGrantsResponse, PipelineContractResponse, PipelineContractsResponse,
    PipelineIntentExecutionOutcomeRequest, PipelineIntentExecutionPreflightResponse,
    PipelineIntentResponse, PipelineIntentsResponse, PrepareGitDeliveryRequest,
    PrepareGitOpsDeliveryRequest, ReconcileAuthorizationCheckResponse, ReconcileBlockerResponse,
    ReconcileDueControllerWaitsRequest, ReconcileDueControllerWaitsResponse,
    ReconcileWorkItemRequest, ReconcileWorkItemResponse, RegistryEvidenceListResponse,
    RegistryEvidenceResponse, ReleaseResponse, ReleasesResponse, RemediationPlanResponse,
    RemediationPlansResponse, ReplacePipelineContractRequest, ReplacePipelineContractResponse,
    ReplanWorkItemRequest, ReplanWorkItemResponse, ResolveGitOpsBaseRevisionRequest,
    ResolveGitOpsBaseRevisionResponse, ReviewApprovalRequest, ReviseChangeSetRequest,
    ReviseChangeSetResponse, ReviseWorkPlanRequest, ReviseWorkPlanResponse,
    RevokePermissionGrantRequest, RunDiffResponse, RunOperatorSummaryResponse, RunResponse,
    RunSummaryResponse, RunsResponse, ScopeOptionsResponse, SdlcFlowResponse, SdlcReadinessFinding,
    SdlcReadinessGateSummary, SdlcReadinessGrantSummary, SdlcReadinessResponse,
    TransitionChangeSetRequest, TransitionChangeSetResponse, TransitionDeploymentContractRequest,
    TransitionDeploymentIntentRequest, TransitionDeploymentIntentResponse,
    TransitionGitOpsChangeSetRequest, TransitionGitOpsChangeSetResponse,
    TransitionPipelineContractRequest, TransitionPipelineIntentRequest,
    TransitionPipelineIntentResponse, TransitionRegistryEvidenceRequest,
    TransitionRegistryEvidenceResponse, TransitionReleaseRequest, TransitionReleaseResponse,
    TransitionRemediationPlanRequest, TransitionRemediationPlanResponse, TransitionWorkItemRequest,
    TransitionWorkPlanRequest, TransitionWorkPlanResponse, TriageItemResponse, TriageResponse,
    TriageSummaryResponse, TrustedEnvelopeResponse, VerifyReleaseRequest, VerifyReleaseResponse,
    WorkItemActionResponse, WorkItemFlowResponse, WorkItemOperatorStateResponse,
    WorkItemPipelineContextResponse, WorkItemPreflightResponse, WorkItemResponse,
    WorkItemsResponse, WorkPlanResponse, WorkPlansResponse, WorkspaceResponse, WorkspacesResponse,
};
use crate::worker::{attempt_spec_for_run, finish_run_from_attempt, ingest_agent_event};
use crate::workspace::WorkspaceProvisioner;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use futures::stream::{self, Stream};
use pharness_core::{
    ActionId, AgentAction, AgentEvent, CapabilityKind, EventId, EventKind, PermissionGrant,
    PermissionGrantPolicy, PermissionGrantScope, PolicyDecision, PolicyMode, ProjectContract,
    ReadOnlyClusterTools, RiskLevel, RunBudget, RunBudgetConsumption, RunId, RunScope,
    SafetyPolicy, SessionId, ToolExecutor, ToolResult,
};
use pharness_runhost::{AttemptOutcome, WorkspaceSourceSpec};
use pharness_store::{
    ApprovalGateListFilter, ApprovalGateSummaryFilter, ApprovalListFilter, ApprovalSummaryFilter,
    AuditEventListFilter, ChangeSetListFilter, ControllerWaitListFilter,
    DeploymentContractListFilter, DeploymentIntentListFilter, GitOpsChangeSetListFilter,
    IncidentListFilter, ObservationListFilter, PipelineContractListFilter,
    PipelineIntentListFilter, RegistryEvidenceListFilter, ReleaseListFilter,
    RemediationPlanListFilter, RunListFilter, RunSummaryFilter, StoredApprovalGate, StoredArtifact,
    StoredAuditEvent, StoredChangeSet, StoredControllerWait, StoredDeploymentContract,
    StoredDeploymentIntent, StoredGitOpsChangeSet, StoredIncident, StoredObservation,
    StoredPermissionGrant, StoredPipelineContract, StoredPipelineIntent, StoredRegistryEvidence,
    StoredRelease, StoredRemediationPlan, StoredWorkItem, StoredWorkPlan, UpdateChangeSetRevision,
    UpdateDeploymentIntentDraft, UpdatePipelineIntentDraft, UpdatePipelineIntentExecution,
    UpdateRegistryEvidenceDraft, UpdateReleaseDraft, UpdateReleaseEvidence, UpdateWorkPlanRevision,
    WorkItemListFilter, WorkPlanListFilter, WorkspaceListFilter,
};
use pharness_store::{
    CreateApprovalGate, CreateArtifact, CreateAuditEvent, CreateChangeSet, CreateControllerWait,
    CreateDeploymentContract, CreateDeploymentIntent, CreateEnvironmentPreparation,
    CreateGitOpsChangeSet, CreateIncident, CreateObservation, CreatePermissionGrant,
    CreatePipelineContract, CreatePipelineIntent, CreateRegistryEvidence, CreateRelease,
    CreateRemediationPlan, CreateRun, CreateSession, CreateWorkItem, CreateWorkPlan,
    CreateWorkspace, ReplacePipelineContract, SqliteStore, StoreError,
    UpdateDeploymentIntentEvidence, UpdateEnvironmentPreparation, UpdatePipelineIntentEvidence,
    UpdateWorkspaceExecution,
};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::path::Path as FsPath;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::timeout;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

mod approvals;
mod auth;
mod deployment;
mod environment;
mod evidence;
mod gitops;
mod internal;
mod operator;
mod pipeline;
mod releases;
mod runs;
mod source;
mod system;
mod work_items;

use approvals::{
    active_permission_grants, append_approval_gate_audit_event,
    append_permission_grant_audit_event, approval_gate_lifecycle_readiness,
    approval_gate_lifecycle_stage, create_permission_grant_record, decide_approval_gate,
    grant_is_unexpired,
};
use auth::{require_operator_token, require_worker_token, OperatorIdentity};
use operator::{
    all_approval_gates_for_operator_groups, all_approvals_for_operator_groups,
    all_work_plans_for_operator_groups, group_operator_records, operator_resource_label,
};
use system::{
    capability_statuses, immutable_git_object_id, immutable_image_digest, protected_target_json,
    BuildMetadata, ProtectedTargetConfiguration, PROTECTED_ARGO_APPLICATION, PROTECTED_ENVIRONMENT,
    PROTECTED_GITOPS_REPO, PROTECTED_IMAGE_NAME, PROTECTED_KUSTOMIZATION_PATH, PROTECTED_NAMESPACE,
    PROTECTED_PIPELINE_NAMESPACE, PROTECTED_PIPELINE_REF, PROTECTED_ROLLBACK_OWNER,
    PROTECTED_SOURCE_REPO, PROTECTED_WORKLOAD_KIND, PROTECTED_WORKLOAD_NAME,
};
use work_items::{flow::*, preflight::*, reconcile::*, waits::*};

#[cfg(test)]
use work_items::actions::*;

#[cfg(test)]
use system::{
    capability_preflight_is_statically_unavailable, capability_verification_summary,
    config_effective, environment_profile_readiness_blocker, system_readiness,
};

#[cfg(test)]
use crate::dto::CapabilityStatusResponse;

#[cfg(test)]
use runs::*;

#[cfg(test)]
use evidence::*;

#[cfg(test)]
use approvals::{
    approval_gate_summary, approval_summary, batch_decide_approval_gates, create_permission_grant,
    deny_approval, get_approval, get_approval_gate, get_permission_grant, list_approval_gates,
    list_approvals, list_permission_grants, revoke_permission_grant, satisfy_approval_gate,
    validate_permission_grant_request, ApprovalGateSummaryQuery, ApprovalSummaryQuery,
    ListApprovalGatesQuery, ListApprovalsQuery, ListPermissionGrantsQuery,
};

const DEFAULT_DIRECT_CAPABILITY_TIMEOUT_MS: u64 = 60_000;
const MAX_DIRECT_CAPABILITY_TIMEOUT_MS: u64 = 300_000;
const DEFAULT_POLICY_SUBJECT: &str = "agent:local-worker";
const DEFAULT_TRUSTED_ENVELOPE_ENVIRONMENT: &str = "local";
const DEFAULT_GIT_WRITER_SUBJECT: &str = "agent:git-writer";
const DEFAULT_GITOPS_WRITER_SUBJECT: &str = "agent:gitops-writer";
const DEFAULT_ARGO_RUNNER_SUBJECT: &str = "agent:argo-runner";
const GIT_DELIVERY_ACTIONS: [&str; 4] = [
    "git_create_branch",
    "git_commit",
    "git_push",
    "github_create_pull_request",
];
const GITOPS_DELIVERY_ACTIONS: [&str; 4] = [
    "git_create_branch",
    "git_commit",
    "git_push",
    "github_create_pull_request",
];
const PIPELINE_DELIVERY_ACTIONS: [&str; 1] = ["tekton_create_pipeline_run"];
const ARGO_SYNC_ACTIONS: [&str; 1] = ["argocd_sync"];
const CLUSTER_DELIVERY_ACTIONS: [&str; 2] = ["tekton_create_pipeline_run", "argocd_sync"];
const PRODUCTION_DELIVERY_ACTIONS: [&str; 1] = ["production_action"];
const CONTROLLER_WAIT_INTERVAL_MS: u128 = 15_000;
const CONTROLLER_WAIT_MAX_CHECKS: u32 = 240;

#[derive(Clone)]
pub struct AppState {
    store: Arc<SqliteStore>,
    worker: RunDispatcher,
    cluster_tools: ReadOnlyClusterTools,
    policy: SafetyPolicy,
    worker_token: Option<String>,
    operator_tokens: Arc<Vec<(String, String)>>,
    workspace: WorkspaceProvisioner,
    build: BuildMetadata,
    protected_target: ProtectedTargetConfiguration,
    environment_profiles: Arc<Vec<pharness_core::EnvironmentProfile>>,
}

pub fn router(
    store: Arc<SqliteStore>,
    worker: RunDispatcher,
    cluster_tools: ReadOnlyClusterTools,
    policy: SafetyPolicy,
    worker_token: Option<String>,
    operator_tokens: Vec<(String, String)>,
    workspace: WorkspaceProvisioner,
) -> Router {
    let state = AppState {
        store,
        worker,
        cluster_tools,
        policy,
        worker_token,
        operator_tokens: Arc::new(operator_tokens),
        workspace,
        build: BuildMetadata::from_env(),
        protected_target: ProtectedTargetConfiguration::from_env(),
        environment_profiles: Arc::new(environment::load_environment_profiles()),
    };

    Router::new()
        .merge(runs::router())
        .merge(system::router())
        .merge(evidence::router())
        .merge(work_items::router())
        .merge(operator::router())
        .merge(source::router())
        .merge(gitops::router())
        .merge(pipeline::router())
        .merge(deployment::router())
        .merge(releases::router())
        .merge(approvals::router())
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_operator_token,
        ))
        .merge(internal::router(state.clone()))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
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

async fn transition_work_item(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(work_item_id): Path<String>,
    Json(request): Json<TransitionWorkItemRequest>,
) -> Result<Json<WorkItemResponse>, ApiError> {
    let current = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    let target = WorkItemStatus::parse(&request.target_status)?;
    WorkItemStatus::parse(&current.status)?.ensure_can_transition_to(target)?;
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    let work_item = state
        .store
        .update_work_item_status(
            &work_item_id,
            target.as_str(),
            actor.clone(),
            reason.clone(),
        )
        .await?;
    append_work_item_audit_event(
        &state.store,
        &work_item,
        &format!("work_item.{}", target.as_str()),
        actor,
        json!({
            "previous_status": current.status,
            "status": work_item.status,
            "reason": reason,
        }),
    )
    .await?;
    Ok(Json(work_item.into()))
}

async fn cancel_work_item(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(work_item_id): Path<String>,
    Json(request): Json<TransitionWorkItemRequest>,
) -> Result<Json<WorkItemResponse>, ApiError> {
    if request.target_status != "cancelled" {
        return Err(ApiError::bad_request(
            "work item cancel requires target_status cancelled",
        ));
    }
    transition_work_item(State(state), identity, Path(work_item_id), Json(request)).await
}

async fn create_work_plan_from_work_item(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(work_item_id): Path<String>,
) -> Result<Json<CreateWorkPlanResponse>, ApiError> {
    if let Some(existing) = state
        .store
        .get_work_plan_by_work_item(&work_item_id)
        .await?
    {
        return Ok(Json(CreateWorkPlanResponse {
            work_plan: existing.into(),
            created: false,
        }));
    }
    let work_item = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    if work_item.status != "planning" {
        return Err(ApiError::conflict(
            "a WorkItem must be planning before it can create a WorkPlan",
        ));
    }
    let actor = identity.map(|Extension(OperatorIdentity(name))| name);
    let session_id = SessionId::new(format!("ses_work_item_{}", unique_suffix()));
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: format!("WorkItem: {}", work_item.title),
            cwd: format!("work-item/{}", work_item.id),
        })
        .await?;
    let work_plan = state
        .store
        .create_work_plan(work_plan_from_work_item(
            &work_item,
            session_id,
            format!("wplan_{}", unique_suffix()),
        ))
        .await?;
    for gate in approval_gates_from_work_item(&work_item, &work_plan) {
        let gate = state.store.create_approval_gate(gate).await?;
        append_approval_gate_audit_event(&state.store, &gate, "approval_gate.created", "created")
            .await?;
    }
    let workspace = state
        .store
        .create_workspace(CreateWorkspace {
            id: format!("ws_{}", unique_suffix()),
            work_item_id: work_item.id.clone(),
            run_id: None,
            status: "declared".to_string(),
            source_repo: work_item.source_repo.clone(),
            source_ref: work_item.source_ref.clone(),
            resolved_commit: None,
            branch: None,
            retention_status: "ephemeral".to_string(),
            actor: actor.clone(),
            reason: Some("WorkItem planning declared an isolated workspace".to_string()),
        })
        .await?;
    append_work_item_audit_event(
        &state.store,
        &work_item,
        "work_item.work_plan_created",
        actor.clone(),
        json!({ "work_plan_id": work_plan.id, "workspace_id": workspace.id }),
    )
    .await?;
    append_workspace_audit_event(&state.store, &workspace, "workspace.declared", actor).await?;

    let work_item = state
        .store
        .update_work_item_status(
            &work_item.id,
            "awaiting_approval",
            None,
            Some("WorkPlan and workspace are ready for review".to_string()),
        )
        .await?;
    append_work_item_audit_event(
        &state.store,
        &work_item,
        "work_item.awaiting_approval",
        None,
        json!({ "work_plan_id": work_plan.id, "workspace_id": workspace.id }),
    )
    .await?;

    Ok(Json(CreateWorkPlanResponse {
        work_plan: work_plan.into(),
        created: true,
    }))
}

#[derive(Debug, serde::Deserialize)]
struct RollbackIntentRequest {
    actor: Option<String>,
    reason: String,
    #[serde(default)]
    expires_at: Option<String>,
}

fn required_baseline_capability_result(
    response: ExecuteCapabilityResponse,
    resource: &str,
) -> Result<ToolResult, ApiError> {
    response.result.ok_or_else(|| {
        let detail = response
            .error
            .as_deref()
            .filter(|error| !error.trim().is_empty())
            .unwrap_or(response.status.as_str());
        ApiError::conflict(format!(
            "production baseline {resource} observation failed: {detail}"
        ))
    })
}

async fn prepare_work_item_rollback_intent(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(work_item_id): Path<String>,
    Json(request): Json<RollbackIntentRequest>,
) -> Result<Json<Value>, ApiError> {
    let reason = required_text(request.reason.clone(), "reason")?;
    let item = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    if !stored_work_item_matches_protected_target(&item) {
        return Err(ApiError::conflict(
            "RollbackIntent is limited to the exact protected production target",
        ));
    }
    if let Some(existing) = latest_rollback_intent(&state, &item, None).await? {
        return Ok(Json(existing));
    }
    let deployment = execute_direct_capability(
        &state,
        AgentAction::KubernetesGet {
            id: ActionId::new(format!("act_{}_baseline_deployment", unique_suffix())),
            reason: reason.clone(),
            resource: "deployments".to_string(),
            namespace: Some(PROTECTED_NAMESPACE.to_string()),
            name: Some(PROTECTED_WORKLOAD_NAME.to_string()),
            all_namespaces: false,
            label_selector: None,
        },
        None,
    )
    .await?;
    let argo = execute_direct_capability(
        &state,
        AgentAction::ArgoGetApp {
            id: ActionId::new(format!("act_{}_baseline_argo", unique_suffix())),
            reason: reason.clone(),
            app: PROTECTED_ARGO_APPLICATION.to_string(),
        },
        None,
    )
    .await?;
    let pods = execute_direct_capability(
        &state,
        AgentAction::KubernetesGet {
            id: ActionId::new(format!("act_{}_baseline_pods", unique_suffix())),
            reason: reason.clone(),
            resource: "pods".to_string(),
            namespace: Some(PROTECTED_NAMESPACE.to_string()),
            name: None,
            all_namespaces: false,
            label_selector: Some("app=yfinance-wrapper".to_string()),
        },
        None,
    )
    .await?;
    let deployment_result = required_baseline_capability_result(deployment, "Deployment")?;
    let argo_result = required_baseline_capability_result(argo, "Argo Application")?;
    let pods_result = required_baseline_capability_result(pods, "Pod")?;
    let expected_image = item
        .gitops_image_name
        .as_deref()
        .ok_or_else(|| ApiError::conflict("protected target has no image name"))?;
    let images = deployment_result
        .content
        .pointer("/analysis/containers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let image_ref = images
        .iter()
        .filter_map(|container| container.get("image").and_then(Value::as_str))
        .find(|image| image.starts_with(&format!("{expected_image}@sha256:")))
        .ok_or_else(|| {
            ApiError::conflict(
                "running protected Deployment does not expose the expected immutable image",
            )
        })?
        .to_string();
    let image_digest = image_ref
        .split_once('@')
        .map(|(_, digest)| digest.to_string())
        .filter(|digest| immutable_image_digest(digest))
        .ok_or_else(|| ApiError::conflict("production baseline image digest is malformed"))?;
    let running_image_ids = pods_result
        .content
        .pointer("/output/items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|pod| {
            pod.pointer("/status/containers")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|container| container.get("imageID").and_then(Value::as_str))
        .collect::<Vec<_>>();
    if running_image_ids.is_empty()
        || !running_image_ids
            .iter()
            .all(|image_id| image_id.ends_with(&image_digest))
    {
        return Err(ApiError::conflict(
            "running Pod imageID does not exactly match the declared production baseline digest",
        ));
    }
    let ready = deployment_result
        .content
        .pointer("/analysis/status")
        .and_then(Value::as_str)
        == Some("healthy");
    if !ready {
        return Err(ApiError::conflict(
            "production baseline Deployment must be healthy before rollback preparation",
        ));
    }
    let argo_revision = argo_result
        .content
        .pointer("/analysis/revision")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("production baseline Argo revision is unavailable"))?;
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let rollback_intent_id = format!("rollback_{}", unique_suffix());
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&item.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent requires a WorkPlan"))?;
    let run_id = work_item_provenance_run_id(&state, &item)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent requires coding run provenance"))?;
    let run = state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
    let gate = state
        .store
        .create_approval_gate(CreateApprovalGate {
            id: format!("agate_{}_rollback", unique_suffix()),
            work_item_id: Some(item.id.clone()),
            remediation_plan_id: None,
            incident_id: None,
            session_id: run.session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "pending".to_string(),
            gate_kind: "production_rollback".to_string(),
            gate_order: 90,
            title: format!("Approve rollback for {}", item.title),
            summary: format!("Restore only {image_digest} through a manual GitOps pull request"),
            risk_level: "critical".to_string(),
            resource_namespace: item.target_namespace.clone(),
            resource_kind: item.workload_kind.clone(),
            resource_name: item.workload_name.clone(),
            gate_json: json!({
                "rollback_intent_id": rollback_intent_id,
                "work_plan_id": work_plan.id,
                "baseline_digest": image_digest,
                "argo_application": PROTECTED_ARGO_APPLICATION,
            }),
        })
        .await?;
    let content = json!({
        "rollback_intent_id": rollback_intent_id,
        "work_item_id": item.id,
        "status": "prepared",
        "baseline": {
            "image_ref": image_ref,
            "image_digest": image_digest,
            "deployment_ready": true,
            "running_image_ids_verified": true,
            "argo_revision": argo_revision,
            "gitops_revision": argo_revision,
        },
        "target": protected_target_json(),
        "rollback_owner": PROTECTED_ROLLBACK_OWNER,
        "approval_gate_id": gate.id,
        "manual_merge_required": true,
        "automatic_rollback": false,
        "reason": reason,
        "created_by": actor,
    });
    let artifact = append_rollback_intent_artifact(&state, &run, &content).await?;
    Ok(Json(rollback_intent_response(&artifact)))
}

async fn get_work_item_rollback_intent(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let item = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    Ok(Json(
        latest_rollback_intent(&state, &item, None)
            .await?
            .unwrap_or_else(|| json!({ "status": "unavailable", "work_item_id": item.id })),
    ))
}

async fn approve_rollback_intent(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(rollback_intent_id): Path<String>,
    Json(request): Json<RollbackIntentRequest>,
) -> Result<Json<Value>, ApiError> {
    let reason = required_text(request.reason.clone(), "reason")?;
    let (item, current, run) = rollback_intent_context(&state, &rollback_intent_id).await?;
    let current_status = current.pointer("/content/status").and_then(Value::as_str);
    if current_status == Some("ready_for_argo_sync") {
        return approve_rollback_argo_sync(
            RollbackArgoApprovalContext {
                state: &state,
                rollback_intent_id: &rollback_intent_id,
                item: &item,
                current: &current,
                run: &run,
            },
            identity,
            request,
            reason,
        )
        .await;
    }
    if current_status != Some("prepared") {
        return Err(ApiError::conflict(
            "RollbackIntent must be prepared for its writer or ready for its explicit Argo approval",
        ));
    }
    let expires_at = bounded_production_grant_expiry(&item, request.expires_at)?;
    let gate_id = current
        .pointer("/content/approval_gate_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("RollbackIntent approval gate is unavailable"))?;
    let gate = state
        .store
        .get_approval_gate(gate_id)
        .await?
        .filter(|gate| matches!(gate.status.as_str(), "pending" | "satisfied"))
        .ok_or_else(|| {
            ApiError::conflict("RollbackIntent approval gate is neither pending nor satisfied")
        })?;
    let baseline_digest_from_intent = current
        .pointer("/content/baseline/image_digest")
        .and_then(Value::as_str)
        .filter(|value| immutable_image_digest(value))
        .ok_or_else(|| ApiError::conflict("RollbackIntent baseline digest is invalid"))?;
    if gate.work_item_id.as_deref() != Some(item.id.as_str())
        || gate.gate_kind != "production_rollback"
        || gate
            .gate_json
            .get("rollback_intent_id")
            .and_then(Value::as_str)
            != Some(rollback_intent_id.as_str())
        || gate
            .gate_json
            .get("baseline_digest")
            .and_then(Value::as_str)
            != Some(baseline_digest_from_intent)
        || gate
            .gate_json
            .get("argo_application")
            .and_then(Value::as_str)
            != Some(PROTECTED_ARGO_APPLICATION)
    {
        return Err(ApiError::conflict(
            "RollbackIntent approval gate no longer matches its immutable target binding",
        ));
    }
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    if gate.status == "pending" {
        state
            .store
            .decide_approval_gate(gate_id, "satisfied", actor.clone(), Some(reason.clone()))
            .await?;
    }
    let content = current
        .get("content")
        .cloned()
        .ok_or_else(|| ApiError::internal("RollbackIntent content is unavailable"))?;
    let mut content = content
        .as_object()
        .cloned()
        .ok_or_else(|| ApiError::internal("RollbackIntent content is malformed"))?;
    let baseline_digest = content
        .get("baseline")
        .and_then(|value| value.get("image_digest"))
        .and_then(Value::as_str)
        .filter(|value| immutable_image_digest(value))
        .ok_or_else(|| ApiError::conflict("RollbackIntent baseline digest is invalid"))?
        .to_string();
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&item.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent WorkPlan is unavailable"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent ChangeSet is unavailable"))?;
    let pipeline_intent = state
        .store
        .get_pipeline_intent_by_change_set(&change_set.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent PipelineIntent is unavailable"))?;
    let source_merge_sha = pipeline_intent
        .intent_json
        .pointer("/source_provenance/merge_commit_sha")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .ok_or_else(|| ApiError::conflict("rollback source merge provenance is unavailable"))?
        .to_string();
    let rollback_base_commit =
        observed_gitops_merge_for_deployment(&state.store, &item, &pipeline_intent)
            .await?
            .and_then(|artifact| artifact.content_json)
            .and_then(|content| {
                content
                    .get("merge_commit_sha")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .filter(|value| is_git_sha(value))
            .ok_or_else(|| {
                ApiError::conflict(
                    "rollback writer authorization requires the observed deployment GitOps merge",
                )
            })?;
    let branch = format!(
        "pharness/rollback-{}",
        safe_id_fragment(&rollback_intent_id)
    );
    let permission_grant = create_permission_grant_record(
        &state.store,
        CreatePermissionGrantRequest {
            subject: DEFAULT_GITOPS_WRITER_SUBJECT.to_string(),
            created_by: actor.clone(),
            reason: format!("RollbackIntent {rollback_intent_id}: {reason}"),
            scope: json!({
                "environment": PROTECTED_ENVIRONMENT,
                "capability_kinds": ["git"],
                "actions": GITOPS_DELIVERY_ACTIONS,
                "max_risk": "critical",
                "repos": [PROTECTED_GITOPS_REPO],
                "branches": [branch],
                "work_item_ids": [item.id],
                "work_plan_ids": [work_plan.id],
                "change_set_ids": [change_set.id],
                "pipeline_intent_ids": [pipeline_intent.id],
                "pipeline_contract_ids": [item.pipeline_contract_id],
                "deployment_contract_ids": [item.deployment_contract_id],
                "source_merge_shas": [source_merge_sha],
                "gitops_merge_shas": [rollback_base_commit],
                "image_digests": [baseline_digest],
                "production_impacting": true,
            }),
            policy: json!({ "policy_mode": "supervised_autonomy" }),
            expires_at: expires_at.clone(),
        },
    )
    .await?;
    content.insert("status".to_string(), json!("approved"));
    content.insert("approved_by".to_string(), json!(actor));
    content.insert("approval_reason".to_string(), json!(reason));
    content.insert("authorization_expires_at".to_string(), json!(expires_at));
    content.insert(
        "permission_grant_id".to_string(),
        json!(permission_grant.id),
    );
    content.insert(
        "rollback_base_commit".to_string(),
        json!(rollback_base_commit),
    );
    let artifact = append_rollback_intent_artifact(&state, &run, &Value::Object(content)).await?;
    Ok(Json(rollback_intent_response(&artifact)))
}

struct RollbackArgoApprovalContext<'a> {
    state: &'a AppState,
    rollback_intent_id: &'a str,
    item: &'a StoredWorkItem,
    current: &'a Value,
    run: &'a pharness_store::StoredRun,
}

async fn approve_rollback_argo_sync(
    context: RollbackArgoApprovalContext<'_>,
    identity: Option<Extension<OperatorIdentity>>,
    request: RollbackIntentRequest,
    reason: String,
) -> Result<Json<Value>, ApiError> {
    let RollbackArgoApprovalContext {
        state,
        rollback_intent_id,
        item,
        current,
        run,
    } = context;
    if !stored_work_item_matches_protected_target(item) {
        return Err(ApiError::conflict(
            "rollback Argo approval is limited to the exact protected production target",
        ));
    }
    let expires_at = bounded_production_grant_expiry(item, request.expires_at)?;
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let content = current
        .get("content")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| ApiError::conflict("RollbackIntent content is unavailable"))?;
    let baseline_digest = content
        .get("baseline")
        .and_then(|value| value.get("image_digest"))
        .and_then(Value::as_str)
        .filter(|value| immutable_image_digest(value))
        .ok_or_else(|| ApiError::conflict("RollbackIntent baseline digest is invalid"))?
        .to_string();
    let gitops_merge_sha = content
        .get("gitops_merge_sha")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .ok_or_else(|| ApiError::conflict("rollback GitOps merge provenance is unavailable"))?
        .to_string();
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&item.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent WorkPlan is unavailable"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent ChangeSet is unavailable"))?;
    let pipeline_intent = state
        .store
        .get_pipeline_intent_by_change_set(&change_set.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent PipelineIntent is unavailable"))?;
    let source_merge_sha = pipeline_intent
        .intent_json
        .pointer("/source_provenance/merge_commit_sha")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .ok_or_else(|| {
            ApiError::conflict("rollback authorization requires source merge provenance")
        })?
        .to_string();

    let mut gate_ids = Vec::new();
    for (order, gate_kind) in [
        (91, "production_rollback_deployment"),
        (92, "cluster_mutation"),
        (93, "production_impact"),
    ] {
        let gate = state
            .store
            .create_approval_gate(CreateApprovalGate {
                id: format!("agate_{}_rollback_argo", unique_suffix()),
                work_item_id: Some(item.id.clone()),
                remediation_plan_id: None,
                incident_id: None,
                session_id: run.session_id.clone(),
                run_id: Some(run.id.clone()),
                status: "pending".to_string(),
                gate_kind: gate_kind.to_string(),
                gate_order: order,
                title: format!("Approve {gate_kind} for {}", item.title),
                summary: format!(
                    "Sync {PROTECTED_ARGO_APPLICATION} only to rollback merge {gitops_merge_sha} and restore {baseline_digest}"
                ),
                risk_level: "critical".to_string(),
                resource_namespace: Some(PROTECTED_NAMESPACE.to_string()),
                resource_kind: Some("Application".to_string()),
                resource_name: Some(PROTECTED_ARGO_APPLICATION.to_string()),
                gate_json: json!({
                    "rollback_intent_id": rollback_intent_id,
                    "work_plan_id": work_plan.id,
                    "gitops_merge_sha": gitops_merge_sha,
                    "baseline_digest": baseline_digest,
                    "argo_application": PROTECTED_ARGO_APPLICATION,
                    "scope": {
                        "work_item_id": item.id,
                        "work_plan_id": work_plan.id,
                        "environment": PROTECTED_ENVIRONMENT,
                        "production_impacting": true,
                        "source_repository": item.source_repo,
                        "source_ref": item.source_ref,
                        "gitops_repository": item.gitops_repo,
                        "gitops_ref": item.gitops_ref,
                        "target_namespace": PROTECTED_NAMESPACE,
                        "argo_application": PROTECTED_ARGO_APPLICATION,
                        "actions": ARGO_SYNC_ACTIONS,
                    },
                }),
            })
            .await?;
        state
            .store
            .decide_approval_gate(&gate.id, "satisfied", actor.clone(), Some(reason.clone()))
            .await?;
        gate_ids.push(gate.id);
    }
    let grant = create_permission_grant_record(
        &state.store,
        CreatePermissionGrantRequest {
            subject: DEFAULT_ARGO_RUNNER_SUBJECT.to_string(),
            created_by: actor.clone(),
            reason: format!("RollbackIntent {rollback_intent_id} Argo sync: {reason}"),
            scope: json!({
                "environment": PROTECTED_ENVIRONMENT,
                "capability_kinds": ["argo_sync"],
                "actions": ARGO_SYNC_ACTIONS,
                "max_risk": "critical",
                "namespaces": [PROTECTED_NAMESPACE],
                "work_item_ids": [item.id],
                "work_plan_ids": [work_plan.id],
                "change_set_ids": [change_set.id],
                "pipeline_intent_ids": [pipeline_intent.id],
                "deployment_intent_ids": [rollback_intent_id],
                "argo_applications": [PROTECTED_ARGO_APPLICATION],
                "pipeline_contract_ids": [item.pipeline_contract_id],
                "deployment_contract_ids": [item.deployment_contract_id],
                "source_merge_shas": [source_merge_sha],
                "gitops_merge_shas": [gitops_merge_sha],
                "image_digests": [baseline_digest],
                "production_impacting": true,
            }),
            policy: json!({ "policy_mode": "supervised_autonomy" }),
            expires_at: expires_at.clone(),
        },
    )
    .await?;
    let mut updated = content;
    updated.insert("status".to_string(), json!("argo_approved"));
    updated.insert("argo_approved_by".to_string(), json!(actor));
    updated.insert("argo_approval_reason".to_string(), json!(reason));
    updated.insert(
        "argo_authorization_expires_at".to_string(),
        json!(expires_at),
    );
    updated.insert("argo_permission_grant_id".to_string(), json!(grant.id));
    updated.insert("argo_approval_gate_ids".to_string(), json!(gate_ids));
    let artifact = append_rollback_intent_artifact(state, run, &Value::Object(updated)).await?;
    Ok(Json(rollback_intent_response(&artifact)))
}

async fn preflight_rollback_intent(
    State(state): State<AppState>,
    Path(rollback_intent_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (item, current, _run) = rollback_intent_context(&state, &rollback_intent_id).await?;
    let status = current.pointer("/content/status").and_then(Value::as_str);
    let argo_phase = status == Some("argo_approved");
    let expires_at = current
        .pointer(if argo_phase {
            "/content/argo_authorization_expires_at"
        } else {
            "/content/authorization_expires_at"
        })
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<u128>().ok());
    let authorization_fresh = expires_at.is_some_and(|value| value > current_millis());
    let permission_grant_id = current
        .pointer(if argo_phase {
            "/content/argo_permission_grant_id"
        } else {
            "/content/permission_grant_id"
        })
        .and_then(Value::as_str);
    let grant_fresh = match permission_grant_id {
        Some(grant_id) => state
            .store
            .get_permission_grant(grant_id)
            .await?
            .is_some_and(|grant| {
                grant.status == "active" && grant_is_unexpired(&grant, current_millis())
            }),
        None => false,
    };
    let exact_binding = if argo_phase {
        match current.get("content").and_then(Value::as_object) {
            Some(content) => {
                validate_rollback_argo_grant(&state, &item, &rollback_intent_id, content)
                    .await
                    .is_ok()
            }
            None => false,
        }
    } else {
        true
    };
    let ready = matches!(status, Some("approved" | "argo_approved"))
        && authorization_fresh
        && grant_fresh
        && exact_binding;
    Ok(Json(json!({
        "rollback_intent_id": rollback_intent_id,
        "status": if ready { if argo_phase { "ready_for_argo" } else { "ready_for_writer" } } else { "blocked" },
        "ready": ready,
        "authorization_fresh": authorization_fresh,
        "grant_fresh": grant_fresh,
        "writer_grant_fresh": !argo_phase && grant_fresh,
        "argo_grant_fresh": argo_phase && grant_fresh,
        "exact_binding": exact_binding,
        "manual_merge_required": true,
        "automatic_rollback": false,
        "content": current.get("content"),
    })))
}

async fn execute_rollback_intent(
    State(state): State<AppState>,
    Path(rollback_intent_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let Json(preflight) =
        preflight_rollback_intent(State(state.clone()), Path(rollback_intent_id.clone())).await?;
    if preflight.get("ready").and_then(Value::as_bool) != Some(true) {
        return Err(ApiError::conflict(
            "RollbackIntent is not ready for its next isolated executor",
        ));
    }
    if preflight.get("status").and_then(Value::as_str) == Some("ready_for_argo") {
        return dispatch_rollback_argo_sync(&state, &rollback_intent_id).await;
    }
    let (item, current, run) = rollback_intent_context(&state, &rollback_intent_id).await?;
    require_fresh_capability(&state, "gitops_writer").await?;
    let settings = state
        .worker
        .gitops_writer_settings()
        .ok_or_else(|| ApiError::conflict("GitOps rollback writer is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == PROTECTED_GITOPS_REPO)
    {
        return Err(ApiError::conflict(
            "protected GitOps repository is not allowlisted for the rollback writer",
        ));
    }
    let content = current
        .get("content")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("RollbackIntent content is unavailable"))?;
    let baseline_digest = content
        .get("baseline")
        .and_then(|value| value.get("image_digest"))
        .and_then(Value::as_str)
        .filter(|value| immutable_image_digest(value))
        .ok_or_else(|| ApiError::conflict("RollbackIntent baseline digest is invalid"))?;
    let base_commit = content
        .get("rollback_base_commit")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .ok_or_else(|| ApiError::conflict("RollbackIntent writer base revision is invalid"))?;
    let permission_grant_id = content
        .get("permission_grant_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("RollbackIntent has no short-lived writer grant"))?;
    let permission_grant = state
        .store
        .get_permission_grant(permission_grant_id)
        .await?
        .filter(|grant| grant.status == "active" && grant_is_unexpired(grant, current_millis()))
        .ok_or_else(|| ApiError::conflict("RollbackIntent writer grant is expired or revoked"))?;
    let grant_scope =
        serde_json::from_value::<PermissionGrantScope>(permission_grant.scope_json)
            .map_err(|_| ApiError::conflict("RollbackIntent writer grant scope is malformed"))?;
    let expected_branch = format!(
        "pharness/rollback-{}",
        safe_id_fragment(&rollback_intent_id)
    );
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&item.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent WorkPlan is unavailable"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent ChangeSet is unavailable"))?;
    let pipeline_intent = state
        .store
        .get_pipeline_intent_by_change_set(&change_set.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent PipelineIntent is unavailable"))?;
    let source_merge_sha = pipeline_intent
        .intent_json
        .pointer("/source_provenance/merge_commit_sha")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .ok_or_else(|| ApiError::conflict("rollback source merge provenance is unavailable"))?;
    if permission_grant.subject != DEFAULT_GITOPS_WRITER_SUBJECT
        || grant_scope.environment.as_deref() != Some(PROTECTED_ENVIRONMENT)
        || grant_scope.capability_kinds != vec![CapabilityKind::Git]
        || grant_scope.repos != vec![PROTECTED_GITOPS_REPO.to_string()]
        || grant_scope.branches != vec![expected_branch.clone()]
        || grant_scope.work_item_ids != vec![item.id.clone()]
        || grant_scope.work_plan_ids != vec![work_plan.id]
        || grant_scope.change_set_ids != vec![change_set.id]
        || grant_scope.pipeline_intent_ids != vec![pipeline_intent.id]
        || grant_scope.pipeline_contract_ids
            != vec![item.pipeline_contract_id.clone().unwrap_or_default()]
        || grant_scope.deployment_contract_ids
            != vec![item.deployment_contract_id.clone().unwrap_or_default()]
        || grant_scope.source_merge_shas != vec![source_merge_sha.to_string()]
        || grant_scope.gitops_merge_shas != vec![base_commit.to_string()]
        || grant_scope.image_digests != vec![baseline_digest.to_string()]
        || grant_scope.production_impacting != Some(true)
        || !GITOPS_DELIVERY_ACTIONS
            .iter()
            .all(|action| grant_scope.actions.iter().any(|allowed| allowed == action))
    {
        return Err(ApiError::conflict(
            "RollbackIntent writer grant no longer matches the exact WorkItem, contracts, digest, repository, and branch",
        ));
    }
    let artifacts = state.store.list_artifacts(&run.id).await?;
    if let Some(existing) = artifacts.iter().find(|artifact| {
        artifact.kind == "rollback_delivery_execution"
            && artifact.content_json.as_ref().is_some_and(|value| {
                value.get("rollback_intent_id").and_then(Value::as_str)
                    == Some(rollback_intent_id.as_str())
            })
    }) {
        let execution_id = existing
            .content_json
            .as_ref()
            .and_then(|value| value.get("execution_id"))
            .and_then(Value::as_str);
        let status = execution_id
            .and_then(|id| {
                artifacts.iter().find_map(|artifact| {
                    (artifact.kind == "rollback_delivery_result")
                        .then_some(artifact.content_json.as_ref())
                        .flatten()
                        .filter(|value| {
                            value.get("execution_id").and_then(Value::as_str) == Some(id)
                        })
                        .and_then(|value| value.get("status").and_then(Value::as_str))
                })
            })
            .unwrap_or("dispatched");
        return Ok(Json(json!({
            "rollback_intent_id": rollback_intent_id,
            "status": status,
            "execution": ArtifactResponse::from(existing.clone()),
            "created": false,
            "manual_merge_required": true,
            "automatic_rollback": false,
        })));
    }
    let execution_id = format!("rbexec_{}", unique_suffix());
    let context = json!({
        "execution_id": execution_id,
        "repository": PROTECTED_GITOPS_REPO,
        "base_ref": item.gitops_ref.as_deref().unwrap_or("main"),
        "base_commit": base_commit,
        "head_branch": expected_branch,
        "kustomization_path": PROTECTED_KUSTOMIZATION_PATH,
        "image_name": PROTECTED_IMAGE_NAME,
        "image_ref": format!("{PROTECTED_IMAGE_NAME}@{baseline_digest}"),
        "commit_subject": format!("rollback(yfinance-wrapper): restore {}", &baseline_digest[..19]),
        "commit_body": format!("Restore the captured known-good digest for RollbackIntent {rollback_intent_id}.\n\nManual merge and explicit Argo approval remain required."),
        "pull_request_title": format!("Rollback yfinance-wrapper to {}", &baseline_digest[..19]),
        "pull_request_body": format!("Restore only the captured known-good image digest.\n\nPHarness RollbackIntent: {rollback_intent_id}\nWorkItem: {}\nRollback owner: {PROTECTED_ROLLBACK_OWNER}", item.id),
        "github_api_url": settings.github_api_url,
        "author_name": settings.author_name,
        "author_email": settings.author_email,
    });
    let execution = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_rollback_delivery_execution", unique_suffix()),
            session_id: run.session_id.clone(),
            run_id: Some(run.id.clone()),
            kind: "rollback_delivery_execution".to_string(),
            label: format!("Rollback GitOps delivery for {rollback_intent_id}"),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "rollback_intent_id": rollback_intent_id,
                "execution_id": execution_id,
                "status": "dispatched",
                "context": context,
            })),
        })
        .await?;
    match state
        .worker
        .dispatch_gitops_delivery(GitOpsDeliveryExecutionRequest {
            gitops_change_set_id: rollback_intent_id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => Ok(Json(json!({
            "rollback_intent_id": rollback_intent_id,
            "status": "dispatched",
            "execution": ArtifactResponse::from(execution),
            "job_name": receipt.job_name,
            "created": true,
            "manual_merge_required": true,
            "automatic_rollback": false,
        }))),
        Err(error) => {
            tracing::warn!(rollback_intent_id = %rollback_intent_id, %error, "rollback writer dispatch failed");
            let failure = append_rollback_delivery_result(
                &state,
                &run,
                &rollback_intent_id,
                &execution_id,
                "dispatch_failed",
                json!({ "error_code": "job_dispatch_failed" }),
            )
            .await?;
            let mut updated = content.clone();
            updated.insert("status".to_string(), json!("attention_required"));
            updated.insert("writer_failure_result_id".to_string(), json!(failure.id));
            append_rollback_intent_artifact(&state, &run, &Value::Object(updated)).await?;
            Ok(Json(json!({
                "rollback_intent_id": rollback_intent_id,
                "status": "dispatch_failed",
                "execution": ArtifactResponse::from(failure),
                "created": true,
                "manual_merge_required": true,
                "automatic_rollback": false,
            })))
        }
    }
}

async fn dispatch_rollback_argo_sync(
    state: &AppState,
    rollback_intent_id: &str,
) -> Result<Json<Value>, ApiError> {
    let (item, current, run) = rollback_intent_context(state, rollback_intent_id).await?;
    let content = current
        .get("content")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("RollbackIntent content is unavailable"))?;
    validate_rollback_argo_grant(state, &item, rollback_intent_id, content).await?;
    if !state.worker.argo_executor_available()
        || !state
            .worker
            .argo_executor_allows_application(PROTECTED_ARGO_APPLICATION)
    {
        return Err(ApiError::conflict(
            "the isolated Argo executor is unavailable for the protected Application",
        ));
    }
    let artifacts = state.store.list_artifacts(&run.id).await?;
    if let Some(existing) = artifacts
        .iter()
        .filter(|artifact| artifact.kind == "rollback_argo_sync_execution")
        .filter(|artifact| {
            artifact.content_json.as_ref().is_some_and(|value| {
                value.get("rollback_intent_id").and_then(Value::as_str) == Some(rollback_intent_id)
            })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
    {
        return Ok(Json(json!({
            "rollback_intent_id": rollback_intent_id,
            "status": "argo_syncing",
            "execution": ArtifactResponse::from(existing.clone()),
            "created": false,
            "automatic_rollback": false,
        })));
    }
    let execution_id = format!("rbaexec_{}", unique_suffix());
    let execution = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_rollback_argo_sync_execution", unique_suffix()),
            session_id: run.session_id.clone(),
            run_id: Some(run.id.clone()),
            kind: "rollback_argo_sync_execution".to_string(),
            label: format!("Rollback Argo sync for {rollback_intent_id}"),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "rollback_intent_id": rollback_intent_id,
                "execution_id": execution_id,
                "status": "dispatched",
                "permission_grant_id": content.get("argo_permission_grant_id"),
                "deployment_contract_id": item.deployment_contract_id,
                "gitops_merge_sha": content.get("gitops_merge_sha"),
                "baseline_digest": content.get("baseline").and_then(|value| value.get("image_digest")),
                "target": protected_target_json(),
            })),
        })
        .await?;
    match state
        .worker
        .dispatch_argo_sync_execution(ArgoSyncExecutionRequest {
            deployment_intent_id: rollback_intent_id.to_string(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => {
            let mut updated = content.clone();
            updated.insert("status".to_string(), json!("argo_syncing"));
            updated.insert("argo_execution_id".to_string(), json!(execution_id));
            append_rollback_intent_artifact(state, &run, &Value::Object(updated)).await?;
            Ok(Json(json!({
                "rollback_intent_id": rollback_intent_id,
                "status": "argo_syncing",
                "execution": ArtifactResponse::from(execution),
                "job_name": receipt.job_name,
                "created": true,
                "automatic_rollback": false,
            })))
        }
        Err(error) => {
            tracing::warn!(rollback_intent_id, %error, "rollback Argo executor dispatch failed");
            let result = append_rollback_argo_sync_result(
                state,
                &run,
                rollback_intent_id,
                &execution_id,
                "dispatch_failed",
                json!({ "error_code": "job_dispatch_failed" }),
            )
            .await?;
            let mut updated = content.clone();
            updated.insert("status".to_string(), json!("attention_required"));
            updated.insert("argo_failure_result_id".to_string(), json!(result.id));
            append_rollback_intent_artifact(state, &run, &Value::Object(updated)).await?;
            Ok(Json(json!({
                "rollback_intent_id": rollback_intent_id,
                "status": "dispatch_failed",
                "execution": ArtifactResponse::from(result),
                "created": true,
                "automatic_rollback": false,
            })))
        }
    }
}

async fn validate_rollback_argo_grant(
    state: &AppState,
    item: &StoredWorkItem,
    rollback_intent_id: &str,
    content: &serde_json::Map<String, Value>,
) -> Result<StoredPermissionGrant, ApiError> {
    let grant_id = content
        .get("argo_permission_grant_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("RollbackIntent has no Argo permission grant"))?;
    let grant = state
        .store
        .get_permission_grant(grant_id)
        .await?
        .filter(|grant| grant.status == "active" && grant_is_unexpired(grant, current_millis()))
        .ok_or_else(|| {
            ApiError::conflict("rollback Argo permission grant is expired or revoked")
        })?;
    let scope = serde_json::from_value::<PermissionGrantScope>(grant.scope_json.clone())
        .map_err(|_| ApiError::conflict("rollback Argo permission grant scope is malformed"))?;
    let baseline_digest = content
        .get("baseline")
        .and_then(|value| value.get("image_digest"))
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("RollbackIntent baseline digest is unavailable"))?;
    let gitops_merge_sha = content
        .get("gitops_merge_sha")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("rollback GitOps merge SHA is unavailable"))?;
    let expected_pipeline_contracts = item
        .pipeline_contract_id
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let expected_deployment_contracts = item
        .deployment_contract_id
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&item.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent WorkPlan is unavailable"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent ChangeSet is unavailable"))?;
    let pipeline_intent = state
        .store
        .get_pipeline_intent_by_change_set(&change_set.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent PipelineIntent is unavailable"))?;
    let source_merge_sha = pipeline_intent
        .intent_json
        .pointer("/source_provenance/merge_commit_sha")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .ok_or_else(|| ApiError::conflict("rollback source merge provenance is unavailable"))?;
    if grant.subject != DEFAULT_ARGO_RUNNER_SUBJECT
        || scope.environment.as_deref() != Some(PROTECTED_ENVIRONMENT)
        || scope.capability_kinds != vec![CapabilityKind::ArgoSync]
        || !ARGO_SYNC_ACTIONS
            .iter()
            .all(|action| scope.actions.iter().any(|allowed| allowed == action))
        || scope.namespaces != vec![PROTECTED_NAMESPACE.to_string()]
        || scope.work_item_ids != vec![item.id.clone()]
        || scope.work_plan_ids != vec![work_plan.id.clone()]
        || scope.change_set_ids != vec![change_set.id.clone()]
        || scope.pipeline_intent_ids != vec![pipeline_intent.id.clone()]
        || scope.deployment_intent_ids != vec![rollback_intent_id.to_string()]
        || scope.argo_applications != vec![PROTECTED_ARGO_APPLICATION.to_string()]
        || scope.pipeline_contract_ids != expected_pipeline_contracts
        || scope.deployment_contract_ids != expected_deployment_contracts
        || scope.source_merge_shas != vec![source_merge_sha.to_string()]
        || scope.gitops_merge_shas != vec![gitops_merge_sha.to_string()]
        || scope.image_digests != vec![baseline_digest.to_string()]
        || scope.production_impacting != Some(true)
    {
        return Err(ApiError::conflict(
            "rollback Argo grant no longer matches the exact WorkItem, contracts, merge, digest, namespace, and Application",
        ));
    }
    let expected_gate_kinds = [
        "production_rollback_deployment",
        "cluster_mutation",
        "production_impact",
    ];
    let gate_ids = content
        .get("argo_approval_gate_ids")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::conflict("rollback Argo approval gates are unavailable"))?;
    if gate_ids.len() != expected_gate_kinds.len() {
        return Err(ApiError::conflict(
            "rollback Argo approval gate set is incomplete",
        ));
    }
    for (gate_id, expected_kind) in gate_ids.iter().zip(expected_gate_kinds) {
        let gate_id = gate_id
            .as_str()
            .ok_or_else(|| ApiError::conflict("rollback Argo approval gate ID is malformed"))?;
        let gate = state
            .store
            .get_approval_gate(gate_id)
            .await?
            .filter(|gate| gate.status == "satisfied")
            .ok_or_else(|| ApiError::conflict("rollback Argo approval gate is not satisfied"))?;
        let gate_scope = gate
            .gate_json
            .get("scope")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                ApiError::conflict("rollback Argo approval gate scope is unavailable")
            })?;
        if gate.work_item_id.as_deref() != Some(item.id.as_str())
            || gate.gate_kind != expected_kind
            || gate_scope.get("work_item_id").and_then(Value::as_str) != Some(item.id.as_str())
            || gate_scope.get("work_plan_id").and_then(Value::as_str) != Some(work_plan.id.as_str())
            || gate_scope.get("environment").and_then(Value::as_str) != Some(PROTECTED_ENVIRONMENT)
            || gate_scope
                .get("production_impacting")
                .and_then(Value::as_bool)
                != Some(true)
            || gate_scope.get("target_namespace").and_then(Value::as_str)
                != Some(PROTECTED_NAMESPACE)
            || gate_scope.get("argo_application").and_then(Value::as_str)
                != Some(PROTECTED_ARGO_APPLICATION)
            || gate
                .gate_json
                .get("gitops_merge_sha")
                .and_then(Value::as_str)
                != Some(gitops_merge_sha)
            || gate
                .gate_json
                .get("baseline_digest")
                .and_then(Value::as_str)
                != Some(baseline_digest)
        {
            return Err(ApiError::conflict(
                "rollback Argo approval gate no longer matches its immutable target binding",
            ));
        }
    }
    Ok(grant)
}

async fn append_rollback_argo_sync_result(
    state: &AppState,
    run: &pharness_store::StoredRun,
    rollback_intent_id: &str,
    execution_id: &str,
    status: &str,
    details: Value,
) -> Result<StoredArtifact, ApiError> {
    if let Some(existing) =
        state
            .store
            .list_artifacts(&run.id)
            .await?
            .into_iter()
            .find(|artifact| {
                artifact.kind == "rollback_argo_sync_result"
                    && artifact.content_json.as_ref().is_some_and(|content| {
                        content.get("rollback_intent_id").and_then(Value::as_str)
                            == Some(rollback_intent_id)
                            && content.get("execution_id").and_then(Value::as_str)
                                == Some(execution_id)
                            && content.get("status").and_then(Value::as_str) == Some(status)
                    })
            })
    {
        return Ok(existing);
    }
    Ok(state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_rollback_argo_sync_result", unique_suffix()),
            session_id: run.session_id.clone(),
            run_id: Some(run.id.clone()),
            kind: "rollback_argo_sync_result".to_string(),
            label: format!("Rollback Argo sync {status} for {rollback_intent_id}"),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "rollback_intent_id": rollback_intent_id,
                "execution_id": execution_id,
                "status": status,
                "details": details,
            })),
        })
        .await?)
}

async fn observe_rollback_intent(
    State(state): State<AppState>,
    Path(rollback_intent_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (_item, current, run) = rollback_intent_context(&state, &rollback_intent_id).await?;
    if current.pointer("/content/status").and_then(Value::as_str) == Some("argo_syncing") {
        return observe_rollback_argo_sync(&state, &rollback_intent_id, &current, &run).await;
    }
    require_fresh_capability(&state, "gitops_observer").await?;
    let settings = state
        .worker
        .gitops_observer_settings()
        .ok_or_else(|| ApiError::conflict("GitOps rollback observer is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == PROTECTED_GITOPS_REPO)
    {
        return Err(ApiError::conflict(
            "protected GitOps repository is not allowlisted for the rollback observer",
        ));
    }
    let artifacts = state.store.list_artifacts(&run.id).await?;
    let result = artifacts
        .iter()
        .filter(|artifact| artifact.kind == "rollback_delivery_result")
        .filter(|artifact| {
            artifact.content_json.as_ref().is_some_and(|value| {
                value.get("rollback_intent_id").and_then(Value::as_str)
                    == Some(rollback_intent_id.as_str())
                    && value.get("status").and_then(Value::as_str) == Some("completed")
            })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .ok_or_else(|| {
            ApiError::conflict(
                "rollback observation requires a completed GitOps pull-request delivery",
            )
        })?;
    let details = result
        .content_json
        .as_ref()
        .and_then(|value| value.get("details"))
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApiError::conflict("rollback delivery result has no pull-request provenance")
        })?;
    let source_commit_sha =
        required_json_string(details, "commit_sha", "rollback delivery result")?;
    let pull_request_url =
        required_json_string(details, "pull_request_url", "rollback delivery result")?;
    let pull_request_number = details
        .get("pull_request_number")
        .and_then(Value::as_u64)
        .ok_or_else(|| ApiError::conflict("rollback delivery result has no pull-request number"))?;
    let head_branch = required_json_string(details, "branch", "rollback delivery result")?;
    if !is_git_sha(&source_commit_sha) || !is_github_pr_url(&pull_request_url) {
        return Err(ApiError::conflict(
            "rollback delivery result has invalid immutable GitHub provenance",
        ));
    }
    if let Some(merged) = artifacts
        .iter()
        .filter(|artifact| artifact.kind == "rollback_delivery_observation")
        .filter(|artifact| {
            artifact.content_json.as_ref().is_some_and(|value| {
                value.get("rollback_intent_id").and_then(Value::as_str)
                    == Some(rollback_intent_id.as_str())
                    && value.get("merged").and_then(Value::as_bool) == Some(true)
            })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
    {
        return Ok(Json(
            json!({ "rollback_intent_id": rollback_intent_id, "status": "merged", "observation": ArtifactResponse::from(merged.clone()), "created": false, "manual_merge_required": true }),
        ));
    }
    let observation_index = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "rollback_delivery_observation_execution"
                && artifact.content_json.as_ref().is_some_and(|value| {
                    value.get("rollback_intent_id").and_then(Value::as_str)
                        == Some(rollback_intent_id.as_str())
                })
        })
        .count();
    let execution_id = format!("rbobs_{}", unique_suffix());
    let execution = state.store.create_artifact(CreateArtifact {
        id: format!("art_{}_rollback_delivery_observation", unique_suffix()),
        session_id: run.session_id.clone(), run_id: Some(run.id.clone()),
        kind: "rollback_delivery_observation_execution".to_string(),
        label: format!("Rollback PR observation {} for {}", observation_index + 1, rollback_intent_id),
        mime_type: Some("application/json".to_string()), path: None, content_text: None,
        content_json: Some(json!({ "rollback_intent_id": rollback_intent_id, "execution_id": execution_id, "status": "dispatched", "source": { "repository": PROTECTED_GITOPS_REPO, "head_branch": head_branch, "source_commit_sha": source_commit_sha, "pull_request_url": pull_request_url, "pull_request_number": pull_request_number, "github_api_url": settings.github_api_url } })),
    }).await?;
    match state
        .worker
        .dispatch_gitops_delivery_observation(GitOpsDeliveryObservationRequest {
            gitops_change_set_id: rollback_intent_id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => Ok(Json(
            json!({ "rollback_intent_id": rollback_intent_id, "status": "observation_dispatched", "execution": ArtifactResponse::from(execution), "job_name": receipt.job_name, "created": true, "manual_merge_required": true }),
        )),
        Err(error) => {
            tracing::warn!(rollback_intent_id = %rollback_intent_id, %error, "rollback observer dispatch failed");
            let failure = append_rollback_delivery_observation(
                &state,
                &run,
                &rollback_intent_id,
                &execution_id,
                "failed",
                json!({ "error_code": "job_dispatch_failed" }),
            )
            .await?;
            Ok(Json(
                json!({ "rollback_intent_id": rollback_intent_id, "status": "dispatch_failed", "observation": ArtifactResponse::from(failure), "created": true, "manual_merge_required": true }),
            ))
        }
    }
}

async fn observe_rollback_argo_sync(
    state: &AppState,
    rollback_intent_id: &str,
    current: &Value,
    run: &pharness_store::StoredRun,
) -> Result<Json<Value>, ApiError> {
    let content = current
        .get("content")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("RollbackIntent content is unavailable"))?;
    let execution_id = content
        .get("argo_execution_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("RollbackIntent has no Argo execution"))?;
    let expected_revision = content
        .get("gitops_merge_sha")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .ok_or_else(|| ApiError::conflict("rollback GitOps merge SHA is unavailable"))?;
    let expected_digest = content
        .get("baseline")
        .and_then(|value| value.get("image_digest"))
        .and_then(Value::as_str)
        .filter(|value| immutable_image_digest(value))
        .ok_or_else(|| ApiError::conflict("RollbackIntent baseline digest is invalid"))?;
    let artifacts = state.store.list_artifacts(&run.id).await?;
    let completed = artifacts
        .iter()
        .filter(|artifact| artifact.kind == "rollback_argo_sync_result")
        .filter(|artifact| {
            artifact.content_json.as_ref().is_some_and(|value| {
                value.get("rollback_intent_id").and_then(Value::as_str) == Some(rollback_intent_id)
                    && value.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                    && value.get("status").and_then(Value::as_str) == Some("completed")
            })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id));
    if completed.is_none() {
        let terminal = artifacts.iter().find(|artifact| {
            artifact.kind == "rollback_argo_sync_result"
                && artifact.content_json.as_ref().is_some_and(|value| {
                    value.get("rollback_intent_id").and_then(Value::as_str)
                        == Some(rollback_intent_id)
                        && value.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                        && matches!(
                            value.get("status").and_then(Value::as_str),
                            Some("failed" | "cancelled" | "dispatch_failed")
                        )
                })
        });
        return Ok(Json(json!({
            "rollback_intent_id": rollback_intent_id,
            "status": if terminal.is_some() { "attention_required" } else { "argo_syncing" },
            "ready": false,
            "automatic_rollback": false,
            "result": terminal.cloned().map(ArtifactResponse::from),
        })));
    }

    let (item, _, _) = rollback_intent_context(state, rollback_intent_id).await?;
    let contract_id = item
        .deployment_contract_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("RollbackIntent DeploymentContract is unavailable"))?;
    let contract = state
        .store
        .get_deployment_contract(contract_id)
        .await?
        .filter(|contract| contract.status == "active")
        .ok_or_else(|| ApiError::conflict("RollbackIntent DeploymentContract is not active"))?;
    let spec = deployment_contract_spec(&contract.contract_json)?;
    validate_protected_production_deployment_contract(&spec)?;
    if contract.target_environment != PROTECTED_ENVIRONMENT
        || contract.target_namespace != PROTECTED_NAMESPACE
        || contract.argo_application != PROTECTED_ARGO_APPLICATION
    {
        return Err(ApiError::conflict(
            "RollbackIntent DeploymentContract no longer matches the protected target",
        ));
    }

    let argo = execute_direct_capability(
        state,
        AgentAction::ArgoGetApp {
            id: ActionId::new(format!("act_{}_rollback_verify_argo", unique_suffix())),
            reason: format!("verify RollbackIntent {rollback_intent_id} Argo state"),
            app: PROTECTED_ARGO_APPLICATION.to_string(),
        },
        None,
    )
    .await?;
    let deployment = execute_direct_capability(
        state,
        AgentAction::KubernetesGet {
            id: ActionId::new(format!(
                "act_{}_rollback_verify_deployment",
                unique_suffix()
            )),
            reason: format!("verify RollbackIntent {rollback_intent_id} Deployment"),
            resource: "deployments".to_string(),
            namespace: Some(PROTECTED_NAMESPACE.to_string()),
            name: Some(PROTECTED_WORKLOAD_NAME.to_string()),
            all_namespaces: false,
            label_selector: None,
        },
        None,
    )
    .await?;
    let pods = execute_direct_capability(
        state,
        AgentAction::KubernetesGet {
            id: ActionId::new(format!("act_{}_rollback_verify_pods", unique_suffix())),
            reason: format!("verify RollbackIntent {rollback_intent_id} Pod imageIDs"),
            resource: "pods".to_string(),
            namespace: Some(PROTECTED_NAMESPACE.to_string()),
            name: None,
            all_namespaces: false,
            label_selector: Some("app=yfinance-wrapper".to_string()),
        },
        None,
    )
    .await?;
    let argo_content = argo
        .result
        .as_ref()
        .map(|result| &result.content)
        .ok_or_else(|| ApiError::conflict("rollback Argo verification did not execute"))?;
    let deployment_content = deployment
        .result
        .as_ref()
        .map(|result| &result.content)
        .ok_or_else(|| ApiError::conflict("rollback Deployment verification did not execute"))?;
    let pod_content = pods
        .result
        .as_ref()
        .map(|result| &result.content)
        .ok_or_else(|| ApiError::conflict("rollback Pod verification did not execute"))?;
    let argo_healthy = argo_content
        .pointer("/analysis/sync_status")
        .and_then(Value::as_str)
        == Some("Synced")
        && argo_content
            .pointer("/analysis/health_status")
            .and_then(Value::as_str)
            == Some("Healthy")
        && argo_content
            .pointer("/analysis/revision")
            .and_then(Value::as_str)
            == Some(expected_revision);
    let deployment_healthy = deployment_content
        .pointer("/analysis/status")
        .and_then(Value::as_str)
        == Some("healthy");
    let image_ids = pod_content
        .pointer("/output/items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|pod| {
            pod.pointer("/status/containers")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|container| container.get("imageID").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let digest_healthy = !image_ids.is_empty()
        && image_ids
            .iter()
            .all(|image_id| image_id.ends_with(expected_digest));
    let healthz = state
        .worker
        .verify_capability("yfinance_healthz", None)
        .await;
    let healthz_healthy = healthz.as_ref().is_ok_and(|outcome| outcome.available);
    let (prometheus_observation, prometheus_check) =
        verify_required_prometheus_inventory(state, None).await?;
    let prometheus_healthy = prometheus_check
        .get("passed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let verified = argo_healthy
        && deployment_healthy
        && digest_healthy
        && healthz_healthy
        && prometheus_healthy;
    let checks = vec![
        execution_check(
            "argo_application_synced_healthy_at_rollback_merge",
            argo_healthy,
            format!("{PROTECTED_ARGO_APPLICATION} checked at {expected_revision}"),
        ),
        execution_check(
            "declared_deployment_rollout_healthy",
            deployment_healthy,
            format!("{PROTECTED_NAMESPACE}/{PROTECTED_WORKLOAD_NAME} rollout checked"),
        ),
        execution_check(
            "running_image_digest",
            digest_healthy,
            format!(
                "{} running imageID(s) checked against {expected_digest}",
                image_ids.len()
            ),
        ),
        execution_check(
            "service_healthz",
            healthz_healthy,
            "Exact apps-prod/yfinance-wrapper Service /healthz checked",
        ),
        prometheus_check,
    ];
    let verification = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_rollback_verification", unique_suffix()),
            session_id: run.session_id.clone(),
            run_id: Some(run.id.clone()),
            kind: "rollback_verification".to_string(),
            label: format!("Rollback verification for {rollback_intent_id}"),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "rollback_intent_id": rollback_intent_id,
                "status": if verified { "verified" } else { "attention_required" },
                "gitops_merge_sha": expected_revision,
                "expected_digest": expected_digest,
                "argo_observation_id": argo.observation_id,
                "deployment_observation_id": deployment.observation_id,
                "pod_observation_id": pods.observation_id,
                "prometheus_observation_id": prometheus_observation.map(|observation| observation.id),
                "checks": checks,
            })),
        })
        .await?;
    let mut updated = content.clone();
    updated.insert(
        "status".to_string(),
        json!(if verified {
            "verified"
        } else {
            "attention_required"
        }),
    );
    updated.insert(
        "verification_artifact_id".to_string(),
        json!(verification.id),
    );
    append_rollback_intent_artifact(state, run, &Value::Object(updated)).await?;
    Ok(Json(json!({
        "rollback_intent_id": rollback_intent_id,
        "status": if verified { "verified" } else { "attention_required" },
        "verified": verified,
        "verification": ArtifactResponse::from(verification),
        "automatic_rollback": false,
    })))
}

async fn require_fresh_capability(state: &AppState, capability: &str) -> Result<(), ApiError> {
    let available = capability_statuses(state)
        .await?
        .into_iter()
        .any(|entry| entry.capability == capability && entry.status == "available");
    if available {
        Ok(())
    } else {
        Err(ApiError::conflict(format!(
            "{capability} requires a fresh passing isolated capability verification"
        )))
    }
}

async fn append_rollback_delivery_result(
    state: &AppState,
    run: &pharness_store::StoredRun,
    rollback_intent_id: &str,
    execution_id: &str,
    status: &str,
    details: Value,
) -> Result<StoredArtifact, ApiError> {
    Ok(state.store.create_artifact(CreateArtifact { id: format!("art_{}_rollback_delivery_result", unique_suffix()), session_id: run.session_id.clone(), run_id: Some(run.id.clone()), kind: "rollback_delivery_result".to_string(), label: format!("Rollback GitOps delivery {status} for {rollback_intent_id}"), mime_type: Some("application/json".to_string()), path: None, content_text: None, content_json: Some(json!({ "rollback_intent_id": rollback_intent_id, "execution_id": execution_id, "status": status, "details": details })) }).await?)
}

async fn append_rollback_delivery_observation(
    state: &AppState,
    run: &pharness_store::StoredRun,
    rollback_intent_id: &str,
    execution_id: &str,
    status: &str,
    details: Value,
) -> Result<StoredArtifact, ApiError> {
    Ok(state.store.create_artifact(CreateArtifact { id: format!("art_{}_rollback_delivery_observation", unique_suffix()), session_id: run.session_id.clone(), run_id: Some(run.id.clone()), kind: "rollback_delivery_observation".to_string(), label: format!("Rollback pull-request observation {status} for {rollback_intent_id}"), mime_type: Some("application/json".to_string()), path: None, content_text: None, content_json: Some(json!({ "rollback_intent_id": rollback_intent_id, "execution_id": execution_id, "status": status, "details": details, "pull_request_state": details.get("pull_request_state"), "merged": details.get("merged"), "merge_commit_sha": details.get("merge_commit_sha") })) }).await?)
}

async fn rollback_intent_context(
    state: &AppState,
    rollback_intent_id: &str,
) -> Result<(StoredWorkItem, Value, pharness_store::StoredRun), ApiError> {
    let items = state
        .store
        .list_work_items(WorkItemListFilter {
            limit: 200,
            ..Default::default()
        })
        .await?;
    for item in items {
        if let Some(current) =
            latest_rollback_intent(state, &item, Some(rollback_intent_id)).await?
        {
            let run_id = work_item_provenance_run_id(state, &item)
                .await?
                .ok_or_else(|| {
                    ApiError::conflict("RollbackIntent run provenance is unavailable")
                })?;
            let run = state
                .store
                .get_run(&run_id)
                .await?
                .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
            return Ok((item, current, run));
        }
    }
    Err(ApiError::not_found("rollback_intent", rollback_intent_id))
}

async fn latest_rollback_intent(
    state: &AppState,
    item: &StoredWorkItem,
    rollback_intent_id: Option<&str>,
) -> Result<Option<Value>, ApiError> {
    let Some(run_id) = work_item_provenance_run_id(state, item).await? else {
        return Ok(None);
    };
    let artifact = state
        .store
        .list_artifacts(&run_id)
        .await?
        .into_iter()
        .filter(|artifact| artifact.kind == "rollback_intent")
        .filter(|artifact| {
            artifact
                .content_json
                .as_ref()
                .and_then(|content| content.get("work_item_id"))
                .and_then(Value::as_str)
                == Some(item.id.as_str())
        })
        .filter(|artifact| {
            rollback_intent_id.map_or(true, |id| {
                artifact
                    .content_json
                    .as_ref()
                    .and_then(|content| content.get("rollback_intent_id"))
                    .and_then(Value::as_str)
                    == Some(id)
            })
        })
        .max_by_key(|artifact| (artifact.created_at.clone(), artifact.id.clone()));
    Ok(artifact.as_ref().map(rollback_intent_response))
}

/// Resolve the durable run that owns delivery and rollback evidence after a
/// coding attempt has finished. `current_run_id` is intentionally cleared at
/// attempt termination, so completed WorkItems must fall back to the captured
/// ChangeSet or WorkPlan provenance instead of losing their evidence graph.
async fn work_item_provenance_run_id(
    state: &AppState,
    item: &StoredWorkItem,
) -> Result<Option<RunId>, ApiError> {
    if let Some(run_id) = item.current_run_id.clone() {
        return Ok(Some(run_id));
    }
    let Some(work_plan) = state.store.get_work_plan_by_work_item(&item.id).await? else {
        return Ok(None);
    };
    if let Some(change_set) = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?
    {
        if change_set.run_id.is_some() {
            return Ok(change_set.run_id);
        }
    }
    Ok(work_plan.run_id)
}

async fn append_rollback_intent_artifact(
    state: &AppState,
    run: &pharness_store::StoredRun,
    content: &Value,
) -> Result<StoredArtifact, ApiError> {
    Ok(state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_rollback_intent", unique_suffix()),
            session_id: run.session_id.clone(),
            run_id: Some(run.id.clone()),
            kind: "rollback_intent".to_string(),
            label: format!(
                "RollbackIntent {}",
                content
                    .get("rollback_intent_id")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
            ),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(content.clone()),
        })
        .await?)
}

fn rollback_intent_response(artifact: &StoredArtifact) -> Value {
    json!({
        "artifact_id": artifact.id,
        "created_at": artifact.created_at,
        "content": artifact.content_json,
    })
}

async fn replan_work_item(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(work_item_id): Path<String>,
    Json(request): Json<ReplanWorkItemRequest>,
) -> Result<Json<ReplanWorkItemResponse>, ApiError> {
    let work_item = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    if !matches!(work_item.status.as_str(), "blocked" | "failed") {
        return Err(ApiError::conflict(
            "WorkItem replan requires blocked or failed status",
        ));
    }
    if !work_item_target_supported(&work_item) {
        return Err(ApiError::conflict(
            "WorkItem replan is limited to dev or the exact protected production target",
        ));
    }
    if work_item.attempt_count >= work_item.max_attempts {
        return Err(ApiError::conflict("WorkItem attempt budget is exhausted"));
    }
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("WorkItem has no WorkPlan to replan"))?;
    if work_plan.status != "approved" {
        return Err(ApiError::conflict(
            "WorkItem WorkPlan must remain approved before a retry can be scheduled",
        ));
    }
    if state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?
        .is_some()
    {
        return Err(ApiError::conflict(
            "WorkItem already has a ChangeSet; revise and review the WorkPlan before another coding attempt",
        ));
    }

    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = required_text(request.reason, "reason")?;
    let previous_status = work_item.status.clone();
    let work_item = state
        .store
        .finish_work_item_attempt(
            &work_item.id,
            "awaiting_approval",
            actor.clone(),
            Some(reason.clone()),
        )
        .await?;
    let attempts_remaining = work_item.max_attempts - work_item.attempt_count;
    append_work_item_audit_event(
        &state.store,
        &work_item,
        "work_item.replanned",
        actor,
        json!({
            "previous_status": previous_status,
            "reason": reason,
            "work_plan_id": work_plan.id,
            "attempt_count": work_item.attempt_count,
            "max_attempts": work_item.max_attempts,
            "attempts_remaining": attempts_remaining,
        }),
    )
    .await?;
    Ok(Json(ReplanWorkItemResponse {
        work_item: work_item.into(),
        work_plan: work_plan.into(),
        attempts_remaining,
    }))
}

async fn execute_work_item(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(work_item_id): Path<String>,
    Json(request): Json<ExecuteWorkItemRequest>,
) -> Result<Json<ExecuteWorkItemResponse>, ApiError> {
    let local_workspace = state.worker.supports_local_workspace();
    let remote_workspace = state.worker.supports_remote_workspace();
    if !local_workspace && !remote_workspace {
        return Err(ApiError::conflict(
            "real coding alpha requires a local or Kubernetes worker",
        ));
    }
    if remote_workspace && !state.workspace.remote_configured() {
        return Err(ApiError::conflict(
            "Kubernetes coding requires PHARNESS_WORKSPACE_ALLOWED_REMOTE_REPOS",
        ));
    }
    let work_item = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    if !work_item_target_supported(&work_item) {
        return Err(ApiError::conflict(
            "coding execution is limited to dev or the exact protected production target",
        ));
    }
    let environment_profile = match work_item.environment_profile_id.as_deref() {
        Some(profile_id) => Some(
            environment::select_profile(
                &state.environment_profiles,
                profile_id,
                &work_item.source_repo,
            )
            .map_err(ApiError::conflict)?
            .clone(),
        ),
        None if work_item.production_impacting => {
            return Err(ApiError::conflict(
                "production coding requires an immutable environment profile",
            ));
        }
        None => None,
    };
    if environment_profile.is_some() && !remote_workspace {
        return Err(ApiError::conflict(
            "immutable environment profile execution requires the Kubernetes worker",
        ));
    }
    if work_item.status != "awaiting_approval" {
        return Err(ApiError::conflict(
            "WorkItem must be awaiting_approval before an execution attempt can start",
        ));
    }
    if work_item.attempt_count >= work_item.max_attempts {
        return Err(ApiError::conflict("WorkItem attempt budget is exhausted"));
    }
    if request
        .max_turns
        .is_some_and(|requested| requested != work_item.run_budget.initial_turns)
    {
        return Err(ApiError::conflict(
            "attempt max_turns is fixed by the WorkItem RunBudget; update it through the wizard or a budget extension",
        ));
    }
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("WorkItem has no WorkPlan"))?;
    if work_plan.status != "approved" {
        return Err(ApiError::conflict(
            "WorkItem WorkPlan must be approved before source execution",
        ));
    }
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let workspace = state
        .store
        .list_workspaces(WorkspaceListFilter {
            work_item_id: Some(work_item_id.clone()),
            status: Some("declared".to_string()),
            limit: 1,
            ..Default::default()
        })
        .await?
        .into_iter()
        .next();
    let workspace = match workspace {
        Some(workspace) => workspace,
        None => {
            state
                .store
                .create_workspace(CreateWorkspace {
                    id: format!("ws_{}", unique_suffix()),
                    work_item_id: work_item.id.clone(),
                    run_id: None,
                    status: "declared".to_string(),
                    source_repo: work_item.source_repo.clone(),
                    source_ref: work_item.source_ref.clone(),
                    resolved_commit: None,
                    branch: None,
                    retention_status: "ephemeral".to_string(),
                    actor: actor.clone(),
                    reason: Some("retry attempt requires a fresh isolated workspace".to_string()),
                })
                .await?
        }
    };
    let reason = clean_optional_text(request.reason).unwrap_or_else(|| {
        if local_workspace {
            "bounded local coding attempt".to_string()
        } else {
            "bounded Kubernetes coding attempt".to_string()
        }
    });
    let attempt = work_item.attempt_count + 1;
    let (cwd, branch, base_commit, workspace_source, workspace_status, execution_kind) =
        if local_workspace {
            let provisioned = state
                .workspace
                .provision(
                    &work_item.id,
                    attempt,
                    &work_item.source_repo,
                    &work_item.source_ref,
                    work_item.source_commit.as_deref(),
                )
                .await
                .map_err(|error| ApiError::conflict(error.to_string()))?;
            (
                provisioned.cwd.to_string_lossy().to_string(),
                provisioned.branch,
                Some(provisioned.resolved_commit),
                None,
                "executing",
                "local_workspace",
            )
        } else {
            let branch = format!("pharness/{}/attempt-{attempt}", work_item.id);
            let source = pharness_runhost::WorkspaceSourceSpec {
                workspace_id: workspace.id.clone(),
                source_repo: work_item.source_repo.clone(),
                source_ref: work_item.source_ref.clone(),
                source_commit: work_item.source_commit.clone(),
                branch: branch.clone(),
                resolved_commit: None,
            };
            state
                .workspace
                .remote_source_allowed(&source)
                .map_err(|error| ApiError::conflict(error.to_string()))?;
            (
                state.worker.effective_cwd("/workspace"),
                branch,
                None,
                Some(source),
                "provisioning",
                "kubernetes_workspace",
            )
        };
    let run_id = RunId::new(format!("run_{}", unique_suffix()));
    let session_id = SessionId::new(format!("ses_{}", run_id.as_str()));
    let run_scope = RunScope {
        run_id: Some(run_id.to_string()),
        namespace: work_item.target_namespace.clone(),
        repo: Some(work_item.source_repo.clone()),
        branch: Some(branch.clone()),
        work_item_id: Some(work_item.id.clone()),
        workspace_id: Some(workspace.id.clone()),
        work_plan_id: Some(work_plan.id.clone()),
        change_set_id: None,
        production_impacting: work_item.production_impacting,
    };
    let contract = work_item
        .repository_contract_json
        .clone()
        .map(serde_json::from_value::<ProjectContract>)
        .transpose()
        .map_err(|error| {
            ApiError::internal(format!("stored repository contract is invalid: {error}"))
        })?;
    if work_item.production_impacting && contract.is_none() {
        return Err(ApiError::conflict(
            "production coding requires a validated repository contract",
        ));
    }
    if let Some(contract) = contract.as_ref() {
        create_permission_grant_record(
            &state.store,
            CreatePermissionGrantRequest {
                subject: state.policy.subject.clone(),
                created_by: actor.clone(),
                reason: format!(
                    "attempt-scoped workspace authorization for WorkItem {} run {}",
                    work_item.id, run_id
                ),
                scope: json!({
                    "environment": state.policy.environment,
                    "capability_kinds": ["filesystem"],
                    "actions": ["write_file", "patch_file", "create_directory"],
                    "max_risk": "medium",
                    "namespaces": work_item.target_namespace.iter().collect::<Vec<_>>(),
                    "repos": [work_item.source_repo],
                    "branches": [branch],
                    "run_ids": [run_id.to_string()],
                    "workspace_ids": [workspace.id],
                    "writable_path_globs": contract.writable_paths,
                    "work_item_ids": [work_item.id],
                    "work_plan_ids": [work_plan.id],
                    "production_impacting": work_item.production_impacting,
                }),
                policy: json!({ "policy_mode": "trusted_writes" }),
                expires_at: Some((current_millis() + 4 * 60 * 60 * 1_000).to_string()),
            },
        )
        .await?;
    }
    let mut policy = run_policy(&state.policy, None);
    policy.permission_grants = active_permission_grants(&state.store).await?;
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: format!("WorkItem attempt: {}", work_item.title),
            cwd: cwd.clone(),
        })
        .await?;
    let run = state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: coding_task_prompt(&work_item),
            cwd: cwd.clone(),
            max_turns: work_item.run_budget.initial_turns,
            initial_status: if environment_profile.is_some() {
                "preparing".to_string()
            } else {
                "queued".to_string()
            },
            execution_target_json: json!({
                "kind": execution_kind,
                "policy": &policy,
                "run_scope": run_scope.to_optional_json(),
                "workspace": {
                    "base_commit": base_commit,
                    "branch": branch,
                },
                "workspace_source": workspace_source,
                "run_budget": &work_item.run_budget,
                "environment_profile_id": work_item.environment_profile_id,
                "repository_contract": work_item.repository_contract_json,
                "selected_acceptance_commands": work_item.acceptance_criteria,
                "runner_profile": environment_profile.clone(),
            }),
        })
        .await?;
    let run = state
        .store
        .set_run_budget(
            &run.id,
            &work_item.run_budget,
            &RunBudgetConsumption {
                allowed_turns: work_item.run_budget.initial_turns,
                allowed_tokens: work_item.run_budget.initial_tokens,
                ..RunBudgetConsumption::default()
            },
        )
        .await?;
    let run = state.store.set_run_origin(&run.id, "controller").await?;
    let run = state
        .store
        .set_run_created_by(&run.id, actor.clone())
        .await?;
    state
        .store
        .append_event(&AgentEvent {
            event_id: EventId::new(format!("evt_{}_1", run_id.as_str())),
            session_id: session_id.clone(),
            run_id: run_id.clone(),
            seq: 1,
            kind: EventKind::RunQueued,
            payload: json!({
                "source": "work_item.execute",
                "worker": state.worker.mode(),
                "provider": state.worker.config_json().get("provider"),
                "model": state.worker.config_json().get("model"),
                "run_scope": run_scope.to_optional_json(),
                "base_commit": base_commit,
                "branch": branch,
            }),
        })
        .await?;
    let workspace = state
        .store
        .update_workspace_execution(
            &workspace.id,
            UpdateWorkspaceExecution {
                run_id: Some(run_id.clone()),
                status: workspace_status.to_string(),
                resolved_commit: base_commit.clone(),
                branch: Some(branch.clone()),
                actor: actor.clone(),
                reason: Some(reason.clone()),
            },
        )
        .await?;
    let work_item = state
        .store
        .start_work_item_attempt(&work_item.id, &run_id, actor.clone(), Some(reason))
        .await?;
    append_workspace_audit_event(
        &state.store,
        &workspace,
        if local_workspace {
            "workspace.provisioned"
        } else {
            "workspace.provisioning_requested"
        },
        actor.clone(),
    )
    .await?;
    append_work_item_audit_event(
        &state.store,
        &work_item,
        "work_item.execution_started",
        actor,
        json!({ "workspace_id": workspace.id, "run_id": run_id, "base_commit": workspace.resolved_commit, "branch": workspace.branch, "execution_kind": execution_kind }),
    )
    .await?;
    if let Some(profile) = environment_profile.as_ref() {
        let source_commit = work_item
            .source_commit
            .clone()
            .ok_or_else(|| ApiError::conflict("environment preparation requires source_commit"))?;
        let preparation = state
            .store
            .create_environment_preparation(CreateEnvironmentPreparation {
                id: format!("prep_{}", unique_suffix()),
                work_item_id: work_item.id.clone(),
                workspace_id: workspace.id.clone(),
                run_id: Some(run.id.clone()),
                status: "queued".to_string(),
                environment_profile_id: profile.id.clone(),
                source_commit,
            })
            .await?;
        let receipt = state
            .worker
            .dispatch_environment_preparation(&run, profile)
            .await
            .map_err(|error| ApiError::internal(error.to_string()))?;
        state
            .store
            .update_environment_preparation(UpdateEnvironmentPreparation {
                id: preparation.id,
                status: "running".to_string(),
                project_contract_json: None,
                project_contract_hash: None,
                environment_snapshot_json: None,
                logs_json: json!([{"step":"dispatch","status":"succeeded","job_name":receipt.job_name}]),
                error: None,
            })
            .await?;
    } else {
        state.worker.spawn_run(run.clone(), cwd);
    }
    Ok(Json(ExecuteWorkItemResponse {
        work_item: work_item.into(),
        workspace: workspace.into(),
        run: run.into(),
    }))
}

async fn capture_work_item_change_set(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(work_item_id): Path<String>,
    Json(request): Json<CaptureWorkItemChangeSetRequest>,
) -> Result<Json<CreateChangeSetResponse>, ApiError> {
    let work_item = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    if !matches!(work_item.status.as_str(), "executing" | "verifying") {
        return Err(ApiError::conflict(
            "WorkItem has no completed coding attempt to capture",
        ));
    }
    let workspace = state
        .store
        .list_workspaces(WorkspaceListFilter {
            work_item_id: Some(work_item_id.clone()),
            status: Some("verifying".to_string()),
            limit: 1,
            ..Default::default()
        })
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| {
            ApiError::conflict("WorkItem has no completed workspace ready for capture")
        })?;
    let run_id = workspace
        .run_id
        .clone()
        .ok_or_else(|| ApiError::conflict("workspace has no run"))?;
    let run = state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
    if run.status != "completed" {
        return Err(ApiError::conflict(
            "coding run must complete before ChangeSet capture",
        ));
    }
    if state
        .store
        .get_change_set_by_work_plan(
            &state
                .store
                .get_work_plan_by_work_item(&work_item_id)
                .await?
                .ok_or_else(|| ApiError::conflict("WorkItem has no WorkPlan"))?
                .id,
        )
        .await?
        .is_some()
    {
        return Err(ApiError::conflict(
            "WorkItem already has a captured ChangeSet",
        ));
    }
    let base_commit = workspace
        .resolved_commit
        .as_deref()
        .ok_or_else(|| ApiError::conflict("workspace has no pinned base commit"))?;
    let (evidence, diff_artifact_id, status_artifact_id, test_events) = if run
        .execution_target_json
        .get("workspace_source")
        .is_some()
    {
        worker_workspace_evidence(&state.store, &run, &workspace).await?
    } else {
        state
            .workspace
            .ensure_managed(FsPath::new(&run.cwd))
            .await
            .map_err(|error| ApiError::conflict(error.to_string()))?;
        let evidence = crate::workspace::collect_git_evidence(FsPath::new(&run.cwd), base_commit)
            .await
            .map_err(|error| ApiError::conflict(error.to_string()))?;
        let artifact = state
            .store
            .create_artifact(CreateArtifact {
                id: format!("art_{}", unique_suffix()),
                session_id: run.session_id.clone(),
                run_id: Some(run_id.clone()),
                kind: "workspace_git_diff".to_string(),
                label: format!("Git diff for {}", work_item.title),
                mime_type: Some("text/x-diff".to_string()),
                path: None,
                content_text: Some(evidence.diff.clone()),
                content_json: None,
            })
            .await?;
        let test_events = shell_test_evidence(&state.store.list_events(&run_id).await?);
        let source_status = state.store.create_artifact(CreateArtifact {
                id: format!("art_{}", unique_suffix()), session_id: run.session_id.clone(), run_id: Some(run_id.clone()),
                kind: "workspace_git_status".to_string(), label: "Captured Git status and test summaries".to_string(),
                mime_type: Some("application/json".to_string()), path: None, content_text: None,
                content_json: Some(json!({ "status": evidence.status, "changed_paths": evidence.changed_paths, "test_events": test_events })),
            }).await?;
        (evidence, artifact.id, source_status.id, test_events)
    };
    if evidence.changed_paths.is_empty() || evidence.diff.is_empty() {
        return Err(ApiError::conflict("coding run produced no source diff"));
    }
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("WorkItem has no WorkPlan"))?;
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let diff_hash = format!("{:x}", Sha256::digest(evidence.diff.as_bytes()));
    let change_set_json = json!({
        "source": { "kind": "workspace_git", "work_item_id": work_item.id, "workspace_id": workspace.id, "base_commit": base_commit, "branch": workspace.branch },
        "evidence": { "git_diff_artifact_id": diff_artifact_id, "git_status_artifact_id": status_artifact_id, "diff_sha256": diff_hash, "changed_paths": evidence.changed_paths },
        "verification": { "run_id": run_id, "test_event_count": test_events.len() }
    });
    let change_set = state
        .store
        .create_change_set(CreateChangeSet {
            id: format!("cset_{}", unique_suffix()),
            work_item_id: Some(work_item.id.clone()),
            work_plan_id: work_plan.id.clone(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: run.session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "proposed".to_string(),
            title: format!("ChangeSet: {}", work_item.title),
            summary: run
                .result_json
                .as_ref()
                .and_then(|result| result.get("summary"))
                .and_then(Value::as_str)
                .unwrap_or(&work_item.intent)
                .to_string(),
            risk_level: work_plan.risk_level.clone(),
            material_hash: material_hash(&change_set_json)?,
            resource_namespace: work_plan.resource_namespace.clone(),
            resource_kind: work_plan.resource_kind.clone(),
            resource_name: work_plan.resource_name.clone(),
            change_set_json,
        })
        .await?;
    let workspace = state
        .store
        .update_workspace_execution(
            &workspace.id,
            UpdateWorkspaceExecution {
                run_id: Some(run_id.clone()),
                status: "captured".to_string(),
                resolved_commit: Some(base_commit.to_string()),
                branch: workspace.branch.clone(),
                actor: actor.clone(),
                reason: clean_optional_text(request.reason),
            },
        )
        .await?;
    let work_item = state
        .store
        .update_work_item_status(
            &work_item.id,
            "awaiting_approval",
            actor.clone(),
            Some("real ChangeSet captured; source review required".to_string()),
        )
        .await?;
    append_change_set_audit_event(&state.store, &change_set, "change_set.captured", actor.clone(), None, json!({ "workspace_id": workspace.id, "run_id": run_id, "git_diff_artifact_id": diff_artifact_id, "git_status_artifact_id": status_artifact_id })).await?;
    append_workspace_audit_event(
        &state.store,
        &workspace,
        "workspace.change_set_captured",
        actor.clone(),
    )
    .await?;
    append_work_item_audit_event(
        &state.store,
        &work_item,
        "work_item.awaiting_approval",
        actor,
        json!({ "change_set_id": change_set.id }),
    )
    .await?;
    Ok(Json(CreateChangeSetResponse {
        change_set: change_set.into(),
        created: true,
    }))
}

async fn create_work_item_pipeline_intent(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(work_item_id): Path<String>,
    Json(request): Json<CreateWorkItemPipelineIntentRequest>,
) -> Result<Json<CreatePipelineIntentResponse>, ApiError> {
    let work_item = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("WorkItem has no WorkPlan"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?
        .ok_or_else(|| {
            ApiError::conflict(
                "WorkItem has no captured ChangeSet; source review and immutable merge evidence are required before a PipelineIntent",
            )
        })?;
    if change_set.work_item_id.as_deref() != Some(work_item.id.as_str()) {
        return Err(ApiError::conflict(
            "WorkItem ChangeSet lineage does not match the requested WorkItem",
        ));
    }

    let source_provenance = work_item_pipeline_source_provenance(&state.store, &change_set)
        .await?
        .ok_or_else(|| {
            ApiError::conflict(
                "WorkItem PipelineIntent requires immutable Git merge provenance before a pipeline definition",
            )
        })?;
    let pipeline_contract_id = required_text(request.pipeline_contract_id, "pipeline_contract_id")?;
    let pipeline_contract = state
        .store
        .get_pipeline_contract(&pipeline_contract_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_contract", &pipeline_contract_id))?;
    if pipeline_contract.status != "active" {
        return Err(ApiError::conflict(format!(
            "WorkItem PipelineIntent requires an active PipelineContract; {} is {}",
            pipeline_contract.id, pipeline_contract.status
        )));
    }
    let mut intent_json = request.intent_json.ok_or_else(|| {
        ApiError::bad_request(
            "WorkItem PipelineIntent requires an exact enabled Tekton execution definition",
        )
    })?;
    let execution = tekton_execution_spec(&intent_json)?;
    if !execution.enabled {
        return Err(ApiError::conflict(
            "WorkItem PipelineIntent execution must be enabled before it can be reviewed against a PipelineContract",
        ));
    }
    let source_revision = required_json_string(
        source_provenance.as_object().ok_or_else(|| {
            ApiError::internal("WorkItem source provenance must have an object body")
        })?,
        "merge_commit_sha",
        "WorkItem source provenance",
    )?;
    execution_matches_pipeline_contract(&execution, &pipeline_contract, Some(&source_revision))?;
    let intent_object = intent_json.as_object_mut().ok_or_else(|| {
        ApiError::bad_request("WorkItem PipelineIntent intent_json must be a JSON object")
    })?;
    intent_object.insert(
        "pipeline_contract".to_string(),
        json!({
            "id": pipeline_contract.id,
            "version": pipeline_contract.version,
            "namespace": pipeline_contract.namespace,
            "pipeline_ref": pipeline_contract.pipeline_ref,
        }),
    );
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    let Json(response) = create_pipeline_intent_from_change_set(
        State(state.clone()),
        Json(CreatePipelineIntentFromChangeSetRequest {
            change_set_id: change_set.id.clone(),
            title: request.title,
            summary: request.summary,
            risk_level: request.risk_level,
            intent_kind: request.intent_kind,
            intent_json: Some(intent_json),
            actor: actor.clone(),
            reason: reason.clone(),
        }),
    )
    .await?;
    if response.created {
        append_work_item_audit_event(
            &state.store,
            &work_item,
            "work_item.pipeline_intent_proposed",
            actor,
            json!({
                "work_plan_id": work_plan.id,
                "change_set_id": change_set.id,
                "pipeline_intent_id": response.pipeline_intent.id,
                "pipeline_contract_id": pipeline_contract.id,
                "pipeline_contract_version": pipeline_contract.version,
                "source_provenance": source_provenance,
                "reason": reason,
            }),
        )
        .await?;
    }

    Ok(Json(response))
}

#[derive(Debug, Default, serde::Deserialize)]
struct WorkItemPipelineContextQuery {
    namespace: Option<String>,
    pipeline_ref: Option<String>,
}

async fn work_item_pipeline_intent_context(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
    Query(query): Query<WorkItemPipelineContextQuery>,
) -> Result<Json<WorkItemPipelineContextResponse>, ApiError> {
    let work_item = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("WorkItem has no WorkPlan"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?
        .ok_or_else(|| {
            ApiError::conflict(
                "WorkItem has no captured ChangeSet; immutable source provenance is unavailable",
            )
        })?;
    if change_set.work_item_id.as_deref() != Some(work_item.id.as_str()) {
        return Err(ApiError::conflict(
            "WorkItem ChangeSet lineage does not match the requested WorkItem",
        ));
    }
    let source_provenance = work_item_pipeline_source_provenance(&state.store, &change_set)
        .await?
        .ok_or_else(|| {
            ApiError::conflict("WorkItem pipeline context requires immutable Git merge provenance")
        })?;
    let contract_namespace = clean_optional_text(query.namespace);
    let contract_pipeline_ref = clean_optional_text(query.pipeline_ref);
    let active_pipeline_contracts = state
        .store
        .list_pipeline_contracts(PipelineContractListFilter {
            namespace: contract_namespace.clone(),
            pipeline_ref: contract_pipeline_ref.clone(),
            status: Some("active".to_string()),
            limit: 200,
            offset: 0,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    let pipeline_intent = state
        .store
        .get_pipeline_intent_by_change_set(&change_set.id)
        .await?
        .map(Into::into);

    Ok(Json(WorkItemPipelineContextResponse {
        work_item: work_item.into(),
        work_plan: work_plan.into(),
        change_set: change_set.into(),
        pipeline_intent,
        source_provenance,
        contract_namespace,
        contract_pipeline_ref,
        active_pipeline_contracts,
    }))
}

fn coding_task_prompt(work_item: &StoredWorkItem) -> String {
    let criteria = work_item
        .acceptance_criteria
        .iter()
        .map(|criterion| format!("- {criterion}"))
        .collect::<Vec<_>>()
        .join("\n");
    let target = if work_item.production_impacting {
        "protected-production source WorkItem"
    } else {
        "development WorkItem"
    };
    format!(
        "Implement this {target} in the current workspace.\n\nIntent:\n{}\n\nAcceptance criteria:\n{}\n\nBoundaries: work only in this workspace; do not read secrets; do not use network access; do not push, commit, create a pull request, deploy, or modify Git configuration other than the isolated workspace's local configuration. Run focused tests when available. Finish with a concise summary of changes and tests.",
        work_item.intent,
        if criteria.is_empty() { "- No explicit criteria were supplied." } else { &criteria }
    )
}

pub(crate) fn shell_test_evidence(events: &[AgentEvent]) -> Vec<Value> {
    let mut active_action = None;
    let mut evidence = Vec::new();
    for event in events {
        if event.kind == EventKind::ToolStarted {
            active_action = event
                .payload
                .get("action")
                .and_then(Value::as_str)
                .map(str::to_string);
            continue;
        }
        if event.kind == EventKind::ToolFinished && active_action.as_deref() == Some("run_shell") {
            evidence.push(json!({
                "event_id": event.event_id,
                "status": event.payload.get("status"),
                "summary": event.payload.get("summary"),
            }));
        }
    }
    evidence
}

async fn worker_workspace_evidence(
    store: &SqliteStore,
    run: &pharness_store::StoredRun,
    workspace: &pharness_store::StoredWorkspace,
) -> Result<(crate::workspace::GitEvidence, String, String, Vec<Value>), ApiError> {
    let artifacts = store.list_artifacts(&run.id).await?;
    let diff_artifact = artifacts
        .iter()
        .rev()
        .find(|artifact| artifact.kind == "workspace_git_diff")
        .ok_or_else(|| ApiError::conflict("worker did not retain workspace Git diff evidence"))?;
    let status_artifact = artifacts
        .iter()
        .rev()
        .find(|artifact| artifact.kind == "workspace_git_status")
        .ok_or_else(|| ApiError::conflict("worker did not retain workspace Git status evidence"))?;
    let diff = diff_artifact
        .content_text
        .clone()
        .ok_or_else(|| ApiError::conflict("workspace Git diff artifact has no text content"))?;
    let status_json = status_artifact
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("workspace Git status artifact has no JSON content"))?;
    let base_commit = status_json
        .get("base_commit")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("workspace Git status artifact has no base commit"))?;
    if workspace.resolved_commit.as_deref() != Some(base_commit) {
        return Err(ApiError::conflict(
            "workspace Git evidence does not match the pinned base commit",
        ));
    }
    let branch = status_json
        .get("branch")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("workspace Git status artifact has no branch"))?;
    if workspace.branch.as_deref() != Some(branch) {
        return Err(ApiError::conflict(
            "workspace Git evidence does not match the pinned branch",
        ));
    }
    let status = status_json
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("workspace Git status artifact has no status"))?
        .to_string();
    let changed_paths = status_json
        .get("changed_paths")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::conflict("workspace Git status artifact has no changed paths"))?
        .iter()
        .map(|path| {
            path.as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| ApiError::conflict("workspace changed path is not a string"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let test_events = status_json
        .get("test_events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok((
        crate::workspace::GitEvidence {
            status,
            diff,
            changed_paths,
        },
        diff_artifact.id.clone(),
        status_artifact.id.clone(),
        test_events,
    ))
}

#[derive(Debug, Default, serde::Deserialize)]
struct ListWorkspacesQuery {
    work_item_id: Option<String>,
    run_id: Option<String>,
    status: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}

async fn list_workspaces(
    State(state): State<AppState>,
    Query(query): Query<ListWorkspacesQuery>,
) -> Result<Json<WorkspacesResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let workspaces = state
        .store
        .list_workspaces(WorkspaceListFilter {
            work_item_id: clean_optional_text(query.work_item_id),
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            status: clean_optional_text(query.status),
            limit,
            offset,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let count = workspaces.len();
    Ok(Json(WorkspacesResponse {
        workspaces,
        count,
        limit,
        offset,
    }))
}

async fn get_workspace(
    State(state): State<AppState>,
    Path(workspace_id): Path<String>,
) -> Result<Json<WorkspaceResponse>, ApiError> {
    let workspace = state
        .store
        .get_workspace(&workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("workspace", &workspace_id))?;
    Ok(Json(workspace.into()))
}

#[derive(Debug, Default, serde::Deserialize)]
struct ListWorkPlansQuery {
    work_item_id: Option<String>,
    remediation_plan_id: Option<String>,
    incident_id: Option<String>,
    run_id: Option<String>,
    status: Option<String>,
    origin: Option<String>,
    actor: Option<String>,
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
    let filter = WorkPlanListFilter {
        work_item_id: clean_optional_text(query.work_item_id),
        remediation_plan_id: clean_optional_text(query.remediation_plan_id),
        incident_id: clean_optional_text(query.incident_id),
        run_id: clean_optional_text(query.run_id).map(RunId::new),
        status: clean_optional_text(query.status),
        origin: clean_optional_text(query.origin),
        created_by: clean_optional_text(query.actor),
        risk_level: clean_optional_text(query.risk_level),
        resource_namespace: clean_optional_text(query.resource_namespace),
        resource_kind: clean_optional_text(query.resource_kind),
        resource_name: clean_optional_text(query.resource_name),
        created_after_ms: query.created_after_ms,
        created_before_ms: query.created_before_ms,
        limit,
        offset,
    };
    let work_plans = state
        .store
        .list_work_plans(filter.clone())
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<WorkPlanResponse>>();
    let count = work_plans.len();
    let group_work_plans = all_work_plans_for_operator_groups(state.store.as_ref(), filter).await?;
    let groups = group_operator_records(group_work_plans.iter().map(|plan| {
        (
            plan.id.clone(),
            plan.created_at.clone(),
            plan.title.clone(),
            operator_resource_label(
                plan.resource_namespace.as_deref(),
                plan.resource_kind.as_deref(),
                plan.resource_name.as_deref(),
            ),
            plan.status.clone(),
        )
    }));

    Ok(Json(WorkPlansResponse {
        work_plans,
        groups,
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
        if let Some(remediation_plan_id) = &work_plan.remediation_plan_id {
            state
                .store
                .stale_approval_gates_for_remediation_plan(
                    remediation_plan_id,
                    actor.clone(),
                    reason.clone().or_else(|| {
                        Some(format!(
                            "work plan {} revised from revision {} to {}",
                            work_plan.id, current.revision, work_plan.revision
                        ))
                    }),
                )
                .await?
        } else if let Some(work_item_id) = &work_plan.work_item_id {
            state
                .store
                .stale_approval_gates_for_work_item(
                    work_item_id,
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
        }
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
    if remediation_plan.requires_approval && remediation_plan.status != "approved" {
        return Err(ApiError::conflict(format!(
            "WorkPlan derivation requires an approved remediation plan; {} is {}",
            remediation_plan.id, remediation_plan.status
        )));
    }
    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let work_plan = state
        .store
        .create_work_plan(work_plan_from_remediation_plan(
            &remediation_plan,
            format!("wplan_{}", unique_suffix()),
        ))
        .await?;
    append_work_plan_audit_event(
        &state.store,
        &work_plan,
        "work_plan.created_from_remediation_plan",
        actor,
        reason,
        json!({
            "remediation_plan_id": remediation_plan.id,
            "incident_id": remediation_plan.incident_id,
            "remediation_plan_status": remediation_plan.status,
            "execution_enabled": false,
        }),
    )
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
        work_item_id: None,
        remediation_plan_id: Some(plan.id.clone()),
        incident_id: Some(plan.incident_id.clone()),
        session_id: plan.session_id.clone(),
        run_id: plan.run_id.clone(),
        status: "proposed".to_string(),
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
            "status": "proposed",
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

fn work_plan_from_work_item(
    item: &StoredWorkItem,
    session_id: SessionId,
    id: String,
) -> CreateWorkPlan {
    CreateWorkPlan {
        id,
        work_item_id: Some(item.id.clone()),
        remediation_plan_id: None,
        incident_id: None,
        session_id,
        run_id: None,
        status: "proposed".to_string(),
        title: format!("WorkPlan: {}", item.title),
        summary: item.intent.clone(),
        risk_level: if item.production_impacting {
            "high".to_string()
        } else {
            "medium".to_string()
        },
        requires_approval: true,
        resource_namespace: item.target_namespace.clone(),
        resource_kind: Some("application".to_string()),
        resource_name: item.argo_application.clone(),
        work_plan_json: json!({
            "source": { "kind": "work_item", "id": item.id },
            "intent": item.intent,
            "acceptance_criteria": item.acceptance_criteria,
            "source_repository": { "repo": item.source_repo, "ref": item.source_ref },
            "gitops_repository": { "repo": item.gitops_repo, "ref": item.gitops_ref },
            "target": {
                "environment": item.target_environment,
                "namespace": item.target_namespace,
                "argo_application": item.argo_application,
                "production_impacting": item.production_impacting,
            },
            "execution": {
                "enabled": false,
                "reason": "workspace execution requires a real pinned Git workspace",
            },
            "approval_gates": work_item_approval_gate_specs(item),
        }),
    }
}

fn work_item_approval_gate_specs(item: &StoredWorkItem) -> Vec<Value> {
    let mut gates = vec![
        json!({ "kind": "source_mutation", "required_before": "creating a source branch, commit, or pull request" }),
        json!({ "kind": "git_mutation", "required_before": "creating a source branch, commit, or pull request" }),
        json!({ "kind": "pipeline_mutation", "required_before": "starting a Tekton PipelineRun" }),
        json!({ "kind": "gitops_mutation", "required_before": "creating a GitOps branch, commit, or pull request" }),
        json!({ "kind": "cluster_mutation", "required_before": "syncing an Argo CD application" }),
    ];
    if item.production_impacting {
        gates.push(json!({ "kind": "production_impact", "required_before": "executing a production-impacting action" }));
        gates.push(json!({ "kind": "production_deployment", "required_before": "opening the bound production window and dispatching the exact Argo sync" }));
    }
    gates
}

fn approval_gates_from_work_item(
    item: &StoredWorkItem,
    work_plan: &StoredWorkPlan,
) -> Vec<CreateApprovalGate> {
    work_item_approval_gate_specs(item)
        .into_iter()
        .enumerate()
        .filter_map(|(index, gate_json)| {
            let gate_kind = approval_gate_kind(&gate_json)?;
            let mut gate_json = gate_json;
            gate_json["scope"] = work_item_approval_gate_scope(item, work_plan, &gate_kind);
            let gate_order = i64::try_from(index).ok()?.saturating_add(1);
            let required_before = gate_json
                .get("required_before")
                .and_then(Value::as_str)
                .unwrap_or("executing a risky action");
            Some(CreateApprovalGate {
                id: format!(
                    "agate_{}_{}_{}",
                    item.id,
                    gate_order,
                    safe_id_fragment(&gate_kind)
                ),
                work_item_id: Some(item.id.clone()),
                remediation_plan_id: None,
                incident_id: None,
                session_id: work_plan.session_id.clone(),
                run_id: work_plan.run_id.clone(),
                status: "pending".to_string(),
                gate_kind: gate_kind.clone(),
                gate_order,
                title: format!("Approve {}", gate_kind.replace('_', " ")),
                summary: format!("Approval required before {required_before}."),
                risk_level: work_plan.risk_level.clone(),
                resource_namespace: work_plan.resource_namespace.clone(),
                resource_kind: work_plan.resource_kind.clone(),
                resource_name: work_plan.resource_name.clone(),
                gate_json,
            })
        })
        .collect()
}

fn work_item_approval_gate_scope(
    item: &StoredWorkItem,
    work_plan: &StoredWorkPlan,
    gate_kind: &str,
) -> Value {
    json!({
        "work_item_id": item.id,
        "work_plan_id": work_plan.id,
        "environment": item.target_environment,
        "production_impacting": item.production_impacting,
        "source_repository": item.source_repo,
        "source_ref": item.source_ref,
        "gitops_repository": item.gitops_repo,
        "gitops_ref": item.gitops_ref,
        "target_namespace": item.target_namespace,
        "argo_application": item.argo_application,
        "actions": approval_gate_actions(gate_kind),
    })
}

fn approval_gate_actions(gate_kind: &str) -> &'static [&'static str] {
    match gate_kind {
        "source_mutation" | "git_mutation" => &GIT_DELIVERY_ACTIONS,
        "gitops_mutation" => &GITOPS_DELIVERY_ACTIONS,
        "pipeline_mutation" => &PIPELINE_DELIVERY_ACTIONS,
        "cluster_mutation" => &CLUSTER_DELIVERY_ACTIONS,
        "production_impact" | "production_deployment" => &PRODUCTION_DELIVERY_ACTIONS,
        _ => &[],
    }
}

fn work_item_gate_scope_matches(
    gate: &StoredApprovalGate,
    item: &StoredWorkItem,
    work_plan: &StoredWorkPlan,
    gate_kind: &str,
) -> bool {
    if gate.work_item_id.as_deref() != Some(item.id.as_str()) || gate.gate_kind != gate_kind {
        return false;
    }
    let Some(scope) = gate.gate_json.get("scope").and_then(Value::as_object) else {
        return false;
    };
    let actions = scope.get("actions").and_then(Value::as_array);
    let expected_actions = approval_gate_actions(gate_kind);
    scope.get("work_item_id").and_then(Value::as_str) == Some(item.id.as_str())
        && scope.get("work_plan_id").and_then(Value::as_str) == Some(work_plan.id.as_str())
        && scope.get("environment").and_then(Value::as_str)
            == Some(item.target_environment.as_str())
        && scope.get("production_impacting").and_then(Value::as_bool)
            == Some(item.production_impacting)
        && scope.get("source_repository").and_then(Value::as_str) == Some(item.source_repo.as_str())
        && scope.get("source_ref").and_then(Value::as_str) == Some(item.source_ref.as_str())
        && scope.get("gitops_repository") == Some(&json!(item.gitops_repo))
        && scope.get("gitops_ref") == Some(&json!(item.gitops_ref))
        && scope.get("target_namespace") == Some(&json!(item.target_namespace))
        && scope.get("argo_application") == Some(&json!(item.argo_application))
        && actions.is_some_and(|actions| {
            expected_actions.iter().all(|expected| {
                actions
                    .iter()
                    .any(|action| action.as_str() == Some(*expected))
            })
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkItemStatus {
    Submitted,
    Planning,
    AwaitingApproval,
    Executing,
    Verifying,
    Blocked,
    Completed,
    Failed,
    Cancelled,
}

impl WorkItemStatus {
    fn parse(value: &str) -> Result<Self, ApiError> {
        match value {
            "submitted" => Ok(Self::Submitted),
            "planning" => Ok(Self::Planning),
            "awaiting_approval" => Ok(Self::AwaitingApproval),
            "executing" => Ok(Self::Executing),
            "verifying" => Ok(Self::Verifying),
            "blocked" => Ok(Self::Blocked),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(ApiError::bad_request(format!(
                "unsupported work item status: {other}"
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Submitted => "submitted",
            Self::Planning => "planning",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Executing => "executing",
            Self::Verifying => "verifying",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn ensure_can_transition_to(self, target: Self) -> Result<(), ApiError> {
        if self == target {
            return Ok(());
        }
        let allowed = match self {
            Self::Submitted => matches!(target, Self::Planning | Self::Cancelled),
            Self::Planning => matches!(
                target,
                Self::AwaitingApproval | Self::Blocked | Self::Failed | Self::Cancelled
            ),
            Self::AwaitingApproval => matches!(
                target,
                Self::Executing | Self::Blocked | Self::Failed | Self::Cancelled
            ),
            Self::Executing => matches!(
                target,
                Self::Verifying | Self::Blocked | Self::Failed | Self::Cancelled
            ),
            Self::Verifying => matches!(
                target,
                Self::Completed | Self::Blocked | Self::Failed | Self::Cancelled
            ),
            Self::Blocked | Self::Failed => matches!(
                target,
                Self::Planning | Self::AwaitingApproval | Self::Cancelled
            ),
            Self::Completed | Self::Cancelled => false,
        };
        if allowed {
            Ok(())
        } else {
            Err(ApiError::conflict(format!(
                "cannot transition work item from {} to {}",
                self.as_str(),
                target.as_str()
            )))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemediationPlanStatus {
    Draft,
    Proposed,
    Approved,
    Executing,
    Blocked,
    Completed,
    Rejected,
    Stale,
}

impl RemediationPlanStatus {
    fn parse(value: &str) -> Result<Self, ApiError> {
        match value {
            "draft" => Ok(Self::Draft),
            "proposed" => Ok(Self::Proposed),
            "approved" => Ok(Self::Approved),
            "executing" => Ok(Self::Executing),
            "blocked" => Ok(Self::Blocked),
            "completed" => Ok(Self::Completed),
            "rejected" => Ok(Self::Rejected),
            "stale" => Ok(Self::Stale),
            other => Err(ApiError::bad_request(format!(
                "unsupported remediation plan status: {other}"
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
            Self::Stale => "stale",
        }
    }

    fn ensure_can_transition_to(self, target: Self) -> Result<(), ApiError> {
        if self == target {
            return Ok(());
        }
        let allowed = match self {
            Self::Draft => matches!(target, Self::Proposed | Self::Rejected),
            Self::Proposed => matches!(target, Self::Approved | Self::Rejected | Self::Draft),
            Self::Approved => matches!(
                target,
                Self::Executing | Self::Rejected | Self::Draft | Self::Stale
            ),
            Self::Executing => matches!(target, Self::Blocked | Self::Completed | Self::Stale),
            Self::Blocked => matches!(
                target,
                Self::Executing | Self::Rejected | Self::Draft | Self::Stale
            ),
            Self::Completed | Self::Rejected | Self::Stale => false,
        };
        if allowed {
            Ok(())
        } else {
            Err(ApiError::conflict(format!(
                "cannot transition remediation plan from {} to {}",
                self.as_str(),
                target.as_str()
            )))
        }
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
    work_item_id: Option<String>,
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
struct ListGitOpsChangeSetsQuery {
    work_item_id: Option<String>,
    pipeline_intent_id: Option<String>,
    deployment_intent_id: Option<String>,
    status: Option<String>,
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
            work_item_id: clean_optional_text(query.work_item_id),
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

async fn list_gitops_change_sets(
    State(state): State<AppState>,
    Query(query): Query<ListGitOpsChangeSetsQuery>,
) -> Result<Json<GitOpsChangeSetsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let gitops_change_sets = state
        .store
        .list_gitops_change_sets(GitOpsChangeSetListFilter {
            work_item_id: clean_optional_text(query.work_item_id),
            pipeline_intent_id: clean_optional_text(query.pipeline_intent_id),
            deployment_intent_id: clean_optional_text(query.deployment_intent_id),
            status: clean_optional_text(query.status),
            limit,
            offset,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let count = gitops_change_sets.len();
    Ok(Json(GitOpsChangeSetsResponse {
        gitops_change_sets,
        count,
        limit,
        offset,
    }))
}

async fn get_gitops_change_set(
    State(state): State<AppState>,
    Path(gitops_change_set_id): Path<String>,
) -> Result<Json<GitOpsChangeSetResponse>, ApiError> {
    let change_set = state
        .store
        .get_gitops_change_set(&gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", &gitops_change_set_id))?;
    Ok(Json(change_set.into()))
}

/// Materialize a GitOps ChangeSet from the immutable, digest-pinned planning
/// artifact. This writes Pharness state only; no Git repository is contacted.
async fn create_gitops_change_set(
    State(state): State<AppState>,
    Json(request): Json<CreateGitOpsChangeSetRequest>,
) -> Result<Json<CreateGitOpsChangeSetResponse>, ApiError> {
    let pipeline_intent_id = required_text(request.pipeline_intent_id, "pipeline_intent_id")?;
    let plan_artifact_id = required_text(
        request.gitops_update_plan_artifact_id,
        "gitops_update_plan_artifact_id",
    )?;
    let intent = state
        .store
        .get_pipeline_intent(&pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", &pipeline_intent_id))?;
    if !pipeline_intent_is_deployment_eligible(&intent.status)
        || intent
            .intent_json
            .pointer("/evidence/status")
            .and_then(Value::as_str)
            != Some("satisfied")
    {
        return Err(ApiError::conflict(
            "GitOps ChangeSet requires a completed PipelineIntent with satisfied evidence",
        ));
    }
    let existing = state
        .store
        .get_gitops_change_set_by_pipeline_intent(&intent.id)
        .await?;
    if let Some(existing) = existing {
        if existing.gitops_update_plan_artifact_id != plan_artifact_id {
            return Err(ApiError::conflict(
                "PipelineIntent already has a GitOps ChangeSet from a different update plan artifact",
            ));
        }
        return Ok(Json(CreateGitOpsChangeSetResponse {
            gitops_change_set: existing.into(),
            created: false,
        }));
    }
    let run_id = intent.run_id.clone().ok_or_else(|| {
        ApiError::conflict("GitOps ChangeSet requires PipelineIntent run provenance")
    })?;
    let artifact = state
        .store
        .get_artifact(&plan_artifact_id)
        .await?
        .ok_or_else(|| ApiError::not_found("artifact", &plan_artifact_id))?;
    if artifact.kind != "gitops_update_plan" || artifact.run_id.as_ref() != Some(&run_id) {
        return Err(ApiError::conflict(
            "GitOps ChangeSet must use a gitops_update_plan artifact from the PipelineIntent run",
        ));
    }
    let plan = artifact.content_json.as_ref().ok_or_else(|| {
        ApiError::conflict("GitOps update plan artifact is missing structured content")
    })?;
    if plan.get("kind").and_then(Value::as_str) != Some("gitops_update_plan")
        || plan.get("version").and_then(Value::as_i64) != Some(1)
        || plan.get("pipeline_intent_id").and_then(Value::as_str) != Some(intent.id.as_str())
        || plan.get("work_plan_id").and_then(Value::as_str) != Some(intent.work_plan_id.as_str())
        || plan.get("change_set_id").and_then(Value::as_str) != Some(intent.change_set_id.as_str())
    {
        return Err(ApiError::conflict(
            "GitOps update plan artifact does not match PipelineIntent lineage",
        ));
    }
    let work_plan = state
        .store
        .get_work_plan(&intent.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &intent.work_plan_id))?;
    let work_item_id = work_plan.work_item_id.as_deref().ok_or_else(|| {
        ApiError::conflict("GitOps ChangeSet requires a WorkItem-backed PipelineIntent")
    })?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    if !work_item_target_supported(&work_item) {
        return Err(ApiError::conflict(
            "GitOps ChangeSet creation is limited to dev or the exact protected production target",
        ));
    }
    if plan.get("work_item_id").and_then(Value::as_str) != Some(work_item.id.as_str()) {
        return Err(ApiError::conflict(
            "GitOps update plan artifact does not match WorkItem lineage",
        ));
    }
    let deployment_intent_id = plan
        .get("deployment_intent_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("GitOps update plan missing deployment_intent_id"))?
        .to_string();
    let deployment_intent = state
        .store
        .get_deployment_intent(&deployment_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_intent", &deployment_intent_id))?;
    if deployment_intent.pipeline_intent_id != intent.id
        || deployment_intent.change_set_id != intent.change_set_id
    {
        return Err(ApiError::conflict(
            "GitOps update plan DeploymentIntent does not match PipelineIntent lineage",
        ));
    }
    let gitops_repo = string_at(plan, "/gitops/repository")
        .ok_or_else(|| ApiError::conflict("GitOps update plan missing repository"))?;
    let gitops_ref = string_at(plan, "/gitops/base_ref")
        .ok_or_else(|| ApiError::conflict("GitOps update plan missing base_ref"))?;
    let head_branch = string_at(plan, "/gitops/head_branch")
        .ok_or_else(|| ApiError::conflict("GitOps update plan missing head_branch"))?;
    let kustomization_path = string_at(plan, "/update/kustomization_path")
        .ok_or_else(|| ApiError::conflict("GitOps update plan missing kustomization_path"))?;
    let image_name = string_at(plan, "/update/image_name")
        .ok_or_else(|| ApiError::conflict("GitOps update plan missing image_name"))?;
    let image_ref = string_at(plan, "/update/new_image")
        .ok_or_else(|| ApiError::conflict("GitOps update plan missing new_image"))?;
    let material_hash = string_at(plan, "/material_hash")
        .ok_or_else(|| ApiError::conflict("GitOps update plan missing material_hash"))?;
    if work_item.gitops_repo.as_deref() != Some(gitops_repo.as_str())
        || work_item.gitops_ref.as_deref() != Some(gitops_ref.as_str())
        || !safe_relative_gitops_path(&kustomization_path)
        || !image_ref.contains("@sha256:")
    {
        return Err(ApiError::conflict(
            "GitOps update plan no longer matches its declared WorkItem target or safety constraints",
        ));
    }
    let expected_material_hash = format!(
        "{:x}",
        Sha256::digest(
            format!(
                "{}\n{}\n{}\n{}\n{}",
                gitops_repo, gitops_ref, kustomization_path, image_name, image_ref
            )
            .as_bytes()
        )
    );
    if material_hash != expected_material_hash {
        return Err(ApiError::conflict(
            "GitOps update plan material hash does not match its immutable target and image update",
        ));
    }
    let change_set = state
        .store
        .create_gitops_change_set(CreateGitOpsChangeSet {
            id: format!("gcset_{}", unique_suffix()),
            work_item_id: work_item.id.clone(),
            work_plan_id: work_plan.id.clone(),
            source_change_set_id: intent.change_set_id.clone(),
            pipeline_intent_id: intent.id.clone(),
            deployment_intent_id: deployment_intent.id,
            gitops_update_plan_artifact_id: artifact.id,
            session_id: intent.session_id.clone(),
            run_id,
            status: "proposed".to_string(),
            title: format!("GitOps ChangeSet: {}", work_item.title),
            summary: format!(
                "Update {} at {} to digest-pinned image {}.",
                gitops_repo, kustomization_path, image_ref
            ),
            risk_level: intent.risk_level.clone(),
            material_hash,
            gitops_repo,
            gitops_ref,
            head_branch,
            kustomization_path,
            image_name,
            image_ref,
            gitops_change_set_json: json!({
                "kind": "gitops_change_set",
                "version": 1,
                "source_plan": plan,
                "execution": {
                    "enabled": false,
                    "reason": "requires approved GitOps ChangeSet, satisfied gitops_mutation gate, and dedicated GitOps writer"
                }
            }),
        })
        .await?;
    append_gitops_change_set_audit_event(
        &state.store,
        &change_set,
        "gitops_change_set.proposed",
        clean_optional_text(request.actor),
        clean_optional_text(request.reason),
        json!({ "source": "gitops_update_plan" }),
    )
    .await?;
    Ok(Json(CreateGitOpsChangeSetResponse {
        gitops_change_set: change_set.into(),
        created: true,
    }))
}

async fn transition_gitops_change_set(
    State(state): State<AppState>,
    Path(gitops_change_set_id): Path<String>,
    Json(request): Json<TransitionGitOpsChangeSetRequest>,
) -> Result<Json<TransitionGitOpsChangeSetResponse>, ApiError> {
    let current = state
        .store
        .get_gitops_change_set(&gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", &gitops_change_set_id))?;
    let target = GitOpsChangeSetStatus::parse(&request.target_status)?;
    GitOpsChangeSetStatus::parse(&current.status)?.ensure_can_transition_to(target)?;
    let change_set = state
        .store
        .update_gitops_change_set_status(
            &gitops_change_set_id,
            target.as_str(),
            clean_optional_text(request.actor.clone()),
            clean_optional_text(request.reason.clone()),
        )
        .await?;
    append_gitops_change_set_audit_event(
        &state.store,
        &change_set,
        &format!("gitops_change_set.{}", target.as_str()),
        clean_optional_text(request.actor),
        clean_optional_text(request.reason),
        json!({ "previous_status": current.status, "target_status": target.as_str() }),
    )
    .await?;
    Ok(Json(TransitionGitOpsChangeSetResponse {
        gitops_change_set: change_set.into(),
    }))
}

async fn repropose_failed_gitops_change_set(
    state: &AppState,
    gitops_change_set_id: &str,
    actor: String,
    reason: String,
) -> Result<TransitionGitOpsChangeSetResponse, ApiError> {
    let current = state
        .store
        .get_gitops_change_set(gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", gitops_change_set_id))?;
    if current.status != "approved" {
        return Err(ApiError::conflict(
            "GitOps delivery retry review requires an approved GitOps ChangeSet",
        ));
    }
    let artifacts = state.store.list_artifacts(&current.run_id).await?;
    let plan = artifacts
        .iter()
        .filter(|artifact| gitops_delivery_plan_matches_change_set(artifact, &current))
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .ok_or_else(|| ApiError::conflict("failed GitOps delivery has no current plan"))?;
    let result = artifacts
        .iter()
        .filter(|artifact| {
            gitops_delivery_artifact_matches_plan(artifact, "gitops_delivery_result", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .ok_or_else(|| ApiError::conflict("GitOps delivery has no terminal result"))?;
    let observation = artifacts
        .iter()
        .filter(|artifact| {
            gitops_delivery_artifact_matches_plan(
                artifact,
                "gitops_delivery_pr_observation",
                &plan.id,
            )
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id));
    let failed = result
        .content_json
        .as_ref()
        .and_then(|content| content.get("status"))
        .and_then(Value::as_str)
        .is_some_and(|status| matches!(status, "failed" | "dispatch_failed"));
    let closed_unmerged = observation.is_some_and(|observation| {
        gitops_observation_closed_unmerged(observation.content_json.as_ref())
    });
    if !failed && !closed_unmerged {
        return Err(ApiError::conflict(
            "GitOps ChangeSet can be re-proposed only after a failed bounded delivery or a closed, unmerged pull request",
        ));
    }
    if artifacts.iter().any(|artifact| {
        gitops_delivery_artifact_matches_plan(artifact, "gitops_delivery_merge", &plan.id)
    }) {
        return Err(ApiError::conflict(
            "GitOps ChangeSet cannot be re-proposed after an observed merge",
        ));
    }
    let grant_id = artifacts
        .iter()
        .filter(|artifact| {
            gitops_delivery_artifact_matches_plan(artifact, "gitops_delivery_execution", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .and_then(|artifact| artifact.content_json.as_ref())
        .and_then(|content| content.get("permission_grant_id"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    if let Some(grant_id) = grant_id.as_deref() {
        if state
            .store
            .get_permission_grant(grant_id)
            .await?
            .is_some_and(|grant| grant.status == "active")
        {
            state
                .store
                .revoke_permission_grant(
                    grant_id,
                    Some(actor.clone()),
                    Some(format!(
                        "GitOps ChangeSet {gitops_change_set_id} delivery reached a terminal state without merge; fresh review and authorization are required"
                    )),
                )
                .await?;
        }
    }
    let previous_revision = current.revision;
    let next_revision = previous_revision + 1;
    let base_branch = current
        .gitops_change_set_json
        .pointer("/source_plan/gitops/head_branch")
        .and_then(Value::as_str)
        .unwrap_or(current.head_branch.as_str());
    // A retry must be a sibling of the failed branch. Git cannot store both
    // `refs/heads/<branch>` and `refs/heads/<branch>/revision-N`, so nesting a
    // retry below a branch that was already pushed creates a ref-lock conflict.
    let next_head_branch = format!("{base_branch}-revision-{next_revision}");
    if next_head_branch.len() > 240
        || next_head_branch.starts_with('-')
        || next_head_branch.contains([' ', '~', '^', ':', '?', '*', '[', '\\', '\n'])
        || next_head_branch.contains("..")
    {
        return Err(ApiError::conflict(
            "GitOps ChangeSet cannot derive a safe revision-scoped retry branch",
        ));
    }
    let change_set = state
        .store
        .repropose_gitops_change_set(
            gitops_change_set_id,
            &next_head_branch,
            Some(actor.clone()),
            Some(reason.clone()),
        )
        .await?;
    append_gitops_change_set_audit_event(
        &state.store,
        &change_set,
        "gitops_change_set.reproposed_after_delivery_failure",
        Some(actor),
        Some(reason),
        json!({
            "previous_revision": previous_revision,
            "revision": change_set.revision,
            "failed_result_artifact_id": result.id,
            "failed_plan_artifact_id": plan.id,
            "closed_pull_request_observation_artifact_id": observation.map(|artifact| &artifact.id),
            "revoked_permission_grant_id": grant_id,
            "external_mutation_observed": closed_unmerged,
            "next_head_branch": change_set.head_branch,
        }),
    )
    .await?;
    Ok(TransitionGitOpsChangeSetResponse {
        gitops_change_set: change_set.into(),
    })
}

/// Resolve the declared GitOps base ref through the read-only observer
/// identity. The result is durable evidence for a later, separately guarded
/// writer; this route itself cannot mutate a repository.
async fn resolve_gitops_base_revision(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(gitops_change_set_id): Path<String>,
    Json(request): Json<ResolveGitOpsBaseRevisionRequest>,
) -> Result<Json<ResolveGitOpsBaseRevisionResponse>, ApiError> {
    let change_set = state
        .store
        .get_gitops_change_set(&gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", &gitops_change_set_id))?;
    if !matches!(change_set.status.as_str(), "proposed" | "approved") {
        return Err(ApiError::conflict(
            "GitOps base revision resolution requires a proposed or approved GitOps ChangeSet",
        ));
    }
    let settings = state.worker.gitops_observer_settings().ok_or_else(|| {
        ApiError::conflict(
            "read-only GitOps observer identity is not configured for GitOps revision resolution",
        )
    })?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &change_set.gitops_repo)
    {
        return Err(ApiError::conflict(
            "GitOps repository is not allowlisted for the read-only Git observer identity",
        ));
    }
    let reason = required_text(request.reason, "reason")?;
    let artifacts = state.store.list_artifacts(&change_set.run_id).await?;
    if let Some(existing) = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "gitops_base_revision_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("gitops_change_set_id").and_then(Value::as_str)
                        == Some(change_set.id.as_str())
                        && content.get("material_hash").and_then(Value::as_str)
                            == Some(change_set.material_hash.as_str())
                        && gitops_artifact_change_set_revision(content) == change_set.revision
                })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
    {
        let execution_id = existing
            .content_json
            .as_ref()
            .and_then(|content| content.get("execution_id"))
            .and_then(Value::as_str);
        let status = execution_id
            .and_then(|execution_id| {
                artifacts.iter().find_map(|artifact| {
                    (artifact.kind == "gitops_base_revision")
                        .then_some(artifact.content_json.as_ref())
                        .flatten()
                        .filter(|content| {
                            content.get("execution_id").and_then(Value::as_str)
                                == Some(execution_id)
                        })
                        .and_then(|content| content.get("status").and_then(Value::as_str))
                })
            })
            .unwrap_or("dispatched")
            .to_string();
        if status != "failed" {
            return Ok(Json(ResolveGitOpsBaseRevisionResponse {
                status,
                execution: existing.clone().into(),
                job_name: None,
                created: false,
            }));
        }
    }

    let execution_id = format!("grev_{}", unique_suffix());
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let execution = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_gitops_base_revision_execution", unique_suffix()),
            session_id: change_set.session_id.clone(),
            run_id: Some(change_set.run_id.clone()),
            kind: "gitops_base_revision_execution".to_string(),
            label: format!("GitOps base revision resolution for {}", change_set.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": execution_id,
                "status": "dispatched",
                "gitops_change_set_id": change_set.id,
                "gitops_change_set_revision": change_set.revision,
                "material_hash": change_set.material_hash,
                "repository": change_set.gitops_repo,
                "base_ref": change_set.gitops_ref,
                "operation": "resolve_base_revision",
                "identity": "agent:git-observer",
                "reason": reason,
            })),
        })
        .await?;
    match state
        .worker
        .dispatch_gitops_revision_resolution(GitOpsRevisionResolutionRequest {
            gitops_change_set_id: change_set.id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => {
            append_gitops_change_set_audit_event(
                &state.store,
                &change_set,
                "gitops_change_set.base_revision_dispatched",
                actor,
                Some(reason),
                json!({ "execution_id": execution_id, "execution_artifact_id": execution.id, "job_name": receipt.job_name }),
            )
            .await?;
            Ok(Json(ResolveGitOpsBaseRevisionResponse {
                status: "dispatched".to_string(),
                execution: execution.into(),
                job_name: Some(receipt.job_name),
                created: true,
            }))
        }
        Err(error) => {
            tracing::warn!(gitops_change_set_id = %change_set.id, %error, "GitOps base revision resolver dispatch failed");
            let result = state
                .store
                .create_artifact(CreateArtifact {
                    id: format!("art_{}_gitops_base_revision", unique_suffix()),
                    session_id: change_set.session_id.clone(),
                    run_id: Some(change_set.run_id.clone()),
                    kind: "gitops_base_revision".to_string(),
                    label: format!(
                        "Failed GitOps base revision resolution for {}",
                        change_set.id
                    ),
                    mime_type: Some("application/json".to_string()),
                    path: None,
                    content_text: None,
                    content_json: Some(json!({
                        "execution_id": execution_id,
                        "status": "failed",
                        "gitops_change_set_id": change_set.id,
                        "gitops_change_set_revision": change_set.revision,
                        "material_hash": change_set.material_hash,
                        "repository": change_set.gitops_repo,
                        "base_ref": change_set.gitops_ref,
                        "execution_artifact_id": execution.id,
                        "identity": "agent:git-observer",
                        "error_code": "job_dispatch_failed",
                    })),
                })
                .await?;
            append_gitops_change_set_audit_event(
                &state.store,
                &change_set,
                "gitops_change_set.base_revision_dispatch_failed",
                actor,
                Some(reason),
                json!({ "execution_id": execution_id, "execution_artifact_id": execution.id, "result_artifact_id": result.id }),
            )
            .await?;
            Ok(Json(ResolveGitOpsBaseRevisionResponse {
                status: "dispatch_failed".to_string(),
                execution: execution.into(),
                job_name: None,
                created: true,
            }))
        }
    }
}

/// Bind an approved GitOps ChangeSet to a read-only, immutable base revision.
/// This produces only a durable writer input: it cannot create a branch,
/// commit a manifest, open a pull request, or trigger Argo reconciliation.
async fn prepare_gitops_change_set_delivery(
    State(state): State<AppState>,
    Path(gitops_change_set_id): Path<String>,
    Json(request): Json<PrepareGitOpsDeliveryRequest>,
) -> Result<Json<GitOpsDeliveryPlanResponse>, ApiError> {
    let change_set = state
        .store
        .get_gitops_change_set(&gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", &gitops_change_set_id))?;
    if change_set.status != "approved" {
        return Err(ApiError::conflict(
            "GitOps delivery planning requires an approved GitOps ChangeSet",
        ));
    }
    let work_plan = state
        .store
        .get_work_plan(&change_set.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &change_set.work_plan_id))?;
    ensure_approved_for_trusted_envelope("work_plan", &work_plan.id, &work_plan.status)?;
    let work_item = state
        .store
        .get_work_item(&change_set.work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &change_set.work_item_id))?;
    ensure_gitops_delivery_target(&work_item, &change_set)?;
    if work_item.production_impacting
        && !latest_rollback_intent(&state, &work_item, None)
            .await?
            .is_some_and(|intent| {
                matches!(
                    intent.pointer("/content/status").and_then(Value::as_str),
                    Some("prepared" | "approved")
                ) && intent
                    .pointer("/content/baseline/image_digest")
                    .and_then(Value::as_str)
                    .is_some_and(immutable_image_digest)
            })
    {
        return Err(ApiError::conflict(
            "production GitOps authorization requires a captured baseline and prepared RollbackIntent",
        ));
    }

    let artifacts = state.store.list_artifacts(&change_set.run_id).await?;
    let base_revision = current_gitops_base_revision(&artifacts, &change_set)?;
    if let Some(existing) = artifacts
        .iter()
        .find(|artifact| gitops_delivery_plan_matches_change_set(artifact, &change_set))
    {
        return Ok(Json(GitOpsDeliveryPlanResponse {
            artifact: existing.clone().into(),
            base_revision: base_revision.into(),
            created: false,
        }));
    }
    let base_commit = base_revision
        .content_json
        .as_ref()
        .and_then(|content| content.get("base_commit"))
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("GitOps base revision has no resolved commit"))?;
    let plan = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_gitops_delivery_plan", unique_suffix()),
            session_id: change_set.session_id.clone(),
            run_id: Some(change_set.run_id.clone()),
            kind: "gitops_delivery_plan".to_string(),
            label: format!("GitOps delivery plan for {}", change_set.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "kind": "gitops_delivery_plan",
                "version": 1,
                "operation": "branch_and_pull_request",
                "gitops_change_set": {
                    "id": change_set.id,
                    "revision": change_set.revision,
                    "material_hash": change_set.material_hash,
                    "work_item_id": change_set.work_item_id,
                    "work_plan_id": change_set.work_plan_id,
                    "source_change_set_id": change_set.source_change_set_id,
                    "pipeline_intent_id": change_set.pipeline_intent_id,
                    "deployment_intent_id": change_set.deployment_intent_id,
                },
                "source": {
                    "repository": change_set.gitops_repo,
                    "base_ref": change_set.gitops_ref,
                    "base_commit": base_commit,
                    "head_branch": change_set.head_branch,
                    "base_revision_artifact_id": base_revision.id,
                    "identity": "agent:git-observer",
                },
                "update": {
                    "operation": "kustomize_set_image",
                    "kustomization_path": change_set.kustomization_path,
                    "image_name": change_set.image_name,
                    "new_image": change_set.image_ref,
                },
                "authorization": {
                    "state": "not_authorized",
                    "reason": "requires a satisfied gitops_mutation gate, matching GitOps writer grant, and dedicated GitOps writer preflight",
                },
                "execution": {
                    "enabled": true,
                    "mode": "gitops_writer_job",
                    "reason": "requires a satisfied gitops_mutation gate, matching plan-scoped grant, configured dedicated GitOps writer, and explicit delivery execution request",
                },
            })),
        })
        .await?;
    append_gitops_change_set_audit_event(
        &state.store,
        &change_set,
        "gitops_change_set.delivery_prepared",
        clean_optional_text(request.actor),
        clean_optional_text(request.reason),
        json!({
            "gitops_delivery_plan_artifact_id": plan.id,
            "gitops_base_revision_artifact_id": base_revision.id,
            "base_commit": base_commit,
        }),
    )
    .await?;

    Ok(Json(GitOpsDeliveryPlanResponse {
        artifact: plan.into(),
        base_revision: base_revision.into(),
        created: true,
    }))
}

fn current_gitops_base_revision(
    artifacts: &[StoredArtifact],
    change_set: &StoredGitOpsChangeSet,
) -> Result<StoredArtifact, ApiError> {
    artifacts
        .iter()
        .filter(|artifact| gitops_base_revision_matches_change_set(artifact, change_set))
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .ok_or_else(|| {
            ApiError::conflict(
                "GitOps delivery planning requires a current resolved immutable base revision",
            )
        })
}

fn gitops_base_revision_matches_change_set(
    artifact: &StoredArtifact,
    change_set: &StoredGitOpsChangeSet,
) -> bool {
    artifact.kind == "gitops_base_revision"
        && artifact.content_json.as_ref().is_some_and(|content| {
            content.get("status").and_then(Value::as_str) == Some("resolved")
                && content.get("gitops_change_set_id").and_then(Value::as_str)
                    == Some(change_set.id.as_str())
                && content.get("material_hash").and_then(Value::as_str)
                    == Some(change_set.material_hash.as_str())
                && gitops_artifact_change_set_revision(content) == change_set.revision
                && content.get("repository").and_then(Value::as_str)
                    == Some(change_set.gitops_repo.as_str())
                && content.get("base_ref").and_then(Value::as_str)
                    == Some(change_set.gitops_ref.as_str())
                && content
                    .get("base_commit")
                    .and_then(Value::as_str)
                    .is_some_and(is_git_sha)
        })
}

fn gitops_artifact_change_set_revision(content: &Value) -> i64 {
    content
        .get("gitops_change_set_revision")
        .and_then(Value::as_i64)
        .unwrap_or(1)
}

fn gitops_delivery_plan_matches_change_set(
    artifact: &StoredArtifact,
    change_set: &StoredGitOpsChangeSet,
) -> bool {
    artifact.kind == "gitops_delivery_plan"
        && artifact.content_json.as_ref().is_some_and(|plan| {
            plan.get("gitops_change_set")
                .and_then(|value| value.get("id"))
                .and_then(Value::as_str)
                == Some(change_set.id.as_str())
                && plan
                    .get("gitops_change_set")
                    .and_then(|value| value.get("revision"))
                    .and_then(Value::as_i64)
                    == Some(change_set.revision)
                && plan
                    .get("gitops_change_set")
                    .and_then(|value| value.get("material_hash"))
                    .and_then(Value::as_str)
                    == Some(change_set.material_hash.as_str())
        })
}

async fn current_gitops_delivery_plan(
    store: &SqliteStore,
    change_set: &StoredGitOpsChangeSet,
) -> Result<(StoredArtifact, StoredArtifact), ApiError> {
    let artifacts = store.list_artifacts(&change_set.run_id).await?;
    let plan = artifacts
        .iter()
        .filter(|artifact| gitops_delivery_plan_matches_change_set(artifact, change_set))
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .ok_or_else(|| {
            ApiError::conflict(
                "GitOps ChangeSet needs a current immutable delivery plan before authorization",
            )
        })?;
    let base_revision_id = plan
        .content_json
        .as_ref()
        .and_then(|content| content.pointer("/source/base_revision_artifact_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ApiError::conflict("GitOps delivery plan has no base revision provenance")
        })?;
    let base_revision = artifacts
        .into_iter()
        .find(|artifact| {
            artifact.id == base_revision_id
                && gitops_base_revision_matches_change_set(artifact, change_set)
        })
        .ok_or_else(|| {
            ApiError::conflict("GitOps delivery plan base revision is no longer current")
        })?;
    Ok((plan, base_revision))
}

async fn authorize_gitops_change_set_delivery(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(gitops_change_set_id): Path<String>,
    Json(request): Json<CreateGitOpsDeliveryAuthorizationRequest>,
) -> Result<Json<GitOpsDeliveryAuthorizationResponse>, ApiError> {
    let change_set = state
        .store
        .get_gitops_change_set(&gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", &gitops_change_set_id))?;
    if change_set.status != "approved" {
        return Err(ApiError::conflict(
            "GitOps delivery authorization requires an approved GitOps ChangeSet",
        ));
    }
    let work_plan = state
        .store
        .get_work_plan(&change_set.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &change_set.work_plan_id))?;
    ensure_approved_for_trusted_envelope("work_plan", &work_plan.id, &work_plan.status)?;
    let work_item = state
        .store
        .get_work_item(&change_set.work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &change_set.work_item_id))?;
    ensure_gitops_delivery_target(&work_item, &change_set)?;
    let (plan, _) = current_gitops_delivery_plan(&state.store, &change_set).await?;
    let subject = clean_optional_text(request.subject)
        .unwrap_or_else(|| DEFAULT_GITOPS_WRITER_SUBJECT.to_string());
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.created_by.clone()));
    let reason = required_text(request.reason, "reason")?;
    let expires_at = bounded_production_grant_expiry(&work_item, request.expires_at)?;
    if let Some(existing) =
        matching_gitops_delivery_grant(&state.store, &subject, &change_set, &work_item, &plan.id)
            .await?
    {
        return Ok(Json(GitOpsDeliveryAuthorizationResponse {
            grant: existing.into(),
            plan: plan.into(),
            created: false,
        }));
    }
    let grant = create_permission_grant_record(
        &state.store,
        CreatePermissionGrantRequest {
            subject,
            created_by: actor.clone(),
            reason: reason.clone(),
            scope: json!({
                "environment": work_item.target_environment,
                "capability_kinds": ["git"],
                "actions": GITOPS_DELIVERY_ACTIONS,
                "max_risk": "high",
                "repos": [change_set.gitops_repo],
                "branches": [change_set.head_branch],
                "work_plan_ids": [change_set.work_plan_id],
                "gitops_change_set_ids": [change_set.id],
                "gitops_delivery_plan_artifact_ids": [plan.id],
                "production_impacting": work_item.production_impacting,
            }),
            policy: json!({ "policy_mode": "supervised_autonomy" }),
            expires_at,
        },
    )
    .await?;
    append_gitops_change_set_audit_event(
        &state.store,
        &change_set,
        "gitops_change_set.delivery_authorized",
        actor,
        Some(reason),
        json!({
            "permission_grant_id": grant.id,
            "gitops_delivery_plan_artifact_id": plan.id,
            "subject": grant.subject,
        }),
    )
    .await?;
    Ok(Json(GitOpsDeliveryAuthorizationResponse {
        grant: grant.into(),
        plan: plan.into(),
        created: true,
    }))
}

async fn preflight_gitops_change_set_delivery(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(gitops_change_set_id): Path<String>,
    Json(request): Json<GitOpsDeliveryPreflightRequest>,
) -> Result<Json<GitOpsDeliveryPreflightResponse>, ApiError> {
    let change_set = state
        .store
        .get_gitops_change_set(&gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", &gitops_change_set_id))?;
    let work_plan = state
        .store
        .get_work_plan(&change_set.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &change_set.work_plan_id))?;
    let work_item = state
        .store
        .get_work_item(&change_set.work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &change_set.work_item_id))?;
    let (plan, base_revision) = current_gitops_delivery_plan(&state.store, &change_set).await?;
    let approval_gate = state
        .store
        .list_approval_gates(ApprovalGateListFilter {
            work_item_id: Some(work_item.id.clone()),
            gate_kind: Some("gitops_mutation".to_string()),
            limit: 20,
            ..ApprovalGateListFilter::default()
        })
        .await?
        .into_iter()
        .find(|gate| work_item_gate_scope_matches(gate, &work_item, &work_plan, "gitops_mutation"));
    let approval_gate_ready = approval_gate
        .as_ref()
        .is_some_and(|gate| matches!(gate.status.as_str(), "satisfied" | "waived"));
    let subject = clean_optional_text(request.subject)
        .unwrap_or_else(|| DEFAULT_GITOPS_WRITER_SUBJECT.to_string());
    let grant =
        matching_gitops_delivery_grant(&state.store, &subject, &change_set, &work_item, &plan.id)
            .await?;
    let authorization_ready = grant.is_some();
    let writer_settings = state.worker.gitops_writer_settings();
    let dispatch_ready = writer_settings.as_ref().is_some_and(|settings| {
        settings
            .allowed_repos
            .iter()
            .any(|repo| repo == &change_set.gitops_repo)
    });
    let target_valid = ensure_gitops_delivery_target(&work_item, &change_set).is_ok();
    let base_commit = base_revision
        .content_json
        .as_ref()
        .and_then(|content| content.get("base_commit"))
        .and_then(Value::as_str)
        .unwrap_or("missing");
    let checks = vec![
        execution_check(
            "gitops_change_set_approved",
            change_set.status == "approved",
            format!("GitOps ChangeSet status is {}", change_set.status),
        ),
        execution_check(
            "work_plan_approved",
            work_plan.status == "approved",
            format!("WorkPlan status is {}", work_plan.status),
        ),
        execution_check(
            "supported_gitops_target",
            target_valid,
            if target_valid {
                format!(
                    "GitOps target is {} at {}",
                    change_set.gitops_repo, change_set.gitops_ref
                )
            } else {
                "GitOps ChangeSet no longer matches a supported dev or exact protected-production WorkItem target"
                    .to_string()
            },
        ),
        execution_check(
            "immutable_gitops_base_revision",
            is_git_sha(base_commit),
            format!("Observer resolved GitOps base commit {base_commit}"),
        ),
        execution_check(
            "work_item_gitops_mutation_gate",
            approval_gate_ready,
            approval_gate
                .as_ref()
                .map(|gate| format!("GitOps mutation gate {} is {}", gate.id, gate.status))
                .unwrap_or_else(|| {
                    "No scoped WorkItem gitops_mutation gate matches this delivery plan".to_string()
                }),
        ),
        execution_check(
            "trusted_gitops_delivery_grant",
            authorization_ready,
            grant
                .as_ref()
                .map(|grant| {
                    format!(
                        "Active supervised-autonomy grant {} matches GitOps writer {}",
                        grant.id, subject
                    )
                })
                .unwrap_or_else(|| {
                    format!(
                        "No active supervised-autonomy GitOps delivery grant matches writer {}",
                        subject
                    )
                }),
        ),
        execution_check(
            "gitops_writer_executor_available",
            dispatch_ready,
            if writer_settings.is_none() {
                "No dedicated GitOps writer executor is configured; branch, commit, push, and pull-request creation remain unavailable".to_string()
            } else {
                format!(
                    "Dedicated GitOps writer is configured but does not allow repository {}",
                    change_set.gitops_repo
                )
            },
        ),
    ];
    let prerequisites_ready = checks
        .iter()
        .filter(|check| {
            check.get("code").and_then(Value::as_str) != Some("gitops_writer_executor_available")
        })
        .all(|check| check.get("passed").and_then(Value::as_bool) == Some(true));
    let status = if prerequisites_ready {
        "ready_for_writer"
    } else {
        "blocked"
    };
    let grant_id = grant.as_ref().map(|grant| grant.id.clone());
    let artifacts = state.store.list_artifacts(&change_set.run_id).await?;
    if let Some(existing) = artifacts.into_iter().find(|artifact| {
        artifact.kind == "gitops_delivery_preflight"
            && artifact.content_json.as_ref().is_some_and(|content| {
                content
                    .get("gitops_delivery_plan_artifact_id")
                    .and_then(Value::as_str)
                    == Some(plan.id.as_str())
                    && content.get("subject").and_then(Value::as_str) == Some(subject.as_str())
                    && content.get("permission_grant_id").and_then(Value::as_str)
                        == grant_id.as_deref()
                    && content.get("approval_gate_id").and_then(Value::as_str)
                        == approval_gate.as_ref().map(|gate| gate.id.as_str())
                    && content.get("approval_gate_status").and_then(Value::as_str)
                        == approval_gate.as_ref().map(|gate| gate.status.as_str())
            })
    }) {
        return Ok(Json(GitOpsDeliveryPreflightResponse {
            status: status.to_string(),
            approval_gate_ready,
            authorization_ready,
            dispatch_ready,
            plan: plan.into(),
            base_revision: base_revision.into(),
            permission_grant: grant.map(Into::into),
            checks,
            artifact: existing.into(),
            created: false,
        }));
    }
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    let artifact = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_gitops_delivery_preflight", unique_suffix()),
            session_id: change_set.session_id.clone(),
            run_id: Some(change_set.run_id.clone()),
            kind: "gitops_delivery_preflight".to_string(),
            label: format!("GitOps delivery preflight for {}", change_set.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "gitops_change_set_id": change_set.id,
                "work_plan_id": change_set.work_plan_id,
                "work_item_id": work_item.id,
                "gitops_delivery_plan_artifact_id": plan.id,
                "gitops_base_revision_artifact_id": base_revision.id,
                "subject": subject,
                "permission_grant_id": grant_id,
                "approval_gate_id": approval_gate.as_ref().map(|gate| &gate.id),
                "approval_gate_status": approval_gate.as_ref().map(|gate| &gate.status),
                "approval_gate_ready": approval_gate_ready,
                "status": status,
                "authorization_ready": authorization_ready,
                "dispatch_ready": dispatch_ready,
                "checks": checks,
                "dispatch": {
                    "state": if dispatch_ready { "configured" } else { "not_configured" },
                    "summary": if dispatch_ready { "Dedicated GitOps writer is configured for this exact repository; an explicit delivery execution request will still revalidate the gate and plan-scoped grant before it can create a branch and pull request" } else { "GitOps writer execution is unavailable until its separate identity and executor are configured" },
                },
                "reason": reason,
            })),
        })
        .await?;
    append_gitops_change_set_audit_event(
        &state.store,
        &change_set,
        "gitops_change_set.delivery_preflighted",
        actor,
        reason,
        json!({
            "gitops_delivery_plan_artifact_id": plan.id,
            "gitops_delivery_preflight_artifact_id": artifact.id,
            "permission_grant_id": grant_id,
            "approval_gate_id": approval_gate.as_ref().map(|gate| &gate.id),
            "approval_gate_ready": approval_gate_ready,
            "subject": subject,
            "status": status,
            "authorization_ready": authorization_ready,
            "dispatch_ready": dispatch_ready,
        }),
    )
    .await?;
    Ok(Json(GitOpsDeliveryPreflightResponse {
        status: status.to_string(),
        approval_gate_ready,
        authorization_ready,
        dispatch_ready,
        plan: plan.into(),
        base_revision: base_revision.into(),
        permission_grant: grant.map(Into::into),
        checks,
        artifact: artifact.into(),
        created: true,
    }))
}

async fn execute_gitops_change_set_delivery(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(gitops_change_set_id): Path<String>,
    Json(request): Json<ExecuteGitOpsDeliveryRequest>,
) -> Result<Json<ExecuteGitOpsDeliveryResponse>, ApiError> {
    let subject = clean_optional_text(request.subject.clone())
        .unwrap_or_else(|| DEFAULT_GITOPS_WRITER_SUBJECT.to_string());
    let actor = identity
        .as_ref()
        .map(|Extension(OperatorIdentity(name))| name.clone())
        .or_else(|| clean_optional_text(request.actor.clone()));
    let reason = clean_optional_text(Some(request.reason))
        .ok_or_else(|| ApiError::bad_request("GitOps delivery execution reason is required"))?;
    let Json(preflight) = preflight_gitops_change_set_delivery(
        State(state.clone()),
        identity,
        Path(gitops_change_set_id.clone()),
        Json(GitOpsDeliveryPreflightRequest {
            subject: Some(subject.clone()),
            actor: actor.clone(),
            reason: Some(reason.clone()),
        }),
    )
    .await?;
    if preflight.status != "ready_for_writer" || !preflight.dispatch_ready {
        return Err(ApiError::conflict(
            "GitOps delivery execution requires a current approved plan, satisfied gate, matching writer grant, and configured dedicated writer",
        ));
    }
    let grant = preflight.permission_grant.clone().ok_or_else(|| {
        ApiError::conflict("GitOps delivery execution requires an active matching writer grant")
    })?;
    let change_set = state
        .store
        .get_gitops_change_set(&gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", &gitops_change_set_id))?;
    let plan = state
        .store
        .get_artifact(&preflight.plan.id)
        .await?
        .ok_or_else(|| ApiError::conflict("current GitOps delivery plan is unavailable"))?;
    let source = gitops_delivery_plan_source(&plan, &change_set)?;
    let settings = state
        .worker
        .gitops_writer_settings()
        .ok_or_else(|| ApiError::conflict("GitOps writer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &source.repository)
    {
        return Err(ApiError::conflict(
            "GitOps delivery repository is not allowlisted for the dedicated GitOps writer",
        ));
    }
    let artifacts = state.store.list_artifacts(&change_set.run_id).await?;
    if let Some(existing) = artifacts.iter().find(|artifact| {
        gitops_delivery_artifact_matches_plan(artifact, "gitops_delivery_execution", &plan.id)
            && artifact.content_json.as_ref().is_some_and(|content| {
                content.get("permission_grant_id").and_then(Value::as_str)
                    == Some(grant.id.as_str())
            })
    }) {
        let terminal_status = existing
            .content_json
            .as_ref()
            .and_then(|content| content.get("execution_id"))
            .and_then(Value::as_str)
            .and_then(|execution_id| {
                artifacts.iter().find_map(|artifact| {
                    (artifact.kind == "gitops_delivery_result")
                        .then_some(artifact.content_json.as_ref())
                        .flatten()
                        .filter(|content| {
                            content.get("execution_id").and_then(Value::as_str)
                                == Some(execution_id)
                        })
                        .and_then(|content| content.get("status").and_then(Value::as_str))
                })
            })
            .unwrap_or("dispatched");
        return Ok(Json(ExecuteGitOpsDeliveryResponse {
            status: terminal_status.to_string(),
            execution: existing.clone().into(),
            plan: plan.into(),
            permission_grant: grant,
            job_name: existing
                .content_json
                .as_ref()
                .and_then(|content| content.get("job_name"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            created: false,
        }));
    }
    let execution_id = format!("gopsexec_{}", unique_suffix());
    let execution = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_gitops_delivery_execution", unique_suffix()),
            session_id: change_set.session_id.clone(),
            run_id: Some(change_set.run_id.clone()),
            kind: "gitops_delivery_execution".to_string(),
            label: format!("GitOps delivery execution for {}", change_set.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": execution_id,
                "status": "dispatched",
                "gitops_change_set_id": change_set.id,
                "gitops_delivery_plan_artifact_id": plan.id,
                "permission_grant_id": grant.id,
                "subject": subject,
                "dispatched_by": actor,
                "reason": reason,
                "source": {
                    "repository": source.repository,
                    "base_ref": source.base_ref,
                    "base_commit": source.base_commit,
                    "head_branch": source.head_branch,
                    "kustomization_path": source.kustomization_path,
                    "image_name": source.image_name,
                    "image_ref": source.image_ref,
                },
            })),
        })
        .await?;
    match state
        .worker
        .dispatch_gitops_delivery(GitOpsDeliveryExecutionRequest {
            gitops_change_set_id: change_set.id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => {
            append_gitops_change_set_audit_event(
                &state.store,
                &change_set,
                "gitops_change_set.delivery_dispatched",
                actor,
                Some(reason),
                json!({
                    "execution_id": execution_id,
                    "gitops_delivery_plan_artifact_id": plan.id,
                    "permission_grant_id": grant.id,
                    "job_name": receipt.job_name,
                }),
            )
            .await?;
            Ok(Json(ExecuteGitOpsDeliveryResponse {
                status: "dispatched".to_string(),
                execution: execution.into(),
                plan: plan.into(),
                permission_grant: grant,
                job_name: Some(receipt.job_name),
                created: true,
            }))
        }
        Err(error) => {
            let failure = persist_gitops_delivery_result(
                &state.store,
                &change_set,
                &plan.id,
                &execution_id,
                "dispatch_failed",
                json!({ "error_code": "job_dispatch_failed" }),
            )
            .await?;
            tracing::warn!(gitops_change_set_id = %change_set.id, %error, "GitOps writer dispatch failed");
            Ok(Json(ExecuteGitOpsDeliveryResponse {
                status: "dispatch_failed".to_string(),
                execution: failure,
                plan: plan.into(),
                permission_grant: grant,
                job_name: None,
                created: true,
            }))
        }
    }
}

async fn observe_gitops_change_set_delivery(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(gitops_change_set_id): Path<String>,
    Json(request): Json<ObserveGitOpsDeliveryRequest>,
) -> Result<Json<ObserveGitOpsDeliveryResponse>, ApiError> {
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(Some(request.reason))
        .ok_or_else(|| ApiError::bad_request("GitOps delivery observation reason is required"))?;
    let change_set = state
        .store
        .get_gitops_change_set(&gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", &gitops_change_set_id))?;
    let (plan, _) = current_gitops_delivery_plan(&state.store, &change_set).await?;
    let source = gitops_delivery_plan_source(&plan, &change_set)?;
    let settings = state
        .worker
        .gitops_observer_settings()
        .ok_or_else(|| ApiError::conflict("GitOps observer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &source.repository)
    {
        return Err(ApiError::conflict(
            "GitOps delivery repository is not allowlisted for the Git observer",
        ));
    }
    let artifacts = state.store.list_artifacts(&change_set.run_id).await?;
    let delivery_result = artifacts
        .iter()
        .filter(|artifact| {
            gitops_delivery_artifact_matches_plan(artifact, "gitops_delivery_result", &plan.id)
        })
        .filter(|artifact| {
            artifact
                .content_json
                .as_ref()
                .and_then(|content| content.get("status"))
                .and_then(Value::as_str)
                == Some("completed")
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .ok_or_else(|| {
            ApiError::conflict(
                "GitOps delivery observation requires a completed branch-and-PR result",
            )
        })?;
    let details = delivery_result
        .content_json
        .as_ref()
        .and_then(|content| content.get("details"))
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApiError::conflict("GitOps delivery result has no pull-request provenance")
        })?;
    let pull_request_number = details
        .get("pull_request_number")
        .and_then(Value::as_u64)
        .ok_or_else(|| ApiError::conflict("GitOps delivery result has no pull-request number"))?;
    let pull_request_url =
        required_json_string(details, "pull_request_url", "GitOps delivery result")?;
    let source_commit_sha = required_json_string(details, "commit_sha", "GitOps delivery result")?;
    if !is_git_sha(&source_commit_sha) || !is_github_pr_url(&pull_request_url) {
        return Err(ApiError::conflict(
            "GitOps delivery result has invalid GitHub provenance",
        ));
    }
    if let Some(existing) = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "gitops_delivery_observation_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content
                        .get("gitops_delivery_plan_artifact_id")
                        .and_then(Value::as_str)
                        == Some(plan.id.as_str())
                        && content
                            .get("gitops_delivery_result_artifact_id")
                            .and_then(Value::as_str)
                            == Some(delivery_result.id.as_str())
                        && !artifacts.iter().any(|failure| {
                            failure.kind == "gitops_delivery_observation_dispatch_failure"
                                && failure
                                    .content_json
                                    .as_ref()
                                    .is_some_and(|failure_content| {
                                        failure_content.get("execution_id").and_then(Value::as_str)
                                            == content.get("execution_id").and_then(Value::as_str)
                                    })
                        })
                })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
    {
        let execution_id = existing
            .content_json
            .as_ref()
            .and_then(|content| content.get("execution_id"))
            .and_then(Value::as_str);
        let terminal_observation = execution_id.and_then(|execution_id| {
            artifacts
                .iter()
                .filter(|artifact| {
                    gitops_delivery_artifact_matches_plan(
                        artifact,
                        "gitops_delivery_pr_observation",
                        &plan.id,
                    ) && artifact.content_json.as_ref().is_some_and(|content| {
                        content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                    })
                })
                .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        });
        if !terminal_observation.is_some_and(|observation| {
            gitops_observation_refreshable(observation.content_json.as_ref())
        }) {
            return Ok(Json(ObserveGitOpsDeliveryResponse {
                status: existing
                    .content_json
                    .as_ref()
                    .and_then(|content| content.get("status"))
                    .and_then(Value::as_str)
                    .unwrap_or("dispatched")
                    .to_string(),
                execution: existing.clone().into(),
                job_name: existing
                    .content_json
                    .as_ref()
                    .and_then(|content| content.get("job_name"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                created: false,
            }));
        }
    }
    let execution_id = format!("gopsobs_{}", unique_suffix());
    let execution = state.store.create_artifact(CreateArtifact {
        id: format!("art_{}_gitops_delivery_observation", unique_suffix()),
        session_id: change_set.session_id.clone(), run_id: Some(change_set.run_id.clone()),
        kind: "gitops_delivery_observation_execution".to_string(),
        label: format!("GitOps delivery observation for {}", change_set.id),
        mime_type: Some("application/json".to_string()), path: None, content_text: None,
        content_json: Some(json!({"execution_id":execution_id,"status":"dispatched","gitops_change_set_id":change_set.id,"gitops_delivery_plan_artifact_id":plan.id,"gitops_delivery_result_artifact_id":delivery_result.id,
            "source":{"repository":source.repository,"head_branch":source.head_branch,"source_commit_sha":source_commit_sha,"pull_request_url":pull_request_url,"pull_request_number":pull_request_number},"dispatched_by":actor,"reason":reason})),
    }).await?;
    match state
        .worker
        .dispatch_gitops_delivery_observation(GitOpsDeliveryObservationRequest {
            gitops_change_set_id: change_set.id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => {
            append_gitops_change_set_audit_event(&state.store, &change_set, "gitops_change_set.delivery_observation_dispatched", actor, Some(reason), json!({"execution_id":execution_id,"gitops_delivery_plan_artifact_id":plan.id,"job_name":receipt.job_name})).await?;
            Ok(Json(ObserveGitOpsDeliveryResponse {
                status: "dispatched".to_string(),
                execution: execution.into(),
                job_name: Some(receipt.job_name),
                created: true,
            }))
        }
        Err(error) => {
            tracing::warn!(gitops_change_set_id = %change_set.id, %error, "GitOps observer dispatch failed");
            let failure = state
                .store
                .create_artifact(CreateArtifact {
                    id: format!(
                        "art_{}_gitops_delivery_observation_dispatch_failure",
                        unique_suffix()
                    ),
                    session_id: change_set.session_id.clone(),
                    run_id: Some(change_set.run_id.clone()),
                    kind: "gitops_delivery_observation_dispatch_failure".to_string(),
                    label: format!(
                        "GitOps delivery observation dispatch failure for {}",
                        change_set.id
                    ),
                    mime_type: Some("application/json".to_string()),
                    path: None,
                    content_text: None,
                    content_json: Some(json!({
                        "execution_id": execution_id,
                        "status": "dispatch_failed",
                        "gitops_change_set_id": change_set.id,
                        "gitops_delivery_plan_artifact_id": plan.id,
                        "gitops_delivery_result_artifact_id": delivery_result.id,
                        "error_code": "gitops_observer_dispatch_failed",
                    })),
                })
                .await?;
            append_gitops_change_set_audit_event(
                &state.store,
                &change_set,
                "gitops_change_set.delivery_observation_dispatch_failed",
                actor,
                Some(reason),
                json!({
                    "execution_id": execution_id,
                    "gitops_delivery_plan_artifact_id": plan.id,
                    "dispatch_failure_artifact_id": failure.id,
                    "error_code": "gitops_observer_dispatch_failed",
                }),
            )
            .await?;
            Ok(Json(ObserveGitOpsDeliveryResponse {
                status: "dispatch_failed".to_string(),
                execution: execution.into(),
                job_name: None,
                created: true,
            }))
        }
    }
}

fn ensure_gitops_delivery_target(
    work_item: &StoredWorkItem,
    change_set: &StoredGitOpsChangeSet,
) -> Result<(), ApiError> {
    if !work_item_target_supported(work_item) {
        return Err(ApiError::conflict(
            "GitOps delivery is limited to dev or the exact protected production target",
        ));
    }
    if work_item.gitops_repo.as_deref() != Some(change_set.gitops_repo.as_str())
        || work_item.gitops_ref.as_deref() != Some(change_set.gitops_ref.as_str())
        || !safe_relative_gitops_path(&change_set.kustomization_path)
        || !change_set.image_ref.contains("@sha256:")
    {
        return Err(ApiError::conflict(
            "GitOps ChangeSet no longer matches its declared WorkItem target or safety constraints",
        ));
    }
    Ok(())
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
    let gitops_change_set = if let Some(pipeline_intent) = &pipeline_intent {
        store
            .get_gitops_change_set_by_pipeline_intent(&pipeline_intent.id)
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
    let git_delivery = git_delivery_flow(store, change_set.as_ref()).await?;
    let gitops_delivery = gitops_delivery_flow(store, gitops_change_set.as_ref()).await?;
    let readiness = build_sdlc_readiness(
        store,
        resource_kind,
        resource_id,
        work_plan.clone(),
        change_set.clone(),
    )
    .await?;
    let incidents =
        collect_sdlc_flow_incidents(store, work_plan.incident_id.as_deref(), release.as_ref())
            .await?;
    let remediation_plans =
        collect_sdlc_flow_remediation_plans(store, &work_plan, &incidents).await?;
    let approval_gates =
        collect_sdlc_flow_approval_gates(store, &work_plan, &remediation_plans).await?;
    let audit_events = collect_sdlc_flow_audit_events(
        store,
        &work_plan,
        change_set.as_ref(),
        pipeline_intent.as_ref(),
        gitops_change_set.as_ref(),
        deployment_intent.as_ref(),
        release.as_ref(),
        registry_evidence.as_ref(),
        &incidents,
        &remediation_plans,
        &approval_gates,
    )
    .await?;

    let mut flow = SdlcFlowResponse {
        resource_kind: resource_kind.to_string(),
        resource_id: resource_id.to_string(),
        readiness,
        delivery_segments: Vec::new(),
        work_plan: work_plan.into(),
        change_set: change_set.map(Into::into),
        pipeline_intent: pipeline_intent.map(Into::into),
        gitops_change_set: gitops_change_set.map(Into::into),
        deployment_intent: deployment_intent.map(Into::into),
        release: release.map(Into::into),
        registry_evidence: registry_evidence.map(Into::into),
        git_delivery,
        gitops_delivery,
        incidents: incidents.into_iter().map(Into::into).collect(),
        remediation_plans: remediation_plans.into_iter().map(Into::into).collect(),
        approval_gates: approval_gates.into_iter().map(Into::into).collect(),
        audit_events: audit_events.into_iter().map(Into::into).collect(),
    };
    flow.delivery_segments = sdlc_flow_delivery_segments(&flow, None);
    Ok(flow)
}

async fn git_delivery_flow(
    store: &SqliteStore,
    change_set: Option<&StoredChangeSet>,
) -> Result<Option<GitDeliveryFlowResponse>, ApiError> {
    let Some(change_set) = change_set else {
        return Ok(None);
    };
    let Some(run_id) = &change_set.run_id else {
        return Ok(None);
    };
    let artifacts = store.list_artifacts(run_id).await?;
    let Some(plan) = artifacts
        .iter()
        .find(|artifact| git_delivery_plan_matches_change_set(artifact, change_set))
    else {
        return Ok(None);
    };
    let latest_preflight = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "git_delivery_preflight"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content
                        .get("git_delivery_plan_artifact_id")
                        .and_then(Value::as_str)
                        == Some(plan.id.as_str())
                })
        })
        .max_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        })
        .cloned()
        .map(Into::into);
    let latest_execution = artifacts
        .iter()
        .filter(|artifact| {
            git_delivery_artifact_matches_plan(artifact, "git_delivery_execution", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    let latest_result = artifacts
        .iter()
        .filter(|artifact| {
            git_delivery_artifact_matches_plan(artifact, "git_delivery_result", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    let latest_observation = artifacts
        .iter()
        .filter(|artifact| {
            git_delivery_artifact_matches_plan(artifact, "git_delivery_pr_observation", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    let latest_merge = artifacts
        .iter()
        .filter(|artifact| {
            git_delivery_artifact_matches_plan(artifact, "git_delivery_merge", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);

    Ok(Some(GitDeliveryFlowResponse {
        plan: plan.clone().into(),
        latest_preflight,
        latest_execution,
        latest_result,
        latest_observation,
        latest_merge,
    }))
}

async fn gitops_delivery_flow(
    store: &SqliteStore,
    change_set: Option<&StoredGitOpsChangeSet>,
) -> Result<Option<GitOpsDeliveryFlowResponse>, ApiError> {
    let Some(change_set) = change_set else {
        return Ok(None);
    };
    let artifacts = store.list_artifacts(&change_set.run_id).await?;
    let Some(plan) = artifacts
        .iter()
        .find(|artifact| gitops_delivery_plan_matches_change_set(artifact, change_set))
    else {
        return Ok(None);
    };
    let base_revision_id = plan
        .content_json
        .as_ref()
        .and_then(|content| content.pointer("/source/base_revision_artifact_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ApiError::conflict("GitOps delivery plan has no base revision provenance")
        })?;
    let base_revision = artifacts
        .iter()
        .find(|artifact| {
            artifact.id == base_revision_id
                && gitops_base_revision_matches_change_set(artifact, change_set)
        })
        .cloned()
        .ok_or_else(|| {
            ApiError::conflict("GitOps delivery plan base revision is no longer current")
        })?;
    let latest_preflight = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "gitops_delivery_preflight"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content
                        .get("gitops_delivery_plan_artifact_id")
                        .and_then(Value::as_str)
                        == Some(plan.id.as_str())
                })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    let latest_execution = artifacts
        .iter()
        .filter(|artifact| {
            gitops_delivery_artifact_matches_plan(artifact, "gitops_delivery_execution", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    let latest_result = artifacts
        .iter()
        .filter(|artifact| {
            gitops_delivery_artifact_matches_plan(artifact, "gitops_delivery_result", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    let latest_observation = artifacts
        .iter()
        .filter(|artifact| {
            gitops_delivery_artifact_matches_plan(
                artifact,
                "gitops_delivery_pr_observation",
                &plan.id,
            )
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    let latest_merge = artifacts
        .iter()
        .filter(|artifact| {
            gitops_delivery_artifact_matches_plan(artifact, "gitops_delivery_merge", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    Ok(Some(GitOpsDeliveryFlowResponse {
        plan: plan.clone().into(),
        base_revision: base_revision.into(),
        latest_preflight,
        latest_execution,
        latest_result,
        latest_observation,
        latest_merge,
    }))
}

async fn deployment_intent_delivery_flow(
    store: &SqliteStore,
    intent: Option<&StoredDeploymentIntent>,
) -> Result<Option<DeploymentIntentDeliveryFlowResponse>, ApiError> {
    let Some(intent) = intent else {
        return Ok(None);
    };
    let release = store
        .get_release_by_deployment_intent(&intent.id)
        .await?
        .map(Into::into);
    let Some(run_id) = intent.run_id.as_ref() else {
        return Ok(Some(DeploymentIntentDeliveryFlowResponse {
            latest_execution: None,
            latest_result: None,
            release,
        }));
    };
    let artifacts = store.list_artifacts(run_id).await?;
    let latest_execution = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "argo_sync_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("deployment_intent_id").and_then(Value::as_str)
                        == Some(intent.id.as_str())
                })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id));
    let execution_id = latest_execution
        .and_then(|artifact| artifact.content_json.as_ref())
        .and_then(|content| content.get("execution_id"))
        .and_then(Value::as_str);
    let latest_result = execution_id.and_then(|execution_id| {
        artifacts
            .iter()
            .filter(|artifact| {
                artifact.kind == "argo_sync_result"
                    && artifact.content_json.as_ref().is_some_and(|content| {
                        content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                    })
            })
            .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
    });

    Ok(Some(DeploymentIntentDeliveryFlowResponse {
        latest_execution: latest_execution.cloned().map(Into::into),
        latest_result: latest_result.cloned().map(Into::into),
        release,
    }))
}

fn git_delivery_artifact_matches_plan(
    artifact: &StoredArtifact,
    kind: &str,
    plan_id: &str,
) -> bool {
    artifact.kind == kind
        && artifact.content_json.as_ref().is_some_and(|content| {
            content
                .get("git_delivery_plan_artifact_id")
                .and_then(Value::as_str)
                == Some(plan_id)
        })
}

async fn collect_sdlc_flow_incidents(
    store: &SqliteStore,
    root_incident_id: Option<&str>,
    release: Option<&StoredRelease>,
) -> Result<Vec<StoredIncident>, ApiError> {
    let mut incident_ids = BTreeSet::new();
    if let Some(root_incident_id) = root_incident_id {
        incident_ids.insert(root_incident_id.to_string());
    }

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
    if let Some(remediation_plan_id) = &work_plan.remediation_plan_id {
        plan_ids.insert(remediation_plan_id.clone());
    }
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
    work_plan: &StoredWorkPlan,
    remediation_plans: &[StoredRemediationPlan],
) -> Result<Vec<StoredApprovalGate>, ApiError> {
    let mut gates = Vec::new();
    let mut seen_gate_ids = BTreeSet::new();
    if let Some(work_item_id) = &work_plan.work_item_id {
        for gate in store
            .list_approval_gates(ApprovalGateListFilter {
                work_item_id: Some(work_item_id.clone()),
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
    gitops_change_set: Option<&StoredGitOpsChangeSet>,
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
    if let Some(gitops_change_set) = gitops_change_set {
        resources.push(("gitops_change_set", gitops_change_set.id.clone()));
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
    let gates = match &work_plan.remediation_plan_id {
        Some(remediation_plan_id) => readiness_gate_summary(store, remediation_plan_id).await?,
        None => SdlcReadinessGateSummary {
            pending: Vec::new(),
            stale: Vec::new(),
            rejected: Vec::new(),
        },
    };
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

#[derive(Debug, Default, serde::Deserialize)]
struct ListDeploymentContractsQuery {
    target_environment: Option<String>,
    target_namespace: Option<String>,
    argo_application: Option<String>,
    status: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}

async fn list_deployment_contracts(
    State(state): State<AppState>,
    Query(query): Query<ListDeploymentContractsQuery>,
) -> Result<Json<DeploymentContractsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let deployment_contracts = state
        .store
        .list_deployment_contracts(DeploymentContractListFilter {
            target_environment: clean_optional_text(query.target_environment),
            target_namespace: clean_optional_text(query.target_namespace),
            argo_application: clean_optional_text(query.argo_application),
            status: clean_optional_text(query.status),
            limit,
            offset,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let count = deployment_contracts.len();
    Ok(Json(DeploymentContractsResponse {
        deployment_contracts,
        count,
        limit,
        offset,
    }))
}

async fn get_deployment_contract(
    State(state): State<AppState>,
    Path(deployment_contract_id): Path<String>,
) -> Result<Json<DeploymentContractResponse>, ApiError> {
    let contract = state
        .store
        .get_deployment_contract(&deployment_contract_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_contract", &deployment_contract_id))?;
    Ok(Json(contract.into()))
}

async fn create_deployment_contract(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Json(request): Json<CreateDeploymentContractRequest>,
) -> Result<Json<DeploymentContractResponse>, ApiError> {
    let target_environment = required_text(request.target_environment, "target_environment")?;
    let target_namespace = required_text(request.target_namespace, "target_namespace")?;
    let argo_application = required_text(request.argo_application, "argo_application")?;
    let version = clean_optional_text(request.version).unwrap_or_else(|| "v1".to_string());
    validate_kubernetes_name("target_environment", &target_environment)?;
    validate_kubernetes_name("target_namespace", &target_namespace)?;
    validate_kubernetes_name("argo_application", &argo_application)?;
    validate_kubernetes_name("version", &version)?;
    let contract_spec = deployment_contract_spec(&request.contract_json)?;
    validate_deployment_contract_spec(&contract_spec)?;
    if target_environment == PROTECTED_ENVIRONMENT
        || target_namespace == PROTECTED_NAMESPACE
        || argo_application == PROTECTED_ARGO_APPLICATION
    {
        if target_environment != PROTECTED_ENVIRONMENT
            || target_namespace != PROTECTED_NAMESPACE
            || argo_application != PROTECTED_ARGO_APPLICATION
        {
            return Err(ApiError::bad_request(
                "production DeploymentContract target must exactly match production/apps-prod/yfinance-wrapper",
            ));
        }
        validate_protected_production_deployment_contract(&contract_spec)?;
    }
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    let contract = state
        .store
        .create_deployment_contract(CreateDeploymentContract {
            id: format!("dcontract_{}", unique_suffix()),
            status: "active".to_string(),
            target_environment,
            target_namespace,
            argo_application,
            version,
            contract_json: request.contract_json,
            actor: actor.clone(),
            reason: reason.clone(),
        })
        .await?;
    append_deployment_contract_audit_event(
        &state.store,
        &contract,
        "deployment_contract.created",
        actor,
        reason,
    )
    .await?;
    Ok(Json(contract.into()))
}

async fn transition_deployment_contract(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(deployment_contract_id): Path<String>,
    Json(request): Json<TransitionDeploymentContractRequest>,
) -> Result<Json<DeploymentContractResponse>, ApiError> {
    let current = state
        .store
        .get_deployment_contract(&deployment_contract_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_contract", &deployment_contract_id))?;
    let target = required_text(request.target_status, "target_status")?;
    if current.status != "active" || target != "retired" {
        return Err(ApiError::conflict(format!(
            "DeploymentContract can only transition from active to retired, not {} to {}",
            current.status, target
        )));
    }
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    let contract = state
        .store
        .update_deployment_contract_status(&current.id, "retired", actor.clone(), reason.clone())
        .await?;
    append_deployment_contract_audit_event(
        &state.store,
        &contract,
        "deployment_contract.retired",
        actor,
        reason,
    )
    .await?;
    Ok(Json(contract.into()))
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

    let source_provenance = work_item_pipeline_source_provenance(&state.store, &change_set).await?;
    let actor = clean_optional_text(actor);
    let reason = clean_optional_text(reason);
    let mut draft = pipeline_intent_draft(
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
    if let Some(source_provenance) = source_provenance {
        let object = draft
            .intent_json
            .as_object_mut()
            .ok_or_else(|| ApiError::internal("pipeline intent draft must have an object body"))?;
        object.insert("source_provenance".to_string(), source_provenance);
    }
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

async fn work_item_pipeline_source_provenance(
    store: &SqliteStore,
    change_set: &StoredChangeSet,
) -> Result<Option<Value>, ApiError> {
    if change_set.work_item_id.is_none() {
        return Ok(None);
    }
    let run_id = change_set.run_id.as_ref().ok_or_else(|| {
        ApiError::conflict("WorkItem PipelineIntent requires coding run provenance")
    })?;
    let artifacts = store.list_artifacts(run_id).await?;
    let plan = artifacts
        .iter()
        .find(|artifact| git_delivery_plan_matches_change_set(artifact, change_set))
        .ok_or_else(|| {
            ApiError::conflict(
                "WorkItem PipelineIntent requires the current Git delivery plan before build",
            )
        })?;
    let merge = artifacts
        .iter()
        .filter(|artifact| git_delivery_artifact_matches_plan(artifact, "git_delivery_merge", &plan.id))
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .ok_or_else(|| {
            ApiError::conflict(
                "WorkItem PipelineIntent requires observed GitHub merge evidence; a mutable PR branch is not a build source",
            )
        })?;
    let merge_content = merge
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git merge evidence has no structured provenance"))?;
    let merge_commit_sha =
        required_json_string(merge_content, "merge_commit_sha", "Git merge evidence")?;
    let head_commit_sha =
        required_json_string(merge_content, "head_commit_sha", "Git merge evidence")?;
    if !is_git_sha(&merge_commit_sha) || !is_git_sha(&head_commit_sha) {
        return Err(ApiError::conflict(
            "Git merge evidence has invalid commit provenance",
        ));
    }
    Ok(Some(json!({
        "kind": "github_merged_pull_request",
        "immutable": true,
        "git_delivery_plan_artifact_id": plan.id,
        "git_delivery_merge_artifact_id": merge.id,
        "repository": plan.content_json.as_ref().and_then(|value| value.pointer("/source/repository")).and_then(Value::as_str),
        "base_commit": plan.content_json.as_ref().and_then(|value| value.pointer("/source/base_commit")).and_then(Value::as_str),
        "head_commit_sha": head_commit_sha,
        "merge_commit_sha": merge_commit_sha,
        "pull_request_url": merge_content.get("pull_request_url"),
        "pull_request_number": merge_content.get("pull_request_number"),
    })))
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

const MAX_PIPELINE_EXECUTION_ATTEMPTS: u64 = 2;

async fn retry_failed_pipeline_intent(
    state: &AppState,
    pipeline_intent_id: &str,
    actor: String,
    reason: String,
) -> Result<PipelineIntentResponse, ApiError> {
    let current = state
        .store
        .get_pipeline_intent(pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", pipeline_intent_id))?;
    if current.status != "failed"
        || !matches!(
            pipeline_intent_execution_state(&current),
            Some("pipeline_run_failed" | "failed" | "dispatch_failed")
        )
        || current
            .intent_json
            .pointer("/execution_evidence/status")
            .and_then(Value::as_str)
            != Some("failed")
    {
        return Err(ApiError::conflict(
            "PipelineIntent retry requires durable terminal failure evidence",
        ));
    }
    let execution_attempt = pipeline_execution_attempt(&current.intent_json)?;
    if execution_attempt >= MAX_PIPELINE_EXECUTION_ATTEMPTS {
        return Err(ApiError::conflict(format!(
            "PipelineIntent has used all {MAX_PIPELINE_EXECUTION_ATTEMPTS} supervised execution attempts"
        )));
    }
    let change_set = state
        .store
        .get_change_set(&current.change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &current.change_set_id))?;
    let work_plan = state
        .store
        .get_work_plan(&current.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &current.work_plan_id))?;
    if change_set.status != "approved" || work_plan.status != "approved" {
        return Err(ApiError::conflict(
            "PipelineIntent retry requires the original approved WorkPlan and ChangeSet",
        ));
    }
    if state
        .store
        .get_deployment_intent_by_pipeline_intent(&current.id)
        .await?
        .is_some()
        || state
            .store
            .get_gitops_change_set_by_pipeline_intent(&current.id)
            .await?
            .is_some()
    {
        return Err(ApiError::conflict(
            "PipelineIntent retry is disabled after downstream delivery has started",
        ));
    }
    if change_set.work_item_id.is_some()
        && immutable_pipeline_source_revision(&current, true)?.is_none()
    {
        return Err(ApiError::conflict(
            "WorkItem PipelineIntent retry requires immutable source merge provenance",
        ));
    }

    let previous_execution_id = current
        .intent_json
        .pointer("/execution_state/execution_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("failed execution ID is unavailable"))?
        .to_string();
    let previous_pipeline_run_name = current
        .intent_json
        .pointer("/execution_state/pipeline_run_name")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("failed PipelineRun name is unavailable"))?
        .to_string();
    let failure_artifact_id = current
        .intent_json
        .pointer("/execution_evidence/artifact_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("failed execution artifact is unavailable"))?
        .to_string();
    let previous_permission_grant_id = current
        .intent_json
        .pointer("/execution_state/permission_grant_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let next_attempt = execution_attempt + 1;
    let mut intent_json = current.intent_json.clone();
    let history_entry = json!({
        "attempt": execution_attempt,
        "status": current.status,
        "execution_state": intent_json.get("execution_state"),
        "execution_evidence": intent_json.get("execution_evidence"),
        "pipeline_run_analysis": intent_json.get("evidence"),
        "build_output": intent_json.get("build_output"),
    });
    let object = intent_json
        .as_object_mut()
        .ok_or_else(|| ApiError::conflict("PipelineIntent body must be an object"))?;
    let history = object
        .entry("execution_history")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| ApiError::conflict("PipelineIntent execution_history must be an array"))?;
    history.push(history_entry);
    object.remove("execution_state");
    object.remove("execution_evidence");
    object.remove("evidence");
    object.remove("build_output");
    object.insert("execution_attempt".to_string(), json!(next_attempt));
    object.insert(
        "retry_context".to_string(),
        json!({
            "previous_attempt": execution_attempt,
            "previous_execution_id": previous_execution_id,
            "previous_pipeline_run_name": previous_pipeline_run_name,
            "failure_artifact_id": failure_artifact_id,
            "reproposed_at": current_millis(),
            "reproposed_by": actor,
            "reason": reason,
        }),
    );

    if let Some(grant_id) = previous_permission_grant_id.as_deref() {
        if let Some(grant) = state.store.get_permission_grant(grant_id).await? {
            if grant.status == "active" {
                let revoked = state
                    .store
                    .revoke_permission_grant(
                        grant_id,
                        Some(actor.clone()),
                        Some(format!(
                            "superseded by supervised PipelineIntent execution attempt {next_attempt}"
                        )),
                    )
                    .await?;
                append_permission_grant_audit_event(
                    &state.store,
                    "permission_grant.revoked",
                    &revoked,
                    Some(actor.clone()),
                )
                .await?;
            }
        }
    }

    let pipeline_intent = state
        .store
        .revise_pipeline_intent_draft(
            &current.id,
            UpdatePipelineIntentDraft {
                title: current.title.clone(),
                summary: current.summary.clone(),
                risk_level: current.risk_level.clone(),
                intent_kind: current.intent_kind.clone(),
                resource_namespace: current.resource_namespace.clone(),
                resource_kind: current.resource_kind.clone(),
                resource_name: current.resource_name.clone(),
                intent_json,
                actor: Some(actor.clone()),
                reason: Some(reason.clone()),
            },
        )
        .await?;
    append_pipeline_intent_audit_event(
        &state.store,
        &pipeline_intent,
        "pipeline_intent.retry_proposed",
        Some(actor),
        Some(reason),
        json!({
            "previous_attempt": execution_attempt,
            "execution_attempt": next_attempt,
            "previous_execution_id": previous_execution_id,
            "previous_pipeline_run_name": previous_pipeline_run_name,
            "failure_artifact_id": failure_artifact_id,
            "previous_permission_grant_id": previous_permission_grant_id,
            "automatic_execution": false,
        }),
    )
    .await?;

    Ok(pipeline_intent.into())
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
    let work_item = match work_plan.work_item_id.as_deref() {
        Some(work_item_id) => Some(
            state
                .store
                .get_work_item(work_item_id)
                .await?
                .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?,
        ),
        None => None,
    };
    if let Some(item) = work_item.as_ref() {
        if !work_item_target_supported(item) {
            return Err(ApiError::conflict(
                "Pipeline trusted envelope target is outside the supported dev or protected-production boundary",
            ));
        }
        if item.production_impacting != execution.production_impacting {
            return Err(ApiError::conflict(
                "Pipeline production impact must exactly match its WorkItem",
            ));
        }
    }
    let reason = clean_optional_text(Some(request.reason.clone()))
        .ok_or_else(|| ApiError::bad_request("trusted envelope reason is required"))?;
    let subject =
        clean_optional_text(request.subject).unwrap_or_else(|| state.policy.subject.clone());
    let environment = work_item
        .as_ref()
        .map(|item| item.target_environment.clone())
        .unwrap_or_else(|| state.policy.environment.clone());
    let expires_at = match work_item.as_ref() {
        Some(item) => bounded_production_grant_expiry(item, request.expires_at)?,
        None => request.expires_at,
    };
    let grant = create_permission_grant_record(
        &state.store,
        CreatePermissionGrantRequest {
            subject,
            created_by: clean_optional_text(request.created_by.clone()),
            reason: reason.clone(),
            scope: json!({
                "environment": environment,
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
            expires_at,
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
    let pipeline_analysis = match request.pipeline_run_analysis.as_ref() {
        Some(analysis) => {
            Some(persist_pipeline_run_analysis(&state.store, &intent, &request, analysis).await?)
        }
        None => None,
    };
    let build_output = match (
        request.status.as_str(),
        request.pipeline_run_analysis.as_ref(),
    ) {
        ("completed", Some(analysis)) => {
            persist_pipeline_build_output(&state.store, &intent, &request, analysis).await?
        }
        _ => None,
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
    if let Some(observation) = &pipeline_analysis {
        set_pipeline_intent_evidence(&mut intent_json, observation);
    }
    if let Some(output) = &build_output {
        set_pipeline_build_output(&mut intent_json, output);
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
            "analysis_observation_id": pipeline_analysis.as_ref().map(|observation| &observation.id),
            "analysis_artifact_id": pipeline_analysis
                .as_ref()
                .and_then(|observation| observation.artifact_id.as_ref()),
            "analysis_error": request.analysis_error,
            "build_output_artifact_id": build_output.as_ref().map(|artifact| &artifact.id),
        }),
    )
    .await?;
    if let Some(observation) = pipeline_analysis {
        append_pipeline_intent_audit_event(
            &state.store,
            &intent,
            "pipeline_intent.evidence_attached",
            Some("executor:tekton".to_string()),
            Some("attached terminal PipelineRunAnalysis".to_string()),
            json!({
                "observation_id": observation.id,
                "artifact_id": observation.artifact_id,
                "evidence_status": intent.intent_json.pointer("/evidence/status"),
                "resource": {
                    "namespace": observation.resource_namespace,
                    "kind": observation.resource_kind,
                    "name": observation.resource_name,
                },
            }),
        )
        .await?;
    } else if let Some(error) = request.analysis_error.as_deref() {
        append_pipeline_intent_audit_event(
            &state.store,
            &intent,
            "pipeline_intent.execution_analysis_failed",
            Some("executor:tekton".to_string()),
            Some(truncate_audit_text(error, 256)),
            json!({
                "execution_id": request.execution_id,
                "pipeline_run_namespace": request.pipeline_run_namespace,
                "pipeline_run_name": request.pipeline_run_name,
            }),
        )
        .await?;
    }
    if let Some(output) = build_output {
        append_pipeline_intent_audit_event(
            &state.store,
            &intent,
            "pipeline_intent.build_output_recorded",
            Some("executor:tekton".to_string()),
            Some("recorded terminal digest-pinned build output".to_string()),
            json!({
                "artifact_id": output.id,
                "status": output.content_json.as_ref().and_then(|content| content.get("status")),
                "image_ref": output.content_json.as_ref().and_then(|content| content.pointer("/image/reference")),
                "source_commit": output.content_json.as_ref().and_then(|content| content.pointer("/source/commit")),
            }),
        )
        .await?;
    }

    if request.status == "completed" {
        match create_declared_deployment_handoff(&state, &intent).await {
            Ok(Some(deployment_intent)) => {
                append_pipeline_intent_audit_event(
                    &state.store,
                    &intent,
                    "pipeline_intent.deployment_handoff_created",
                    Some("executor:tekton".to_string()),
                    Some(
                        "created proposed DeploymentIntent from terminal build evidence"
                            .to_string(),
                    ),
                    json!({
                        "deployment_intent_id": deployment_intent.id,
                        "target_environment": deployment_intent.target_environment,
                        "target_namespace": deployment_intent.target_namespace,
                        "argo_application": deployment_intent.argo_application,
                    }),
                )
                .await?;
            }
            Ok(None) => {}
            Err(error) => {
                append_pipeline_intent_audit_event(
                    &state.store,
                    &intent,
                    "pipeline_intent.deployment_handoff_failed",
                    Some("executor:tekton".to_string()),
                    Some(truncate_audit_text(&error.message, 256)),
                    json!({ "execution_id": request.execution_id }),
                )
                .await?;
            }
        }
    }
    Ok(Json(intent.into()))
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PipelineDeploymentHandoffSpec {
    target_environment: String,
    target_namespace: String,
    argo_application: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    risk_level: Option<String>,
}

async fn create_declared_deployment_handoff(
    state: &AppState,
    pipeline_intent: &StoredPipelineIntent,
) -> Result<Option<StoredDeploymentIntent>, ApiError> {
    let remediation_plan_id = pipeline_intent.remediation_plan_id.clone();
    let incident_id = pipeline_intent.incident_id.clone();
    let Some(raw_handoff) = pipeline_intent.intent_json.get("deployment_handoff") else {
        return Ok(None);
    };
    let handoff = serde_json::from_value::<PipelineDeploymentHandoffSpec>(raw_handoff.clone())
        .map_err(|error| {
            ApiError::bad_request(format!("pipeline deployment_handoff is invalid: {error}"))
        })?;
    validate_pipeline_deployment_handoff(&handoff)?;

    if pipeline_intent
        .intent_json
        .pointer("/evidence/status")
        .and_then(Value::as_str)
        != Some("satisfied")
    {
        return Err(ApiError::conflict(
            "pipeline deployment_handoff requires satisfied PipelineRunAnalysis evidence",
        ));
    }
    if state
        .store
        .get_deployment_intent_by_pipeline_intent(&pipeline_intent.id)
        .await?
        .is_some()
    {
        return Ok(None);
    }

    let title = clean_optional_text(handoff.title)
        .unwrap_or_else(|| format!("DeploymentIntent: {}", pipeline_intent.title));
    let summary = clean_optional_text(handoff.summary).unwrap_or_else(|| {
        format!(
            "Proposed Argo CD sync for {} after terminal PipelineRunAnalysis",
            handoff.argo_application
        )
    });
    let risk_level = clean_optional_text(handoff.risk_level)
        .unwrap_or_else(|| pipeline_intent.risk_level.clone());
    let intent_json = deployment_intent_json(
        pipeline_intent,
        "argo_sync_deploy",
        Some(&handoff.target_environment),
        Some(&handoff.target_namespace),
        Some(&handoff.argo_application),
        None,
    )?;
    let deployment_intent = state
        .store
        .create_deployment_intent(CreateDeploymentIntent {
            id: format!("dint_{}", unique_suffix()),
            pipeline_intent_id: pipeline_intent.id.clone(),
            change_set_id: pipeline_intent.change_set_id.clone(),
            work_plan_id: pipeline_intent.work_plan_id.clone(),
            remediation_plan_id,
            incident_id,
            session_id: pipeline_intent.session_id.clone(),
            run_id: pipeline_intent.run_id.clone(),
            status: "proposed".to_string(),
            title,
            summary,
            risk_level,
            intent_kind: "argo_sync_deploy".to_string(),
            target_environment: Some(handoff.target_environment),
            target_namespace: Some(handoff.target_namespace),
            argo_application: Some(handoff.argo_application),
            resource_namespace: pipeline_intent.resource_namespace.clone(),
            resource_kind: pipeline_intent.resource_kind.clone(),
            resource_name: pipeline_intent.resource_name.clone(),
            intent_json,
        })
        .await?;
    append_deployment_intent_audit_event(
        &state.store,
        &deployment_intent,
        "deployment_intent.auto_proposed",
        Some("executor:tekton".to_string()),
        Some("created from declared terminal PipelineIntent handoff".to_string()),
        json!({
            "source": "pipeline_intent.deployment_handoff",
            "pipeline_intent_id": pipeline_intent.id,
            "pipeline_evidence_status": pipeline_intent.intent_json.pointer("/evidence/status"),
            "execution_evidence": pipeline_intent.intent_json.get("execution_evidence"),
        }),
    )
    .await?;
    Ok(Some(deployment_intent))
}

fn validate_pipeline_deployment_handoff(
    handoff: &PipelineDeploymentHandoffSpec,
) -> Result<(), ApiError> {
    validate_kubernetes_name(
        "deployment_handoff.target_environment",
        &handoff.target_environment,
    )?;
    validate_kubernetes_name(
        "deployment_handoff.target_namespace",
        &handoff.target_namespace,
    )?;
    validate_kubernetes_name(
        "deployment_handoff.argo_application",
        &handoff.argo_application,
    )
}

/// Prepare a reviewable, digest-pinned Kustomize update. This is deliberately
/// a durable plan only: a later GitOps ChangeSet/PR executor must consume this
/// exact artifact rather than treating Argo sync as source provenance.
async fn create_gitops_update_plan(
    State(state): State<AppState>,
    Path(pipeline_intent_id): Path<String>,
    Json(request): Json<CreateGitOpsUpdatePlanRequest>,
) -> Result<Json<GitOpsUpdatePlanResponse>, ApiError> {
    let intent = state
        .store
        .get_pipeline_intent(&pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", &pipeline_intent_id))?;
    if !pipeline_intent_is_gitops_update_eligible(&intent) {
        return Err(ApiError::conflict(
            "GitOps update planning requires an eligible PipelineIntent with satisfied PipelineRunAnalysis evidence",
        ));
    }
    let work_plan = state
        .store
        .get_work_plan(&intent.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &intent.work_plan_id))?;
    let work_item_id = work_plan.work_item_id.as_deref().ok_or_else(|| {
        ApiError::conflict("GitOps update planning requires a WorkItem-backed PipelineIntent")
    })?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    if !work_item_target_supported(&work_item) {
        return Err(ApiError::conflict(
            "GitOps update planning is limited to dev or the exact protected production target",
        ));
    }
    let gitops_repo = work_item.gitops_repo.clone().ok_or_else(|| {
        ApiError::conflict("WorkItem must declare gitops_repo before GitOps update planning")
    })?;
    let gitops_ref = work_item.gitops_ref.clone().ok_or_else(|| {
        ApiError::conflict("WorkItem must declare gitops_ref before GitOps update planning")
    })?;
    let kustomization_path = required_text(request.kustomization_path, "kustomization_path")?;
    if !safe_relative_gitops_path(&kustomization_path) {
        return Err(ApiError::bad_request(
            "kustomization_path must be a safe relative repository path",
        ));
    }
    let image_name = required_text(request.image_name, "image_name")?;
    let deployment_intent = state
        .store
        .get_deployment_intent_by_pipeline_intent(&intent.id)
        .await?
        .ok_or_else(|| ApiError::conflict("PipelineIntent has no declared DeploymentIntent"))?;
    let run_id = intent.run_id.clone().ok_or_else(|| {
        ApiError::conflict("GitOps update planning requires pipeline run provenance")
    })?;
    let artifacts = state.store.list_artifacts(&run_id).await?;
    let build_output = current_pipeline_build_output(&artifacts, &intent)?;
    let requested_image_ref = clean_optional_text(request.image_ref);
    let image_ref = match (requested_image_ref, build_output.as_ref()) {
        (Some(requested), Some(output)) => {
            if requested != output.image_reference {
                return Err(ApiError::conflict(
                    "explicit GitOps image_ref does not match the verified PipelineRun build output",
                ));
            }
            requested
        }
        (Some(requested), None) => requested,
        (None, Some(output)) => output.image_reference.clone(),
        (None, None) => {
            return Err(ApiError::conflict(
                "GitOps image_ref is required until the PipelineRun records a verified digest-pinned build output",
            ))
        }
    };
    if !valid_digest_pinned_image_reference(&image_ref) {
        return Err(ApiError::bad_request(
            "GitOps image_ref must be a valid digest-pinned image with @sha256:<64 hex>",
        ));
    }
    let material_hash = format!(
        "{:x}",
        Sha256::digest(
            format!(
                "{}\n{}\n{}\n{}\n{}",
                gitops_repo, gitops_ref, kustomization_path, image_name, image_ref
            )
            .as_bytes()
        )
    );
    if let Some(existing) = artifacts.iter().find(|artifact| {
        artifact.kind == "gitops_update_plan"
            && artifact.content_json.as_ref().is_some_and(|content| {
                content.get("pipeline_intent_id").and_then(Value::as_str)
                    == Some(intent.id.as_str())
                    && content.get("material_hash").and_then(Value::as_str)
                        == Some(material_hash.as_str())
            })
    }) {
        return Ok(Json(GitOpsUpdatePlanResponse {
            artifact: existing.clone().into(),
            created: false,
        }));
    }
    let artifact = state.store.create_artifact(CreateArtifact {
        id: format!("art_{}_gitops_update_plan", unique_suffix()), session_id: intent.session_id.clone(), run_id: Some(run_id),
        kind: "gitops_update_plan".to_string(), label: format!("GitOps update plan for PipelineIntent {}", intent.id),
        mime_type: Some("application/json".to_string()), path: None, content_text: None,
        content_json: Some(json!({
            "kind": "gitops_update_plan", "version": 1, "operation": "kustomize_set_image", "material_hash": material_hash,
            "work_item_id": work_item.id, "work_plan_id": work_plan.id, "change_set_id": intent.change_set_id,
            "pipeline_intent_id": intent.id, "deployment_intent_id": deployment_intent.id,
            "gitops": { "repository": gitops_repo, "base_ref": gitops_ref, "head_branch": format!("pharness/gitops/{}/{}", safe_id_fragment(&work_item.id), safe_id_fragment(&intent.id)) },
            "build_output": build_output.as_ref().map(|output| json!({
                "artifact_id": output.artifact_id,
                "image_url": output.image_url,
                "image_digest": output.image_digest,
                "source_commit": output.source_commit,
            })),
            "update": { "kustomization_path": kustomization_path, "image_name": image_name, "new_image": image_ref },
            "execution": { "enabled": false, "reason": "requires a reviewed GitOps ChangeSet and dedicated GitOps writer" }
        })),
    }).await?;
    append_pipeline_intent_audit_event(&state.store, &intent, "pipeline_intent.gitops_update_planned", clean_optional_text(request.actor), clean_optional_text(request.reason), json!({ "artifact_id": artifact.id, "material_hash": material_hash, "deployment_intent_id": deployment_intent.id })).await?;
    Ok(Json(GitOpsUpdatePlanResponse {
        artifact: artifact.into(),
        created: true,
    }))
}

fn safe_relative_gitops_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        && path.len() <= 512
}

#[derive(Debug, Clone)]
struct VerifiedPipelineBuildOutput {
    artifact_id: String,
    image_url: String,
    image_digest: String,
    image_reference: String,
    source_commit: Option<String>,
}

fn current_pipeline_build_output(
    artifacts: &[StoredArtifact],
    intent: &StoredPipelineIntent,
) -> Result<Option<VerifiedPipelineBuildOutput>, ApiError> {
    let Some(artifact) = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "pipeline_build_output"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("pipeline_intent_id").and_then(Value::as_str)
                        == Some(intent.id.as_str())
                })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
    else {
        return Ok(None);
    };
    let content = artifact.content_json.as_ref().ok_or_else(|| {
        ApiError::conflict("Pipeline build-output artifact has no structured provenance")
    })?;
    if content.get("status").and_then(Value::as_str) != Some("verified") {
        return Err(ApiError::conflict(
            "Pipeline build-output provenance is not trusted for GitOps planning",
        ));
    }
    let image_url = content
        .pointer("/image/url")
        .and_then(Value::as_str)
        .filter(|value| safe_oci_image_component(value))
        .map(ToOwned::to_owned)
        .ok_or_else(|| ApiError::conflict("Pipeline build-output has no valid image URL"))?;
    let image_digest = content
        .pointer("/image/digest")
        .and_then(Value::as_str)
        .filter(|value| is_sha256_digest(value))
        .map(ToOwned::to_owned)
        .ok_or_else(|| ApiError::conflict("Pipeline build-output has invalid image digest"))?;
    let image_reference = content
        .pointer("/image/reference")
        .and_then(Value::as_str)
        .filter(|value| valid_digest_pinned_image_reference(value))
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            ApiError::conflict("Pipeline build-output has invalid digest-pinned image reference")
        })?;
    let source_commit = content
        .pointer("/source/commit")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .map(ToOwned::to_owned);
    Ok(Some(VerifiedPipelineBuildOutput {
        artifact_id: artifact.id.clone(),
        image_url,
        image_digest,
        image_reference,
        source_commit,
    }))
}

fn valid_digest_pinned_image_reference(value: &str) -> bool {
    match value.rsplit_once('@') {
        Some((repository, digest)) => {
            safe_oci_image_component(repository) && is_sha256_digest(digest)
        }
        None => false,
    }
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
    set_pipeline_intent_evidence(&mut intent_json, observation);

    intent_json
}

fn set_pipeline_intent_evidence(intent_json: &mut Value, observation: &StoredObservation) {
    let evidence = pipeline_intent_evidence_json(observation);
    if let Some(object) = intent_json.as_object_mut() {
        object.insert("evidence".to_string(), evidence);
    }
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
    #[serde(default)]
    source_revision_param: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DeploymentContractSpec {
    operation: String,
    #[serde(default)]
    prune: bool,
    #[serde(default)]
    force: bool,
    #[serde(default)]
    workload_kind: Option<String>,
    #[serde(default)]
    workload_name: Option<String>,
    #[serde(default)]
    service_name: Option<String>,
    #[serde(default)]
    service_port: Option<u16>,
    #[serde(default)]
    health_path: Option<String>,
    #[serde(default)]
    post_sync_verification: PostSyncVerificationContract,
}

/// Explicit, deliberately small runtime-verification policy attached to an
/// exact DeploymentContract. More observability sources can be added here as
/// independently reviewed contract fields; this is not an arbitrary query
/// escape hatch.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PostSyncVerificationContract {
    #[serde(default)]
    service_healthz: VerificationRequirement,
    #[serde(default)]
    prometheus_inventory: VerificationRequirement,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum VerificationRequirement {
    #[default]
    Disabled,
    Required,
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

fn pipeline_execution_preflight_response(
    preflight: PipelineIntentExecutionPreflight,
) -> PipelineIntentExecutionPreflightResponse {
    PipelineIntentExecutionPreflightResponse {
        ready: preflight.ready,
        manifest: preflight.manifest,
        checks: preflight.checks,
        permission_grant_id: preflight.grant_id,
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PipelineContractBinding {
    id: String,
    version: String,
    namespace: String,
    pipeline_ref: String,
}

fn pipeline_contract_binding(
    intent_json: &Value,
) -> Result<Option<PipelineContractBinding>, ApiError> {
    let Some(binding) = intent_json.get("pipeline_contract") else {
        return Ok(None);
    };
    serde_json::from_value(binding.clone())
        .map(Some)
        .map_err(|error| {
            ApiError::conflict(format!(
                "PipelineIntent has invalid pinned PipelineContract provenance: {error}"
            ))
        })
}

#[derive(Debug, Clone)]
struct DeploymentTarget {
    environment: String,
    namespace: String,
    application: String,
}

struct DeploymentIntentExecutionPreflight {
    ready: bool,
    intent: StoredDeploymentIntent,
    contract: Option<StoredDeploymentContract>,
    grant: Option<StoredPermissionGrant>,
    gitops_merge: Option<ArtifactResponse>,
    checks: Vec<Value>,
}

fn deployment_target(intent: &StoredDeploymentIntent) -> Result<DeploymentTarget, ApiError> {
    Ok(DeploymentTarget {
        environment: intent.target_environment.clone().ok_or_else(|| {
            ApiError::conflict("DeploymentIntent target_environment is required for Argo preflight")
        })?,
        namespace: intent.target_namespace.clone().ok_or_else(|| {
            ApiError::conflict("DeploymentIntent target_namespace is required for Argo preflight")
        })?,
        application: intent.argo_application.clone().ok_or_else(|| {
            ApiError::conflict("DeploymentIntent argo_application is required for Argo preflight")
        })?,
    })
}

fn ensure_supported_deployment_target(
    work_item: &StoredWorkItem,
    target: &DeploymentTarget,
) -> Result<(), ApiError> {
    if !work_item_target_supported(work_item) {
        return Err(ApiError::conflict(
            "Argo trusted envelopes require either a non-production dev WorkItem or the exact protected production target",
        ));
    }
    if target.environment != work_item.target_environment
        || work_item.target_namespace.as_deref() != Some(target.namespace.as_str())
        || work_item.argo_application.as_deref() != Some(target.application.as_str())
    {
        return Err(ApiError::conflict(
            "DeploymentIntent target must exactly match its WorkItem target",
        ));
    }
    Ok(())
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
    let work_item = match work_plan.work_item_id.as_deref() {
        Some(work_item_id) => Some(
            state
                .store
                .get_work_item(work_item_id)
                .await?
                .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?,
        ),
        None => None,
    };
    let execution = tekton_execution_spec(&intent.intent_json)?;
    let immutable_source_revision =
        immutable_pipeline_source_revision(&intent, change_set.work_item_id.is_some())?;
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

    let contract = match pipeline_contract_binding(&intent.intent_json)? {
        Some(binding) => match state.store.get_pipeline_contract(&binding.id).await? {
            None => {
                checks.push(execution_check(
                    "active_pipeline_contract",
                    false,
                    format!("Pinned PipelineContract {} no longer exists", binding.id),
                ));
                None
            }
            Some(contract)
                if contract.status != "active"
                    || contract.version != binding.version
                    || contract.namespace != binding.namespace
                    || contract.pipeline_ref != binding.pipeline_ref
                    || contract.namespace != execution.namespace
                    || contract.pipeline_ref != execution.pipeline_ref =>
            {
                checks.push(execution_check(
                    "active_pipeline_contract",
                    false,
                    format!(
                        "Pinned PipelineContract {} no longer matches its active execution contract",
                        binding.id
                    ),
                ));
                None
            }
            Some(contract) => {
                checks.push(execution_check(
                    "active_pipeline_contract",
                    true,
                    format!(
                        "Pinned active PipelineContract {} version {} matches",
                        contract.id, contract.version
                    ),
                ));
                Some(contract)
            }
        },
        None if change_set.work_item_id.is_some() => {
            checks.push(execution_check(
                "active_pipeline_contract",
                false,
                "WorkItem PipelineIntent requires an exact pinned PipelineContract before execution",
            ));
            None
        }
        None => {
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
            match contracts.as_slice() {
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
                    Some(contract.clone())
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
            }
        }
    };
    if let Some(contract) = contract.as_ref() {
        match execution_matches_pipeline_contract(
            &execution,
            contract,
            immutable_source_revision.as_deref(),
        ) {
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

    let gates = match (
        intent.remediation_plan_id.as_deref(),
        work_plan.work_item_id.as_deref(),
    ) {
        (Some(remediation_plan_id), _) => {
            state
                .store
                .list_approval_gates(ApprovalGateListFilter {
                    remediation_plan_id: Some(remediation_plan_id.to_string()),
                    limit: 200,
                    ..ApprovalGateListFilter::default()
                })
                .await?
        }
        (None, Some(work_item_id)) => {
            state
                .store
                .list_approval_gates(ApprovalGateListFilter {
                    work_item_id: Some(work_item_id.to_string()),
                    limit: 200,
                    ..ApprovalGateListFilter::default()
                })
                .await?
        }
        (None, None) => Vec::new(),
    };
    let required_kinds = if execution.production_impacting {
        ["pipeline_mutation", "production_impact"].as_slice()
    } else {
        ["pipeline_mutation"].as_slice()
    };
    for kind in required_kinds {
        let matching = gates
            .iter()
            .filter(|gate| {
                work_item.as_ref().map_or_else(
                    || gate.gate_kind == *kind,
                    |item| work_item_gate_scope_matches(gate, item, &work_plan, kind),
                )
            })
            .collect::<Vec<_>>();
        let satisfied = !matching.is_empty()
            && matching
                .iter()
                .all(|gate| matches!(gate.status.as_str(), "satisfied" | "waived"));
        checks.push(execution_check(
            format!("approval_gate_{kind}"),
            satisfied,
            if matching.is_empty() {
                if work_item.is_some() {
                    format!("Required scoped WorkItem {kind} approval gate is missing")
                } else {
                    format!("Required {kind} approval gate is missing")
                }
            } else {
                format!("{} {kind} gate(s) are satisfied or waived", matching.len())
            },
        ));
    }
    // WorkItem gates are phase-scoped. A pending GitOps gate must not block a
    // separately authorized Tekton build; it is evaluated at GitOps delivery.

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
    let now = current_millis();
    let work_plan = store
        .get_work_plan(&intent.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &intent.work_plan_id))?;
    let expected_environment = match work_plan.work_item_id.as_deref() {
        Some(work_item_id) => {
            store
                .get_work_item(work_item_id)
                .await?
                .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?
                .target_environment
        }
        None => policy.environment.clone(),
    };
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
            && scope.environment.as_deref() == Some(expected_environment.as_str())
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

async fn deployment_intent_execution_preflight(
    state: &AppState,
    deployment_intent_id: &str,
) -> Result<DeploymentIntentExecutionPreflight, ApiError> {
    let intent = state
        .store
        .get_deployment_intent(deployment_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_intent", deployment_intent_id))?;
    let pipeline_intent = state
        .store
        .get_pipeline_intent(&intent.pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", &intent.pipeline_intent_id))?;
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
    let work_item = match work_plan.work_item_id.as_deref() {
        Some(work_item_id) => Some(
            state
                .store
                .get_work_item(work_item_id)
                .await?
                .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?,
        ),
        None => None,
    };
    let target = deployment_target(&intent).ok();
    let pipeline_evidence = ensure_pipeline_evidence_ready_for_deployment(&pipeline_intent);
    let mut checks = vec![
        execution_check(
            "deployment_intent_approved",
            intent.status == "approved",
            format!("DeploymentIntent status is {}", intent.status),
        ),
        execution_check(
            "pipeline_intent_approved",
            pipeline_intent.status == "approved",
            format!("PipelineIntent status is {}", pipeline_intent.status),
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
            "pipeline_evidence_ready",
            pipeline_evidence.is_ok(),
            pipeline_evidence
                .err()
                .map(|error| error.message)
                .unwrap_or_else(|| {
                    "PipelineRun evidence is satisfied and matches the executed PipelineRun"
                        .to_string()
                }),
        ),
    ];

    let development_target = work_item
        .as_ref()
        .zip(target.as_ref())
        .and_then(|(item, target)| ensure_supported_deployment_target(item, target).err());
    checks.push(execution_check(
        "supported_work_item_target",
        work_item.is_some() && target.is_some() && development_target.is_none(),
        match (work_item.as_ref(), target.as_ref(), development_target) {
            (None, _, _) => "Argo preflight requires a WorkItem-backed delivery chain".to_string(),
            (_, None, _) => {
                "DeploymentIntent needs target environment, namespace, and Argo application"
                    .to_string()
            }
            (_, _, Some(error)) => error.message,
            _ => {
                "DeploymentIntent exactly matches a supported dev or protected-production WorkItem target".to_string()
            }
        },
    ));
    if let Some(item) = work_item.as_ref().filter(|item| item.production_impacting) {
        let rollback_ready = latest_rollback_intent(state, item, None)
            .await?
            .is_some_and(|intent| {
                matches!(
                    intent.pointer("/content/status").and_then(Value::as_str),
                    Some("prepared" | "approved")
                ) && intent
                    .pointer("/content/baseline/image_digest")
                    .and_then(Value::as_str)
                    .is_some_and(immutable_image_digest)
            });
        checks.push(execution_check(
            "production_baseline_and_rollback",
            rollback_ready,
            if rollback_ready {
                "Production baseline and digest-bound RollbackIntent are present".to_string()
            } else {
                "Production Argo execution requires a captured baseline and prepared RollbackIntent"
                    .to_string()
            },
        ));
    }

    let gitops_merge = match work_item.as_ref() {
        Some(work_item) => {
            match observed_gitops_merge_for_deployment(&state.store, work_item, &pipeline_intent)
                .await
            {
                Ok(Some(merge)) => {
                    let merge_sha = merge
                        .content_json
                        .as_ref()
                        .and_then(|content| content.get("merge_commit_sha"))
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    checks.push(execution_check(
                        "gitops_revision_merged",
                        true,
                        format!(
                            "GitOps merge artifact {} records immutable revision {}",
                            merge.id, merge_sha
                        ),
                    ));
                    Some(merge)
                }
                Ok(None) => {
                    checks.push(execution_check(
                    "gitops_revision_merged",
                    true,
                    "WorkItem does not declare a GitOps repository/ref; no GitOps merge is required",
                ));
                    None
                }
                Err(error) => {
                    checks.push(execution_check(
                        "gitops_revision_merged",
                        false,
                        error.message,
                    ));
                    None
                }
            }
        }
        None => {
            checks.push(execution_check(
                "gitops_revision_merged",
                false,
                "Argo preflight requires a WorkItem-backed delivery chain",
            ));
            None
        }
    };

    let contracts = if let Some(target) = target.as_ref() {
        state
            .store
            .list_deployment_contracts(DeploymentContractListFilter {
                target_environment: Some(target.environment.clone()),
                target_namespace: Some(target.namespace.clone()),
                argo_application: Some(target.application.clone()),
                status: Some("active".to_string()),
                limit: 10,
                ..DeploymentContractListFilter::default()
            })
            .await?
    } else {
        Vec::new()
    };
    let contract = match contracts.as_slice() {
        [contract] => match deployment_contract_spec(&contract.contract_json).and_then(|spec| {
            validate_deployment_contract_spec(&spec)?;
            if contract.target_environment == PROTECTED_ENVIRONMENT {
                validate_protected_production_deployment_contract(&spec)?;
            }
            Ok(())
        }) {
            Ok(()) => {
                checks.push(execution_check(
                    "active_deployment_contract",
                    true,
                    format!(
                        "Active DeploymentContract {} version {} exactly matches target",
                        contract.id, contract.version
                    ),
                ));
                Some(contract.clone())
            }
            Err(error) => {
                checks.push(execution_check(
                    "active_deployment_contract",
                    false,
                    error.message,
                ));
                None
            }
        },
        [] => {
            checks.push(execution_check(
                "active_deployment_contract",
                false,
                "No active DeploymentContract exactly matches the deployment target",
            ));
            None
        }
        _ => {
            checks.push(execution_check(
                "active_deployment_contract",
                false,
                "Multiple active DeploymentContracts match the target; retire the older contract",
            ));
            None
        }
    };

    let deployment_gate_kinds: &[&str] = if work_item
        .as_ref()
        .is_some_and(|item| item.production_impacting)
    {
        &[
            "cluster_mutation",
            "production_impact",
            "production_deployment",
        ]
    } else {
        &["cluster_mutation"]
    };
    for gate_kind in deployment_gate_kinds {
        let matching_gate = match work_item.as_ref() {
            Some(work_item) => state
                .store
                .list_approval_gates(ApprovalGateListFilter {
                    work_item_id: Some(work_item.id.clone()),
                    gate_kind: Some((*gate_kind).to_string()),
                    limit: 20,
                    ..ApprovalGateListFilter::default()
                })
                .await?
                .into_iter()
                .find(|gate| work_item_gate_scope_matches(gate, work_item, &work_plan, gate_kind)),
            None => None,
        };
        let approval_gate_ready = matching_gate
            .as_ref()
            .is_some_and(|gate| matches!(gate.status.as_str(), "satisfied" | "waived"));
        checks.push(execution_check(
            format!("approval_gate_{gate_kind}"),
            approval_gate_ready,
            matching_gate
                .as_ref()
                .map(|gate| format!("Scoped {gate_kind} gate {} is {}", gate.id, gate.status))
                .unwrap_or_else(|| format!("Required scoped WorkItem {gate_kind} gate is missing")),
        ));
    }

    let grant = match (target.as_ref(), work_item.as_ref()) {
        (Some(target), Some(work_item)) => {
            matching_deployment_execution_grant(
                &state.store,
                &intent,
                &work_plan,
                work_item,
                target,
            )
            .await?
        }
        _ => None,
    };
    checks.push(execution_check(
        "trusted_execution_envelope",
        grant.is_some(),
        grant
            .as_ref()
            .map(|grant| {
                format!(
                    "Active supervised-autonomy grant {} matches the DeploymentIntent",
                    grant.id
                )
            })
            .unwrap_or_else(|| {
                "No active supervised-autonomy grant matches this DeploymentIntent".to_string()
            }),
    ));

    let ready = checks
        .iter()
        .all(|check| check.get("passed").and_then(Value::as_bool) == Some(true));
    Ok(DeploymentIntentExecutionPreflight {
        ready,
        intent,
        contract,
        grant,
        gitops_merge,
        checks,
    })
}

/// Return immutable GitOps merge evidence when the WorkItem declares a GitOps
/// source of truth. A missing target intentionally stays compatible with the
/// existing non-GitOps dev delivery path; a partially declared or unmerged
/// target blocks Argo execution.
async fn observed_gitops_merge_for_deployment(
    store: &SqliteStore,
    work_item: &StoredWorkItem,
    pipeline_intent: &StoredPipelineIntent,
) -> Result<Option<ArtifactResponse>, ApiError> {
    let (gitops_repo, gitops_ref) = match (&work_item.gitops_repo, &work_item.gitops_ref) {
        (None, None) => return Ok(None),
        (Some(repository), Some(reference)) => (repository, reference),
        _ => {
            return Err(ApiError::conflict(
                "WorkItem must declare both gitops_repo and gitops_ref before Argo execution",
            ))
        }
    };
    let change_set = store
        .get_gitops_change_set_by_pipeline_intent(&pipeline_intent.id)
        .await?
        .ok_or_else(|| {
            ApiError::conflict(
                "Deployment requires a GitOps ChangeSet for the completed PipelineIntent",
            )
        })?;
    if change_set.status != "approved"
        || change_set.gitops_repo != *gitops_repo
        || change_set.gitops_ref != *gitops_ref
    {
        return Err(ApiError::conflict(
            "GitOps ChangeSet is not the current approved target declared by the WorkItem",
        ));
    }
    ensure_gitops_delivery_target(work_item, &change_set)?;
    let flow = gitops_delivery_flow(store, Some(&change_set)).await?;
    let merge = flow.and_then(|flow| flow.latest_merge).ok_or_else(|| {
        ApiError::conflict("Deployment requires an observed immutable GitOps pull-request merge")
    })?;
    if !merge
        .content_json
        .as_ref()
        .and_then(|content| content.get("merge_commit_sha"))
        .and_then(Value::as_str)
        .is_some_and(is_git_sha)
    {
        return Err(ApiError::conflict(
            "GitOps merge evidence has no valid immutable merge commit SHA",
        ));
    }
    Ok(Some(merge))
}

async fn matching_deployment_execution_grant(
    store: &SqliteStore,
    intent: &StoredDeploymentIntent,
    work_plan: &StoredWorkPlan,
    work_item: &StoredWorkItem,
    target: &DeploymentTarget,
) -> Result<Option<StoredPermissionGrant>, ApiError> {
    let now = current_millis();
    let production_binding = if work_item.production_impacting {
        let pipeline_intent = store
            .get_pipeline_intent(&intent.pipeline_intent_id)
            .await?
            .ok_or_else(|| ApiError::not_found("pipeline_intent", &intent.pipeline_intent_id))?;
        let source_merge_sha = pipeline_intent
            .intent_json
            .pointer("/source_provenance/merge_commit_sha")
            .and_then(Value::as_str)
            .filter(|value| is_git_sha(value))
            .ok_or_else(|| ApiError::conflict("source merge provenance is unavailable"))?
            .to_string();
        let image_digest = pipeline_intent
            .intent_json
            .pointer("/build_output/image_digest")
            .and_then(Value::as_str)
            .filter(|value| is_sha256_digest(value))
            .ok_or_else(|| ApiError::conflict("build image digest provenance is unavailable"))?
            .to_string();
        let gitops_merge = observed_gitops_merge_for_deployment(store, work_item, &pipeline_intent)
            .await?
            .ok_or_else(|| ApiError::conflict("GitOps merge provenance is unavailable"))?;
        let gitops_merge_sha = gitops_merge
            .content_json
            .as_ref()
            .and_then(|content| content.get("merge_commit_sha"))
            .and_then(Value::as_str)
            .filter(|value| is_git_sha(value))
            .ok_or_else(|| ApiError::conflict("GitOps merge provenance is malformed"))?
            .to_string();
        Some((source_merge_sha, gitops_merge_sha, image_digest))
    } else {
        None
    };
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
        let production_binding_matches = match production_binding.as_ref() {
            Some((source_merge_sha, gitops_merge_sha, image_digest)) => {
                scope.work_item_ids == [work_item.id.clone()]
                    && scope.pipeline_contract_ids
                        == work_item
                            .pipeline_contract_id
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                    && scope.deployment_contract_ids
                        == work_item
                            .deployment_contract_id
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                    && scope.source_merge_shas == [source_merge_sha.clone()]
                    && scope.gitops_merge_shas == [gitops_merge_sha.clone()]
                    && scope.image_digests == [image_digest.clone()]
            }
            None => true,
        };
        let matches = grant.subject == DEFAULT_ARGO_RUNNER_SUBJECT
            && scope.environment.as_deref() == Some(target.environment.as_str())
            && grant_policy.policy_mode == PolicyMode::SupervisedAutonomy
            && scope.capability_kinds.contains(&CapabilityKind::ArgoSync)
            && scope.actions.iter().any(|action| action == "argocd_sync")
            && scope
                .max_risk
                .is_some_and(|risk| risk_rank(risk) >= risk_rank(RiskLevel::High))
            && scope
                .namespaces
                .iter()
                .any(|namespace| namespace == &target.namespace)
            && scope.work_plan_ids.iter().any(|id| id == &work_plan.id)
            && scope
                .change_set_ids
                .iter()
                .any(|id| id == &intent.change_set_id)
            && scope
                .pipeline_intent_ids
                .iter()
                .any(|id| id == &intent.pipeline_intent_id)
            && scope
                .deployment_intent_ids
                .iter()
                .any(|id| id == &intent.id)
            && scope
                .argo_applications
                .iter()
                .any(|application| application == &target.application)
            && scope.production_impacting == Some(work_item.production_impacting)
            && production_binding_matches;
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

fn deployment_contract_spec(value: &Value) -> Result<DeploymentContractSpec, ApiError> {
    if !value.is_object() {
        return Err(ApiError::bad_request(
            "deployment contract contract_json must be a JSON object",
        ));
    }
    serde_json::from_value::<DeploymentContractSpec>(value.clone()).map_err(|error| {
        ApiError::bad_request(format!(
            "deployment contract contract_json is invalid: {error}"
        ))
    })
}

fn validate_deployment_contract_spec(contract: &DeploymentContractSpec) -> Result<(), ApiError> {
    if contract.operation != "sync" {
        return Err(ApiError::bad_request(
            "deployment contract operation must be sync",
        ));
    }
    if contract.prune || contract.force {
        return Err(ApiError::bad_request(
            "deployment contract prune and force must remain false",
        ));
    }
    Ok(())
}

fn validate_protected_production_deployment_contract(
    contract: &DeploymentContractSpec,
) -> Result<(), ApiError> {
    if contract.workload_kind.as_deref() != Some(PROTECTED_WORKLOAD_KIND)
        || contract.workload_name.as_deref() != Some(PROTECTED_WORKLOAD_NAME)
        || contract.service_name.as_deref() != Some(PROTECTED_WORKLOAD_NAME)
        || contract.service_port != Some(8090)
        || contract.health_path.as_deref() != Some("/healthz")
        || contract.post_sync_verification.service_healthz != VerificationRequirement::Required
    {
        return Err(ApiError::bad_request(
            "protected production DeploymentContract must pin Deployment/yfinance-wrapper and the exact yfinance-wrapper:8090/healthz check",
        ));
    }
    Ok(())
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
    if let Some(source_revision_param) = &contract.source_revision_param {
        validate_kubernetes_name(
            "pipeline contract source_revision_param",
            source_revision_param,
        )?;
        let parameter = contract
            .params
            .iter()
            .find(|parameter| parameter.name == *source_revision_param)
            .ok_or_else(|| {
                ApiError::bad_request(
                    "pipeline contract source_revision_param must name a declared parameter",
                )
            })?;
        if !parameter.required || parameter.value_type != "scalar" {
            return Err(ApiError::bad_request(
                "pipeline contract source_revision_param must name a required scalar parameter",
            ));
        }
    }
    Ok(())
}

fn execution_matches_pipeline_contract(
    execution: &TektonExecutionSpec,
    stored: &StoredPipelineContract,
    immutable_source_revision: Option<&str>,
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
    if let Some(immutable_source_revision) = immutable_source_revision {
        let source_revision_param = contract.source_revision_param.as_deref().ok_or_else(|| {
            ApiError::conflict(
                "WorkItem PipelineIntent requires an active PipelineContract with source_revision_param",
            )
        })?;
        if execution.params.get(source_revision_param) != Some(&json!(immutable_source_revision)) {
            return Err(ApiError::conflict(format!(
                "WorkItem PipelineIntent parameter {source_revision_param} must equal the observed merged commit"
            )));
        }
    }
    Ok(())
}

fn immutable_pipeline_source_revision(
    intent: &StoredPipelineIntent,
    work_item_delivery: bool,
) -> Result<Option<String>, ApiError> {
    if !work_item_delivery {
        return Ok(None);
    }
    let provenance = intent
        .intent_json
        .get("source_provenance")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApiError::conflict(
                "WorkItem PipelineIntent requires immutable Git merge provenance before execution",
            )
        })?;
    if provenance.get("kind").and_then(Value::as_str) != Some("github_merged_pull_request")
        || provenance.get("immutable").and_then(Value::as_bool) != Some(true)
    {
        return Err(ApiError::conflict(
            "WorkItem PipelineIntent source provenance must be an observed immutable GitHub merge",
        ));
    }
    let revision = required_json_string(provenance, "merge_commit_sha", "source provenance")?;
    if !is_git_sha(&revision) {
        return Err(ApiError::conflict(
            "WorkItem PipelineIntent source provenance has an invalid merge commit",
        ));
    }
    Ok(Some(revision))
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
    let execution_attempt = pipeline_execution_attempt(&intent.intent_json)?;
    let name = if execution_attempt == 1 {
        format!("pharness-{intent_label}")
    } else {
        format!("pharness-{intent_label}-{execution_attempt}")
    };
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
    let mut manifest = json!({
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
    });
    if let Some(merge_commit_sha) = intent
        .intent_json
        .pointer("/source_provenance/merge_commit_sha")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
    {
        manifest["metadata"]["annotations"] = json!({
            "pharness.lucas.engineering/source-commit": merge_commit_sha,
        });
    }
    Ok(manifest)
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

fn set_pipeline_build_output(intent_json: &mut Value, artifact: &ArtifactResponse) {
    let content = artifact.content_json.as_ref();
    if let Some(intent) = intent_json.as_object_mut() {
        intent.insert(
            "build_output".to_string(),
            json!({
                "artifact_id": artifact.id,
                "status": content.and_then(|value| value.get("status")),
                "image_ref": content.and_then(|value| value.pointer("/image/reference")),
                "image_digest": content.and_then(|value| value.pointer("/image/digest")),
                "source_commit": content.and_then(|value| value.pointer("/source/commit")),
            }),
        );
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

#[derive(Debug, Clone)]
struct PipelineBuildOutput {
    image_url: String,
    image_digest: String,
    image_reference: String,
    source_commit: Option<String>,
    status: &'static str,
    reason: Option<&'static str>,
}

/// Persist only compact, digest-pinned output that the terminal PipelineRun
/// reported. This is build provenance, not a registry inspection or a trust
/// assertion about signatures, SBOMs, or vulnerabilities.
async fn persist_pipeline_build_output(
    store: &SqliteStore,
    intent: &StoredPipelineIntent,
    outcome: &PipelineIntentExecutionOutcomeRequest,
    analysis: &Value,
) -> Result<Option<ArtifactResponse>, ApiError> {
    let Some(output) = pipeline_build_output_from_analysis(intent, analysis) else {
        return Ok(None);
    };
    let artifact_id = format!("art_pipeline_build_output_{}", outcome.execution_id);
    if let Some(existing) = store.get_artifact(&artifact_id).await? {
        return Ok(Some(existing.into()));
    }
    let artifact = store
        .create_artifact(CreateArtifact {
            id: artifact_id,
            session_id: intent.session_id.clone(),
            run_id: intent.run_id.clone(),
            kind: "pipeline_build_output".to_string(),
            label: format!("Digest-pinned build output for PipelineIntent {}", intent.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "version": 1,
                "status": output.status,
                "reason": output.reason,
                "pipeline_intent_id": intent.id,
                "execution_id": outcome.execution_id,
                "pipeline_run": {
                    "namespace": outcome.pipeline_run_namespace,
                    "name": outcome.pipeline_run_name,
                },
                "image": {
                    "url": output.image_url,
                    "digest": output.image_digest,
                    "reference": output.image_reference,
                },
                "source": {
                    "commit": output.source_commit,
                    "expected_merge_commit": intent.intent_json.pointer("/source_provenance/merge_commit_sha"),
                },
            })),
        })
        .await?;
    Ok(Some(artifact.into()))
}

fn pipeline_build_output_from_analysis(
    intent: &StoredPipelineIntent,
    analysis: &Value,
) -> Option<PipelineBuildOutput> {
    let image_url = analysis
        .pointer("/outputs/image_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| safe_oci_image_component(value))?
        .to_string();
    let image_digest = analysis
        .pointer("/outputs/image_digest")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| is_sha256_digest(value))?
        .to_string();
    let image_reference = match image_url.split_once('@') {
        Some((repository, embedded_digest))
            if safe_oci_image_component(repository) && embedded_digest == image_digest =>
        {
            format!("{repository}@{image_digest}")
        }
        Some(_) => return None,
        None => format!("{image_url}@{image_digest}"),
    };
    let source_commit = analysis
        .pointer("/outputs/commit")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| is_git_sha(value))
        .map(ToOwned::to_owned);
    let expected_merge = intent
        .intent_json
        .pointer("/source_provenance/merge_commit_sha")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value));
    let (status, reason) = match expected_merge {
        Some(expected) if source_commit.as_deref() == Some(expected) => ("verified", None),
        Some(_) => ("untrusted", Some("source_commit_mismatch")),
        None => ("verified", None),
    };
    Some(PipelineBuildOutput {
        image_url,
        image_digest,
        image_reference,
        source_commit,
        status,
        reason,
    })
}

fn safe_oci_image_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.contains(['\0', '\r', '\n', ' ', '\t'])
        && !value.contains("://")
}

fn is_sha256_digest(value: &str) -> bool {
    value.len() == "sha256:".len() + 64
        && value.starts_with("sha256:")
        && value["sha256:".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
}

async fn persist_pipeline_run_analysis(
    store: &SqliteStore,
    intent: &StoredPipelineIntent,
    outcome: &PipelineIntentExecutionOutcomeRequest,
    analysis: &Value,
) -> Result<StoredObservation, ApiError> {
    validate_terminal_pipeline_run_analysis(outcome, analysis)?;

    let artifact_id = format!("art_pipeline_analysis_{}", outcome.execution_id);
    let observation_id = format!("obs_pipeline_analysis_{}", outcome.execution_id);
    let namespace = outcome.pipeline_run_namespace.clone();
    let name = outcome.pipeline_run_name.clone();
    let content = json!({
        "source": "tekton",
        "resource": "pipeline_run_analysis",
        "namespace": namespace,
        "name": name,
        "analysis": analysis,
    });
    let artifact = match store.get_artifact(&artifact_id).await? {
        Some(existing) => existing,
        None => {
            store
                .create_artifact(CreateArtifact {
                    id: artifact_id,
                    session_id: intent.session_id.clone(),
                    run_id: intent.run_id.clone(),
                    kind: "pipeline_run_analysis".to_string(),
                    label: format!(
                        "PipelineRunAnalysis: {}",
                        name.as_deref().unwrap_or(&outcome.execution_id)
                    ),
                    mime_type: Some("application/json".to_string()),
                    path: None,
                    content_text: None,
                    content_json: Some(content),
                })
                .await?
        }
    };
    match store.get_observation(&observation_id).await? {
        Some(existing) => Ok(existing),
        None => Ok(store
            .create_observation(CreateObservation {
                id: observation_id,
                session_id: intent.session_id.clone(),
                run_id: intent.run_id.clone(),
                source: "tekton".to_string(),
                kind: "pipeline_run_analysis".to_string(),
                subject: name.clone().unwrap_or_else(|| outcome.execution_id.clone()),
                summary: format!(
                    "Terminal PipelineRunAnalysis for {}",
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
                artifact_id: Some(artifact.id),
                data_json: json!({ "analysis": analysis }),
            })
            .await?),
    }
}

fn validate_terminal_pipeline_run_analysis(
    outcome: &PipelineIntentExecutionOutcomeRequest,
    analysis: &Value,
) -> Result<(), ApiError> {
    if analysis.get("kind").and_then(Value::as_str) != Some("PipelineRunAnalysis") {
        return Err(ApiError::bad_request(
            "terminal execution analysis must be a PipelineRunAnalysis",
        ));
    }
    if let Some(namespace) = outcome.pipeline_run_namespace.as_deref() {
        if analysis
            .pointer("/pipeline_run/namespace")
            .and_then(Value::as_str)
            != Some(namespace)
        {
            return Err(ApiError::bad_request(
                "terminal execution analysis must match the PipelineRun namespace",
            ));
        }
    }
    if let Some(name) = outcome.pipeline_run_name.as_deref() {
        if analysis
            .pointer("/pipeline_run/name")
            .and_then(Value::as_str)
            != Some(name)
        {
            return Err(ApiError::bad_request(
                "terminal execution analysis must match the PipelineRun name",
            ));
        }
    }
    let observed_status = analysis.pointer("/summary/status").and_then(Value::as_str);
    let status_matches = match outcome.status.as_str() {
        "completed" => observed_status == Some("succeeded"),
        // Tekton reports a cancelled PipelineRun separately, but both terminal
        // states are an unsuccessful execution from the delivery controller's
        // perspective and must retain the same bounded failure path.
        "failed" => matches!(observed_status, Some("failed" | "cancelled")),
        _ => {
            return Err(ApiError::bad_request(
                "terminal execution analysis requires a completed or failed outcome",
            ))
        }
    };
    if !status_matches {
        return Err(ApiError::bad_request(
            "terminal execution analysis status must match the executor outcome",
        ));
    }

    Ok(())
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
    let remediation_plan_id = pipeline_intent.remediation_plan_id.clone();
    let incident_id = pipeline_intent.incident_id.clone();

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
            remediation_plan_id,
            incident_id,
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

async fn create_deployment_intent_trusted_envelope(
    State(state): State<AppState>,
    Path(deployment_intent_id): Path<String>,
    Json(request): Json<CreateDeploymentIntentTrustedEnvelopeRequest>,
) -> Result<Json<TrustedEnvelopeResponse>, ApiError> {
    let intent = state
        .store
        .get_deployment_intent(&deployment_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_intent", &deployment_intent_id))?;
    let pipeline_intent = state
        .store
        .get_pipeline_intent(&intent.pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", &intent.pipeline_intent_id))?;
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
    ensure_approved_for_trusted_envelope(
        "pipeline_intent",
        &pipeline_intent.id,
        &pipeline_intent.status,
    )?;
    ensure_approved_for_trusted_envelope("deployment_intent", &intent.id, &intent.status)?;

    let work_item_id = work_plan.work_item_id.as_deref().ok_or_else(|| {
        ApiError::conflict("Deployment trusted envelopes require a WorkItem-backed delivery chain")
    })?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let target = deployment_target(&intent)?;
    ensure_supported_deployment_target(&work_item, &target)?;

    let reason = clean_optional_text(Some(request.reason.clone()))
        .ok_or_else(|| ApiError::bad_request("trusted envelope reason is required"))?;
    let actor = clean_optional_text(request.created_by.clone());
    let subject = clean_optional_text(request.subject)
        .unwrap_or_else(|| DEFAULT_ARGO_RUNNER_SUBJECT.to_string());
    let expires_at = bounded_production_grant_expiry(&work_item, request.expires_at)?;
    let (source_merge_shas, gitops_merge_shas, image_digests) = if work_item.production_impacting {
        let source_merge_sha = pipeline_intent
            .intent_json
            .pointer("/source_provenance/merge_commit_sha")
            .and_then(Value::as_str)
            .filter(|value| is_git_sha(value))
            .ok_or_else(|| {
                ApiError::conflict(
                    "production deployment authorization requires immutable source merge provenance",
                )
            })?;
        let image_digest = pipeline_intent
            .intent_json
            .pointer("/build_output/image_digest")
            .and_then(Value::as_str)
            .filter(|value| is_sha256_digest(value))
            .ok_or_else(|| {
                ApiError::conflict(
                    "production deployment authorization requires a verified build image digest",
                )
            })?;
        let gitops_merge =
            observed_gitops_merge_for_deployment(&state.store, &work_item, &pipeline_intent)
                .await?
                .ok_or_else(|| {
                    ApiError::conflict(
                "production deployment authorization requires immutable GitOps merge provenance",
            )
                })?;
        let gitops_merge_sha = gitops_merge
            .content_json
            .as_ref()
            .and_then(|content| content.get("merge_commit_sha"))
            .and_then(Value::as_str)
            .filter(|value| is_git_sha(value))
            .ok_or_else(|| ApiError::conflict("GitOps merge provenance is malformed"))?;
        (
            vec![source_merge_sha.to_string()],
            vec![gitops_merge_sha.to_string()],
            vec![image_digest.to_string()],
        )
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };
    let grant = create_permission_grant_record(
        &state.store,
        CreatePermissionGrantRequest {
            subject,
            created_by: actor.clone(),
            reason: reason.clone(),
            scope: json!({
                "environment": target.environment,
                "capability_kinds": ["argo_sync"],
                "actions": ARGO_SYNC_ACTIONS,
                "max_risk": "high",
                "namespaces": [target.namespace],
                "work_item_ids": [work_item.id],
                "work_plan_ids": [work_plan.id],
                "change_set_ids": [change_set.id],
                "pipeline_intent_ids": [pipeline_intent.id],
                "deployment_intent_ids": [intent.id],
                "argo_applications": [target.application],
                "pipeline_contract_ids": work_item.pipeline_contract_id.iter().cloned().collect::<Vec<_>>(),
                "deployment_contract_ids": work_item.deployment_contract_id.iter().cloned().collect::<Vec<_>>(),
                "source_merge_shas": source_merge_shas,
                "gitops_merge_shas": gitops_merge_shas,
                "image_digests": image_digests,
                "production_impacting": work_item.production_impacting,
            }),
            policy: json!({ "policy_mode": "supervised_autonomy" }),
            expires_at,
        },
    )
    .await?;
    append_deployment_intent_audit_event(
        &state.store,
        &intent,
        "deployment_intent.trusted_envelope_created",
        actor,
        Some(reason),
        json!({
            "permission_grant_id": grant.id,
            "subject": grant.subject,
            "target": {
                "environment": target.environment,
                "namespace": target.namespace,
                "argo_application": target.application,
            },
        }),
    )
    .await?;

    Ok(Json(TrustedEnvelopeResponse {
        grant: grant.into(),
    }))
}

async fn preflight_deployment_intent(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(deployment_intent_id): Path<String>,
    Json(request): Json<DeploymentIntentPreflightRequest>,
) -> Result<Json<DeploymentIntentPreflightResponse>, ApiError> {
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    let preflight = deployment_intent_execution_preflight(&state, &deployment_intent_id).await?;
    let dispatch_ready = state.worker.argo_executor_available()
        && deployment_target(&preflight.intent)
            .ok()
            .is_some_and(|target| {
                state
                    .worker
                    .argo_executor_allows_application(&target.application)
            });
    append_deployment_intent_audit_event(
        &state.store,
        &preflight.intent,
        "deployment_intent.preflighted",
        actor,
        reason,
        json!({
            "ready_for_argo_runner": preflight.ready,
            "dispatch_ready": dispatch_ready,
            "deployment_contract_id": preflight.contract.as_ref().map(|contract| &contract.id),
            "permission_grant_id": preflight.grant.as_ref().map(|grant| &grant.id),
            "gitops_delivery_merge_artifact_id": preflight.gitops_merge.as_ref().map(|artifact| &artifact.id),
            "checks": preflight.checks,
        }),
    )
    .await?;

    Ok(Json(DeploymentIntentPreflightResponse {
        status: if preflight.ready {
            "ready_for_argo_runner"
        } else {
            "blocked"
        }
        .to_string(),
        ready_for_argo_runner: preflight.ready,
        dispatch_ready,
        deployment_intent: preflight.intent.into(),
        deployment_contract: preflight.contract.map(Into::into),
        permission_grant: preflight.grant.map(Into::into),
        checks: preflight.checks,
    }))
}

async fn execute_deployment_intent(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(deployment_intent_id): Path<String>,
    Json(request): Json<ExecuteDeploymentIntentRequest>,
) -> Result<Json<ExecuteDeploymentIntentResponse>, ApiError> {
    let actor = identity
        .as_ref()
        .map(|Extension(OperatorIdentity(name))| name.clone())
        .or_else(|| clean_optional_text(request.actor.clone()));
    let preflight = deployment_intent_execution_preflight(&state, &deployment_intent_id).await?;
    let target = deployment_target(&preflight.intent)?;
    let gitops_merge = preflight.gitops_merge.clone();
    let dispatch_ready = state.worker.argo_executor_available()
        && state
            .worker
            .argo_executor_allows_application(&target.application);
    let response_status = if preflight.ready && dispatch_ready {
        "ready"
    } else {
        "blocked"
    };
    if request.dry_run || !preflight.ready || !dispatch_ready {
        return Ok(Json(ExecuteDeploymentIntentResponse {
            status: response_status.to_string(),
            ready: preflight.ready && dispatch_ready,
            dry_run: request.dry_run,
            deployment_intent: preflight.intent.into(),
            deployment_contract: preflight.contract.map(Into::into),
            permission_grant: preflight.grant.map(Into::into),
            checks: preflight.checks,
            execution: None,
            execution_id: None,
            executor_job_name: None,
            created: false,
        }));
    }

    let reason = clean_optional_text(request.reason)
        .ok_or_else(|| ApiError::bad_request("Argo sync execution reason is required"))?;
    let intent = preflight.intent;
    let contract = preflight
        .contract
        .ok_or_else(|| ApiError::internal("ready Argo preflight omitted deployment contract"))?;
    let grant = preflight
        .grant
        .ok_or_else(|| ApiError::internal("ready Argo preflight omitted permission grant"))?;
    let run_id = intent
        .run_id
        .clone()
        .ok_or_else(|| ApiError::conflict("DeploymentIntent has no coding run provenance"))?;
    let artifacts = state.store.list_artifacts(&run_id).await?;
    if let Some(existing) = artifacts.iter().find(|artifact| {
        argo_sync_execution_matches(artifact, &intent, &contract, &grant, gitops_merge.as_ref())
    }) {
        let execution_id = existing
            .content_json
            .as_ref()
            .and_then(|value| value.get("execution_id"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let status = execution_id
            .as_deref()
            .and_then(|execution_id| {
                artifacts.iter().find_map(|artifact| {
                    (artifact.kind == "argo_sync_result")
                        .then_some(artifact.content_json.as_ref())
                        .flatten()
                        .filter(|content| {
                            content.get("execution_id").and_then(Value::as_str)
                                == Some(execution_id)
                        })
                        .and_then(|content| content.get("status").and_then(Value::as_str))
                })
            })
            .unwrap_or("dispatched")
            .to_string();
        return Ok(Json(ExecuteDeploymentIntentResponse {
            status,
            ready: true,
            dry_run: false,
            deployment_intent: intent.into(),
            deployment_contract: Some(contract.into()),
            permission_grant: Some(grant.into()),
            checks: preflight.checks,
            execution: Some(existing.clone().into()),
            execution_id,
            executor_job_name: None,
            created: false,
        }));
    }

    let execution_id = format!("aexec_{}", unique_suffix());
    let execution = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_argo_sync_execution", unique_suffix()),
            session_id: intent.session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "argo_sync_execution".to_string(),
            label: format!("Argo sync execution for DeploymentIntent {}", intent.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": execution_id,
                "status": "dispatched",
                "deployment_intent_id": intent.id,
                "deployment_contract_id": contract.id,
                "permission_grant_id": grant.id,
                "gitops_delivery_merge_artifact_id": gitops_merge.as_ref().map(|artifact| &artifact.id),
                "gitops_merge_commit_sha": gitops_merge.as_ref().and_then(|artifact| artifact.content_json.as_ref()).and_then(|content| content.get("merge_commit_sha")).and_then(Value::as_str),
                "target": {
                    "environment": target.environment,
                    "namespace": target.namespace,
                    "argo_application": target.application,
                },
                "dispatched_by": actor,
                "reason": reason,
            })),
        })
        .await?;

    match state
        .worker
        .dispatch_argo_sync_execution(ArgoSyncExecutionRequest {
            deployment_intent_id: intent.id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => {
            let dispatch = state
                .store
                .create_artifact(CreateArtifact {
                    id: format!("art_{}_argo_sync_dispatch", unique_suffix()),
                    session_id: intent.session_id.clone(),
                    run_id: Some(run_id),
                    kind: "argo_sync_dispatch".to_string(),
                    label: format!("Argo sync Job dispatch for DeploymentIntent {}", intent.id),
                    mime_type: Some("application/json".to_string()),
                    path: None,
                    content_text: None,
                    content_json: Some(json!({
                        "execution_id": execution_id,
                        "argo_sync_execution_artifact_id": execution.id,
                        "deployment_intent_id": intent.id,
                        "executor_job_name": receipt.job_name,
                    })),
                })
                .await?;
            append_deployment_intent_audit_event(
                &state.store,
                &intent,
                "deployment_intent.argo_sync_dispatched",
                actor,
                Some(reason),
                json!({
                    "execution_id": execution_id,
                    "execution_artifact_id": execution.id,
                    "dispatch_artifact_id": dispatch.id,
                    "executor_job_name": receipt.job_name,
                    "deployment_contract_id": contract.id,
                    "permission_grant_id": grant.id,
                    "gitops_delivery_merge_artifact_id": gitops_merge.as_ref().map(|artifact| &artifact.id),
                }),
            )
            .await?;
            Ok(Json(ExecuteDeploymentIntentResponse {
                status: "dispatched".to_string(),
                ready: true,
                dry_run: false,
                deployment_intent: intent.into(),
                deployment_contract: Some(contract.into()),
                permission_grant: Some(grant.into()),
                checks: preflight.checks,
                execution: Some(execution.into()),
                execution_id: Some(execution_id),
                executor_job_name: Some(receipt.job_name),
                created: true,
            }))
        }
        Err(error) => {
            tracing::warn!(deployment_intent_id = %intent.id, %error, "Argo executor dispatch failed");
            let result = persist_argo_sync_result(
                &state.store,
                &intent,
                &run_id,
                &execution_id,
                "dispatch_failed",
                json!({ "error_code": "job_dispatch_failed" }),
            )
            .await?;
            append_deployment_intent_audit_event(
                &state.store,
                &intent,
                "deployment_intent.argo_sync_dispatch_failed",
                None,
                None,
                json!({
                    "execution_id": execution_id,
                    "execution_artifact_id": execution.id,
                    "result_artifact_id": result.id,
                    "error_code": "job_dispatch_failed",
                    "gitops_delivery_merge_artifact_id": gitops_merge.as_ref().map(|artifact| &artifact.id),
                }),
            )
            .await?;
            Ok(Json(ExecuteDeploymentIntentResponse {
                status: "dispatch_failed".to_string(),
                ready: true,
                dry_run: false,
                deployment_intent: intent.into(),
                deployment_contract: Some(contract.into()),
                permission_grant: Some(grant.into()),
                checks: preflight.checks,
                execution: Some(execution.into()),
                execution_id: Some(execution_id),
                executor_job_name: None,
                created: true,
            }))
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct InternalArgoSyncQuery {
    execution_id: String,
}

async fn internal_argo_sync_context(
    State(state): State<AppState>,
    Path(deployment_intent_id): Path<String>,
    Query(query): Query<InternalArgoSyncQuery>,
) -> Result<Json<ArgoSyncContextResponse>, ApiError> {
    if deployment_intent_id.starts_with("rollback_") {
        return internal_rollback_argo_sync_context(
            &state,
            &deployment_intent_id,
            &query.execution_id,
        )
        .await;
    }
    let (intent, _run_id, execution) =
        current_argo_sync_execution(&state, &deployment_intent_id, &query.execution_id).await?;
    let preflight = deployment_intent_execution_preflight(&state, &intent.id).await?;
    let target = deployment_target(&preflight.intent)?;
    if !preflight.ready
        || !state
            .worker
            .argo_executor_allows_application(&target.application)
    {
        return Err(ApiError::conflict(
            "Argo sync context is no longer authorized or the executor is unavailable",
        ));
    }
    let content = execution
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Argo sync execution has no structured content"))?;
    if content
        .get("deployment_contract_id")
        .and_then(Value::as_str)
        != preflight
            .contract
            .as_ref()
            .map(|contract| contract.id.as_str())
        || content.get("permission_grant_id").and_then(Value::as_str)
            != preflight.grant.as_ref().map(|grant| grant.id.as_str())
    {
        return Err(ApiError::conflict(
            "Argo sync execution is stale relative to its contract or permission grant",
        ));
    }
    if content
        .get("gitops_delivery_merge_artifact_id")
        .and_then(Value::as_str)
        != preflight
            .gitops_merge
            .as_ref()
            .map(|artifact| artifact.id.as_str())
    {
        return Err(ApiError::conflict(
            "Argo sync execution is stale relative to its observed GitOps merge",
        ));
    }
    Ok(Json(ArgoSyncContextResponse {
        execution_id: query.execution_id,
        target_namespace: target.namespace,
        argo_application: target.application,
        revision: preflight.gitops_merge.as_ref().and_then(|artifact| {
            artifact
                .content_json
                .as_ref()
                .and_then(|content| content.get("merge_commit_sha"))
                .and_then(Value::as_str)
                .filter(|revision| is_git_sha(revision))
                .map(str::to_string)
        }),
        poll_seconds: argo_executor_poll_seconds(&state),
    }))
}

async fn internal_argo_sync_control(
    State(state): State<AppState>,
    Path(deployment_intent_id): Path<String>,
    Query(query): Query<InternalArgoSyncQuery>,
) -> Result<Json<ArgoSyncControlResponse>, ApiError> {
    if deployment_intent_id.starts_with("rollback_") {
        let (item, _current, _run) = rollback_intent_context(&state, &deployment_intent_id).await?;
        return Ok(Json(ArgoSyncControlResponse {
            cancelled: item.status == "cancelled",
        }));
    }
    let (intent, _run_id, _execution) =
        current_argo_sync_execution(&state, &deployment_intent_id, &query.execution_id).await?;
    let work_plan = state
        .store
        .get_work_plan(&intent.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::conflict("Argo sync WorkPlan is unavailable"))?;
    let cancelled = match work_plan.work_item_id.as_deref() {
        Some(work_item_id) => state
            .store
            .get_work_item(work_item_id)
            .await?
            .is_some_and(|work_item| work_item.status == "cancelled"),
        None => false,
    };
    Ok(Json(ArgoSyncControlResponse { cancelled }))
}

async fn internal_argo_sync_outcome(
    State(state): State<AppState>,
    Path(deployment_intent_id): Path<String>,
    Json(request): Json<ArgoSyncOutcomeRequest>,
) -> Result<Json<ArtifactResponse>, ApiError> {
    if deployment_intent_id.starts_with("rollback_") {
        return internal_rollback_argo_sync_outcome(&state, &deployment_intent_id, request).await;
    }
    let (intent, run_id, execution) =
        current_argo_sync_execution(&state, &deployment_intent_id, &request.execution_id).await?;
    let execution_content = execution
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Argo sync execution has no structured content"))?;
    let result = match request.status.as_str() {
        "submitted" => {
            persist_argo_sync_result(
                &state.store,
                &intent,
                &run_id,
                &request.execution_id,
                "submitted",
                json!({}),
            )
            .await?
        }
        "completed" => {
            let sync_status = clean_optional_text(request.sync_status).ok_or_else(|| {
                ApiError::bad_request("completed Argo outcome requires sync_status")
            })?;
            let operation_phase =
                clean_optional_text(request.operation_phase).ok_or_else(|| {
                    ApiError::bad_request("completed Argo outcome requires operation_phase")
                })?;
            if sync_status != "Synced" || operation_phase != "Succeeded" {
                return Err(ApiError::conflict(
                    "completed Argo outcome requires Synced status and Succeeded operation phase",
                ));
            }
            persist_argo_sync_result(
                &state.store,
                &intent,
                &run_id,
                &request.execution_id,
                "completed",
                json!({
                    "sync_status": sync_status,
                    "health_status": clean_optional_text(request.health_status),
                    "operation_phase": operation_phase,
                    "revision": clean_optional_text(request.revision),
                }),
            )
            .await?
        }
        "failed" | "cancelled" => {
            let fallback = if request.status == "cancelled" {
                "cancelled"
            } else {
                "argo_sync_failed"
            };
            persist_argo_sync_result(
                &state.store,
                &intent,
                &run_id,
                &request.execution_id,
                &request.status,
                json!({
                    "error_code": normalized_executor_error_code(request.error_code, fallback),
                    "sync_status": clean_optional_text(request.sync_status),
                    "health_status": clean_optional_text(request.health_status),
                    "operation_phase": clean_optional_text(request.operation_phase),
                    "revision": clean_optional_text(request.revision),
                }),
            )
            .await?
        }
        _ => {
            return Err(ApiError::bad_request(
                "Argo sync outcome status must be submitted, completed, failed, or cancelled",
            ))
        }
    };
    append_deployment_intent_audit_event(
        &state.store,
        &intent,
        &format!("deployment_intent.argo_sync_{}", request.status),
        Some(DEFAULT_ARGO_RUNNER_SUBJECT.to_string()),
        None,
        json!({
            "execution_id": request.execution_id,
            "execution_artifact_id": execution.id,
            "result_artifact_id": result.id,
            "deployment_contract_id": execution_content.get("deployment_contract_id"),
            "permission_grant_id": execution_content.get("permission_grant_id"),
        }),
    )
    .await?;
    Ok(Json(result))
}

async fn current_rollback_argo_sync_execution(
    state: &AppState,
    rollback_intent_id: &str,
    execution_id: &str,
) -> Result<
    (
        StoredWorkItem,
        Value,
        pharness_store::StoredRun,
        StoredArtifact,
    ),
    ApiError,
> {
    let (item, current, run) = rollback_intent_context(state, rollback_intent_id).await?;
    let artifacts = state.store.list_artifacts(&run.id).await?;
    let execution = artifacts
        .iter()
        .filter(|artifact| artifact.kind == "rollback_argo_sync_execution")
        .filter(|artifact| {
            artifact.content_json.as_ref().is_some_and(|content| {
                content.get("rollback_intent_id").and_then(Value::as_str)
                    == Some(rollback_intent_id)
                    && content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
            })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .ok_or_else(|| ApiError::conflict("rollback Argo sync execution is unavailable"))?;
    let latest = artifacts
        .iter()
        .filter(|artifact| artifact.kind == "rollback_argo_sync_execution")
        .filter(|artifact| {
            artifact.content_json.as_ref().is_some_and(|content| {
                content.get("rollback_intent_id").and_then(Value::as_str)
                    == Some(rollback_intent_id)
            })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id));
    if latest.map(|artifact| artifact.id.as_str()) != Some(execution.id.as_str()) {
        return Err(ApiError::conflict(
            "rollback Argo sync execution is no longer current",
        ));
    }
    Ok((item, current, run, execution))
}

async fn internal_rollback_argo_sync_context(
    state: &AppState,
    rollback_intent_id: &str,
    execution_id: &str,
) -> Result<Json<ArgoSyncContextResponse>, ApiError> {
    let (item, current, _run, execution) =
        current_rollback_argo_sync_execution(state, rollback_intent_id, execution_id).await?;
    let content = current
        .get("content")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("RollbackIntent content is unavailable"))?;
    validate_rollback_argo_grant(state, &item, rollback_intent_id, content).await?;
    if current.pointer("/content/status").and_then(Value::as_str) != Some("argo_syncing")
        || !state
            .worker
            .argo_executor_allows_application(PROTECTED_ARGO_APPLICATION)
    {
        return Err(ApiError::conflict(
            "rollback Argo sync is no longer authorized for the protected Application",
        ));
    }
    let revision = content
        .get("gitops_merge_sha")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .ok_or_else(|| ApiError::conflict("rollback GitOps merge SHA is unavailable"))?;
    let execution_content = execution
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("rollback Argo execution content is unavailable"))?;
    if execution_content
        .get("permission_grant_id")
        .and_then(Value::as_str)
        != content
            .get("argo_permission_grant_id")
            .and_then(Value::as_str)
        || execution_content
            .get("gitops_merge_sha")
            .and_then(Value::as_str)
            != Some(revision)
    {
        return Err(ApiError::conflict(
            "rollback Argo execution is stale relative to its grant or observed merge",
        ));
    }
    Ok(Json(ArgoSyncContextResponse {
        execution_id: execution_id.to_string(),
        target_namespace: PROTECTED_NAMESPACE.to_string(),
        argo_application: PROTECTED_ARGO_APPLICATION.to_string(),
        revision: Some(revision.to_string()),
        poll_seconds: argo_executor_poll_seconds(state),
    }))
}

async fn internal_rollback_argo_sync_outcome(
    state: &AppState,
    rollback_intent_id: &str,
    request: ArgoSyncOutcomeRequest,
) -> Result<Json<ArtifactResponse>, ApiError> {
    let (_item, current, run, _execution) =
        current_rollback_argo_sync_execution(state, rollback_intent_id, &request.execution_id)
            .await?;
    let expected_revision = current
        .pointer("/content/gitops_merge_sha")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .ok_or_else(|| ApiError::conflict("rollback GitOps merge SHA is unavailable"))?;
    let result = match request.status.as_str() {
        "submitted" => {
            append_rollback_argo_sync_result(
                state,
                &run,
                rollback_intent_id,
                &request.execution_id,
                "submitted",
                json!({}),
            )
            .await?
        }
        "completed" => {
            let sync_status = clean_optional_text(request.sync_status).ok_or_else(|| {
                ApiError::bad_request("completed Argo outcome requires sync_status")
            })?;
            let operation_phase =
                clean_optional_text(request.operation_phase).ok_or_else(|| {
                    ApiError::bad_request("completed Argo outcome requires operation_phase")
                })?;
            let revision = clean_optional_text(request.revision)
                .filter(|revision| revision == expected_revision)
                .ok_or_else(|| {
                    ApiError::conflict(
                        "rollback Argo outcome revision does not match the observed GitOps merge",
                    )
                })?;
            if sync_status != "Synced" || operation_phase != "Succeeded" {
                return Err(ApiError::conflict(
                    "completed rollback Argo outcome requires Synced and Succeeded",
                ));
            }
            append_rollback_argo_sync_result(
                state,
                &run,
                rollback_intent_id,
                &request.execution_id,
                "completed",
                json!({
                    "sync_status": sync_status,
                    "health_status": clean_optional_text(request.health_status),
                    "operation_phase": operation_phase,
                    "revision": revision,
                }),
            )
            .await?
        }
        "failed" | "cancelled" => append_rollback_argo_sync_result(
            state,
            &run,
            rollback_intent_id,
            &request.execution_id,
            &request.status,
            json!({
                "error_code": normalized_executor_error_code(
                    request.error_code,
                    if request.status == "cancelled" { "cancelled" } else { "argo_sync_failed" },
                ),
                "sync_status": clean_optional_text(request.sync_status),
                "health_status": clean_optional_text(request.health_status),
                "operation_phase": clean_optional_text(request.operation_phase),
                "revision": clean_optional_text(request.revision),
            }),
        )
        .await?,
        _ => {
            return Err(ApiError::bad_request(
                "rollback Argo outcome status must be submitted, completed, failed, or cancelled",
            ))
        }
    };
    if matches!(request.status.as_str(), "failed" | "cancelled") {
        let mut updated = current
            .get("content")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| ApiError::conflict("RollbackIntent content is unavailable"))?;
        updated.insert("status".to_string(), json!("attention_required"));
        updated.insert("argo_failure_result_id".to_string(), json!(result.id));
        append_rollback_intent_artifact(state, &run, &Value::Object(updated)).await?;
    }
    Ok(Json(result.into()))
}

fn argo_executor_poll_seconds(state: &AppState) -> u64 {
    state
        .worker
        .config_json()
        .pointer("/argo_executor/poll_seconds")
        .and_then(Value::as_u64)
        .filter(|seconds| *seconds > 0)
        .unwrap_or(5)
}

async fn current_argo_sync_execution(
    state: &AppState,
    deployment_intent_id: &str,
    execution_id: &str,
) -> Result<(StoredDeploymentIntent, RunId, StoredArtifact), ApiError> {
    let intent = state
        .store
        .get_deployment_intent(deployment_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_intent", deployment_intent_id))?;
    let run_id = intent.run_id.clone().ok_or_else(|| {
        ApiError::conflict("Argo sync DeploymentIntent has no coding run provenance")
    })?;
    let artifacts = state.store.list_artifacts(&run_id).await?;
    let execution = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "argo_sync_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("deployment_intent_id").and_then(Value::as_str)
                        == Some(intent.id.as_str())
                        && content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .ok_or_else(|| ApiError::conflict("Argo sync execution is unavailable"))?;
    let latest = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "argo_sync_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("deployment_intent_id").and_then(Value::as_str)
                        == Some(intent.id.as_str())
                })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id));
    if latest.map(|artifact| artifact.id.as_str()) != Some(execution.id.as_str()) {
        return Err(ApiError::conflict(
            "Argo sync execution is no longer current for this DeploymentIntent",
        ));
    }
    Ok((intent, run_id, execution))
}

fn argo_sync_execution_matches(
    artifact: &StoredArtifact,
    intent: &StoredDeploymentIntent,
    contract: &StoredDeploymentContract,
    grant: &StoredPermissionGrant,
    gitops_merge: Option<&ArtifactResponse>,
) -> bool {
    artifact.kind == "argo_sync_execution"
        && artifact.content_json.as_ref().is_some_and(|content| {
            content.get("deployment_intent_id").and_then(Value::as_str) == Some(intent.id.as_str())
                && content
                    .get("deployment_contract_id")
                    .and_then(Value::as_str)
                    == Some(contract.id.as_str())
                && content.get("permission_grant_id").and_then(Value::as_str)
                    == Some(grant.id.as_str())
                && content
                    .get("gitops_delivery_merge_artifact_id")
                    .and_then(Value::as_str)
                    == gitops_merge.map(|artifact| artifact.id.as_str())
        })
}

async fn persist_argo_sync_result(
    store: &SqliteStore,
    intent: &StoredDeploymentIntent,
    run_id: &RunId,
    execution_id: &str,
    status: &str,
    details: Value,
) -> Result<ArtifactResponse, ApiError> {
    if let Some(existing) = store
        .list_artifacts(run_id)
        .await?
        .into_iter()
        .find(|artifact| {
            artifact.kind == "argo_sync_result"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                        && content.get("status").and_then(Value::as_str) == Some(status)
                })
        })
    {
        return Ok(existing.into());
    }
    Ok(store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_argo_sync_result", unique_suffix()),
            session_id: intent.session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "argo_sync_result".to_string(),
            label: format!("Argo sync {} for DeploymentIntent {}", status, intent.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": execution_id,
                "status": status,
                "deployment_intent_id": intent.id,
                "details": details,
            })),
        })
        .await?
        .into())
}

fn normalized_executor_error_code(value: Option<String>, fallback: &str) -> String {
    let Some(value) = clean_optional_text(value) else {
        return fallback.to_string();
    };
    if value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        value
    } else {
        fallback.to_string()
    }
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
    let build_output =
        pipeline_build_output_for_deployment_intent(&state, &deployment_intent).await?;
    let requested_image_digest = clean_optional_text(request.image_digest);
    if let (Some(output), Some(requested)) = (&build_output, &requested_image_digest) {
        if requested != &output.image_digest {
            return Err(ApiError::conflict(format!(
                "Release image_digest must match verified Pipeline build output {}",
                output.image_digest
            )));
        }
    }
    let image_digest = requested_image_digest.or_else(|| {
        build_output
            .as_ref()
            .map(|output| output.image_digest.clone())
    });
    let rollback_ref = clean_optional_text(request.rollback_ref);
    let release_json = release_json(
        &deployment_intent,
        ReleaseJsonInput {
            release_kind: &release_kind,
            version: version.as_deref(),
            commit_sha: commit_sha.as_deref(),
            image_digest: image_digest.as_deref(),
            rollback_ref: rollback_ref.as_deref(),
            release_json: request.release_json,
            build_output: build_output.as_ref(),
        },
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
                    "pipeline_build_output_artifact_id": release
                        .release_json
                        .pointer("/build_output/artifact_id"),
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
            "pipeline_build_output_artifact_id": release
                .release_json
                .pointer("/build_output/artifact_id"),
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

/// Verifies the state that an Argo sync deliberately does not assert: the
/// Application must be synced and healthy, and the declared Deployment must
/// report a healthy rollout. This is a typed read-only path; it has no Argo
/// mutation or shell escape hatch.
async fn verify_release(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(release_id): Path<String>,
    Json(request): Json<VerifyReleaseRequest>,
) -> Result<Json<VerifyReleaseResponse>, ApiError> {
    let current = state
        .store
        .get_release(&release_id)
        .await?
        .ok_or_else(|| ApiError::not_found("release", &release_id))?;
    if !matches!(current.status.as_str(), "approved" | "completed") {
        return Err(ApiError::conflict(format!(
            "post-sync verification requires an approved or completed Release; {} is {}",
            current.id, current.status
        )));
    }
    let intent = state
        .store
        .get_deployment_intent(&current.deployment_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_intent", &current.deployment_intent_id))?;
    let work_plan = state
        .store
        .get_work_plan(&intent.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &intent.work_plan_id))?;
    let work_item_id = work_plan.work_item_id.as_deref().ok_or_else(|| {
        ApiError::conflict("post-sync verification requires a WorkItem-backed delivery chain")
    })?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let target = deployment_target(&intent)?;
    ensure_supported_deployment_target(&work_item, &target)?;
    let run_id = intent.run_id.clone().ok_or_else(|| {
        ApiError::conflict("post-sync verification requires DeploymentIntent coding run provenance")
    })?;
    let artifacts = state.store.list_artifacts(&run_id).await?;
    let (sync_execution, sync_result) = completed_argo_sync_result(&artifacts, &intent).ok_or_else(|| {
        ApiError::conflict(
            "post-sync verification requires the current Argo sync execution to have a completed result",
        )
    })?;
    let verification_contract =
        deployment_contract_for_sync_execution(&state.store, &target, sync_execution).await?;
    let prometheus_inventory_required = verification_contract
        .as_ref()
        .map(|contract| deployment_contract_spec(&contract.contract_json))
        .transpose()?
        .map(|contract| {
            contract.post_sync_verification.prometheus_inventory
                == VerificationRequirement::Required
        })
        .unwrap_or(false);

    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    if request.complete && reason.is_none() {
        return Err(ApiError::bad_request(
            "release completion requires an explicit verification reason",
        ));
    }

    let argo_action = AgentAction::ArgoGetApp {
        id: "release.verify_argo_application".into(),
        reason: format!("verify Release {} post-sync Argo state", current.id),
        app: target.application.clone(),
    };
    let argo_response = execute_direct_capability(&state, argo_action, request.timeout_ms).await?;
    let argo_observation_id = successful_direct_observation_id(&argo_response, "Argo Application")?;
    let argo_observation = state
        .store
        .get_observation(&argo_observation_id)
        .await?
        .ok_or_else(|| ApiError::internal("Argo verification observation was not persisted"))?;

    let workload_action =
        release_workload_verification_action(&intent, verification_contract.as_ref(), &current.id)?;
    let workload_response =
        execute_direct_capability(&state, workload_action, request.timeout_ms).await?;
    let workload_observation_id =
        successful_direct_observation_id(&workload_response, "Deployment rollout")?;
    let workload_observation = state
        .store
        .get_observation(&workload_observation_id)
        .await?
        .ok_or_else(|| ApiError::internal("workload verification observation was not persisted"))?;

    let argo_healthy = argo_observation
        .data_json
        .pointer("/analysis/sync_status")
        .and_then(Value::as_str)
        == Some("Synced")
        && argo_observation
            .data_json
            .pointer("/analysis/health_status")
            .and_then(Value::as_str)
            == Some("Healthy");
    let rollout_healthy = workload_observation
        .data_json
        .pointer("/analysis/status")
        .and_then(Value::as_str)
        == Some("healthy");
    let runtime_image_check = if work_item.production_impacting {
        let expected_digest = pipeline_build_output_for_deployment_intent(&state, &intent)
            .await?
            .map(|output| output.image_digest)
            .ok_or_else(|| {
                ApiError::conflict(
                    "production verification requires the verified Pipeline build digest",
                )
            })?;
        let response = execute_direct_capability(
            &state,
            AgentAction::KubernetesGet {
                id: "release.verify_running_image_ids".into(),
                reason: format!("verify Release {} running Pod imageIDs", current.id),
                resource: "pods".to_string(),
                namespace: Some(PROTECTED_NAMESPACE.to_string()),
                name: None,
                all_namespaces: false,
                label_selector: Some("app=yfinance-wrapper".to_string()),
            },
            request.timeout_ms,
        )
        .await?;
        let image_ids = response
            .result
            .as_ref()
            .and_then(|result| result.content.pointer("/output/items"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|pod| {
                pod.pointer("/status/containers")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
            })
            .filter_map(|container| container.get("imageID").and_then(Value::as_str))
            .collect::<Vec<_>>();
        execution_check(
            "running_image_digest",
            !image_ids.is_empty()
                && image_ids
                    .iter()
                    .all(|image_id| image_id.ends_with(&expected_digest)),
            format!(
                "{} running container imageID(s) checked against {}",
                image_ids.len(),
                expected_digest
            ),
        )
    } else {
        execution_check(
            "running_image_digest",
            true,
            "Exact Pod imageID verification is not required for legacy dev delivery",
        )
    };
    let service_health_check = if work_item.production_impacting {
        let outcome = state
            .worker
            .verify_capability("yfinance_healthz", None)
            .await;
        execution_check(
            "service_healthz",
            outcome.as_ref().is_ok_and(|outcome| outcome.available),
            if outcome.as_ref().is_ok_and(|outcome| outcome.available) {
                "Exact apps-prod/yfinance-wrapper Service /healthz check passed"
            } else {
                "Exact apps-prod/yfinance-wrapper Service /healthz check failed"
            },
        )
    } else {
        execution_check(
            "service_healthz",
            true,
            "Bounded Service /healthz verification is not required for legacy dev delivery",
        )
    };
    let (observability_observation, observability_check) = if prometheus_inventory_required {
        verify_required_prometheus_inventory(&state, request.timeout_ms).await?
    } else {
        (
            None,
            execution_check(
                "prometheus_inventory",
                true,
                "Prometheus inventory verification is disabled by the active DeploymentContract",
            ),
        )
    };
    let observability_healthy = observability_check
        .get("passed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let running_image_healthy = runtime_image_check
        .get("passed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let service_healthy = service_health_check
        .get("passed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let verified = argo_healthy
        && rollout_healthy
        && running_image_healthy
        && service_healthy
        && observability_healthy;
    let mut checks = vec![
        execution_check(
            "completed_argo_sync",
            true,
            format!(
                "completed sync result artifact {} is current",
                sync_result.id
            ),
        ),
        execution_check(
            "argo_application_synced_healthy",
            argo_healthy,
            verification_observation_summary(&argo_observation),
        ),
        execution_check(
            "declared_deployment_rollout_healthy",
            rollout_healthy,
            verification_observation_summary(&workload_observation),
        ),
    ];
    checks.push(runtime_image_check);
    checks.push(service_health_check);
    checks.push(observability_check);

    let release_json = release_json_with_post_sync_verification(
        &current,
        PostSyncVerificationEvidence {
            sync_result,
            argo_observation: &argo_observation,
            workload_observation: &workload_observation,
            deployment_contract: verification_contract.as_ref(),
            observability_observation: observability_observation.as_ref(),
            prometheus_inventory_required,
            verified,
            checks: &checks,
        },
    );
    let mut release = state
        .store
        .update_release_evidence(
            &current.id,
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
        if verified {
            "release.post_sync_verified"
        } else {
            "release.post_sync_attention_required"
        },
        actor.clone(),
        reason.clone(),
        json!({
            "argo_sync_result_artifact_id": sync_result.id,
            "argo_observation_id": argo_observation.id,
            "workload_observation_id": workload_observation.id,
            "deployment_contract_id": verification_contract.as_ref().map(|contract| &contract.id),
            "observability_observation_id": observability_observation.as_ref().map(|observation| &observation.id),
            "checks": checks,
        }),
    )
    .await?;

    let mut completed = false;
    if request.complete && verified && release.status == "approved" {
        release = state
            .store
            .update_release_status(&release.id, "completed", actor.clone(), reason.clone())
            .await?;
        append_release_audit_event(
            &state.store,
            &release,
            "release.completed",
            actor,
            reason,
            json!({
                "verification": "post_sync",
                "argo_observation_id": argo_observation.id,
                "workload_observation_id": workload_observation.id,
                "observability_observation_id": observability_observation.as_ref().map(|observation| &observation.id),
            }),
        )
        .await?;
        completed = true;
    }

    Ok(Json(VerifyReleaseResponse {
        status: if verified {
            "verified".to_string()
        } else {
            "attention_required".to_string()
        },
        verified,
        completed,
        release: release.into(),
        argo_observation: argo_observation.into(),
        workload_observation: workload_observation.into(),
        observability_observation: observability_observation.map(Into::into),
        checks,
    }))
}

async fn deployment_contract_for_sync_execution(
    store: &SqliteStore,
    target: &DeploymentTarget,
    execution: &StoredArtifact,
) -> Result<Option<StoredDeploymentContract>, ApiError> {
    let contract_id = execution
        .content_json
        .as_ref()
        .and_then(|content| content.get("deployment_contract_id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let Some(contract_id) = contract_id else {
        // Legacy receipt: no contract-backed runtime criterion is available to
        // adopt after a sync. It may still use the original rollout checks.
        return Ok(None);
    };
    let contract = store
        .get_deployment_contract(&contract_id)
        .await?
        .ok_or_else(|| {
            ApiError::conflict("Argo sync execution references a missing DeploymentContract")
        })?;
    if contract.status != "active" {
        return Err(ApiError::conflict(
            "Argo sync execution DeploymentContract is no longer active; run a new reviewed sync",
        ));
    }
    if contract.target_environment != target.environment
        || contract.target_namespace != target.namespace
        || contract.argo_application != target.application
    {
        return Err(ApiError::conflict(
            "Argo sync execution DeploymentContract does not match the Release target",
        ));
    }
    let spec = deployment_contract_spec(&contract.contract_json)?;
    validate_deployment_contract_spec(&spec)?;
    if contract.target_environment == PROTECTED_ENVIRONMENT {
        validate_protected_production_deployment_contract(&spec)?;
    }
    Ok(Some(contract))
}

async fn verify_required_prometheus_inventory(
    state: &AppState,
    timeout_ms: Option<u64>,
) -> Result<(Option<StoredObservation>, Value), ApiError> {
    let response = execute_direct_capability(
        state,
        AgentAction::PrometheusInventory {
            id: "release.verify_prometheus_inventory".into(),
            reason: "verify Release post-sync Prometheus inventory".to_string(),
        },
        timeout_ms,
    )
    .await?;
    if response.status != "ok" || !response.executed {
        return Ok((
            None,
            execution_check(
                "prometheus_inventory",
                false,
                format!(
                    "required Prometheus inventory was unavailable: {}",
                    response.error.unwrap_or(response.status)
                ),
            ),
        ));
    }
    let Some(observation_id) = response.observation_id else {
        return Ok((
            None,
            execution_check(
                "prometheus_inventory",
                false,
                "required Prometheus inventory did not persist an observation",
            ),
        ));
    };
    let observation = state
        .store
        .get_observation(&observation_id)
        .await?
        .ok_or_else(|| {
            ApiError::internal("Prometheus verification observation was not persisted")
        })?;
    let healthy = release_prometheus_inventory_collected(&observation.data_json);
    Ok((
        Some(observation.clone()),
        execution_check(
            "prometheus_inventory",
            healthy,
            release_prometheus_inventory_summary(&observation.data_json),
        ),
    ))
}

fn release_prometheus_inventory_collected(data: &Value) -> bool {
    ["targets", "rules", "alerts"].into_iter().all(|section| {
        data.pointer(&format!("/inventory/{section}/status"))
            .and_then(Value::as_str)
            == Some("success")
    })
}

fn release_prometheus_inventory_summary(data: &Value) -> String {
    let unhealthy_targets = data
        .pointer("/inventory/targets/unhealthy_count")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let problem_rules = data
        .pointer("/inventory/rules/problem_rule_count")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let alerts = data
        .pointer("/inventory/alerts/alert_count")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    format!(
        "Prometheus inventory collected; recorded {unhealthy_targets} unhealthy target(s), {problem_rules} problem rule(s), and {alerts} alert(s) as non-workload-scoped evidence"
    )
}

fn successful_direct_observation_id(
    response: &ExecuteCapabilityResponse,
    description: &str,
) -> Result<String, ApiError> {
    if response.status != "ok" || !response.executed {
        return Err(ApiError::conflict(format!(
            "{description} verification failed: {}",
            response
                .error
                .as_deref()
                .unwrap_or(response.status.as_str())
        )));
    }
    response.observation_id.clone().ok_or_else(|| {
        ApiError::internal(format!(
            "{description} verification did not produce an observation"
        ))
    })
}

fn release_workload_verification_action(
    intent: &StoredDeploymentIntent,
    deployment_contract: Option<&StoredDeploymentContract>,
    release_id: &str,
) -> Result<AgentAction, ApiError> {
    let (resource_kind, namespace, name) = if let Some(contract) = deployment_contract {
        let spec = deployment_contract_spec(&contract.contract_json)?;
        match (spec.workload_kind, spec.workload_name) {
            (Some(kind), Some(name)) => (kind, contract.target_namespace.clone(), name),
            (None, None) => release_intent_workload_target(intent)?,
            _ => {
                return Err(ApiError::conflict(
                    "DeploymentContract post-sync verification must declare both workload_kind and workload_name",
                ))
            }
        }
    } else {
        release_intent_workload_target(intent)?
    };
    let resource_kind = resource_kind.trim().to_ascii_lowercase();
    if !matches!(resource_kind.as_str(), "deployment" | "deployments") {
        return Err(ApiError::conflict(
            "post-sync verification currently supports only a declared Deployment resource",
        ));
    }
    Ok(AgentAction::KubernetesGet {
        id: "release.verify_deployment".into(),
        reason: format!("verify Release {release_id} declared Deployment rollout"),
        resource: "deployments".to_string(),
        namespace: Some(namespace),
        name: Some(name),
        all_namespaces: false,
        label_selector: None,
    })
}

fn release_intent_workload_target(
    intent: &StoredDeploymentIntent,
) -> Result<(String, String, String), ApiError> {
    let resource_kind = intent.resource_kind.clone().ok_or_else(|| {
        ApiError::conflict(
            "post-sync verification currently supports only a declared Deployment resource",
        )
    })?;
    let namespace = intent.resource_namespace.clone().ok_or_else(|| {
        ApiError::conflict("post-sync verification requires a declared Deployment namespace")
    })?;
    let name = intent.resource_name.clone().ok_or_else(|| {
        ApiError::conflict("post-sync verification requires a declared Deployment name")
    })?;
    Ok((resource_kind, namespace, name))
}

fn completed_argo_sync_result<'a>(
    artifacts: &'a [StoredArtifact],
    intent: &StoredDeploymentIntent,
) -> Option<(&'a StoredArtifact, &'a StoredArtifact)> {
    let execution = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "argo_sync_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("deployment_intent_id").and_then(Value::as_str)
                        == Some(intent.id.as_str())
                })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))?;
    let execution_id = execution
        .content_json
        .as_ref()?
        .get("execution_id")
        .and_then(Value::as_str)?;
    let result = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "argo_sync_result"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                        && content.get("status").and_then(Value::as_str) == Some("completed")
                })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))?;
    Some((execution, result))
}

struct PostSyncVerificationEvidence<'a> {
    sync_result: &'a StoredArtifact,
    argo_observation: &'a StoredObservation,
    workload_observation: &'a StoredObservation,
    deployment_contract: Option<&'a StoredDeploymentContract>,
    observability_observation: Option<&'a StoredObservation>,
    prometheus_inventory_required: bool,
    verified: bool,
    checks: &'a [Value],
}

fn release_json_with_post_sync_verification(
    current: &StoredRelease,
    evidence: PostSyncVerificationEvidence<'_>,
) -> Value {
    let mut release_json = current.release_json.clone();
    let verification = json!({
        "status": if evidence.verified { "verified" } else { "attention_required" },
        "runtime_ready": evidence.verified,
        "review_required": !evidence.verified,
        "argo_sync_result_artifact_id": evidence.sync_result.id,
        "argo_observation_id": evidence.argo_observation.id,
        "workload_observation_id": evidence.workload_observation.id,
        "deployment_contract_id": evidence.deployment_contract.map(|contract| contract.id.clone()),
        "deployment_contract_version": evidence.deployment_contract.map(|contract| contract.version.clone()),
        "observability": {
            "prometheus_inventory": {
                "required": evidence.prometheus_inventory_required,
                "status": if !evidence.prometheus_inventory_required {
                    "disabled"
                } else if evidence.observability_observation
                    .map(|observation| release_prometheus_inventory_collected(&observation.data_json))
                    .unwrap_or(false)
                {
                    "observed"
                } else {
                    "attention_required"
                },
                "observation_id": evidence.observability_observation.map(|observation| observation.id.clone()),
            }
        },
        "checks": evidence.checks,
    });
    if let Some(object) = release_json.as_object_mut() {
        object.insert("post_sync_verification".to_string(), verification);
    }
    release_json
}

fn verification_observation_summary(observation: &StoredObservation) -> String {
    observation.summary.chars().take(256).collect::<String>()
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
                work_item_id: None,
                remediation_plan_id: Some(plan.id.clone()),
                incident_id: Some(plan.incident_id.clone()),
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

struct ReleaseJsonInput<'a> {
    release_kind: &'a str,
    version: Option<&'a str>,
    commit_sha: Option<&'a str>,
    image_digest: Option<&'a str>,
    rollback_ref: Option<&'a str>,
    release_json: Option<serde_json::Value>,
    build_output: Option<&'a VerifiedPipelineBuildOutput>,
}

fn release_json(
    deployment_intent: &StoredDeploymentIntent,
    input: ReleaseJsonInput<'_>,
) -> Result<serde_json::Value, ApiError> {
    let mut release_json = if let Some(release_json) = input.release_json {
        if !release_json.is_object() {
            return Err(ApiError::bad_request(
                "release release_json must be a JSON object",
            ));
        }
        release_json
    } else {
        json!({
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
                "release_kind": input.release_kind,
                "target_environment": deployment_intent.target_environment,
                "target_namespace": deployment_intent.target_namespace,
                "argo_application": deployment_intent.argo_application,
                "version": input.version,
                "commit_sha": input.commit_sha,
                "image_digest": input.image_digest,
                "rollback_ref": input.rollback_ref,
            },
            "verification": {
                "required": ["argo_health", "lgtm_signals", "audit_event"]
            }
        })
    };
    if let Some(build_output) = input.build_output {
        release_json
            .as_object_mut()
            .expect("release_json is validated as an object")
            .insert(
                "build_output".to_string(),
                json!({
                    "status": "verified",
                    "artifact_id": build_output.artifact_id,
                    "image_url": build_output.image_url,
                    "image_digest": build_output.image_digest,
                    "image_reference": build_output.image_reference,
                    "source_commit": build_output.source_commit,
                }),
            );
    }
    Ok(release_json)
}

async fn pipeline_build_output_for_deployment_intent(
    state: &AppState,
    deployment_intent: &StoredDeploymentIntent,
) -> Result<Option<VerifiedPipelineBuildOutput>, ApiError> {
    let intent = state
        .store
        .get_pipeline_intent(&deployment_intent.pipeline_intent_id)
        .await?
        .ok_or_else(|| {
            ApiError::not_found("pipeline_intent", &deployment_intent.pipeline_intent_id)
        })?;
    let Some(run_id) = intent.run_id.as_ref() else {
        return Ok(None);
    };
    let artifacts = state.store.list_artifacts(run_id).await?;
    current_pipeline_build_output(&artifacts, &intent)
}

fn release_pipeline_build_output(
    release: &StoredRelease,
) -> Result<Option<VerifiedPipelineBuildOutput>, ApiError> {
    let Some(content) = release.release_json.get("build_output") else {
        return Ok(None);
    };
    if content.get("status").and_then(Value::as_str) != Some("verified") {
        return Err(ApiError::conflict(
            "Release build-output provenance is not verified",
        ));
    }
    let artifact_id = content
        .get("artifact_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| ApiError::conflict("Release build-output has no artifact id"))?;
    let image_url = content
        .get("image_url")
        .and_then(Value::as_str)
        .filter(|value| safe_oci_image_component(value))
        .map(ToOwned::to_owned)
        .ok_or_else(|| ApiError::conflict("Release build-output has no valid image URL"))?;
    let image_digest = content
        .get("image_digest")
        .and_then(Value::as_str)
        .filter(|value| is_sha256_digest(value))
        .map(ToOwned::to_owned)
        .ok_or_else(|| ApiError::conflict("Release build-output has invalid image digest"))?;
    if release
        .image_digest
        .as_deref()
        .is_some_and(|digest| digest != image_digest)
    {
        return Err(ApiError::conflict(
            "Release image digest does not match build-output provenance",
        ));
    }
    let image_reference = content
        .get("image_reference")
        .and_then(Value::as_str)
        .filter(|value| valid_digest_pinned_image_reference(value))
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            ApiError::conflict("Release build-output has invalid digest-pinned image reference")
        })?;
    let source_commit = content
        .get("source_commit")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .map(ToOwned::to_owned);
    Ok(Some(VerifiedPipelineBuildOutput {
        artifact_id,
        image_url,
        image_digest,
        image_reference,
        source_commit,
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
    let build_output = release_pipeline_build_output(&release)?;
    let requested_image_ref = clean_optional_text(request.image_ref);
    if let (Some(output), Some(requested)) = (&build_output, &requested_image_ref) {
        if requested != &output.image_reference {
            return Err(ApiError::conflict(format!(
                "Registry evidence image_ref must match Release build output {}",
                output.image_reference
            )));
        }
    }
    let image_ref = requested_image_ref.or_else(|| {
        build_output
            .as_ref()
            .map(|output| output.image_reference.clone())
    });
    let requested_image_digest = clean_optional_text(request.image_digest);
    if let (Some(output), Some(requested)) = (&build_output, &requested_image_digest) {
        if requested != &output.image_digest {
            return Err(ApiError::conflict(format!(
                "Registry evidence image_digest must match Release build output {}",
                output.image_digest
            )));
        }
    }
    let image_digest = requested_image_digest
        .or_else(|| {
            build_output
                .as_ref()
                .map(|output| output.image_digest.clone())
        })
        .or(release.image_digest.clone());
    let tag = clean_optional_text(request.tag);
    let source = clean_optional_text(request.source).unwrap_or_else(|| {
        if build_output.is_some() {
            "tekton_build_output".to_string()
        } else {
            "manual".to_string()
        }
    });
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
    if let Some(build_output) = release_pipeline_build_output(&release)? {
        if image_ref != build_output.image_reference {
            return Err(ApiError::conflict(format!(
                "Registry inspection image_ref must match Release build output {}",
                build_output.image_reference
            )));
        }
    }

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
                "pipeline_build_output_artifact_id": evidence
                    .evidence_json
                    .pointer("/build_output/artifact_id"),
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
            "pipeline_build_output_artifact_id": evidence
                .evidence_json
                .pointer("/build_output/artifact_id"),
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
    let mut evidence_json = if let Some(evidence_json) = input.evidence_json {
        ensure_json_object(&evidence_json, "evidence_json")?;
        evidence_json
    } else {
        json!({
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
        })
    };
    if let Some(output) = release_pipeline_build_output(release)? {
        evidence_json
            .as_object_mut()
            .expect("evidence_json is validated as an object")
            .insert(
                "build_output".to_string(),
                json!({
                    "artifact_id": output.artifact_id,
                    "image_reference": output.image_reference,
                    "image_digest": output.image_digest,
                    "source_commit": output.source_commit,
                }),
            );
    }
    Ok(evidence_json)
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
    let (Some(remediation_plan_id), Some(incident_id)) = (
        work_plan.remediation_plan_id.clone(),
        work_plan.incident_id.clone(),
    ) else {
        return Err(ApiError::conflict(
            "WorkItem-backed ChangeSets require captured workspace Git diff provenance",
        ));
    };
    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let material_hash = material_hash(&request.change_set_json)?;
    let change_set = state
        .store
        .create_change_set(CreateChangeSet {
            id: format!("cset_{}", unique_suffix()),
            work_item_id: None,
            work_plan_id: work_plan.id.clone(),
            remediation_plan_id: Some(remediation_plan_id),
            incident_id: Some(incident_id),
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

fn coding_run_scope_matches_source(
    run_scope: &RunScope,
    work_item_id: &str,
    workspace_id: &str,
    source_repo: &str,
    branch: &str,
    production_impacting: bool,
) -> bool {
    run_scope.work_item_id.as_deref() == Some(work_item_id)
        && run_scope.workspace_id.as_deref() == Some(workspace_id)
        && run_scope.repo.as_deref() == Some(source_repo)
        && run_scope.branch.as_deref() == Some(branch)
        && run_scope.production_impacting == production_impacting
}

async fn prepare_change_set_git_delivery(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
    Json(request): Json<PrepareGitDeliveryRequest>,
) -> Result<Json<GitDeliveryPlanResponse>, ApiError> {
    let change_set = state
        .store
        .get_change_set(&change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &change_set_id))?;
    ensure_approved_for_trusted_envelope("change_set", &change_set.id, &change_set.status)?;
    let work_plan = state
        .store
        .get_work_plan(&change_set.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &change_set.work_plan_id))?;
    ensure_approved_for_trusted_envelope("work_plan", &work_plan.id, &work_plan.status)?;
    let work_item_id = change_set.work_item_id.as_deref().ok_or_else(|| {
        ApiError::conflict("Git delivery preflight requires a WorkItem-backed ChangeSet")
    })?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    if !work_item_target_supported(&work_item) {
        return Err(ApiError::conflict(
            "Git delivery preflight is limited to dev or the exact protected production target",
        ));
    }
    let run_id = change_set
        .run_id
        .clone()
        .ok_or_else(|| ApiError::conflict("ChangeSet has no coding run provenance"))?;
    let source = change_set
        .change_set_json
        .get("source")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("ChangeSet has no workspace Git source provenance"))?;
    if source.get("kind").and_then(Value::as_str) != Some("workspace_git") {
        return Err(ApiError::conflict(
            "Git delivery preflight requires workspace Git source provenance",
        ));
    }
    let workspace_id = required_json_string(source, "workspace_id", "workspace source")?;
    let base_commit = required_json_string(source, "base_commit", "workspace source")?;
    let branch = required_json_string(source, "branch", "workspace source")?;
    WorkspaceSourceSpec {
        workspace_id: workspace_id.clone(),
        source_repo: work_item.source_repo.clone(),
        source_ref: work_item.source_ref.clone(),
        source_commit: None,
        branch: branch.clone(),
        resolved_commit: Some(base_commit.clone()),
    }
    .validate()
    .map_err(|error| ApiError::conflict(error.to_string()))?;
    let workspace = state
        .store
        .get_workspace(&workspace_id)
        .await?
        .ok_or_else(|| ApiError::conflict("ChangeSet workspace provenance is unavailable"))?;
    if workspace.work_item_id != work_item.id
        || workspace.run_id.as_ref() != Some(&run_id)
        || workspace.source_repo != work_item.source_repo
        || workspace.source_ref != work_item.source_ref
        || workspace.resolved_commit.as_deref() != Some(base_commit.as_str())
        || workspace.branch.as_deref() != Some(branch.as_str())
    {
        return Err(ApiError::conflict(
            "ChangeSet source provenance does not match the durable workspace",
        ));
    }
    let run = state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
    let run_scope = RunScope::from_execution_target(&run.execution_target_json).unwrap_or_default();
    if !coding_run_scope_matches_source(
        &run_scope,
        &work_item.id,
        &workspace.id,
        &work_item.source_repo,
        &branch,
        work_item.production_impacting,
    ) {
        return Err(ApiError::conflict(
            "ChangeSet source provenance does not match the coding run scope",
        ));
    }

    let evidence = change_set
        .change_set_json
        .get("evidence")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("ChangeSet has no Git evidence provenance"))?;
    let diff_artifact_id = required_json_string(evidence, "git_diff_artifact_id", "Git evidence")?;
    let status_artifact_id =
        required_json_string(evidence, "git_status_artifact_id", "Git evidence")?;
    let expected_diff_sha256 = required_json_string(evidence, "diff_sha256", "Git evidence")?;
    let artifacts = state.store.list_artifacts(&run_id).await?;
    let diff_artifact = artifacts
        .iter()
        .find(|artifact| artifact.id == diff_artifact_id && artifact.kind == "workspace_git_diff")
        .ok_or_else(|| ApiError::conflict("ChangeSet Git diff artifact is unavailable"))?;
    let diff = diff_artifact
        .content_text
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::conflict("ChangeSet Git diff artifact has no diff content"))?;
    if diff.len() > 512 * 1024
        || format!("{:x}", Sha256::digest(diff.as_bytes())) != expected_diff_sha256
    {
        return Err(ApiError::conflict(
            "ChangeSet Git diff artifact does not match its recorded digest",
        ));
    }
    let status_artifact = artifacts
        .iter()
        .find(|artifact| {
            artifact.id == status_artifact_id && artifact.kind == "workspace_git_status"
        })
        .ok_or_else(|| ApiError::conflict("ChangeSet Git status artifact is unavailable"))?;
    let changed_paths = status_artifact
        .content_json
        .as_ref()
        .and_then(|content| content.get("changed_paths"))
        .and_then(Value::as_array)
        .filter(|paths| !paths.is_empty())
        .ok_or_else(|| ApiError::conflict("ChangeSet Git status artifact has no changed paths"))?;
    if changed_paths.iter().any(|path| path.as_str().is_none()) {
        return Err(ApiError::conflict(
            "ChangeSet Git status artifact has an invalid changed path",
        ));
    }

    if let Some(existing) = artifacts.iter().find(|artifact| {
        artifact.kind == "git_delivery_plan"
            && artifact.content_json.as_ref().is_some_and(|plan| {
                plan.get("change_set")
                    .and_then(|value| value.get("id"))
                    .and_then(Value::as_str)
                    == Some(change_set.id.as_str())
                    && plan
                        .get("change_set")
                        .and_then(|value| value.get("revision"))
                        .and_then(Value::as_i64)
                        == Some(change_set.revision)
                    && plan
                        .get("change_set")
                        .and_then(|value| value.get("material_hash"))
                        .and_then(Value::as_str)
                        == Some(change_set.material_hash.as_str())
            })
    }) {
        return Ok(Json(GitDeliveryPlanResponse {
            artifact: existing.clone().into(),
            created: false,
        }));
    }

    let title = compact_delivery_subject(&change_set.title);
    let plan = json!({
        "kind": "git_delivery_plan",
        "version": 1,
        "operation": "branch_and_pull_request",
        "change_set": {
            "id": change_set.id,
            "revision": change_set.revision,
            "material_hash": change_set.material_hash,
            "work_plan_id": change_set.work_plan_id,
            "work_item_id": work_item.id,
        },
        "source": {
            "repository": work_item.source_repo,
            "base_ref": work_item.source_ref,
            "base_commit": base_commit,
            "head_branch": branch,
            "workspace_id": workspace_id,
        },
        "evidence": {
            "git_diff_artifact_id": diff_artifact.id,
            "git_status_artifact_id": status_artifact.id,
            "diff_sha256": expected_diff_sha256,
            "changed_paths": changed_paths,
        },
        "commit": {
            "subject": title,
            "body": format!("ChangeSet {} revision {}\n\n{}", change_set.id, change_set.revision, change_set.summary),
        },
        "pull_request": {
            "title": compact_delivery_subject(&change_set.title),
            "body": format!("{}\n\nPharness ChangeSet: {}\nWorkItem: {}", change_set.summary, change_set.id, work_item.id),
        },
        "authorization": {
            "state": "not_authorized",
            "reason": "Git writer identity and typed Git delivery grant are required before this plan can execute",
        },
    });
    let artifact = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_git_delivery", unique_suffix()),
            session_id: change_set.session_id.clone(),
            run_id: Some(run_id),
            kind: "git_delivery_plan".to_string(),
            label: format!("Git delivery plan for ChangeSet {}", change_set.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(plan),
        })
        .await?;
    append_change_set_audit_event(
        &state.store,
        &change_set,
        "change_set.git_delivery_prepared",
        clean_optional_text(request.actor),
        clean_optional_text(request.reason),
        json!({ "git_delivery_plan_artifact_id": artifact.id }),
    )
    .await?;

    Ok(Json(GitDeliveryPlanResponse {
        artifact: artifact.into(),
        created: true,
    }))
}

async fn authorize_change_set_git_delivery(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(change_set_id): Path<String>,
    Json(request): Json<CreateGitDeliveryAuthorizationRequest>,
) -> Result<Json<GitDeliveryAuthorizationResponse>, ApiError> {
    let change_set = state
        .store
        .get_change_set(&change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &change_set_id))?;
    ensure_approved_for_trusted_envelope("change_set", &change_set.id, &change_set.status)?;
    let work_plan = state
        .store
        .get_work_plan(&change_set.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &change_set.work_plan_id))?;
    ensure_approved_for_trusted_envelope("work_plan", &work_plan.id, &work_plan.status)?;
    let work_item_id = change_set.work_item_id.as_deref().ok_or_else(|| {
        ApiError::conflict("Git delivery authorization requires a WorkItem-backed ChangeSet")
    })?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    if !work_item_target_supported(&work_item) {
        return Err(ApiError::conflict(
            "Git delivery authorization is limited to dev or the exact protected production target",
        ));
    }
    let run_id = change_set
        .run_id
        .clone()
        .ok_or_else(|| ApiError::conflict("ChangeSet has no coding run provenance"))?;
    let plan = current_git_delivery_plan(&state.store, &run_id, &change_set).await?;
    let source = git_delivery_plan_source(&plan, &work_item)?;
    let branch = source.head_branch;

    let subject = clean_optional_text(request.subject)
        .unwrap_or_else(|| DEFAULT_GIT_WRITER_SUBJECT.to_string());
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.created_by.clone()));
    let reason = clean_optional_text(Some(request.reason))
        .ok_or_else(|| ApiError::bad_request("Git delivery authorization reason is required"))?;
    let expires_at = bounded_production_grant_expiry(&work_item, request.expires_at)?;
    if let Some(existing) = matching_git_delivery_grant(
        &state.store,
        &subject,
        &change_set,
        &work_item,
        &branch,
        &plan.id,
    )
    .await?
    {
        return Ok(Json(GitDeliveryAuthorizationResponse {
            grant: existing.into(),
            plan: plan.into(),
            created: false,
        }));
    }

    let grant = create_permission_grant_record(
        &state.store,
        CreatePermissionGrantRequest {
            subject,
            created_by: actor.clone(),
            reason: reason.clone(),
            scope: json!({
                "environment": work_item.target_environment,
                "capability_kinds": ["git"],
                "actions": GIT_DELIVERY_ACTIONS,
                "max_risk": "high",
                "repos": [work_item.source_repo],
                "branches": [branch],
                "work_plan_ids": [change_set.work_plan_id],
                "change_set_ids": [change_set.id],
                "git_delivery_plan_artifact_ids": [plan.id],
                "production_impacting": work_item.production_impacting,
            }),
            policy: json!({ "policy_mode": "supervised_autonomy" }),
            expires_at,
        },
    )
    .await?;
    append_change_set_audit_event(
        &state.store,
        &change_set,
        "change_set.git_delivery_authorized",
        actor,
        Some(reason),
        json!({
            "permission_grant_id": grant.id,
            "git_delivery_plan_artifact_id": plan.id,
            "subject": grant.subject,
        }),
    )
    .await?;

    Ok(Json(GitDeliveryAuthorizationResponse {
        grant: grant.into(),
        plan: plan.into(),
        created: true,
    }))
}

async fn preflight_change_set_git_delivery(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(change_set_id): Path<String>,
    Json(request): Json<GitDeliveryPreflightRequest>,
) -> Result<Json<GitDeliveryPreflightResponse>, ApiError> {
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
    let work_item_id = change_set.work_item_id.as_deref().ok_or_else(|| {
        ApiError::conflict("Git delivery preflight requires a WorkItem-backed ChangeSet")
    })?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let run_id = change_set
        .run_id
        .clone()
        .ok_or_else(|| ApiError::conflict("ChangeSet has no coding run provenance"))?;
    let plan = current_git_delivery_plan(&state.store, &run_id, &change_set).await?;
    let source = git_delivery_plan_source(&plan, &work_item)?;
    let approval_gate = state
        .store
        .list_approval_gates(ApprovalGateListFilter {
            work_item_id: Some(work_item.id.clone()),
            gate_kind: Some("git_mutation".to_string()),
            limit: 20,
            ..ApprovalGateListFilter::default()
        })
        .await?
        .into_iter()
        .find(|gate| work_item_gate_scope_matches(gate, &work_item, &work_plan, "git_mutation"));
    let approval_gate_ready = approval_gate
        .as_ref()
        .is_some_and(|gate| matches!(gate.status.as_str(), "satisfied" | "waived"));
    let source_approval_gate = if work_item.production_impacting {
        state
            .store
            .list_approval_gates(ApprovalGateListFilter {
                work_item_id: Some(work_item.id.clone()),
                gate_kind: Some("source_mutation".to_string()),
                limit: 20,
                ..ApprovalGateListFilter::default()
            })
            .await?
            .into_iter()
            .find(|gate| {
                work_item_gate_scope_matches(gate, &work_item, &work_plan, "source_mutation")
            })
    } else {
        None
    };
    let source_approval_ready = !work_item.production_impacting
        || source_approval_gate
            .as_ref()
            .is_some_and(|gate| matches!(gate.status.as_str(), "satisfied" | "waived"));
    let subject = clean_optional_text(request.subject)
        .unwrap_or_else(|| DEFAULT_GIT_WRITER_SUBJECT.to_string());
    let grant = matching_git_delivery_grant(
        &state.store,
        &subject,
        &change_set,
        &work_item,
        &source.head_branch,
        &plan.id,
    )
    .await?;
    let authorization_ready = grant.is_some();
    let dispatch_ready = state.worker.git_writer_available();
    let checks = vec![
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
            "supported_target",
            work_item_target_supported(&work_item),
            format!(
                "WorkItem targets {}{}",
                work_item.target_environment,
                if work_item.production_impacting {
                    " (protected production)"
                } else {
                    ""
                }
            ),
        ),
        execution_check(
            "immutable_source_provenance",
            true,
            format!(
                "Plan pins {} at {} from workspace {}",
                source.repository, source.base_commit, source.workspace_id
            ),
        ),
        execution_check(
            "work_item_git_mutation_gate",
            approval_gate_ready,
            approval_gate
                .as_ref()
                .map(|gate| format!("Git mutation gate {} is {}", gate.id, gate.status))
                .unwrap_or_else(|| {
                    "No scoped WorkItem git_mutation gate matches this Git delivery plan"
                        .to_string()
                }),
        ),
        execution_check(
            "work_item_source_mutation_gate",
            source_approval_ready,
            if work_item.production_impacting {
                source_approval_gate
                    .as_ref()
                    .map(|gate| format!("Source mutation gate {} is {}", gate.id, gate.status))
                    .unwrap_or_else(|| {
                        "No scoped WorkItem source_mutation gate matches this Git delivery plan"
                            .to_string()
                    })
            } else {
                "Separate source_mutation gate is not required for legacy dev delivery".to_string()
            },
        ),
        execution_check(
            "trusted_git_delivery_grant",
            authorization_ready,
            grant
                .as_ref()
                .map(|grant| {
                    format!(
                        "Active supervised-autonomy grant {} matches Git writer {}",
                        grant.id, subject
                    )
                })
                .unwrap_or_else(|| {
                    format!(
                        "No active supervised-autonomy Git delivery grant matches writer {}",
                        subject
                    )
                }),
        ),
        execution_check(
            "git_writer_executor_available",
            dispatch_ready,
            if dispatch_ready {
                "Dedicated Git writer Job is configured"
            } else {
                "No Git writer executor is configured; remote branch, commit, push, and pull-request creation remain unavailable"
            },
        ),
    ];
    let prerequisites_ready = checks
        .iter()
        .filter(|check| {
            check.get("code").and_then(Value::as_str) != Some("git_writer_executor_available")
        })
        .all(|check| check.get("passed").and_then(Value::as_bool) == Some(true));
    // "ready_for_writer" says the immutable plan and grant are ready. The
    // separate dispatch flag tells the operator whether this environment has
    // actually provisioned the isolated writer identity yet.
    let status = if prerequisites_ready {
        "ready_for_writer"
    } else {
        "blocked"
    };
    let grant_id = grant.as_ref().map(|grant| grant.id.clone());

    let artifacts = state.store.list_artifacts(&run_id).await?;
    if let Some(existing) = artifacts.into_iter().find(|artifact| {
        artifact.kind == "git_delivery_preflight"
            && artifact.content_json.as_ref().is_some_and(|content| {
                content
                    .get("git_delivery_plan_artifact_id")
                    .and_then(Value::as_str)
                    == Some(plan.id.as_str())
                    && content.get("subject").and_then(Value::as_str) == Some(subject.as_str())
                    && content.get("permission_grant_id").and_then(Value::as_str)
                        == grant_id.as_deref()
                    && content.get("approval_gate_id").and_then(Value::as_str)
                        == approval_gate.as_ref().map(|gate| gate.id.as_str())
                    && content.get("approval_gate_status").and_then(Value::as_str)
                        == approval_gate.as_ref().map(|gate| gate.status.as_str())
            })
    }) {
        return Ok(Json(GitDeliveryPreflightResponse {
            status: status.to_string(),
            approval_gate_ready,
            authorization_ready,
            dispatch_ready,
            plan: plan.into(),
            permission_grant: grant.map(Into::into),
            checks,
            artifact: existing.into(),
            created: false,
        }));
    }

    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    let artifact = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_git_delivery_preflight", unique_suffix()),
            session_id: change_set.session_id.clone(),
            run_id: Some(run_id),
            kind: "git_delivery_preflight".to_string(),
            label: format!("Git delivery preflight for ChangeSet {}", change_set.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "change_set_id": change_set.id,
                "work_plan_id": change_set.work_plan_id,
                "work_item_id": work_item.id,
                "git_delivery_plan_artifact_id": plan.id,
                "subject": subject,
                "permission_grant_id": grant_id,
                "approval_gate_id": approval_gate.as_ref().map(|gate| &gate.id),
                "approval_gate_status": approval_gate.as_ref().map(|gate| &gate.status),
                "approval_gate_ready": approval_gate_ready,
                "status": status,
                "authorization_ready": authorization_ready,
                "dispatch_ready": dispatch_ready,
                "checks": checks,
                "dispatch": {
                    "state": if dispatch_ready { "configured" } else { "not_configured" },
                    "summary": if dispatch_ready { "Dedicated Git writer is configured; execution remains operator-invoked" } else { "Git writer execution is intentionally unavailable until its isolated identity and executor are configured" },
                },
                "reason": reason,
            })),
        })
        .await?;
    append_change_set_audit_event(
        &state.store,
        &change_set,
        "change_set.git_delivery_preflighted",
        actor,
        reason,
        json!({
            "git_delivery_plan_artifact_id": plan.id,
            "git_delivery_preflight_artifact_id": artifact.id,
            "permission_grant_id": grant_id,
            "approval_gate_id": approval_gate.as_ref().map(|gate| &gate.id),
            "approval_gate_ready": approval_gate_ready,
            "subject": subject,
            "status": status,
            "authorization_ready": authorization_ready,
            "dispatch_ready": dispatch_ready,
        }),
    )
    .await?;

    Ok(Json(GitDeliveryPreflightResponse {
        status: status.to_string(),
        approval_gate_ready,
        authorization_ready,
        dispatch_ready,
        plan: plan.into(),
        permission_grant: grant.map(Into::into),
        checks,
        artifact: artifact.into(),
        created: true,
    }))
}

async fn execute_change_set_git_delivery(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(change_set_id): Path<String>,
    Json(request): Json<ExecuteGitDeliveryRequest>,
) -> Result<Json<ExecuteGitDeliveryResponse>, ApiError> {
    let subject = clean_optional_text(request.subject.clone())
        .unwrap_or_else(|| DEFAULT_GIT_WRITER_SUBJECT.to_string());
    let actor = identity
        .as_ref()
        .map(|Extension(OperatorIdentity(name))| name.clone())
        .or_else(|| clean_optional_text(request.actor.clone()));
    let reason = clean_optional_text(Some(request.reason))
        .ok_or_else(|| ApiError::bad_request("Git delivery execution reason is required"))?;

    // Re-run the exact preflight just before dispatch. Stored preflights are
    // evidence, not authorization: a ChangeSet revision or grant revocation
    // must stop a subsequent executor call.
    let Json(preflight) = preflight_change_set_git_delivery(
        State(state.clone()),
        identity,
        Path(change_set_id.clone()),
        Json(GitDeliveryPreflightRequest {
            subject: Some(subject.clone()),
            actor: actor.clone(),
            reason: Some(reason.clone()),
        }),
    )
    .await?;
    if preflight.status != "ready_for_writer" || !preflight.dispatch_ready {
        return Err(ApiError::conflict(
            "Git delivery execution requires a current approved plan, matching writer grant, and configured dedicated writer",
        ));
    }
    let grant = preflight.permission_grant.clone().ok_or_else(|| {
        ApiError::conflict("Git delivery execution requires an active matching writer grant")
    })?;
    let change_set = state
        .store
        .get_change_set(&change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &change_set_id))?;
    let run_id = change_set
        .run_id
        .clone()
        .ok_or_else(|| ApiError::conflict("ChangeSet has no coding run provenance"))?;
    let plan = state
        .store
        .get_artifact(&preflight.plan.id)
        .await?
        .ok_or_else(|| ApiError::conflict("current Git delivery plan is unavailable"))?;
    let work_item_id = change_set.work_item_id.as_deref().ok_or_else(|| {
        ApiError::conflict("Git delivery execution requires a WorkItem-backed ChangeSet")
    })?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let source = git_delivery_plan_source(&plan, &work_item)?;
    let settings = state
        .worker
        .git_writer_settings()
        .ok_or_else(|| ApiError::conflict("Git writer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &source.repository)
    {
        return Err(ApiError::conflict(
            "Git delivery repository is not allowlisted for the Git writer",
        ));
    }

    let artifacts = state.store.list_artifacts(&run_id).await?;
    if let Some(existing) = artifacts.iter().find(|artifact| {
        git_delivery_artifact_matches_plan(artifact, "git_delivery_execution", &plan.id)
            && artifact.content_json.as_ref().is_some_and(|content| {
                content.get("permission_grant_id").and_then(Value::as_str)
                    == Some(grant.id.as_str())
            })
    }) {
        let execution_id = existing
            .content_json
            .as_ref()
            .and_then(|value| value.get("execution_id"))
            .and_then(Value::as_str);
        let terminal_status = execution_id.and_then(|execution_id| {
            artifacts.iter().find_map(|artifact| {
                (artifact.kind == "git_delivery_result")
                    .then_some(artifact.content_json.as_ref())
                    .flatten()
                    .filter(|content| {
                        content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                    })
                    .and_then(|content| content.get("status").and_then(Value::as_str))
            })
        });
        return Ok(Json(ExecuteGitDeliveryResponse {
            status: terminal_status.unwrap_or("dispatched").to_string(),
            execution: existing.clone().into(),
            plan: plan.into(),
            permission_grant: grant,
            job_name: existing
                .content_json
                .as_ref()
                .and_then(|value| value.get("job_name"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            created: false,
        }));
    }

    let execution_id = format!("gexec_{}", unique_suffix());
    let execution = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_git_delivery_execution", unique_suffix()),
            session_id: change_set.session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "git_delivery_execution".to_string(),
            label: format!("Git delivery execution for ChangeSet {}", change_set.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": execution_id,
                "status": "dispatched",
                "change_set_id": change_set.id,
                "git_delivery_plan_artifact_id": plan.id,
                "permission_grant_id": grant.id,
                "subject": subject,
                "dispatched_by": actor,
                "reason": reason,
                "source": {
                    "repository": source.repository,
                    "base_ref": source.base_ref,
                    "base_commit": source.base_commit,
                    "head_branch": source.head_branch,
                },
            })),
        })
        .await?;

    match state
        .worker
        .dispatch_git_delivery(GitDeliveryExecutionRequest {
            change_set_id: change_set.id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => {
            append_change_set_audit_event(
                &state.store,
                &change_set,
                "change_set.git_delivery_dispatched",
                actor,
                Some(reason),
                json!({
                    "execution_id": execution_id,
                    "git_delivery_plan_artifact_id": plan.id,
                    "permission_grant_id": grant.id,
                    "job_name": receipt.job_name,
                }),
            )
            .await?;
            Ok(Json(ExecuteGitDeliveryResponse {
                status: "dispatched".to_string(),
                execution: execution.into(),
                plan: plan.into(),
                permission_grant: grant,
                job_name: Some(receipt.job_name),
                created: true,
            }))
        }
        Err(error) => {
            let failure = persist_git_delivery_result(
                &state.store,
                &change_set,
                &run_id,
                &plan.id,
                &execution_id,
                "dispatch_failed",
                json!({ "error_code": "job_dispatch_failed" }),
            )
            .await?;
            append_change_set_audit_event(
                &state.store,
                &change_set,
                "change_set.git_delivery_dispatch_failed",
                None,
                None,
                json!({ "execution_id": execution_id, "error_code": "job_dispatch_failed" }),
            )
            .await?;
            tracing::warn!(change_set_id = %change_set.id, %error, "Git writer dispatch failed");
            Ok(Json(ExecuteGitDeliveryResponse {
                status: "dispatch_failed".to_string(),
                execution: failure,
                plan: plan.into(),
                permission_grant: grant,
                job_name: None,
                created: true,
            }))
        }
    }
}

async fn observe_change_set_git_delivery(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(change_set_id): Path<String>,
    Json(request): Json<ObserveGitDeliveryRequest>,
) -> Result<Json<ObserveGitDeliveryResponse>, ApiError> {
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(Some(request.reason))
        .ok_or_else(|| ApiError::bad_request("Git delivery observation reason is required"))?;
    let change_set = state
        .store
        .get_change_set(&change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &change_set_id))?;
    let run_id = change_set
        .run_id
        .clone()
        .ok_or_else(|| ApiError::conflict("Git delivery ChangeSet has no coding run provenance"))?;
    let work_item = state
        .store
        .get_work_item(change_set.work_item_id.as_deref().ok_or_else(|| {
            ApiError::conflict("Git delivery observation requires a WorkItem-backed ChangeSet")
        })?)
        .await?
        .ok_or_else(|| ApiError::conflict("Git delivery WorkItem is unavailable"))?;
    if !work_item_target_supported(&work_item) {
        return Err(ApiError::conflict(
            "Git delivery observation is limited to dev or the exact protected production target",
        ));
    }
    let plan = current_git_delivery_plan(&state.store, &run_id, &change_set).await?;
    let source = git_delivery_plan_source(&plan, &work_item)?;
    let settings = state
        .worker
        .git_observer_settings()
        .ok_or_else(|| ApiError::conflict("Git observer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &source.repository)
    {
        return Err(ApiError::conflict(
            "Git delivery repository is not allowlisted for the Git observer",
        ));
    }
    let artifacts = state.store.list_artifacts(&run_id).await?;
    let delivery_result = artifacts
        .iter()
        .filter(|artifact| {
            git_delivery_artifact_matches_plan(artifact, "git_delivery_result", &plan.id)
        })
        .filter(|artifact| {
            artifact
                .content_json
                .as_ref()
                .and_then(|value| value.get("status"))
                .and_then(Value::as_str)
                == Some("completed")
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .ok_or_else(|| {
            ApiError::conflict("Git delivery observation requires a completed branch-and-PR result")
        })?;
    let details = delivery_result
        .content_json
        .as_ref()
        .and_then(|value| value.get("details"))
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git delivery result has no pull-request provenance"))?;
    let pull_request_number = details
        .get("pull_request_number")
        .and_then(Value::as_u64)
        .ok_or_else(|| ApiError::conflict("Git delivery result has no pull-request number"))?;
    let pull_request_url =
        required_json_string(details, "pull_request_url", "Git delivery result")?;
    let source_commit_sha = required_json_string(details, "commit_sha", "Git delivery result")?;
    if !is_git_sha(&source_commit_sha) || !is_github_pr_url(&pull_request_url) {
        return Err(ApiError::conflict(
            "Git delivery result has invalid GitHub provenance",
        ));
    }
    if let Some(existing) = artifacts.iter().find(|artifact| {
        artifact.kind == "git_delivery_observation_execution"
            && artifact.content_json.as_ref().is_some_and(|content| {
                content
                    .get("git_delivery_plan_artifact_id")
                    .and_then(Value::as_str)
                    == Some(plan.id.as_str())
                    && content
                        .get("git_delivery_result_artifact_id")
                        .and_then(Value::as_str)
                        == Some(delivery_result.id.as_str())
                    && !artifacts.iter().any(|failure| {
                        failure.kind == "git_delivery_observation_dispatch_failure"
                            && failure
                                .content_json
                                .as_ref()
                                .is_some_and(|failure_content| {
                                    failure_content.get("execution_id").and_then(Value::as_str)
                                        == content.get("execution_id").and_then(Value::as_str)
                                })
                    })
            })
    }) {
        return Ok(Json(ObserveGitDeliveryResponse {
            status: existing
                .content_json
                .as_ref()
                .and_then(|value| value.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("dispatched")
                .to_string(),
            execution: existing.clone().into(),
            job_name: existing
                .content_json
                .as_ref()
                .and_then(|value| value.get("job_name"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            created: false,
        }));
    }
    let execution_id = format!("gobs_{}", unique_suffix());
    let execution = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_git_delivery_observation", unique_suffix()),
            session_id: change_set.session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "git_delivery_observation_execution".to_string(),
            label: format!("Git delivery observation for ChangeSet {}", change_set.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": execution_id,
                "status": "dispatched",
                "change_set_id": change_set.id,
                "git_delivery_plan_artifact_id": plan.id,
                "git_delivery_result_artifact_id": delivery_result.id,
                "source": { "repository": source.repository, "head_branch": source.head_branch, "source_commit_sha": source_commit_sha, "pull_request_url": pull_request_url, "pull_request_number": pull_request_number },
                "dispatched_by": actor,
                "reason": reason,
            })),
        })
        .await?;
    match state
        .worker
        .dispatch_git_delivery_observation(GitDeliveryObservationRequest {
            change_set_id: change_set.id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => {
            append_change_set_audit_event(&state.store, &change_set, "change_set.git_delivery_observation_dispatched", actor, Some(reason), json!({ "execution_id": execution_id, "git_delivery_plan_artifact_id": plan.id, "job_name": receipt.job_name })).await?;
            Ok(Json(ObserveGitDeliveryResponse {
                status: "dispatched".to_string(),
                execution: execution.into(),
                job_name: Some(receipt.job_name),
                created: true,
            }))
        }
        Err(error) => {
            tracing::warn!(change_set_id = %change_set.id, %error, "Git observer dispatch failed");
            let failure = state
                .store
                .create_artifact(CreateArtifact {
                    id: format!(
                        "art_{}_git_delivery_observation_dispatch_failure",
                        unique_suffix()
                    ),
                    session_id: change_set.session_id.clone(),
                    run_id: Some(run_id),
                    kind: "git_delivery_observation_dispatch_failure".to_string(),
                    label: format!(
                        "Git delivery observation dispatch failure for ChangeSet {}",
                        change_set.id
                    ),
                    mime_type: Some("application/json".to_string()),
                    path: None,
                    content_text: None,
                    content_json: Some(json!({
                        "execution_id": execution_id,
                        "status": "dispatch_failed",
                        "change_set_id": change_set.id,
                        "git_delivery_plan_artifact_id": plan.id,
                        "git_delivery_result_artifact_id": delivery_result.id,
                        "error_code": "git_observer_dispatch_failed",
                    })),
                })
                .await?;
            append_change_set_audit_event(
                &state.store,
                &change_set,
                "change_set.git_delivery_observation_dispatch_failed",
                actor,
                Some(reason),
                json!({
                    "execution_id": execution_id,
                    "git_delivery_plan_artifact_id": plan.id,
                    "dispatch_failure_artifact_id": failure.id,
                    "error_code": "git_observer_dispatch_failed",
                }),
            )
            .await?;
            Ok(Json(ObserveGitDeliveryResponse {
                status: "dispatch_failed".to_string(),
                execution: execution.into(),
                job_name: None,
                created: true,
            }))
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct InternalGitDeliveryContextQuery {
    execution_id: String,
}

async fn internal_git_delivery_context(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
    Query(query): Query<InternalGitDeliveryContextQuery>,
) -> Result<Json<GitDeliveryContextResponse>, ApiError> {
    let (change_set, run_id, plan, execution) =
        current_git_delivery_execution(&state, &change_set_id, &query.execution_id).await?;
    let work_item = state
        .store
        .get_work_item(
            change_set
                .work_item_id
                .as_deref()
                .ok_or_else(|| ApiError::conflict("Git delivery ChangeSet has no WorkItem"))?,
        )
        .await?
        .ok_or_else(|| ApiError::conflict("Git delivery WorkItem is unavailable"))?;
    let source = git_delivery_plan_source(&plan, &work_item)?;
    let settings = state
        .worker
        .git_writer_settings()
        .ok_or_else(|| ApiError::conflict("Git writer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &source.repository)
    {
        return Err(ApiError::conflict(
            "Git delivery repository is not allowlisted for the Git writer",
        ));
    }
    let evidence = plan
        .content_json
        .as_ref()
        .and_then(|value| value.get("evidence"))
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git delivery plan has no evidence"))?;
    let diff_id = required_json_string(evidence, "git_diff_artifact_id", "Git delivery evidence")?;
    let diff = state
        .store
        .list_artifacts(&run_id)
        .await?
        .into_iter()
        .find(|artifact| artifact.id == diff_id && artifact.kind == "workspace_git_diff")
        .and_then(|artifact| artifact.content_text)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::conflict("Git delivery diff evidence is unavailable"))?;
    let plan_json = plan
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git delivery plan has no structured content"))?;
    let commit = plan_json
        .get("commit")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git delivery plan has no commit metadata"))?;
    let pull_request = plan_json
        .get("pull_request")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git delivery plan has no pull request metadata"))?;
    let _ = execution;
    Ok(Json(GitDeliveryContextResponse {
        execution_id: query.execution_id,
        repository: source.repository,
        base_ref: source.base_ref,
        base_commit: source.base_commit,
        head_branch: source.head_branch,
        diff,
        commit_subject: required_json_string(commit, "subject", "Git delivery commit")?,
        commit_body: required_json_string(commit, "body", "Git delivery commit")?,
        pull_request_title: required_json_string(
            pull_request,
            "title",
            "Git delivery pull request",
        )?,
        pull_request_body: required_json_string(pull_request, "body", "Git delivery pull request")?,
        github_api_url: settings.github_api_url,
        author_name: settings.author_name,
        author_email: settings.author_email,
    }))
}

async fn internal_git_delivery_outcome(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
    Json(request): Json<GitDeliveryOutcomeRequest>,
) -> Result<Json<ArtifactResponse>, ApiError> {
    let (change_set, run_id, plan, _execution) =
        current_git_delivery_execution(&state, &change_set_id, &request.execution_id).await?;
    let result = match request.status.as_str() {
        "completed" => {
            let branch = clean_optional_text(request.branch).ok_or_else(|| ApiError::bad_request("completed Git delivery outcome requires branch"))?;
            let sha = clean_optional_text(request.commit_sha).ok_or_else(|| ApiError::bad_request("completed Git delivery outcome requires commit_sha"))?;
            let url = clean_optional_text(request.pull_request_url).ok_or_else(|| ApiError::bad_request("completed Git delivery outcome requires pull_request_url"))?;
            let number = request.pull_request_number.ok_or_else(|| ApiError::bad_request("completed Git delivery outcome requires pull_request_number"))?;
            if !is_git_sha(&sha) || !is_github_pr_url(&url) {
                return Err(ApiError::bad_request(
                    "completed Git delivery outcome has invalid GitHub provenance",
                ));
            }
            let work_item = state.store.get_work_item(change_set.work_item_id.as_deref().ok_or_else(|| ApiError::conflict("Git delivery ChangeSet has no WorkItem"))?).await?
                .ok_or_else(|| ApiError::conflict("Git delivery WorkItem is unavailable"))?;
            let source = git_delivery_plan_source(&plan, &work_item)?;
            if branch != source.head_branch { return Err(ApiError::conflict("Git delivery outcome branch does not match immutable plan")); }
            let expected_pr_prefix = format!(
                "https://github.com/{}/pull/",
                source.repository.trim_start_matches("https://github.com/").trim_end_matches(".git")
            );
            if !url.starts_with(&expected_pr_prefix) || !url.ends_with(&number.to_string()) {
                return Err(ApiError::conflict(
                    "Git delivery pull request does not match immutable repository provenance",
                ));
            }
            persist_git_delivery_result(&state.store, &change_set, &run_id, &plan.id, &request.execution_id, "completed", json!({
                "branch": branch, "commit_sha": sha, "pull_request_url": url, "pull_request_number": number,
            })).await?
        }
        "failed" => persist_git_delivery_result(&state.store, &change_set, &run_id, &plan.id, &request.execution_id, "failed", json!({
            "error_code": clean_optional_text(request.error_code).unwrap_or_else(|| "git_writer_failed".to_string()),
        })).await?,
        _ => return Err(ApiError::bad_request("Git delivery outcome status must be completed or failed")),
    };
    append_change_set_audit_event(&state.store, &change_set, &format!("change_set.git_delivery_{}", request.status), Some(DEFAULT_GIT_WRITER_SUBJECT.to_string()), None,
        json!({ "execution_id": request.execution_id, "git_delivery_plan_artifact_id": plan.id, "result_artifact_id": result.id }))
        .await?;
    Ok(Json(result))
}

async fn internal_git_delivery_observation_context(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
    Query(query): Query<InternalGitDeliveryContextQuery>,
) -> Result<Json<GitDeliveryObservationContextResponse>, ApiError> {
    let (change_set, run_id, plan, execution) =
        current_git_delivery_observation(&state, &change_set_id, &query.execution_id).await?;
    let work_item = state
        .store
        .get_work_item(change_set.work_item_id.as_deref().ok_or_else(|| {
            ApiError::conflict("Git delivery observation ChangeSet has no WorkItem")
        })?)
        .await?
        .ok_or_else(|| ApiError::conflict("Git delivery observation WorkItem is unavailable"))?;
    let source = git_delivery_plan_source(&plan, &work_item)?;
    let settings = state
        .worker
        .git_observer_settings()
        .ok_or_else(|| ApiError::conflict("Git observer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &source.repository)
    {
        return Err(ApiError::conflict(
            "Git delivery repository is not allowlisted for the Git observer",
        ));
    }
    let execution_content = execution
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git observation execution has no structured content"))?;
    let source_content = execution_content
        .get("source")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git observation execution has no source provenance"))?;
    let _ = run_id;
    Ok(Json(GitDeliveryObservationContextResponse {
        execution_id: query.execution_id,
        repository: source.repository,
        head_branch: required_json_string(source_content, "head_branch", "Git observation source")?,
        source_commit_sha: required_json_string(
            source_content,
            "source_commit_sha",
            "Git observation source",
        )?,
        pull_request_url: required_json_string(
            source_content,
            "pull_request_url",
            "Git observation source",
        )?,
        pull_request_number: source_content
            .get("pull_request_number")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                ApiError::conflict("Git observation source has no pull-request number")
            })?,
        github_api_url: settings.github_api_url,
    }))
}

async fn internal_git_delivery_observation_outcome(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
    Json(request): Json<GitDeliveryObservationOutcomeRequest>,
) -> Result<Json<ArtifactResponse>, ApiError> {
    let (change_set, run_id, plan, execution) =
        current_git_delivery_observation(&state, &change_set_id, &request.execution_id).await?;
    let execution_content = execution
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git observation execution has no structured content"))?;
    let expected = execution_content
        .get("source")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git observation execution has no source provenance"))?;
    let artifact: ArtifactResponse = match request.status.as_str() {
        "observed" => {
            let pull_request_state = clean_optional_text(request.pull_request_state)
                .ok_or_else(|| ApiError::bad_request("observed Git outcome requires pull_request_state"))?;
            if !matches!(pull_request_state.as_str(), "open" | "closed") {
                return Err(ApiError::bad_request("Git pull_request_state must be open or closed"));
            }
            let merged = request.merged.ok_or_else(|| ApiError::bad_request("observed Git outcome requires merged"))?;
            let head_branch = clean_optional_text(request.head_branch)
                .ok_or_else(|| ApiError::bad_request("observed Git outcome requires head_branch"))?;
            let head_commit_sha = clean_optional_text(request.head_commit_sha)
                .ok_or_else(|| ApiError::bad_request("observed Git outcome requires head_commit_sha"))?;
            if !is_git_sha(&head_commit_sha)
                || expected.get("head_branch").and_then(Value::as_str) != Some(head_branch.as_str())
                || expected.get("source_commit_sha").and_then(Value::as_str) != Some(head_commit_sha.as_str())
            {
                return Err(ApiError::conflict("Git observation does not match the delivered branch commit"));
            }
            let merge_commit_sha = clean_optional_text(request.merge_commit_sha);
            if merged {
                let merge_commit_sha = merge_commit_sha.as_deref().ok_or_else(|| {
                    ApiError::bad_request("merged Git outcome requires merge_commit_sha")
                })?;
                if pull_request_state != "closed" || !is_git_sha(merge_commit_sha) {
                    return Err(ApiError::bad_request("merged Git outcome has invalid merge provenance"));
                }
            } else if merge_commit_sha.is_some() {
                return Err(ApiError::bad_request("unmerged Git outcome must not include merge_commit_sha"));
            }
            let observation = state.store.create_artifact(CreateArtifact {
                id: format!("art_{}_git_delivery_pr_observation", unique_suffix()),
                session_id: change_set.session_id.clone(), run_id: Some(run_id.clone()),
                kind: "git_delivery_pr_observation".to_string(), label: format!("GitHub PR observation for ChangeSet {}", change_set.id),
                mime_type: Some("application/json".to_string()), path: None, content_text: None,
                content_json: Some(json!({ "execution_id": request.execution_id, "status": "observed", "change_set_id": change_set.id, "git_delivery_plan_artifact_id": plan.id, "pull_request_state": pull_request_state, "merged": merged, "head_branch": head_branch, "head_commit_sha": head_commit_sha, "merge_commit_sha": merge_commit_sha })),
            }).await?;
            if let Some(merge_commit_sha) = merge_commit_sha {
                state.store.create_artifact(CreateArtifact {
                    id: format!("art_{}_git_delivery_merge", unique_suffix()),
                    session_id: change_set.session_id.clone(), run_id: Some(run_id.clone()),
                    kind: "git_delivery_merge".to_string(), label: format!("Immutable Git merge for ChangeSet {}", change_set.id),
                    mime_type: Some("application/json".to_string()), path: None, content_text: None,
                    content_json: Some(json!({ "execution_id": request.execution_id, "change_set_id": change_set.id, "git_delivery_plan_artifact_id": plan.id, "pull_request_url": expected.get("pull_request_url"), "pull_request_number": expected.get("pull_request_number"), "head_commit_sha": head_commit_sha, "merge_commit_sha": merge_commit_sha })),
                }).await?;
            }
            observation.into()
        }
        "failed" => state.store.create_artifact(CreateArtifact {
            id: format!("art_{}_git_delivery_pr_observation", unique_suffix()),
            session_id: change_set.session_id.clone(), run_id: Some(run_id.clone()),
            kind: "git_delivery_pr_observation".to_string(), label: format!("Failed GitHub PR observation for ChangeSet {}", change_set.id),
            mime_type: Some("application/json".to_string()), path: None, content_text: None,
            content_json: Some(json!({ "execution_id": request.execution_id, "status": "failed", "change_set_id": change_set.id, "git_delivery_plan_artifact_id": plan.id, "error_code": clean_optional_text(request.error_code).unwrap_or_else(|| "git_observer_failed".to_string()) })),
        }).await?.into(),
        _ => return Err(ApiError::bad_request("Git observation outcome status must be observed or failed")),
    };
    append_change_set_audit_event(&state.store, &change_set, &format!("change_set.git_delivery_observation_{}", request.status), Some("agent:git-observer".to_string()), None, json!({ "execution_id": request.execution_id, "git_delivery_plan_artifact_id": plan.id, "observation_artifact_id": artifact.id }))
        .await?;
    Ok(Json(artifact))
}

#[derive(Debug, serde::Deserialize)]
struct InternalGitOpsBaseRevisionQuery {
    execution_id: String,
}

async fn internal_gitops_base_revision_context(
    State(state): State<AppState>,
    Path(gitops_change_set_id): Path<String>,
    Query(query): Query<InternalGitOpsBaseRevisionQuery>,
) -> Result<Json<GitOpsBaseRevisionContextResponse>, ApiError> {
    let (change_set, execution) =
        current_gitops_base_revision_execution(&state, &gitops_change_set_id, &query.execution_id)
            .await?;
    let settings = state.worker.gitops_observer_settings().ok_or_else(|| {
        ApiError::conflict(
            "read-only GitOps observer identity is not configured for GitOps revision resolution",
        )
    })?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &change_set.gitops_repo)
    {
        return Err(ApiError::conflict(
            "GitOps repository is not allowlisted for the read-only Git observer identity",
        ));
    }
    let _ = execution;
    Ok(Json(GitOpsBaseRevisionContextResponse {
        execution_id: query.execution_id,
        repository: change_set.gitops_repo,
        base_ref: change_set.gitops_ref,
        github_api_url: settings.github_api_url,
    }))
}

async fn internal_gitops_base_revision_outcome(
    State(state): State<AppState>,
    Path(gitops_change_set_id): Path<String>,
    Json(request): Json<GitOpsBaseRevisionOutcomeRequest>,
) -> Result<Json<ArtifactResponse>, ApiError> {
    let (change_set, execution) = current_gitops_base_revision_execution(
        &state,
        &gitops_change_set_id,
        &request.execution_id,
    )
    .await?;
    let result = match request.status.as_str() {
        "resolved" => {
            let base_commit = clean_optional_text(request.base_commit)
                .ok_or_else(|| ApiError::bad_request("resolved base revision outcome requires base_commit"))?;
            if !is_git_sha(&base_commit) {
                return Err(ApiError::bad_request(
                    "resolved base revision outcome requires a 40-character Git SHA",
                ));
            }
            state
                .store
                .create_artifact(CreateArtifact {
                    id: format!("art_{}_gitops_base_revision", unique_suffix()),
                    session_id: change_set.session_id.clone(),
                    run_id: Some(change_set.run_id.clone()),
                    kind: "gitops_base_revision".to_string(),
                    label: format!("Resolved GitOps base revision for {}", change_set.id),
                    mime_type: Some("application/json".to_string()),
                    path: None,
                    content_text: None,
                    content_json: Some(json!({
                        "execution_id": request.execution_id,
                        "status": "resolved",
                        "gitops_change_set_id": change_set.id,
                        "gitops_change_set_revision": change_set.revision,
                        "material_hash": change_set.material_hash,
                        "repository": change_set.gitops_repo,
                        "base_ref": change_set.gitops_ref,
                        "base_commit": base_commit,
                        "execution_artifact_id": execution.id,
                        "identity": "agent:git-observer",
                    })),
                })
                .await?
        }
        "failed" => state
            .store
            .create_artifact(CreateArtifact {
                id: format!("art_{}_gitops_base_revision", unique_suffix()),
                session_id: change_set.session_id.clone(),
                run_id: Some(change_set.run_id.clone()),
                kind: "gitops_base_revision".to_string(),
                label: format!("Failed GitOps base revision resolution for {}", change_set.id),
                mime_type: Some("application/json".to_string()),
                path: None,
                content_text: None,
                content_json: Some(json!({
                    "execution_id": request.execution_id,
                    "status": "failed",
                    "gitops_change_set_id": change_set.id,
                    "gitops_change_set_revision": change_set.revision,
                    "material_hash": change_set.material_hash,
                    "repository": change_set.gitops_repo,
                    "base_ref": change_set.gitops_ref,
                    "execution_artifact_id": execution.id,
                    "identity": "agent:git-observer",
                    "error_code": clean_optional_text(request.error_code).unwrap_or_else(|| "gitops_revision_resolver_failed".to_string()),
                })),
            })
            .await?,
        _ => {
            return Err(ApiError::bad_request(
                "GitOps base revision outcome status must be resolved or failed",
            ))
        }
    };
    append_gitops_change_set_audit_event(
        &state.store,
        &change_set,
        &format!("gitops_change_set.base_revision_{}", request.status),
        Some("agent:git-observer".to_string()),
        None,
        json!({ "execution_id": request.execution_id, "execution_artifact_id": execution.id, "result_artifact_id": result.id }),
    )
    .await?;
    Ok(Json(result.into()))
}

async fn current_gitops_base_revision_execution(
    state: &AppState,
    gitops_change_set_id: &str,
    execution_id: &str,
) -> Result<(StoredGitOpsChangeSet, StoredArtifact), ApiError> {
    let change_set = state
        .store
        .get_gitops_change_set(gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", gitops_change_set_id))?;
    let execution = state
        .store
        .list_artifacts(&change_set.run_id)
        .await?
        .into_iter()
        .find(|artifact| {
            artifact.kind == "gitops_base_revision_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                        && content.get("gitops_change_set_id").and_then(Value::as_str)
                            == Some(change_set.id.as_str())
                        && content.get("material_hash").and_then(Value::as_str)
                            == Some(change_set.material_hash.as_str())
                        && gitops_artifact_change_set_revision(content) == change_set.revision
                })
        })
        .ok_or_else(|| ApiError::conflict("GitOps base revision execution is not current"))?;
    Ok((change_set, execution))
}

#[derive(Debug, serde::Deserialize)]
struct InternalGitOpsDeliveryQuery {
    execution_id: String,
}

async fn internal_gitops_delivery_context(
    State(state): State<AppState>,
    Path(gitops_change_set_id): Path<String>,
    Query(query): Query<InternalGitOpsDeliveryQuery>,
) -> Result<Json<GitOpsDeliveryContextResponse>, ApiError> {
    if gitops_change_set_id.starts_with("rollback_") {
        return internal_rollback_delivery_context(
            &state,
            &gitops_change_set_id,
            &query.execution_id,
        )
        .await;
    }
    let (change_set, plan, _execution) =
        current_gitops_delivery_execution(&state, &gitops_change_set_id, &query.execution_id)
            .await?;
    let source = gitops_delivery_plan_source(&plan, &change_set)?;
    let settings = state.worker.gitops_writer_settings().ok_or_else(|| {
        ApiError::conflict("GitOps writer executor is not configured for delivery context")
    })?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &source.repository)
    {
        return Err(ApiError::conflict(
            "GitOps repository is not allowlisted for the dedicated GitOps writer",
        ));
    }
    Ok(Json(GitOpsDeliveryContextResponse {
        execution_id: query.execution_id,
        repository: source.repository,
        base_ref: source.base_ref,
        base_commit: source.base_commit,
        head_branch: source.head_branch,
        kustomization_path: source.kustomization_path,
        image_name: source.image_name,
        image_ref: source.image_ref,
        commit_subject: compact_delivery_subject(&change_set.title),
        commit_body: format!(
            "GitOps ChangeSet {} revision {}\n\n{}",
            change_set.id, change_set.revision, change_set.summary
        ),
        pull_request_title: compact_delivery_subject(&change_set.title),
        pull_request_body: format!(
            "{}\n\nPharness GitOps ChangeSet: {}\nWorkItem: {}",
            change_set.summary, change_set.id, change_set.work_item_id
        ),
        github_api_url: settings.github_api_url,
        author_name: settings.author_name,
        author_email: settings.author_email,
    }))
}

async fn internal_gitops_delivery_outcome(
    State(state): State<AppState>,
    Path(gitops_change_set_id): Path<String>,
    Json(request): Json<GitOpsDeliveryOutcomeRequest>,
) -> Result<Json<ArtifactResponse>, ApiError> {
    if gitops_change_set_id.starts_with("rollback_") {
        return internal_rollback_delivery_outcome(&state, &gitops_change_set_id, request).await;
    }
    let (change_set, plan, _execution) =
        current_gitops_delivery_execution(&state, &gitops_change_set_id, &request.execution_id)
            .await?;
    let result = match request.status.as_str() {
        "completed" => {
            let branch = clean_optional_text(request.branch).ok_or_else(|| {
                ApiError::bad_request("completed GitOps delivery requires branch")
            })?;
            let commit_sha = clean_optional_text(request.commit_sha).ok_or_else(|| {
                ApiError::bad_request("completed GitOps delivery requires commit_sha")
            })?;
            let pull_request_url =
                clean_optional_text(request.pull_request_url).ok_or_else(|| {
                    ApiError::bad_request("completed GitOps delivery requires pull_request_url")
                })?;
            let pull_request_number = request.pull_request_number.ok_or_else(|| {
                ApiError::bad_request("completed GitOps delivery requires pull_request_number")
            })?;
            let source = gitops_delivery_plan_source(&plan, &change_set)?;
            if branch != source.head_branch
                || !is_git_sha(&commit_sha)
                || !is_github_pr_url(&pull_request_url)
            {
                return Err(ApiError::conflict(
                    "GitOps delivery outcome does not match immutable branch or GitHub provenance",
                ));
            }
            let expected_pr_prefix = format!(
                "https://github.com/{}/pull/",
                source
                    .repository
                    .trim_start_matches("https://github.com/")
                    .trim_end_matches(".git")
            );
            if !pull_request_url.starts_with(&expected_pr_prefix)
                || !pull_request_url.ends_with(&pull_request_number.to_string())
            {
                return Err(ApiError::conflict(
                    "GitOps pull request does not match immutable repository provenance",
                ));
            }
            persist_gitops_delivery_result(
                &state.store,
                &change_set,
                &plan.id,
                &request.execution_id,
                "completed",
                json!({
                    "branch": branch,
                    "commit_sha": commit_sha,
                    "pull_request_url": pull_request_url,
                    "pull_request_number": pull_request_number,
                }),
            )
            .await?
        }
        "failed" => {
            persist_gitops_delivery_result(
                &state.store,
                &change_set,
                &plan.id,
                &request.execution_id,
                "failed",
                json!({
                    "error_code": clean_optional_text(request.error_code)
                        .unwrap_or_else(|| "gitops_writer_failed".to_string()),
                }),
            )
            .await?
        }
        _ => {
            return Err(ApiError::bad_request(
                "GitOps delivery outcome status must be completed or failed",
            ))
        }
    };
    append_gitops_change_set_audit_event(
        &state.store,
        &change_set,
        &format!("gitops_change_set.delivery_{}", request.status),
        Some(DEFAULT_GITOPS_WRITER_SUBJECT.to_string()),
        None,
        json!({
            "execution_id": request.execution_id,
            "gitops_delivery_plan_artifact_id": plan.id,
            "result_artifact_id": result.id,
        }),
    )
    .await?;
    Ok(Json(result))
}

async fn internal_gitops_delivery_observation_context(
    State(state): State<AppState>,
    Path(gitops_change_set_id): Path<String>,
    Query(query): Query<InternalGitOpsDeliveryQuery>,
) -> Result<Json<GitOpsDeliveryObservationContextResponse>, ApiError> {
    if gitops_change_set_id.starts_with("rollback_") {
        return internal_rollback_delivery_observation_context(
            &state,
            &gitops_change_set_id,
            &query.execution_id,
        )
        .await;
    }
    let (change_set, _plan, execution) =
        current_gitops_delivery_observation(&state, &gitops_change_set_id, &query.execution_id)
            .await?;
    let settings = state
        .worker
        .gitops_observer_settings()
        .ok_or_else(|| ApiError::conflict("GitOps observer executor is not configured"))?;
    let source = execution
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|content| content.get("source"))
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApiError::conflict("GitOps observation execution has no source provenance")
        })?;
    let repository = required_json_string(source, "repository", "GitOps observation source")?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &repository)
    {
        return Err(ApiError::conflict(
            "GitOps delivery repository is not allowlisted for the Git observer",
        ));
    }
    let _ = change_set;
    Ok(Json(GitOpsDeliveryObservationContextResponse {
        execution_id: query.execution_id,
        repository,
        head_branch: required_json_string(source, "head_branch", "GitOps observation source")?,
        source_commit_sha: required_json_string(
            source,
            "source_commit_sha",
            "GitOps observation source",
        )?,
        pull_request_url: required_json_string(
            source,
            "pull_request_url",
            "GitOps observation source",
        )?,
        pull_request_number: source
            .get("pull_request_number")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                ApiError::conflict("GitOps observation source has no pull-request number")
            })?,
        github_api_url: settings.github_api_url,
    }))
}

async fn internal_gitops_delivery_observation_outcome(
    State(state): State<AppState>,
    Path(gitops_change_set_id): Path<String>,
    Json(request): Json<GitOpsDeliveryObservationOutcomeRequest>,
) -> Result<Json<ArtifactResponse>, ApiError> {
    if gitops_change_set_id.starts_with("rollback_") {
        return internal_rollback_delivery_observation_outcome(
            &state,
            &gitops_change_set_id,
            request,
        )
        .await;
    }
    let (change_set, plan, execution) =
        current_gitops_delivery_observation(&state, &gitops_change_set_id, &request.execution_id)
            .await?;
    let expected = execution
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|content| content.get("source"))
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApiError::conflict("GitOps observation execution has no source provenance")
        })?;
    let artifact: ArtifactResponse = match request.status.as_str() {
        "observed" => {
            let state_value = clean_optional_text(request.pull_request_state).ok_or_else(|| ApiError::bad_request("observed GitOps outcome requires pull_request_state"))?;
            let merged = request.merged.ok_or_else(|| ApiError::bad_request("observed GitOps outcome requires merged"))?;
            let branch = clean_optional_text(request.head_branch).ok_or_else(|| ApiError::bad_request("observed GitOps outcome requires head_branch"))?;
            let commit = clean_optional_text(request.head_commit_sha).ok_or_else(|| ApiError::bad_request("observed GitOps outcome requires head_commit_sha"))?;
            if !matches!(state_value.as_str(), "open" | "closed") || !is_git_sha(&commit)
                || expected.get("head_branch").and_then(Value::as_str) != Some(branch.as_str())
                || expected.get("source_commit_sha").and_then(Value::as_str) != Some(commit.as_str()) {
                return Err(ApiError::conflict("GitOps observation does not match the delivered branch commit"));
            }
            let merge = clean_optional_text(request.merge_commit_sha);
            if merged && (state_value != "closed" || !merge.as_deref().is_some_and(is_git_sha)) {
                return Err(ApiError::bad_request("merged GitOps outcome has invalid merge provenance"));
            }
            if !merged && merge.is_some() { return Err(ApiError::bad_request("unmerged GitOps outcome must not include merge_commit_sha")); }
            let observation = state.store.create_artifact(CreateArtifact { id:format!("art_{}_gitops_delivery_pr_observation",unique_suffix()),session_id:change_set.session_id.clone(),run_id:Some(change_set.run_id.clone()),kind:"gitops_delivery_pr_observation".to_string(),label:format!("GitOps PR observation for {}",change_set.id),mime_type:Some("application/json".to_string()),path:None,content_text:None,content_json:Some(json!({"execution_id":request.execution_id,"status":"observed","gitops_change_set_id":change_set.id,"gitops_delivery_plan_artifact_id":plan.id,"pull_request_state":state_value,"merged":merged,"head_branch":branch,"head_commit_sha":commit,"merge_commit_sha":merge})) }).await?;
            if let Some(merge_sha) = merge { state.store.create_artifact(CreateArtifact { id:format!("art_{}_gitops_delivery_merge",unique_suffix()),session_id:change_set.session_id.clone(),run_id:Some(change_set.run_id.clone()),kind:"gitops_delivery_merge".to_string(),label:format!("Immutable GitOps merge for {}",change_set.id),mime_type:Some("application/json".to_string()),path:None,content_text:None,content_json:Some(json!({"execution_id":request.execution_id,"gitops_change_set_id":change_set.id,"gitops_delivery_plan_artifact_id":plan.id,"pull_request_url":expected.get("pull_request_url"),"pull_request_number":expected.get("pull_request_number"),"head_commit_sha":commit,"merge_commit_sha":merge_sha})) }).await?; }
            observation.into()
        }
        "failed" => state.store.create_artifact(CreateArtifact { id:format!("art_{}_gitops_delivery_pr_observation",unique_suffix()),session_id:change_set.session_id.clone(),run_id:Some(change_set.run_id.clone()),kind:"gitops_delivery_pr_observation".to_string(),label:format!("Failed GitOps PR observation for {}",change_set.id),mime_type:Some("application/json".to_string()),path:None,content_text:None,content_json:Some(json!({"execution_id":request.execution_id,"status":"failed","gitops_change_set_id":change_set.id,"gitops_delivery_plan_artifact_id":plan.id,"error_code":clean_optional_text(request.error_code).unwrap_or_else(|| "gitops_observer_failed".to_string())})) }).await?.into(),
        _ => return Err(ApiError::bad_request("GitOps observation outcome status must be observed or failed")),
    };
    append_gitops_change_set_audit_event(&state.store,&change_set,&format!("gitops_change_set.delivery_observation_{}",request.status),Some("agent:git-observer".to_string()),None,json!({"execution_id":request.execution_id,"gitops_delivery_plan_artifact_id":plan.id,"observation_artifact_id":artifact.id})).await?;
    Ok(Json(artifact))
}

async fn internal_rollback_delivery_context(
    state: &AppState,
    rollback_intent_id: &str,
    execution_id: &str,
) -> Result<Json<GitOpsDeliveryContextResponse>, ApiError> {
    let (_item, _intent, run) = rollback_intent_context(state, rollback_intent_id).await?;
    let execution = state
        .store
        .list_artifacts(&run.id)
        .await?
        .into_iter()
        .find(|artifact| {
            artifact.kind == "rollback_delivery_execution"
                && artifact.content_json.as_ref().is_some_and(|value| {
                    value.get("rollback_intent_id").and_then(Value::as_str)
                        == Some(rollback_intent_id)
                        && value.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                })
        })
        .ok_or_else(|| ApiError::conflict("rollback delivery execution is not current"))?;
    let context = execution
        .content_json
        .as_ref()
        .and_then(|value| value.get("context"))
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApiError::conflict("rollback delivery execution has no immutable context")
        })?;
    let repository = required_json_string(context, "repository", "rollback delivery context")?;
    let settings = state
        .worker
        .gitops_writer_settings()
        .ok_or_else(|| ApiError::conflict("GitOps rollback writer is not configured"))?;
    if repository != PROTECTED_GITOPS_REPO
        || !settings
            .allowed_repos
            .iter()
            .any(|repo| repo == &repository)
    {
        return Err(ApiError::conflict(
            "rollback repository is not allowlisted for the isolated GitOps writer",
        ));
    }
    let response = GitOpsDeliveryContextResponse {
        execution_id: required_json_string(context, "execution_id", "rollback delivery context")?,
        repository,
        base_ref: required_json_string(context, "base_ref", "rollback delivery context")?,
        base_commit: required_json_string(context, "base_commit", "rollback delivery context")?,
        head_branch: required_json_string(context, "head_branch", "rollback delivery context")?,
        kustomization_path: required_json_string(
            context,
            "kustomization_path",
            "rollback delivery context",
        )?,
        image_name: required_json_string(context, "image_name", "rollback delivery context")?,
        image_ref: required_json_string(context, "image_ref", "rollback delivery context")?,
        commit_subject: required_json_string(
            context,
            "commit_subject",
            "rollback delivery context",
        )?,
        commit_body: required_json_string(context, "commit_body", "rollback delivery context")?,
        pull_request_title: required_json_string(
            context,
            "pull_request_title",
            "rollback delivery context",
        )?,
        pull_request_body: required_json_string(
            context,
            "pull_request_body",
            "rollback delivery context",
        )?,
        github_api_url: settings.github_api_url,
        author_name: settings.author_name,
        author_email: settings.author_email,
    };
    if response.execution_id != execution_id
        || response.kustomization_path != PROTECTED_KUSTOMIZATION_PATH
        || response.image_name != PROTECTED_IMAGE_NAME
        || !response
            .image_ref
            .starts_with(&format!("{PROTECTED_IMAGE_NAME}@sha256:"))
        || !is_git_sha(&response.base_commit)
    {
        return Err(ApiError::conflict(
            "rollback delivery context no longer matches the protected target",
        ));
    }
    Ok(Json(response))
}

async fn internal_rollback_delivery_outcome(
    state: &AppState,
    rollback_intent_id: &str,
    request: GitOpsDeliveryOutcomeRequest,
) -> Result<Json<ArtifactResponse>, ApiError> {
    let Json(context) =
        internal_rollback_delivery_context(state, rollback_intent_id, &request.execution_id)
            .await?;
    let (_item, current, run) = rollback_intent_context(state, rollback_intent_id).await?;
    let result = match request.status.as_str() {
        "completed" => {
            let branch = clean_optional_text(request.branch).ok_or_else(|| ApiError::bad_request("completed rollback delivery requires branch"))?;
            let commit_sha = clean_optional_text(request.commit_sha).ok_or_else(|| ApiError::bad_request("completed rollback delivery requires commit_sha"))?;
            let pull_request_url = clean_optional_text(request.pull_request_url).ok_or_else(|| ApiError::bad_request("completed rollback delivery requires pull_request_url"))?;
            let pull_request_number = request.pull_request_number.ok_or_else(|| ApiError::bad_request("completed rollback delivery requires pull_request_number"))?;
            let expected_prefix = "https://github.com/lward27/lucas_engineering/pull/";
            if branch != context.head_branch || !is_git_sha(&commit_sha) || !pull_request_url.starts_with(expected_prefix) || !pull_request_url.ends_with(&pull_request_number.to_string()) {
                return Err(ApiError::conflict("rollback delivery outcome does not match immutable GitHub provenance"));
            }
            append_rollback_delivery_result(state, &run, rollback_intent_id, &request.execution_id, "completed", json!({ "branch": branch, "commit_sha": commit_sha, "pull_request_url": pull_request_url, "pull_request_number": pull_request_number })).await?
        }
        "failed" => append_rollback_delivery_result(state, &run, rollback_intent_id, &request.execution_id, "failed", json!({ "error_code": clean_optional_text(request.error_code).unwrap_or_else(|| "gitops_writer_failed".to_string()) })).await?,
        _ => return Err(ApiError::bad_request("rollback delivery outcome status must be completed or failed")),
    };
    if request.status == "completed" {
        let mut content = current
            .get("content")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| ApiError::conflict("RollbackIntent content is unavailable"))?;
        content.insert("status".to_string(), json!("awaiting_manual_merge"));
        content.insert(
            "pull_request".to_string(),
            result
                .content_json
                .as_ref()
                .and_then(|value| value.get("details"))
                .cloned()
                .unwrap_or(Value::Null),
        );
        append_rollback_intent_artifact(state, &run, &Value::Object(content)).await?;
    } else if request.status == "failed" {
        let mut content = current
            .get("content")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| ApiError::conflict("RollbackIntent content is unavailable"))?;
        content.insert("status".to_string(), json!("attention_required"));
        content.insert("writer_failure_result_id".to_string(), json!(result.id));
        append_rollback_intent_artifact(state, &run, &Value::Object(content)).await?;
    }
    Ok(Json(result.into()))
}

async fn internal_rollback_delivery_observation_context(
    state: &AppState,
    rollback_intent_id: &str,
    execution_id: &str,
) -> Result<Json<GitOpsDeliveryObservationContextResponse>, ApiError> {
    let (_item, _intent, run) = rollback_intent_context(state, rollback_intent_id).await?;
    let execution = state
        .store
        .list_artifacts(&run.id)
        .await?
        .into_iter()
        .find(|artifact| {
            artifact.kind == "rollback_delivery_observation_execution"
                && artifact.content_json.as_ref().is_some_and(|value| {
                    value.get("rollback_intent_id").and_then(Value::as_str)
                        == Some(rollback_intent_id)
                        && value.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                })
        })
        .ok_or_else(|| ApiError::conflict("rollback observation execution is not current"))?;
    let source = execution
        .content_json
        .as_ref()
        .and_then(|value| value.get("source"))
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("rollback observation has no source provenance"))?;
    let repository = required_json_string(source, "repository", "rollback observation source")?;
    let settings = state
        .worker
        .gitops_observer_settings()
        .ok_or_else(|| ApiError::conflict("GitOps rollback observer is not configured"))?;
    if repository != PROTECTED_GITOPS_REPO
        || !settings
            .allowed_repos
            .iter()
            .any(|repo| repo == &repository)
    {
        return Err(ApiError::conflict(
            "rollback repository is not allowlisted for the isolated GitOps observer",
        ));
    }
    Ok(Json(GitOpsDeliveryObservationContextResponse {
        execution_id: execution_id.to_string(),
        repository,
        head_branch: required_json_string(source, "head_branch", "rollback observation source")?,
        source_commit_sha: required_json_string(
            source,
            "source_commit_sha",
            "rollback observation source",
        )?,
        pull_request_url: required_json_string(
            source,
            "pull_request_url",
            "rollback observation source",
        )?,
        pull_request_number: source
            .get("pull_request_number")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                ApiError::conflict("rollback observation source has no pull-request number")
            })?,
        github_api_url: settings.github_api_url,
    }))
}

async fn internal_rollback_delivery_observation_outcome(
    state: &AppState,
    rollback_intent_id: &str,
    request: GitOpsDeliveryObservationOutcomeRequest,
) -> Result<Json<ArtifactResponse>, ApiError> {
    let Json(context) = internal_rollback_delivery_observation_context(
        state,
        rollback_intent_id,
        &request.execution_id,
    )
    .await?;
    let (_item, current, run) = rollback_intent_context(state, rollback_intent_id).await?;
    let artifact = match request.status.as_str() {
        "observed" => {
            let pull_request_state = clean_optional_text(request.pull_request_state).ok_or_else(|| ApiError::bad_request("observed rollback outcome requires pull_request_state"))?;
            let merged = request.merged.ok_or_else(|| ApiError::bad_request("observed rollback outcome requires merged"))?;
            let head_branch = clean_optional_text(request.head_branch).ok_or_else(|| ApiError::bad_request("observed rollback outcome requires head_branch"))?;
            let head_commit_sha = clean_optional_text(request.head_commit_sha).ok_or_else(|| ApiError::bad_request("observed rollback outcome requires head_commit_sha"))?;
            let merge_commit_sha = clean_optional_text(request.merge_commit_sha);
            if !matches!(pull_request_state.as_str(), "open" | "closed") || head_branch != context.head_branch || head_commit_sha != context.source_commit_sha || !is_git_sha(&head_commit_sha) || (merged && (pull_request_state != "closed" || !merge_commit_sha.as_deref().is_some_and(is_git_sha))) || (!merged && merge_commit_sha.is_some()) {
                return Err(ApiError::conflict("rollback observation does not match the delivered immutable pull request"));
            }
            append_rollback_delivery_observation(state, &run, rollback_intent_id, &request.execution_id, "observed", json!({ "pull_request_state": pull_request_state, "merged": merged, "head_branch": head_branch, "head_commit_sha": head_commit_sha, "merge_commit_sha": merge_commit_sha })).await?
        }
        "failed" => append_rollback_delivery_observation(state, &run, rollback_intent_id, &request.execution_id, "failed", json!({ "error_code": clean_optional_text(request.error_code).unwrap_or_else(|| "gitops_observer_failed".to_string()) })).await?,
        _ => return Err(ApiError::bad_request("rollback observation outcome status must be observed or failed")),
    };
    if request.status == "observed" {
        let merged = artifact
            .content_json
            .as_ref()
            .and_then(|value| value.get("merged"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut content = current
            .get("content")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| ApiError::conflict("RollbackIntent content is unavailable"))?;
        content.insert(
            "status".to_string(),
            json!(if merged {
                "ready_for_argo_sync"
            } else {
                "awaiting_manual_merge"
            }),
        );
        if merged {
            content.insert(
                "gitops_merge_sha".to_string(),
                artifact
                    .content_json
                    .as_ref()
                    .and_then(|value| value.get("merge_commit_sha"))
                    .cloned()
                    .unwrap_or(Value::Null),
            );
        }
        append_rollback_intent_artifact(state, &run, &Value::Object(content)).await?;
    } else if request.status == "failed" {
        let mut content = current
            .get("content")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| ApiError::conflict("RollbackIntent content is unavailable"))?;
        content.insert("status".to_string(), json!("attention_required"));
        content.insert("observer_failure_result_id".to_string(), json!(artifact.id));
        append_rollback_intent_artifact(state, &run, &Value::Object(content)).await?;
    }
    Ok(Json(artifact.into()))
}

async fn current_gitops_delivery_observation(
    state: &AppState,
    gitops_change_set_id: &str,
    execution_id: &str,
) -> Result<(StoredGitOpsChangeSet, StoredArtifact, StoredArtifact), ApiError> {
    let change_set = state
        .store
        .get_gitops_change_set(gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", gitops_change_set_id))?;
    let artifacts = state.store.list_artifacts(&change_set.run_id).await?;
    let execution = artifacts
        .iter()
        .find(|artifact| {
            artifact.kind == "gitops_delivery_observation_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                })
        })
        .cloned()
        .ok_or_else(|| ApiError::conflict("GitOps observation execution is not current"))?;
    let plan_id = execution
        .content_json
        .as_ref()
        .and_then(|content| content.get("gitops_delivery_plan_artifact_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("GitOps observation execution has no plan provenance"))?;
    let plan = artifacts
        .into_iter()
        .find(|artifact| {
            artifact.id == plan_id && gitops_delivery_plan_matches_change_set(artifact, &change_set)
        })
        .ok_or_else(|| {
            ApiError::conflict("GitOps observation execution plan is no longer current")
        })?;
    Ok((change_set, plan, execution))
}

async fn current_gitops_delivery_execution(
    state: &AppState,
    gitops_change_set_id: &str,
    execution_id: &str,
) -> Result<(StoredGitOpsChangeSet, StoredArtifact, StoredArtifact), ApiError> {
    let change_set = state
        .store
        .get_gitops_change_set(gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", gitops_change_set_id))?;
    let artifacts = state.store.list_artifacts(&change_set.run_id).await?;
    let execution = artifacts
        .iter()
        .find(|artifact| {
            artifact.kind == "gitops_delivery_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                        && content.get("gitops_change_set_id").and_then(Value::as_str)
                            == Some(change_set.id.as_str())
                })
        })
        .cloned()
        .ok_or_else(|| ApiError::conflict("GitOps delivery execution is not current"))?;
    let plan_id = execution
        .content_json
        .as_ref()
        .and_then(|content| content.get("gitops_delivery_plan_artifact_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("GitOps delivery execution has no plan provenance"))?;
    let plan = artifacts
        .into_iter()
        .find(|artifact| {
            artifact.id == plan_id && gitops_delivery_plan_matches_change_set(artifact, &change_set)
        })
        .ok_or_else(|| ApiError::conflict("GitOps delivery execution plan is no longer current"))?;
    Ok((change_set, plan, execution))
}

#[derive(Debug, Clone)]
struct GitOpsDeliveryPlanSource {
    repository: String,
    base_ref: String,
    base_commit: String,
    head_branch: String,
    kustomization_path: String,
    image_name: String,
    image_ref: String,
}

fn gitops_delivery_plan_source(
    plan: &StoredArtifact,
    change_set: &StoredGitOpsChangeSet,
) -> Result<GitOpsDeliveryPlanSource, ApiError> {
    let content = plan
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("GitOps delivery plan has no structured content"))?;
    if content.get("operation").and_then(Value::as_str) != Some("branch_and_pull_request") {
        return Err(ApiError::conflict(
            "GitOps delivery plan does not describe a branch-and-pull-request operation",
        ));
    }
    let source = content
        .get("source")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("GitOps delivery plan has no source provenance"))?;
    let update = content
        .get("update")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("GitOps delivery plan has no update operation"))?;
    let result = GitOpsDeliveryPlanSource {
        repository: required_json_string(source, "repository", "GitOps delivery plan source")?,
        base_ref: required_json_string(source, "base_ref", "GitOps delivery plan source")?,
        base_commit: required_json_string(source, "base_commit", "GitOps delivery plan source")?,
        head_branch: required_json_string(source, "head_branch", "GitOps delivery plan source")?,
        kustomization_path: required_json_string(
            update,
            "kustomization_path",
            "GitOps delivery plan update",
        )?,
        image_name: required_json_string(update, "image_name", "GitOps delivery plan update")?,
        image_ref: required_json_string(update, "new_image", "GitOps delivery plan update")?,
    };
    if result.repository != change_set.gitops_repo
        || result.base_ref != change_set.gitops_ref
        || result.head_branch != change_set.head_branch
        || result.kustomization_path != change_set.kustomization_path
        || result.image_name != change_set.image_name
        || result.image_ref != change_set.image_ref
        || !is_git_sha(&result.base_commit)
        || !safe_relative_gitops_path(&result.kustomization_path)
        || !result.image_ref.contains("@sha256:")
    {
        return Err(ApiError::conflict(
            "GitOps delivery plan no longer matches the immutable ChangeSet target",
        ));
    }
    Ok(result)
}

fn gitops_delivery_artifact_matches_plan(
    artifact: &StoredArtifact,
    kind: &str,
    plan_id: &str,
) -> bool {
    artifact.kind == kind
        && artifact.content_json.as_ref().is_some_and(|content| {
            content
                .get("gitops_delivery_plan_artifact_id")
                .and_then(Value::as_str)
                == Some(plan_id)
        })
}

async fn persist_gitops_delivery_result(
    store: &SqliteStore,
    change_set: &StoredGitOpsChangeSet,
    plan_id: &str,
    execution_id: &str,
    status: &str,
    details: Value,
) -> Result<ArtifactResponse, ApiError> {
    Ok(store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_gitops_delivery_result", unique_suffix()),
            session_id: change_set.session_id.clone(),
            run_id: Some(change_set.run_id.clone()),
            kind: "gitops_delivery_result".to_string(),
            label: format!("GitOps delivery {} for {}", status, change_set.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": execution_id,
                "status": status,
                "gitops_change_set_id": change_set.id,
                "gitops_delivery_plan_artifact_id": plan_id,
                "details": details,
            })),
        })
        .await?
        .into())
}

async fn current_git_delivery_execution(
    state: &AppState,
    change_set_id: &str,
    execution_id: &str,
) -> Result<(StoredChangeSet, RunId, StoredArtifact, StoredArtifact), ApiError> {
    let change_set = state
        .store
        .get_change_set(change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", change_set_id))?;
    let run_id = change_set
        .run_id
        .clone()
        .ok_or_else(|| ApiError::conflict("Git delivery ChangeSet has no coding run provenance"))?;
    let artifacts = state.store.list_artifacts(&run_id).await?;
    let execution = artifacts
        .iter()
        .find(|artifact| {
            artifact.kind == "git_delivery_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                })
        })
        .cloned()
        .ok_or_else(|| ApiError::conflict("Git delivery execution is not current"))?;
    let plan_id = execution
        .content_json
        .as_ref()
        .and_then(|value| value.get("git_delivery_plan_artifact_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("Git delivery execution has no plan provenance"))?;
    let plan = artifacts
        .into_iter()
        .find(|artifact| {
            artifact.id == plan_id && git_delivery_plan_matches_change_set(artifact, &change_set)
        })
        .ok_or_else(|| ApiError::conflict("Git delivery execution plan is no longer current"))?;
    Ok((change_set, run_id, plan, execution))
}

async fn current_git_delivery_observation(
    state: &AppState,
    change_set_id: &str,
    execution_id: &str,
) -> Result<(StoredChangeSet, RunId, StoredArtifact, StoredArtifact), ApiError> {
    let change_set = state
        .store
        .get_change_set(change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", change_set_id))?;
    let run_id = change_set.run_id.clone().ok_or_else(|| {
        ApiError::conflict("Git observation ChangeSet has no coding run provenance")
    })?;
    let artifacts = state.store.list_artifacts(&run_id).await?;
    let execution = artifacts
        .iter()
        .find(|artifact| {
            artifact.kind == "git_delivery_observation_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                })
        })
        .cloned()
        .ok_or_else(|| ApiError::conflict("Git observation execution is not current"))?;
    let plan_id = execution
        .content_json
        .as_ref()
        .and_then(|value| value.get("git_delivery_plan_artifact_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("Git observation execution has no plan provenance"))?;
    let plan = artifacts
        .into_iter()
        .find(|artifact| {
            artifact.id == plan_id && git_delivery_plan_matches_change_set(artifact, &change_set)
        })
        .ok_or_else(|| ApiError::conflict("Git observation execution plan is no longer current"))?;
    Ok((change_set, run_id, plan, execution))
}

async fn persist_git_delivery_result(
    store: &SqliteStore,
    change_set: &StoredChangeSet,
    run_id: &RunId,
    plan_id: &str,
    execution_id: &str,
    status: &str,
    details: Value,
) -> Result<ArtifactResponse, ApiError> {
    Ok(store.create_artifact(CreateArtifact {
        id: format!("art_{}_git_delivery_result", unique_suffix()),
        session_id: change_set.session_id.clone(), run_id: Some(run_id.clone()),
        kind: "git_delivery_result".to_string(), label: format!("Git delivery {} for ChangeSet {}", status, change_set.id),
        mime_type: Some("application/json".to_string()), path: None, content_text: None,
        content_json: Some(json!({ "execution_id": execution_id, "status": status, "change_set_id": change_set.id, "git_delivery_plan_artifact_id": plan_id, "details": details })),
    }).await?.into())
}

fn is_git_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn is_github_pr_url(value: &str) -> bool {
    let parts = value
        .strip_prefix("https://github.com/")
        .map(|value| value.split('/').collect::<Vec<_>>());
    matches!(parts, Some(parts) if parts.len() == 4 && parts[2] == "pull" && parts[3].parse::<u64>().is_ok())
}

#[derive(Debug, Clone)]
struct GitDeliveryPlanSource {
    repository: String,
    base_ref: String,
    base_commit: String,
    head_branch: String,
    workspace_id: String,
}

fn git_delivery_plan_source(
    plan: &StoredArtifact,
    work_item: &StoredWorkItem,
) -> Result<GitDeliveryPlanSource, ApiError> {
    let plan_json = plan
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git delivery plan has no structured content"))?;
    if plan_json.get("operation").and_then(Value::as_str) != Some("branch_and_pull_request") {
        return Err(ApiError::conflict(
            "Git delivery plan does not describe a branch-and-pull-request operation",
        ));
    }
    let source = plan_json
        .get("source")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git delivery plan has no source provenance"))?;
    let repository = required_json_string(source, "repository", "Git delivery plan source")?;
    let base_ref = required_json_string(source, "base_ref", "Git delivery plan source")?;
    let base_commit = required_json_string(source, "base_commit", "Git delivery plan source")?;
    let head_branch = required_json_string(source, "head_branch", "Git delivery plan source")?;
    let workspace_id = required_json_string(source, "workspace_id", "Git delivery plan source")?;
    WorkspaceSourceSpec {
        workspace_id: workspace_id.clone(),
        source_repo: repository.clone(),
        source_ref: base_ref.clone(),
        source_commit: None,
        branch: head_branch.clone(),
        resolved_commit: Some(base_commit.clone()),
    }
    .validate()
    .map_err(|error| ApiError::conflict(error.to_string()))?;
    if repository != work_item.source_repo || base_ref != work_item.source_ref {
        return Err(ApiError::conflict(
            "Git delivery plan source does not match the WorkItem target",
        ));
    }
    Ok(GitDeliveryPlanSource {
        repository,
        base_ref,
        base_commit,
        head_branch,
        workspace_id,
    })
}

async fn current_git_delivery_plan(
    store: &SqliteStore,
    run_id: &RunId,
    change_set: &StoredChangeSet,
) -> Result<StoredArtifact, ApiError> {
    store
        .list_artifacts(run_id)
        .await?
        .into_iter()
        .find(|artifact| git_delivery_plan_matches_change_set(artifact, change_set))
        .ok_or_else(|| {
            ApiError::conflict(
                "ChangeSet needs a current immutable Git delivery plan before authorization",
            )
        })
}

fn git_delivery_plan_matches_change_set(
    artifact: &StoredArtifact,
    change_set: &StoredChangeSet,
) -> bool {
    artifact.kind == "git_delivery_plan"
        && artifact.content_json.as_ref().is_some_and(|plan| {
            plan.get("change_set")
                .and_then(|value| value.get("id"))
                .and_then(Value::as_str)
                == Some(change_set.id.as_str())
                && plan
                    .get("change_set")
                    .and_then(|value| value.get("revision"))
                    .and_then(Value::as_i64)
                    == Some(change_set.revision)
                && plan
                    .get("change_set")
                    .and_then(|value| value.get("material_hash"))
                    .and_then(Value::as_str)
                    == Some(change_set.material_hash.as_str())
        })
}

async fn matching_git_delivery_grant(
    store: &SqliteStore,
    subject: &str,
    change_set: &StoredChangeSet,
    work_item: &StoredWorkItem,
    branch: &str,
    plan_artifact_id: &str,
) -> Result<Option<StoredPermissionGrant>, ApiError> {
    let now = current_millis();
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
        let policy = serde_json::from_value::<PermissionGrantPolicy>(grant.policy_json.clone())
            .map_err(|error| {
                ApiError::internal(format!(
                    "permission grant {} has invalid policy: {error}",
                    grant.id
                ))
            })?;
        let has_all_actions = GIT_DELIVERY_ACTIONS
            .iter()
            .all(|action| scope.actions.iter().any(|allowed| allowed == action));
        let matches = grant.subject == subject
            && policy.policy_mode == PolicyMode::SupervisedAutonomy
            && scope.environment.as_deref() == Some(work_item.target_environment.as_str())
            && scope.capability_kinds == vec![CapabilityKind::Git]
            && scope.actions.len() == GIT_DELIVERY_ACTIONS.len()
            && has_all_actions
            && scope
                .max_risk
                .is_some_and(|risk| risk_rank(risk) >= risk_rank(RiskLevel::High))
            && scope.repos == vec![work_item.source_repo.clone()]
            && scope.branches == vec![branch.to_string()]
            && scope.work_plan_ids == vec![change_set.work_plan_id.clone()]
            && scope.change_set_ids == vec![change_set.id.clone()]
            && scope.git_delivery_plan_artifact_ids == vec![plan_artifact_id.to_string()]
            && scope.production_impacting == Some(work_item.production_impacting);
        if matches {
            return Ok(Some(grant));
        }
    }
    Ok(None)
}

async fn matching_gitops_delivery_grant(
    store: &SqliteStore,
    subject: &str,
    change_set: &StoredGitOpsChangeSet,
    work_item: &StoredWorkItem,
    plan_artifact_id: &str,
) -> Result<Option<StoredPermissionGrant>, ApiError> {
    let now = current_millis();
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
        let policy = serde_json::from_value::<PermissionGrantPolicy>(grant.policy_json.clone())
            .map_err(|error| {
                ApiError::internal(format!(
                    "permission grant {} has invalid policy: {error}",
                    grant.id
                ))
            })?;
        let has_all_actions = GITOPS_DELIVERY_ACTIONS
            .iter()
            .all(|action| scope.actions.iter().any(|allowed| allowed == action));
        let matches = grant.subject == subject
            && policy.policy_mode == PolicyMode::SupervisedAutonomy
            && scope.environment.as_deref() == Some(work_item.target_environment.as_str())
            && scope.capability_kinds == vec![CapabilityKind::Git]
            && scope.actions.len() == GITOPS_DELIVERY_ACTIONS.len()
            && has_all_actions
            && scope
                .max_risk
                .is_some_and(|risk| risk_rank(risk) >= risk_rank(RiskLevel::High))
            && scope.repos == vec![change_set.gitops_repo.clone()]
            && scope.branches == vec![change_set.head_branch.clone()]
            && scope.work_plan_ids == vec![change_set.work_plan_id.clone()]
            && scope.gitops_change_set_ids == vec![change_set.id.clone()]
            && scope.gitops_delivery_plan_artifact_ids == vec![plan_artifact_id.to_string()]
            && scope.production_impacting == Some(work_item.production_impacting)
            && work_item.gitops_repo.as_deref() == Some(change_set.gitops_repo.as_str())
            && work_item.gitops_ref.as_deref() == Some(change_set.gitops_ref.as_str());
        if matches {
            return Ok(Some(grant));
        }
    }
    Ok(None)
}

fn required_json_string(
    object: &Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<String, ApiError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| ApiError::conflict(format!("{label} is missing {key}")))
}

fn compact_delivery_subject(value: &str) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let subject = if compact.is_empty() {
        "Pharness ChangeSet".to_string()
    } else {
        compact
    };
    subject.chars().take(72).collect()
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
        if let Some(remediation_plan_id) = &change_set.remediation_plan_id {
            state
                .store
                .stale_approval_gates_for_remediation_plan(
                    remediation_plan_id,
                    actor.clone(),
                    reason.clone().or_else(|| {
                        Some(format!(
                            "change set {} revised from revision {} to {}",
                            change_set.id, current.revision, change_set.revision
                        ))
                    }),
                )
                .await?
        } else if let Some(work_item_id) = &change_set.work_item_id {
            state
                .store
                .stale_approval_gates_for_work_item(
                    work_item_id,
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
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GitOpsChangeSetStatus {
    Proposed,
    Approved,
    Rejected,
    Applied,
    Stale,
}

impl GitOpsChangeSetStatus {
    fn parse(value: &str) -> Result<Self, ApiError> {
        match value {
            "proposed" => Ok(Self::Proposed),
            "approved" => Ok(Self::Approved),
            "rejected" => Ok(Self::Rejected),
            "applied" => Ok(Self::Applied),
            "stale" => Ok(Self::Stale),
            other => Err(ApiError::bad_request(format!(
                "unsupported GitOps change set status: {other}"
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Applied => "applied",
            Self::Stale => "stale",
        }
    }

    fn ensure_can_transition_to(self, target: Self) -> Result<(), ApiError> {
        let allowed = match self {
            Self::Proposed => matches!(target, Self::Approved | Self::Rejected | Self::Stale),
            Self::Approved => matches!(target, Self::Applied | Self::Rejected | Self::Stale),
            Self::Rejected | Self::Applied | Self::Stale => false,
        };
        if allowed {
            Ok(())
        } else {
            Err(ApiError::conflict(format!(
                "cannot transition GitOps change set from {} to {}",
                self.as_str(),
                target.as_str()
            )))
        }
    }
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

async fn append_gitops_change_set_audit_event(
    store: &SqliteStore,
    change_set: &StoredGitOpsChangeSet,
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
            resource_kind: "gitops_change_set".to_string(),
            resource_id: change_set.id.clone(),
            run_id: Some(change_set.run_id.clone()),
            payload_json: json!({
                "gitops_change_set_id": change_set.id,
                "work_item_id": change_set.work_item_id,
                "work_plan_id": change_set.work_plan_id,
                "source_change_set_id": change_set.source_change_set_id,
                "pipeline_intent_id": change_set.pipeline_intent_id,
                "deployment_intent_id": change_set.deployment_intent_id,
                "gitops_update_plan_artifact_id": change_set.gitops_update_plan_artifact_id,
                "run_id": change_set.run_id.as_str(),
                "status": change_set.status,
                "material_hash": change_set.material_hash,
                "gitops": {
                    "repository": change_set.gitops_repo,
                    "base_ref": change_set.gitops_ref,
                    "head_branch": change_set.head_branch,
                    "kustomization_path": change_set.kustomization_path,
                    "image_name": change_set.image_name,
                    "image_ref": change_set.image_ref,
                },
                "reason": reason,
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

async fn append_deployment_contract_audit_event(
    store: &SqliteStore,
    contract: &StoredDeploymentContract,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", contract.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "deployment_contract".to_string(),
            resource_id: contract.id.clone(),
            run_id: None,
            payload_json: json!({
                "deployment_contract_id": contract.id,
                "status": contract.status,
                "target_environment": contract.target_environment,
                "target_namespace": contract.target_namespace,
                "argo_application": contract.argo_application,
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
                "work_item_id": plan.work_item_id,
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

async fn append_work_item_audit_event(
    store: &SqliteStore,
    item: &StoredWorkItem,
    kind: &str,
    actor: Option<String>,
    extra: serde_json::Value,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", item.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "work_item".to_string(),
            resource_id: item.id.clone(),
            run_id: item.current_run_id.clone(),
            payload_json: json!({
                "work_item_id": item.id,
                "status": item.status,
                "title": item.title,
                "intent": item.intent,
                "source": { "repo": item.source_repo, "ref": item.source_ref },
                "target": {
                    "environment": item.target_environment,
                    "namespace": item.target_namespace,
                    "argo_application": item.argo_application,
                    "production_impacting": item.production_impacting,
                },
                "budget": {
                    "attempts": item.max_attempts,
                    "elapsed_seconds": item.max_elapsed_seconds,
                },
                "extra": extra,
            }),
        })
        .await
        .map(|_| ())
}

async fn append_controller_wait_audit_event(
    store: &SqliteStore,
    wait: &StoredControllerWait,
    kind: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", wait.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "controller_wait".to_string(),
            resource_id: wait.id.clone(),
            run_id: wait.run_id.clone(),
            payload_json: json!({
                "controller_wait_id": wait.id,
                "work_item_id": wait.work_item_id,
                "run_id": wait.run_id.as_ref().map(RunId::as_str),
                "status": wait.status,
                "wait_kind": wait.wait_kind,
                "subject": { "kind": wait.subject_kind, "id": wait.subject_id },
                "next_check_at": wait.next_check_at,
                "deadline_at": wait.deadline_at,
                "max_checks": wait.max_checks,
                "check_count": wait.check_count,
                "reason": reason,
                "automatic_execution": false,
                "automatic_retry": false,
                "automatic_rollback": false,
            }),
        })
        .await
        .map(|_| ())
}

async fn append_workspace_audit_event(
    store: &SqliteStore,
    workspace: &pharness_store::StoredWorkspace,
    kind: &str,
    actor: Option<String>,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", workspace.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "workspace".to_string(),
            resource_id: workspace.id.clone(),
            run_id: workspace.run_id.clone(),
            payload_json: json!({
                "workspace_id": workspace.id,
                "work_item_id": workspace.work_item_id,
                "status": workspace.status,
                "source": { "repo": workspace.source_repo, "ref": workspace.source_ref },
                "resolved_commit": workspace.resolved_commit,
                "branch": workspace.branch,
                "retention_status": workspace.retention_status,
            }),
        })
        .await
        .map(|_| ())
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

fn run_budget_from_request(request: &CreateWorkItemRequest) -> Result<RunBudget, ApiError> {
    let defaults = RunBudget::default();
    let budget = RunBudget {
        initial_turns: request
            .initial_turn_budget
            .unwrap_or(defaults.initial_turns),
        hard_turns: request.hard_turn_budget.unwrap_or(defaults.hard_turns),
        initial_tokens: request
            .initial_token_budget
            .unwrap_or(defaults.initial_tokens),
        hard_tokens: request.hard_token_budget.unwrap_or(defaults.hard_tokens),
        active_execution_seconds: request
            .active_execution_seconds
            .or(request.max_elapsed_seconds)
            .unwrap_or(defaults.active_execution_seconds),
        recoverable_tool_errors: request
            .recoverable_tool_error_limit
            .unwrap_or(defaults.recoverable_tool_errors),
        identical_failures: request
            .identical_failure_limit
            .unwrap_or(defaults.identical_failures),
        verification_reserve_turns: defaults.verification_reserve_turns,
    };
    budget
        .validate()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    if !(60..=86_400).contains(&budget.active_execution_seconds) {
        return Err(ApiError::bad_request(
            "active execution budget must be between 60 and 86400 seconds",
        ));
    }
    Ok(budget)
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
            StoreError::Conflict(message) => Self::conflict(message),
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
mod tests;
