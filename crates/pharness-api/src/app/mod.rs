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
use deployment::{contracts::*, execution::*, intents::*};
use gitops::{change_sets::*, delivery::*};
use operator::{
    all_approval_gates_for_operator_groups, all_approvals_for_operator_groups,
    all_work_plans_for_operator_groups, group_operator_records, operator_resource_label,
};
use pipeline::{execution::*, intents::*};
use releases::*;
use source::{change_sets::*, git_delivery::*, work_plans::*};
use system::{
    capability_statuses, immutable_git_object_id, immutable_image_digest, protected_target_json,
    BuildMetadata, ProtectedTargetConfiguration, PROTECTED_ARGO_APPLICATION, PROTECTED_ENVIRONMENT,
    PROTECTED_GITOPS_REPO, PROTECTED_IMAGE_NAME, PROTECTED_KUSTOMIZATION_PATH, PROTECTED_NAMESPACE,
    PROTECTED_PIPELINE_NAMESPACE, PROTECTED_PIPELINE_REF, PROTECTED_ROLLBACK_OWNER,
    PROTECTED_SOURCE_REPO, PROTECTED_WORKLOAD_KIND, PROTECTED_WORKLOAD_NAME,
};
pub(crate) use work_items::attempts::shell_test_evidence;
use work_items::{flow::*, preflight::*, reconcile::*, rollback::*, waits::*};

#[cfg(test)]
use work_items::actions::*;

#[cfg(test)]
use work_items::attempts::*;

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

fn risk_rank(risk: RiskLevel) -> u8 {
    match risk {
        RiskLevel::Low => 1,
        RiskLevel::Medium => 2,
        RiskLevel::High => 3,
        RiskLevel::Critical => 4,
    }
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
