use pharness_core::{RunId, SessionId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSession {
    pub id: SessionId,
    pub title: String,
    pub cwd: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRun {
    pub id: RunId,
    pub session_id: SessionId,
    pub user_task: String,
    pub cwd: String,
    pub max_turns: u32,
    pub initial_status: String,
    pub execution_target_json: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRun {
    pub id: RunId,
    pub session_id: SessionId,
    pub cwd: String,
    pub status: String,
    pub user_task: String,
    pub max_turns: u32,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub cancel_requested_at: Option<String>,
    pub error: Option<String>,
    pub result_json: Option<serde_json::Value>,
    pub execution_target_json: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RunListFilter {
    pub status: Option<String>,
    pub namespace: Option<String>,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub production_impacting: Option<bool>,
    pub started_after_ms: Option<i64>,
    pub started_before_ms: Option<i64>,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RunSummaryFilter {
    pub status: Option<String>,
    pub namespace: Option<String>,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub production_impacting: Option<bool>,
    pub started_after_ms: Option<i64>,
    pub started_before_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSummary {
    pub total: u64,
    pub by_status: Vec<CountBucket>,
    pub by_age_bucket: Vec<CountBucket>,
    pub by_namespace: Vec<CountBucket>,
    pub by_repo: Vec<CountBucket>,
    pub by_branch: Vec<CountBucket>,
    pub by_production_impacting: Vec<BooleanCountBucket>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateApproval {
    pub id: String,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub status: String,
    pub kind: String,
    pub summary: String,
    pub risk_level: String,
    pub run_scope_json: Option<serde_json::Value>,
    pub action_json: Option<serde_json::Value>,
    pub preview_json: Option<serde_json::Value>,
    pub resume_messages_json: Option<serde_json::Value>,
    pub turns_completed: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredApproval {
    pub id: String,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub status: String,
    pub kind: String,
    pub summary: String,
    pub risk_level: String,
    pub requested_at: String,
    pub decided_at: Option<String>,
    pub decided_by: Option<String>,
    pub decision_reason: Option<String>,
    pub run_scope_json: Option<serde_json::Value>,
    pub action_json: Option<serde_json::Value>,
    pub preview_json: Option<serde_json::Value>,
    pub resume_messages_json: Option<serde_json::Value>,
    pub turns_completed: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ApprovalListFilter {
    pub status: Option<String>,
    pub namespace: Option<String>,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub production_impacting: Option<bool>,
    pub requested_after_ms: Option<i64>,
    pub requested_before_ms: Option<i64>,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ApprovalSummaryFilter {
    pub status: Option<String>,
    pub namespace: Option<String>,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub production_impacting: Option<bool>,
    pub requested_after_ms: Option<i64>,
    pub requested_before_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CountBucket {
    pub value: Option<String>,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BooleanCountBucket {
    pub value: Option<bool>,
    pub count: u64,
}

pub type ApprovalCountBucket = CountBucket;
pub type ApprovalBooleanCountBucket = BooleanCountBucket;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalSummary {
    pub total: u64,
    pub by_status: Vec<ApprovalCountBucket>,
    pub by_kind: Vec<ApprovalCountBucket>,
    pub by_risk_level: Vec<ApprovalCountBucket>,
    pub by_age_bucket: Vec<ApprovalCountBucket>,
    pub by_namespace: Vec<ApprovalCountBucket>,
    pub by_repo: Vec<ApprovalCountBucket>,
    pub by_branch: Vec<ApprovalCountBucket>,
    pub by_production_impacting: Vec<ApprovalBooleanCountBucket>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateFileChange {
    pub id: String,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub path: String,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
    pub diff: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredFileChange {
    pub id: String,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub path: String,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
    pub diff: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateArtifact {
    pub id: String,
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub kind: String,
    pub label: String,
    pub mime_type: Option<String>,
    pub path: Option<String>,
    pub content_text: Option<String>,
    pub content_json: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredArtifact {
    pub id: String,
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub kind: String,
    pub label: String,
    pub mime_type: Option<String>,
    pub path: Option<String>,
    pub content_text: Option<String>,
    pub content_json: Option<serde_json::Value>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateObservation {
    pub id: String,
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub source: String,
    pub kind: String,
    pub subject: String,
    pub summary: String,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub resource_ref_json: Option<serde_json::Value>,
    pub artifact_id: Option<String>,
    pub data_json: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredObservation {
    pub id: String,
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub source: String,
    pub kind: String,
    pub subject: String,
    pub summary: String,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub resource_ref_json: Option<serde_json::Value>,
    pub artifact_id: Option<String>,
    pub data_json: serde_json::Value,
    pub observed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ObservationListFilter {
    pub run_id: Option<RunId>,
    pub source: Option<String>,
    pub kind: Option<String>,
    pub subject: Option<String>,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub observed_after_ms: Option<i64>,
    pub observed_before_ms: Option<i64>,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateIncident {
    pub id: String,
    pub observation_id: String,
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub status: String,
    pub severity: String,
    pub title: String,
    pub summary: String,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub data_json: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredIncident {
    pub id: String,
    pub observation_id: String,
    pub session_id: SessionId,
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

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct IncidentListFilter {
    pub run_id: Option<RunId>,
    pub status: Option<String>,
    pub severity: Option<String>,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub created_after_ms: Option<i64>,
    pub created_before_ms: Option<i64>,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRemediationPlan {
    pub id: String,
    pub incident_id: String,
    pub session_id: SessionId,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRemediationPlan {
    pub id: String,
    pub incident_id: String,
    pub session_id: SessionId,
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

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RemediationPlanListFilter {
    pub incident_id: Option<String>,
    pub run_id: Option<RunId>,
    pub status: Option<String>,
    pub risk_level: Option<String>,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub created_after_ms: Option<i64>,
    pub created_before_ms: Option<i64>,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateWorkPlan {
    pub id: String,
    pub remediation_plan_id: String,
    pub incident_id: String,
    pub session_id: SessionId,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredWorkPlan {
    pub id: String,
    pub remediation_plan_id: String,
    pub incident_id: String,
    pub session_id: SessionId,
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

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WorkPlanListFilter {
    pub remediation_plan_id: Option<String>,
    pub incident_id: Option<String>,
    pub run_id: Option<RunId>,
    pub status: Option<String>,
    pub risk_level: Option<String>,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub created_after_ms: Option<i64>,
    pub created_before_ms: Option<i64>,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateWorkPlanRevision {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub risk_level: Option<String>,
    pub requires_approval: Option<bool>,
    pub work_plan_json: serde_json::Value,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateChangeSet {
    pub id: String,
    pub work_plan_id: String,
    pub remediation_plan_id: String,
    pub incident_id: String,
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub status: String,
    pub title: String,
    pub summary: String,
    pub risk_level: String,
    pub material_hash: String,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub change_set_json: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredChangeSet {
    pub id: String,
    pub work_plan_id: String,
    pub remediation_plan_id: String,
    pub incident_id: String,
    pub session_id: SessionId,
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

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ChangeSetListFilter {
    pub work_plan_id: Option<String>,
    pub remediation_plan_id: Option<String>,
    pub incident_id: Option<String>,
    pub run_id: Option<RunId>,
    pub status: Option<String>,
    pub risk_level: Option<String>,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub created_after_ms: Option<i64>,
    pub created_before_ms: Option<i64>,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateChangeSetRevision {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub risk_level: Option<String>,
    pub material_hash: String,
    pub change_set_json: serde_json::Value,
    pub actor: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateApprovalGate {
    pub id: String,
    pub remediation_plan_id: String,
    pub incident_id: String,
    pub session_id: SessionId,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredApprovalGate {
    pub id: String,
    pub remediation_plan_id: String,
    pub incident_id: String,
    pub session_id: SessionId,
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

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ApprovalGateListFilter {
    pub remediation_plan_id: Option<String>,
    pub incident_id: Option<String>,
    pub run_id: Option<RunId>,
    pub status: Option<String>,
    pub gate_kind: Option<String>,
    pub risk_level: Option<String>,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub created_after_ms: Option<i64>,
    pub created_before_ms: Option<i64>,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ApprovalGateSummaryFilter {
    pub remediation_plan_id: Option<String>,
    pub incident_id: Option<String>,
    pub run_id: Option<RunId>,
    pub status: Option<String>,
    pub gate_kind: Option<String>,
    pub risk_level: Option<String>,
    pub resource_namespace: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_name: Option<String>,
    pub created_after_ms: Option<i64>,
    pub created_before_ms: Option<i64>,
}

pub type ApprovalGateCountBucket = CountBucket;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalGateSummary {
    pub total: u64,
    pub by_status: Vec<ApprovalGateCountBucket>,
    pub by_gate_kind: Vec<ApprovalGateCountBucket>,
    pub by_risk_level: Vec<ApprovalGateCountBucket>,
    pub by_age_bucket: Vec<ApprovalGateCountBucket>,
    pub by_resource_namespace: Vec<ApprovalGateCountBucket>,
    pub by_resource_kind: Vec<ApprovalGateCountBucket>,
    pub by_resource_name: Vec<ApprovalGateCountBucket>,
    pub by_incident_id: Vec<ApprovalGateCountBucket>,
    pub by_remediation_plan_id: Vec<ApprovalGateCountBucket>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatePermissionGrant {
    pub id: String,
    pub subject: String,
    pub reason: String,
    pub scope_json: serde_json::Value,
    pub policy_json: serde_json::Value,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredPermissionGrant {
    pub id: String,
    pub subject: String,
    pub status: String,
    pub reason: String,
    pub scope_json: serde_json::Value,
    pub policy_json: serde_json::Value,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
    pub revoked_by: Option<String>,
    pub revoke_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAuditEvent {
    pub id: String,
    pub kind: String,
    pub actor: Option<String>,
    pub resource_kind: String,
    pub resource_id: String,
    pub run_id: Option<RunId>,
    pub payload_json: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAuditEvent {
    pub id: String,
    pub kind: String,
    pub actor: Option<String>,
    pub resource_kind: String,
    pub resource_id: String,
    pub run_id: Option<RunId>,
    pub payload_json: serde_json::Value,
    pub created_at: String,
}
