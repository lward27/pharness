use pharness_core::RunId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSubjectWorkspace {
    pub id: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub run_id: Option<RunId>,
    pub status: String,
    pub source_repo: String,
    pub source_ref: String,
    pub source_commit: String,
    pub branch: Option<String>,
    pub retention_status: String,
    pub actor: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSubjectWorkspace {
    pub id: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub run_id: Option<RunId>,
    pub status: String,
    pub source_repo: String,
    pub source_ref: String,
    pub source_commit: String,
    pub resolved_commit: Option<String>,
    pub branch: Option<String>,
    pub retention_status: String,
    pub created_at: String,
    pub updated_at: String,
    pub status_changed_at: String,
    pub status_changed_by: Option<String>,
    pub status_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSubjectEnvironmentPreparation {
    pub id: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub workspace_id: String,
    pub run_id: Option<RunId>,
    pub status: String,
    pub environment_profile_id: String,
    pub source_commit: String,
    pub input_hash: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompleteSubjectEnvironmentPreparation {
    pub id: String,
    pub status: String,
    pub resolved_commit: Option<String>,
    pub repository_contract: Option<serde_json::Value>,
    pub repository_contract_hash: Option<String>,
    pub environment_snapshot: Option<serde_json::Value>,
    pub acceptance_results: serde_json::Value,
    pub logs: serde_json::Value,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSubjectEnvironmentPreparation {
    pub id: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub workspace_id: String,
    pub run_id: Option<RunId>,
    pub status: String,
    pub environment_profile_id: String,
    pub source_commit: String,
    pub input_hash: String,
    pub input: serde_json::Value,
    pub repository_contract: Option<serde_json::Value>,
    pub repository_contract_hash: Option<String>,
    pub environment_snapshot: Option<serde_json::Value>,
    pub acceptance_results: serde_json::Value,
    pub logs: serde_json::Value,
    pub error_code: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
