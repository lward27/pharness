use pharness_core::{AgentAction, AgentEvent, PolicyDecision, RunId, ToolResult};
use pharness_store::{StoredApproval, StoredArtifact, StoredFileChange, StoredRun};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct CreateRunRequest {
    pub task: String,
    pub cwd: Option<String>,
    pub max_turns: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunResponse {
    pub id: RunId,
    pub status: String,
    pub task: String,
    pub max_turns: u32,
    pub result: Option<serde_json::Value>,
}

impl From<StoredRun> for RunResponse {
    fn from(run: StoredRun) -> Self {
        Self {
            id: run.id,
            status: run.status,
            task: run.user_task,
            max_turns: run.max_turns,
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

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalResponse {
    pub id: String,
    pub run_id: RunId,
    pub status: String,
    pub kind: String,
    pub summary: String,
    pub risk_level: String,
    pub turns_completed: u32,
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
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecuteCapabilityResponse {
    pub status: String,
    pub action: String,
    pub decision: PolicyDecision,
    pub executed: bool,
    pub result: Option<ToolResult>,
    pub error: Option<String>,
}
