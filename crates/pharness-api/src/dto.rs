use pharness_core::{
    AgentAction, AgentEvent, PolicyDecision, PolicyMode, RunBudget, RunBudgetConsumption, RunId,
    RunScope, ToolResult,
};
use pharness_store::{
    ApprovalGateSummary, ApprovalSummary, RunSummary, StoredApproval, StoredApprovalGate,
    StoredArtifact, StoredAuditEvent, StoredChangeSet, StoredControllerWait,
    StoredDeploymentContract, StoredDeploymentIntent, StoredFileChange, StoredGitOpsChangeSet,
    StoredIncident, StoredObservation, StoredPermissionGrant, StoredPipelineContract,
    StoredPipelineIntent, StoredRegistryEvidence, StoredRelease, StoredRemediationPlan, StoredRun,
    StoredWorkItem, StoredWorkPlan, StoredWorkspace,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct CreateRunRequest {
    pub task: String,
    pub cwd: Option<String>,
    pub max_turns: Option<u32>,
    #[serde(default)]
    pub policy_mode: Option<PolicyMode>,
    #[serde(default)]
    pub scope: Option<RunScope>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunResponse {
    pub id: RunId,
    pub status: String,
    pub task: String,
    pub max_turns: u32,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub cancel_requested_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<RunScope>,
    pub result: Option<serde_json::Value>,
    pub origin: String,
    pub created_by: Option<String>,
    pub run_budget: RunBudget,
    pub budget_consumption: RunBudgetConsumption,
    pub stop_reason: Option<String>,
}

impl From<StoredRun> for RunResponse {
    fn from(run: StoredRun) -> Self {
        let scope = RunScope::from_execution_target(&run.execution_target_json);
        Self {
            id: run.id,
            status: run.status,
            task: run.user_task,
            max_turns: run.max_turns,
            started_at: run.started_at,
            finished_at: run.finished_at.clone(),
            cancel_requested_at: run.cancel_requested_at,
            scope,
            result: run.result_json.or_else(|| {
                run.finished_at.map(|finished_at| {
                    serde_json::json!({
                        "finished_at": finished_at,
                        "error": run.error,
                    })
                })
            }),
            origin: run.origin,
            created_by: run.created_by,
            run_budget: run.run_budget,
            budget_consumption: run.budget_consumption,
            stop_reason: run.stop_reason,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RunsResponse {
    pub runs: Vec<RunResponse>,
    pub groups: Vec<OperatorResourceGroupResponse>,
    pub count: usize,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct OperatorResourceGroupMemberResponse {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OperatorResourceGroupResponse {
    pub key: String,
    pub title: String,
    pub resource: String,
    pub status: String,
    pub count: usize,
    pub members: Vec<OperatorResourceGroupMemberResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunSummaryResponse {
    pub summary: RunSummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventsResponse {
    pub events: Vec<AgentEvent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunDiffResponse {
    pub run_id: RunId,
    pub changes: Vec<FileChangeResponse>,
    pub diff: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunOperatorSummaryResponse {
    pub run_id: RunId,
    pub turns: u32,
    pub recoverable_failures: u32,
    pub retries: u32,
    pub estimated_context_tokens: u64,
    pub actual_prompt_tokens: u64,
    pub actual_completion_tokens: u64,
    pub actual_total_tokens: u64,
    pub compactions: u64,
    pub truncated_tool_results: u64,
    pub tools_started: u32,
    pub tools_completed: u32,
    pub tools_failed: u32,
    pub changed_paths: Vec<String>,
    pub diff_reference: String,
    pub test_commands: Vec<String>,
    pub test_results: Vec<serde_json::Value>,
    pub acceptance_evidence: Vec<serde_json::Value>,
    pub pending_approvals: Vec<String>,
    pub environment_discovery_turns: u32,
    pub approval_count: u32,
    pub approval_wait_ms: u64,
    pub preparation_duration_ms: Option<u64>,
    pub budget_extensions: u32,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactsResponse {
    pub artifacts: Vec<ArtifactResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactResponse {
    pub id: String,
    pub run_id: Option<RunId>,
    pub kind: String,
    pub label: String,
    pub mime_type: Option<String>,
    pub path: Option<String>,
    pub content_text: Option<String>,
    pub content_json: Option<serde_json::Value>,
    pub created_at: String,
}

impl From<StoredArtifact> for ArtifactResponse {
    fn from(artifact: StoredArtifact) -> Self {
        Self {
            id: artifact.id,
            run_id: artifact.run_id,
            kind: artifact.kind,
            label: artifact.label,
            mime_type: artifact.mime_type,
            path: artifact.path,
            content_text: artifact.content_text,
            content_json: artifact.content_json,
            created_at: artifact.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ObservationsResponse {
    pub observations: Vec<ObservationResponse>,
    pub count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateObservationRequest {
    pub id: Option<String>,
    pub session_id: Option<String>,
    pub run_id: Option<RunId>,
    pub source: String,
    pub kind: String,
    pub subject: String,
    pub summary: String,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub resource_ref: Option<serde_json::Value>,
    pub artifact_id: Option<String>,
    pub data_json: Option<serde_json::Value>,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObservationResponse {
    pub id: String,
    pub run_id: Option<RunId>,
    pub source: String,
    pub kind: String,
    pub subject: String,
    pub summary: String,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub resource_ref: Option<serde_json::Value>,
    pub artifact_id: Option<String>,
    pub data_json: serde_json::Value,
    pub observed_at: String,
}

impl From<StoredObservation> for ObservationResponse {
    fn from(observation: StoredObservation) -> Self {
        Self {
            id: observation.id,
            run_id: observation.run_id,
            source: observation.source,
            kind: observation.kind,
            subject: observation.subject,
            summary: observation.summary,
            resource_namespace: observation.resource_namespace,
            resource_kind: observation.resource_kind,
            resource_name: observation.resource_name,
            resource_ref: observation.resource_ref_json,
            artifact_id: observation.artifact_id,
            data_json: observation.data_json,
            observed_at: observation.observed_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct IncidentsResponse {
    pub incidents: Vec<IncidentResponse>,
    pub count: usize,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateIncidentRequest {
    pub id: Option<String>,
    pub observation_id: String,
    pub status: Option<String>,
    pub severity: String,
    pub title: String,
    pub summary: String,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub data_json: Option<serde_json::Value>,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IncidentResponse {
    pub id: String,
    pub observation_id: String,
    pub run_id: Option<RunId>,
    pub status: String,
    pub severity: String,
    pub title: String,
    pub summary: String,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub data_json: serde_json::Value,
    pub created_at: String,
}

impl From<StoredIncident> for IncidentResponse {
    fn from(incident: StoredIncident) -> Self {
        Self {
            id: incident.id,
            observation_id: incident.observation_id,
            run_id: incident.run_id,
            status: incident.status,
            severity: incident.severity,
            title: incident.title,
            summary: incident.summary,
            resource_namespace: incident.resource_namespace,
            resource_kind: incident.resource_kind,
            resource_name: incident.resource_name,
            data_json: incident.data_json,
            created_at: incident.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RemediationPlansResponse {
    pub remediation_plans: Vec<RemediationPlanResponse>,
    pub count: usize,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateRemediationPlanRequest {
    pub id: Option<String>,
    pub incident_id: String,
    pub status: Option<String>,
    pub title: String,
    pub summary: String,
    pub risk_level: String,
    pub requires_approval: Option<bool>,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub plan_json: Option<serde_json::Value>,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemediationPlanResponse {
    pub id: String,
    pub incident_id: String,
    pub run_id: Option<RunId>,
    pub status: String,
    pub title: String,
    pub summary: String,
    pub risk_level: String,
    pub requires_approval: bool,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub plan_json: serde_json::Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransitionRemediationPlanRequest {
    pub target_status: String,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransitionRemediationPlanResponse {
    pub remediation_plan: RemediationPlanResponse,
}

impl From<StoredRemediationPlan> for RemediationPlanResponse {
    fn from(plan: StoredRemediationPlan) -> Self {
        Self {
            id: plan.id,
            incident_id: plan.incident_id,
            run_id: plan.run_id,
            status: plan.status,
            title: plan.title,
            summary: plan.summary,
            risk_level: plan.risk_level,
            requires_approval: plan.requires_approval,
            resource_namespace: plan.resource_namespace,
            resource_kind: plan.resource_kind,
            resource_name: plan.resource_name,
            plan_json: plan.plan_json,
            created_at: plan.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkPlansResponse {
    pub work_plans: Vec<WorkPlanResponse>,
    pub groups: Vec<OperatorResourceGroupResponse>,
    pub count: usize,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkPlanResponse {
    pub id: String,
    pub work_item_id: Option<String>,
    pub remediation_plan_id: Option<String>,
    pub incident_id: Option<String>,
    pub run_id: Option<RunId>,
    pub status: String,
    pub title: String,
    pub summary: String,
    pub risk_level: String,
    pub requires_approval: bool,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub work_plan_json: serde_json::Value,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub revision: i64,
    pub status_changed_at: Option<String>,
    pub status_changed_by: Option<String>,
    pub status_reason: Option<String>,
    pub created_by: Option<String>,
    pub origin: String,
}

impl From<StoredWorkPlan> for WorkPlanResponse {
    fn from(plan: StoredWorkPlan) -> Self {
        Self {
            id: plan.id,
            work_item_id: plan.work_item_id,
            remediation_plan_id: plan.remediation_plan_id,
            incident_id: plan.incident_id,
            run_id: plan.run_id,
            status: plan.status,
            title: plan.title,
            summary: plan.summary,
            risk_level: plan.risk_level,
            requires_approval: plan.requires_approval,
            resource_namespace: plan.resource_namespace,
            resource_kind: plan.resource_kind,
            resource_name: plan.resource_name,
            work_plan_json: plan.work_plan_json,
            created_at: plan.created_at,
            updated_at: plan.updated_at,
            revision: plan.revision,
            status_changed_at: plan.status_changed_at,
            status_changed_by: plan.status_changed_by,
            status_reason: plan.status_reason,
            created_by: plan.created_by,
            origin: plan.origin,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateWorkItemRequest {
    pub title: String,
    pub intent: String,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    pub source_repo: String,
    #[serde(default = "default_source_ref")]
    pub source_ref: String,
    pub source_commit: Option<String>,
    pub pipeline_contract_id: Option<String>,
    pub deployment_contract_id: Option<String>,
    pub gitops_repo: Option<String>,
    pub gitops_ref: Option<String>,
    pub gitops_kustomization_path: Option<String>,
    pub gitops_image_name: Option<String>,
    pub target_environment: String,
    pub target_namespace: Option<String>,
    pub argo_application: Option<String>,
    pub workload_kind: Option<String>,
    pub workload_name: Option<String>,
    pub rollback_owner: Option<String>,
    #[serde(default)]
    pub production_impacting: bool,
    pub max_attempts: Option<u32>,
    pub max_elapsed_seconds: Option<u64>,
    pub environment_profile_id: Option<String>,
    pub initial_turn_budget: Option<u32>,
    pub hard_turn_budget: Option<u32>,
    pub initial_token_budget: Option<u64>,
    pub hard_token_budget: Option<u64>,
    pub active_execution_seconds: Option<u64>,
    pub recoverable_tool_error_limit: Option<u32>,
    pub identical_failure_limit: Option<u32>,
    pub actor: Option<String>,
    /// Hash returned by the latest read-only preflight. Production creation
    /// requires an exact match so the browser cannot submit a stale preview.
    #[serde(default)]
    pub preflight_state_hash: Option<String>,
}

fn default_source_ref() -> String {
    "main".to_string()
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkItemResponse {
    pub id: String,
    pub status: String,
    pub title: String,
    pub intent: String,
    pub acceptance_criteria: Vec<String>,
    pub source_repo: String,
    pub source_ref: String,
    pub source_commit: Option<String>,
    pub pipeline_contract_id: Option<String>,
    pub deployment_contract_id: Option<String>,
    pub gitops_repo: Option<String>,
    pub gitops_ref: Option<String>,
    pub gitops_kustomization_path: Option<String>,
    pub gitops_image_name: Option<String>,
    pub target_environment: String,
    pub target_namespace: Option<String>,
    pub argo_application: Option<String>,
    pub workload_kind: Option<String>,
    pub workload_name: Option<String>,
    pub rollback_owner: Option<String>,
    pub production_impacting: bool,
    pub max_attempts: u32,
    pub max_elapsed_seconds: u64,
    pub attempt_count: u32,
    pub current_run_id: Option<RunId>,
    pub created_by: Option<String>,
    pub origin: String,
    pub created_at: String,
    pub updated_at: String,
    pub status_changed_at: String,
    pub status_changed_by: Option<String>,
    pub status_reason: Option<String>,
    pub environment_profile_id: Option<String>,
    pub run_budget: RunBudget,
    pub repository_contract: Option<serde_json::Value>,
    pub repository_contract_hash: Option<String>,
    pub environment_preparation_status: String,
    pub current_environment_snapshot_id: Option<String>,
}

impl From<StoredWorkItem> for WorkItemResponse {
    fn from(item: StoredWorkItem) -> Self {
        Self {
            id: item.id,
            status: item.status,
            title: item.title,
            intent: item.intent,
            acceptance_criteria: item.acceptance_criteria,
            source_repo: item.source_repo,
            source_ref: item.source_ref,
            source_commit: item.source_commit,
            pipeline_contract_id: item.pipeline_contract_id,
            deployment_contract_id: item.deployment_contract_id,
            gitops_repo: item.gitops_repo,
            gitops_ref: item.gitops_ref,
            gitops_kustomization_path: item.gitops_kustomization_path,
            gitops_image_name: item.gitops_image_name,
            target_environment: item.target_environment,
            target_namespace: item.target_namespace,
            argo_application: item.argo_application,
            workload_kind: item.workload_kind,
            workload_name: item.workload_name,
            rollback_owner: item.rollback_owner,
            production_impacting: item.production_impacting,
            max_attempts: item.max_attempts,
            max_elapsed_seconds: item.max_elapsed_seconds,
            attempt_count: item.attempt_count,
            current_run_id: item.current_run_id,
            created_by: item.created_by,
            origin: item.origin,
            created_at: item.created_at,
            updated_at: item.updated_at,
            status_changed_at: item.status_changed_at,
            status_changed_by: item.status_changed_by,
            status_reason: item.status_reason,
            environment_profile_id: item.environment_profile_id,
            run_budget: item.run_budget,
            repository_contract: item.repository_contract_json,
            repository_contract_hash: item.repository_contract_hash,
            environment_preparation_status: item.environment_preparation_status,
            current_environment_snapshot_id: item.current_environment_snapshot_id,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EnvironmentProfileResponse {
    pub id: String,
    pub status: String,
    pub image: String,
    pub revision: String,
    pub platform: String,
    pub required_executables: Vec<String>,
    pub preparation_strategy: String,
    pub service_account: String,
    pub repository_allowlist: Vec<String>,
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnvironmentProfilesResponse {
    pub profiles: Vec<EnvironmentProfileResponse>,
    pub provider_transport_attempts: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnvironmentPreparationResponse {
    pub id: String,
    pub work_item_id: String,
    pub workspace_id: String,
    pub run_id: Option<RunId>,
    pub status: String,
    pub environment_profile_id: String,
    pub source_commit: String,
    pub project_contract: Option<serde_json::Value>,
    pub project_contract_hash: Option<String>,
    pub environment_snapshot: Option<serde_json::Value>,
    pub logs: serde_json::Value,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

impl From<pharness_store::StoredEnvironmentPreparation> for EnvironmentPreparationResponse {
    fn from(value: pharness_store::StoredEnvironmentPreparation) -> Self {
        Self {
            id: value.id,
            work_item_id: value.work_item_id,
            workspace_id: value.workspace_id,
            run_id: value.run_id,
            status: value.status,
            environment_profile_id: value.environment_profile_id,
            source_commit: value.source_commit,
            project_contract: value.project_contract_json,
            project_contract_hash: value.project_contract_hash,
            environment_snapshot: value.environment_snapshot_json,
            logs: value.logs_json,
            error: value.error,
            started_at: value.started_at,
            finished_at: value.finished_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BudgetExtensionResponse {
    pub id: String,
    pub work_item_id: String,
    pub run_id: RunId,
    pub status: String,
    pub turn_increment: u32,
    pub token_increment: u64,
    pub state_hash: String,
    pub requested_at: String,
    pub approved_at: Option<String>,
    pub approved_by: Option<String>,
}

impl From<pharness_store::StoredBudgetExtension> for BudgetExtensionResponse {
    fn from(value: pharness_store::StoredBudgetExtension) -> Self {
        Self {
            id: value.id,
            work_item_id: value.work_item_id,
            run_id: value.run_id,
            status: value.status,
            turn_increment: value.turn_increment,
            token_increment: value.token_increment,
            state_hash: value.state_hash,
            requested_at: value.requested_at,
            approved_at: value.approved_at,
            approved_by: value.approved_by,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApproveBudgetExtensionRequest {
    pub actor: String,
    pub reason: String,
    pub state_hash: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkItemsResponse {
    pub work_items: Vec<WorkItemResponse>,
    pub count: usize,
    pub limit: u32,
    pub offset: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator_state: Option<std::collections::BTreeMap<String, WorkItemOperatorStateResponse>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkItemOperatorStateResponse {
    pub current_boundary: String,
    pub attempts_remaining: u32,
    pub attention_reason: Option<String>,
    pub active_wait: Option<ControllerWaitResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkItemPipelineContextResponse {
    pub work_item: WorkItemResponse,
    pub work_plan: WorkPlanResponse,
    pub change_set: ChangeSetResponse,
    pub pipeline_intent: Option<PipelineIntentResponse>,
    pub source_provenance: serde_json::Value,
    pub contract_namespace: Option<String>,
    pub contract_pipeline_ref: Option<String>,
    pub active_pipeline_contracts: Vec<PipelineContractResponse>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransitionWorkItemRequest {
    pub target_status: String,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceResponse {
    pub id: String,
    pub work_item_id: String,
    pub run_id: Option<RunId>,
    pub status: String,
    pub source_repo: String,
    pub source_ref: String,
    pub resolved_commit: Option<String>,
    pub branch: Option<String>,
    pub retention_status: String,
    pub created_at: String,
    pub updated_at: String,
    pub status_changed_at: String,
    pub status_changed_by: Option<String>,
    pub status_reason: Option<String>,
}

impl From<StoredWorkspace> for WorkspaceResponse {
    fn from(workspace: StoredWorkspace) -> Self {
        Self {
            id: workspace.id,
            work_item_id: workspace.work_item_id,
            run_id: workspace.run_id,
            status: workspace.status,
            source_repo: workspace.source_repo,
            source_ref: workspace.source_ref,
            resolved_commit: workspace.resolved_commit,
            branch: workspace.branch,
            retention_status: workspace.retention_status,
            created_at: workspace.created_at,
            updated_at: workspace.updated_at,
            status_changed_at: workspace.status_changed_at,
            status_changed_by: workspace.status_changed_by,
            status_reason: workspace.status_reason,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspacesResponse {
    pub workspaces: Vec<WorkspaceResponse>,
    pub count: usize,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExecuteWorkItemRequest {
    pub actor: Option<String>,
    pub reason: Option<String>,
    pub max_turns: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecuteWorkItemResponse {
    pub work_item: WorkItemResponse,
    pub workspace: WorkspaceResponse,
    pub run: RunResponse,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReconcileWorkItemRequest {
    #[serde(default)]
    pub apply: bool,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub max_turns: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReconcileWorkItemResponse {
    pub action: String,
    pub applied: bool,
    pub work_item: WorkItemResponse,
    pub work_plan: Option<WorkPlanResponse>,
    pub workspace: Option<WorkspaceResponse>,
    pub run: Option<RunResponse>,
    pub change_set: Option<ChangeSetResponse>,
    pub git_delivery_preflight: Option<GitDeliveryPreflightResponse>,
    pub pipeline_intent: Option<PipelineIntentResponse>,
    pub pipeline_execution_preflight: Option<PipelineIntentExecutionPreflightResponse>,
    pub deployment_intent: Option<DeploymentIntentResponse>,
    pub deployment_execution_preflight: Option<DeploymentIntentPreflightResponse>,
    pub deployment_delivery: Option<DeploymentIntentDeliveryFlowResponse>,
    pub gitops_change_set: Option<GitOpsChangeSetResponse>,
    pub gitops_delivery: Option<GitOpsDeliveryFlowResponse>,
    pub gitops_delivery_preflight: Option<GitOpsDeliveryPreflightResponse>,
    pub controller_wait: Option<ControllerWaitResponse>,
    pub message: String,
    pub boundary: String,
    pub can_apply: bool,
    pub effect_summary: String,
    pub blockers: Vec<ReconcileBlockerResponse>,
    pub authorization_checks: Vec<ReconcileAuthorizationCheckResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReconcileBlockerResponse {
    pub code: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReconcileAuthorizationCheckResponse {
    pub kind: String,
    pub status: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkItemActionResponse {
    pub id: String,
    pub lifecycle_stage: String,
    pub resource: String,
    pub status: String,
    pub effect_class: String,
    pub blockers: Vec<ReconcileBlockerResponse>,
    pub approval_required: bool,
    pub approval_requirements: Vec<String>,
    pub external_effect_summary: String,
    pub state_hash: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExecuteWorkItemActionRequest {
    pub actor: Option<String>,
    pub reason: String,
    pub state_hash: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AdvanceWorkItemRequest {
    pub actor: Option<String>,
    pub reason: String,
    #[serde(default)]
    pub max_steps: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdvanceWorkItemResponse {
    pub steps: Vec<ReconcileWorkItemResponse>,
    pub stopped_at: WorkItemActionResponse,
}

#[derive(Debug, Clone, Serialize)]
pub struct CapabilityStatusResponse {
    pub capability: String,
    pub status: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkItemPreflightResponse {
    pub ready: bool,
    pub state_hash: String,
    pub normalized_submission: serde_json::Value,
    pub selected_contracts: serde_json::Value,
    pub checks: Vec<CapabilityStatusResponse>,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
    pub predicted_external_mutations: Vec<String>,
    pub production_gates: Vec<String>,
    pub rollback_prerequisites: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemReadinessResponse {
    pub api_revision: String,
    pub ui_revision: String,
    pub runtime_image_digest: String,
    pub ui_image_digest: String,
    pub platform_versions_match: bool,
    pub capabilities: Vec<CapabilityStatusResponse>,
    pub repository_allowlists: serde_json::Value,
    pub targets: serde_json::Value,
    pub environment_profiles: Vec<EnvironmentProfileResponse>,
    pub blockers: Vec<String>,
}

/// A read-only WorkItem-rooted delivery view. The nested reconcile response is
/// always produced with `apply=false`; it is safe for dashboards to poll.
#[derive(Debug, Clone, Serialize)]
pub struct WorkItemFlowResponse {
    pub work_item: WorkItemResponse,
    pub reconcile_preview: ReconcileWorkItemResponse,
    pub sdlc_flow: Option<SdlcFlowResponse>,
    pub delivery_segments: Vec<DeliverySegmentResponse>,
    pub workspaces: Vec<WorkspaceResponse>,
    pub controller_waits: Vec<ControllerWaitResponse>,
    pub audit_events: Vec<AuditEventResponse>,
    pub action_rail: Vec<WorkItemActionResponse>,
    pub delivery_configuration: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_mode: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeliverySegmentResourceResponse {
    pub kind: String,
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeliverySegmentResponse {
    pub key: String,
    pub label: String,
    pub status: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopping_reason: Option<String>,
    pub resources: Vec<DeliverySegmentResourceResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TriageItemResponse {
    pub kind: String,
    pub id: String,
    pub title: String,
    pub summary: String,
    pub status: String,
    pub risk_level: String,
    pub origin: String,
    pub created_at: String,
    pub resource_kind: String,
    pub resource_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_item_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TriageSummaryResponse {
    pub pending_approval_gates: usize,
    pub pending_tool_approvals: usize,
    pub blocked_work_items: usize,
    pub expired_controller_waits: usize,
    pub proposed_remediation_plans: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct TriageResponse {
    pub items: Vec<TriageItemResponse>,
    pub summary: TriageSummaryResponse,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScopeOptionsResponse {
    pub environments: Vec<String>,
    pub namespaces: Vec<String>,
    pub repositories: Vec<String>,
    pub branches: Vec<String>,
    pub actors: Vec<String>,
    pub origins: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ControllerWaitResponse {
    pub id: String,
    pub work_item_id: String,
    pub run_id: Option<RunId>,
    pub status: String,
    pub wait_kind: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub next_check_at: String,
    pub deadline_at: String,
    pub max_checks: u32,
    pub check_count: u32,
    pub data_json: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
    pub resolved_at: Option<String>,
    pub resolution_reason: Option<String>,
}

impl From<StoredControllerWait> for ControllerWaitResponse {
    fn from(wait: StoredControllerWait) -> Self {
        Self {
            id: wait.id,
            work_item_id: wait.work_item_id,
            run_id: wait.run_id,
            status: wait.status,
            wait_kind: wait.wait_kind,
            subject_kind: wait.subject_kind,
            subject_id: wait.subject_id,
            next_check_at: wait.next_check_at,
            deadline_at: wait.deadline_at,
            max_checks: wait.max_checks,
            check_count: wait.check_count,
            data_json: wait.data_json,
            created_at: wait.created_at,
            updated_at: wait.updated_at,
            resolved_at: wait.resolved_at,
            resolution_reason: wait.resolution_reason,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ControllerWaitsResponse {
    pub controller_waits: Vec<ControllerWaitResponse>,
    pub count: usize,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReconcileDueControllerWaitsRequest {
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub actor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ControllerWaitTickResult {
    pub controller_wait: ControllerWaitResponse,
    pub outcome: String,
    pub next_action: Option<String>,
    pub work_item: WorkItemResponse,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReconcileDueControllerWaitsResponse {
    pub evaluated_at: String,
    pub checked: usize,
    pub pending: usize,
    pub progressed: usize,
    pub blocked: usize,
    pub results: Vec<ControllerWaitTickResult>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReplanWorkItemRequest {
    #[serde(default)]
    pub actor: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplanWorkItemResponse {
    pub work_item: WorkItemResponse,
    pub work_plan: WorkPlanResponse,
    pub attempts_remaining: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CaptureWorkItemChangeSetRequest {
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateWorkPlanFromRemediationPlanRequest {
    pub remediation_plan_id: String,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateWorkPlanResponse {
    pub work_plan: WorkPlanResponse,
    pub created: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReviseWorkPlanRequest {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub risk_level: Option<String>,
    pub requires_approval: Option<bool>,
    pub work_plan_json: serde_json::Value,
    pub actor: Option<String>,
    pub reason: Option<String>,
    #[serde(default = "default_material_change")]
    pub material_change: bool,
}

fn default_material_change() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviseWorkPlanResponse {
    pub work_plan: WorkPlanResponse,
    pub invalidated_gates: Vec<ApprovalGateResponse>,
    pub invalidated_change_set: Option<ChangeSetResponse>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransitionWorkPlanRequest {
    pub target_status: String,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransitionWorkPlanResponse {
    pub work_plan: WorkPlanResponse,
}

#[derive(Debug, Clone, Serialize)]
pub struct SdlcReadinessResponse {
    pub resource_kind: String,
    pub resource_id: String,
    pub ready: bool,
    pub summary: String,
    pub work_plan: WorkPlanResponse,
    pub change_set: Option<ChangeSetResponse>,
    pub pipeline_intent: Option<PipelineIntentResponse>,
    pub deployment_intent: Option<DeploymentIntentResponse>,
    pub release: Option<ReleaseResponse>,
    pub registry_evidence: Option<RegistryEvidenceResponse>,
    pub blockers: Vec<SdlcReadinessFinding>,
    pub warnings: Vec<SdlcReadinessFinding>,
    pub approval_gates: SdlcReadinessGateSummary,
    pub trusted_envelopes: SdlcReadinessGrantSummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct SdlcReadinessFinding {
    pub code: String,
    pub message: String,
    pub resource_kind: String,
    pub resource_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SdlcReadinessGateSummary {
    pub pending: Vec<ApprovalGateResponse>,
    pub stale: Vec<ApprovalGateResponse>,
    pub rejected: Vec<ApprovalGateResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SdlcReadinessGrantSummary {
    pub active: Vec<PermissionGrantResponse>,
    pub stale: Vec<PermissionGrantResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SdlcFlowResponse {
    pub resource_kind: String,
    pub resource_id: String,
    pub readiness: SdlcReadinessResponse,
    /// Server-derived source-to-verification stages for legacy Flow deep links.
    pub delivery_segments: Vec<DeliverySegmentResponse>,
    pub work_plan: WorkPlanResponse,
    pub change_set: Option<ChangeSetResponse>,
    pub pipeline_intent: Option<PipelineIntentResponse>,
    pub gitops_change_set: Option<GitOpsChangeSetResponse>,
    pub gitops_delivery: Option<GitOpsDeliveryFlowResponse>,
    pub deployment_intent: Option<DeploymentIntentResponse>,
    pub release: Option<ReleaseResponse>,
    pub registry_evidence: Option<RegistryEvidenceResponse>,
    pub git_delivery: Option<GitDeliveryFlowResponse>,
    pub incidents: Vec<IncidentResponse>,
    pub remediation_plans: Vec<RemediationPlanResponse>,
    pub approval_gates: Vec<ApprovalGateResponse>,
    pub audit_events: Vec<AuditEventResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangeSetsResponse {
    pub change_sets: Vec<ChangeSetResponse>,
    pub count: usize,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangeSetResponse {
    pub id: String,
    pub work_item_id: Option<String>,
    pub work_plan_id: String,
    pub remediation_plan_id: Option<String>,
    pub incident_id: Option<String>,
    pub run_id: Option<RunId>,
    pub status: String,
    pub title: String,
    pub summary: String,
    pub risk_level: String,
    pub material_hash: String,
    pub revision: i64,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub change_set_json: serde_json::Value,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub status_changed_at: Option<String>,
    pub status_changed_by: Option<String>,
    pub status_reason: Option<String>,
}

impl From<StoredChangeSet> for ChangeSetResponse {
    fn from(change_set: StoredChangeSet) -> Self {
        Self {
            id: change_set.id,
            work_item_id: change_set.work_item_id,
            work_plan_id: change_set.work_plan_id,
            remediation_plan_id: change_set.remediation_plan_id,
            incident_id: change_set.incident_id,
            run_id: change_set.run_id,
            status: change_set.status,
            title: change_set.title,
            summary: change_set.summary,
            risk_level: change_set.risk_level,
            material_hash: change_set.material_hash,
            revision: change_set.revision,
            resource_namespace: change_set.resource_namespace,
            resource_kind: change_set.resource_kind,
            resource_name: change_set.resource_name,
            change_set_json: change_set.change_set_json,
            created_at: change_set.created_at,
            updated_at: change_set.updated_at,
            status_changed_at: change_set.status_changed_at,
            status_changed_by: change_set.status_changed_by,
            status_reason: change_set.status_reason,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateChangeSetRequest {
    pub work_plan_id: String,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub risk_level: Option<String>,
    pub change_set_json: serde_json::Value,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateChangeSetResponse {
    pub change_set: ChangeSetResponse,
    pub created: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PrepareGitDeliveryRequest {
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitDeliveryPlanResponse {
    pub artifact: ArtifactResponse,
    pub created: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateGitDeliveryAuthorizationRequest {
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub created_by: Option<String>,
    pub reason: String,
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitDeliveryAuthorizationResponse {
    pub grant: PermissionGrantResponse,
    pub plan: ArtifactResponse,
    pub created: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitDeliveryPreflightRequest {
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitDeliveryPreflightResponse {
    pub status: String,
    pub approval_gate_ready: bool,
    pub authorization_ready: bool,
    pub dispatch_ready: bool,
    pub plan: ArtifactResponse,
    pub permission_grant: Option<PermissionGrantResponse>,
    pub checks: Vec<serde_json::Value>,
    pub artifact: ArtifactResponse,
    pub created: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExecuteGitDeliveryRequest {
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecuteGitDeliveryResponse {
    pub status: String,
    pub execution: ArtifactResponse,
    pub plan: ArtifactResponse,
    pub permission_grant: PermissionGrantResponse,
    pub job_name: Option<String>,
    pub created: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ObserveGitDeliveryRequest {
    #[serde(default)]
    pub actor: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObserveGitDeliveryResponse {
    pub status: String,
    pub execution: ArtifactResponse,
    pub job_name: Option<String>,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitDeliveryObservationContextResponse {
    pub execution_id: String,
    pub repository: String,
    pub base_ref: String,
    pub head_branch: String,
    pub source_commit_sha: String,
    pub pull_request_url: String,
    pub pull_request_number: u64,
    pub github_api_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitDeliveryObservationOutcomeRequest {
    pub execution_id: String,
    pub status: String,
    #[serde(default)]
    pub pull_request_state: Option<String>,
    #[serde(default)]
    pub merged: Option<bool>,
    #[serde(default)]
    pub merge_commit_sha: Option<String>,
    #[serde(default)]
    pub head_branch: Option<String>,
    #[serde(default)]
    pub head_commit_sha: Option<String>,
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub authoritative_rules_succeeded: bool,
    #[serde(default)]
    pub required_checks: serde_json::Value,
    #[serde(default)]
    pub check_runs: serde_json::Value,
    #[serde(default)]
    pub commit_statuses: serde_json::Value,
    #[serde(default)]
    pub provider_check_status: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitDeliveryContextResponse {
    pub execution_id: String,
    pub repository: String,
    pub base_ref: String,
    pub base_commit: String,
    pub head_branch: String,
    pub diff: String,
    pub commit_subject: String,
    pub commit_body: String,
    pub pull_request_title: String,
    pub pull_request_body: String,
    pub github_api_url: String,
    pub author_name: String,
    pub author_email: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitDeliveryOutcomeRequest {
    pub execution_id: String,
    pub status: String,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub commit_sha: Option<String>,
    #[serde(default)]
    pub pull_request_url: Option<String>,
    #[serde(default)]
    pub pull_request_number: Option<u64>,
    #[serde(default)]
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitDeliveryFlowResponse {
    pub plan: ArtifactResponse,
    pub latest_preflight: Option<ArtifactResponse>,
    pub latest_execution: Option<ArtifactResponse>,
    pub latest_result: Option<ArtifactResponse>,
    pub latest_observation: Option<ArtifactResponse>,
    pub latest_merge: Option<ArtifactResponse>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReviseChangeSetRequest {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub risk_level: Option<String>,
    pub change_set_json: serde_json::Value,
    pub actor: Option<String>,
    pub reason: Option<String>,
    #[serde(default = "default_material_change")]
    pub material_change: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviseChangeSetResponse {
    pub change_set: ChangeSetResponse,
    pub material_hash_changed: bool,
    pub invalidated_gates: Vec<ApprovalGateResponse>,
    pub invalidated_pipeline_intent: Option<PipelineIntentResponse>,
    pub invalidated_deployment_intent: Option<DeploymentIntentResponse>,
    pub invalidated_release: Option<ReleaseResponse>,
    pub invalidated_registry_evidence: Option<RegistryEvidenceResponse>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransitionChangeSetRequest {
    pub target_status: String,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransitionChangeSetResponse {
    pub change_set: ChangeSetResponse,
}

#[derive(Debug, Clone, Serialize)]
pub struct PipelineIntentsResponse {
    pub pipeline_intents: Vec<PipelineIntentResponse>,
    pub count: usize,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct PipelineContractsResponse {
    pub pipeline_contracts: Vec<PipelineContractResponse>,
    pub count: usize,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct PipelineContractResponse {
    pub id: String,
    pub status: String,
    pub namespace: String,
    pub pipeline_ref: String,
    pub version: String,
    pub contract_json: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
    pub status_changed_at: String,
    pub status_changed_by: Option<String>,
    pub status_reason: Option<String>,
}

impl From<StoredPipelineContract> for PipelineContractResponse {
    fn from(contract: StoredPipelineContract) -> Self {
        Self {
            id: contract.id,
            status: contract.status,
            namespace: contract.namespace,
            pipeline_ref: contract.pipeline_ref,
            version: contract.version,
            contract_json: contract.contract_json,
            created_at: contract.created_at,
            updated_at: contract.updated_at,
            status_changed_at: contract.status_changed_at,
            status_changed_by: contract.status_changed_by,
            status_reason: contract.status_reason,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DeploymentContractsResponse {
    pub deployment_contracts: Vec<DeploymentContractResponse>,
    pub count: usize,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeploymentContractResponse {
    pub id: String,
    pub status: String,
    pub target_environment: String,
    pub target_namespace: String,
    pub argo_application: String,
    pub version: String,
    pub contract_json: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
    pub status_changed_at: String,
    pub status_changed_by: Option<String>,
    pub status_reason: Option<String>,
}

impl From<StoredDeploymentContract> for DeploymentContractResponse {
    fn from(contract: StoredDeploymentContract) -> Self {
        Self {
            id: contract.id,
            status: contract.status,
            target_environment: contract.target_environment,
            target_namespace: contract.target_namespace,
            argo_application: contract.argo_application,
            version: contract.version,
            contract_json: contract.contract_json,
            created_at: contract.created_at,
            updated_at: contract.updated_at,
            status_changed_at: contract.status_changed_at,
            status_changed_by: contract.status_changed_by,
            status_reason: contract.status_reason,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateDeploymentContractRequest {
    pub target_environment: String,
    pub target_namespace: String,
    pub argo_application: String,
    pub version: Option<String>,
    pub contract_json: serde_json::Value,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransitionDeploymentContractRequest {
    pub target_status: String,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatePipelineContractRequest {
    pub namespace: String,
    pub pipeline_ref: String,
    pub version: Option<String>,
    pub contract_json: serde_json::Value,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransitionPipelineContractRequest {
    pub target_status: String,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReplacePipelineContractRequest {
    pub version: String,
    pub contract_json: serde_json::Value,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplacePipelineContractResponse {
    pub retired_contract: PipelineContractResponse,
    pub pipeline_contract: PipelineContractResponse,
}

#[derive(Debug, Clone, Serialize)]
pub struct PipelineIntentResponse {
    pub id: String,
    pub change_set_id: String,
    pub work_plan_id: String,
    pub remediation_plan_id: Option<String>,
    pub incident_id: Option<String>,
    pub run_id: Option<RunId>,
    pub status: String,
    pub title: String,
    pub summary: String,
    pub risk_level: String,
    pub intent_kind: String,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub intent_json: serde_json::Value,
    pub execution_state: Option<serde_json::Value>,
    pub execution_evidence: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub status_changed_at: Option<String>,
    pub status_changed_by: Option<String>,
    pub status_reason: Option<String>,
}

impl From<StoredPipelineIntent> for PipelineIntentResponse {
    fn from(intent: StoredPipelineIntent) -> Self {
        Self {
            id: intent.id,
            change_set_id: intent.change_set_id,
            work_plan_id: intent.work_plan_id,
            remediation_plan_id: intent.remediation_plan_id,
            incident_id: intent.incident_id,
            run_id: intent.run_id,
            status: intent.status,
            title: intent.title,
            summary: intent.summary,
            risk_level: intent.risk_level,
            intent_kind: intent.intent_kind,
            resource_namespace: intent.resource_namespace,
            resource_kind: intent.resource_kind,
            resource_name: intent.resource_name,
            execution_state: intent.intent_json.get("execution_state").cloned(),
            execution_evidence: intent.intent_json.get("execution_evidence").cloned(),
            intent_json: intent.intent_json,
            created_at: intent.created_at,
            updated_at: intent.updated_at,
            status_changed_at: intent.status_changed_at,
            status_changed_by: intent.status_changed_by,
            status_reason: intent.status_reason,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatePipelineIntentFromChangeSetRequest {
    pub change_set_id: String,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub risk_level: Option<String>,
    pub intent_kind: Option<String>,
    #[serde(default)]
    pub intent_json: Option<serde_json::Value>,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateWorkItemPipelineIntentRequest {
    pub pipeline_contract_id: String,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub risk_level: Option<String>,
    pub intent_kind: Option<String>,
    #[serde(default)]
    pub intent_json: Option<serde_json::Value>,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreatePipelineIntentResponse {
    pub pipeline_intent: PipelineIntentResponse,
    pub created: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateGitOpsUpdatePlanRequest {
    /// Optional explicit digest-pinned image. When omitted, Pharness derives
    /// it from a verified terminal PipelineRun build-output artifact.
    #[serde(default)]
    pub image_ref: Option<String>,
    pub kustomization_path: String,
    pub image_name: String,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitOpsUpdatePlanResponse {
    pub artifact: ArtifactResponse,
    pub created: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateGitOpsChangeSetRequest {
    pub pipeline_intent_id: String,
    pub gitops_update_plan_artifact_id: String,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransitionGitOpsChangeSetRequest {
    pub target_status: String,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitOpsChangeSetResponse {
    pub id: String,
    pub work_item_id: String,
    pub work_plan_id: String,
    pub source_change_set_id: String,
    pub pipeline_intent_id: String,
    pub deployment_intent_id: String,
    pub gitops_update_plan_artifact_id: String,
    pub run_id: RunId,
    pub status: String,
    pub title: String,
    pub summary: String,
    pub risk_level: String,
    pub material_hash: String,
    pub revision: i64,
    pub gitops_repo: String,
    pub gitops_ref: String,
    pub head_branch: String,
    pub kustomization_path: String,
    pub image_name: String,
    pub image_ref: String,
    pub gitops_change_set_json: serde_json::Value,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub status_changed_at: Option<String>,
    pub status_changed_by: Option<String>,
    pub status_reason: Option<String>,
}

impl From<StoredGitOpsChangeSet> for GitOpsChangeSetResponse {
    fn from(change_set: StoredGitOpsChangeSet) -> Self {
        Self {
            id: change_set.id,
            work_item_id: change_set.work_item_id,
            work_plan_id: change_set.work_plan_id,
            source_change_set_id: change_set.source_change_set_id,
            pipeline_intent_id: change_set.pipeline_intent_id,
            deployment_intent_id: change_set.deployment_intent_id,
            gitops_update_plan_artifact_id: change_set.gitops_update_plan_artifact_id,
            run_id: change_set.run_id,
            status: change_set.status,
            title: change_set.title,
            summary: change_set.summary,
            risk_level: change_set.risk_level,
            material_hash: change_set.material_hash,
            revision: change_set.revision,
            gitops_repo: change_set.gitops_repo,
            gitops_ref: change_set.gitops_ref,
            head_branch: change_set.head_branch,
            kustomization_path: change_set.kustomization_path,
            image_name: change_set.image_name,
            image_ref: change_set.image_ref,
            gitops_change_set_json: change_set.gitops_change_set_json,
            created_at: change_set.created_at,
            updated_at: change_set.updated_at,
            status_changed_at: change_set.status_changed_at,
            status_changed_by: change_set.status_changed_by,
            status_reason: change_set.status_reason,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GitOpsChangeSetsResponse {
    pub gitops_change_sets: Vec<GitOpsChangeSetResponse>,
    pub count: usize,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateGitOpsChangeSetResponse {
    pub gitops_change_set: GitOpsChangeSetResponse,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransitionGitOpsChangeSetResponse {
    pub gitops_change_set: GitOpsChangeSetResponse,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResolveGitOpsBaseRevisionRequest {
    #[serde(default)]
    pub actor: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolveGitOpsBaseRevisionResponse {
    pub status: String,
    pub execution: ArtifactResponse,
    pub job_name: Option<String>,
    pub created: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PrepareGitOpsDeliveryRequest {
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitOpsDeliveryPlanResponse {
    pub artifact: ArtifactResponse,
    pub base_revision: ArtifactResponse,
    pub created: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateGitOpsDeliveryAuthorizationRequest {
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub created_by: Option<String>,
    pub reason: String,
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitOpsDeliveryAuthorizationResponse {
    pub grant: PermissionGrantResponse,
    pub plan: ArtifactResponse,
    pub created: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitOpsDeliveryPreflightRequest {
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitOpsDeliveryPreflightResponse {
    pub status: String,
    pub approval_gate_ready: bool,
    pub authorization_ready: bool,
    pub dispatch_ready: bool,
    pub plan: ArtifactResponse,
    pub base_revision: ArtifactResponse,
    pub permission_grant: Option<PermissionGrantResponse>,
    pub checks: Vec<serde_json::Value>,
    pub artifact: ArtifactResponse,
    pub created: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExecuteGitOpsDeliveryRequest {
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecuteGitOpsDeliveryResponse {
    pub status: String,
    pub execution: ArtifactResponse,
    pub plan: ArtifactResponse,
    pub permission_grant: PermissionGrantResponse,
    pub job_name: Option<String>,
    pub created: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ObserveGitOpsDeliveryRequest {
    #[serde(default)]
    pub actor: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObserveGitOpsDeliveryResponse {
    pub status: String,
    pub execution: ArtifactResponse,
    pub job_name: Option<String>,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitOpsDeliveryFlowResponse {
    pub plan: ArtifactResponse,
    pub base_revision: ArtifactResponse,
    pub latest_preflight: Option<ArtifactResponse>,
    pub latest_execution: Option<ArtifactResponse>,
    pub latest_result: Option<ArtifactResponse>,
    pub latest_observation: Option<ArtifactResponse>,
    pub latest_merge: Option<ArtifactResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitOpsDeliveryContextResponse {
    pub execution_id: String,
    pub repository: String,
    pub base_ref: String,
    pub base_commit: String,
    pub head_branch: String,
    pub kustomization_path: String,
    pub image_name: String,
    pub image_ref: String,
    pub commit_subject: String,
    pub commit_body: String,
    pub pull_request_title: String,
    pub pull_request_body: String,
    pub github_api_url: String,
    pub author_name: String,
    pub author_email: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitOpsDeliveryOutcomeRequest {
    pub execution_id: String,
    pub status: String,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub commit_sha: Option<String>,
    #[serde(default)]
    pub pull_request_url: Option<String>,
    #[serde(default)]
    pub pull_request_number: Option<u64>,
    #[serde(default)]
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitOpsDeliveryObservationContextResponse {
    pub execution_id: String,
    pub repository: String,
    pub head_branch: String,
    pub source_commit_sha: String,
    pub pull_request_url: String,
    pub pull_request_number: u64,
    pub github_api_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitOpsDeliveryObservationOutcomeRequest {
    pub execution_id: String,
    pub status: String,
    #[serde(default)]
    pub pull_request_state: Option<String>,
    #[serde(default)]
    pub merged: Option<bool>,
    #[serde(default)]
    pub merge_commit_sha: Option<String>,
    #[serde(default)]
    pub head_branch: Option<String>,
    #[serde(default)]
    pub head_commit_sha: Option<String>,
    #[serde(default)]
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitOpsBaseRevisionContextResponse {
    pub execution_id: String,
    pub repository: String,
    pub base_ref: String,
    pub github_api_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitOpsBaseRevisionOutcomeRequest {
    pub execution_id: String,
    pub status: String,
    #[serde(default)]
    pub base_commit: Option<String>,
    #[serde(default)]
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransitionPipelineIntentRequest {
    pub target_status: String,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransitionPipelineIntentResponse {
    pub pipeline_intent: PipelineIntentResponse,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AttachPipelineIntentEvidenceRequest {
    pub observation_id: String,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttachPipelineIntentEvidenceResponse {
    pub pipeline_intent: PipelineIntentResponse,
    pub observation: ObservationResponse,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatePipelineIntentTrustedEnvelopeRequest {
    pub subject: Option<String>,
    pub created_by: Option<String>,
    pub reason: String,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExecutePipelineIntentRequest {
    #[serde(default = "default_dry_run")]
    pub dry_run: bool,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

fn default_dry_run() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutePipelineIntentResponse {
    pub status: String,
    pub ready: bool,
    pub dry_run: bool,
    pub pipeline_intent: PipelineIntentResponse,
    pub manifest: Option<serde_json::Value>,
    pub checks: Vec<serde_json::Value>,
    pub permission_grant_id: Option<String>,
    pub execution_id: Option<String>,
    pub executor_job_name: Option<String>,
}

/// Read-only readiness result used by the WorkItem controller. Dispatching a
/// PipelineRun still requires the separate explicit PipelineIntent execute API.
#[derive(Debug, Clone, Serialize)]
pub struct PipelineIntentExecutionPreflightResponse {
    pub ready: bool,
    pub manifest: Option<serde_json::Value>,
    pub checks: Vec<serde_json::Value>,
    pub permission_grant_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PipelineIntentExecutionOutcomeRequest {
    pub execution_id: String,
    pub status: String,
    pub pipeline_run_namespace: Option<String>,
    pub pipeline_run_name: Option<String>,
    pub error: Option<String>,
    pub pipeline_run_analysis: Option<serde_json::Value>,
    pub analysis_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeploymentIntentsResponse {
    pub deployment_intents: Vec<DeploymentIntentResponse>,
    pub count: usize,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeploymentIntentResponse {
    pub id: String,
    pub pipeline_intent_id: String,
    pub change_set_id: String,
    pub work_plan_id: String,
    pub remediation_plan_id: Option<String>,
    pub incident_id: Option<String>,
    pub run_id: Option<RunId>,
    pub status: String,
    pub title: String,
    pub summary: String,
    pub risk_level: String,
    pub intent_kind: String,
    pub target_environment: Option<String>,
    pub target_namespace: Option<String>,
    pub argo_application: Option<String>,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub intent_json: serde_json::Value,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub status_changed_at: Option<String>,
    pub status_changed_by: Option<String>,
    pub status_reason: Option<String>,
}

impl From<StoredDeploymentIntent> for DeploymentIntentResponse {
    fn from(intent: StoredDeploymentIntent) -> Self {
        Self {
            id: intent.id,
            pipeline_intent_id: intent.pipeline_intent_id,
            change_set_id: intent.change_set_id,
            work_plan_id: intent.work_plan_id,
            remediation_plan_id: intent.remediation_plan_id,
            incident_id: intent.incident_id,
            run_id: intent.run_id,
            status: intent.status,
            title: intent.title,
            summary: intent.summary,
            risk_level: intent.risk_level,
            intent_kind: intent.intent_kind,
            target_environment: intent.target_environment,
            target_namespace: intent.target_namespace,
            argo_application: intent.argo_application,
            resource_namespace: intent.resource_namespace,
            resource_kind: intent.resource_kind,
            resource_name: intent.resource_name,
            intent_json: intent.intent_json,
            created_at: intent.created_at,
            updated_at: intent.updated_at,
            status_changed_at: intent.status_changed_at,
            status_changed_by: intent.status_changed_by,
            status_reason: intent.status_reason,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateDeploymentIntentFromPipelineIntentRequest {
    pub pipeline_intent_id: String,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub risk_level: Option<String>,
    pub intent_kind: Option<String>,
    pub target_environment: Option<String>,
    pub target_namespace: Option<String>,
    pub argo_application: Option<String>,
    #[serde(default)]
    pub intent_json: Option<serde_json::Value>,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateDeploymentIntentResponse {
    pub deployment_intent: DeploymentIntentResponse,
    pub created: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransitionDeploymentIntentRequest {
    pub target_status: String,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransitionDeploymentIntentResponse {
    pub deployment_intent: DeploymentIntentResponse,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttachDeploymentIntentEvidenceResponse {
    pub deployment_intent: DeploymentIntentResponse,
    pub observation: ObservationResponse,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AttachDeploymentIntentEvidenceRequest {
    pub observation_id: String,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateDeploymentIntentTrustedEnvelopeRequest {
    pub subject: Option<String>,
    pub created_by: Option<String>,
    pub reason: String,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeploymentIntentPreflightRequest {
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeploymentIntentPreflightResponse {
    pub status: String,
    pub ready_for_argo_runner: bool,
    pub dispatch_ready: bool,
    pub deployment_intent: DeploymentIntentResponse,
    pub deployment_contract: Option<DeploymentContractResponse>,
    pub permission_grant: Option<PermissionGrantResponse>,
    pub checks: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExecuteDeploymentIntentRequest {
    #[serde(default = "default_dry_run")]
    pub dry_run: bool,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecuteDeploymentIntentResponse {
    pub status: String,
    pub ready: bool,
    pub dry_run: bool,
    pub deployment_intent: DeploymentIntentResponse,
    pub deployment_contract: Option<DeploymentContractResponse>,
    pub permission_grant: Option<PermissionGrantResponse>,
    pub checks: Vec<serde_json::Value>,
    pub execution: Option<ArtifactResponse>,
    pub execution_id: Option<String>,
    pub executor_job_name: Option<String>,
    pub created: bool,
}

/// Durable deployment progress used by a WorkItem controller. It is derived
/// from execution artifacts and never represents permission to dispatch Argo.
#[derive(Debug, Clone, Serialize)]
pub struct DeploymentIntentDeliveryFlowResponse {
    pub latest_execution: Option<ArtifactResponse>,
    pub latest_result: Option<ArtifactResponse>,
    pub release: Option<ReleaseResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArgoSyncContextResponse {
    pub execution_id: String,
    pub target_namespace: String,
    pub argo_application: String,
    pub revision: Option<String>,
    pub poll_seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArgoSyncControlResponse {
    pub cancelled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArgoSyncOutcomeRequest {
    pub execution_id: String,
    pub status: String,
    #[serde(default)]
    pub sync_status: Option<String>,
    #[serde(default)]
    pub health_status: Option<String>,
    #[serde(default)]
    pub operation_phase: Option<String>,
    #[serde(default)]
    pub revision: Option<String>,
    #[serde(default)]
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleasesResponse {
    pub releases: Vec<ReleaseResponse>,
    pub count: usize,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseResponse {
    pub id: String,
    pub deployment_intent_id: String,
    pub pipeline_intent_id: String,
    pub change_set_id: String,
    pub work_plan_id: String,
    pub remediation_plan_id: Option<String>,
    pub incident_id: Option<String>,
    pub run_id: Option<RunId>,
    pub status: String,
    pub title: String,
    pub summary: String,
    pub risk_level: String,
    pub release_kind: String,
    pub target_environment: Option<String>,
    pub target_namespace: Option<String>,
    pub argo_application: Option<String>,
    pub version: Option<String>,
    pub commit_sha: Option<String>,
    pub image_digest: Option<String>,
    pub rollback_ref: Option<String>,
    pub release_json: serde_json::Value,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub status_changed_at: Option<String>,
    pub status_changed_by: Option<String>,
    pub status_reason: Option<String>,
}

impl From<StoredRelease> for ReleaseResponse {
    fn from(release: StoredRelease) -> Self {
        Self {
            id: release.id,
            deployment_intent_id: release.deployment_intent_id,
            pipeline_intent_id: release.pipeline_intent_id,
            change_set_id: release.change_set_id,
            work_plan_id: release.work_plan_id,
            remediation_plan_id: release.remediation_plan_id,
            incident_id: release.incident_id,
            run_id: release.run_id,
            status: release.status,
            title: release.title,
            summary: release.summary,
            risk_level: release.risk_level,
            release_kind: release.release_kind,
            target_environment: release.target_environment,
            target_namespace: release.target_namespace,
            argo_application: release.argo_application,
            version: release.version,
            commit_sha: release.commit_sha,
            image_digest: release.image_digest,
            rollback_ref: release.rollback_ref,
            release_json: release.release_json,
            created_at: release.created_at,
            updated_at: release.updated_at,
            status_changed_at: release.status_changed_at,
            status_changed_by: release.status_changed_by,
            status_reason: release.status_reason,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateReleaseFromDeploymentIntentRequest {
    pub deployment_intent_id: String,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub risk_level: Option<String>,
    pub release_kind: Option<String>,
    pub version: Option<String>,
    pub commit_sha: Option<String>,
    pub image_digest: Option<String>,
    pub rollback_ref: Option<String>,
    #[serde(default)]
    pub release_json: Option<serde_json::Value>,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateReleaseResponse {
    pub release: ReleaseResponse,
    pub created: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransitionReleaseRequest {
    pub target_status: String,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransitionReleaseResponse {
    pub release: ReleaseResponse,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AttachReleaseEvidenceRequest {
    pub observation_id: String,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttachReleaseEvidenceResponse {
    pub release: ReleaseResponse,
    pub observation: ObservationResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incident: Option<IncidentResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation_plan: Option<RemediationPlanResponse>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VerifyReleaseRequest {
    #[serde(default)]
    pub complete: bool,
    pub actor: Option<String>,
    pub reason: Option<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerifyReleaseResponse {
    pub status: String,
    pub verified: bool,
    pub completed: bool,
    pub release: ReleaseResponse,
    pub argo_observation: ObservationResponse,
    pub workload_observation: ObservationResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observability_observation: Option<ObservationResponse>,
    pub checks: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegistryEvidenceListResponse {
    pub registry_evidence: Vec<RegistryEvidenceResponse>,
    pub count: usize,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegistryEvidenceResponse {
    pub id: String,
    pub release_id: String,
    pub deployment_intent_id: String,
    pub pipeline_intent_id: String,
    pub change_set_id: String,
    pub work_plan_id: String,
    pub remediation_plan_id: Option<String>,
    pub incident_id: Option<String>,
    pub run_id: Option<RunId>,
    pub status: String,
    pub title: String,
    pub summary: String,
    pub risk_level: String,
    pub registry: Option<String>,
    pub repository: Option<String>,
    pub image_ref: Option<String>,
    pub image_digest: Option<String>,
    pub tag: Option<String>,
    pub source: String,
    pub verification_status: String,
    pub evidence_json: serde_json::Value,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub status_changed_at: Option<String>,
    pub status_changed_by: Option<String>,
    pub status_reason: Option<String>,
}

impl From<StoredRegistryEvidence> for RegistryEvidenceResponse {
    fn from(evidence: StoredRegistryEvidence) -> Self {
        Self {
            id: evidence.id,
            release_id: evidence.release_id,
            deployment_intent_id: evidence.deployment_intent_id,
            pipeline_intent_id: evidence.pipeline_intent_id,
            change_set_id: evidence.change_set_id,
            work_plan_id: evidence.work_plan_id,
            remediation_plan_id: evidence.remediation_plan_id,
            incident_id: evidence.incident_id,
            run_id: evidence.run_id,
            status: evidence.status,
            title: evidence.title,
            summary: evidence.summary,
            risk_level: evidence.risk_level,
            registry: evidence.registry,
            repository: evidence.repository,
            image_ref: evidence.image_ref,
            image_digest: evidence.image_digest,
            tag: evidence.tag,
            source: evidence.source,
            verification_status: evidence.verification_status,
            evidence_json: evidence.evidence_json,
            created_at: evidence.created_at,
            updated_at: evidence.updated_at,
            status_changed_at: evidence.status_changed_at,
            status_changed_by: evidence.status_changed_by,
            status_reason: evidence.status_reason,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateRegistryEvidenceFromReleaseRequest {
    pub release_id: String,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub risk_level: Option<String>,
    pub registry: Option<String>,
    pub repository: Option<String>,
    pub image_ref: Option<String>,
    pub image_digest: Option<String>,
    pub tag: Option<String>,
    pub source: Option<String>,
    pub verification_status: Option<String>,
    #[serde(default)]
    pub evidence_json: Option<serde_json::Value>,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateRegistryEvidenceResponse {
    pub registry_evidence: RegistryEvidenceResponse,
    pub created: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateRegistryEvidenceFromInspectionRequest {
    pub release_id: String,
    pub image_ref: String,
    pub registry_base_url: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub risk_level: Option<String>,
    pub actor: Option<String>,
    pub reason: Option<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateRegistryEvidenceFromInspectionResponse {
    pub registry_evidence: Option<RegistryEvidenceResponse>,
    pub created: bool,
    pub inspection: ExecuteCapabilityResponse,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransitionRegistryEvidenceRequest {
    pub target_status: String,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransitionRegistryEvidenceResponse {
    pub registry_evidence: RegistryEvidenceResponse,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalGatesResponse {
    pub approval_gates: Vec<ApprovalGateResponse>,
    pub groups: Vec<OperatorResourceGroupResponse>,
    pub count: usize,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalGateSummaryResponse {
    pub summary: ApprovalGateSummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalGateResponse {
    pub id: String,
    pub work_item_id: Option<String>,
    pub remediation_plan_id: Option<String>,
    pub incident_id: Option<String>,
    pub run_id: Option<RunId>,
    pub status: String,
    pub origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    pub gate_kind: String,
    pub gate_order: i64,
    pub title: String,
    pub summary: String,
    pub risk_level: String,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub gate_json: serde_json::Value,
    pub created_at: String,
    pub decided_at: Option<String>,
    pub decided_by: Option<String>,
    pub decision_reason: Option<String>,
    pub stale_at: Option<String>,
    pub stale_by: Option<String>,
    pub stale_reason: Option<String>,
    pub actionable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle_blocker: Option<String>,
}

impl From<StoredApprovalGate> for ApprovalGateResponse {
    fn from(gate: StoredApprovalGate) -> Self {
        let actionable = gate.status == "pending";
        Self {
            id: gate.id,
            work_item_id: gate.work_item_id,
            remediation_plan_id: gate.remediation_plan_id,
            incident_id: gate.incident_id,
            run_id: gate.run_id,
            status: gate.status,
            origin: gate.origin,
            created_by: gate.created_by,
            gate_kind: gate.gate_kind,
            gate_order: gate.gate_order,
            title: gate.title,
            summary: gate.summary,
            risk_level: gate.risk_level,
            resource_namespace: gate.resource_namespace,
            resource_kind: gate.resource_kind,
            resource_name: gate.resource_name,
            gate_json: gate.gate_json,
            created_at: gate.created_at,
            decided_at: gate.decided_at,
            decided_by: gate.decided_by,
            decision_reason: gate.decision_reason,
            stale_at: gate.stale_at,
            stale_by: gate.stale_by,
            stale_reason: gate.stale_reason,
            actionable,
            lifecycle_blocker: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DecideApprovalGateRequest {
    pub decided_by: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DecideApprovalGateResponse {
    pub approval_gate: ApprovalGateResponse,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BatchDecideApprovalGatesRequest {
    pub gate_ids: Vec<String>,
    pub decision: String,
    pub decided_by: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchDecideApprovalGatesResponse {
    pub approval_gates: Vec<ApprovalGateResponse>,
    pub batch_audit_event_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileChangeResponse {
    pub id: String,
    pub path: String,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
    pub diff: String,
    pub created_at: String,
}

impl From<StoredFileChange> for FileChangeResponse {
    fn from(change: StoredFileChange) -> Self {
        Self {
            id: change.id,
            path: change.path,
            before_hash: change.before_hash,
            after_hash: change.after_hash,
            diff: change.diff,
            created_at: change.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalsResponse {
    pub approvals: Vec<ApprovalResponse>,
    pub groups: Vec<OperatorResourceGroupResponse>,
    pub count: usize,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalSummaryResponse {
    pub summary: ApprovalSummary,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approve,
    Deny,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DecideApprovalRequest {
    pub decision: ApprovalDecision,
    pub decided_by: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewApprovalRequest {
    pub decided_by: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalResponse {
    pub id: String,
    pub run_id: RunId,
    pub status: String,
    pub kind: String,
    pub summary: String,
    pub risk_level: String,
    pub turns_completed: u32,
    pub requested_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<RunScope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<serde_json::Value>,
    pub action: Option<serde_json::Value>,
    pub origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
}

impl From<StoredApproval> for ApprovalResponse {
    fn from(approval: StoredApproval) -> Self {
        Self {
            id: approval.id,
            run_id: approval.run_id,
            status: approval.status,
            kind: approval.kind,
            summary: approval.summary,
            risk_level: approval.risk_level,
            turns_completed: approval.turns_completed,
            requested_at: approval.requested_at,
            decided_at: approval.decided_at,
            decided_by: approval.decided_by,
            decision_reason: approval.decision_reason,
            scope: approval
                .run_scope_json
                .and_then(|value| serde_json::from_value::<RunScope>(value).ok())
                .filter(|scope| !scope.is_empty()),
            preview: approval.preview_json,
            action: approval.action_json,
            origin: approval.origin,
            created_by: approval.created_by,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DecideApprovalResponse {
    pub approval: ApprovalResponse,
    pub run: RunResponse,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExecuteCapabilityRequest {
    pub action: AgentAction,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecuteCapabilityResponse {
    pub status: String,
    pub action: String,
    pub decision: PolicyDecision,
    pub executed: bool,
    pub cancelled: bool,
    pub timeout_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation_id: Option<String>,
    pub result: Option<ToolResult>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatePermissionGrantRequest {
    pub subject: String,
    #[serde(default)]
    pub created_by: Option<String>,
    pub reason: String,
    pub scope: serde_json::Value,
    pub policy: serde_json::Value,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateTrustedEnvelopeRequest {
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub created_by: Option<String>,
    pub reason: String,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub production_impacting: Option<bool>,
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrustedEnvelopeResponse {
    pub grant: PermissionGrantResponse,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RevokePermissionGrantRequest {
    pub revoked_by: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PermissionGrantsResponse {
    pub grants: Vec<PermissionGrantResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditEventsResponse {
    pub events: Vec<AuditEventResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditEventResponse {
    pub id: String,
    pub kind: String,
    pub actor: Option<String>,
    pub resource_kind: String,
    pub resource_id: String,
    pub run_id: Option<RunId>,
    pub origin: String,
    pub payload: serde_json::Value,
    pub created_at: String,
}

impl From<StoredAuditEvent> for AuditEventResponse {
    fn from(event: StoredAuditEvent) -> Self {
        Self {
            id: event.id,
            kind: event.kind,
            actor: event.actor,
            resource_kind: event.resource_kind,
            resource_id: event.resource_id,
            run_id: event.run_id,
            origin: event.origin,
            payload: event.payload_json,
            created_at: event.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PermissionGrantResponse {
    pub id: String,
    pub subject: String,
    pub status: String,
    pub reason: String,
    pub scope: serde_json::Value,
    pub policy: serde_json::Value,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
    pub revoked_by: Option<String>,
    pub revoke_reason: Option<String>,
}

impl From<StoredPermissionGrant> for PermissionGrantResponse {
    fn from(grant: StoredPermissionGrant) -> Self {
        Self {
            id: grant.id,
            subject: grant.subject,
            status: grant.status,
            reason: grant.reason,
            scope: grant.scope_json,
            policy: grant.policy_json,
            created_at: grant.created_at,
            expires_at: grant.expires_at,
            revoked_at: grant.revoked_at,
            revoked_by: grant.revoked_by,
            revoke_reason: grant.revoke_reason,
        }
    }
}
