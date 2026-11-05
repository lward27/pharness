//! The static Campus lab catalog: which assignments can be launched, under
//! which profile and policy, and how many lab slots the tenant has.
//!
//! The catalog is read once at startup from `PHARNESS_CAMPUS_LABS_FILE`.
//! Every profile and policy layer is validated and every assignment's
//! effective policy is composed while loading, so a launch takes its image,
//! resources, network, lifetime and budget from this file only, never from
//! the request.

use super::config::CampusConfig;
use anyhow::{bail, Context};
use pharness_core::campus_lab_session::kubernetes::{
    is_label_value, CampusSessionSizing, KubernetesCampusProviderConfig,
};
use pharness_core::{CampusPolicyLayer, CampusProfile, CampusProfileKind, EffectiveCampusPolicy};
use pharness_store::{CampusAssignmentPolicies, CampusLabLimits};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

pub(crate) const ENV_LABS_FILE: &str = "PHARNESS_CAMPUS_LABS_FILE";
pub(crate) const ENV_LAB_PROVIDER: &str = "PHARNESS_CAMPUS_LAB_PROVIDER";
pub(crate) const ENV_LAB_PROVIDER_CONFIG_FILE: &str = "PHARNESS_CAMPUS_LAB_PROVIDER_CONFIG_FILE";
pub(crate) const CAMPUS_LAB_CATALOG_SCHEMA: &str = "pharness.dev/campus-lab-catalog/v1alpha1";

/// Milestone 1 capacity: five concurrent lab slots per tenant.
pub(crate) const DEFAULT_LAB_SLOTS: i64 = 5;
const MAX_LAB_SLOTS: i64 = 64;
/// One live lab session per student across the whole tenant.
pub(crate) const MAX_ACTIVE_PER_IDENTITY: i64 = 1;
/// No lab session outlives two hours, whatever its profile allows.
pub(crate) const MAX_SESSION_SECONDS: i64 = 2 * 60 * 60;
const MIN_SESSION_SECONDS: i64 = 5 * 60;
const MAX_CATALOG_BYTES: u64 = 1024 * 1024;
const MAX_PROVIDER_CONFIG_BYTES: u64 = 256 * 1024;
const MAX_ASSIGNMENTS: usize = 500;
const MAX_TITLE_CHARS: usize = 200;
pub(crate) const MAX_LAB_ID_CHARS: usize = 128;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogDocument {
    schema_version: String,
    #[serde(default)]
    lab_slots: Option<i64>,
    /// Journal ids of the lab slots, which the execution provider maps to
    /// its slot namespaces. Defaults to `<tenant>-lab-slot-NN`.
    #[serde(default)]
    lab_slot_ids: Option<Vec<String>>,
    #[serde(default)]
    max_active_per_course: Option<i64>,
    #[serde(default)]
    max_session_seconds: Option<i64>,
    #[serde(default)]
    recorded_qualifications: Vec<String>,
    profiles: Vec<serde_json::Value>,
    policy_layers: Vec<serde_json::Value>,
    assignments: Vec<AssignmentDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssignmentDocument {
    course_id: String,
    assignment_id: String,
    title: String,
    profile_id: String,
    #[serde(default)]
    assignment_layer: Option<serde_json::Value>,
}

pub(crate) struct CampusLabAssignment {
    pub(crate) course_id: String,
    pub(crate) assignment_id: String,
    pub(crate) title: String,
    pub(crate) profile_kind: CampusProfileKind,
    pub(crate) policy: EffectiveCampusPolicy,
    /// The policy lifetime, capped at the catalog's maximum.
    pub(crate) session_seconds: i64,
}

pub(crate) struct CampusLabCatalog {
    pub(crate) tenant_id: String,
    pub(crate) limits: CampusLabLimits,
    /// One per lab slot, in order.
    pub(crate) slot_ids: Vec<String>,
    /// Every listed profile; an execution provider launches only these.
    pub(crate) profiles: Vec<CampusProfile>,
    assignments: BTreeMap<(String, String), CampusLabAssignment>,
}

/// Which execution provider runs lab sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CampusLabProviderKind {
    /// Lab sessions can be listed, inspected and stopped, but nothing can be
    /// launched until a provider is configured.
    None,
    /// The in-memory fake provider. It creates nothing real and is accepted
    /// only together with the loopback development flag.
    Fake,
    /// Session Jobs in admin-precreated lab slot namespaces, configured by
    /// `PHARNESS_CAMPUS_LAB_PROVIDER_CONFIG_FILE`.
    Kubernetes,
}

/// The Kubernetes provider configuration file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KubernetesLabProviderFile {
    pub(crate) kubectl: String,
    /// Arguments before every kubectl command, selecting the controller
    /// identity, e.g. `--kubeconfig=/etc/pharness/campus-controller/kubeconfig`.
    pub(crate) global_args: Vec<String>,
    pub(crate) provider: KubernetesCampusProviderConfig,
}

pub(crate) struct CampusLabsConfig {
    pub(crate) catalog: CampusLabCatalog,
    pub(crate) provider: CampusLabProviderKind,
    /// Present exactly when `provider` is `Kubernetes`.
    pub(crate) kubernetes: Option<KubernetesLabProviderFile>,
}

pub(crate) fn is_lab_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_LAB_ID_CHARS
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Load the lab catalog named by `PHARNESS_CAMPUS_LABS_FILE`. Returns
/// `Ok(None)` when no catalog is configured; any invalid entry is a startup
/// error.
pub(crate) fn load_campus_labs_from_env(
    config: &CampusConfig,
) -> anyhow::Result<Option<CampusLabsConfig>> {
    load_campus_labs(config, |name| std::env::var(name).ok())
}

pub(crate) fn load_campus_labs(
    config: &CampusConfig,
    lookup: impl Fn(&str) -> Option<String>,
) -> anyhow::Result<Option<CampusLabsConfig>> {
    let value = |name: &str| {
        lookup(name)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };
    let provider = match value(ENV_LAB_PROVIDER).as_deref() {
        None | Some("none") => CampusLabProviderKind::None,
        Some("fake") if config.allow_insecure_loopback_http => CampusLabProviderKind::Fake,
        Some("fake") => bail!(
            "{ENV_LAB_PROVIDER}=fake is only accepted for local development with PHARNESS_CAMPUS_DEV_ALLOW_INSECURE_LOOPBACK_HTTP=true"
        ),
        Some("kubernetes") => CampusLabProviderKind::Kubernetes,
        Some(other) => bail!("{ENV_LAB_PROVIDER} must be none, fake or kubernetes, not {other}"),
    };
    let provider_file = value(ENV_LAB_PROVIDER_CONFIG_FILE);
    if provider_file.is_some() != (provider == CampusLabProviderKind::Kubernetes) {
        bail!("{ENV_LAB_PROVIDER_CONFIG_FILE} is required with, and only with, {ENV_LAB_PROVIDER}=kubernetes");
    }
    let Some(path) = value(ENV_LABS_FILE) else {
        if provider != CampusLabProviderKind::None {
            bail!("{ENV_LAB_PROVIDER} requires {ENV_LABS_FILE}");
        }
        return Ok(None);
    };
    let document = read_bounded(ENV_LABS_FILE, &path, MAX_CATALOG_BYTES)?;
    let catalog = CampusLabCatalog::parse(&config.tenant_id, &document)
        .with_context(|| format!("{ENV_LABS_FILE} at {path} is invalid"))?;
    let kubernetes = match provider_file {
        Some(path) => {
            let document = read_bounded(
                ENV_LAB_PROVIDER_CONFIG_FILE,
                &path,
                MAX_PROVIDER_CONFIG_BYTES,
            )?;
            let file = KubernetesLabProviderFile::parse(&document, &catalog)
                .with_context(|| format!("{ENV_LAB_PROVIDER_CONFIG_FILE} at {path} is invalid"))?;
            Some(file)
        }
        None => None,
    };
    Ok(Some(CampusLabsConfig {
        catalog,
        provider,
        kubernetes,
    }))
}

fn read_bounded(name: &str, path: &str, max_bytes: u64) -> anyhow::Result<String> {
    let mut document = String::new();
    std::fs::File::open(path)
        .with_context(|| format!("failed to open {name} at {path}"))?
        .take(max_bytes + 1)
        .read_to_string(&mut document)
        .with_context(|| format!("failed to read {name} at {path}"))?;
    if document.len() as u64 > max_bytes {
        bail!("{name} at {path} is larger than {max_bytes} bytes");
    }
    Ok(document)
}

impl KubernetesLabProviderFile {
    /// Parse and check the file against the catalog: the provider must map
    /// exactly the catalog's lab slots, and every assignment must fit a
    /// slot, so a misconfiguration fails at startup rather than at launch.
    pub(crate) fn parse(document: &str, catalog: &CampusLabCatalog) -> anyhow::Result<Self> {
        let file: Self = serde_json::from_str(document)
            .context("the provider configuration is not valid JSON")?;
        if file.kubectl.trim().is_empty() {
            bail!("kubectl is required");
        }
        file.provider.validate()?;
        let configured = file.provider.slots.keys().collect::<BTreeSet<_>>();
        let listed = catalog.slot_ids.iter().collect::<BTreeSet<_>>();
        if configured != listed {
            bail!("provider.slots must map exactly the catalog's lab slot ids");
        }
        for assignment in catalog.assignments.values() {
            CampusSessionSizing::derive(assignment.policy.constraints(), &file.provider)
                .with_context(|| {
                    format!(
                        "assignment {}/{} does not fit a lab slot",
                        assignment.course_id, assignment.assignment_id
                    )
                })?;
        }
        Ok(file)
    }
}

fn bounded(
    name: &str,
    value: Option<i64>,
    default: i64,
    min: i64,
    max: i64,
) -> anyhow::Result<i64> {
    let value = value.unwrap_or(default);
    if !(min..=max).contains(&value) {
        bail!("{name} must be between {min} and {max}");
    }
    Ok(value)
}

impl CampusLabCatalog {
    pub(crate) fn parse(tenant_id: &str, document: &str) -> anyhow::Result<Self> {
        let document: CatalogDocument =
            serde_json::from_str(document).context("the catalog is not valid JSON")?;
        if document.schema_version != CAMPUS_LAB_CATALOG_SCHEMA {
            bail!("schema_version must be {CAMPUS_LAB_CATALOG_SCHEMA}");
        }
        let listed_slots = document
            .lab_slot_ids
            .as_ref()
            .map(|ids| i64::try_from(ids.len()).unwrap_or(i64::MAX));
        let lab_slots = bounded(
            "lab_slots",
            document.lab_slots.or(listed_slots),
            DEFAULT_LAB_SLOTS,
            1,
            MAX_LAB_SLOTS,
        )?;
        let slot_ids = match document.lab_slot_ids {
            Some(ids) => {
                if listed_slots != Some(lab_slots) {
                    bail!("lab_slots must equal the number of lab_slot_ids");
                }
                let unique = ids.iter().collect::<BTreeSet<_>>().len() == ids.len();
                if !unique || !ids.iter().all(|id| is_label_value(id) && !id.is_empty()) {
                    bail!("lab_slot_ids must be distinct, non-empty Kubernetes label values");
                }
                ids
            }
            None => (1..=lab_slots)
                .map(|index| format!("{tenant_id}-lab-slot-{index:02}"))
                .collect(),
        };
        let max_active_per_course = bounded(
            "max_active_per_course",
            document.max_active_per_course,
            lab_slots,
            1,
            lab_slots,
        )?;
        let max_session_seconds = bounded(
            "max_session_seconds",
            document.max_session_seconds,
            MAX_SESSION_SECONDS,
            MIN_SESSION_SECONDS,
            MAX_SESSION_SECONDS,
        )?;
        let qualifications = document
            .recorded_qualifications
            .into_iter()
            .collect::<BTreeSet<_>>();

        let mut profiles = BTreeMap::new();
        for (index, profile) in document.profiles.iter().enumerate() {
            let profile = CampusProfile::parse(&profile.to_string())
                .with_context(|| format!("profiles[{index}] is invalid"))?;
            if profiles.insert(profile.id.clone(), profile).is_some() {
                bail!("profiles[{index}] repeats a profile id");
            }
        }
        let shared_layers = document
            .policy_layers
            .iter()
            .enumerate()
            .map(|(index, layer)| {
                CampusPolicyLayer::parse(&layer.to_string())
                    .with_context(|| format!("policy_layers[{index}] is invalid"))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        if document.assignments.len() > MAX_ASSIGNMENTS {
            bail!("at most {MAX_ASSIGNMENTS} assignments may be listed");
        }
        let mut assignments = BTreeMap::new();
        for (index, entry) in document.assignments.into_iter().enumerate() {
            let context = || format!("assignments[{index}] is invalid");
            if !is_lab_identifier(&entry.course_id) || !is_lab_identifier(&entry.assignment_id) {
                return Err(anyhow::anyhow!(
                    "course_id and assignment_id must be 1-128 characters of [A-Za-z0-9._-]"
                ))
                .with_context(context);
            }
            let title = entry.title.trim().to_string();
            if title.is_empty()
                || title.chars().count() > MAX_TITLE_CHARS
                || title.chars().any(char::is_control)
            {
                return Err(anyhow::anyhow!(
                    "title must be 1-{MAX_TITLE_CHARS} printable characters"
                ))
                .with_context(context);
            }
            let profile = profiles
                .get(&entry.profile_id)
                .ok_or_else(|| anyhow::anyhow!("profile {} is not listed", entry.profile_id))
                .with_context(context)?;
            let mut layers = shared_layers.clone();
            if let Some(layer) = &entry.assignment_layer {
                layers.push(CampusPolicyLayer::parse(&layer.to_string()).with_context(context)?);
            }
            let policy = EffectiveCampusPolicy::compose(profile, &layers, &qualifications)
                .with_context(context)?;
            let session_seconds =
                i64::from(policy.constraints().ttl_seconds).min(max_session_seconds);
            let key = (entry.course_id.clone(), entry.assignment_id.clone());
            let assignment = CampusLabAssignment {
                course_id: entry.course_id,
                assignment_id: entry.assignment_id,
                title,
                profile_kind: profile.kind,
                policy,
                session_seconds,
            };
            if assignments.insert(key, assignment).is_some() {
                bail!("assignments[{index}] repeats a course and assignment id");
            }
        }
        Ok(Self {
            tenant_id: tenant_id.to_string(),
            limits: CampusLabLimits {
                lab_slots,
                max_active_per_identity: MAX_ACTIVE_PER_IDENTITY,
                max_active_per_course,
            },
            slot_ids,
            profiles: profiles.into_values().collect(),
            assignments,
        })
    }

    pub(crate) fn assignment(
        &self,
        course_id: &str,
        assignment_id: &str,
    ) -> Option<&CampusLabAssignment> {
        self.assignments
            .get(&(course_id.to_string(), assignment_id.to_string()))
    }

    pub(crate) fn course_assignments<'a>(
        &'a self,
        course_id: &'a str,
    ) -> impl Iterator<Item = &'a CampusLabAssignment> + 'a {
        self.assignments
            .values()
            .filter(move |assignment| assignment.course_id == course_id)
    }
}

impl CampusAssignmentPolicies for CampusLabCatalog {
    fn effective_policy(
        &self,
        tenant_id: &str,
        course_id: &str,
        assignment_id: &str,
    ) -> Option<EffectiveCampusPolicy> {
        if tenant_id != self.tenant_id {
            return None;
        }
        self.assignment(course_id, assignment_id)
            .map(|assignment| assignment.policy.clone())
    }
}
