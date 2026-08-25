use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootstrapOrganization {
    pub id: String,
    pub organization_key: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredOrganization {
    pub id: String,
    pub organization_key: String,
    pub display_name: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateProductAggregate {
    pub id: String,
    pub organization_id: String,
    pub product_key: String,
    pub display_name: String,
    pub description: String,
    pub owner_principal: String,
    pub snapshot_id: String,
    pub snapshot_json: serde_json::Value,
    pub snapshot_hash: String,
    pub actor: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredProduct {
    pub id: String,
    pub organization_id: String,
    pub product_key: String,
    pub display_name: String,
    pub description: String,
    pub owner_principal: String,
    pub state_version: u64,
    pub current_model_snapshot_id: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateProductAggregate {
    pub id: String,
    pub expected_state_version: u64,
    pub product_key: String,
    pub display_name: String,
    pub description: String,
    pub owner_principal: String,
    pub snapshot_id: String,
    pub snapshot_json: serde_json::Value,
    pub snapshot_hash: String,
    pub actor: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredProductModelSnapshot {
    pub id: String,
    pub product_id: String,
    pub version: u64,
    pub model_json: serde_json::Value,
    pub content_hash: String,
    pub created_by: String,
    pub creation_reason: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRepository {
    pub id: String,
    pub provider: String,
    pub external_id: String,
    pub canonical_url: String,
    pub default_branch: String,
    pub registered_commit: String,
    pub state_version: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredService {
    pub id: String,
    pub product_id: String,
    pub service_key: String,
    pub display_name: String,
    pub description: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRepositoryBinding {
    pub id: String,
    pub product_id: String,
    pub repository_id: String,
    pub status: String,
    pub current_revision_id: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRepositoryBindingRevision {
    pub id: String,
    pub binding_id: String,
    pub revision: u64,
    pub service_ids: Vec<String>,
    pub scopes: Vec<String>,
    pub status: String,
    pub evidence_json: serde_json::Value,
    pub content_hash: String,
    pub reviewed_by: String,
    pub review_reason: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterRepositoryAggregate {
    pub repository: StoredRepositoryDraft,
    pub binding_id: String,
    pub binding_revision_id: String,
    pub onboarding_id: String,
    pub binding_content_hash: String,
    pub evidence_json: serde_json::Value,
    pub product_id: String,
    pub expected_product_state_version: u64,
    pub snapshot_id: String,
    pub snapshot_json: serde_json::Value,
    pub snapshot_hash: String,
    pub actor: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRepositoryDraft {
    pub id: String,
    pub provider: String,
    pub external_id: String,
    pub canonical_url: String,
    pub default_branch: String,
    pub registered_commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisteredRepositoryAggregate {
    pub repository: StoredRepository,
    pub binding: StoredRepositoryBinding,
    pub binding_revision: StoredRepositoryBindingRevision,
    pub snapshot: StoredProductModelSnapshot,
    pub onboarding: crate::StoredRepositoryOnboarding,
}
