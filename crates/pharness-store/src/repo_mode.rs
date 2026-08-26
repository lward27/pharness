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
pub struct StoredEvidenceValidation {
    pub id: String,
    pub work_item_id: String,
    pub stage_execution_id: Option<String>,
    pub validator_key: String,
    pub schema_version: String,
    pub status: String,
    pub subject: serde_json::Value,
    pub evidence_refs: serde_json::Value,
    pub facts: serde_json::Value,
    pub contradictions: serde_json::Value,
    pub content_hash: String,
    pub validated_at: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateOperatorAnnotationDecision {
    pub id: String,
    pub annotation_id: String,
    pub work_item_id: String,
    pub decision: String,
    pub action_id: String,
    pub actor: String,
    pub reason: String,
    pub state_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredOperatorAnnotationDecision {
    pub id: String,
    pub annotation_id: String,
    pub work_item_id: String,
    pub decision: String,
    pub action_id: String,
    pub actor: String,
    pub reason: String,
    pub state_hash: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateStageChainAuthorization {
    pub id: String,
    pub work_item_id: String,
    pub work_plan_id: String,
    pub work_plan_revision: i64,
    pub product_model_snapshot_id: String,
    pub product_model_snapshot_hash: String,
    pub repository_id: String,
    pub source_commit: String,
    pub workspace_id: String,
    pub writable_paths: serde_json::Value,
    pub profile_chain: serde_json::Value,
    pub budget_chain: serde_json::Value,
    pub state_hash: String,
    pub created_by: String,
    pub creation_reason: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredStageChainAuthorization {
    pub id: String,
    pub work_item_id: String,
    pub work_plan_id: String,
    pub work_plan_revision: i64,
    pub product_model_snapshot_id: String,
    pub product_model_snapshot_hash: String,
    pub repository_id: String,
    pub source_commit: String,
    pub workspace_id: String,
    pub writable_paths: serde_json::Value,
    pub profile_chain: serde_json::Value,
    pub budget_chain: serde_json::Value,
    pub state_hash: String,
    pub status: String,
    pub created_by: String,
    pub creation_reason: String,
    pub created_at: String,
    pub expires_at: String,
    pub revoked_at: Option<String>,
    pub revocation_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSourceDeliveryIntent {
    pub id: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub repository_id: String,
    pub source_repo: String,
    pub base_ref: String,
    pub base_commit: String,
    pub head_branch: String,
    pub patch_artifact_id: Option<String>,
    pub patch_hash: String,
    pub authorization: serde_json::Value,
    pub created_by: String,
    pub creation_reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSourceDeliveryIntent {
    pub id: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub repository_id: String,
    pub source_repo: String,
    pub base_ref: String,
    pub base_commit: String,
    pub head_branch: String,
    pub patch_artifact_id: Option<String>,
    pub patch_hash: String,
    pub status: String,
    pub state_version: u64,
    pub authorization: serde_json::Value,
    pub writer_execution_id: Option<String>,
    pub observer_execution_id: Option<String>,
    pub pull_request: Option<serde_json::Value>,
    pub merge_provenance: Option<serde_json::Value>,
    pub provider_checks: Option<serde_json::Value>,
    pub created_by: String,
    pub creation_reason: String,
    pub created_at: String,
    pub updated_at: String,
    pub status_changed_at: String,
    pub status_changed_by: Option<String>,
    pub status_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateEvidenceRetrieval {
    pub id: String,
    pub event_id: String,
    pub work_item_id: String,
    pub stage_execution_id: String,
    pub run_id: RunId,
    pub actor: String,
    pub evidence_kind: String,
    pub evidence_id: String,
    pub evidence_version: String,
    pub returned_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateProviderCheckSetObservation {
    pub id: String,
    pub source_delivery_intent_id: String,
    pub phase: String,
    pub repository_id: String,
    pub pull_request_number: u64,
    pub head_sha: String,
    pub required_set_hash: String,
    pub authoritative_rules_succeeded: bool,
    pub status: String,
    pub required_checks: serde_json::Value,
    pub check_runs: serde_json::Value,
    pub commit_statuses: serde_json::Value,
    pub content_hash: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredProviderCheckSetObservation {
    pub id: String,
    pub source_delivery_intent_id: String,
    pub phase: String,
    pub repository_id: String,
    pub pull_request_number: u64,
    pub head_sha: String,
    pub required_set_hash: String,
    pub authoritative_rules_succeeded: bool,
    pub status: String,
    pub required_checks: serde_json::Value,
    pub check_runs: serde_json::Value,
    pub commit_statuses: serde_json::Value,
    pub content_hash: String,
    pub observed_at: String,
    pub expires_at: String,
}
