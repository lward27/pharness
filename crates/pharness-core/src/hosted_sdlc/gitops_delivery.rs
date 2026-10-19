//! One shape for every hosted GitOps write so the isolated worker and its
//! GitHub transport are written once. Each environment keeps its own authority
//! and plan types; the trait only exposes what the transport needs to read one
//! file, commit once on the expected head and prove the result afterwards.
use super::production::{HostedProductionAuthority, ProductionGitOpsPlan};
use super::staging::{HostedStagingAuthority, StagingGitOpsPlan};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

pub trait GitOpsAuthority: Serialize + DeserializeOwned + Clone + PartialEq {
    /// Lower-case environment name used in error codes and operation keys.
    const ENVIRONMENT: &'static str;
    fn coordinates(&self) -> Result<(&'static str, &'static str), String>;
    fn execution_id(&self) -> &str;
    fn deployment_intent_id(&self) -> &str;
    fn created_at_ms(&self) -> i64;
    fn expires_at_ms(&self) -> i64;
    fn validate_identity(&self) -> Result<(), String>;
    fn material_hash(&self) -> Result<String, String>;
    fn image_ref(&self) -> Result<String, String>;
}

pub trait GitOpsPlan: Serialize + DeserializeOwned + Clone + PartialEq {
    type Authority: GitOpsAuthority;
    /// Build the plan from the file the writer just read. Environments whose
    /// plan is fixed by a human decision refuse to derive one here.
    fn from_observed(
        authority: &Self::Authority,
        base_commit_sha: String,
        original_blob_sha: String,
        original_content: String,
    ) -> Result<Self, String>;
    fn base_commit_sha(&self) -> &str;
    fn original_blob_sha(&self) -> &str;
    fn updated_content(&self) -> &str;
    /// Hash of the record this plan is bound to (authority or proposal).
    fn binding_hash(&self) -> &str;
    fn validate(&self, authority: &Self::Authority) -> Result<(), String>;
    /// Full dispatch check including the plan, because some decisions are
    /// bound to the plan hash rather than to the authority alone.
    fn validate_for_dispatch(&self, authority: &Self::Authority, now_ms: i64)
        -> Result<(), String>;
    fn material_hash(&self) -> Result<String, String>;
    fn commit_headline(&self, authority: &Self::Authority) -> String;
    fn commit_body(&self, authority: &Self::Authority) -> Result<String, String>;
    fn commit_request(&self, authority: &Self::Authority, now_ms: i64) -> Result<Value, String>;
}

impl GitOpsAuthority for HostedStagingAuthority {
    const ENVIRONMENT: &'static str = "staging";
    fn coordinates(&self) -> Result<(&'static str, &'static str), String> {
        HostedStagingAuthority::coordinates(self)
    }
    fn execution_id(&self) -> &str {
        &self.execution_id
    }
    fn deployment_intent_id(&self) -> &str {
        &self.deployment_intent_id
    }
    fn created_at_ms(&self) -> i64 {
        self.created_at_ms
    }
    fn expires_at_ms(&self) -> i64 {
        self.expires_at_ms
    }
    fn validate_identity(&self) -> Result<(), String> {
        HostedStagingAuthority::validate_identity(self)
    }
    fn material_hash(&self) -> Result<String, String> {
        HostedStagingAuthority::material_hash(self)
    }
    fn image_ref(&self) -> Result<String, String> {
        HostedStagingAuthority::image_ref(self)
    }
}

impl GitOpsPlan for StagingGitOpsPlan {
    type Authority = HostedStagingAuthority;
    fn from_observed(
        authority: &Self::Authority,
        base_commit_sha: String,
        original_blob_sha: String,
        original_content: String,
    ) -> Result<Self, String> {
        let (_, image) = authority.coordinates()?;
        let updated_content = super::gitops_patch::update_kustomization_image(
            &original_content,
            image,
            &authority.image_ref()?,
        )?;
        let plan = Self {
            schema_version: super::staging::PLAN_SCHEMA.into(),
            authority_hash: authority.material_hash()?,
            base_commit_sha,
            original_blob_sha,
            original_content,
            updated_content,
        };
        plan.validate(authority)?;
        Ok(plan)
    }
    fn base_commit_sha(&self) -> &str {
        &self.base_commit_sha
    }
    fn original_blob_sha(&self) -> &str {
        &self.original_blob_sha
    }
    fn updated_content(&self) -> &str {
        &self.updated_content
    }
    fn binding_hash(&self) -> &str {
        &self.authority_hash
    }
    fn validate(&self, authority: &Self::Authority) -> Result<(), String> {
        StagingGitOpsPlan::validate(self, authority)
    }
    fn validate_for_dispatch(
        &self,
        authority: &Self::Authority,
        now_ms: i64,
    ) -> Result<(), String> {
        StagingGitOpsPlan::validate(self, authority)?;
        authority.validate_for_dispatch(now_ms)
    }
    fn material_hash(&self) -> Result<String, String> {
        StagingGitOpsPlan::material_hash(self)
    }
    fn commit_headline(&self, authority: &Self::Authority) -> String {
        StagingGitOpsPlan::commit_headline(self, authority)
    }
    fn commit_body(&self, authority: &Self::Authority) -> Result<String, String> {
        StagingGitOpsPlan::commit_body(self, authority)
    }
    fn commit_request(&self, authority: &Self::Authority, now_ms: i64) -> Result<Value, String> {
        StagingGitOpsPlan::commit_request(self, authority, now_ms)
    }
}

impl GitOpsAuthority for HostedProductionAuthority {
    const ENVIRONMENT: &'static str = "production";
    fn coordinates(&self) -> Result<(&'static str, &'static str), String> {
        HostedProductionAuthority::coordinates(self)
    }
    fn execution_id(&self) -> &str {
        &self.execution_id
    }
    fn deployment_intent_id(&self) -> &str {
        &self.proposal.deployment_intent_id
    }
    fn created_at_ms(&self) -> i64 {
        self.created_at_ms
    }
    fn expires_at_ms(&self) -> i64 {
        self.expires_at_ms
    }
    fn validate_identity(&self) -> Result<(), String> {
        HostedProductionAuthority::validate_identity(self)
    }
    fn material_hash(&self) -> Result<String, String> {
        HostedProductionAuthority::material_hash(self)
    }
    fn image_ref(&self) -> Result<String, String> {
        HostedProductionAuthority::image_ref(self)
    }
}

impl GitOpsPlan for ProductionGitOpsPlan {
    type Authority = HostedProductionAuthority;
    fn from_observed(
        _authority: &Self::Authority,
        _base_commit_sha: String,
        _original_blob_sha: String,
        _original_content: String,
    ) -> Result<Self, String> {
        Err("production plans are fixed by the human decision; the writer cannot derive one".into())
    }
    fn base_commit_sha(&self) -> &str {
        &self.base_commit_sha
    }
    fn original_blob_sha(&self) -> &str {
        &self.original_blob_sha
    }
    fn updated_content(&self) -> &str {
        &self.updated_content
    }
    fn binding_hash(&self) -> &str {
        &self.proposal_hash
    }
    fn validate(&self, authority: &Self::Authority) -> Result<(), String> {
        authority.validate_identity()?;
        ProductionGitOpsPlan::validate(self, &authority.proposal)
    }
    fn validate_for_dispatch(
        &self,
        authority: &Self::Authority,
        now_ms: i64,
    ) -> Result<(), String> {
        authority.validate_for_dispatch(self, now_ms)
    }
    fn material_hash(&self) -> Result<String, String> {
        ProductionGitOpsPlan::material_hash(self)
    }
    fn commit_headline(&self, authority: &Self::Authority) -> String {
        ProductionGitOpsPlan::commit_headline(self, authority)
    }
    fn commit_body(&self, authority: &Self::Authority) -> Result<String, String> {
        ProductionGitOpsPlan::commit_body(self, authority)
    }
    fn commit_request(&self, authority: &Self::Authority, now_ms: i64) -> Result<Value, String> {
        ProductionGitOpsPlan::commit_request(self, authority, now_ms)
    }
}
