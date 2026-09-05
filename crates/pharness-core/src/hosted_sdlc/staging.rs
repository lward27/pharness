//! Finite staging authority. Production cannot be expressed by this contract.
use super::gitops_patch::update_kustomization_image;
use crate::canonical_json_sha256;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const AUTHORITY_SCHEMA: &str = "pharness.dev/hosted-staging-gitops/v1alpha1";
pub const PLAN_SCHEMA: &str = "pharness.dev/hosted-staging-gitops-plan/v1alpha1";
pub const GITOPS_REPOSITORY: &str = "https://github.com/lward27/lucas_engineering.git";
pub const MAX_FILE_BYTES: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedStagingAuthority {
    pub schema_version: String,
    pub work_item_id: String,
    pub operation_id: String,
    pub execution_id: String,
    pub pipeline_intent_id: String,
    pub deployment_intent_id: String,
    pub gitops_change_set_id: String,
    pub workflow_policy_hash: String,
    pub build_evidence_hash: String,
    pub application_repository: String,
    pub source_commit_sha: String,
    pub image_digest: String,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
}

impl HostedStagingAuthority {
    pub fn coordinates(&self) -> Result<(&'static str, &'static str), String> {
        match self.application_repository.as_str() {
            "https://github.com/lward27/yfinance_wrapper.git" => Ok((
                "charts/finance-staging/yfinance/kustomization.yaml",
                "registry.lucas.engineering/yfinance_wrapper",
            )),
            "https://github.com/lward27/finance-frontend.git" => Ok((
                "charts/finance-staging/frontend/kustomization.yaml",
                "registry.lucas.engineering/finance-frontend",
            )),
            _ => Err("staging authority requires a registered Finance application".into()),
        }
    }

    pub fn validate_identity(&self) -> Result<(), String> {
        if self.schema_version != AUTHORITY_SCHEMA
            || [
                &self.work_item_id,
                &self.operation_id,
                &self.execution_id,
                &self.pipeline_intent_id,
                &self.deployment_intent_id,
                &self.gitops_change_set_id,
            ]
            .iter()
            .any(|id| !identity(id))
            || !sha(&self.source_commit_sha, 40)
            || [
                &self.workflow_policy_hash,
                &self.build_evidence_hash,
                &self.image_digest,
            ]
            .iter()
            .any(|value| !digest(value))
            || self.created_at_ms <= 0
            || self.expires_at_ms <= self.created_at_ms
            || self.expires_at_ms > self.created_at_ms.saturating_add(3_600_000)
        {
            return Err("staging authority requires immutable source, build, workflow and bounded operation identities".into());
        }
        self.coordinates()?;
        Ok(())
    }

    pub fn validate_for_dispatch(&self, now_ms: i64) -> Result<(), String> {
        self.validate_identity()?;
        if now_ms < self.created_at_ms || now_ms >= self.expires_at_ms {
            return Err("staging authority is not within its original execution window".into());
        }
        Ok(())
    }

    pub fn material_hash(&self) -> Result<String, String> {
        canonical_json_sha256(&serde_json::to_value(self).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }

    pub fn image_ref(&self) -> Result<String, String> {
        let (_, image) = self.coordinates()?;
        Ok(format!("{image}@{}", self.image_digest))
    }
}

/// The immutable file observed at a specific GitOps commit and its sole allowed
/// edit. Persist this before admission; a browser or worker cannot replace it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagingGitOpsPlan {
    pub schema_version: String,
    pub authority_hash: String,
    pub base_commit_sha: String,
    pub original_blob_sha: String,
    pub original_content: String,
    pub updated_content: String,
}

impl StagingGitOpsPlan {
    pub fn validate(&self, authority: &HostedStagingAuthority) -> Result<(), String> {
        authority.validate_identity()?;
        if self.schema_version != PLAN_SCHEMA
            || self.authority_hash != authority.material_hash()?
            || !sha(&self.base_commit_sha, 40)
            || !sha(&self.original_blob_sha, 40)
            || self.original_content.is_empty()
            || self.original_content.len() > MAX_FILE_BYTES
            || self.updated_content.len() > MAX_FILE_BYTES
        {
            return Err(
                "staging plan is not bound to the original authority and bounded GitOps file"
                    .into(),
            );
        }
        let document: Value = serde_yaml::from_str(&self.original_content)
            .map_err(|_| "staging Kustomization is not valid YAML".to_string())?;
        if document["apiVersion"] != "kustomize.config.k8s.io/v1beta1"
            || document["kind"] != "Kustomization"
            || document["namespace"] != "apps-staging"
        {
            return Err("staging Kustomization must explicitly target apps-staging".into());
        }
        let (_, image) = authority.coordinates()?;
        let expected =
            update_kustomization_image(&self.original_content, image, &authority.image_ref()?)?;
        if expected != self.updated_content || expected == self.original_content {
            return Err("staging plan must change exactly the declared image digest; other changes and no-ops are unavailable".into());
        }
        Ok(())
    }

    pub fn material_hash(&self) -> Result<String, String> {
        canonical_json_sha256(&serde_json::to_value(self).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }

    pub fn commit_headline(&self, authority: &HostedStagingAuthority) -> String {
        format!("PHarness staging: {}", authority.operation_id)
    }

    pub fn commit_body(&self, authority: &HostedStagingAuthority) -> Result<String, String> {
        Ok(format!(
            "WorkItem: {}\nDeployment: {}\nAuthority: {}\nPlan: {}\nSource: {}\nImage: {}",
            authority.work_item_id,
            authority.deployment_intent_id,
            self.authority_hash,
            self.material_hash()?,
            authority.source_commit_sha,
            authority.image_ref()?
        ))
    }

    /// One server-side expected-head update. clientMutationId is correlation,
    /// not an idempotency guarantee; the caller must never retry this POST.
    /// The controller must grant one admission after checking pause/deadline and
    /// current source/build/policy evidence. This function grants no authority.
    pub fn commit_request(
        &self,
        authority: &HostedStagingAuthority,
        now_ms: i64,
    ) -> Result<Value, String> {
        self.validate(authority)?;
        authority.validate_for_dispatch(now_ms)?;
        let (path, _) = authority.coordinates()?;
        Ok(json!({
            "query":"mutation PharnessStaging($input: CreateCommitOnBranchInput!) { createCommitOnBranch(input: $input) { clientMutationId commit { oid } } }",
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

fn identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
}
fn sha(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|v| sha(v, 64))
}

#[cfg(test)]
mod tests;
