//! Kubernetes execution provider for Campus lab sessions.
//!
//! Each session occupancy runs as one Job in an admin-precreated slot
//! namespace (`charts/campus-lab` in the GitOps repository). The slots carry
//! the guardrails the controller cannot change: Pod Security `restricted`, a
//! ResourceQuota and LimitRange, a default-deny NetworkPolicy plus the
//! policies that open only the broker and the Campus egress proxy, a default
//! ServiceAccount without a token, the LUC-121 image allowlist and the
//! independent `pharness-campus-lab-session-guard` admission policy. The
//! controller's RBAC is limited to Jobs, Pods, ConfigMaps and the session
//! token Secrets in the slots, read access to the slot NetworkPolicies and the
//! qualified RuntimeClass.
//!
//! Network: a launch is refused unless the slot's admin-owned NetworkPolicies
//! exist ([`KubernetesCampusProviderConfig::required_network_policies`]), and
//! a session with approved egress is refused unless every destination is an
//! approved Campus egress route and the egress proxy is configured. Every
//! session Pod starts behind a network gate init container that holds the
//! workspace back until the policies are in force for the Pod.
//!
//! Broker token: each occupancy gets a scoped LUC-131 inference token in a
//! Secret owned by the anchor, mounted read-only into the workspace container
//! only. A repeated provision replaces it once it is due for rotation.
//!
//! Fencing: every occupancy has an anchor ConfigMap with a deterministic name.
//! The Job is owned by the anchor. `delete` replaces the anchor with a
//! tombstone of the same name, so no new anchor and therefore no new Job can
//! be created for that occupancy, and the garbage collector removes any Job a
//! stale worker creates against the deleted anchor. A provision also refuses
//! when a newer slot generation has an anchor or tombstone.

mod kube_api;
mod template;

#[cfg(test)]
mod fake_kube;
#[cfg(test)]
mod tests;

pub use kube_api::{
    classify_kubectl_error, delete_args, get_args, list_args, CampusKubeApi, CampusKubeError,
    CampusKubeKind, KubectlCampusKubeApi,
};
pub use template::{
    approved_egress_hosts, broker_token_labels, check_launchable_in_slot, is_label_value,
    network_gate_command, op_label, record_selector, render_anchor, render_session_job,
    render_tombstone, session_command, session_resource_name, sha256_hex, workload_selector,
    CampusJobTemplateInput, CampusSessionSizing, CampusTemplateError, NETWORK_GATE_CONTAINER,
    SESSION_CONTAINER, SESSION_UID, TMP_MOUNT, WORKSPACE_MOUNT,
};

use super::{
    CampusDeleteOutcome, CampusExecutionProvider, CampusObservation, CampusProviderError,
    CampusProvisionOutcome, CampusProvisionSpec, CampusWorkloadRef, OwnedCampusResource,
};
use crate::campus_broker_token::{CampusBrokerTokenError, CampusBrokerTokenIssuer};
use crate::campus_capability::{
    check_sandbox_environment, CampusAuthoritySource, CampusAuthorityStatus, CampusBrokerClock,
};
use crate::campus_sandbox_pod::{
    CampusSandboxBrokerBinding, CampusSandboxEgress, CAMPUS_BROKER_TOKEN_ROTATE_AT_ANNOTATION,
    CAMPUS_SANDBOX_AUDIENCE,
};
use crate::{canonical_json_sha256, CampusProfile};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use url::Url;

pub const LABEL_CONTROLLER: &str = "pharness.dev/campus-controller";
pub const LABEL_TENANT: &str = "pharness.dev/campus-tenant";
pub const LABEL_SESSION: &str = "pharness.dev/campus-session";
pub const LABEL_SLOT: &str = "pharness.dev/campus-slot";
pub const LABEL_GENERATION: &str = "pharness.dev/campus-generation";
/// `anchor`, `workload` or `tombstone`.
pub const LABEL_RECORD: &str = "pharness.dev/campus-record";
/// A digest of the provision operation key; the full key is an annotation.
pub const LABEL_OP: &str = "pharness.dev/campus-op";
pub const LABEL_PROFILE: &str = "pharness.dev/campus-profile";
pub const ANNOTATION_OP_KEY: &str = "pharness.dev/campus-op-key";
pub const ANNOTATION_SPEC_SHA256: &str = "pharness.dev/campus-spec-sha256";
pub const ANNOTATION_PROFILE_HASH: &str = "pharness.dev/campus-profile-hash";
pub const ANNOTATION_POLICY_HASH: &str = "pharness.dev/campus-effective-policy-hash";
pub const ANNOTATION_COURSE: &str = "pharness.dev/campus-course";
pub const ANNOTATION_EXPIRES_AT: &str = "pharness.dev/campus-expires-at";
pub const RECORD_ANCHOR: &str = "anchor";
pub const RECORD_WORKLOAD: &str = "workload";
pub const RECORD_TOMBSTONE: &str = "tombstone";
/// The session's broker token Secret.
pub const RECORD_BROKER_TOKEN: &str = "broker-token";
/// The slot policy every launch requires: deny all ingress and egress.
pub const DEFAULT_DENY_NETWORK_POLICY: &str = "campus-lab-default-deny";

/// Container waiting reasons that will not resolve on their own.
const FATAL_WAITING_REASONS: &[&str] = &[
    "ErrImagePull",
    "ImagePullBackOff",
    "InvalidImageName",
    "CreateContainerConfigError",
    "CreateContainerError",
    "RunContainerError",
    "CrashLoopBackOff",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusRuntimeOverhead {
    pub cpu_millicores: u32,
    pub memory_mib: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusRuntimeConfig {
    /// The only RuntimeClass session Pods may use. There is no fallback.
    pub class_name: String,
    pub handler: String,
    /// The dedicated pool; must equal the RuntimeClass scheduling selector.
    pub node_selector: BTreeMap<String, String>,
    /// Must equal the RuntimeClass `overhead.podFixed`, which the API server
    /// adds to every Pod's quota usage.
    pub overhead: CampusRuntimeOverhead,
}

/// The Campus egress proxy, as session Pods reach it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusEgressProxyConfig {
    /// `http://<address>:<port>`; an address, so the session needs no DNS.
    pub proxy_url: Url,
    /// The public certificate of the Campus egress CA.
    pub ca_certificate_pem: String,
    /// The slot NetworkPolicy that admits egress-labelled Pods to the proxy.
    /// A session with approved egress is refused when it is missing.
    pub network_policy: String,
}

/// The startup network gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusNetworkGateConfig {
    /// `address:port` destinations the slot policy denies and that always
    /// accept connections otherwise, such as the API server's ClusterIP.
    pub canaries: Vec<String>,
    pub timeout_seconds: u32,
}

/// Mirrors the slot ResourceQuota and LimitRange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusSlotCapacity {
    pub cpu_millicores: u32,
    pub memory_mib: u32,
    pub ephemeral_storage_mib: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KubernetesCampusProviderConfig {
    /// Value of `pharness.dev/campus-controller`; the admission guard requires
    /// it.
    pub controller: String,
    /// Lab slot id (as in the journal) to its admin-precreated namespace.
    pub slots: BTreeMap<String, String>,
    pub runtime: CampusRuntimeConfig,
    pub slot_capacity: CampusSlotCapacity,
    /// What PID 1 execs after the process limit is set. It must keep running
    /// for the life of the session.
    pub workspace_command: Vec<String>,
    /// Extra, non-secret environment for the session container.
    pub environment: BTreeMap<String, String>,
    /// Container ephemeral-storage limit. The root filesystem is read-only
    /// and the volumes are memory-backed, so this only covers logs.
    pub log_storage_mib: u32,
    pub finished_job_ttl_seconds: u32,
    /// A Pod still terminating after this long is reported as unreachable.
    pub stuck_termination_seconds: i64,
    /// Where session Pods reach the Campus broker. Not loopback: the broker
    /// is never inside the Pod.
    pub broker_url: Url,
    /// Slot NetworkPolicies that must exist before any launch, starting with
    /// the default deny.
    pub required_network_policies: Vec<String>,
    /// Without it, sessions with approved egress are refused.
    #[serde(default)]
    pub egress: Option<CampusEgressProxyConfig>,
    pub network_gate: CampusNetworkGateConfig,
}

impl KubernetesCampusProviderConfig {
    /// The M1 lab: five slots on the gVisor node, matching `charts/campus-lab`.
    pub fn m1_lab() -> Self {
        Self {
            controller: "pharness-campus-controller".into(),
            slots: (1..=5)
                .map(|index| {
                    let name = format!("campus-lab-slot-{index}");
                    (name.clone(), name)
                })
                .collect(),
            runtime: CampusRuntimeConfig {
                class_name: "gvisor".into(),
                handler: "runsc".into(),
                node_selector: BTreeMap::from([(
                    "pharness.dev/campus-lab-runtime".into(),
                    "gvisor".into(),
                )]),
                overhead: CampusRuntimeOverhead {
                    cpu_millicores: 50,
                    memory_mib: 128,
                },
            },
            slot_capacity: CampusSlotCapacity {
                cpu_millicores: 2_000,
                memory_mib: 3_072,
                ephemeral_storage_mib: 256,
            },
            workspace_command: vec!["sleep".into(), "2147483647".into()],
            environment: BTreeMap::new(),
            log_storage_mib: 128,
            finished_job_ttl_seconds: 300,
            stuck_termination_seconds: 120,
            // The pinned ClusterIP of the broker Service, so the session needs
            // no DNS.
            broker_url: Url::parse("http://10.43.130.31:4790/").expect("static URL"),
            required_network_policies: vec![
                DEFAULT_DENY_NETWORK_POLICY.into(),
                "campus-lab-session-broker".into(),
            ],
            egress: None,
            network_gate: CampusNetworkGateConfig {
                canaries: vec!["10.43.0.1:443".into(), "1.1.1.1:443".into()],
                timeout_seconds: 60,
            },
        }
    }

    /// The egress proxy binding for session Pods, when one is configured.
    pub fn sandbox_egress(&self) -> Result<Option<CampusSandboxEgress>, CampusProviderConfigError> {
        self.egress
            .as_ref()
            .map(|egress| {
                CampusSandboxEgress::new(&egress.proxy_url, &egress.ca_certificate_pem)
                    .map_err(|error| CampusProviderConfigError(error.to_string()))
            })
            .transpose()
    }

    pub fn validate(&self) -> Result<(), CampusProviderConfigError> {
        let invalid = |reason: String| Err(CampusProviderConfigError(reason));
        if !is_label_value(&self.controller) {
            return invalid(format!(
                "controller {:?} is not a label value",
                self.controller
            ));
        }
        if self.slots.is_empty() {
            return invalid("at least one lab slot is required".into());
        }
        let mut namespaces = BTreeSet::new();
        for (slot, namespace) in &self.slots {
            if !is_label_value(slot) {
                return invalid(format!("slot id {slot:?} is not a label value"));
            }
            if !is_dns_label(namespace) || !namespaces.insert(namespace) {
                return invalid(format!(
                    "slot namespace {namespace:?} is invalid or repeated"
                ));
            }
        }
        if !is_dns_label(&self.runtime.class_name) || self.runtime.handler.is_empty() {
            return invalid("runtime class name and handler are required".into());
        }
        if self.runtime.node_selector.is_empty()
            || !self
                .runtime
                .node_selector
                .values()
                .all(|value| is_label_value(value))
        {
            return invalid("the runtime node selector must name the dedicated pool".into());
        }
        if self.workspace_command.is_empty() || self.workspace_command.iter().any(String::is_empty)
        {
            return invalid("workspace_command is required".into());
        }
        for reserved in ["HOME", "TMPDIR", "LANG"] {
            if self.environment.contains_key(reserved) {
                return invalid(format!("environment must not override {reserved}"));
            }
        }
        check_sandbox_environment(
            self.environment
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
        )
        .map_err(|error| CampusProviderConfigError(error.to_string()))?;
        let capacity = &self.slot_capacity;
        if capacity.cpu_millicores == 0
            || capacity.memory_mib == 0
            || self.log_storage_mib == 0
            || self.log_storage_mib > capacity.ephemeral_storage_mib
        {
            return invalid("slot capacity and log storage must be positive and consistent".into());
        }
        if self.stuck_termination_seconds < 1 {
            return invalid("stuck_termination_seconds must be positive".into());
        }
        let probe = crate::CampusWorkloadRef {
            session_id: "probe".into(),
            slot_id: "probe".into(),
            slot_generation: 1,
        };
        CampusSandboxBrokerBinding::new(&self.broker_url, &probe)
            .map_err(|error| CampusProviderConfigError(error.to_string()))?;
        if !self
            .required_network_policies
            .iter()
            .any(|name| name == DEFAULT_DENY_NETWORK_POLICY)
            || !self
                .required_network_policies
                .iter()
                .chain(self.egress.iter().map(|egress| &egress.network_policy))
                .all(|name| is_dns_label(name))
        {
            return invalid(format!(
                "required_network_policies must include {DEFAULT_DENY_NETWORK_POLICY} and name \
                 only valid policies"
            ));
        }
        self.sandbox_egress()?;
        let gate = &self.network_gate;
        let canaries_valid = !gate.canaries.is_empty()
            && gate
                .canaries
                .iter()
                .all(|canary| canary.parse::<std::net::SocketAddr>().is_ok());
        if !canaries_valid || !(5..=600).contains(&gate.timeout_seconds) {
            return invalid(
                "the network gate needs address:port canaries and a 5 to 600 second timeout".into(),
            );
        }
        Ok(())
    }
}

fn is_dns_label(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid Kubernetes Campus provider configuration: {0}")]
pub struct CampusProviderConfigError(pub String);

/// The real execution provider: admission-controlled Jobs in lab slots.
pub struct KubernetesCampusProvider {
    config: KubernetesCampusProviderConfig,
    egress: Option<CampusSandboxEgress>,
    profiles: BTreeMap<(String, String), CampusProfile>,
    authority: Arc<dyn CampusAuthoritySource>,
    issuer: Arc<dyn CampusBrokerTokenIssuer>,
    api: Arc<dyn CampusKubeApi>,
    clock: Arc<dyn CampusBrokerClock>,
}

/// What a provision is about to create, fully rendered.
struct Prepared {
    namespace: String,
    name: String,
    tenant_id: String,
    anchor: Value,
    job: Value,
    binding: CampusSandboxBrokerBinding,
}

type Refusal = String;

impl KubernetesCampusProvider {
    /// `profiles` are the approved profile documents; a session may only
    /// launch a profile listed here whose hash matches exactly.
    pub fn new(
        config: KubernetesCampusProviderConfig,
        profiles: impl IntoIterator<Item = CampusProfile>,
        authority: Arc<dyn CampusAuthoritySource>,
        issuer: Arc<dyn CampusBrokerTokenIssuer>,
        api: Arc<dyn CampusKubeApi>,
        clock: Arc<dyn CampusBrokerClock>,
    ) -> Result<Self, CampusProviderConfigError> {
        config.validate()?;
        let egress = config.sandbox_egress()?;
        let mut catalog = BTreeMap::new();
        for profile in profiles {
            profile
                .validate()
                .map_err(|error| CampusProviderConfigError(error.to_string()))?;
            let key = (profile.id.clone(), profile.version.clone());
            if catalog.insert(key, profile).is_some() {
                return Err(CampusProviderConfigError(
                    "duplicate profile version".into(),
                ));
            }
        }
        Ok(Self {
            config,
            egress,
            profiles: catalog,
            authority,
            issuer,
            api,
            clock,
        })
    }

    pub fn config(&self) -> &KubernetesCampusProviderConfig {
        &self.config
    }

    fn namespace(&self, workload: &CampusWorkloadRef) -> Result<&str, Refusal> {
        self.config
            .slots
            .get(&workload.slot_id)
            .map(String::as_str)
            .ok_or_else(|| format!("slot {} is not a configured lab slot", workload.slot_id))
    }

    async fn prepare(
        &self,
        op_key: &str,
        spec: &CampusProvisionSpec,
    ) -> Result<Result<Prepared, Refusal>, CampusProviderError> {
        let namespace = match self.namespace(&spec.workload) {
            Ok(namespace) => namespace.to_string(),
            Err(reason) => return Ok(Err(reason)),
        };
        let Some(snapshot) = self
            .authority
            .session(&spec.workload.session_id)
            .await
            .map_err(|error| CampusProviderError::Unreachable(error.to_string()))?
        else {
            return Ok(Err(
                "no trusted authority is recorded for the session".into()
            ));
        };
        if !matches!(
            snapshot.status,
            CampusAuthorityStatus::Provisioning | CampusAuthorityStatus::Active
        ) {
            return Ok(Err("the session authority is revoked".into()));
        }
        let authority = snapshot.authority;
        if authority.owner.tenant_id != spec.tenant_id || authority.course_id != spec.course_id {
            return Ok(Err(
                "the spec's tenant or course differs from the session authority".into(),
            ));
        }
        if authority.policy.profile() != &spec.profile {
            return Ok(Err(
                "the spec's profile differs from the effective policy".into()
            ));
        }
        let Some(profile) = self
            .profiles
            .get(&(spec.profile.id.clone(), spec.profile.version.clone()))
            .filter(|profile| profile.profile_hash() == spec.profile.profile_hash)
        else {
            return Ok(Err(format!(
                "profile {} {} is not approved for the lab slots",
                spec.profile.id, spec.profile.version
            )));
        };
        let constraints = authority.policy.constraints();
        let now = self.clock.now();
        let expires_at = spec.expires_at.min(authority.expires_at);
        let deadline = (expires_at - now)
            .min(i64::from(constraints.ttl_seconds))
            .min(i64::from(crate::campus_profile::MAX_TTL_SECONDS));
        if deadline < 1 {
            return Ok(Err("the session has expired".into()));
        }
        let spec_sha256 = serde_json::to_value(spec)
            .and_then(|value| canonical_json_sha256(&value))
            .map_err(|error| CampusProviderError::Internal(error.to_string()))?;
        let network = authority.sandbox_network();
        let binding = match CampusSandboxBrokerBinding::new(&self.config.broker_url, &spec.workload)
        {
            Ok(binding) => binding,
            Err(error) => return Ok(Err(error.to_string())),
        };
        let binding = match approved_egress_hosts(&network) {
            Ok(hosts) if hosts.is_empty() => binding,
            Ok(hosts) => match &self.egress {
                Some(egress) => binding.with_egress(egress.clone()),
                None => {
                    return Ok(Err(
                        CampusTemplateError::ApprovedEgress(hosts.len()).to_string()
                    ))
                }
            },
            Err(error) => return Ok(Err(error.to_string())),
        };
        let input = CampusJobTemplateInput {
            config: &self.config,
            namespace: &namespace,
            workload: &spec.workload,
            tenant_id: &spec.tenant_id,
            course_id: &spec.course_id,
            op_key,
            spec_sha256: &spec_sha256,
            profile,
            constraints,
            network: &network,
            binding: &binding,
            effective_policy_hash: authority.policy.effective_policy_hash(),
            expires_at,
            active_deadline_seconds: deadline,
        };
        let rendered = render_session_job(&input).and_then(|job| {
            render_anchor(
                &self.config.controller,
                &namespace,
                &spec.tenant_id,
                &spec.workload,
                op_key,
                &spec_sha256,
            )
            .map(|anchor| (anchor, job))
        });
        Ok(rendered
            .map(|(anchor, job)| Prepared {
                name: session_resource_name(&spec.workload),
                namespace,
                tenant_id: spec.tenant_id.clone(),
                anchor,
                job,
                binding,
            })
            .map_err(|error| error.to_string()))
    }

    /// Refuse a launch into a slot whose admin-owned network policies are
    /// missing: without them the session would not be isolated.
    async fn check_network_enforcement(
        &self,
        namespace: &str,
        egress: bool,
    ) -> Result<Result<(), Refusal>, CampusProviderError> {
        let egress_policy = self
            .config
            .egress
            .as_ref()
            .filter(|_| egress)
            .map(|egress| &egress.network_policy);
        for name in self
            .config
            .required_network_policies
            .iter()
            .chain(egress_policy)
        {
            let policy = match self
                .api
                .get(CampusKubeKind::NetworkPolicy, namespace, name)
                .await
            {
                Ok(policy) => policy,
                Err(CampusKubeError::Rejected(message)) => {
                    return Ok(Err(format!(
                        "cannot read NetworkPolicy {name} in {namespace}: {message}"
                    )))
                }
                Err(error) => return Err(provider_error(error)),
            };
            let Some(policy) = policy else {
                return Ok(Err(format!(
                    "slot network enforcement is missing: NetworkPolicy {name} does not exist \
                     in {namespace}"
                )));
            };
            if name == DEFAULT_DENY_NETWORK_POLICY && !is_default_deny(&policy) {
                return Ok(Err(format!(
                    "NetworkPolicy {name} in {namespace} no longer denies all ingress and egress"
                )));
            }
        }
        Ok(Ok(()))
    }

    /// Deliver the occupancy's broker token: create its Secret, or replace it
    /// once the token is due for rotation. Returns whether a new token was
    /// written.
    async fn ensure_broker_token(
        &self,
        namespace: &str,
        op_key: &str,
        tenant_id: &str,
        workload: &CampusWorkloadRef,
        binding: &CampusSandboxBrokerBinding,
        anchor_uid: &str,
    ) -> Result<Result<bool, Refusal>, CampusProviderError> {
        let name = binding.secret_name();
        let existing = self
            .api
            .get(CampusKubeKind::Secret, namespace, name)
            .await
            .map_err(provider_error)?;
        let now = self.clock.now();
        if let Some(existing) = &existing {
            let owned = existing
                .pointer("/metadata/ownerReferences/0/uid")
                .and_then(Value::as_str)
                == Some(anchor_uid);
            if !owned || workload_of(existing).as_ref() != Some(workload) {
                return Ok(Err(
                    "a Secret that is not this occupancy's holds the token name".into(),
                ));
            }
            let due = annotation(existing, CAMPUS_BROKER_TOKEN_ROTATE_AT_ANNOTATION)
                .and_then(|value| value.parse::<i64>().ok())
                .map_or(true, |rotate_at| now >= rotate_at);
            if !due {
                return Ok(Ok(false));
            }
        }
        let token = match self.issuer.issue(workload, CAMPUS_SANDBOX_AUDIENCE).await {
            Ok(token) => token,
            Err(CampusBrokerTokenError::NotIssuable(reason)) => {
                return Ok(Err(format!("no broker token can be issued: {reason}")))
            }
            Err(CampusBrokerTokenError::Unavailable(reason)) => {
                return Err(CampusProviderError::Unreachable(reason))
            }
        };
        let labels = match broker_token_labels(&self.config.controller, tenant_id, workload) {
            Ok(labels) => labels,
            Err(error) => return Ok(Err(error.to_string())),
        };
        let mut manifest = binding
            .secret_manifest(namespace, &labels, &token)
            .map_err(|error| CampusProviderError::Internal(error.to_string()))?;
        manifest["metadata"]["annotations"][ANNOTATION_OP_KEY] = op_key.into();
        manifest["metadata"]["ownerReferences"] = json!([{
            "apiVersion": "v1",
            "kind": "ConfigMap",
            "name": session_resource_name(workload),
            "uid": anchor_uid,
            "blockOwnerDeletion": false,
        }]);
        let mut current = existing;
        for _ in 0..3 {
            let result = match &current {
                None => self.api.create(&manifest).await,
                Some(stored) => {
                    let mut replacement = manifest.clone();
                    replacement["metadata"]["resourceVersion"] = stored
                        .pointer("/metadata/resourceVersion")
                        .cloned()
                        .unwrap_or(Value::Null);
                    self.api.replace(&replacement).await
                }
            };
            match result {
                Ok(_) => return Ok(Ok(true)),
                // A concurrent provision wrote first. This token is the newest,
                // so replace theirs rather than keep a token the issuer may
                // already have revoked.
                Err(CampusKubeError::AlreadyExists(_) | CampusKubeError::Failed(_)) => {
                    current = self
                        .api
                        .get(CampusKubeKind::Secret, namespace, name)
                        .await
                        .map_err(provider_error)?;
                }
                Err(CampusKubeError::Rejected(message)) => {
                    return Ok(Err(format!(
                        "the slot refused the broker token Secret: {message}"
                    )))
                }
                Err(error) => return Err(provider_error(error)),
            }
        }
        Err(CampusProviderError::Internal(
            "the broker token Secret kept changing".into(),
        ))
    }

    /// Rotate a running occupancy's broker token when it is due. Returns
    /// whether a new token was delivered. The kubelet refreshes the mounted
    /// file in place; the Pod reads it on every request.
    pub async fn rotate_broker_token(
        &self,
        workload: &CampusWorkloadRef,
    ) -> Result<bool, CampusProviderError> {
        let namespace = self
            .namespace(workload)
            .map_err(CampusProviderError::Internal)?;
        let name = session_resource_name(workload);
        let anchor = self
            .api
            .get(CampusKubeKind::ConfigMap, namespace, &name)
            .await
            .map_err(provider_error)?
            .filter(|anchor| anchor_state(anchor) == Some("active"))
            .ok_or_else(|| CampusProviderError::Internal("the occupancy is not active".into()))?;
        let job = self
            .api
            .get(CampusKubeKind::Job, namespace, &name)
            .await
            .map_err(provider_error)?
            .ok_or_else(|| CampusProviderError::Internal("the occupancy has no Job".into()))?;
        let (Some(anchor_uid), Some(tenant_id), Some(op_key)) = (
            anchor.pointer("/metadata/uid").and_then(Value::as_str),
            label(&anchor, LABEL_TENANT),
            annotation(&anchor, ANNOTATION_OP_KEY),
        ) else {
            return Err(CampusProviderError::Internal(
                "the anchor has no uid or tenant".into(),
            ));
        };
        let binding = CampusSandboxBrokerBinding::new(&self.config.broker_url, workload)
            .map_err(|error| CampusProviderError::Internal(error.to_string()))?;
        let has_egress = job
            .pointer("/spec/template/metadata/labels")
            .and_then(|labels| labels.get(crate::campus_egress::CAMPUS_EGRESS_LABEL))
            .is_some();
        let binding = match (&self.egress, has_egress) {
            (Some(egress), true) => binding.with_egress(egress.clone()),
            (None, true) => {
                return Err(CampusProviderError::Internal(
                    "the session has egress but no egress proxy is configured".into(),
                ))
            }
            (_, false) => binding,
        };
        self.ensure_broker_token(namespace, op_key, tenant_id, workload, &binding, anchor_uid)
            .await?
            .map_err(CampusProviderError::Internal)
    }

    async fn check_runtime(&self) -> Result<Result<(), Refusal>, CampusProviderError> {
        let runtime = &self.config.runtime;
        let found = match self
            .api
            .get(CampusKubeKind::RuntimeClass, "", &runtime.class_name)
            .await
        {
            Ok(found) => found,
            Err(CampusKubeError::Rejected(message)) => {
                return Ok(Err(format!(
                    "cannot read RuntimeClass {}: {message}",
                    runtime.class_name
                )))
            }
            Err(error) => return Err(provider_error(error)),
        };
        let Some(class) = found else {
            return Ok(Err(format!(
                "RuntimeClass {} is not installed; refusing to schedule without it",
                runtime.class_name
            )));
        };
        let handler = class.get("handler").and_then(Value::as_str);
        let pool_matches = runtime.node_selector.iter().all(|(key, value)| {
            class
                .pointer("/scheduling/nodeSelector")
                .and_then(|selector| selector.get(key))
                .and_then(Value::as_str)
                == Some(value.as_str())
        });
        if handler != Some(runtime.handler.as_str()) || !pool_matches {
            return Ok(Err(format!(
                "RuntimeClass {} does not use handler {} on the qualified pool",
                runtime.class_name, runtime.handler
            )));
        }
        Ok(Ok(()))
    }

    /// Refuse a stale occupancy and an operation key reused for another
    /// workload.
    async fn check_fences(
        &self,
        namespace: &str,
        op_key: &str,
        spec: &CampusProvisionSpec,
    ) -> Result<Result<(), Refusal>, CampusProviderError> {
        let controller = &self.config.controller;
        let records = self
            .api
            .list(
                CampusKubeKind::ConfigMap,
                namespace,
                &format!(
                    "{LABEL_CONTROLLER}={controller},{LABEL_RECORD} in ({RECORD_ANCHOR},{RECORD_TOMBSTONE})"
                ),
            )
            .await
            .map_err(provider_error)?;
        if records.iter().filter_map(workload_of).any(|other| {
            other.slot_id == spec.workload.slot_id
                && other.slot_generation > spec.workload.slot_generation
        }) {
            return Ok(Err("a newer occupant of the lab slot exists".into()));
        }
        let op = op_label(op_key);
        let name = session_resource_name(&spec.workload);
        for slot_namespace in self.config.slots.values() {
            let anchors = self
                .api
                .list(
                    CampusKubeKind::ConfigMap,
                    slot_namespace,
                    &format!("{LABEL_CONTROLLER}={controller},{LABEL_RECORD}={RECORD_ANCHOR},{LABEL_OP}={op}"),
                )
                .await
                .map_err(provider_error)?;
            if anchors.iter().any(|anchor| {
                slot_namespace != namespace || object_name(anchor) != Some(name.as_str())
            }) {
                return Ok(Err("operation key was reused with a different spec".into()));
            }
        }
        Ok(Ok(()))
    }

    /// Find or create the occupancy's active anchor.
    async fn ensure_anchor(
        &self,
        prepared: &Prepared,
        op_key: &str,
    ) -> Result<Result<Value, Refusal>, CampusProviderError> {
        let spec_sha256 = annotation(&prepared.anchor, ANNOTATION_SPEC_SHA256).unwrap_or_default();
        for _ in 0..3 {
            let existing = self
                .api
                .get(
                    CampusKubeKind::ConfigMap,
                    &prepared.namespace,
                    &prepared.name,
                )
                .await
                .map_err(provider_error)?;
            match existing {
                Some(anchor) => {
                    return Ok(match anchor_state(&anchor) {
                        Some("tombstone") => Err("the workload was already deleted".into()),
                        Some("active")
                            if annotation(&anchor, ANNOTATION_OP_KEY) != Some(op_key) =>
                        {
                            Err("the workload belongs to another provision operation".into())
                        }
                        Some("active")
                            if annotation(&anchor, ANNOTATION_SPEC_SHA256) != Some(spec_sha256) =>
                        {
                            Err("operation key was reused with a different spec".into())
                        }
                        Some("active") => Ok(anchor),
                        _ => Err("the workload anchor is in an unknown state".into()),
                    });
                }
                None => match self.api.create(&prepared.anchor).await {
                    Ok(anchor) => return Ok(Ok(anchor)),
                    Err(CampusKubeError::AlreadyExists(_)) => continue,
                    Err(CampusKubeError::Rejected(message)) => {
                        return Ok(Err(format!(
                            "the slot refused the workload anchor: {message}"
                        )))
                    }
                    Err(error) => return Err(provider_error(error)),
                },
            }
        }
        Err(CampusProviderError::Internal(
            "the workload anchor kept changing".into(),
        ))
    }

    /// `None` when no Job has the workload's name, otherwise whether it is
    /// this operation's Job.
    async fn existing_job(
        &self,
        prepared: &Prepared,
        op_key: &str,
        spec: &CampusProvisionSpec,
    ) -> Result<Option<bool>, CampusProviderError> {
        let existing = self
            .api
            .get(CampusKubeKind::Job, &prepared.namespace, &prepared.name)
            .await
            .map_err(provider_error)?;
        Ok(existing.map(|existing| {
            workload_of(&existing).as_ref() == Some(&spec.workload)
                && annotation(&existing, ANNOTATION_OP_KEY) == Some(op_key)
                && annotation(&existing, ANNOTATION_SPEC_SHA256)
                    == annotation(&prepared.anchor, ANNOTATION_SPEC_SHA256)
        }))
    }

    async fn observe_in(
        &self,
        namespace: &str,
        workload: &CampusWorkloadRef,
    ) -> Result<CampusObservation, CampusProviderError> {
        let selector = record_selector(&self.config.controller, workload, RECORD_WORKLOAD);
        let jobs = self
            .api
            .list(CampusKubeKind::Job, namespace, &selector)
            .await
            .map_err(provider_error)?;
        let pods = self
            .api
            .list(CampusKubeKind::Pod, namespace, &selector)
            .await
            .map_err(provider_error)?;
        Ok(observation(&jobs, &pods))
    }

    async fn fence_deleted(
        &self,
        namespace: &str,
        op_key: &str,
        workload: &CampusWorkloadRef,
    ) -> Result<bool, CampusKubeError> {
        let name = session_resource_name(workload);
        for _ in 0..3 {
            let mut tenant = "unknown".to_string();
            match self
                .api
                .get(CampusKubeKind::ConfigMap, namespace, &name)
                .await?
            {
                Some(record) if anchor_state(&record) == Some("tombstone") => return Ok(true),
                Some(record) => {
                    if let Some(found) = label(&record, LABEL_TENANT) {
                        tenant = found.to_string();
                    }
                    self.api
                        .delete(CampusKubeKind::ConfigMap, namespace, &name)
                        .await?;
                }
                None => {}
            }
            let tombstone = render_tombstone(
                &self.config.controller,
                namespace,
                &tenant,
                workload,
                op_key,
            )
            .map_err(|error| CampusKubeError::Rejected(error.to_string()))?;
            match self.api.create(&tombstone).await {
                Ok(_) => return Ok(true),
                Err(CampusKubeError::AlreadyExists(_)) => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(false)
    }

    async fn delete_in(
        &self,
        namespace: &str,
        op_key: &str,
        workload: &CampusWorkloadRef,
    ) -> Result<CampusDeleteOutcome, CampusKubeError> {
        if !self.fence_deleted(namespace, op_key, workload).await? {
            return Ok(CampusDeleteOutcome::InProgress);
        }
        let selector = record_selector(&self.config.controller, workload, RECORD_WORKLOAD);
        // The token goes first: a stopped session's Pod must not keep it while
        // it terminates.
        let token_selector =
            record_selector(&self.config.controller, workload, RECORD_BROKER_TOKEN);
        for secret in self
            .api
            .list(CampusKubeKind::Secret, namespace, &token_selector)
            .await?
        {
            if let Some(name) = object_name(&secret) {
                self.api
                    .delete(CampusKubeKind::Secret, namespace, name)
                    .await?;
            }
        }
        for kind in [CampusKubeKind::Job, CampusKubeKind::Pod] {
            for object in self.api.list(kind, namespace, &selector).await? {
                if object.pointer("/metadata/deletionTimestamp").is_none() {
                    if let Some(name) = object_name(&object) {
                        self.api.delete(kind, namespace, name).await?;
                    }
                }
            }
        }
        let jobs = self
            .api
            .list(CampusKubeKind::Job, namespace, &selector)
            .await?;
        let pods = self
            .api
            .list(CampusKubeKind::Pod, namespace, &selector)
            .await?;
        let secrets = self
            .api
            .list(CampusKubeKind::Secret, namespace, &token_selector)
            .await?;
        if jobs.is_empty() && pods.is_empty() && secrets.is_empty() {
            self.prune_tombstones(namespace, workload).await?;
            return Ok(CampusDeleteOutcome::Deleted);
        }
        let now = self.clock.now();
        let stuck = |object: &Value| {
            object
                .pointer("/metadata/deletionTimestamp")
                .and_then(Value::as_str)
                .and_then(parse_rfc3339_utc)
                .is_some_and(|since| now - since > self.config.stuck_termination_seconds)
        };
        if let Some(pod) = pods.iter().find(|pod| stuck(pod)) {
            return Ok(CampusDeleteOutcome::Unreachable {
                reason: format!(
                    "Pod {} on node {} has not confirmed termination",
                    object_name(pod).unwrap_or("?"),
                    pod.pointer("/spec/nodeName")
                        .and_then(Value::as_str)
                        .unwrap_or("?")
                ),
            });
        }
        if let Some(job) = jobs.iter().find(|job| stuck(job)) {
            return Ok(CampusDeleteOutcome::Blocked {
                reason: format!(
                    "Job {} is held by finalizers {}",
                    object_name(job).unwrap_or("?"),
                    job.pointer("/metadata/finalizers")
                        .cloned()
                        .unwrap_or(Value::Null)
                ),
            });
        }
        Ok(CampusDeleteOutcome::InProgress)
    }

    /// Older tombstones are no longer needed once a newer occupancy is fenced:
    /// a stale provision for them is refused by the generation check.
    async fn prune_tombstones(
        &self,
        namespace: &str,
        workload: &CampusWorkloadRef,
    ) -> Result<(), CampusKubeError> {
        let controller = &self.config.controller;
        let tombstones = self
            .api
            .list(
                CampusKubeKind::ConfigMap,
                namespace,
                &format!(
                    "{LABEL_CONTROLLER}={controller},{LABEL_SLOT}={},{LABEL_RECORD}={RECORD_TOMBSTONE}",
                    workload.slot_id
                ),
            )
            .await?;
        for tombstone in tombstones {
            let older = workload_of(&tombstone)
                .is_some_and(|other| other.slot_generation < workload.slot_generation);
            if let (true, Some(name)) = (older, object_name(&tombstone)) {
                self.api
                    .delete(CampusKubeKind::ConfigMap, namespace, name)
                    .await?;
            }
        }
        Ok(())
    }
}

/// Selects every Pod and denies all ingress and egress.
fn is_default_deny(policy: &Value) -> bool {
    let empty = |value: Option<&Value>| match value {
        None | Some(Value::Null) => true,
        Some(Value::Array(items)) => items.is_empty(),
        Some(Value::Object(fields)) => fields.values().all(|field| match field {
            Value::Object(map) => map.is_empty(),
            Value::Array(items) => items.is_empty(),
            _ => false,
        }),
        Some(_) => false,
    };
    let spec = policy.get("spec");
    let types = spec
        .and_then(|spec| spec.get("policyTypes"))
        .and_then(Value::as_array)
        .map(|types| types.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    empty(spec.and_then(|spec| spec.get("podSelector")))
        && types.contains(&"Ingress")
        && types.contains(&"Egress")
        && empty(spec.and_then(|spec| spec.get("ingress")))
        && empty(spec.and_then(|spec| spec.get("egress")))
}

fn provider_error(error: CampusKubeError) -> CampusProviderError {
    match error {
        CampusKubeError::Unreachable(message) => CampusProviderError::Unreachable(message),
        other => CampusProviderError::Internal(other.to_string()),
    }
}

fn object_name(object: &Value) -> Option<&str> {
    object.pointer("/metadata/name").and_then(Value::as_str)
}

fn label<'a>(object: &'a Value, key: &str) -> Option<&'a str> {
    object
        .pointer("/metadata/labels")
        .and_then(|labels| labels.get(key))
        .and_then(Value::as_str)
}

fn annotation<'a>(object: &'a Value, key: &str) -> Option<&'a str> {
    object
        .pointer("/metadata/annotations")
        .and_then(|annotations| annotations.get(key))
        .and_then(Value::as_str)
}

fn anchor_state(object: &Value) -> Option<&str> {
    object.pointer("/data/state").and_then(Value::as_str)
}

fn workload_of(object: &Value) -> Option<CampusWorkloadRef> {
    Some(CampusWorkloadRef {
        session_id: label(object, LABEL_SESSION)?.to_string(),
        slot_id: label(object, LABEL_SLOT)?.to_string(),
        slot_generation: label(object, LABEL_GENERATION)?.parse().ok()?,
    })
}

fn condition_true(object: &Value, kind: &str) -> Option<String> {
    object
        .pointer("/status/conditions")
        .and_then(Value::as_array)?
        .iter()
        .find(|condition| {
            condition.get("type").and_then(Value::as_str) == Some(kind)
                && condition.get("status").and_then(Value::as_str) == Some("True")
        })
        .map(|condition| {
            [
                condition.get("reason").and_then(Value::as_str),
                condition.get("message").and_then(Value::as_str),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(": ")
        })
}

/// Classify a workload from its Job and Pods.
fn observation(jobs: &[Value], pods: &[Value]) -> CampusObservation {
    let Some(job) = jobs.first() else {
        return CampusObservation::Absent;
    };
    let workspace_id = object_name(job).unwrap_or_default().to_string();
    if let Some(reason) = condition_true(job, "Failed") {
        return CampusObservation::Failed {
            reason: format!("session Job failed: {reason}"),
        };
    }
    if condition_true(job, "Complete").is_some() {
        return CampusObservation::Failed {
            reason: "the workspace process exited".into(),
        };
    }
    if job.pointer("/metadata/deletionTimestamp").is_some() {
        return CampusObservation::Failed {
            reason: "the session Job is being deleted".into(),
        };
    }
    let live = pods
        .iter()
        .filter(|pod| pod.pointer("/metadata/deletionTimestamp").is_none())
        .collect::<Vec<_>>();
    if live.iter().any(|pod| {
        pod.pointer("/status/phase").and_then(Value::as_str) == Some("Running")
            && condition_true(pod, "Ready").is_some()
    }) {
        return CampusObservation::Ready { workspace_id };
    }
    for pod in &live {
        if pod.pointer("/status/phase").and_then(Value::as_str) == Some("Failed") {
            let reason = pod
                .pointer("/status/reason")
                .and_then(Value::as_str)
                .unwrap_or("Failed");
            return CampusObservation::Failed {
                reason: format!("session Pod failed: {reason}"),
            };
        }
        let waiting = pod
            .pointer("/status/containerStatuses")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|status| {
                status
                    .pointer("/state/waiting/reason")
                    .and_then(Value::as_str)
            })
            .find(|reason| FATAL_WAITING_REASONS.contains(reason));
        if let Some(reason) = waiting {
            return CampusObservation::Failed {
                reason: format!("session container cannot start: {reason}"),
            };
        }
    }
    CampusObservation::Provisioning
}

/// Seconds since the epoch for `YYYY-MM-DDTHH:MM:SSZ`, the form the API
/// server uses for timestamps.
fn parse_rfc3339_utc(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[19] != b'Z'
    {
        return None;
    }
    let number = |range: std::ops::Range<usize>| value.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

#[async_trait]
impl CampusExecutionProvider for KubernetesCampusProvider {
    async fn provision(
        &self,
        op_key: &str,
        spec: &CampusProvisionSpec,
    ) -> Result<CampusProvisionOutcome, CampusProviderError> {
        let rejected = |reason: String| Ok(CampusProvisionOutcome::Rejected { reason });
        let prepared = match self.prepare(op_key, spec).await? {
            Ok(prepared) => prepared,
            Err(reason) => return rejected(reason),
        };
        if let Err(reason) = self.check_runtime().await? {
            return rejected(reason);
        }
        if let Err(reason) = self
            .check_network_enforcement(&prepared.namespace, prepared.binding.egress().is_some())
            .await?
        {
            return rejected(reason);
        }
        if let Err(reason) = self.check_fences(&prepared.namespace, op_key, spec).await? {
            return rejected(reason);
        }
        let anchor = match self.ensure_anchor(&prepared, op_key).await? {
            Ok(anchor) => anchor,
            Err(reason) => return rejected(reason),
        };
        let Some(anchor_uid) = anchor.pointer("/metadata/uid").and_then(Value::as_str) else {
            return Err(CampusProviderError::Internal(
                "the anchor has no uid".into(),
            ));
        };
        match self
            .ensure_broker_token(
                &prepared.namespace,
                op_key,
                &prepared.tenant_id,
                &spec.workload,
                &prepared.binding,
                anchor_uid,
            )
            .await?
        {
            Ok(_) => {}
            Err(reason) => return rejected(reason),
        }
        let mut job = prepared.job.clone();
        job["metadata"]["ownerReferences"] = json!([{
            "apiVersion": "v1",
            "kind": "ConfigMap",
            "name": prepared.name,
            "uid": anchor_uid,
            "blockOwnerDeletion": false,
        }]);
        // Look before creating: the slot quota is checked before the name, so a
        // repeated create of an existing Job is refused as over quota rather
        // than reported as already existing.
        let held = self.existing_job(&prepared, op_key, spec).await?;
        match held {
            Some(true) => {}
            Some(false) => return rejected("a different Job holds the workload's name".into()),
            None => match self.api.create(&job).await {
                Ok(_) => {}
                Err(CampusKubeError::AlreadyExists(_)) => {
                    if self.existing_job(&prepared, op_key, spec).await? != Some(true) {
                        return rejected("a different Job holds the workload's name".into());
                    }
                }
                Err(CampusKubeError::Rejected(message)) => {
                    // A concurrent call with the same key may have won the race.
                    if self.existing_job(&prepared, op_key, spec).await? != Some(true) {
                        return rejected(format!(
                            "the lab slot refused the session Job: {message}"
                        ));
                    }
                }
                Err(error) => return Err(provider_error(error)),
            },
        }
        // A delete may have fenced the occupancy while the Job was created.
        let current = self
            .api
            .get(
                CampusKubeKind::ConfigMap,
                &prepared.namespace,
                &prepared.name,
            )
            .await
            .map_err(provider_error)?;
        let still_active = current.as_ref().is_some_and(|current| {
            anchor_state(current) == Some("active")
                && current.pointer("/metadata/uid").and_then(Value::as_str) == Some(anchor_uid)
        });
        if !still_active {
            let _ = self
                .api
                .delete(CampusKubeKind::Job, &prepared.namespace, &prepared.name)
                .await;
            return rejected("the workload was deleted while it was being provisioned".into());
        }
        let workspace_id = prepared.name.clone();
        match self.observe_in(&prepared.namespace, &spec.workload).await? {
            CampusObservation::Ready { workspace_id } => {
                Ok(CampusProvisionOutcome::Ready { workspace_id })
            }
            CampusObservation::Failed { reason } => rejected(reason),
            CampusObservation::Absent | CampusObservation::Provisioning => {
                Ok(CampusProvisionOutcome::InProgress { workspace_id })
            }
        }
    }

    async fn observe(
        &self,
        workload: &CampusWorkloadRef,
    ) -> Result<CampusObservation, CampusProviderError> {
        let namespace = self
            .namespace(workload)
            .map_err(CampusProviderError::Internal)?;
        self.observe_in(namespace, workload).await
    }

    async fn delete(
        &self,
        op_key: &str,
        workload: &CampusWorkloadRef,
    ) -> Result<CampusDeleteOutcome, CampusProviderError> {
        let namespace = match self.namespace(workload) {
            Ok(namespace) => namespace,
            Err(reason) => return Ok(CampusDeleteOutcome::Blocked { reason }),
        };
        let labels_valid = is_label_value(&workload.session_id)
            && is_label_value(&workload.slot_id)
            && workload.slot_generation >= 1;
        if !labels_valid {
            // Nothing with these labels can exist: the provider never creates it.
            return Ok(CampusDeleteOutcome::Deleted);
        }
        match self.delete_in(namespace, op_key, workload).await {
            Ok(outcome) => Ok(outcome),
            Err(CampusKubeError::Unreachable(reason)) => {
                Ok(CampusDeleteOutcome::Unreachable { reason })
            }
            Err(CampusKubeError::Rejected(reason)) => Ok(CampusDeleteOutcome::Blocked { reason }),
            Err(error) => Err(CampusProviderError::Internal(error.to_string())),
        }
    }

    async fn list_owned(&self) -> Result<Vec<OwnedCampusResource>, CampusProviderError> {
        let controller = &self.config.controller;
        let selector = format!(
            "{LABEL_CONTROLLER}={controller},{LABEL_RECORD} in \
             ({RECORD_ANCHOR},{RECORD_WORKLOAD},{RECORD_BROKER_TOKEN})"
        );
        let mut owned = Vec::new();
        for namespace in self.config.slots.values() {
            for (kind, name) in [
                (CampusKubeKind::Job, "job"),
                (CampusKubeKind::Pod, "pod"),
                (CampusKubeKind::ConfigMap, "anchor"),
                (CampusKubeKind::Secret, "broker-token"),
            ] {
                let objects = self
                    .api
                    .list(kind, namespace, &selector)
                    .await
                    .map_err(provider_error)?;
                for object in objects {
                    let (Some(workload), Some(object_id)) =
                        (workload_of(&object), object_name(&object))
                    else {
                        continue;
                    };
                    owned.push(OwnedCampusResource {
                        resource_id: format!("{namespace}/{name}/{object_id}"),
                        kind: name.into(),
                        workload,
                        op_key: annotation(&object, ANNOTATION_OP_KEY)
                            .unwrap_or_default()
                            .to_string(),
                    });
                }
            }
        }
        owned.sort();
        Ok(owned)
    }
}
