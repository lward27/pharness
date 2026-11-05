//! The Pod-spec contract for a Campus student workload.
//!
//! Everything inside a session Pod is student-controllable: the student's
//! code, shell, browser terminal and any process they start. The contract
//! therefore gives the Pod nothing a student could use against the platform:
//!
//! - no Kubernetes service-account token (`automountServiceAccountToken:
//!   false`, no projected volumes);
//! - no host namespaces and no shared process namespace, so the student sees
//!   only their own processes;
//! - exactly one container. A trusted broker is never a sidecar: a sidecar
//!   shares the network namespace (its listeners are reachable on loopback)
//!   and is reachable through shared volumes, so the broker runs as a
//!   separate process outside the Pod and is reached over the network at
//!   [`CampusSandboxBrokerBinding::broker_url`], which must not be loopback;
//! - only literal environment values, none of them credential-shaped
//!   ([`check_sandbox_environment`]), and no `envFrom` or `valueFrom`. The
//!   platform variables (the broker URL and, for a session with approved
//!   egress, the proxy and CA variables of [`CampusSandboxEgress`]) have
//!   exactly the binding's values and appear nowhere else;
//! - only `emptyDir`, `persistentVolumeClaim` and `configMap` volumes, plus
//!   exactly one `secret` volume: the session's own broker token (and the
//!   public Campus egress CA certificate), mounted read-only at
//!   [`CAMPUS_BROKER_TOKEN_DIR`];
//! - no privileged containers, privilege escalation or added capabilities.
//!
//! [`check_campus_sandbox_pod_spec`] enforces this on the exact `PodSpec` a
//! provider is about to submit. It is a contract check, not a substitute for
//! admission policy or for verifying a running Pod.

use crate::campus_broker_token::{CampusBrokerAudience, CampusIssuedBrokerToken};
use crate::campus_capability::check_sandbox_environment;
use crate::CampusWorkloadRef;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::net::Ipv4Addr;
use url::{Host, Url};

/// The only platform-provided variable: where the broker is. It carries no
/// credential.
pub const CAMPUS_BROKER_URL_ENV: &str = "CAMPUS_BROKER_URL";
pub const CAMPUS_BROKER_TOKEN_DIR: &str = "/var/run/campus-broker";
/// Read the token from here on every request; it is rotated in place.
pub const CAMPUS_BROKER_TOKEN_PATH: &str = "/var/run/campus-broker/token";
pub const CAMPUS_BROKER_TOKEN_VOLUME: &str = "campus-broker-token";
pub const CAMPUS_BROKER_TOKEN_SECRET_KEY: &str = "token";
/// The public certificate of the Campus egress CA, next to the token. The
/// egress proxy presents certificates from this CA for the approved hosts.
pub const CAMPUS_EGRESS_CA_SECRET_KEY: &str = "egress-ca.pem";
pub const CAMPUS_EGRESS_CA_PATH: &str = "/var/run/campus-broker/egress-ca.pem";
/// Where HTTPS clients find the egress proxy.
pub const CAMPUS_EGRESS_PROXY_ENV: &str = "HTTPS_PROXY";
/// Keeps the broker off the proxy.
pub const CAMPUS_NO_PROXY_ENV: &str = "NO_PROXY";
/// Trust-store variables pointed at [`CAMPUS_EGRESS_CA_PATH`], for OpenSSL
/// and curl (git), Python `ssl`, requests and pip, and Node (npm).
pub const CAMPUS_EGRESS_CA_ENV: &[&str] = &[
    "SSL_CERT_FILE",
    "CURL_CA_BUNDLE",
    "GIT_SSL_CAINFO",
    "REQUESTS_CA_BUNDLE",
    "PIP_CERT",
    "NODE_EXTRA_CA_CERTS",
];
const MAX_CA_PEM_BYTES: usize = 16 * 1024;
/// Annotations on the token Secret. They are not secret and let a provider
/// decide when to rotate without keeping state.
pub const CAMPUS_BROKER_TOKEN_ID_ANNOTATION: &str = "pharness.dev/campus-broker-token-id";
pub const CAMPUS_BROKER_TOKEN_ROTATE_AT_ANNOTATION: &str =
    "pharness.dev/campus-broker-token-rotate-at";

/// Mount paths conventionally used for platform credentials. Nothing may be
/// mounted there, so tools that look for a service-account token find none.
const RESERVED_MOUNT_PREFIXES: &[&str] = &["/var/run/secrets", "/run/secrets"];
const ALLOWED_VOLUME_SOURCES: &[&str] = &["emptyDir", "persistentVolumeClaim", "configMap"];

/// The rule a Pod spec broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CampusPodSpecRule {
    Shape,
    ServiceAccountToken,
    HostNamespace,
    ProcessNamespaceSharing,
    SingleContainer,
    EphemeralContainers,
    EnvFromSource,
    EnvValueFrom,
    Environment,
    BrokerUrl,
    EgressProxy,
    Privileged,
    PrivilegeEscalation,
    AddedCapabilities,
    VolumeSource,
    SecretVolume,
    TokenMount,
    MountPath,
    UnknownVolume,
}

impl CampusPodSpecRule {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shape => "shape",
            Self::ServiceAccountToken => "service_account_token",
            Self::HostNamespace => "host_namespace",
            Self::ProcessNamespaceSharing => "process_namespace_sharing",
            Self::SingleContainer => "single_container",
            Self::EphemeralContainers => "ephemeral_containers",
            Self::EnvFromSource => "env_from_source",
            Self::EnvValueFrom => "env_value_from",
            Self::Environment => "environment",
            Self::BrokerUrl => "broker_url",
            Self::EgressProxy => "egress_proxy",
            Self::Privileged => "privileged",
            Self::PrivilegeEscalation => "privilege_escalation",
            Self::AddedCapabilities => "added_capabilities",
            Self::VolumeSource => "volume_source",
            Self::SecretVolume => "secret_volume",
            Self::TokenMount => "token_mount",
            Self::MountPath => "mount_path",
            Self::UnknownVolume => "unknown_volume",
        }
    }
}

impl fmt::Display for CampusPodSpecRule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("campus sandbox pod spec breaks {rule}: {detail}")]
pub struct CampusPodSpecViolation {
    pub rule: CampusPodSpecRule,
    pub detail: String,
}

fn violation(rule: CampusPodSpecRule, detail: impl Into<String>) -> CampusPodSpecViolation {
    CampusPodSpecViolation {
        rule,
        detail: detail.into(),
    }
}

/// The deterministic name of a workload's token Secret. It is derived from
/// the full fenced workload, so a later occupancy of the slot never reuses
/// it, and it is a valid DNS-1123 name whatever the ids contain.
pub fn campus_broker_token_secret_name(workload: &CampusWorkloadRef) -> String {
    let mut hasher = Sha256::new();
    for part in [
        workload.session_id.as_bytes(),
        workload.slot_id.as_bytes(),
        workload.slot_generation.to_string().as_bytes(),
    ] {
        hasher.update(part);
        hasher.update([0]);
    }
    let digest = format!("{:x}", hasher.finalize());
    format!("campus-broker-{}", &digest[..24])
}

/// How a session with approved egress reaches the Campus egress proxy: its
/// address and the public CA certificate the proxy's certificates chain to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampusSandboxEgress {
    proxy_url: String,
    ca_certificate_pem: String,
}

impl CampusSandboxEgress {
    /// Refuses a proxy that is not plain `http://host:port` to a network
    /// endpoint outside the Pod, and anything but PEM certificates as the CA.
    pub fn new(proxy_url: &Url, ca_certificate_pem: &str) -> Result<Self, CampusPodSpecViolation> {
        let refuse = |detail: &str| Err(violation(CampusPodSpecRule::EgressProxy, detail));
        check_network_endpoint(proxy_url, CampusPodSpecRule::EgressProxy)?;
        if proxy_url.scheme() != "http" || !matches!(proxy_url.path(), "" | "/") {
            return refuse("the egress proxy URL must be http://<host>:<port>");
        }
        let (Some(host), Some(port)) = (proxy_url.host_str(), proxy_url.port()) else {
            return refuse("the egress proxy URL must name a host and an explicit port");
        };
        let pem = ca_certificate_pem.trim();
        let blocks = pem.matches("-----BEGIN CERTIFICATE-----").count();
        let only_certificates = blocks > 0
            && blocks == pem.matches("-----END CERTIFICATE-----").count()
            && pem.starts_with("-----BEGIN CERTIFICATE-----")
            && pem.ends_with("-----END CERTIFICATE-----")
            && pem.lines().all(|line| {
                line == "-----BEGIN CERTIFICATE-----"
                    || line == "-----END CERTIFICATE-----"
                    || (!line.is_empty()
                        && line.len() <= 76
                        && line.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=')
                        }))
            });
        if !only_certificates || pem.len() > MAX_CA_PEM_BYTES {
            return refuse("the egress CA must be PEM certificates only");
        }
        Ok(Self {
            proxy_url: format!("http://{host}:{port}"),
            ca_certificate_pem: format!("{pem}\n"),
        })
    }

    /// The `HTTPS_PROXY` value: `http://<host>:<port>`.
    pub fn proxy_url(&self) -> &str {
        &self.proxy_url
    }

    pub fn ca_certificate_pem(&self) -> &str {
        &self.ca_certificate_pem
    }
}

/// What a provider gives one session Pod to reach the broker: the broker URL
/// and the name of the Secret holding the session's inference token, and for
/// a session with approved egress, the egress proxy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampusSandboxBrokerBinding {
    broker_url: Url,
    secret_name: String,
    egress: Option<CampusSandboxEgress>,
}

impl CampusSandboxBrokerBinding {
    /// Refuses a broker URL that is not plain HTTP(S) to a network host, so
    /// the broker can never be a listener inside the Pod or the metadata
    /// service.
    pub fn new(
        broker_url: &Url,
        workload: &CampusWorkloadRef,
    ) -> Result<Self, CampusPodSpecViolation> {
        check_network_endpoint(broker_url, CampusPodSpecRule::BrokerUrl)?;
        Ok(Self {
            broker_url: broker_url.clone(),
            secret_name: campus_broker_token_secret_name(workload),
            egress: None,
        })
    }

    /// Give the session the egress proxy. Only for a session whose effective
    /// policy approves egress.
    pub fn with_egress(mut self, egress: CampusSandboxEgress) -> Self {
        self.egress = Some(egress);
        self
    }

    pub fn egress(&self) -> Option<&CampusSandboxEgress> {
        self.egress.as_ref()
    }

    pub fn broker_url(&self) -> &Url {
        &self.broker_url
    }

    pub fn secret_name(&self) -> &str {
        &self.secret_name
    }

    /// The platform variables, by name. They name the broker, the egress
    /// proxy and the CA file, and carry no credential.
    pub fn platform_environment(&self) -> Vec<(&'static str, String)> {
        let mut environment = vec![(CAMPUS_BROKER_URL_ENV, self.broker_url.to_string())];
        if let Some(egress) = &self.egress {
            environment.push((CAMPUS_EGRESS_PROXY_ENV, egress.proxy_url.clone()));
            environment.push((
                CAMPUS_NO_PROXY_ENV,
                self.broker_url.host_str().unwrap_or_default().to_string(),
            ));
            for name in CAMPUS_EGRESS_CA_ENV {
                environment.push((name, CAMPUS_EGRESS_CA_PATH.to_string()));
            }
        }
        environment
    }

    /// The container `env` entries the platform adds.
    pub fn env(&self) -> Vec<Value> {
        self.platform_environment()
            .into_iter()
            .map(|(name, value)| json!({ "name": name, "value": value }))
            .collect()
    }

    fn secret_keys(&self) -> Vec<&'static str> {
        let mut keys = vec![CAMPUS_BROKER_TOKEN_SECRET_KEY];
        if self.egress.is_some() {
            keys.push(CAMPUS_EGRESS_CA_SECRET_KEY);
        }
        keys
    }

    /// The Pod volume holding the token. Mode 0440 keeps it out of reach of
    /// other users in the container where the image runs as non-root.
    pub fn volume(&self) -> Value {
        json!({
            "name": CAMPUS_BROKER_TOKEN_VOLUME,
            "secret": {
                "secretName": self.secret_name,
                "defaultMode": 0o440,
                "items": self
                    .secret_keys()
                    .into_iter()
                    .map(|key| json!({ "key": key, "path": key }))
                    .collect::<Vec<_>>(),
            },
        })
    }

    /// The student container's mount of [`Self::volume`]. No `subPath`, so
    /// the kubelet updates the file in place when the token rotates.
    pub fn volume_mount(&self) -> Value {
        json!({
            "name": CAMPUS_BROKER_TOKEN_VOLUME,
            "mountPath": CAMPUS_BROKER_TOKEN_DIR,
            "readOnly": true,
        })
    }

    /// The token Secret to create or replace. It contains the raw token, so
    /// it must go to the Kubernetes API only (for example on stdin), never to
    /// a log or a command line.
    pub fn secret_manifest(
        &self,
        namespace: &str,
        labels: &BTreeMap<String, String>,
        token: &CampusIssuedBrokerToken,
    ) -> Result<Value, CampusPodSpecViolation> {
        if !token.audience.sandbox_may_hold() {
            return Err(violation(
                CampusPodSpecRule::SecretVolume,
                format!("a {} token may not be given to a sandbox", token.audience),
            ));
        }
        if campus_broker_token_secret_name(&token.workload) != self.secret_name {
            return Err(violation(
                CampusPodSpecRule::SecretVolume,
                "the token was issued for another workload",
            ));
        }
        let mut data = Map::new();
        data.insert(
            CAMPUS_BROKER_TOKEN_SECRET_KEY.into(),
            token.secret.expose().into(),
        );
        if let Some(egress) = &self.egress {
            data.insert(
                CAMPUS_EGRESS_CA_SECRET_KEY.into(),
                egress.ca_certificate_pem.clone().into(),
            );
        }
        Ok(json!({
            "apiVersion": "v1",
            "kind": "Secret",
            "type": "Opaque",
            "metadata": {
                "name": self.secret_name,
                "namespace": namespace,
                "labels": labels,
                "annotations": {
                    CAMPUS_BROKER_TOKEN_ID_ANNOTATION: token.token_id,
                    CAMPUS_BROKER_TOKEN_ROTATE_AT_ANNOTATION: token.rotate_at().to_string(),
                },
            },
            "stringData": data,
        }))
    }
}

fn check_network_endpoint(
    url: &Url,
    rule: CampusPodSpecRule,
) -> Result<(), CampusPodSpecViolation> {
    let refuse = |detail: &str| Err(violation(rule, detail));
    if !matches!(url.scheme(), "http" | "https") {
        return refuse("the URL must be http or https");
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return refuse("the URL must not carry credentials, a query or a fragment");
    }
    let local = match url.host() {
        None => return refuse("the URL has no host"),
        Some(Host::Domain(domain)) => {
            let domain = domain.trim_end_matches('.').to_ascii_lowercase();
            domain == "localhost"
                || domain.ends_with(".localhost")
                || domain == "metadata.google.internal"
        }
        Some(Host::Ipv4(address)) => is_local_v4(address),
        Some(Host::Ipv6(address)) => {
            address.is_loopback()
                || address.is_unspecified()
                || (address.segments()[0] & 0xffc0) == 0xfe80
                || address.to_ipv4_mapped().is_some_and(is_local_v4)
        }
    };
    if local {
        return refuse(
            "the endpoint must be outside the Pod, not loopback, link-local or a \
             metadata service",
        );
    }
    Ok(())
}

fn is_local_v4(address: Ipv4Addr) -> bool {
    address.is_loopback()
        || address.is_unspecified()
        || address.is_link_local()
        || address.is_broadcast()
}

fn object<'a>(
    value: &'a Value,
    what: &str,
) -> Result<&'a Map<String, Value>, CampusPodSpecViolation> {
    value
        .as_object()
        .ok_or_else(|| violation(CampusPodSpecRule::Shape, format!("{what} is not an object")))
}

fn array<'a>(value: Option<&'a Value>, what: &str) -> Result<&'a [Value], CampusPodSpecViolation> {
    match value {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(items)) => Ok(items),
        Some(_) => Err(violation(
            CampusPodSpecRule::Shape,
            format!("{what} is not an array"),
        )),
    }
}

fn is_true(value: Option<&Value>) -> bool {
    value.and_then(Value::as_bool).unwrap_or(false)
}

fn name_of<'a>(
    value: &'a Map<String, Value>,
    what: &str,
) -> Result<&'a str, CampusPodSpecViolation> {
    value
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| violation(CampusPodSpecRule::Shape, format!("{what} has no name")))
}

/// Check the `PodSpec` of a session workload (for a Job, its
/// `spec.template.spec`) against the contract in the module documentation.
/// A provider must call this on the exact spec it submits and refuse to
/// submit on any violation.
pub fn check_campus_sandbox_pod_spec(
    spec: &Value,
    binding: &CampusSandboxBrokerBinding,
) -> Result<(), CampusPodSpecViolation> {
    use CampusPodSpecRule as R;
    let spec = object(spec, "the pod spec")?;
    if spec.get("automountServiceAccountToken") != Some(&Value::Bool(false)) {
        return Err(violation(
            R::ServiceAccountToken,
            "automountServiceAccountToken must be false",
        ));
    }
    for field in ["hostNetwork", "hostPID", "hostIPC"] {
        if is_true(spec.get(field)) {
            return Err(violation(
                R::HostNamespace,
                format!("{field} must be false"),
            ));
        }
    }
    if is_true(spec.get("shareProcessNamespace")) {
        return Err(violation(
            R::ProcessNamespaceSharing,
            "shareProcessNamespace must be false",
        ));
    }
    if !array(spec.get("ephemeralContainers"), "ephemeralContainers")?.is_empty() {
        return Err(violation(
            R::EphemeralContainers,
            "ephemeral containers are not allowed",
        ));
    }

    let volumes = check_volumes(array(spec.get("volumes"), "volumes")?, binding)?;
    let containers = array(spec.get("containers"), "containers")?;
    let [main] = containers else {
        return Err(violation(
            R::SingleContainer,
            format!(
                "a session pod runs exactly one container, not {}; a trusted broker must \
                 not be a sidecar",
                containers.len()
            ),
        ));
    };
    for init in array(spec.get("initContainers"), "initContainers")? {
        check_container(init, &volumes, binding, false)?;
    }
    check_container(main, &volumes, binding, true)
}

/// Returns the names of the declared volumes.
fn check_volumes(
    volumes: &[Value],
    binding: &CampusSandboxBrokerBinding,
) -> Result<Vec<String>, CampusPodSpecViolation> {
    use CampusPodSpecRule as R;
    let mut names = Vec::with_capacity(volumes.len());
    let mut token_volumes = 0;
    for volume in volumes {
        let volume = object(volume, "a volume")?;
        let name = name_of(volume, "a volume")?;
        let sources = volume
            .keys()
            .filter(|key| key.as_str() != "name")
            .collect::<Vec<_>>();
        let [source] = sources.as_slice() else {
            return Err(violation(
                R::VolumeSource,
                format!("volume {name} must have exactly one source"),
            ));
        };
        if source.as_str() == "secret" {
            check_token_volume(name, &volume["secret"], binding)?;
            token_volumes += 1;
        } else if !ALLOWED_VOLUME_SOURCES.contains(&source.as_str()) {
            return Err(violation(
                R::VolumeSource,
                format!("volume {name} uses {source}; only emptyDir, persistentVolumeClaim, configMap and the broker token secret are allowed"),
            ));
        }
        names.push(name.to_string());
    }
    if token_volumes != 1 {
        return Err(violation(
            R::SecretVolume,
            "the pod must declare exactly the session's broker token volume",
        ));
    }
    Ok(names)
}

fn check_token_volume(
    name: &str,
    secret: &Value,
    binding: &CampusSandboxBrokerBinding,
) -> Result<(), CampusPodSpecViolation> {
    let refuse = |detail: String| Err(violation(CampusPodSpecRule::SecretVolume, detail));
    if name != CAMPUS_BROKER_TOKEN_VOLUME {
        return refuse(format!(
            "secret volume {name} is not the broker token volume; no other secret may be mounted"
        ));
    }
    let secret = object(secret, "the token volume source")?;
    if secret.get("secretName").and_then(Value::as_str) != Some(binding.secret_name()) {
        return refuse("the token volume must name the session's own token secret".into());
    }
    if is_true(secret.get("optional")) {
        return refuse("the token volume must not be optional".into());
    }
    let items = array(secret.get("items"), "the token volume items")?;
    let mut projected = items
        .iter()
        .map(|item| {
            let key = item.get("key").and_then(Value::as_str);
            (key == item.get("path").and_then(Value::as_str))
                .then_some(key)
                .flatten()
        })
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default();
    projected.sort_unstable();
    let mut expected = binding.secret_keys();
    expected.sort_unstable();
    if projected != expected {
        return refuse(format!(
            "the token volume must project exactly {} at their own paths",
            expected.join(" and ")
        ));
    }
    Ok(())
}

fn check_container(
    container: &Value,
    volumes: &[String],
    binding: &CampusSandboxBrokerBinding,
    main: bool,
) -> Result<(), CampusPodSpecViolation> {
    use CampusPodSpecRule as R;
    let container = object(container, "a container")?;
    let name = name_of(container, "a container")?;
    if !array(container.get("envFrom"), "envFrom")?.is_empty() {
        return Err(violation(
            R::EnvFromSource,
            format!("container {name} uses envFrom"),
        ));
    }
    let mut environment = Vec::new();
    for entry in array(container.get("env"), "env")? {
        let entry = object(entry, "an env entry")?;
        let variable = name_of(entry, "an env entry")?;
        if entry.contains_key("valueFrom") {
            return Err(violation(
                R::EnvValueFrom,
                format!("container {name} takes {variable} from valueFrom"),
            ));
        }
        let value = match entry.get("value") {
            None | Some(Value::Null) => "",
            Some(Value::String(value)) => value.as_str(),
            Some(_) => {
                return Err(violation(
                    R::Shape,
                    format!("{variable} has a non-string value"),
                ))
            }
        };
        environment.push((variable, value));
    }
    check_sandbox_environment(environment.iter().copied())
        .map_err(|error| violation(R::Environment, format!("container {name}: {error}")))?;
    let platform = binding.platform_environment();
    let reserved = [
        CAMPUS_BROKER_URL_ENV,
        CAMPUS_EGRESS_PROXY_ENV,
        CAMPUS_NO_PROXY_ENV,
    ]
    .into_iter()
    .chain(CAMPUS_EGRESS_CA_ENV.iter().copied());
    for reserved in reserved {
        let found = environment
            .iter()
            .filter(|(variable, _)| *variable == reserved)
            .map(|(_, value)| *value)
            .collect::<Vec<_>>();
        let expected = platform
            .iter()
            .filter(|(variable, _)| main && *variable == reserved)
            .map(|(_, value)| value.as_str())
            .collect::<Vec<_>>();
        if found != expected {
            let rule = if reserved == CAMPUS_BROKER_URL_ENV {
                R::BrokerUrl
            } else {
                R::EgressProxy
            };
            return Err(violation(
                rule,
                format!(
                    "container {name} must have {reserved} {}",
                    if expected.is_empty() {
                        "never".to_string()
                    } else {
                        "exactly once, with the binding's value".to_string()
                    }
                ),
            ));
        }
    }

    let security = match container.get("securityContext") {
        None | Some(Value::Null) => Map::new(),
        Some(value) => object(value, "a securityContext")?.clone(),
    };
    if is_true(security.get("privileged")) {
        return Err(violation(
            R::Privileged,
            format!("container {name} is privileged"),
        ));
    }
    if security.get("allowPrivilegeEscalation") != Some(&Value::Bool(false)) {
        return Err(violation(
            R::PrivilegeEscalation,
            format!("container {name} must set allowPrivilegeEscalation to false"),
        ));
    }
    if let Some(capabilities) = security.get("capabilities") {
        if !array(capabilities.get("add"), "capabilities.add")?.is_empty() {
            return Err(violation(
                R::AddedCapabilities,
                format!("container {name} adds capabilities"),
            ));
        }
    }

    let mut token_mounts = 0;
    for mount in array(container.get("volumeMounts"), "volumeMounts")? {
        let mount = object(mount, "a volume mount")?;
        let volume = name_of(mount, "a volume mount")?;
        if !volumes.iter().any(|declared| declared == volume) {
            return Err(violation(
                R::UnknownVolume,
                format!("container {name} mounts undeclared volume {volume}"),
            ));
        }
        let path = mount.get("mountPath").and_then(Value::as_str).unwrap_or("");
        let normalized = path.trim_end_matches('/');
        if RESERVED_MOUNT_PREFIXES
            .iter()
            .any(|prefix| normalized == *prefix || normalized.starts_with(&format!("{prefix}/")))
        {
            return Err(violation(
                R::MountPath,
                format!("container {name} mounts {volume} at reserved path {path}"),
            ));
        }
        if volume == CAMPUS_BROKER_TOKEN_VOLUME {
            token_mounts += 1;
            let exact = normalized == CAMPUS_BROKER_TOKEN_DIR
                && mount.get("readOnly") == Some(&Value::Bool(true))
                && !mount.contains_key("subPath")
                && !mount.contains_key("subPathExpr");
            if !main || !exact {
                return Err(violation(
                    R::TokenMount,
                    format!(
                        "only the student container may mount the token, read-only at \
                         {CAMPUS_BROKER_TOKEN_DIR} without subPath (container {name})"
                    ),
                ));
            }
        }
    }
    if main && token_mounts != 1 {
        return Err(violation(
            R::TokenMount,
            format!("container {name} must mount the broker token exactly once"),
        ));
    }
    Ok(())
}

/// The audience a provider puts into a sandbox.
pub const CAMPUS_SANDBOX_AUDIENCE: CampusBrokerAudience = CampusBrokerAudience::Inference;

#[cfg(test)]
mod tests;
