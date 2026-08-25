use pharness_core::{RunBudget, RunId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRepoWorkItem {
    pub id: String,
    pub product_id: String,
    pub repository_id: String,
    pub product_model_snapshot_id: String,
    pub product_model_snapshot_hash: String,
    pub repository_contract_version_id: String,
    pub contract_version: String,
    pub title: String,
    pub intent: String,
    pub acceptance_command_names: Vec<String>,
    pub acceptance_commands: Vec<String>,
    pub context_repositories: serde_json::Value,
    pub source_repo: String,
    pub source_ref: String,
    pub source_commit: String,
    pub environment_profile_id: String,
    pub run_budget: RunBudget,
    pub max_attempts: u32,
    pub repository_contract_json: serde_json::Value,
    pub repository_contract_hash: String,
    pub actor: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRepoWorkItemMetadata {
    pub work_item_id: String,
    pub mode: String,
    pub product_id: String,
    pub repository_id: String,
    pub product_model_snapshot_id: String,
    pub product_model_snapshot_hash: String,
    pub repository_contract_version_id: String,
    pub contract_version: String,
    pub acceptance_command_names: Vec<String>,
    pub context_repositories: serde_json::Value,
    pub current_stage_execution_id: Option<String>,
    pub state_version: u64,
    pub closed_at: Option<String>,
    pub closure_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateStageExecution {
    pub id: String,
    pub work_item_id: String,
    pub stage_key: String,
    pub sequence: u64,
    pub status: String,
    pub agent_profile_id: Option<String>,
    pub agent_profile_version: Option<String>,
    pub agent_profile_hash: Option<String>,
    pub context_pack_id: Option<String>,
    pub run_id: Option<RunId>,
    pub workspace_id: Option<String>,
    pub input_snapshot: serde_json::Value,
    pub input_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredStageExecution {
    pub id: String,
    pub work_item_id: String,
    pub stage_key: String,
    pub sequence: u64,
    pub status: String,
    pub agent_profile_id: Option<String>,
    pub agent_profile_version: Option<String>,
    pub agent_profile_hash: Option<String>,
    pub context_pack_id: Option<String>,
    pub run_id: Option<RunId>,
    pub workspace_id: Option<String>,
    pub input_snapshot: serde_json::Value,
    pub input_hash: String,
    pub stop_reason: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SealStageOutcome {
    pub id: String,
    pub stage_execution_id: String,
    pub work_item_id: String,
    pub stage_key: String,
    pub status: String,
    pub outcome: serde_json::Value,
    pub content_hash: String,
    pub state_version: u64,
    pub supersedes_outcome_id: Option<String>,
    pub actor: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredStageOutcome {
    pub id: String,
    pub stage_execution_id: String,
    pub work_item_id: String,
    pub stage_key: String,
    pub status: String,
    pub schema_version: String,
    pub outcome: serde_json::Value,
    pub content_hash: String,
    pub state_version: u64,
    pub supersedes_outcome_id: Option<String>,
    pub sealed_by: String,
    pub sealed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateEvidenceValidation {
    pub id: String,
    pub work_item_id: String,
    pub stage_execution_id: Option<String>,
    pub validator_key: String,
    pub status: String,
    pub subject: serde_json::Value,
    pub evidence_refs: serde_json::Value,
    pub facts: serde_json::Value,
    pub contradictions: serde_json::Value,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAgentContextPack {
    pub id: String,
    pub work_item_id: String,
    pub stage_execution_id: String,
    pub context: serde_json::Value,
    pub estimated_tokens: u64,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAgentContextPack {
    pub id: String,
    pub work_item_id: String,
    pub stage_execution_id: String,
    pub schema_version: String,
    pub context: serde_json::Value,
    pub estimated_tokens: u64,
    pub content_hash: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateOperatorAnnotation {
    pub id: String,
    pub work_item_id: String,
    pub target_kind: String,
    pub target_id: String,
    pub statement: String,
    pub evidence_refs: serde_json::Value,
    pub requested_effect: String,
    pub actor: String,
    pub reason: String,
    pub state_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredOperatorAnnotation {
    pub id: String,
    pub work_item_id: String,
    pub target_kind: String,
    pub target_id: String,
    pub statement: String,
    pub evidence_refs: serde_json::Value,
    pub requested_effect: String,
    pub actor: String,
    pub reason: String,
    pub state_hash: String,
    pub created_at: String,
}
