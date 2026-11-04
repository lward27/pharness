//! Campus policy composition.
//!
//! A Campus session runs under one effective policy composed from its
//! [`CampusProfile`] and authority-tagged [`CampusPolicyLayer`]s in the fixed
//! order platform, institution, assignment, student. Platform and institution
//! layers are mandatory; assignment and student layers are optional.
//!
//! Composition is monotonic. Sets (tools, egress destinations, model aliases)
//! are intersected and limits (TTL, active time, CPU, memory, storage, PIDs,
//! spend and token budgets) take the minimum. A lower authority that asks for
//! more than an authority above it is refused with
//! [`CampusPolicyError::Widening`] rather than clamped, so the refusal itself
//! is evidence of the attempt and an effective policy always equals what every
//! authority wrote.
//!
//! Platform and institution layers are shared across profiles, so they are
//! checked against each other and only narrow the profile. Assignment and
//! student layers are written against one profile, so they are checked
//! against the effective policy composed so far, which includes the profile.
//!
//! Platform and institution mandatory denies remove tools, egress hosts and
//! model aliases from the effective policy and deny operation kinds outright.
//! [`EffectiveCampusPolicy::evaluate_action`] checks them before the SDLC
//! [`SafetyPolicy`] baseline, its permission grants or any Campus approval, so
//! none of those can lift them.
//!
//! This layer decides what an agent may ask the runtime to do. It is not the
//! isolation boundary: shell access still depends on the sandbox, network and
//! resource controls the execution provider enforces.

use super::{classify_command, CommandClass, PolicyDecision, RiskLevel, SafetyPolicy};
use crate::campus_profile::{
    is_campus_tool, validate_egress_host, validate_identifier, MAX_CONCURRENT_MODEL_REQUESTS,
    MAX_CPU_MILLICORES, MAX_EGRESS_DESTINATIONS, MAX_EPHEMERAL_STORAGE_MIB, MAX_MEMORY_MIB,
    MAX_OUTPUT_TOKENS, MAX_PIDS, MAX_SESSION_ALLOCATION_MICRO_USD, MAX_TTL_SECONDS, MIN_PIDS,
    MIN_TTL_SECONDS,
};
use crate::{
    canonical_json_sha256, ActionId, AgentAction, ApprovalKind, CampusEgressDestination,
    CampusProfile, CampusProfileError, CampusProfileRef, RunScope,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const CAMPUS_POLICY_LAYER_SCHEMA: &str = "pharness.dev/campus-policy-layer/v1alpha1";
pub const EFFECTIVE_CAMPUS_POLICY_SCHEMA: &str = "pharness.dev/effective-campus-policy/v1alpha1";
/// The `RunBudget` hard-token cap.
pub const MAX_HARD_TOKENS: u64 = 1_000_000;
const MAX_LAYER_BYTES: usize = 64 * 1024;

/// The authorities that may constrain a Campus session, in composition order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusAuthority {
    Platform,
    Institution,
    /// The instructor who sets the assignment.
    Assignment,
    Student,
}

impl CampusAuthority {
    pub const REQUIRED: [Self; 2] = [Self::Platform, Self::Institution];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Platform => "platform",
            Self::Institution => "institution",
            Self::Assignment => "assignment",
            Self::Student => "student",
        }
    }

    /// Shared authorities write policy independently of any one profile and
    /// are the only ones that may declare mandatory denies.
    pub fn is_shared(self) -> bool {
        matches!(self, Self::Platform | Self::Institution)
    }
}

impl fmt::Display for CampusAuthority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Where an effective bound or a decision came from: the profile envelope or
/// one of the authorities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusPolicySource {
    Profile,
    Platform,
    Institution,
    Assignment,
    Student,
}

impl From<CampusAuthority> for CampusPolicySource {
    fn from(authority: CampusAuthority) -> Self {
        match authority {
            CampusAuthority::Platform => Self::Platform,
            CampusAuthority::Institution => Self::Institution,
            CampusAuthority::Assignment => Self::Assignment,
            CampusAuthority::Student => Self::Student,
        }
    }
}

impl fmt::Display for CampusPolicySource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Profile => "profile",
            Self::Platform => "platform",
            Self::Institution => "institution",
            Self::Assignment => "assignment",
            Self::Student => "student",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusPolicyDimension {
    Tools,
    Egress,
    ModelAliases,
    TtlSeconds,
    ActiveExecutionSeconds,
    CpuMillicores,
    MemoryMib,
    EphemeralStorageMib,
    Pids,
    SessionAllocationMicroUsd,
    HardTokens,
    MaxOutputTokens,
    MaxConcurrentRequests,
}

impl CampusPolicyDimension {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tools => "tools",
            Self::Egress => "egress",
            Self::ModelAliases => "model_aliases",
            Self::TtlSeconds => "ttl_seconds",
            Self::ActiveExecutionSeconds => "active_execution_seconds",
            Self::CpuMillicores => "cpu_millicores",
            Self::MemoryMib => "memory_mib",
            Self::EphemeralStorageMib => "ephemeral_storage_mib",
            Self::Pids => "pids",
            Self::SessionAllocationMicroUsd => "session_allocation_micro_usd",
            Self::HardTokens => "hard_tokens",
            Self::MaxOutputTokens => "max_output_tokens",
            Self::MaxConcurrentRequests => "max_concurrent_requests",
        }
    }
}

impl fmt::Display for CampusPolicyDimension {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Ceilings one authority places on a session. An omitted dimension inherits
/// the bound above it and never widens anything. Set dimensions are
/// allowlists; limit dimensions are maximums.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusPolicyConstraints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub egress: Option<Vec<CampusEgressDestination>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_aliases: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_execution_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_millicores: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mib: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ephemeral_storage_mib: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pids: Option<u32>,
    /// Integer currency units, never floating point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_allocation_micro_usd: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hard_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_concurrent_requests: Option<u32>,
}

/// Denies that no lower authority, permission grant or approval can lift.
/// `approval_kinds` uses the approval vocabulary: an operation of a listed
/// kind is denied instead of becoming approvable. Every field is required so
/// an omitted list is never mistaken for an intentional empty one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusMandatoryDenies {
    pub tools: Vec<String>,
    pub egress_hosts: Vec<String>,
    pub model_aliases: Vec<String>,
    pub approval_kinds: Vec<ApprovalKind>,
}

impl CampusMandatoryDenies {
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
            && self.egress_hosts.is_empty()
            && self.model_aliases.is_empty()
            && self.approval_kinds.is_empty()
    }

    fn denies(&self, dimension: CampusPolicyDimension, value: &str) -> bool {
        match dimension {
            CampusPolicyDimension::Tools => self.tools.iter().any(|tool| tool == value),
            CampusPolicyDimension::Egress => {
                let host = egress_key_host(value);
                self.egress_hosts.iter().any(|denied| denied == host)
            }
            CampusPolicyDimension::ModelAliases => {
                self.model_aliases.iter().any(|alias| alias == value)
            }
            _ => false,
        }
    }

    fn normalized(&self) -> Self {
        let mut denies = self.clone();
        denies.tools.sort();
        denies.egress_hosts.sort();
        denies.model_aliases.sort();
        denies
            .approval_kinds
            .sort_by_key(|kind| approval_kind_name(*kind));
        denies
    }
}

/// One authority's policy for Campus sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusPolicyLayer {
    pub schema_version: String,
    pub authority: CampusAuthority,
    pub id: String,
    pub version: String,
    pub constraints: CampusPolicyConstraints,
    pub mandatory_denies: CampusMandatoryDenies,
}

/// The identity an effective policy records for each layer it composed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusPolicyLayerRef {
    pub authority: CampusAuthority,
    pub id: String,
    pub version: String,
    pub layer_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CampusPolicyError {
    #[error("campus profile refused: {0}")]
    Profile(#[from] CampusProfileError),
    #[error("campus policy layer does not match {CAMPUS_POLICY_LAYER_SCHEMA}: {0}")]
    Malformed(String),
    #[error("campus {authority} policy layer field {field} is invalid: {reason}")]
    InvalidLayer {
        authority: CampusAuthority,
        field: String,
        reason: String,
    },
    #[error("campus {authority} policy layer is required")]
    MissingLayer { authority: CampusAuthority },
    #[error("campus {authority} policy layer appears more than once")]
    DuplicateAuthority { authority: CampusAuthority },
    #[error("campus {authority} policy layer must not follow the {after} layer")]
    OutOfOrder {
        authority: CampusAuthority,
        after: CampusAuthority,
    },
    #[error(
        "campus {authority} policy would widen {dimension}: {requested} exceeds the bound set by {bounded_by} ({permitted})"
    )]
    Widening {
        authority: CampusAuthority,
        dimension: CampusPolicyDimension,
        requested: String,
        permitted: String,
        bounded_by: CampusPolicySource,
    },
    #[error("campus {authority} policy requests {dimension} {value}, which {denied_by} policy mandatorily denies")]
    MandatoryDenyConflict {
        authority: CampusAuthority,
        dimension: CampusPolicyDimension,
        value: String,
        denied_by: CampusAuthority,
    },
    #[error("campus {authority} policy may not set {dimension}")]
    DimensionNotPermitted {
        authority: CampusAuthority,
        dimension: CampusPolicyDimension,
    },
    #[error("effective campus policy leaves no {dimension}; refusing launch")]
    EmptyDimension { dimension: CampusPolicyDimension },
}

/// The dimensions a student may set, matching the profile's student choices.
const STUDENT_DIMENSIONS: [CampusPolicyDimension; 2] = [
    CampusPolicyDimension::ModelAliases,
    CampusPolicyDimension::TtlSeconds,
];

impl CampusPolicyLayer {
    /// Parse and fully validate a layer document.
    pub fn parse(document: &str) -> Result<Self, CampusPolicyError> {
        if document.len() > MAX_LAYER_BYTES {
            return Err(CampusPolicyError::Malformed(format!(
                "document exceeds {MAX_LAYER_BYTES} bytes"
            )));
        }
        let layer: Self = serde_json::from_str(document)
            .map_err(|error| CampusPolicyError::Malformed(error.to_string()))?;
        layer.validate()?;
        Ok(layer)
    }

    pub fn validate(&self) -> Result<(), CampusPolicyError> {
        let authority = self.authority;
        let invalid = |field: &str, reason: &str| CampusPolicyError::InvalidLayer {
            authority,
            field: field.into(),
            reason: reason.into(),
        };
        let from_profile = |error| layer_error(authority, error);
        if self.schema_version != CAMPUS_POLICY_LAYER_SCHEMA {
            return Err(invalid(
                "schema_version",
                &format!("expected {CAMPUS_POLICY_LAYER_SCHEMA}"),
            ));
        }
        validate_identifier("id", &self.id).map_err(from_profile)?;
        validate_identifier("version", &self.version).map_err(from_profile)?;

        let constraints = &self.constraints;
        if let Some(tools) = &constraints.tools {
            validate_identifiers(authority, "constraints.tools", tools)?;
            if let Some(tool) = tools.iter().find(|tool| !is_campus_tool(tool)) {
                return Err(invalid(
                    "constraints.tools",
                    &format!("{tool} is not a Campus tool"),
                ));
            }
        }
        if let Some(egress) = &constraints.egress {
            if egress.len() > MAX_EGRESS_DESTINATIONS {
                return Err(invalid(
                    "constraints.egress",
                    &format!("at most {MAX_EGRESS_DESTINATIONS} destinations"),
                ));
            }
            let mut seen = BTreeSet::new();
            for (index, destination) in egress.iter().enumerate() {
                let field = format!("constraints.egress[{index}]");
                validate_egress_host(&field, &destination.host).map_err(from_profile)?;
                if destination.port == 0 {
                    return Err(invalid(&field, "port must be between 1 and 65535"));
                }
                if !seen.insert(destination) {
                    return Err(invalid(&field, "duplicate destination"));
                }
            }
        }
        if let Some(aliases) = &constraints.model_aliases {
            validate_identifiers(authority, "constraints.model_aliases", aliases)?;
        }
        let limits = [
            (
                "constraints.ttl_seconds",
                constraints.ttl_seconds.map(u64::from),
                u64::from(MIN_TTL_SECONDS),
                u64::from(MAX_TTL_SECONDS),
            ),
            (
                "constraints.active_execution_seconds",
                constraints.active_execution_seconds,
                1,
                u64::from(MAX_TTL_SECONDS),
            ),
            (
                "constraints.cpu_millicores",
                constraints.cpu_millicores.map(u64::from),
                100,
                u64::from(MAX_CPU_MILLICORES),
            ),
            (
                "constraints.memory_mib",
                constraints.memory_mib.map(u64::from),
                128,
                u64::from(MAX_MEMORY_MIB),
            ),
            (
                "constraints.ephemeral_storage_mib",
                constraints.ephemeral_storage_mib.map(u64::from),
                128,
                u64::from(MAX_EPHEMERAL_STORAGE_MIB),
            ),
            (
                "constraints.pids",
                constraints.pids.map(u64::from),
                u64::from(MIN_PIDS),
                u64::from(MAX_PIDS),
            ),
            (
                "constraints.session_allocation_micro_usd",
                constraints.session_allocation_micro_usd,
                1,
                MAX_SESSION_ALLOCATION_MICRO_USD,
            ),
            (
                "constraints.hard_tokens",
                constraints.hard_tokens,
                1,
                MAX_HARD_TOKENS,
            ),
            (
                "constraints.max_output_tokens",
                constraints.max_output_tokens.map(u64::from),
                1,
                u64::from(MAX_OUTPUT_TOKENS),
            ),
            (
                "constraints.max_concurrent_requests",
                constraints.max_concurrent_requests.map(u64::from),
                1,
                u64::from(MAX_CONCURRENT_MODEL_REQUESTS),
            ),
        ];
        for (field, value, min, max) in limits {
            if value.is_some_and(|value| !(min..=max).contains(&value)) {
                return Err(invalid(field, &format!("must be between {min} and {max}")));
            }
        }

        let denies = &self.mandatory_denies;
        if !authority.is_shared() && !denies.is_empty() {
            return Err(invalid(
                "mandatory_denies",
                "only platform and institution layers may declare mandatory denies",
            ));
        }
        validate_identifiers(authority, "mandatory_denies.tools", &denies.tools)?;
        validate_identifiers(
            authority,
            "mandatory_denies.model_aliases",
            &denies.model_aliases,
        )?;
        let mut hosts = BTreeSet::new();
        for (index, host) in denies.egress_hosts.iter().enumerate() {
            let field = format!("mandatory_denies.egress_hosts[{index}]");
            validate_egress_host(&field, host).map_err(from_profile)?;
            if !hosts.insert(host) {
                return Err(invalid(&field, "duplicate host"));
            }
        }
        let mut kinds = BTreeSet::new();
        if let Some(kind) = denies
            .approval_kinds
            .iter()
            .find(|kind| !kinds.insert(approval_kind_name(**kind)))
        {
            return Err(invalid(
                "mandatory_denies.approval_kinds",
                &format!("duplicate value {}", approval_kind_name(*kind)),
            ));
        }
        Ok(())
    }

    /// Content hash over the canonical JSON form with every list sorted, so
    /// key order, whitespace and list order do not affect it.
    pub fn layer_hash(&self) -> String {
        let mut normalized = self.clone();
        let constraints = &mut normalized.constraints;
        for values in [&mut constraints.tools, &mut constraints.model_aliases]
            .into_iter()
            .flatten()
        {
            values.sort();
        }
        if let Some(egress) = &mut constraints.egress {
            egress.sort();
        }
        normalized.mandatory_denies = normalized.mandatory_denies.normalized();
        let value = serde_json::to_value(&normalized).expect("campus policy layer serializes");
        canonical_json_sha256(&value).expect("canonical campus policy layer serializes")
    }

    pub fn layer_ref(&self) -> CampusPolicyLayerRef {
        CampusPolicyLayerRef {
            authority: self.authority,
            id: self.id.clone(),
            version: self.version.clone(),
            layer_hash: self.layer_hash(),
        }
    }
}

fn layer_error(authority: CampusAuthority, error: CampusProfileError) -> CampusPolicyError {
    match error {
        CampusProfileError::Invalid { field, reason } => CampusPolicyError::InvalidLayer {
            authority,
            field,
            reason,
        },
        other => CampusPolicyError::Profile(other),
    }
}

fn validate_identifiers(
    authority: CampusAuthority,
    field: &str,
    values: &[String],
) -> Result<(), CampusPolicyError> {
    let mut seen = BTreeSet::new();
    for value in values {
        validate_identifier(field, value).map_err(|error| layer_error(authority, error))?;
        if !seen.insert(value) {
            return Err(CampusPolicyError::InvalidLayer {
                authority,
                field: field.into(),
                reason: format!("duplicate value {value}"),
            });
        }
    }
    Ok(())
}

fn approval_kind_name(kind: ApprovalKind) -> &'static str {
    match kind {
        ApprovalKind::FileWrite => "file_write",
        ApprovalKind::ShellCommand => "shell_command",
        ApprovalKind::Network => "network",
        ApprovalKind::Destructive => "destructive",
        ApprovalKind::Privileged => "privileged",
        ApprovalKind::SecretAccess => "secret_access",
        ApprovalKind::RegistryWrite => "registry_write",
        ApprovalKind::TektonRun => "tekton_run",
        ApprovalKind::ArgoSync => "argo_sync",
        ApprovalKind::DatabaseChange => "database_change",
        ApprovalKind::ProductionChange => "production_change",
    }
}

fn egress_key(destination: &CampusEgressDestination) -> String {
    format!("{}:{}", destination.host, destination.port)
}

// Hosts are validated DNS names and never contain ':'.
fn egress_key_host(key: &str) -> &str {
    key.rsplit_once(':').map_or(key, |(host, _)| host)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Bound {
    Set(BTreeSet<String>),
    Limit(u64),
}

impl Bound {
    fn describe(&self) -> String {
        match self {
            Self::Set(values) if values.is_empty() => "none".into(),
            Self::Set(values) => values.iter().cloned().collect::<Vec<_>>().join(", "),
            Self::Limit(value) => value.to_string(),
        }
    }
}

impl CampusPolicyConstraints {
    fn bounds(&self) -> Vec<(CampusPolicyDimension, Bound)> {
        use CampusPolicyDimension as D;
        let set = |values: &Vec<String>| Bound::Set(values.iter().cloned().collect());
        let mut bounds = Vec::new();
        if let Some(tools) = &self.tools {
            bounds.push((D::Tools, set(tools)));
        }
        if let Some(egress) = &self.egress {
            bounds.push((
                D::Egress,
                Bound::Set(egress.iter().map(egress_key).collect()),
            ));
        }
        if let Some(aliases) = &self.model_aliases {
            bounds.push((D::ModelAliases, set(aliases)));
        }
        let limits = [
            (D::TtlSeconds, self.ttl_seconds.map(u64::from)),
            (D::ActiveExecutionSeconds, self.active_execution_seconds),
            (D::CpuMillicores, self.cpu_millicores.map(u64::from)),
            (D::MemoryMib, self.memory_mib.map(u64::from)),
            (
                D::EphemeralStorageMib,
                self.ephemeral_storage_mib.map(u64::from),
            ),
            (D::Pids, self.pids.map(u64::from)),
            (
                D::SessionAllocationMicroUsd,
                self.session_allocation_micro_usd,
            ),
            (D::HardTokens, self.hard_tokens),
            (D::MaxOutputTokens, self.max_output_tokens.map(u64::from)),
            (
                D::MaxConcurrentRequests,
                self.max_concurrent_requests.map(u64::from),
            ),
        ];
        bounds.extend(
            limits.into_iter().filter_map(|(dimension, value)| {
                value.map(|value| (dimension, Bound::Limit(value)))
            }),
        );
        bounds
    }
}

/// Set members removed from the profile, keyed by dimension and member (egress
/// members are `host:port`), with the source that removed each one.
pub type RemovedMembers = BTreeMap<CampusPolicyDimension, BTreeMap<String, CampusPolicySource>>;

/// Bounds in force at one point of the composition, who set each one and
/// which authority removed each set member.
#[derive(Debug, Default)]
struct BoundState {
    bounds: BTreeMap<CampusPolicyDimension, Bound>,
    sources: BTreeMap<CampusPolicyDimension, CampusPolicySource>,
    removed: RemovedMembers,
}

impl BoundState {
    fn from_profile(profile: &CampusProfile) -> Self {
        use CampusPolicyDimension as D;
        let set = |values: &[String]| Bound::Set(values.iter().cloned().collect());
        let bounds = BTreeMap::from([
            (
                D::Tools,
                Bound::Set(
                    profile
                        .capabilities
                        .iter()
                        .map(|binding| binding.tool.clone())
                        .collect(),
                ),
            ),
            (
                D::Egress,
                Bound::Set(profile.network.egress.iter().map(egress_key).collect()),
            ),
            (D::ModelAliases, set(&profile.model_policy.model_aliases)),
            (D::TtlSeconds, Bound::Limit(profile.ttl_seconds.into())),
            (
                D::ActiveExecutionSeconds,
                Bound::Limit(profile.agent_budget.active_execution_seconds),
            ),
            (
                D::CpuMillicores,
                Bound::Limit(profile.resources.cpu_millicores.into()),
            ),
            (
                D::MemoryMib,
                Bound::Limit(profile.resources.memory_mib.into()),
            ),
            (
                D::EphemeralStorageMib,
                Bound::Limit(profile.resources.ephemeral_storage_mib.into()),
            ),
            (D::Pids, Bound::Limit(profile.resources.pids.into())),
            (
                D::SessionAllocationMicroUsd,
                Bound::Limit(profile.model_policy.session_allocation_micro_usd),
            ),
            (
                D::HardTokens,
                Bound::Limit(profile.agent_budget.hard_tokens),
            ),
            (
                D::MaxOutputTokens,
                Bound::Limit(profile.model_policy.max_output_tokens.into()),
            ),
            (
                D::MaxConcurrentRequests,
                Bound::Limit(profile.model_policy.max_concurrent_requests.into()),
            ),
        ]);
        let sources = bounds
            .keys()
            .map(|dimension| (*dimension, CampusPolicySource::Profile))
            .collect();
        Self {
            bounds,
            sources,
            removed: BTreeMap::new(),
        }
    }

    fn check(
        &self,
        authority: CampusAuthority,
        dimension: CampusPolicyDimension,
        requested: &Bound,
    ) -> Result<(), CampusPolicyError> {
        let Some(bound) = self.bounds.get(&dimension) else {
            return Ok(());
        };
        let widened = match (requested, bound) {
            (Bound::Set(requested), Bound::Set(permitted)) => {
                requested.difference(permitted).next().cloned()
            }
            (Bound::Limit(requested), Bound::Limit(permitted)) => {
                (requested > permitted).then(|| requested.to_string())
            }
            _ => unreachable!("a dimension is always a set or always a limit"),
        };
        match widened {
            None => Ok(()),
            Some(requested) => Err(CampusPolicyError::Widening {
                authority,
                dimension,
                requested,
                permitted: bound.describe(),
                bounded_by: self.sources[&dimension],
            }),
        }
    }

    /// Provenance moves to `source` only when it strictly narrows the bound.
    fn narrow(
        &mut self,
        source: CampusPolicySource,
        dimension: CampusPolicyDimension,
        requested: Bound,
    ) {
        let narrowed = match self.bounds.get_mut(&dimension) {
            None => {
                self.bounds.insert(dimension, requested);
                true
            }
            Some(Bound::Set(current)) => {
                let Bound::Set(requested) = requested else {
                    unreachable!("a dimension is always a set or always a limit")
                };
                let removed = current
                    .iter()
                    .filter(|value| !requested.contains(*value))
                    .cloned()
                    .collect::<Vec<_>>();
                current.retain(|value| requested.contains(value));
                let members = self.removed.entry(dimension).or_default();
                for value in &removed {
                    members.insert(value.clone(), source);
                }
                !removed.is_empty()
            }
            Some(Bound::Limit(current)) => {
                let Bound::Limit(requested) = requested else {
                    unreachable!("a dimension is always a set or always a limit")
                };
                let narrower = requested < *current;
                *current = (*current).min(requested);
                narrower
            }
        };
        if narrowed {
            self.sources.insert(dimension, source);
        }
    }

    fn apply_denies(&mut self, authority: CampusAuthority, denies: &CampusMandatoryDenies) {
        for dimension in [
            CampusPolicyDimension::Tools,
            CampusPolicyDimension::Egress,
            CampusPolicyDimension::ModelAliases,
        ] {
            if let Some(Bound::Set(values)) = self.bounds.get_mut(&dimension) {
                let removed = values
                    .iter()
                    .filter(|value| denies.denies(dimension, value))
                    .cloned()
                    .collect::<Vec<_>>();
                values.retain(|value| !denies.denies(dimension, value));
                let members = self.removed.entry(dimension).or_default();
                for value in &removed {
                    members.insert(value.clone(), authority.into());
                }
                if !removed.is_empty() {
                    self.sources.insert(dimension, authority.into());
                }
            }
        }
    }

    fn set(&self, dimension: CampusPolicyDimension) -> &BTreeSet<String> {
        match &self.bounds[&dimension] {
            Bound::Set(values) => values,
            Bound::Limit(_) => unreachable!("{dimension} is a set dimension"),
        }
    }

    fn limit(&self, dimension: CampusPolicyDimension) -> u64 {
        match &self.bounds[&dimension] {
            Bound::Limit(value) => *value,
            Bound::Set(_) => unreachable!("{dimension} is a limit dimension"),
        }
    }

    fn limit_u32(&self, dimension: CampusPolicyDimension) -> u32 {
        u32::try_from(self.limit(dimension)).expect("u32 dimensions only narrow from u32 bounds")
    }
}

fn check_layer_order(layers: &[CampusPolicyLayer]) -> Result<(), CampusPolicyError> {
    let mut seen = BTreeSet::new();
    let mut previous: Option<CampusAuthority> = None;
    for layer in layers {
        let authority = layer.authority;
        if !seen.insert(authority) {
            return Err(CampusPolicyError::DuplicateAuthority { authority });
        }
        if let Some(after) = previous.filter(|after| authority < *after) {
            return Err(CampusPolicyError::OutOfOrder { authority, after });
        }
        previous = Some(authority);
    }
    match CampusAuthority::REQUIRED
        .into_iter()
        .find(|authority| !seen.contains(authority))
    {
        Some(authority) => Err(CampusPolicyError::MissingLayer { authority }),
        None => Ok(()),
    }
}

fn check_mandatory_conflicts(
    layer: &CampusPolicyLayer,
    denies: &[CampusAuthorityDenies],
) -> Result<(), CampusPolicyError> {
    for (dimension, bound) in layer.constraints.bounds() {
        let Bound::Set(values) = bound else {
            continue;
        };
        for value in values {
            if let Some(denied) = denies
                .iter()
                .find(|denied| denied.denies.denies(dimension, &value))
            {
                return Err(CampusPolicyError::MandatoryDenyConflict {
                    authority: layer.authority,
                    dimension,
                    value,
                    denied_by: denied.authority,
                });
            }
        }
    }
    Ok(())
}

/// A student may only make the choices the profile offers: a subset of its
/// student model aliases and a TTL no shorter than its student minimum.
fn check_student_layer(
    layer: &CampusPolicyLayer,
    profile: &CampusProfile,
) -> Result<(), CampusPolicyError> {
    let choices = &profile.student_choices;
    for (dimension, bound) in layer.constraints.bounds() {
        match (dimension, bound) {
            (CampusPolicyDimension::ModelAliases, Bound::Set(aliases)) => {
                if let Some(alias) = aliases
                    .iter()
                    .find(|alias| !choices.model_aliases.contains(alias))
                {
                    return Err(CampusPolicyError::Widening {
                        authority: CampusAuthority::Student,
                        dimension,
                        requested: alias.clone(),
                        permitted: Bound::Set(choices.model_aliases.iter().cloned().collect())
                            .describe(),
                        bounded_by: CampusPolicySource::Profile,
                    });
                }
            }
            (CampusPolicyDimension::TtlSeconds, Bound::Limit(ttl)) => {
                if ttl < u64::from(choices.min_ttl_seconds) {
                    return Err(CampusPolicyError::InvalidLayer {
                        authority: CampusAuthority::Student,
                        field: "constraints.ttl_seconds".into(),
                        reason: format!(
                            "must not be below the profile student minimum {}",
                            choices.min_ttl_seconds
                        ),
                    });
                }
            }
            (dimension, _) if !STUDENT_DIMENSIONS.contains(&dimension) => {
                return Err(CampusPolicyError::DimensionNotPermitted {
                    authority: CampusAuthority::Student,
                    dimension,
                });
            }
            _ => {}
        }
    }
    Ok(())
}

/// The concrete bounds a Campus session runs under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CampusEffectiveConstraints {
    pub tools: BTreeSet<String>,
    /// Empty means the session has no external network access.
    pub egress: BTreeSet<CampusEgressDestination>,
    pub model_aliases: BTreeSet<String>,
    pub ttl_seconds: u32,
    pub active_execution_seconds: u64,
    pub cpu_millicores: u32,
    pub memory_mib: u32,
    pub ephemeral_storage_mib: u32,
    pub pids: u32,
    pub session_allocation_micro_usd: u64,
    pub hard_tokens: u64,
    pub max_output_tokens: u32,
    pub max_concurrent_requests: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CampusAuthorityDenies {
    pub authority: CampusAuthority,
    pub denies: CampusMandatoryDenies,
}

/// The composed policy for one Campus session. It can only be produced by
/// [`EffectiveCampusPolicy::compose`], and its hash binds the profile hash,
/// every layer hash, the effective bounds, the mandatory denies and the
/// provenance of each dimension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EffectiveCampusPolicy {
    schema_version: String,
    profile: CampusProfileRef,
    layers: Vec<CampusPolicyLayerRef>,
    constraints: CampusEffectiveConstraints,
    mandatory_denies: Vec<CampusAuthorityDenies>,
    provenance: BTreeMap<CampusPolicyDimension, CampusPolicySource>,
    /// Profile members each authority removed, by dimension.
    removed: RemovedMembers,
    effective_policy_hash: String,
}

impl EffectiveCampusPolicy {
    /// Compose `profile` with `layers`, which must be in authority order and
    /// include the platform and institution layers. Any missing, duplicate,
    /// out-of-order or invalid layer, any widening attempt and any profile
    /// whose launch gate is not qualified refuses the launch.
    pub fn compose(
        profile: &CampusProfile,
        layers: &[CampusPolicyLayer],
        recorded_qualifications: &BTreeSet<String>,
    ) -> Result<Self, CampusPolicyError> {
        profile.validate()?;
        profile.check_launchable(recorded_qualifications)?;
        check_layer_order(layers)?;
        for layer in layers {
            layer.validate()?;
        }

        let mut effective = BoundState::from_profile(profile);
        let mut shared_ceiling = BoundState::default();
        let mut denies: Vec<CampusAuthorityDenies> = Vec::new();
        for layer in layers {
            let authority = layer.authority;
            if !layer.mandatory_denies.is_empty() {
                denies.push(CampusAuthorityDenies {
                    authority,
                    denies: layer.mandatory_denies.normalized(),
                });
            }
            check_mandatory_conflicts(layer, &denies)?;
            let bounds = layer.constraints.bounds();
            let against = if authority.is_shared() {
                &shared_ceiling
            } else {
                &effective
            };
            for (dimension, bound) in &bounds {
                against.check(authority, *dimension, bound)?;
            }
            if authority == CampusAuthority::Student {
                check_student_layer(layer, profile)?;
            }
            for (dimension, bound) in bounds {
                if authority.is_shared() {
                    shared_ceiling.narrow(authority.into(), dimension, bound.clone());
                }
                effective.narrow(authority.into(), dimension, bound);
            }
            effective.apply_denies(authority, &layer.mandatory_denies);
        }

        // Active execution time can never outlast the session.
        let ttl = effective.limit(CampusPolicyDimension::TtlSeconds);
        if effective.limit(CampusPolicyDimension::ActiveExecutionSeconds) > ttl {
            let source = effective.sources[&CampusPolicyDimension::TtlSeconds];
            effective.narrow(
                source,
                CampusPolicyDimension::ActiveExecutionSeconds,
                Bound::Limit(ttl),
            );
        }
        for dimension in [
            CampusPolicyDimension::Tools,
            CampusPolicyDimension::ModelAliases,
        ] {
            if effective.set(dimension).is_empty() {
                return Err(CampusPolicyError::EmptyDimension { dimension });
            }
        }

        use CampusPolicyDimension as D;
        let constraints = CampusEffectiveConstraints {
            tools: effective.set(D::Tools).clone(),
            egress: effective
                .set(D::Egress)
                .iter()
                .map(|key| {
                    let (host, port) = key.rsplit_once(':').expect("egress keys carry a port");
                    CampusEgressDestination {
                        host: host.into(),
                        port: port.parse().expect("egress keys carry a valid port"),
                    }
                })
                .collect(),
            model_aliases: effective.set(D::ModelAliases).clone(),
            ttl_seconds: effective.limit_u32(D::TtlSeconds),
            active_execution_seconds: effective.limit(D::ActiveExecutionSeconds),
            cpu_millicores: effective.limit_u32(D::CpuMillicores),
            memory_mib: effective.limit_u32(D::MemoryMib),
            ephemeral_storage_mib: effective.limit_u32(D::EphemeralStorageMib),
            pids: effective.limit_u32(D::Pids),
            session_allocation_micro_usd: effective.limit(D::SessionAllocationMicroUsd),
            hard_tokens: effective.limit(D::HardTokens),
            max_output_tokens: effective.limit_u32(D::MaxOutputTokens),
            max_concurrent_requests: effective.limit_u32(D::MaxConcurrentRequests),
        };
        let mut policy = Self {
            schema_version: EFFECTIVE_CAMPUS_POLICY_SCHEMA.into(),
            profile: profile.profile_ref(),
            layers: layers.iter().map(CampusPolicyLayer::layer_ref).collect(),
            constraints,
            mandatory_denies: denies,
            provenance: effective.sources,
            removed: effective
                .removed
                .into_iter()
                .filter(|(_, members)| !members.is_empty())
                .collect(),
            effective_policy_hash: String::new(),
        };
        policy.effective_policy_hash = policy.compute_hash();
        Ok(policy)
    }

    fn compute_hash(&self) -> String {
        let mut value = serde_json::to_value(self).expect("effective campus policy serializes");
        value
            .as_object_mut()
            .expect("effective campus policy is an object")
            .remove("effective_policy_hash");
        canonical_json_sha256(&value).expect("canonical effective campus policy serializes")
    }

    pub fn effective_policy_hash(&self) -> &str {
        &self.effective_policy_hash
    }

    pub fn profile_hash(&self) -> &str {
        &self.profile.profile_hash
    }

    pub fn profile(&self) -> &CampusProfileRef {
        &self.profile
    }

    pub fn layers(&self) -> &[CampusPolicyLayerRef] {
        &self.layers
    }

    pub fn constraints(&self) -> &CampusEffectiveConstraints {
        &self.constraints
    }

    pub fn mandatory_denies(&self) -> &[CampusAuthorityDenies] {
        &self.mandatory_denies
    }

    pub fn provenance(&self) -> &BTreeMap<CampusPolicyDimension, CampusPolicySource> {
        &self.provenance
    }

    /// The source that set the effective bound for `dimension`.
    pub fn source(&self, dimension: CampusPolicyDimension) -> CampusPolicySource {
        self.provenance[&dimension]
    }

    pub fn removed(&self) -> &RemovedMembers {
        &self.removed
    }

    /// Who excluded `value` from a set dimension: the authority that removed
    /// it, or the profile when the profile never offered it.
    pub fn exclusion_source(
        &self,
        dimension: CampusPolicyDimension,
        value: &str,
    ) -> CampusPolicySource {
        self.removed
            .get(&dimension)
            .and_then(|members| members.get(value))
            .copied()
            .unwrap_or(CampusPolicySource::Profile)
    }

    fn mandatory_denier(
        &self,
        denies: impl Fn(&CampusMandatoryDenies) -> bool,
    ) -> Option<CampusAuthority> {
        self.mandatory_denies
            .iter()
            .find(|denied| denies(&denied.denies))
            .map(|denied| denied.authority)
    }

    fn record(&self, outcome: CampusDecisionOutcome, subject: String) -> CampusDecisionRecord {
        CampusDecisionRecord {
            outcome,
            subject,
            authority: CampusPolicySource::Platform,
            reason: CampusDecisionReason::BaselineAllow,
            summary: String::new(),
            risk: None,
            approval_kind: None,
            grant_id: None,
            approved_by: None,
            effective_policy_hash: self.effective_policy_hash.clone(),
            profile_hash: self.profile.profile_hash.clone(),
        }
    }

    /// Decide whether a Campus session may use `model_alias`.
    pub fn authorize_model_alias(&self, model_alias: &str) -> CampusDecisionRecord {
        self.authorize_member(
            CampusPolicyDimension::ModelAliases,
            format!("model:{model_alias}"),
            model_alias,
        )
    }

    /// Decide whether a Campus session may reach `host:port`. The network
    /// enforcer still owns resolution-time checks.
    pub fn authorize_egress(&self, host: &str, port: u16) -> CampusDecisionRecord {
        let key = format!("{host}:{port}");
        self.authorize_member(CampusPolicyDimension::Egress, format!("egress:{key}"), &key)
    }

    fn authorize_member(
        &self,
        dimension: CampusPolicyDimension,
        subject: String,
        value: &str,
    ) -> CampusDecisionRecord {
        if let Some(authority) = self.mandatory_denier(|denies| denies.denies(dimension, value)) {
            return CampusDecisionRecord {
                authority: authority.into(),
                reason: CampusDecisionReason::MandatoryDeny,
                summary: format!("{value} is mandatorily denied by {authority} policy"),
                ..self.record(CampusDecisionOutcome::Deny, subject)
            };
        }
        let permitted = match dimension {
            CampusPolicyDimension::Tools => self.constraints.tools.contains(value),
            CampusPolicyDimension::ModelAliases => self.constraints.model_aliases.contains(value),
            CampusPolicyDimension::Egress => self
                .constraints
                .egress
                .iter()
                .any(|destination| egress_key(destination) == value),
            _ => unreachable!("only set dimensions have members"),
        };
        let (outcome, authority, reason, summary) = if permitted {
            (
                CampusDecisionOutcome::Allow,
                self.source(dimension),
                CampusDecisionReason::WithinEffectivePolicy,
                format!("{value} is within the effective Campus {dimension}"),
            )
        } else {
            (
                CampusDecisionOutcome::Deny,
                self.exclusion_source(dimension, value),
                CampusDecisionReason::OutsideEffectivePolicy,
                format!("{value} is outside the effective Campus {dimension}"),
            )
        };
        CampusDecisionRecord {
            authority,
            reason,
            summary,
            ..self.record(outcome, subject)
        }
    }

    /// The Campus entry point to the SDLC [`SafetyPolicy`] evaluator.
    ///
    /// Order: platform and institution mandatory tool denies, the effective
    /// tool set, mandatory operation-kind denies, then the unchanged baseline
    /// decision, its permission grants and finally a role-bounded Campus
    /// approval. Because mandatory denies are checked first, neither a grant
    /// nor an approval can turn them into an allow. Baseline decisions are
    /// attributed to the platform, which owns the baseline policy.
    pub fn evaluate_action(
        &self,
        baseline: &SafetyPolicy,
        action: &AgentAction,
        run_scope: &RunScope,
        approval: Option<&CampusApproval>,
    ) -> CampusDecisionRecord {
        let decision = baseline.evaluate_action_without_grants(action);
        self.evaluate_with_baseline(baseline, action, decision, run_scope, approval)
    }

    /// [`Self::evaluate_action`] for an action a Campus sandbox will run.
    ///
    /// The SDLC baseline allows every workspace command because the SDLC
    /// runtime confines it to an executable allowlist declared by the
    /// repository contract. A Campus sandbox has no such allowlist, so a
    /// workspace command is given the baseline decision of the same command
    /// line run as a shell command; every other action is evaluated exactly
    /// as [`Self::evaluate_action`] does.
    pub fn evaluate_sandbox_action(
        &self,
        baseline: &SafetyPolicy,
        action: &AgentAction,
        run_scope: &RunScope,
        approval: Option<&CampusApproval>,
    ) -> CampusDecisionRecord {
        let decision = match action {
            AgentAction::RunWorkspaceCommand {
                id,
                reason,
                executable,
                args,
                cwd,
                timeout_ms,
            } => baseline.evaluate_action_without_grants(&AgentAction::RunShell {
                id: id.clone(),
                reason: reason.clone(),
                cmd: command_line(executable, args),
                cwd: cwd.clone(),
                timeout_ms: *timeout_ms,
                dry_run: false,
            }),
            _ => baseline.evaluate_action_without_grants(action),
        };
        self.evaluate_with_baseline(baseline, action, decision, run_scope, approval)
    }

    fn evaluate_with_baseline(
        &self,
        baseline: &SafetyPolicy,
        action: &AgentAction,
        decision: PolicyDecision,
        run_scope: &RunScope,
        approval: Option<&CampusApproval>,
    ) -> CampusDecisionRecord {
        let tool = action.kind_name();
        let subject = format!("action:{tool}");
        if let Some(authority) =
            self.mandatory_denier(|denies| denies.tools.iter().any(|t| t == tool))
        {
            return CampusDecisionRecord {
                authority: authority.into(),
                reason: CampusDecisionReason::MandatoryDeny,
                summary: format!("{tool} is mandatorily denied by {authority} policy"),
                ..self.record(CampusDecisionOutcome::Deny, subject)
            };
        }
        if !self.constraints.tools.contains(tool) {
            return CampusDecisionRecord {
                authority: self.exclusion_source(CampusPolicyDimension::Tools, tool),
                reason: CampusDecisionReason::OutsideEffectivePolicy,
                summary: format!("{tool} is outside the effective Campus tools"),
                ..self.record(CampusDecisionOutcome::Deny, subject)
            };
        }

        let risk = match &decision {
            PolicyDecision::Allow { risk, .. }
            | PolicyDecision::Ask { risk, .. }
            | PolicyDecision::Deny { risk, .. } => *risk,
        };
        if let Some(kind) = operation_kind(action, &decision) {
            if let Some(authority) =
                self.mandatory_denier(|denies| denies.approval_kinds.contains(&kind))
            {
                return CampusDecisionRecord {
                    authority: authority.into(),
                    reason: CampusDecisionReason::MandatoryDeny,
                    summary: format!(
                        "{} operations are mandatorily denied by {authority} policy",
                        approval_kind_name(kind)
                    ),
                    risk: Some(risk),
                    approval_kind: Some(kind),
                    ..self.record(CampusDecisionOutcome::Deny, subject)
                };
            }
        }

        let base = CampusDecisionRecord {
            risk: Some(risk),
            ..self.record(CampusDecisionOutcome::Allow, subject)
        };
        match decision {
            PolicyDecision::Allow { summary, .. } => CampusDecisionRecord {
                reason: CampusDecisionReason::BaselineAllow,
                summary,
                ..base
            },
            PolicyDecision::Deny { summary, .. } => CampusDecisionRecord {
                outcome: CampusDecisionOutcome::Deny,
                reason: CampusDecisionReason::BaselineDeny,
                summary,
                ..base
            },
            ask @ PolicyDecision::Ask { approval_kind, .. } => {
                let base = CampusDecisionRecord {
                    approval_kind: Some(approval_kind),
                    ..base
                };
                if !CampusApproverRole::ALL
                    .iter()
                    .any(|role| role.may_approve(approval_kind))
                {
                    return CampusDecisionRecord {
                        outcome: CampusDecisionOutcome::Deny,
                        reason: CampusDecisionReason::ApprovalUnavailable,
                        summary: format!(
                            "no Campus role may approve {} operations",
                            approval_kind_name(approval_kind)
                        ),
                        ..base
                    };
                }
                let PolicyDecision::Ask { summary, .. } = &ask else {
                    unreachable!("matched as ask")
                };
                let summary = summary.clone();
                if let PolicyDecision::Allow {
                    summary,
                    grant_id: Some(grant_id),
                    ..
                } = baseline.apply_permission_grants(action, run_scope, ask)
                {
                    return CampusDecisionRecord {
                        reason: CampusDecisionReason::PermissionGrant,
                        summary,
                        grant_id: Some(grant_id),
                        ..base
                    };
                }
                let ask = CampusDecisionRecord {
                    outcome: CampusDecisionOutcome::Ask,
                    reason: CampusDecisionReason::ApprovalRequired,
                    summary: summary.clone(),
                    ..base
                };
                let Some(approval) = approval else {
                    return ask;
                };
                if approval.action_id != *action.id() || approval.approval_kind != approval_kind {
                    return CampusDecisionRecord {
                        reason: CampusDecisionReason::ApprovalMismatch,
                        summary: format!("{summary}; approval does not match this action"),
                        ..ask
                    };
                }
                if !approval.role.may_approve(approval_kind) {
                    return CampusDecisionRecord {
                        authority: approval.role.authority().into(),
                        reason: CampusDecisionReason::ApproverRoleNotPermitted,
                        summary: format!(
                            "{summary}; {} may not approve {} operations",
                            approval.role.as_str(),
                            approval_kind_name(approval_kind)
                        ),
                        ..ask
                    };
                }
                CampusDecisionRecord {
                    outcome: CampusDecisionOutcome::Allow,
                    authority: approval.role.authority().into(),
                    reason: CampusDecisionReason::RoleApproval,
                    summary: format!("{summary}; approved by {}", approval.role.as_str()),
                    approved_by: Some(approval.role),
                    ..ask
                }
            }
        }
    }
}

/// The operation kind a mandatory deny is matched against. Writes and shell
/// commands carry a kind even when the baseline allows them, so a trusted
/// baseline mode cannot step around a mandatory deny. Command classification
/// is heuristic defence in depth, not the isolation boundary.
fn operation_kind(action: &AgentAction, decision: &PolicyDecision) -> Option<ApprovalKind> {
    if let PolicyDecision::Ask { approval_kind, .. } = decision {
        return Some(*approval_kind);
    }
    match action {
        AgentAction::WriteFile { .. }
        | AgentAction::PatchFile { .. }
        | AgentAction::ApplyPatch { .. }
        | AgentAction::CreateDirectory { .. } => Some(ApprovalKind::FileWrite),
        AgentAction::RunShell { cmd, .. } => command_kind(cmd),
        AgentAction::RunWorkspaceCommand {
            executable, args, ..
        } => command_kind(&command_line(executable, args)),
        _ => None,
    }
}

/// The command line a workspace command is classified as. Arguments are
/// joined unquoted, which is enough for the heuristic classifier.
pub(crate) fn command_line(executable: &str, args: &[String]) -> String {
    std::iter::once(executable)
        .chain(args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

fn command_kind(command: &str) -> Option<ApprovalKind> {
    match classify_command(command) {
        CommandClass::SafeReadOnly => None,
        CommandClass::WriteLocalProject | CommandClass::Unknown => Some(ApprovalKind::ShellCommand),
        CommandClass::DestructiveLocal => Some(ApprovalKind::Destructive),
        CommandClass::Network | CommandClass::EnvironmentSetup => Some(ApprovalKind::Network),
        CommandClass::Privileged => Some(ApprovalKind::Privileged),
        CommandClass::SecretAccessing => Some(ApprovalKind::SecretAccess),
    }
}

/// Who may approve an approval-required Campus operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusApproverRole {
    Instructor,
    Student,
}

impl CampusApproverRole {
    pub const ALL: [Self; 2] = [Self::Instructor, Self::Student];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Instructor => "instructor",
            Self::Student => "student",
        }
    }

    pub fn authority(self) -> CampusAuthority {
        match self {
            Self::Instructor => CampusAuthority::Assignment,
            Self::Student => CampusAuthority::Student,
        }
    }

    /// Students may approve work inside their own session workspace;
    /// instructors may additionally approve destructive local operations.
    /// Network, privileged, secret and delivery operations are approvable by
    /// no Campus role.
    pub fn may_approve(self, kind: ApprovalKind) -> bool {
        match self {
            Self::Instructor => matches!(
                kind,
                ApprovalKind::FileWrite | ApprovalKind::ShellCommand | ApprovalKind::Destructive
            ),
            Self::Student => matches!(kind, ApprovalKind::FileWrite | ApprovalKind::ShellCommand),
        }
    }
}

/// An approval recorded for one pending action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusApproval {
    pub action_id: ActionId,
    pub approval_kind: ApprovalKind,
    pub role: CampusApproverRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusDecisionOutcome {
    Allow,
    Ask,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusDecisionReason {
    /// A platform or institution mandatory deny; nothing lifts it.
    MandatoryDeny,
    /// The tool, model alias or destination is outside the effective policy.
    OutsideEffectivePolicy,
    WithinEffectivePolicy,
    BaselineAllow,
    BaselineDeny,
    ApprovalRequired,
    PermissionGrant,
    RoleApproval,
    ApprovalMismatch,
    ApproverRoleNotPermitted,
    /// No Campus role may approve this kind of operation.
    ApprovalUnavailable,
}

/// Evidence for one Campus policy decision, bound to the exact effective
/// policy and profile it was made under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CampusDecisionRecord {
    pub outcome: CampusDecisionOutcome,
    pub subject: String,
    pub authority: CampusPolicySource,
    pub reason: CampusDecisionReason,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<RiskLevel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_kind: Option<ApprovalKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approved_by: Option<CampusApproverRole>,
    pub effective_policy_hash: String,
    pub profile_hash: String,
}

impl CampusDecisionRecord {
    pub fn is_allow(&self) -> bool {
        self.outcome == CampusDecisionOutcome::Allow
    }
}

#[cfg(test)]
mod tests;
