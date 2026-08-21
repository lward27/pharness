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
mod audit;
mod auth;
mod capabilities;
mod deployment;
mod environment;
mod evidence;
mod gitops;
mod internal;
mod operator;
mod pipeline;
mod releases;
mod runs;
mod sdlc;
mod source;
mod system;
mod work_items;

use approvals::{
    active_permission_grants, append_approval_gate_audit_event,
    append_permission_grant_audit_event, approval_gate_lifecycle_readiness,
    approval_gate_lifecycle_stage, create_permission_grant_record, decide_approval_gate,
    grant_is_unexpired,
};
use audit::*;
use auth::{require_operator_token, require_worker_token, OperatorIdentity};
use capabilities::*;
use deployment::{contracts::*, execution::*, intents::*};
use gitops::{change_sets::*, delivery::*};
use operator::{
    all_approval_gates_for_operator_groups, all_approvals_for_operator_groups,
    all_work_plans_for_operator_groups, group_operator_records, operator_resource_label,
};
use pipeline::{execution::*, intents::*};
use releases::*;
use sdlc::*;
use source::{change_sets::*, git_delivery::*, work_plans::*};
use system::{
    capability_statuses, immutable_git_object_id, immutable_image_digest, protected_target_json,
    BuildMetadata, ProtectedTargetConfiguration, PROTECTED_ARGO_APPLICATION, PROTECTED_ENVIRONMENT,
    PROTECTED_GITOPS_REPO, PROTECTED_IMAGE_NAME, PROTECTED_KUSTOMIZATION_PATH, PROTECTED_NAMESPACE,
    PROTECTED_PIPELINE_NAMESPACE, PROTECTED_PIPELINE_REF, PROTECTED_ROLLBACK_OWNER,
    PROTECTED_SOURCE_REPO, PROTECTED_WORKLOAD_KIND, PROTECTED_WORKLOAD_NAME,
};
pub(crate) use work_items::attempts::shell_test_evidence;
use work_items::{flow::*, lifecycle::*, preflight::*, reconcile::*, rollback::*, waits::*};

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
