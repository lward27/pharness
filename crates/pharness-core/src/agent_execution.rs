use crate::{canonical_json_sha256, InferenceStage, ReasoningEffort};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const AGENT_EXECUTION_POLICY_SCHEMA: &str = "pharness.dev/agent-execution-policy/v1alpha1";
pub const AGENT_EXECUTION_REGISTRY_SCHEMA: &str = "pharness.dev/agent-execution-registry/v1alpha1";
pub const RESOLVED_AGENT_EXECUTION_BINDING_SCHEMA: &str =
    "pharness.dev/resolved-agent-execution-binding/v1alpha1";
pub const AGENT_EXECUTION_EVALUATION_SCHEMA: &str =
    "pharness.dev/agent-execution-evaluation/v1alpha1";
pub const CODEX_PROTOCOL_EVALUATION_SCHEMA: &str =
    "pharness.dev/codex-protocol-evaluation/v1alpha1";
pub const CODEX_PROTOCOL_SUITE_ID: &str = "codex-app-server-protocol-v1";
pub const CODEX_PROTOCOL_CASES: [&str; 10] = [
    "planner_structured_submission",
    "builder_edit_and_structured_completion",
    "deterministic_command_execution",
    "repair_after_seeded_test_failure",
    "read_only_verification",
    "app_server_interruption_and_resume",
    "invalid_structured_output",
    "tool_command_network_denial",
    "authentication_path_read_denial",
    "subscription_quota_or_provider_error",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentExecutionQualificationContract {
    pub suite_id: String,
    pub suite_hash: String,
    pub semantic_attempts: u32,
    pub fixtures_per_attempt: usize,
    pub protocol_suite_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageExecutionDriver {
    PharnessRunhost,
    CodexAppServer,
}

impl StageExecutionDriver {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PharnessRunhost => "pharness_runhost",
            Self::CodexAppServer => "codex_app_server",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAuthenticationClass {
    ChatgptSession,
    ApiKey,
    WorkloadIdentity,
}

impl AgentAuthenticationClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ChatgptSession => "chatgpt_session",
            Self::ApiKey => "api_key",
            Self::WorkloadIdentity => "workload_identity",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentHostLifecycleState {
    Ready,
    Draining,
    Unavailable,
    Retired,
}

impl AgentHostLifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Draining => "draining",
            Self::Unavailable => "unavailable",
            Self::Retired => "retired",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentExecutionPolicyRef {
    pub policy_id: String,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSandboxPolicy {
    pub workspace_write: bool,
    pub command_network_access: bool,
    pub git_metadata_read_only: bool,
    pub credential_paths_deny_read: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentExecutionPolicyRevision {
    pub schema_version: String,
    pub policy_id: String,
    pub revision: String,
    pub display_name: String,
    pub driver: StageExecutionDriver,
    pub eligible_stages: Vec<InferenceStage>,
    pub runner_images: BTreeMap<String, String>,
    pub host_pool: String,
    pub codex_version: String,
    pub model: String,
    pub reasoning_effort: ReasoningEffort,
    pub prompt_revision: String,
    pub prompt_hash: String,
    pub output_schema_hash: String,
    pub sandbox: AgentSandboxPolicy,
    pub allowed_authentication: Vec<AgentAuthenticationClass>,
    pub active_time_seconds: u64,
    pub protocol_restart_limit: u32,
    #[serde(default)]
    pub selectable: bool,
    pub policy_hash: String,
}

impl AgentExecutionPolicyRevision {
    pub fn computed_hash(&self) -> Result<String, serde_json::Error> {
        let mut material = serde_json::to_value(self)?;
        material["policy_hash"] = serde_json::Value::String(String::new());
        canonical_json_sha256(&material)
    }

    pub fn validate(&self) -> Result<(), AgentExecutionConfigError> {
        if self.schema_version != AGENT_EXECUTION_POLICY_SCHEMA {
            return Err(AgentExecutionConfigError::UnsupportedSchema(
                self.schema_version.clone(),
            ));
        }
        validate_identifier("policy_id", &self.policy_id)?;
        validate_identifier("revision", &self.revision)?;
        validate_identifier("host_pool", &self.host_pool)?;
        validate_identifier("prompt_revision", &self.prompt_revision)?;
        if self.display_name.trim().is_empty()
            || self.codex_version.trim().is_empty()
            || self.model.trim().is_empty()
            || self.eligible_stages.is_empty()
            || self.runner_images.is_empty()
            || self.allowed_authentication.is_empty()
            || self.active_time_seconds == 0
            || self.protocol_restart_limit > 1
        {
            return Err(AgentExecutionConfigError::InvalidPolicy);
        }
        let mut stages = BTreeSet::new();
        if self
            .eligible_stages
            .iter()
            .any(|stage| !stages.insert(*stage))
        {
            return Err(AgentExecutionConfigError::DuplicateStage);
        }
        let mut authentication = BTreeSet::new();
        if self
            .allowed_authentication
            .iter()
            .any(|class| !authentication.insert(*class))
        {
            return Err(AgentExecutionConfigError::DuplicateAuthenticationClass);
        }
        for (profile, image) in &self.runner_images {
            validate_identifier("environment_profile", profile)?;
            if !is_digest_pinned_image(image) {
                return Err(AgentExecutionConfigError::MutableRunnerImage(image.clone()));
            }
        }
        for hash in [&self.prompt_hash, &self.output_schema_hash] {
            if !is_canonical_sha256(hash) {
                return Err(AgentExecutionConfigError::InvalidHash(hash.clone()));
            }
        }
        if self.driver == StageExecutionDriver::CodexAppServer
            && (self.sandbox.command_network_access
                || !self.sandbox.git_metadata_read_only
                || !self.sandbox.credential_paths_deny_read)
        {
            return Err(AgentExecutionConfigError::UnsafeCodexSandbox);
        }
        let expected = self
            .computed_hash()
            .map_err(AgentExecutionConfigError::Serialize)?;
        if expected != self.policy_hash {
            return Err(AgentExecutionConfigError::HashMismatch {
                expected,
                actual: self.policy_hash.clone(),
            });
        }
        Ok(())
    }

    pub fn supports(
        &self,
        stage: InferenceStage,
        environment_profile: &str,
        authentication: AgentAuthenticationClass,
    ) -> bool {
        self.selectable
            && self.eligible_stages.contains(&stage)
            && self.runner_images.contains_key(environment_profile)
            && self.allowed_authentication.contains(&authentication)
    }

    pub fn qualification_contract(
        &self,
    ) -> Result<AgentExecutionQualificationContract, AgentExecutionConfigError> {
        let stage = self
            .eligible_stages
            .first()
            .copied()
            .filter(|_| self.eligible_stages.len() == 1)
            .ok_or(AgentExecutionConfigError::QualificationStageUnavailable)?;
        let (suite_id, fixtures_per_attempt) = match stage {
            InferenceStage::Plan => ("planner-v2", 12),
            InferenceStage::Implement => ("coding-v2", 24),
            InferenceStage::Repair => ("repair-v2", 24),
            InferenceStage::Verify => ("verifier-v2", 24),
            _ => return Err(AgentExecutionConfigError::QualificationStageUnavailable),
        };
        let suite_hash = crate::inference_qualification_suite_hash(suite_id)
            .map_err(AgentExecutionConfigError::QualificationSuite)?;
        let protocol_suite_hash = canonical_json_sha256(&serde_json::json!({
            "schema_version":CODEX_PROTOCOL_EVALUATION_SCHEMA,
            "suite_id":CODEX_PROTOCOL_SUITE_ID,
            "fixture_revision":"codex-app-server-protocol-v1.0",
            "codex_version":self.codex_version,
            "policy_hash":self.policy_hash,
            "cases":CODEX_PROTOCOL_CASES,
            "attempts":3,
        }))
        .map_err(AgentExecutionConfigError::Serialize)?;
        Ok(AgentExecutionQualificationContract {
            suite_id: suite_id.into(),
            suite_hash,
            semantic_attempts: 2,
            fixtures_per_attempt,
            protocol_suite_hash,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentExecutionRegistry {
    pub schema_version: String,
    pub policies: Vec<AgentExecutionPolicyRevision>,
    pub defaults: BTreeMap<InferenceStage, AgentExecutionPolicyRef>,
    pub config_hash: String,
}

impl AgentExecutionRegistry {
    pub fn finalize_hashes(&mut self) -> Result<(), AgentExecutionConfigError> {
        for policy in &mut self.policies {
            let expected = policy
                .computed_hash()
                .map_err(AgentExecutionConfigError::Serialize)?;
            if policy.policy_hash.is_empty() {
                policy.policy_hash = expected;
            } else if policy.policy_hash != expected {
                return Err(AgentExecutionConfigError::HashMismatch {
                    expected,
                    actual: policy.policy_hash.clone(),
                });
            }
        }
        let expected = self
            .computed_hash()
            .map_err(AgentExecutionConfigError::Serialize)?;
        if self.config_hash.is_empty() {
            self.config_hash = expected;
        } else if self.config_hash != expected {
            return Err(AgentExecutionConfigError::HashMismatch {
                expected,
                actual: self.config_hash.clone(),
            });
        }
        self.validate()
    }

    pub fn computed_hash(&self) -> Result<String, serde_json::Error> {
        let mut material = serde_json::to_value(self)?;
        material["config_hash"] = serde_json::Value::String(String::new());
        canonical_json_sha256(&material)
    }

    pub fn validate(&self) -> Result<(), AgentExecutionConfigError> {
        if self.schema_version != AGENT_EXECUTION_REGISTRY_SCHEMA {
            return Err(AgentExecutionConfigError::UnsupportedSchema(
                self.schema_version.clone(),
            ));
        }
        let mut revisions = BTreeSet::new();
        type HostPoolContract<'a> = (
            &'a str,
            &'a BTreeMap<String, String>,
            BTreeSet<AgentAuthenticationClass>,
        );
        let mut host_pools: BTreeMap<&str, HostPoolContract<'_>> = BTreeMap::new();
        for policy in &self.policies {
            policy.validate()?;
            if !revisions.insert((policy.policy_id.clone(), policy.revision.clone())) {
                return Err(AgentExecutionConfigError::DuplicateRevision);
            }
            let authentication = policy
                .allowed_authentication
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            if let Some((codex_version, runner_images, allowed_authentication)) =
                host_pools.get(policy.host_pool.as_str())
            {
                if *codex_version != policy.codex_version
                    || *runner_images != &policy.runner_images
                    || allowed_authentication != &authentication
                {
                    return Err(AgentExecutionConfigError::InconsistentHostPool(
                        policy.host_pool.clone(),
                    ));
                }
            } else {
                host_pools.insert(
                    policy.host_pool.as_str(),
                    (
                        policy.codex_version.as_str(),
                        &policy.runner_images,
                        authentication,
                    ),
                );
            }
        }
        for (stage, reference) in &self.defaults {
            let policy = self
                .policy(&reference.policy_id, &reference.revision)
                .ok_or(AgentExecutionConfigError::DefaultPolicyUnavailable)?;
            if !policy.selectable || !policy.eligible_stages.contains(stage) {
                return Err(AgentExecutionConfigError::DefaultPolicyUnavailable);
            }
        }
        let expected = self
            .computed_hash()
            .map_err(AgentExecutionConfigError::Serialize)?;
        if expected != self.config_hash {
            return Err(AgentExecutionConfigError::HashMismatch {
                expected,
                actual: self.config_hash.clone(),
            });
        }
        Ok(())
    }

    pub fn policy(&self, policy_id: &str, revision: &str) -> Option<&AgentExecutionPolicyRevision> {
        self.policies
            .iter()
            .find(|policy| policy.policy_id == policy_id && policy.revision == revision)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedAgentExecutionBinding {
    pub schema_version: String,
    pub policy: AgentExecutionPolicyRevision,
    pub stage: InferenceStage,
    pub environment_profile_id: String,
    pub runner_image: String,
    pub authentication_class: AgentAuthenticationClass,
    pub host_pool: String,
    pub binding_hash: String,
}

impl ResolvedAgentExecutionBinding {
    pub fn computed_hash(&self) -> Result<String, serde_json::Error> {
        let mut material = serde_json::to_value(self)?;
        material["binding_hash"] = serde_json::Value::String(String::new());
        canonical_json_sha256(&material)
    }

    pub fn validate(&self) -> Result<(), AgentExecutionConfigError> {
        if self.schema_version != RESOLVED_AGENT_EXECUTION_BINDING_SCHEMA {
            return Err(AgentExecutionConfigError::UnsupportedSchema(
                self.schema_version.clone(),
            ));
        }
        self.policy.validate()?;
        if self.host_pool != self.policy.host_pool
            || self.policy.runner_images.get(&self.environment_profile_id)
                != Some(&self.runner_image)
            || !self.policy.supports(
                self.stage,
                &self.environment_profile_id,
                self.authentication_class,
            )
        {
            return Err(AgentExecutionConfigError::BindingMismatch);
        }
        let expected = self
            .computed_hash()
            .map_err(AgentExecutionConfigError::Serialize)?;
        if expected != self.binding_hash {
            return Err(AgentExecutionConfigError::HashMismatch {
                expected,
                actual: self.binding_hash.clone(),
            });
        }
        Ok(())
    }
}

fn validate_identifier(field: &'static str, value: &str) -> Result<(), AgentExecutionConfigError> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(AgentExecutionConfigError::InvalidIdentifier {
            field,
            value: value.to_string(),
        })
    }
}

fn is_digest_pinned_image(value: &str) -> bool {
    let Some((repository, digest)) = value.rsplit_once("@sha256:") else {
        return false;
    };
    !repository.is_empty()
        && digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_canonical_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hash| {
        hash.len() == 64
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

#[derive(Debug, Error)]
pub enum AgentExecutionConfigError {
    #[error("unsupported agent execution schema {0}")]
    UnsupportedSchema(String),
    #[error("invalid {field} identifier {value:?}")]
    InvalidIdentifier { field: &'static str, value: String },
    #[error("agent execution policy is incomplete")]
    InvalidPolicy,
    #[error("agent execution policy contains a duplicate stage")]
    DuplicateStage,
    #[error("agent execution policy contains a duplicate authentication class")]
    DuplicateAuthenticationClass,
    #[error("agent execution registry contains a duplicate policy revision")]
    DuplicateRevision,
    #[error("agent execution host pool {0} mixes incompatible Codex versions, runner images, or authentication classes")]
    InconsistentHostPool(String),
    #[error("agent execution default is unavailable")]
    DefaultPolicyUnavailable,
    #[error("runner image must be digest pinned: {0}")]
    MutableRunnerImage(String),
    #[error("invalid canonical sha256 hash {0}")]
    InvalidHash(String),
    #[error("Codex App Server policy does not enforce the required sandbox boundary")]
    UnsafeCodexSandbox,
    #[error("agent execution policy has no supported qualification stage")]
    QualificationStageUnavailable,
    #[error("agent execution qualification suite is unavailable: {0}")]
    QualificationSuite(String),
    #[error("resolved agent execution binding does not match its policy")]
    BindingMismatch,
    #[error("agent execution hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("agent execution configuration serialization failed: {0}")]
    Serialize(serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> AgentExecutionPolicyRevision {
        let mut policy = AgentExecutionPolicyRevision {
            schema_version: AGENT_EXECUTION_POLICY_SCHEMA.into(),
            policy_id: "codex-sol-builder-v1".into(),
            revision: "v1".into(),
            display_name: "Codex Sol Builder".into(),
            driver: StageExecutionDriver::CodexAppServer,
            eligible_stages: vec![InferenceStage::Implement],
            runner_images: BTreeMap::from([(
                "python-3.11".into(),
                format!(
                    "registry.example/pharness-python-runner@sha256:{}",
                    "a".repeat(64)
                ),
            )]),
            host_pool: "codex-linux-amd64".into(),
            codex_version: "0.151.0-alpha.7.2".into(),
            model: "gpt-5.6-sol".into(),
            reasoning_effort: ReasoningEffort::High,
            prompt_revision: "repo-builder-v2".into(),
            prompt_hash: format!("sha256:{}", "b".repeat(64)),
            output_schema_hash: format!("sha256:{}", "c".repeat(64)),
            sandbox: AgentSandboxPolicy {
                workspace_write: true,
                command_network_access: false,
                git_metadata_read_only: true,
                credential_paths_deny_read: true,
            },
            allowed_authentication: vec![AgentAuthenticationClass::ChatgptSession],
            active_time_seconds: 3_600,
            protocol_restart_limit: 1,
            selectable: true,
            policy_hash: String::new(),
        };
        policy.policy_hash = policy.computed_hash().unwrap();
        policy
    }

    #[test]
    fn policy_and_binding_hashes_are_deterministic() {
        let policy = policy();
        policy.validate().unwrap();
        let mut binding = ResolvedAgentExecutionBinding {
            schema_version: RESOLVED_AGENT_EXECUTION_BINDING_SCHEMA.into(),
            runner_image: policy.runner_images["python-3.11"].clone(),
            host_pool: policy.host_pool.clone(),
            policy,
            stage: InferenceStage::Implement,
            environment_profile_id: "python-3.11".into(),
            authentication_class: AgentAuthenticationClass::ChatgptSession,
            binding_hash: String::new(),
        };
        binding.binding_hash = binding.computed_hash().unwrap();
        binding.validate().unwrap();
    }

    #[test]
    fn mutable_runner_or_unsafe_sandbox_is_rejected() {
        let mut mutable = policy();
        mutable.runner_images.insert(
            "python-3.11".into(),
            "registry.example/runner:latest".into(),
        );
        mutable.policy_hash = mutable.computed_hash().unwrap();
        assert!(matches!(
            mutable.validate(),
            Err(AgentExecutionConfigError::MutableRunnerImage(_))
        ));

        let mut unsafe_policy = policy();
        unsafe_policy.sandbox.command_network_access = true;
        unsafe_policy.policy_hash = unsafe_policy.computed_hash().unwrap();
        assert!(matches!(
            unsafe_policy.validate(),
            Err(AgentExecutionConfigError::UnsafeCodexSandbox)
        ));
    }

    #[test]
    fn qualification_contract_is_bound_to_exact_policy_and_stage() {
        let builder = policy();
        let builder_contract = builder.qualification_contract().unwrap();
        assert_eq!(builder_contract.suite_id, "coding-v2");
        assert_eq!(builder_contract.semantic_attempts, 2);
        assert_eq!(builder_contract.fixtures_per_attempt, 24);

        let mut verifier = builder.clone();
        verifier.policy_id = "codex-sol-verifier-v1".into();
        verifier.eligible_stages = vec![InferenceStage::Verify];
        verifier.policy_hash = verifier.computed_hash().unwrap();
        let verifier_contract = verifier.qualification_contract().unwrap();
        assert_eq!(verifier_contract.suite_id, "verifier-v2");
        assert_ne!(builder_contract.suite_hash, verifier_contract.suite_hash);
        assert_ne!(
            builder_contract.protocol_suite_hash,
            verifier_contract.protocol_suite_hash
        );

        let mut unsupported = builder;
        unsupported.eligible_stages = vec![InferenceStage::Test];
        unsupported.policy_hash = unsupported.computed_hash().unwrap();
        assert!(matches!(
            unsupported.qualification_contract(),
            Err(AgentExecutionConfigError::QualificationStageUnavailable)
        ));
    }

    #[test]
    fn one_host_pool_cannot_mix_incompatible_runtime_requirements() {
        let first = policy();
        let mut second = policy();
        second.policy_id = "codex-sol-verifier-v1".into();
        second.eligible_stages = vec![InferenceStage::Verify];
        second.codex_version = "codex-cli 999.0.0".into();
        second.policy_hash = second.computed_hash().unwrap();
        let mut registry = AgentExecutionRegistry {
            schema_version: AGENT_EXECUTION_REGISTRY_SCHEMA.into(),
            policies: vec![first, second],
            defaults: BTreeMap::new(),
            config_hash: String::new(),
        };
        registry.config_hash = registry.computed_hash().unwrap();
        assert!(matches!(
            registry.validate(),
            Err(AgentExecutionConfigError::InconsistentHostPool(_))
        ));
    }
}
