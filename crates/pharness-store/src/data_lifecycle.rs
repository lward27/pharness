use serde::{Deserialize, Serialize};

pub const RETENTION_POLICY_VERSION: &str = "pharness.dev/retention-policy/v1alpha1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseGeneration {
    pub id: String,
    pub created_at: String,
    pub initializing_revision: String,
    pub schema_version: String,
    pub purpose: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateArchiveRecord {
    pub id: String,
    pub database_generation_id: String,
    pub archived_generation_id: String,
    pub database_claim: String,
    pub archive_claim: String,
    pub database_sha256: String,
    pub manifest_sha256: String,
    pub archive: serde_json::Value,
    pub deletion_eligible_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredArchiveRecord {
    pub id: String,
    pub database_generation_id: String,
    pub archived_generation_id: String,
    pub database_claim: String,
    pub archive_claim: String,
    pub database_sha256: String,
    pub manifest_sha256: String,
    pub archive: serde_json::Value,
    pub status: String,
    pub created_at: String,
    pub deletion_eligible_at: String,
    pub deleted_at: Option<String>,
    pub deletion_receipt_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteArchiveRecord {
    pub archive_id: String,
    pub preview_id: String,
    pub receipt_id: String,
    pub state_hash: String,
    pub actor: String,
    pub reason: String,
    pub deleted_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRetentionHold {
    pub id: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub reason: String,
    pub actor: String,
    pub expires_at: Option<String>,
    pub state_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRetentionHold {
    pub id: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub reason: String,
    pub actor: String,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub released_at: Option<String>,
    pub released_by: Option<String>,
    pub release_reason: Option<String>,
    pub state_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRetentionPreview {
    pub id: String,
    pub database_generation_id: String,
    pub preview: serde_json::Value,
    pub content_hash: String,
    pub state_hash: String,
    pub actor: String,
    pub reason: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRetentionPreview {
    pub id: String,
    pub database_generation_id: String,
    pub policy_version: String,
    pub status: String,
    pub preview: serde_json::Value,
    pub content_hash: String,
    pub state_hash: String,
    pub actor: String,
    pub reason: String,
    pub created_at: String,
    pub expires_at: String,
    pub executed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRetentionReceipt {
    pub id: String,
    pub preview_id: String,
    pub database_generation_id: String,
    pub policy_version: String,
    pub status: String,
    pub receipt: serde_json::Value,
    pub content_hash: String,
    pub actor: String,
    pub reason: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataInventory {
    pub database_generation: Option<DatabaseGeneration>,
    pub table_counts: serde_json::Value,
    pub retained_bytes: serde_json::Value,
    pub active_holds: u64,
    pub archives: u64,
    pub as_of: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRunSummaryRecord {
    pub id: String,
    pub run_id: String,
    pub work_item_id: Option<String>,
    pub summary: serde_json::Value,
    pub content_hash: String,
    pub sealed_at: String,
    pub compacted_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceValidationReference {
    pub id: String,
    pub evidence_validation_id: String,
    pub reference_kind: String,
    pub reference_id: String,
    pub reference_hash: String,
    pub created_at: String,
}
