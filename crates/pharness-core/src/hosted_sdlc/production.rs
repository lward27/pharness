//! Material for one human production decision. These pure checks neither
//! authenticate an approver nor grant a worker permission to change GitOps.
//! The controller must establish native evidence origin and persist admission.
use super::gitops_patch::update_kustomization_image;
use crate::canonical_json_sha256;
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod approval;
pub use approval::HumanProductionApproval;
#[cfg(test)]
mod tests;

pub const PROPOSAL_SCHEMA: &str = "pharness.dev/hosted-production-proposal/v1alpha1";
pub const PLAN_SCHEMA: &str = "pharness.dev/hosted-production-plan/v1alpha1";
pub const APPROVAL_SCHEMA: &str = "pharness.dev/hosted-production-approval/v1alpha1";
pub const MAX_APPROVAL_MS: i64 = 30 * 60 * 1000;

/// References identify controller-sealed evidence, not agent assertions. Changing
/// any reference, source, digest, target or recovery permission changes the
/// decision material. An approval cannot transfer to another operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionProposal {
    pub schema_version: String,
    pub work_item_id: String,
    pub operation_id: String,
    pub pipeline_intent_id: String,
    pub deployment_intent_id: String,
    pub workflow_policy_hash: String,
    pub application_repository: String,
    pub source_commit_sha: String,
    pub build_evidence_hash: String,
    pub image_digest: String,
    pub staging_evidence_artifact_id: String,
    pub staging_evidence_hash: String,
    pub baseline_evidence_artifact_id: String,
    pub baseline_evidence_hash: String,
    pub previous_image_digest: String,
    pub production_configuration_hash: String,
    pub rollback_compatibility_hash: String,
    pub rollback: super::HostedRollbackPermission,
    pub created_at_ms: i64,
}

impl ProductionProposal {
    pub fn coordinates(&self) -> Result<(&'static str, &'static str), String> {
        match self.application_repository.as_str() {
            "https://github.com/lward27/yfinance_wrapper.git" => Ok((
                "charts/yfinance-wrapper/kustomization.yaml",
                "registry.lucas.engineering/yfinance_wrapper",
            )),
            "https://github.com/lward27/finance-frontend.git" => Ok((
                "charts/finance-frontend/kustomization.yaml",
                "registry.lucas.engineering/finance-frontend",
            )),
            _ => Err("production requires a registered Finance application".into()),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != PROPOSAL_SCHEMA
            || [
                &self.work_item_id,
                &self.operation_id,
                &self.pipeline_intent_id,
                &self.deployment_intent_id,
                &self.staging_evidence_artifact_id,
                &self.baseline_evidence_artifact_id,
            ]
            .iter()
            .any(|v| !identifier(v))
            || !hex(&self.source_commit_sha, 40)
            || [
                &self.workflow_policy_hash,
                &self.build_evidence_hash,
                &self.image_digest,
                &self.staging_evidence_hash,
                &self.baseline_evidence_hash,
                &self.previous_image_digest,
                &self.production_configuration_hash,
                &self.rollback_compatibility_hash,
            ]
            .iter()
            .any(|v| !digest(v))
            || self.created_at_ms <= 0
            || self.image_digest == self.previous_image_digest
            || self.staging_evidence_artifact_id == self.baseline_evidence_artifact_id
        {
            return Err("production decision requires distinct immutable candidate/baseline images and recorded source, evidence and configuration identities".into());
        }
        self.coordinates()?;
        Ok(())
    }

    pub fn material_hash(&self) -> Result<String, String> {
        material_hash(self)
    }

    pub fn image_ref(&self) -> Result<String, String> {
        Ok(format!("{}@{}", self.coordinates()?.1, self.image_digest))
    }
}

/// Retain both complete file contents for review. A production change can only
/// replace the single digest already pinned in the authoritative application
/// path. Namespace ownership is also checked by the native Argo/Deployment
/// reader; existing production Kustomizations do not declare a namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionGitOpsPlan {
    pub schema_version: String,
    pub proposal_hash: String,
    pub base_commit_sha: String,
    pub original_blob_sha: String,
    pub original_content: String,
    pub updated_content: String,
}

impl ProductionGitOpsPlan {
    pub fn validate(&self, proposal: &ProductionProposal) -> Result<(), String> {
        proposal.validate()?;
        if self.schema_version != PLAN_SCHEMA
            || self.proposal_hash != proposal.material_hash()?
            || !hex(&self.base_commit_sha, 40)
            || !hex(&self.original_blob_sha, 40)
            || self.original_content.is_empty()
            || self.original_content.len() > super::staging::MAX_FILE_BYTES
            || self.updated_content.len() > super::staging::MAX_FILE_BYTES
        {
            return Err(
                "production plan requires its original proposal, GitOps revision and bounded file"
                    .into(),
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
        let (_, image) = proposal.coordinates()?;
        // The baseline digest must be the value in the actual observed file;
        // merely hashing an unrelated healthy deployment cannot authorize it.
        let baseline = update_kustomization_image(
            &self.original_content,
            image,
            &format!("{image}@{}", proposal.previous_image_digest),
        )?;
        let candidate =
            update_kustomization_image(&self.original_content, image, &proposal.image_ref()?)?;
        if baseline != self.original_content
            || candidate != self.updated_content
            || self.original_content == self.updated_content
        {
            return Err("production plan must replace exactly the recorded baseline digest with the staged candidate; other changes and no-ops are unavailable".into());
        }
        Ok(())
    }

    pub fn material_hash(&self) -> Result<String, String> {
        material_hash(self)
    }
}

fn material_hash(value: &impl Serialize) -> Result<String, String> {
    canonical_json_sha256(&serde_json::to_value(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|v| hex(v, 64))
}
