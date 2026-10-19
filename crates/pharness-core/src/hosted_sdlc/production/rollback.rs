//! One automatic return to the recorded previous image after a confirmed
//! production regression. The authority copies the proposal verbatim and binds
//! the failed window and the admitted commit it undoes; the plan is derived
//! from the file as observed, because no human reviews a rollback diff.
use super::{
    digest, hex, identifier, material_hash, update_kustomization_image, ProductionProposal,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const ROLLBACK_AUTHORITY_SCHEMA: &str = "pharness.dev/hosted-production-rollback/v1alpha1";
pub const ROLLBACK_PLAN_SCHEMA: &str = "pharness.dev/hosted-production-rollback-plan/v1alpha1";
pub const MAX_ROLLBACK_MS: i64 = 3_600_000;

/// The window whose failure authorizes the rollback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RollbackTrigger {
    Release,
    Observe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedProductionRollbackAuthority {
    pub schema_version: String,
    pub execution_id: String,
    pub delivery_execution_id: String,
    pub delivery_authority_hash: String,
    pub proposal: ProductionProposal,
    /// The approved commit this rollback reverses; the plan must still find
    /// the candidate pinned exactly as that commit left it.
    pub admitted_gitops_commit_sha: String,
    pub trigger: RollbackTrigger,
    pub failed_result_artifact_id: String,
    pub failed_result_sha256: String,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
}

impl HostedProductionRollbackAuthority {
    pub fn coordinates(&self) -> Result<(&'static str, &'static str), String> {
        self.proposal.coordinates()
    }

    pub fn validate_identity(&self) -> Result<(), String> {
        self.proposal.validate()?;
        if self.schema_version != ROLLBACK_AUTHORITY_SCHEMA
            || [
                &self.execution_id,
                &self.delivery_execution_id,
                &self.failed_result_artifact_id,
            ]
            .iter()
            .any(|id| !identifier(id))
            || self.execution_id == self.delivery_execution_id
            || !digest(&self.delivery_authority_hash)
            || !digest(&self.failed_result_sha256)
            || !hex(&self.admitted_gitops_commit_sha, 40)
            || self.proposal.rollback
                != super::super::HostedRollbackPermission::OnePreviousVerifiedDeployment
            || self.created_at_ms < self.proposal.created_at_ms
            || self.expires_at_ms <= self.created_at_ms
            || self.expires_at_ms > self.created_at_ms.saturating_add(MAX_ROLLBACK_MS)
        {
            return Err("production rollback requires a proposal that permits one previous deployment, the admitted commit it reverses and the failed window that proves the regression".into());
        }
        Ok(())
    }

    pub fn validate_for_dispatch(&self, now_ms: i64) -> Result<(), String> {
        self.validate_identity()?;
        if now_ms < self.created_at_ms || now_ms >= self.expires_at_ms {
            return Err("production rollback is not within its original execution window".into());
        }
        Ok(())
    }

    pub fn material_hash(&self) -> Result<String, String> {
        material_hash(self)
    }

    /// The image the rollback restores: the baseline the human approved
    /// replacing, never a digest read from the cluster.
    pub fn image_ref(&self) -> Result<String, String> {
        Ok(format!(
            "{}@{}",
            self.coordinates()?.1,
            self.proposal.previous_image_digest
        ))
    }
}

/// Derived by the writer from the file at the current head. It is accepted
/// only if that file still pins exactly the candidate the failed window ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionRollbackPlan {
    pub schema_version: String,
    pub authority_hash: String,
    pub base_commit_sha: String,
    pub original_blob_sha: String,
    pub original_content: String,
    pub updated_content: String,
}

impl ProductionRollbackPlan {
    pub fn from_observed(
        authority: &HostedProductionRollbackAuthority,
        base_commit_sha: String,
        original_blob_sha: String,
        original_content: String,
    ) -> Result<Self, String> {
        let (_, image) = authority.coordinates()?;
        let updated_content =
            update_kustomization_image(&original_content, image, &authority.image_ref()?)?;
        let plan = Self {
            schema_version: ROLLBACK_PLAN_SCHEMA.into(),
            authority_hash: authority.material_hash()?,
            base_commit_sha,
            original_blob_sha,
            original_content,
            updated_content,
        };
        plan.validate(authority)?;
        Ok(plan)
    }

    pub fn validate(&self, authority: &HostedProductionRollbackAuthority) -> Result<(), String> {
        authority.validate_identity()?;
        if self.schema_version != ROLLBACK_PLAN_SCHEMA
            || self.authority_hash != authority.material_hash()?
            || !hex(&self.base_commit_sha, 40)
            || !hex(&self.original_blob_sha, 40)
            || self.original_content.is_empty()
            || self.original_content.len() > super::super::staging::MAX_FILE_BYTES
            || self.updated_content.len() > super::super::staging::MAX_FILE_BYTES
        {
            return Err(
                "production rollback plan requires its authority, GitOps revision and bounded file"
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
        let (_, image) = authority.coordinates()?;
        // Any other edit since the approved commit means production is no
        // longer the state the failed window measured; stop instead of guessing.
        let candidate = update_kustomization_image(
            &self.original_content,
            image,
            &authority.proposal.image_ref()?,
        )?;
        let previous =
            update_kustomization_image(&self.original_content, image, &authority.image_ref()?)?;
        if candidate != self.original_content
            || previous != self.updated_content
            || self.original_content == self.updated_content
        {
            return Err("production rollback must replace exactly the released candidate digest with the recorded previous digest; a drifted file cannot be rolled back automatically".into());
        }
        Ok(())
    }

    pub fn material_hash(&self) -> Result<String, String> {
        material_hash(self)
    }

    pub fn commit_headline(&self, authority: &HostedProductionRollbackAuthority) -> String {
        format!(
            "PHarness production rollback: {}",
            authority.proposal.operation_id
        )
    }

    pub fn commit_body(
        &self,
        authority: &HostedProductionRollbackAuthority,
    ) -> Result<String, String> {
        Ok(format!(
            "WorkItem: {}\nDeployment: {}\nRollback-authority: {}\nPlan: {}\nReverses: {}\nFailed-window: {} {}\nTrigger: {}\nImage: {}\nReplaced-image: {}",
            authority.proposal.work_item_id,
            authority.proposal.deployment_intent_id,
            self.authority_hash,
            self.material_hash()?,
            authority.admitted_gitops_commit_sha,
            authority.failed_result_artifact_id,
            authority.failed_result_sha256,
            match authority.trigger {
                RollbackTrigger::Release => "release",
                RollbackTrigger::Observe => "observe",
            },
            authority.image_ref()?,
            authority.proposal.image_ref()?
        ))
    }

    /// One server-side expected-head update, admitted at most once by the
    /// controller over the recorded failed window. Never retried.
    pub fn commit_request(
        &self,
        authority: &HostedProductionRollbackAuthority,
        now_ms: i64,
    ) -> Result<Value, String> {
        self.validate(authority)?;
        authority.validate_for_dispatch(now_ms)?;
        let (path, _) = authority.coordinates()?;
        Ok(json!({
            "query":"mutation PharnessProductionRollback($input: CreateCommitOnBranchInput!) { createCommitOnBranch(input: $input) { clientMutationId commit { oid } } }",
            "variables":{"input":{
                "branch":{"repositoryNameWithOwner":"lward27/lucas_engineering","refName":"main"},
                "expectedHeadOid":self.base_commit_sha,
                "clientMutationId":authority.execution_id,
                "message":{"headline":self.commit_headline(authority),"body":self.commit_body(authority)?},
                "fileChanges":{"additions":[{"path":path,"contents":STANDARD.encode(self.updated_content.as_bytes())}]}
            }}
        }))
    }
}
