//! The first hosted workflow is deliberately tied to the Lucas Engineering
//! delivery path. Existing PipelineContract and DeploymentContract documents
//! remain the execution contracts; this snapshot binds them to one WorkItem.

use crate::{canonical_json_sha256, AgentProfile, RunBudget};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

mod source_merge;
pub use source_merge::{HostedSourceMergeAuthority, HOSTED_SOURCE_MERGE_SCHEMA};

pub const HOSTED_WORKFLOW_SCHEMA: &str = "pharness.dev/hosted-workflow/v1alpha1";
pub const LUCAS_DELIVERY_BINDING_SCHEMA: &str = "pharness.dev/lucas-delivery-binding/v1alpha1";
pub const HOSTED_REQUIRED_EVIDENCE: [&str; 11] = [
    "acceptance_criteria",
    "tested_source",
    "source_merge",
    "tekton_run",
    "image_digest",
    "staging_gitops_revision",
    "staging_runtime_verification",
    "human_production_approval",
    "production_gitops_revision",
    "production_deployment",
    "production_runtime_verification",
];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HostedWorkflowConfig {
    /// Readers ship first. Enabling this flag cuts over Product-scoped creation;
    /// missing or invalid bindings must never fall back to source-only work.
    pub enabled: bool,
    pub bindings: Vec<LucasDeliveryBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LucasDeliveryBinding {
    pub schema_version: String,
    pub id: String,
    pub revision: String,
    pub product_id: String,
    pub repository_id: String,
    pub source_repo: String,
    pub source_ref: String,
    pub cluster_context: String,
    pub pipeline_contract_id: String,
    pub image_name: String,
    pub gitops_repo: String,
    pub gitops_ref: String,
    pub staging: HostedDeploymentBinding,
    pub production: HostedDeploymentBinding,
    pub rollback_permitted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedDeploymentBinding {
    pub deployment_contract_id: String,
    pub kustomization_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedWorkflowPolicySnapshot {
    pub schema_version: String,
    pub delivery_binding: LucasDeliveryBinding,
    pub delivery_binding_hash: String,
    pub pipeline_contract: Value,
    pub staging_contract: Value,
    pub production_contract: Value,
    pub builder_budget: RunBudget,
    pub max_attempts: u32,
    pub agent_profiles: Vec<AgentProfile>,
    pub inference_registry_hash: String,
    pub stage_inference: Value,
    pub automatic_actions: Vec<HostedAutomaticAction>,
    pub production_approval: ProductionApprovalBoundary,
    pub rollback: HostedRollbackPermission,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostedAutomaticAction {
    Discover,
    Plan,
    Implement,
    Test,
    Verify,
    SourceDelivery,
    Build,
    StagingDelivery,
    Observe,
}

impl HostedAutomaticAction {
    pub fn authorized_sequence() -> Vec<Self> {
        vec![
            Self::Discover,
            Self::Plan,
            Self::Implement,
            Self::Test,
            Self::Verify,
            Self::SourceDelivery,
            Self::Build,
            Self::StagingDelivery,
            Self::Observe,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductionApprovalBoundary {
    BeforeGitopsMerge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostedRollbackPermission {
    Disabled,
    OnePreviousVerifiedDeployment,
}

impl HostedWorkflowConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.enabled && self.bindings.is_empty() {
            return Err("hosted creation requires at least one delivery binding".into());
        }
        let mut repositories = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for binding in &self.bindings {
            binding.validate()?;
            if !repositories.insert((&binding.product_id, &binding.repository_id))
                || !ids.insert(&binding.id)
            {
                return Err("hosted delivery bindings must have unique identities and Product/Repository pairs".into());
            }
        }
        Ok(())
    }
}

impl LucasDeliveryBinding {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != LUCAS_DELIVERY_BINDING_SCHEMA {
            return Err("unsupported Lucas delivery binding schema".into());
        }
        if self.cluster_context != "lucas_engineering" {
            return Err("hosted delivery is limited to lucas_engineering".into());
        }
        for field in [
            &self.id,
            &self.revision,
            &self.product_id,
            &self.repository_id,
            &self.pipeline_contract_id,
            &self.staging.deployment_contract_id,
            &self.production.deployment_contract_id,
        ] {
            if field.trim().is_empty() || field.len() > 200 || field.contains(char::is_whitespace) {
                return Err(
                    "delivery binding identities must be nonblank bounded identifiers".into(),
                );
            }
        }
        if self.source_ref != "main" || self.gitops_ref != "main" {
            return Err(
                "Lucas hosted delivery currently requires main for source and GitOps".into(),
            );
        }
        for repo in [&self.source_repo, &self.gitops_repo] {
            let url = url::Url::parse(repo).map_err(|_| "invalid delivery repository URL")?;
            let parts = url
                .path()
                .trim_start_matches('/')
                .split('/')
                .collect::<Vec<_>>();
            if url.scheme() != "https"
                || url.host_str() != Some("github.com")
                || !url.username().is_empty()
                || url.password().is_some()
                || url.port().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || parts.len() != 2
                || parts
                    .iter()
                    .any(|part| part.is_empty() || part.contains('%'))
                || !repo.ends_with(".git")
            {
                return Err(
                    "delivery repositories must be exact canonical GitHub HTTPS URLs".into(),
                );
            }
        }
        if self.source_repo == self.gitops_repo {
            return Err(
                "the application and separately authorized GitOps repository must be distinct"
                    .into(),
            );
        }
        if !self.image_name.starts_with("registry.lucas.engineering/")
            || self.image_name.ends_with('/')
            || self.image_name.contains(['@', ':', '*', '?'])
            || self.image_name.contains(char::is_whitespace)
            || !self
                .image_name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-/".contains(c))
            || self
                .image_name
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
        {
            return Err("image_name must name one untagged Lucas registry repository".into());
        }
        for target in [&self.staging, &self.production] {
            if !target.kustomization_path.ends_with("/kustomization.yaml")
                || target.kustomization_path.starts_with('/')
                || target
                    .kustomization_path
                    .split('/')
                    .any(|part| matches!(part, "" | "." | ".."))
                || target.kustomization_path.contains(['*', '?', '\\'])
            {
                return Err(
                    "delivery paths must name exact repository-relative Kustomizations".into(),
                );
            }
        }
        if self.staging.deployment_contract_id == self.production.deployment_contract_id
            || self.staging.kustomization_path == self.production.kustomization_path
        {
            return Err(
                "staging and production require distinct contracts and GitOps paths".into(),
            );
        }
        Ok(())
    }
}

impl HostedWorkflowPolicySnapshot {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != HOSTED_WORKFLOW_SCHEMA {
            return Err("unsupported hosted workflow schema; use a compatible reader".into());
        }
        self.delivery_binding.validate()?;
        let binding = serde_json::to_value(&self.delivery_binding).map_err(|e| e.to_string())?;
        if canonical_json_sha256(&binding).map_err(|e| e.to_string())? != self.delivery_binding_hash
        {
            return Err("hosted delivery binding hash does not match its snapshot".into());
        }
        self.builder_budget.validate().map_err(|e| e.to_string())?;
        if !(1..=2).contains(&self.max_attempts) {
            return Err("hosted work permits at most one bounded correction".into());
        }
        if self.automatic_actions != HostedAutomaticAction::authorized_sequence() {
            return Err("hosted automatic authority does not match the supported workflow".into());
        }
        if self.delivery_binding.rollback_permitted
            != (self.rollback == HostedRollbackPermission::OnePreviousVerifiedDeployment)
        {
            return Err("rollback permission must match the delivery binding".into());
        }
        for (snapshot, expected_id) in [
            (
                &self.pipeline_contract,
                &self.delivery_binding.pipeline_contract_id,
            ),
            (
                &self.staging_contract,
                &self.delivery_binding.staging.deployment_contract_id,
            ),
            (
                &self.production_contract,
                &self.delivery_binding.production.deployment_contract_id,
            ),
        ] {
            if snapshot.get("id").and_then(Value::as_str) != Some(expected_id.as_str())
                || snapshot.get("status").and_then(Value::as_str) != Some("active")
            {
                return Err(
                    "hosted work requires the exact active delivery contract snapshots".into(),
                );
            }
        }
        if self.pipeline_contract["namespace"] != "tekton-pipelines"
            || self.staging_contract["target_namespace"] != "apps-staging"
            || self.production_contract["target_namespace"] != "apps-prod"
            || self.staging_contract["target_environment"] != "staging"
            || self.production_contract["target_environment"] != "production"
            || self.staging_contract["argo_application"]
                == self.production_contract["argo_application"]
        {
            return Err(
                "delivery contract snapshots do not isolate Lucas staging and production".into(),
            );
        }
        let expected_profiles = [
            "repo-builder",
            "repo-planner",
            "repo-repair",
            "repo-test-diagnoser",
            "repo-verifier",
        ];
        let mut actual_profiles = self
            .agent_profiles
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>();
        actual_profiles.sort_unstable();
        if actual_profiles != expected_profiles {
            return Err(
                "hosted work requires all five pinned Coding Reliability V2 profiles".into(),
            );
        }
        for profile in &self.agent_profiles {
            profile.budget.validate().map_err(|e| e.to_string())?;
        }
        let ceiling = &self
            .agent_profiles
            .iter()
            .find(|p| p.id == "repo-builder")
            .unwrap()
            .budget;
        if self.builder_budget.initial_turns > ceiling.initial_turns
            || self.builder_budget.hard_turns > ceiling.hard_turns
            || self.builder_budget.initial_tokens > ceiling.initial_tokens
            || self.builder_budget.hard_tokens > ceiling.hard_tokens
            || self.builder_budget.active_execution_seconds > ceiling.active_execution_seconds
            || self.builder_budget.recoverable_tool_errors > ceiling.recoverable_tool_errors
            || self.builder_budget.identical_failures > ceiling.identical_failures
        {
            return Err("hosted Builder limits cannot exceed the pinned profile".into());
        }
        if !self.inference_registry_hash.starts_with("sha256:")
            || !["plan", "implement", "repair", "test_diagnosis", "verify"]
                .iter()
                .all(|stage| {
                    self.stage_inference
                        .get(stage)
                        .is_some_and(Value::is_object)
                })
            || self.stage_inference["test"]["mode"] != "deterministic"
        {
            return Err(
                "hosted work requires pinned gateway selections for all engineering stages".into(),
            );
        }
        Ok(())
    }
}
