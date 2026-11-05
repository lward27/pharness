//! Campus lab profiles.
//!
//! A Campus profile is a strict, versioned envelope around the existing
//! [`AgentProfile`] tool and budget shape. It adds the sandbox isolation,
//! immutable image, resource, network, TTL, model and capability bounds that a
//! Campus session runs under. Every field is required: an omitted field must
//! never fall back to a default that grants more authority.

use crate::{canonical_json_sha256, AgentProfile, RunBudget};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CAMPUS_PROFILE_SCHEMA: &str = "pharness.dev/campus-profile/v1alpha1";
/// Recorded only after the Security profile passes its hardware-backed gate.
pub const SECURITY_PROFILE_QUALIFICATION: &str = "campus-security-profile";

pub const MAX_CPU_MILLICORES: u32 = 4_000;
pub const MAX_MEMORY_MIB: u32 = 8_192;
pub const MAX_EPHEMERAL_STORAGE_MIB: u32 = 16_384;
pub const MIN_PIDS: u32 = 32;
pub const MAX_PIDS: u32 = 1_024;
pub const MIN_TTL_SECONDS: u32 = 300;
pub const MAX_TTL_SECONDS: u32 = 14_400;
pub const MAX_SESSION_ALLOCATION_MICRO_USD: u64 = 10_000_000;
pub const MAX_OUTPUT_TOKENS: u32 = 32_768;
pub const MAX_CONCURRENT_MODEL_REQUESTS: u32 = 4;
pub const MAX_EGRESS_DESTINATIONS: usize = 32;
const MAX_PROFILE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusProfileKind {
    Research,
    Coding,
    Security,
}

/// Required isolation class. The execution provider maps it to a qualified
/// RuntimeClass and refuses the launch if none is available; it never falls
/// back to a weaker runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusIsolation {
    Gvisor,
    HardwareIsolated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusResourceCeilings {
    pub cpu_millicores: u32,
    pub memory_mib: u32,
    pub ephemeral_storage_mib: u32,
    pub pids: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusNetworkMode {
    Denied,
    ApprovedEgress,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusNetworkProfile {
    pub mode: CampusNetworkMode,
    pub egress: Vec<CampusEgressDestination>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusEgressDestination {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusAgentBudget {
    pub initial_turns: u32,
    pub hard_turns: u32,
    pub initial_tokens: u64,
    pub hard_tokens: u64,
    pub active_execution_seconds: u64,
}

impl CampusAgentBudget {
    pub fn run_budget(&self) -> RunBudget {
        RunBudget {
            initial_turns: self.initial_turns,
            hard_turns: self.hard_turns,
            initial_tokens: self.initial_tokens,
            hard_tokens: self.hard_tokens,
            active_execution_seconds: self.active_execution_seconds,
            recoverable_tool_errors: 4,
            identical_failures: 2,
            verification_reserve_turns: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusModelPolicy {
    pub model_aliases: Vec<String>,
    /// Integer currency units, never floating point.
    pub session_allocation_micro_usd: u64,
    pub max_output_tokens: u32,
    pub max_concurrent_requests: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusCapabilityScope {
    AgentControl,
    SessionWorkspace,
    SessionShell,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusCapabilityBinding {
    pub tool: String,
    pub scope: CampusCapabilityScope,
}

/// Choices a student may make at launch. Each one can only narrow the
/// profile: a subset of the approved model aliases and a shorter TTL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusStudentChoices {
    pub model_aliases: Vec<String>,
    pub min_ttl_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusLaunchGate {
    Enabled,
    RequiresQualification(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusProfile {
    pub schema_version: String,
    pub id: String,
    pub version: String,
    pub kind: CampusProfileKind,
    pub isolation: CampusIsolation,
    pub image: String,
    pub resources: CampusResourceCeilings,
    pub network: CampusNetworkProfile,
    pub ttl_seconds: u32,
    pub agent_budget: CampusAgentBudget,
    pub model_policy: CampusModelPolicy,
    pub capabilities: Vec<CampusCapabilityBinding>,
    pub student_choices: CampusStudentChoices,
    pub launch_gate: CampusLaunchGate,
}

/// The identity a Campus session records for the profile it launched with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusProfileRef {
    pub id: String,
    pub version: String,
    pub profile_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CampusProfileError {
    #[error("campus profile does not match {CAMPUS_PROFILE_SCHEMA}: {0}")]
    Malformed(String),
    #[error("campus profile field {field} is invalid: {reason}")]
    Invalid { field: String, reason: String },
    #[error(
        "campus profile {profile} cannot launch until qualification {qualification} is recorded"
    )]
    LaunchRefused {
        profile: String,
        qualification: String,
    },
}

fn invalid(field: impl Into<String>, reason: impl Into<String>) -> CampusProfileError {
    CampusProfileError::Invalid {
        field: field.into(),
        reason: reason.into(),
    }
}

/// Tools a Campus profile may bind, each with the only scope it may run in.
/// SDLC submission, evidence and cluster tools are intentionally absent.
const CAMPUS_TOOL_CATALOG: &[(&str, CampusCapabilityScope)] = &[
    ("finish", CampusCapabilityScope::AgentControl),
    ("environment_info", CampusCapabilityScope::SessionWorkspace),
    ("list_dir", CampusCapabilityScope::SessionWorkspace),
    ("read_file", CampusCapabilityScope::SessionWorkspace),
    ("search_files", CampusCapabilityScope::SessionWorkspace),
    ("create_directory", CampusCapabilityScope::SessionWorkspace),
    ("write_file", CampusCapabilityScope::SessionWorkspace),
    ("patch_file", CampusCapabilityScope::SessionWorkspace),
    ("apply_patch", CampusCapabilityScope::SessionWorkspace),
    ("git_diff", CampusCapabilityScope::SessionWorkspace),
    ("git_status", CampusCapabilityScope::SessionWorkspace),
    ("run_workspace_command", CampusCapabilityScope::SessionShell),
];

pub(crate) fn is_campus_tool(tool: &str) -> bool {
    CAMPUS_TOOL_CATALOG.iter().any(|(name, _)| *name == tool)
}

#[cfg(test)]
pub(crate) fn campus_tool_catalog() -> &'static [(&'static str, CampusCapabilityScope)] {
    CAMPUS_TOOL_CATALOG
}

impl CampusProfile {
    /// Parse and fully validate a profile document.
    pub fn parse(document: &str) -> Result<Self, CampusProfileError> {
        if document.len() > MAX_PROFILE_BYTES {
            return Err(CampusProfileError::Malformed(format!(
                "document exceeds {MAX_PROFILE_BYTES} bytes"
            )));
        }
        let profile: Self = serde_json::from_str(document)
            .map_err(|error| CampusProfileError::Malformed(error.to_string()))?;
        profile.validate()?;
        Ok(profile)
    }

    pub fn validate(&self) -> Result<(), CampusProfileError> {
        if self.schema_version != CAMPUS_PROFILE_SCHEMA {
            return Err(invalid(
                "schema_version",
                format!("expected {CAMPUS_PROFILE_SCHEMA}"),
            ));
        }
        validate_identifier("id", &self.id)?;
        if !self.id.starts_with("campus-") {
            return Err(invalid("id", "must start with campus-"));
        }
        validate_identifier("version", &self.version)?;
        self.validate_isolation()?;
        validate_immutable_image(&self.image)?;
        self.validate_resources()?;
        self.validate_network()?;
        self.validate_time_and_budget()?;
        self.validate_model_policy()?;
        self.validate_capabilities()?;
        self.validate_student_choices()?;
        Ok(())
    }

    fn validate_isolation(&self) -> Result<(), CampusProfileError> {
        if self.kind == CampusProfileKind::Security {
            if self.isolation != CampusIsolation::HardwareIsolated {
                return Err(invalid(
                    "isolation",
                    "security profiles require hardware_isolated",
                ));
            }
            if self.launch_gate
                != CampusLaunchGate::RequiresQualification(SECURITY_PROFILE_QUALIFICATION.into())
            {
                return Err(invalid(
                    "launch_gate",
                    format!(
                        "security profiles require qualification {SECURITY_PROFILE_QUALIFICATION}"
                    ),
                ));
            }
        }
        if let CampusLaunchGate::RequiresQualification(qualification) = &self.launch_gate {
            validate_identifier("launch_gate", qualification)?;
        }
        Ok(())
    }

    fn validate_resources(&self) -> Result<(), CampusProfileError> {
        let resources = &self.resources;
        check_range(
            "resources.cpu_millicores",
            resources.cpu_millicores,
            100,
            MAX_CPU_MILLICORES,
        )?;
        check_range(
            "resources.memory_mib",
            resources.memory_mib,
            128,
            MAX_MEMORY_MIB,
        )?;
        check_range(
            "resources.ephemeral_storage_mib",
            resources.ephemeral_storage_mib,
            128,
            MAX_EPHEMERAL_STORAGE_MIB,
        )?;
        check_range("resources.pids", resources.pids, MIN_PIDS, MAX_PIDS)
    }

    fn validate_network(&self) -> Result<(), CampusProfileError> {
        let egress = &self.network.egress;
        match self.network.mode {
            CampusNetworkMode::Denied if !egress.is_empty() => {
                return Err(invalid(
                    "network.egress",
                    "denied network must not list destinations",
                ))
            }
            CampusNetworkMode::ApprovedEgress if egress.is_empty() => {
                return Err(invalid(
                    "network.egress",
                    "approved_egress requires at least one destination",
                ))
            }
            _ => {}
        }
        if self.kind == CampusProfileKind::Security
            && self.network.mode != CampusNetworkMode::Denied
        {
            return Err(invalid(
                "network.mode",
                "security profiles must not reach external destinations",
            ));
        }
        if egress.len() > MAX_EGRESS_DESTINATIONS {
            return Err(invalid(
                "network.egress",
                format!("at most {MAX_EGRESS_DESTINATIONS} destinations"),
            ));
        }
        let mut seen = BTreeSet::new();
        for (index, destination) in egress.iter().enumerate() {
            let field = format!("network.egress[{index}]");
            validate_egress_host(&field, &destination.host)?;
            if destination.port == 0 {
                return Err(invalid(field, "port must be between 1 and 65535"));
            }
            if !seen.insert(destination) {
                return Err(invalid(field, "duplicate destination"));
            }
        }
        Ok(())
    }

    fn validate_time_and_budget(&self) -> Result<(), CampusProfileError> {
        check_range(
            "ttl_seconds",
            self.ttl_seconds,
            MIN_TTL_SECONDS,
            MAX_TTL_SECONDS,
        )?;
        self.agent_budget
            .run_budget()
            .validate()
            .map_err(|error| invalid("agent_budget", error.to_string()))?;
        if self.agent_budget.active_execution_seconds > u64::from(self.ttl_seconds) {
            return Err(invalid(
                "agent_budget.active_execution_seconds",
                "must not exceed ttl_seconds",
            ));
        }
        Ok(())
    }

    fn validate_model_policy(&self) -> Result<(), CampusProfileError> {
        let policy = &self.model_policy;
        validate_identifier_set("model_policy.model_aliases", &policy.model_aliases, false)?;
        if policy.session_allocation_micro_usd == 0
            || policy.session_allocation_micro_usd > MAX_SESSION_ALLOCATION_MICRO_USD
        {
            return Err(invalid(
                "model_policy.session_allocation_micro_usd",
                format!("must be between 1 and {MAX_SESSION_ALLOCATION_MICRO_USD}"),
            ));
        }
        check_range(
            "model_policy.max_output_tokens",
            policy.max_output_tokens,
            1,
            MAX_OUTPUT_TOKENS,
        )?;
        check_range(
            "model_policy.max_concurrent_requests",
            policy.max_concurrent_requests,
            1,
            MAX_CONCURRENT_MODEL_REQUESTS,
        )
    }

    fn validate_capabilities(&self) -> Result<(), CampusProfileError> {
        if self.capabilities.is_empty() {
            return Err(invalid("capabilities", "at least one tool is required"));
        }
        let mut seen = BTreeSet::new();
        for (index, binding) in self.capabilities.iter().enumerate() {
            let field = format!("capabilities[{index}]");
            let Some((_, scope)) = CAMPUS_TOOL_CATALOG
                .iter()
                .find(|(tool, _)| *tool == binding.tool)
            else {
                return Err(invalid(
                    field,
                    format!("{} is not a Campus tool", binding.tool),
                ));
            };
            if *scope != binding.scope {
                return Err(invalid(
                    field,
                    format!("{} must be bound to scope {scope:?}", binding.tool),
                ));
            }
            if !seen.insert(binding.tool.as_str()) {
                return Err(invalid(field, "duplicate tool"));
            }
        }
        Ok(())
    }

    fn validate_student_choices(&self) -> Result<(), CampusProfileError> {
        let choices = &self.student_choices;
        validate_identifier_set(
            "student_choices.model_aliases",
            &choices.model_aliases,
            true,
        )?;
        if let Some(alias) = choices
            .model_aliases
            .iter()
            .find(|alias| !self.model_policy.model_aliases.contains(alias))
        {
            return Err(invalid(
                "student_choices.model_aliases",
                format!("{alias} is not an approved model alias"),
            ));
        }
        if choices.min_ttl_seconds < MIN_TTL_SECONDS || choices.min_ttl_seconds > self.ttl_seconds {
            return Err(invalid(
                "student_choices.min_ttl_seconds",
                format!("must be between {MIN_TTL_SECONDS} and ttl_seconds"),
            ));
        }
        Ok(())
    }

    /// Content hash over the canonical JSON form; key order and whitespace in
    /// the source document do not affect it.
    pub fn profile_hash(&self) -> String {
        let value = serde_json::to_value(self).expect("campus profile serializes");
        canonical_json_sha256(&value).expect("canonical campus profile serializes")
    }

    pub fn profile_ref(&self) -> CampusProfileRef {
        CampusProfileRef {
            id: self.id.clone(),
            version: self.version.clone(),
            profile_hash: self.profile_hash(),
        }
    }

    /// Refuse a launch whose gate has not been qualified.
    pub fn check_launchable(
        &self,
        recorded_qualifications: &BTreeSet<String>,
    ) -> Result<(), CampusProfileError> {
        match &self.launch_gate {
            CampusLaunchGate::Enabled => Ok(()),
            CampusLaunchGate::RequiresQualification(qualification)
                if recorded_qualifications.contains(qualification) =>
            {
                Ok(())
            }
            CampusLaunchGate::RequiresQualification(qualification) => {
                Err(CampusProfileError::LaunchRefused {
                    profile: self.id.clone(),
                    qualification: qualification.clone(),
                })
            }
        }
    }

    /// The `AgentProfile` the PHarness runtime enforces for this envelope. Its
    /// tool allowlist and budget come only from the envelope, and its hash is
    /// the envelope hash, so a session records one profile identity.
    pub fn agent_profile(
        &self,
        model_alias: &str,
        prompt_version: &str,
    ) -> Result<AgentProfile, CampusProfileError> {
        if !self
            .model_policy
            .model_aliases
            .iter()
            .any(|alias| alias == model_alias)
        {
            return Err(invalid(
                "model_alias",
                format!("{model_alias} is not approved for {}", self.id),
            ));
        }
        validate_identifier("prompt_version", prompt_version)?;
        Ok(AgentProfile {
            id: self.id.clone(),
            version: self.version.clone(),
            profile_hash: self.profile_hash(),
            prompt_version: prompt_version.into(),
            model: model_alias.into(),
            tools: self
                .capabilities
                .iter()
                .map(|binding| binding.tool.clone())
                .collect(),
            budget: self.agent_budget.run_budget(),
        })
    }
}

fn check_range(field: &str, value: u32, min: u32, max: u32) -> Result<(), CampusProfileError> {
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(invalid(field, format!("must be between {min} and {max}")))
    }
}

pub(crate) fn validate_identifier(field: &str, value: &str) -> Result<(), CampusProfileError> {
    let valid = (1..=64).contains(&value.len())
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        });
    if valid {
        Ok(())
    } else {
        Err(invalid(field, "must be a lowercase identifier"))
    }
}

fn validate_identifier_set(
    field: &str,
    values: &[String],
    allow_empty: bool,
) -> Result<(), CampusProfileError> {
    if values.is_empty() && !allow_empty {
        return Err(invalid(field, "at least one value is required"));
    }
    let mut seen = BTreeSet::new();
    for value in values {
        validate_identifier(field, value)?;
        if !seen.insert(value) {
            return Err(invalid(field, format!("duplicate value {value}")));
        }
    }
    Ok(())
}

/// Only `repository@sha256:<digest>` is accepted. Tags, including a tag next
/// to a digest, are rejected so the reference has exactly one meaning.
fn validate_immutable_image(value: &str) -> Result<(), CampusProfileError> {
    let Some((repository, digest)) = value.split_once("@sha256:") else {
        return Err(invalid("image", "must be repository@sha256:digest"));
    };
    let digest_valid = digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    let last_segment = repository.rsplit('/').next().unwrap_or_default();
    let repository_valid = repository.contains('/')
        && !last_segment.is_empty()
        && !last_segment.contains(':')
        && repository.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'-' | b'_' | b'/' | b':')
        });
    if digest_valid && repository_valid {
        Ok(())
    } else {
        Err(invalid(
            "image",
            "must be an untagged lowercase repository with a sha256 digest",
        ))
    }
}

/// Destinations are DNS names only. IP literals, wildcards and local or
/// metadata names are rejected; resolution-time checks remain the network
/// enforcer's job.
pub(crate) fn validate_egress_host(field: &str, host: &str) -> Result<(), CampusProfileError> {
    let labels = host.split('.').collect::<Vec<_>>();
    let well_formed = host.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
        && labels
            .last()
            .is_some_and(|tld| tld.bytes().any(|byte| byte.is_ascii_lowercase()));
    let local = [
        "localhost",
        "local",
        "internal",
        "localdomain",
        "home",
        "lan",
    ]
    .iter()
    .any(|suffix| labels.last() == Some(suffix))
        || labels.first() == Some(&"metadata");
    if well_formed && !local {
        Ok(())
    } else {
        Err(invalid(
            field,
            format!("{host} is not an approved public DNS name"),
        ))
    }
}

#[cfg(test)]
mod tests;
