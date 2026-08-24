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
