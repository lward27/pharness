use pharness_core::{
    AgentAction, AgentEvent, PolicyDecision, PolicyMode, RunId, RunScope, ToolResult,
};
use pharness_store::{
    ApprovalGateSummary, ApprovalSummary, RunSummary, StoredApproval, StoredApprovalGate,
    StoredArtifact, StoredAuditEvent, StoredChangeSet, StoredDeploymentIntent, StoredFileChange,
    StoredIncident, StoredObservation, StoredPermissionGrant, StoredPipelineIntent,
    StoredRegistryEvidence, StoredRelease, StoredRemediationPlan, StoredRun, StoredWorkPlan,
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
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RunsResponse {
    pub runs: Vec<RunResponse>,
    pub count: usize,
    pub limit: u32,
    pub offset: u32,
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
    pub count: usize,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkPlanResponse {
    pub id: String,
    pub remediation_plan_id: String,
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
    pub work_plan_json: serde_json::Value,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub revision: i64,
    pub status_changed_at: Option<String>,
    pub status_changed_by: Option<String>,
    pub status_reason: Option<String>,
}

impl From<StoredWorkPlan> for WorkPlanResponse {
    fn from(plan: StoredWorkPlan) -> Self {
        Self {
            id: plan.id,
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
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateWorkPlanFromRemediationPlanRequest {
    pub remediation_plan_id: String,
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
    pub work_plan: WorkPlanResponse,
    pub change_set: Option<ChangeSetResponse>,
    pub pipeline_intent: Option<PipelineIntentResponse>,
    pub deployment_intent: Option<DeploymentIntentResponse>,
    pub release: Option<ReleaseResponse>,
    pub registry_evidence: Option<RegistryEvidenceResponse>,
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
    pub work_plan_id: String,
    pub remediation_plan_id: String,
    pub incident_id: String,
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
pub struct PipelineIntentResponse {
    pub id: String,
    pub change_set_id: String,
    pub work_plan_id: String,
    pub remediation_plan_id: String,
    pub incident_id: String,
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

#[derive(Debug, Clone, Serialize)]
pub struct CreatePipelineIntentResponse {
    pub pipeline_intent: PipelineIntentResponse,
    pub created: bool,
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
    pub remediation_plan_id: String,
    pub incident_id: String,
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
    pub remediation_plan_id: String,
    pub incident_id: String,
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
    pub remediation_plan_id: String,
    pub incident_id: String,
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
    pub remediation_plan_id: String,
    pub incident_id: String,
    pub run_id: Option<RunId>,
    pub status: String,
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
}

impl From<StoredApprovalGate> for ApprovalGateResponse {
    fn from(gate: StoredApprovalGate) -> Self {
        Self {
            id: gate.id,
            remediation_plan_id: gate.remediation_plan_id,
            incident_id: gate.incident_id,
            run_id: gate.run_id,
            status: gate.status,
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
