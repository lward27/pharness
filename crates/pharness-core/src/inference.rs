use crate::{canonical_json_sha256, ToolProtocolMode};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use thiserror::Error;
use url::Url;

pub const INFERENCE_TARGET_SCHEMA: &str = "pharness.dev/inference-target/v1alpha1";
pub const INFERENCE_POLICY_SCHEMA: &str = "pharness.dev/stage-inference-policy/v1alpha1";
pub const RESOLVED_INFERENCE_BINDING_SCHEMA: &str =
    "pharness.dev/resolved-inference-binding/v1alpha1";
pub const INFERENCE_QUALIFICATION_SUITE_SCHEMA: &str =
    "pharness.dev/inference-qualification-suite/v1alpha1";

pub fn inference_qualification_suite_hash(suite_id: &str) -> Result<String, String> {
    let fixture_revision = match suite_id {
        "onboarding-v1" | "planner-v1" | "tester-v1" | "verifier-v1" => "stage-qualification-v1.0",
        "coding-v1" => "coding-v1.7",
        _ => {
            return Err(format!(
                "unsupported inference qualification suite {suite_id:?}"
            ))
        }
    };
    canonical_json_sha256(&serde_json::json!({
        "schema_version":INFERENCE_QUALIFICATION_SUITE_SCHEMA,
        "suite_id":suite_id,
        "fixture_revision":fixture_revision,
    }))
    .map_err(|error| error.to_string())
}
pub const MODEL_GRANT_SCHEMA: &str = "pharness.dev/model-grant/v1alpha1";
pub const INFERENCE_REGISTRY_SCHEMA: &str = "pharness.dev/inference-registry/v1alpha1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceBackendKind {
    Fireworks,
    Openrouter,
    LmStudio,
    LlamaCpp,
    OpenaiCompatible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceStage {
    Onboarding,
    Plan,
    Implement,
    Test,
    Verify,
}

impl InferenceStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Onboarding => "onboarding",
            Self::Plan => "plan",
            Self::Implement => "implement",
            Self::Test => "test",
            Self::Verify => "verify",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl ReasoningEffort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningContextMode {
    ProviderDefault,
    Disabled,
    CurrentTurn,
    AllTurns,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasoningRequestPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<ReasoningEffort>,
    pub context_mode: ReasoningContextMode,
    #[serde(default)]
    pub expose_replay: bool,
}

impl Default for ReasoningRequestPolicy {
    fn default() -> Self {
        Self {
            effort: None,
            context_mode: ReasoningContextMode::ProviderDefault,
            expose_replay: true,
        }
    }
}

/// Opaque provider reasoning state required to continue a tool-calling turn.
/// It is operational transcript material, never controller evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ReasoningReplay {
    Text(String),
    Structured(serde_json::Value),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceCapabilities {
    pub native_tools: bool,
    pub streaming: bool,
    pub json_schema: bool,
    #[serde(default)]
    pub stream_options: bool,
    #[serde(default)]
    pub reasoning_efforts: Vec<ReasoningEffort>,
    #[serde(default)]
    pub reasoning_context_modes: Vec<ReasoningContextMode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceTransportPolicy {
    #[serde(default = "default_connect_timeout_seconds")]
    pub connect_timeout_seconds: u64,
    #[serde(default = "default_first_response_timeout_seconds")]
    pub first_response_timeout_seconds: u64,
    #[serde(default = "default_stream_idle_timeout_seconds")]
    pub stream_idle_timeout_seconds: u64,
    #[serde(default = "default_transport_attempts")]
    pub max_attempts: u32,
    #[serde(default)]
    pub allow_insecure_private_http: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_cidr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_port: Option<u16>,
}

const fn default_connect_timeout_seconds() -> u64 {
    5
}

const fn default_first_response_timeout_seconds() -> u64 {
    300
}

const fn default_stream_idle_timeout_seconds() -> u64 {
    120
}

const fn default_transport_attempts() -> u32 {
    3
}

impl Default for InferenceTransportPolicy {
    fn default() -> Self {
        Self {
            connect_timeout_seconds: default_connect_timeout_seconds(),
            first_response_timeout_seconds: default_first_response_timeout_seconds(),
            stream_idle_timeout_seconds: default_stream_idle_timeout_seconds(),
            max_attempts: default_transport_attempts(),
            allow_insecure_private_http: false,
            private_cidr: None,
            private_port: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenRouterRoutePolicy {
    pub provider_slug: String,
    #[serde(default = "default_true")]
    pub require_parameters: bool,
    #[serde(default)]
    pub allow_fallbacks: bool,
    #[serde(default = "default_deny")]
    pub data_collection: String,
}

fn default_true() -> bool {
    true
}

fn default_deny() -> String {
    "deny".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceTargetRevision {
    pub schema_version: String,
    pub target_id: String,
    pub revision: String,
    pub display_name: String,
    pub backend_kind: InferenceBackendKind,
    pub protocol: String,
    pub upstream_base_url: String,
    pub upstream_model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authentication_binding: Option<String>,
    pub transport: InferenceTransportPolicy,
    pub capabilities: InferenceCapabilities,
    pub context_limit_tokens: u32,
    pub output_limit_tokens: u32,
    pub allowed_stages: Vec<InferenceStage>,
    #[serde(default)]
    pub selectable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openrouter: Option<OpenRouterRoutePolicy>,
    pub config_hash: String,
}

impl InferenceTargetRevision {
    pub fn computed_hash(&self) -> Result<String, serde_json::Error> {
        let mut material = serde_json::to_value(self)?;
        material["config_hash"] = serde_json::Value::String(String::new());
        canonical_json_sha256(&material)
    }

    pub fn validate(&self) -> Result<(), InferenceConfigError> {
        if self.schema_version != INFERENCE_TARGET_SCHEMA {
            return Err(InferenceConfigError::UnsupportedSchema(
                self.schema_version.clone(),
            ));
        }
        validate_identifier("target_id", &self.target_id)?;
        validate_identifier("revision", &self.revision)?;
        if self.protocol != "openai_chat_completions_v1" {
            return Err(InferenceConfigError::UnsupportedProtocol(
                self.protocol.clone(),
            ));
        }
        if self.upstream_model.trim().is_empty() {
            return Err(InferenceConfigError::MissingModel);
        }
        if self.allowed_stages.is_empty() {
            return Err(InferenceConfigError::NoAllowedStages);
        }
        if self.output_limit_tokens == 0
            || self.context_limit_tokens == 0
            || self.output_limit_tokens >= self.context_limit_tokens
        {
            return Err(InferenceConfigError::InvalidTokenLimits);
        }
        if self.transport.max_attempts == 0 || self.transport.max_attempts > 3 {
            return Err(InferenceConfigError::InvalidTransportAttempts);
        }
        validate_target_url(&self.upstream_base_url, &self.transport)?;
        match (&self.backend_kind, &self.openrouter) {
            (InferenceBackendKind::Openrouter, Some(route))
                if route.provider_slug.trim().is_empty()
                    || route.allow_fallbacks
                    || !route.require_parameters
                    || route.data_collection != "deny" =>
            {
                return Err(InferenceConfigError::UnsafeOpenRouterRoute);
            }
            (InferenceBackendKind::Openrouter, Some(_)) => {}
            (InferenceBackendKind::Openrouter, None) => {
                return Err(InferenceConfigError::MissingOpenRouterRoute)
            }
            (_, Some(_)) => return Err(InferenceConfigError::UnexpectedOpenRouterRoute),
            _ => {}
        }
        let expected = self
            .computed_hash()
            .map_err(InferenceConfigError::Serialize)?;
        if expected != self.config_hash {
            return Err(InferenceConfigError::HashMismatch {
                expected,
                actual: self.config_hash.clone(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceTargetRef {
    pub target_id: String,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferencePolicyRef {
    pub policy_id: String,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageInferencePolicyRevision {
    pub schema_version: String,
    pub policy_id: String,
    pub revision: String,
    pub display_name: String,
    pub eligible_profiles: Vec<String>,
    pub eligible_stages: Vec<InferenceStage>,
    pub target: InferenceTargetRef,
    pub target_hash: String,
    pub reasoning: ReasoningRequestPolicy,
    /// Exact thousandths, so hashes remain deterministic and Eq-safe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature_milli: Option<u16>,
    pub max_output_tokens: u32,
    pub max_input_tokens: u32,
    pub tool_protocol: ToolProtocolMode,
    pub transport_max_attempts: u32,
    #[serde(default)]
    pub selectable: bool,
    pub policy_hash: String,
}

impl StageInferencePolicyRevision {
    pub fn temperature(&self) -> Option<f32> {
        self.temperature_milli
            .map(|value| f32::from(value) / 1000.0)
    }

    pub fn computed_hash(&self) -> Result<String, serde_json::Error> {
        let mut material = serde_json::to_value(self)?;
        material["policy_hash"] = serde_json::Value::String(String::new());
        canonical_json_sha256(&material)
    }

    pub fn validate_for_target(
        &self,
        target: &InferenceTargetRevision,
    ) -> Result<(), InferenceConfigError> {
        if self.schema_version != INFERENCE_POLICY_SCHEMA {
            return Err(InferenceConfigError::UnsupportedSchema(
                self.schema_version.clone(),
            ));
        }
        validate_identifier("policy_id", &self.policy_id)?;
        validate_identifier("revision", &self.revision)?;
        if self.target.target_id != target.target_id
            || self.target.revision != target.revision
            || self.target_hash != target.config_hash
        {
            return Err(InferenceConfigError::TargetBindingMismatch);
        }
        if self.eligible_profiles.is_empty()
            || self
                .eligible_profiles
                .iter()
                .any(|profile| profile.is_empty())
            || self.eligible_stages.is_empty()
            || self
                .eligible_stages
                .iter()
                .any(|stage| !target.allowed_stages.contains(stage))
        {
            return Err(InferenceConfigError::StageNotAllowed);
        }
        if self.max_output_tokens == 0
            || self.max_output_tokens > target.output_limit_tokens
            || self.max_input_tokens == 0
            || self.max_input_tokens > target.context_limit_tokens
        {
            return Err(InferenceConfigError::InvalidTokenLimits);
        }
        if self.transport_max_attempts == 0
            || self.transport_max_attempts > target.transport.max_attempts
        {
            return Err(InferenceConfigError::InvalidTransportAttempts);
        }
        if let Some(effort) = self.reasoning.effort {
            if !target.capabilities.reasoning_efforts.contains(&effort) {
                return Err(InferenceConfigError::UnsupportedReasoningEffort);
            }
        }
        if self.reasoning.context_mode != ReasoningContextMode::ProviderDefault
            && !target
                .capabilities
                .reasoning_context_modes
                .contains(&self.reasoning.context_mode)
        {
            return Err(InferenceConfigError::UnsupportedReasoningContext);
        }
        if self.tool_protocol == ToolProtocolMode::NativeTools && !target.capabilities.native_tools
        {
            return Err(InferenceConfigError::NativeToolsUnavailable);
        }
        let expected = self
            .computed_hash()
            .map_err(InferenceConfigError::Serialize)?;
        if expected != self.policy_hash {
            return Err(InferenceConfigError::HashMismatch {
                expected,
                actual: self.policy_hash.clone(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceRegistry {
    pub schema_version: String,
    pub targets: Vec<InferenceTargetRevision>,
    pub policies: Vec<StageInferencePolicyRevision>,
    pub defaults: BTreeMap<InferenceStage, InferencePolicyRef>,
    pub config_hash: String,
}

impl InferenceRegistry {
    /// Complete hashes omitted by a GitOps-authored registry while rejecting
    /// every supplied but incorrect hash. Revision identifiers still remain
    /// operator-owned and immutable; this only makes the canonical content
    /// hashes deterministic across the API and gateway.
    pub fn finalize_hashes(&mut self) -> Result<(), InferenceConfigError> {
        for target in &mut self.targets {
            let expected = target
                .computed_hash()
                .map_err(InferenceConfigError::Serialize)?;
            if target.config_hash.is_empty() {
                target.config_hash = expected;
            } else if target.config_hash != expected {
                return Err(InferenceConfigError::HashMismatch {
                    expected,
                    actual: target.config_hash.clone(),
                });
            }
        }
        for policy in &mut self.policies {
            let target = self
                .targets
                .iter()
                .find(|target| {
                    target.target_id == policy.target.target_id
                        && target.revision == policy.target.revision
                })
                .ok_or(InferenceConfigError::TargetBindingMismatch)?;
            if policy.target_hash.is_empty() {
                policy.target_hash = target.config_hash.clone();
            } else if policy.target_hash != target.config_hash {
                return Err(InferenceConfigError::TargetBindingMismatch);
            }
            let expected = policy
                .computed_hash()
                .map_err(InferenceConfigError::Serialize)?;
            if policy.policy_hash.is_empty() {
                policy.policy_hash = expected;
            } else if policy.policy_hash != expected {
                return Err(InferenceConfigError::HashMismatch {
                    expected,
                    actual: policy.policy_hash.clone(),
                });
            }
        }
        let expected = self
            .computed_hash()
            .map_err(InferenceConfigError::Serialize)?;
        if self.config_hash.is_empty() {
            self.config_hash = expected;
        } else if self.config_hash != expected {
            return Err(InferenceConfigError::HashMismatch {
                expected,
                actual: self.config_hash.clone(),
            });
        }
        self.validate()
    }

    pub fn computed_hash(&self) -> Result<String, serde_json::Error> {
        let mut value = serde_json::to_value(self)?;
        value["config_hash"] = serde_json::Value::String(String::new());
        canonical_json_sha256(&value)
    }

    pub fn validate(&self) -> Result<(), InferenceConfigError> {
        if self.schema_version != INFERENCE_REGISTRY_SCHEMA {
            return Err(InferenceConfigError::UnsupportedSchema(
                self.schema_version.clone(),
            ));
        }
        let mut target_keys = BTreeSet::new();
        for target in &self.targets {
            target.validate()?;
            if !target_keys.insert((target.target_id.clone(), target.revision.clone())) {
                return Err(InferenceConfigError::DuplicateRevision);
            }
        }
        let mut policy_keys = BTreeSet::new();
        for policy in &self.policies {
            let target = self
                .targets
                .iter()
                .find(|target| {
                    target.target_id == policy.target.target_id
                        && target.revision == policy.target.revision
                })
                .ok_or(InferenceConfigError::TargetBindingMismatch)?;
            policy.validate_for_target(target)?;
            if !policy_keys.insert((policy.policy_id.clone(), policy.revision.clone())) {
                return Err(InferenceConfigError::DuplicateRevision);
            }
        }
        for (stage, policy_ref) in &self.defaults {
            let policy = self
                .policies
                .iter()
                .find(|policy| {
                    policy.policy_id == policy_ref.policy_id
                        && policy.revision == policy_ref.revision
                })
                .ok_or(InferenceConfigError::DefaultPolicyUnavailable)?;
            if !policy.selectable || !policy.eligible_stages.contains(stage) {
                return Err(InferenceConfigError::DefaultPolicyUnavailable);
            }
        }
        let expected = self
            .computed_hash()
            .map_err(InferenceConfigError::Serialize)?;
        if expected != self.config_hash {
            return Err(InferenceConfigError::HashMismatch {
                expected,
                actual: self.config_hash.clone(),
            });
        }
        Ok(())
    }

    pub fn target(&self, target_id: &str, revision: &str) -> Option<&InferenceTargetRevision> {
        self.targets
            .iter()
            .find(|target| target.target_id == target_id && target.revision == revision)
    }

    pub fn policy(&self, policy_id: &str, revision: &str) -> Option<&StageInferencePolicyRevision> {
        self.policies
            .iter()
            .find(|policy| policy.policy_id == policy_id && policy.revision == revision)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedInferenceBinding {
    pub schema_version: String,
    pub target: InferenceTargetRevision,
    pub policy: StageInferencePolicyRevision,
    pub prompt_version: String,
    pub tool_schema_hash: String,
    pub profile_budget_hash: String,
    pub base_agent_profile_hash: String,
    pub agent_profile_hash: String,
    pub binding_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelGrantClaims {
    pub schema_version: String,
    pub run_id: String,
    pub stage_execution_id: String,
    pub selection_id: String,
    pub target: InferenceTargetRef,
    pub target_hash: String,
    pub policy: InferencePolicyRef,
    pub policy_hash: String,
    pub request_sequence: u32,
    pub request_body_hash: String,
    pub nonce: String,
    pub issued_at_epoch_seconds: u64,
    pub expires_at_epoch_seconds: u64,
}

impl ModelGrantClaims {
    pub fn target_alias(&self) -> String {
        format!("{}@{}", self.target.target_id, self.target.revision)
    }

    pub fn validate_at(&self, now_epoch_seconds: u64) -> Result<(), ModelGrantError> {
        if self.schema_version != MODEL_GRANT_SCHEMA {
            return Err(ModelGrantError::UnsupportedSchema(
                self.schema_version.clone(),
            ));
        }
        if self.run_id.is_empty()
            || self.stage_execution_id.is_empty()
            || self.selection_id.is_empty()
            || self.target_hash.len() != 64
            || self.policy_hash.len() != 64
            || self.request_body_hash.len() != 64
            || self.nonce.len() < 16
        {
            return Err(ModelGrantError::InvalidClaims);
        }
        if self.expires_at_epoch_seconds <= self.issued_at_epoch_seconds
            || self
                .expires_at_epoch_seconds
                .saturating_sub(self.issued_at_epoch_seconds)
                > 60
        {
            return Err(ModelGrantError::InvalidLifetime);
        }
        if now_epoch_seconds < self.issued_at_epoch_seconds.saturating_sub(5)
            || now_epoch_seconds >= self.expires_at_epoch_seconds
        {
            return Err(ModelGrantError::Expired);
        }
        Ok(())
    }
}

pub fn sign_model_grant(claims: &ModelGrantClaims, key: &[u8]) -> Result<String, ModelGrantError> {
    if key.len() < 32 {
        return Err(ModelGrantError::WeakSigningKey);
    }
    let payload = serde_json::to_vec(claims).map_err(ModelGrantError::Serialize)?;
    let encoded_payload = URL_SAFE_NO_PAD.encode(payload);
    let mut mac =
        Hmac::<Sha256>::new_from_slice(key).map_err(|_| ModelGrantError::WeakSigningKey)?;
    mac.update(encoded_payload.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    Ok(format!("{encoded_payload}.{signature}"))
}

pub fn verify_model_grant(
    token: &str,
    key: &[u8],
    now_epoch_seconds: u64,
) -> Result<ModelGrantClaims, ModelGrantError> {
    if key.len() < 32 {
        return Err(ModelGrantError::WeakSigningKey);
    }
    let (encoded_payload, encoded_signature) = token
        .split_once('.')
        .ok_or(ModelGrantError::MalformedToken)?;
    if encoded_payload.contains('.') || encoded_signature.contains('.') {
        return Err(ModelGrantError::MalformedToken);
    }
    let signature = URL_SAFE_NO_PAD
        .decode(encoded_signature)
        .map_err(|_| ModelGrantError::MalformedToken)?;
    let mut mac =
        Hmac::<Sha256>::new_from_slice(key).map_err(|_| ModelGrantError::WeakSigningKey)?;
    mac.update(encoded_payload.as_bytes());
    mac.verify_slice(&signature)
        .map_err(|_| ModelGrantError::InvalidSignature)?;
    let payload = URL_SAFE_NO_PAD
        .decode(encoded_payload)
        .map_err(|_| ModelGrantError::MalformedToken)?;
    let claims: ModelGrantClaims =
        serde_json::from_slice(&payload).map_err(ModelGrantError::Serialize)?;
    claims.validate_at(now_epoch_seconds)?;
    Ok(claims)
}

#[derive(Debug, Error)]
pub enum ModelGrantError {
    #[error("unsupported model-grant schema {0}")]
    UnsupportedSchema(String),
    #[error("model-grant claims are invalid")]
    InvalidClaims,
    #[error("model-grant lifetime must be between one and 60 seconds")]
    InvalidLifetime,
    #[error("model grant is expired or not yet valid")]
    Expired,
    #[error("model-grant signing key must contain at least 32 bytes")]
    WeakSigningKey,
    #[error("model-grant token is malformed")]
    MalformedToken,
    #[error("model-grant signature is invalid")]
    InvalidSignature,
    #[error("model-grant serialization failed: {0}")]
    Serialize(serde_json::Error),
}

impl ResolvedInferenceBinding {
    pub fn computed_agent_profile_hash(&self) -> Result<String, serde_json::Error> {
        canonical_json_sha256(&serde_json::json!({
            "base_agent_profile_hash":self.base_agent_profile_hash,
            "target_hash":self.target.config_hash,
            "policy_hash":self.policy.policy_hash,
            "prompt_version":self.prompt_version,
            "tool_schema_hash":self.tool_schema_hash,
            "profile_budget_hash":self.profile_budget_hash,
        }))
        .map(|hash| format!("sha256:{hash}"))
    }

    pub fn computed_hash(&self) -> Result<String, serde_json::Error> {
        let mut material = serde_json::to_value(self)?;
        material["binding_hash"] = serde_json::Value::String(String::new());
        canonical_json_sha256(&material)
    }

    pub fn validate(&self) -> Result<(), InferenceConfigError> {
        if self.schema_version != RESOLVED_INFERENCE_BINDING_SCHEMA {
            return Err(InferenceConfigError::UnsupportedSchema(
                self.schema_version.clone(),
            ));
        }
        self.target.validate()?;
        self.policy.validate_for_target(&self.target)?;
        if self.base_agent_profile_hash.len() != 71
            || !self.base_agent_profile_hash.starts_with("sha256:")
            || self.agent_profile_hash
                != self
                    .computed_agent_profile_hash()
                    .map_err(InferenceConfigError::Serialize)?
        {
            return Err(InferenceConfigError::AgentProfileHashMismatch);
        }
        let expected = self
            .computed_hash()
            .map_err(InferenceConfigError::Serialize)?;
        if expected != self.binding_hash {
            return Err(InferenceConfigError::HashMismatch {
                expected,
                actual: self.binding_hash.clone(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum InferenceConfigError {
    #[error("unsupported inference schema {0}")]
    UnsupportedSchema(String),
    #[error("unsupported inference protocol {0}")]
    UnsupportedProtocol(String),
    #[error("{field} is not a valid immutable identifier: {value}")]
    InvalidIdentifier { field: &'static str, value: String },
    #[error("inference target model is missing")]
    MissingModel,
    #[error("inference target has no allowed stages")]
    NoAllowedStages,
    #[error("inference token limits are invalid")]
    InvalidTokenLimits,
    #[error("inference transport attempts must be between one and three")]
    InvalidTransportAttempts,
    #[error("inference target URL is invalid: {0}")]
    InvalidUrl(String),
    #[error("inference target URL contains credentials, query parameters, or a fragment")]
    UnsafeUrl,
    #[error("plain HTTP inference requires an exact private IP and CIDR opt-in")]
    InsecureHttpNotAllowed,
    #[error("loopback, link-local, unspecified, multicast, and metadata targets are forbidden")]
    ForbiddenAddress,
    #[error("OpenRouter target is missing its pinned provider route")]
    MissingOpenRouterRoute,
    #[error("OpenRouter routing must pin one provider, require parameters, deny collection, and disable fallback")]
    UnsafeOpenRouterRoute,
    #[error("OpenRouter routing is valid only for OpenRouter targets")]
    UnexpectedOpenRouterRoute,
    #[error("inference policy target revision/hash does not match")]
    TargetBindingMismatch,
    #[error("inference policy stage is not allowed by the target")]
    StageNotAllowed,
    #[error("inference target does not support the requested reasoning effort")]
    UnsupportedReasoningEffort,
    #[error("inference target does not support the requested reasoning context mode")]
    UnsupportedReasoningContext,
    #[error("native tools are unavailable for this inference target")]
    NativeToolsUnavailable,
    #[error("resolved inference binding does not match its AgentProfile hash")]
    AgentProfileHashMismatch,
    #[error("inference registry contains a duplicate immutable revision")]
    DuplicateRevision,
    #[error("inference registry default policy is missing, ineligible, or unselectable")]
    DefaultPolicyUnavailable,
    #[error("inference configuration hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("inference configuration serialization failed: {0}")]
    Serialize(serde_json::Error),
}

fn validate_identifier(field: &'static str, value: &str) -> Result<(), InferenceConfigError> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_.".contains(&byte)
        });
    if valid {
        Ok(())
    } else {
        Err(InferenceConfigError::InvalidIdentifier {
            field,
            value: value.to_string(),
        })
    }
}

fn validate_target_url(
    input: &str,
    transport: &InferenceTransportPolicy,
) -> Result<(), InferenceConfigError> {
    let url =
        Url::parse(input).map_err(|error| InferenceConfigError::InvalidUrl(error.to_string()))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(InferenceConfigError::UnsafeUrl);
    }
    let host = url
        .host_str()
        .ok_or_else(|| InferenceConfigError::InvalidUrl("host is missing".into()))?;
    if host.eq_ignore_ascii_case("localhost")
        || host.eq_ignore_ascii_case("metadata.google.internal")
        || host == "169.254.169.254"
    {
        return Err(InferenceConfigError::ForbiddenAddress);
    }
    match url.scheme() {
        "https" => {
            if transport.allow_insecure_private_http
                || transport.private_cidr.is_some()
                || transport.private_port.is_some()
            {
                return Err(InferenceConfigError::InsecureHttpNotAllowed);
            }
            if let Ok(ip) = host.parse::<IpAddr>() {
                reject_forbidden_ip(ip)?;
            }
            Ok(())
        }
        "http" => {
            let ip = host
                .parse::<IpAddr>()
                .map_err(|_| InferenceConfigError::InsecureHttpNotAllowed)?;
            reject_forbidden_ip(ip)?;
            let expected_cidr = exact_cidr(ip);
            if !is_private_ip(ip)
                || !transport.allow_insecure_private_http
                || transport.private_cidr.as_deref() != Some(expected_cidr.as_str())
                || transport.private_port != url.port_or_known_default()
            {
                return Err(InferenceConfigError::InsecureHttpNotAllowed);
            }
            Ok(())
        }
        _ => Err(InferenceConfigError::InvalidUrl(
            "scheme must be https or explicitly approved private http".into(),
        )),
    }
}

fn reject_forbidden_ip(ip: IpAddr) -> Result<(), InferenceConfigError> {
    let forbidden = match ip {
        IpAddr::V4(value) => {
            value.is_loopback()
                || value.is_link_local()
                || value.is_unspecified()
                || value.is_multicast()
                || value.octets() == [169, 254, 169, 254]
        }
        IpAddr::V6(value) => {
            value.is_loopback()
                || value.is_unspecified()
                || value.is_multicast()
                || (value.segments()[0] & 0xffc0) == 0xfe80
        }
    };
    if forbidden {
        Err(InferenceConfigError::ForbiddenAddress)
    } else {
        Ok(())
    }
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(value) => value.is_private(),
        IpAddr::V6(value) => (value.segments()[0] & 0xfe00) == 0xfc00,
    }
}

fn exact_cidr(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(value) => format!("{value}/32"),
        IpAddr::V6(value) => format!("{value}/128"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(url: &str, transport: InferenceTransportPolicy) -> InferenceTargetRevision {
        let mut target = InferenceTargetRevision {
            schema_version: INFERENCE_TARGET_SCHEMA.into(),
            target_id: "local-qwen".into(),
            revision: "v1".into(),
            display_name: "Local Qwen".into(),
            backend_kind: InferenceBackendKind::LmStudio,
            protocol: "openai_chat_completions_v1".into(),
            upstream_base_url: url.into(),
            upstream_model: "qwen".into(),
            authentication_binding: Some("lm-studio-token".into()),
            transport,
            capabilities: InferenceCapabilities {
                native_tools: true,
                streaming: true,
                json_schema: true,
                stream_options: false,
                reasoning_efforts: vec![ReasoningEffort::Low],
                reasoning_context_modes: vec![ReasoningContextMode::CurrentTurn],
            },
            context_limit_tokens: 32_768,
            output_limit_tokens: 4_096,
            allowed_stages: vec![InferenceStage::Test],
            selectable: false,
            openrouter: None,
            config_hash: String::new(),
        };
        target.config_hash = target.computed_hash().unwrap();
        target
    }

    #[test]
    fn accepts_exact_private_http_opt_in() {
        let transport = InferenceTransportPolicy {
            allow_insecure_private_http: true,
            private_cidr: Some("192.168.1.40/32".into()),
            private_port: Some(1234),
            ..InferenceTransportPolicy::default()
        };
        target("http://192.168.1.40:1234/v1", transport)
            .validate()
            .unwrap();
    }

    #[test]
    fn rejects_hostname_for_insecure_http() {
        let transport = InferenceTransportPolicy {
            allow_insecure_private_http: true,
            private_cidr: Some("192.168.1.40/32".into()),
            private_port: Some(1234),
            ..InferenceTransportPolicy::default()
        };
        assert!(matches!(
            target("http://model-host.local:1234/v1", transport).validate(),
            Err(InferenceConfigError::InsecureHttpNotAllowed)
        ));
    }

    #[test]
    fn rejects_openrouter_fallback() {
        let mut target = target(
            "https://openrouter.ai/api/v1",
            InferenceTransportPolicy::default(),
        );
        target.backend_kind = InferenceBackendKind::Openrouter;
        target.openrouter = Some(OpenRouterRoutePolicy {
            provider_slug: "deepinfra/turbo".into(),
            require_parameters: true,
            allow_fallbacks: true,
            data_collection: "deny".into(),
        });
        target.config_hash = target.computed_hash().unwrap();
        assert!(matches!(
            target.validate(),
            Err(InferenceConfigError::UnsafeOpenRouterRoute)
        ));
    }

    #[test]
    fn model_grants_are_short_lived_and_tamper_evident() {
        let claims = ModelGrantClaims {
            schema_version: MODEL_GRANT_SCHEMA.into(),
            run_id: "run_1".into(),
            stage_execution_id: "stage_1".into(),
            selection_id: "selection_1".into(),
            target: InferenceTargetRef {
                target_id: "fireworks".into(),
                revision: "v1".into(),
            },
            target_hash: "a".repeat(64),
            policy: InferencePolicyRef {
                policy_id: "planner".into(),
                revision: "v1".into(),
            },
            policy_hash: "b".repeat(64),
            request_sequence: 1,
            request_body_hash: "c".repeat(64),
            nonce: "nonce_nonce_nonce".into(),
            issued_at_epoch_seconds: 100,
            expires_at_epoch_seconds: 160,
        };
        let key = [7_u8; 32];
        let token = sign_model_grant(&claims, &key).unwrap();
        assert_eq!(verify_model_grant(&token, &key, 120).unwrap(), claims);
        assert!(matches!(
            verify_model_grant(&token, &key, 160),
            Err(ModelGrantError::Expired)
        ));
        let tampered = format!("{token}x");
        assert!(matches!(
            verify_model_grant(&tampered, &key, 120),
            Err(ModelGrantError::InvalidSignature | ModelGrantError::MalformedToken)
        ));
    }

    #[test]
    fn resolved_agent_profile_hash_binds_the_inference_policy() {
        let mut target = target(
            "https://models.example.com/v1",
            InferenceTransportPolicy::default(),
        );
        target
            .capabilities
            .reasoning_efforts
            .push(ReasoningEffort::High);
        target.config_hash = target.computed_hash().unwrap();
        let mut policy = StageInferencePolicyRevision {
            schema_version: INFERENCE_POLICY_SCHEMA.into(),
            policy_id: "tester-local-v1".into(),
            revision: "v1".into(),
            display_name: "Tester local low".into(),
            eligible_profiles: vec!["repo-tester".into()],
            eligible_stages: vec![InferenceStage::Test],
            target: InferenceTargetRef {
                target_id: target.target_id.clone(),
                revision: target.revision.clone(),
            },
            target_hash: target.config_hash.clone(),
            reasoning: ReasoningRequestPolicy {
                effort: Some(ReasoningEffort::Low),
                context_mode: ReasoningContextMode::CurrentTurn,
                expose_replay: true,
            },
            temperature_milli: Some(0),
            max_output_tokens: 4_096,
            max_input_tokens: 16_000,
            tool_protocol: ToolProtocolMode::NativeTools,
            transport_max_attempts: 3,
            selectable: true,
            policy_hash: String::new(),
        };
        policy.policy_hash = policy.computed_hash().unwrap();
        let mut binding = ResolvedInferenceBinding {
            schema_version: RESOLVED_INFERENCE_BINDING_SCHEMA.into(),
            target,
            policy,
            prompt_version: "repo-test-v1".into(),
            base_agent_profile_hash: format!("sha256:{}", "a".repeat(64)),
            agent_profile_hash: String::new(),
            tool_schema_hash: "tools".into(),
            profile_budget_hash: "budget".into(),
            binding_hash: String::new(),
        };
        let first_hash = binding.computed_agent_profile_hash().unwrap();

        binding.policy.reasoning.effort = Some(ReasoningEffort::High);
        binding.policy.policy_hash = binding.policy.computed_hash().unwrap();
        let second_hash = binding.computed_agent_profile_hash().unwrap();

        assert_ne!(first_hash, second_hash);
        binding.agent_profile_hash = second_hash;
        binding.binding_hash = binding.computed_hash().unwrap();
        binding.validate().unwrap();
    }
}
