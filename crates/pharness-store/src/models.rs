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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateApproval {
    pub id: String,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub status: String,
    pub kind: String,
    pub summary: String,
    pub risk_level: String,
    pub action_json: Option<serde_json::Value>,
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
    pub action_json: Option<serde_json::Value>,
    pub resume_messages_json: Option<serde_json::Value>,
    pub turns_completed: u32,
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
