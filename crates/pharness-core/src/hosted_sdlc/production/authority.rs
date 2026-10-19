//! Bounded production identities. A preview reads the production Kustomization at
//! the current GitOps head so a proposal can bind the exact base, blob and
//! baseline digest. A delivery authority adds execution identity and a window to
//! one human decision; it can never outlast that decision.
use super::{
    coordinates, digest, hex, identifier, material_hash, HumanProductionApproval,
    ProductionGitOpsPlan, ProductionProposal,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PREVIEW_AUTHORITY_SCHEMA: &str = "pharness.dev/hosted-production-preview/v1alpha1";
pub const PREVIEW_SCHEMA: &str = "pharness.dev/hosted-production-preview-file/v1alpha1";
pub const AUTHORITY_SCHEMA: &str = "pharness.dev/hosted-production-gitops/v1alpha1";
pub const MAX_DELIVERY_MS: i64 = 3_600_000;

/// Read-only. The reader identity fetches one file at the GitOps head and
/// reports it; nothing in this record permits a write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionPreviewAuthority {
    pub schema_version: String,
    pub work_item_id: String,
    pub operation_id: String,
    pub execution_id: String,
    pub pipeline_intent_id: String,
    pub deployment_intent_id: String,
    pub workflow_policy_hash: String,
    pub application_repository: String,
    pub source_commit_sha: String,
    pub image_digest: String,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
}

impl ProductionPreviewAuthority {
    pub fn coordinates(&self) -> Result<(&'static str, &'static str), String> {
        coordinates(&self.application_repository)
    }

    pub fn validate_identity(&self) -> Result<(), String> {
        if self.schema_version != PREVIEW_AUTHORITY_SCHEMA
            || [
                &self.work_item_id,
                &self.operation_id,
                &self.execution_id,
                &self.pipeline_intent_id,
                &self.deployment_intent_id,
            ]
            .iter()
            .any(|id| !identifier(id))
            || !hex(&self.source_commit_sha, 40)
            || !digest(&self.workflow_policy_hash)
            || !digest(&self.image_digest)
            || self.created_at_ms <= 0
            || self.expires_at_ms <= self.created_at_ms
            || self.expires_at_ms > self.created_at_ms.saturating_add(MAX_DELIVERY_MS)
        {
            return Err("production preview requires immutable source, build, workflow and bounded operation identities".into());
        }
        self.coordinates()?;
        Ok(())
    }

    pub fn validate_for_dispatch(&self, now_ms: i64) -> Result<(), String> {
        self.validate_identity()?;
        if now_ms < self.created_at_ms || now_ms >= self.expires_at_ms {
            return Err("production preview is not within its original execution window".into());
        }
        Ok(())
    }

    pub fn material_hash(&self) -> Result<String, String> {
        material_hash(self)
    }

    pub fn image_ref(&self) -> Result<String, String> {
        Ok(format!("{}@{}", self.coordinates()?.1, self.image_digest))
    }
}

/// The production Kustomization exactly as observed at one GitOps commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionPreview {
    pub schema_version: String,
    pub authority_hash: String,
    pub base_commit_sha: String,
    pub original_blob_sha: String,
    pub original_content: String,
}

impl ProductionPreview {
    pub fn validate(&self, authority: &ProductionPreviewAuthority) -> Result<(), String> {
        authority.validate_identity()?;
        if self.schema_version != PREVIEW_SCHEMA
            || self.authority_hash != authority.material_hash()?
            || !hex(&self.base_commit_sha, 40)
            || !hex(&self.original_blob_sha, 40)
            || self.original_content.is_empty()
            || self.original_content.len() > super::super::staging::MAX_FILE_BYTES
        {
            return Err(
                "production preview is not bound to its authority and bounded GitOps file".into(),
            );
        }
        let document: Value = serde_yaml::from_str(&self.original_content)
            .map_err(|_| "production Kustomization is not valid YAML".to_string())?;
        if document["apiVersion"] != "kustomize.config.k8s.io/v1beta1"
            || document["kind"] != "Kustomization"
            || (!document["namespace"].is_null() && document["namespace"] != "apps-prod")
        {
            return Err("production Kustomization has an unsupported kind or namespace".into());
        }
        self.previous_image_digest(authority)?;
        Ok(())
    }

    /// The digest production runs today, read from the reviewed file rather
    /// than from any cluster claim.
    pub fn previous_image_digest(
        &self,
        authority: &ProductionPreviewAuthority,
    ) -> Result<String, String> {
        let document: Value = serde_yaml::from_str(&self.original_content)
            .map_err(|_| "production Kustomization is not valid YAML".to_string())?;
        let (_, image) = authority.coordinates()?;
        document["images"]
            .as_array()
            .and_then(|images| images.iter().find(|entry| entry["name"] == image))
            .and_then(|entry| entry["digest"].as_str())
            .filter(|value| digest(value))
            .map(str::to_owned)
            .ok_or_else(|| "production baseline requires the original immutable image".into())
    }

    pub fn material_hash(&self) -> Result<String, String> {
        material_hash(self)
    }
}

/// One admitted production write. The proposal and the human decision are
/// copied verbatim so the isolated writer can re-check them before its single
/// mutation; the controller still grants admission separately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedProductionAuthority {
    pub schema_version: String,
    pub execution_id: String,
    pub gitops_change_set_id: String,
    pub approval_artifact_id: String,
    pub proposal: ProductionProposal,
    pub approval: HumanProductionApproval,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
}

impl HostedProductionAuthority {
    pub fn coordinates(&self) -> Result<(&'static str, &'static str), String> {
        self.proposal.coordinates()
    }

    pub fn validate_identity(&self) -> Result<(), String> {
        self.proposal.validate()?;
        if self.schema_version != AUTHORITY_SCHEMA
            || [
                &self.execution_id,
                &self.gitops_change_set_id,
                &self.approval_artifact_id,
            ]
            .iter()
            .any(|id| !identifier(id))
            || self.approval.schema_version != super::APPROVAL_SCHEMA
            || self.approval.work_item_id != self.proposal.work_item_id
            || self.approval.operation_id != self.proposal.operation_id
            || self.approval.proposal_hash != self.proposal.material_hash()?
            || self.created_at_ms < self.approval.approved_at_ms
            || self.expires_at_ms <= self.created_at_ms
            || self.expires_at_ms > self.created_at_ms.saturating_add(MAX_DELIVERY_MS)
            || self.expires_at_ms > self.approval.expires_at_ms
        {
            return Err("production authority requires one structurally valid human decision over its own proposal and a window inside that decision".into());
        }
        Ok(())
    }

    /// The plan is required because the decision is bound to its hash; an
    /// authority alone never proves which diff was approved.
    pub fn validate_for_dispatch(
        &self,
        plan: &ProductionGitOpsPlan,
        now_ms: i64,
    ) -> Result<(), String> {
        self.validate_identity()?;
        self.approval
            .validate_for_delivery(&self.proposal, plan, now_ms)?;
        if now_ms < self.created_at_ms || now_ms >= self.expires_at_ms {
            return Err("production authority is not within its original execution window".into());
        }
        Ok(())
    }

    pub fn material_hash(&self) -> Result<String, String> {
        material_hash(self)
    }

    pub fn image_ref(&self) -> Result<String, String> {
        self.proposal.image_ref()
    }
}
