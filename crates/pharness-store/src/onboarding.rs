use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRepositoryOnboarding {
    pub id: String,
    pub product_id: String,
    pub repository_id: String,
    pub binding_id: String,
    pub onboarding_kind: String,
    pub status: String,
    pub registered_commit: String,
    pub resolved_commit: Option<String>,
    pub current_discovery_id: Option<String>,
    pub current_proposal_revision: u64,
    pub approved_proposal_hash: Option<String>,
    pub source_delivery_intent_id: Option<String>,
    pub contract_version_id: Option<String>,
    pub readiness_assessment_id: Option<String>,
    pub proposer_run_id: Option<String>,
    pub proposer_profile_hash: Option<String>,
    pub proposer_stop_reason: Option<String>,
    pub patch_execution_id: Option<String>,
    pub patch_artifact_id: Option<String>,
    pub patch_hash: Option<String>,
    pub validation_execution_id: Option<String>,
    pub validation_stop_reason: Option<String>,
    pub state_version: u64,
    pub blockers: Vec<serde_json::Value>,
    pub created_by: String,
    pub creation_reason: String,
    pub created_at: String,
    pub updated_at: String,
    pub status_changed_at: String,
    pub status_changed_by: Option<String>,
    pub status_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRepositoryOnboarding {
    pub id: String,
    pub product_id: String,
    pub repository_id: String,
    pub binding_id: String,
    pub onboarding_kind: String,
    pub registered_commit: String,
    pub actor: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRepositoryDiscovery {
    pub id: String,
    pub onboarding_id: String,
    pub source_commit: String,
    pub resolved_commit: Option<String>,
    pub status: String,
    pub schema_version: String,
    pub inventory_json: Option<serde_json::Value>,
    pub content_hash: Option<String>,
    pub error_code: Option<String>,
    pub error_summary: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRepositoryOnboardingProposal {
    pub id: String,
    pub onboarding_id: String,
    pub expected_state_version: u64,
    pub proposal: serde_json::Value,
    pub content_hash: String,
    pub discovery_id: String,
    pub discovery_hash: String,
    pub actor: String,
    pub origin: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRepositoryOnboardingProposal {
    pub id: String,
    pub onboarding_id: String,
    pub revision: u64,
    pub status: String,
    pub proposal: serde_json::Value,
    pub content_hash: String,
    pub discovery_id: String,
    pub discovery_hash: String,
    pub created_by: String,
    pub origin: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovedOnboardingService {
    pub id: String,
    pub service_key: String,
    pub display_name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovedOnboardingProductModelChange {
    pub product_id: String,
    pub expected_product_state_version: u64,
    pub services: Vec<ApprovedOnboardingService>,
    pub binding_id: String,
    pub binding_revision_id: Option<String>,
    pub binding_service_ids: Vec<String>,
    pub binding_scopes: Vec<String>,
    pub binding_evidence: serde_json::Value,
    pub binding_content_hash: Option<String>,
    pub snapshot_id: String,
    pub snapshot: serde_json::Value,
    pub snapshot_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApproveRepositoryOnboardingProposal {
    pub onboarding_id: String,
    pub proposal_id: String,
    pub proposal_hash: String,
    pub expected_state_version: u64,
    pub actor: String,
    pub reason: String,
    pub model_change: Option<ApprovedOnboardingProductModelChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRepositoryContractVersion {
    pub id: String,
    pub repository_id: String,
    pub onboarding_id: String,
    pub source_commit: String,
    pub contract: serde_json::Value,
    pub content_hash: String,
    pub merge_provenance: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRepositoryContractVersion {
    pub id: String,
    pub repository_id: String,
    pub onboarding_id: String,
    pub source_commit: String,
    pub contract_path: String,
    pub api_version: String,
    pub contract: serde_json::Value,
    pub content_hash: String,
    pub merge_provenance: serde_json::Value,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRepositoryReadinessAssessment {
    pub id: String,
    pub repository_id: String,
    pub source_commit: String,
    pub contract_version_id: Option<String>,
    pub contract_hash: Option<String>,
    pub dependency_lock_hash: Option<String>,
    pub environment_profile_id: Option<String>,
    pub environment_profile_revision: Option<String>,
    pub runner_image_digest: Option<String>,
    pub validation_policy_version: String,
    pub contract_status: String,
    pub coding_status: String,
    pub checks: serde_json::Value,
    pub blockers: serde_json::Value,
    pub warnings: serde_json::Value,
    pub evidence_refs: serde_json::Value,
    pub input_hash: String,
    pub content_hash: String,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRepositoryReadinessAssessment {
    pub id: String,
    pub repository_id: String,
    pub source_commit: String,
    pub contract_version_id: Option<String>,
    pub contract_hash: Option<String>,
    pub dependency_lock_hash: Option<String>,
    pub environment_profile_id: Option<String>,
    pub environment_profile_revision: Option<String>,
    pub runner_image_digest: Option<String>,
    pub validation_policy_version: String,
    pub contract_status: String,
    pub coding_status: String,
    pub checks: serde_json::Value,
    pub blockers: serde_json::Value,
    pub warnings: serde_json::Value,
    pub evidence_refs: serde_json::Value,
    pub input_hash: String,
    pub content_hash: String,
    pub assessed_at: String,
    pub expires_at: Option<String>,
}
