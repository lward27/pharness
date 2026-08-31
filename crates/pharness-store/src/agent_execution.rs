use pharness_core::{ResolvedAgentExecutionBinding, RunId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAgentExecutionSelection {
    pub id: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub stage_key: String,
    pub resolved_binding: ResolvedAgentExecutionBinding,
    pub actor: String,
    pub reason: String,
    pub state_hash: String,
    pub supersedes_selection_id: Option<String>,
    pub stage_execution_id: Option<String>,
    pub run_id: Option<RunId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAgentExecutionSelection {
    pub id: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub stage_key: String,
    pub policy_id: String,
    pub policy_revision: String,
    pub policy_hash: String,
    pub resolved_binding: ResolvedAgentExecutionBinding,
    pub binding_hash: String,
    pub actor: String,
    pub reason: String,
    pub state_hash: String,
    pub supersedes_selection_id: Option<String>,
    pub stage_execution_id: Option<String>,
    pub run_id: Option<RunId>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateAgentExecutionPolicyQualification {
    pub id: String,
    pub policy_id: String,
    pub policy_revision: String,
    pub policy_hash: String,
    pub runtime_revision: String,
    pub suite_id: String,
    pub suite_hash: String,
    pub attempts: u32,
    pub metrics: serde_json::Value,
    pub verdict: String,
    pub evidence_artifact_id: Option<String>,
    pub actor: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredAgentExecutionPolicyQualification {
    pub id: String,
    pub policy_id: String,
    pub policy_revision: String,
    pub policy_hash: String,
    pub runtime_revision: String,
    pub suite_id: String,
    pub suite_hash: String,
    pub attempts: u32,
    pub metrics: serde_json::Value,
    pub verdict: String,
    pub evidence_artifact_id: Option<String>,
    pub actor: String,
    pub reason: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAgentHostEnrollment {
    pub id: String,
    pub display_name: String,
    pub host_pool: String,
    pub token_hash: String,
    pub actor: String,
    pub reason: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAgentHostEnrollment {
    pub id: String,
    pub display_name: String,
    pub host_pool: String,
    pub actor: String,
    pub reason: String,
    pub created_at: String,
    pub expires_at: String,
    pub consumed_at: Option<String>,
    pub consumed_by_host_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrollAgentHost {
    pub id: String,
    pub enrollment_id: String,
    pub enrollment_token_hash: String,
    pub credential_hash: String,
    pub platform: String,
    pub architecture: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAgentHost {
    pub id: String,
    pub display_name: String,
    pub host_pool: String,
    pub lifecycle_state: String,
    pub enrollment_id: String,
    pub platform: String,
    pub architecture: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_contact_at: Option<String>,
    pub retired_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAgentHostCapabilitySnapshot {
    pub id: String,
    pub host_id: String,
    pub platform: String,
    pub architecture: String,
    pub codex_version: String,
    pub podman_version: Option<String>,
    pub execution_mode: String,
    pub authentication_class: String,
    pub authentication_ready: bool,
    pub supported_profiles: Vec<String>,
    pub runner_images: serde_json::Value,
    pub available_slots: u32,
    pub storage: serde_json::Value,
    pub status: String,
    pub blockers: serde_json::Value,
    pub content_hash: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAgentHostCapabilitySnapshot {
    pub id: String,
    pub host_id: String,
    pub platform: String,
    pub architecture: String,
    pub codex_version: String,
    pub podman_version: Option<String>,
    pub execution_mode: String,
    pub authentication_class: String,
    pub authentication_ready: bool,
    pub supported_profiles: Vec<String>,
    pub runner_images: serde_json::Value,
    pub available_slots: u32,
    pub storage: serde_json::Value,
    pub status: String,
    pub blockers: serde_json::Value,
    pub content_hash: String,
    pub created_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAgentLease {
    pub id: String,
    pub run_id: RunId,
    pub stage_execution_id: String,
    pub host_pool: String,
    pub pinned_host_id: Option<String>,
    pub workspace_id: String,
    pub environment_profile_id: String,
    pub runner_image: String,
    pub binding_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAgentLease {
    pub id: String,
    pub run_id: RunId,
    pub stage_execution_id: String,
    pub host_pool: String,
    pub pinned_host_id: Option<String>,
    pub host_id: Option<String>,
    pub workspace_id: String,
    pub environment_profile_id: String,
    pub runner_image: String,
    pub binding_hash: String,
    pub state: String,
    pub remote_thread_id: Option<String>,
    pub completion_hash: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
    pub claimed_at: Option<String>,
    pub heartbeat_at: Option<String>,
    pub expires_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimedAgentLease {
    pub lease: StoredAgentLease,
    pub lease_token: String,
}
