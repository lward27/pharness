use pharness_core::ResolvedInferenceBinding;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateStageInferenceSelection {
    pub id: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub stage_key: String,
    pub resolved_binding: ResolvedInferenceBinding,
    pub effective_settings: serde_json::Value,
    pub actor: String,
    pub reason: String,
    pub state_hash: String,
    pub supersedes_selection_id: Option<String>,
    pub stage_execution_id: Option<String>,
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredStageInferenceSelection {
    pub id: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub stage_key: String,
    pub target_id: String,
    pub target_revision: String,
    pub target_hash: String,
    pub policy_id: String,
    pub policy_revision: String,
    pub policy_hash: String,
    pub effective_settings: serde_json::Value,
    pub resolved_binding: ResolvedInferenceBinding,
    pub binding_hash: String,
    pub actor: String,
    pub reason: String,
    pub state_hash: String,
    pub supersedes_selection_id: Option<String>,
    pub stage_execution_id: Option<String>,
    pub run_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateInferenceTargetVerification {
    pub id: String,
    pub target_id: String,
    pub target_revision: String,
    pub target_hash: String,
    pub status: String,
    pub reachability: String,
    pub model_visible: bool,
    pub streaming_compatible: bool,
    pub tool_compatible: bool,
    pub observed_capabilities: serde_json::Value,
    pub sanitized_failure: Option<String>,
    pub actor: String,
    pub reason: String,
    pub config_hash: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredInferenceTargetVerification {
    pub id: String,
    pub target_id: String,
    pub target_revision: String,
    pub target_hash: String,
    pub status: String,
    pub reachability: String,
    pub model_visible: bool,
    pub streaming_compatible: bool,
    pub tool_compatible: bool,
    pub observed_capabilities: serde_json::Value,
    pub sanitized_failure: Option<String>,
    pub actor: String,
    pub reason: String,
    pub config_hash: String,
    pub created_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateInferencePolicyQualification {
    pub id: String,
    pub policy_id: String,
    pub policy_revision: String,
    pub policy_hash: String,
    pub target_id: String,
    pub target_revision: String,
    pub target_hash: String,
    pub agent_profile_id: String,
    pub agent_profile_hash: String,
    pub suite_id: String,
    pub suite_hash: String,
    pub runtime_revision: String,
    pub attempts: u32,
    pub metrics: serde_json::Value,
    pub verdict: String,
    pub evidence_artifact_id: Option<String>,
    pub actor: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredInferencePolicyQualification {
    pub id: String,
    pub policy_id: String,
    pub policy_revision: String,
    pub policy_hash: String,
    pub target_id: String,
    pub target_revision: String,
    pub target_hash: String,
    pub agent_profile_id: String,
    pub agent_profile_hash: String,
    pub suite_id: String,
    pub suite_hash: String,
    pub runtime_revision: String,
    pub attempts: u32,
    pub metrics: serde_json::Value,
    pub verdict: String,
    pub evidence_artifact_id: Option<String>,
    pub actor: String,
    pub reason: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateModelGrantIssuance {
    pub run_id: String,
    pub request_sequence: u32,
    pub selection_id: String,
    pub request_body_hash: String,
    pub nonce: String,
    pub issued_at_epoch_seconds: u64,
    pub expires_at_epoch_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateInferenceEvaluation {
    pub id: String,
    pub suite_id: String,
    pub suite_hash: String,
    pub attempts: u32,
    pub agent_profile_id: String,
    pub agent_profile_hash: String,
    pub resolved_binding: ResolvedInferenceBinding,
    pub runtime_revision: String,
    pub actor: String,
    pub reason: String,
    pub config_hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredInferenceEvaluation {
    pub id: String,
    pub status: String,
    pub suite_id: String,
    pub suite_hash: String,
    pub attempts: u32,
    pub agent_profile_id: String,
    pub agent_profile_hash: String,
    pub target_id: String,
    pub target_revision: String,
    pub target_hash: String,
    pub policy_id: String,
    pub policy_revision: String,
    pub policy_hash: String,
    pub resolved_binding: ResolvedInferenceBinding,
    pub binding_hash: String,
    pub runtime_revision: String,
    pub actor: String,
    pub reason: String,
    pub config_hash: String,
    pub job_name: Option<String>,
    pub report: Option<serde_json::Value>,
    pub report_hash: Option<String>,
    pub failure: Option<String>,
    pub qualification_id: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateInferenceEvaluationGrantIssuance {
    pub evaluation_id: String,
    pub fixture_run_id: String,
    pub request_sequence: u32,
    pub request_body_hash: String,
    pub nonce: String,
    pub issued_at_epoch_seconds: u64,
    pub expires_at_epoch_seconds: u64,
}
