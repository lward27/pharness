//! The immutable session Job template.
//!
//! Everything a session Pod runs with is fixed here and derived only from the
//! approved profile, the session's effective policy and the admin-owned
//! provider configuration. Nothing comes from the student or the request.

use super::{
    KubernetesCampusProviderConfig, ANNOTATION_COURSE, ANNOTATION_EXPIRES_AT, ANNOTATION_OP_KEY,
    ANNOTATION_POLICY_HASH, ANNOTATION_PROFILE_HASH, ANNOTATION_SPEC_SHA256, LABEL_CONTROLLER,
    LABEL_GENERATION, LABEL_OP, LABEL_PROFILE, LABEL_RECORD, LABEL_SESSION, LABEL_SLOT,
    LABEL_TENANT, RECORD_ANCHOR, RECORD_BROKER_TOKEN, RECORD_TOMBSTONE, RECORD_WORKLOAD,
};
use crate::campus_capability::{
    check_sandbox_environment, CampusSandboxError, CampusSandboxNetwork,
};
use crate::campus_egress::{
    format_campus_egress_hosts, CAMPUS_EGRESS_HOSTS_ANNOTATION, CAMPUS_EGRESS_LABEL,
    CAMPUS_EGRESS_LABEL_VALUE, CAMPUS_EGRESS_PORT, CAMPUS_EGRESS_ROUTES,
};
use crate::campus_sandbox_pod::{
    check_campus_sandbox_pod_spec, CampusPodSpecViolation, CampusSandboxBrokerBinding,
};
use crate::policy::CampusEffectiveConstraints;
use crate::{
    CampusIsolation, CampusLaunchGate, CampusNetworkMode, CampusProfile, CampusProfileKind,
    CampusWorkloadRef,
};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// UID and GID of the unprivileged user in the approved runner images.
pub const SESSION_UID: i64 = 65532;
pub const WORKSPACE_MOUNT: &str = "/workspace";
pub const TMP_MOUNT: &str = "/tmp";
pub const SESSION_CONTAINER: &str = "workspace";
/// Init container that holds the workspace back until the slot's network
/// isolation is in force for the Pod.
pub const NETWORK_GATE_CONTAINER: &str = "network-gate";
const TERMINATION_GRACE_SECONDS: i64 = 5;
const MIN_TMP_MIB: u32 = 16;
const MAX_TMP_MIB: u32 = 256;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CampusTemplateError {
    #[error("profile {0} is a security profile; Kubernetes lab slots never launch it")]
    SecurityProfile(String),
    #[error("profile {0} requires {1:?} isolation, which the lab slots do not provide")]
    Isolation(String, CampusIsolation),
    #[error("profile {0} has an unqualified launch gate")]
    LaunchGate(String),
    #[error(
        "the session network allows {0} egress destinations, but no Campus egress proxy is \
         configured for the lab slots"
    )]
    ApprovedEgress(usize),
    #[error("egress destination {0} is not an approved Campus egress route")]
    UnapprovedEgress(String),
    #[error("session pod spec refused: {0}")]
    PodSpec(#[from] CampusPodSpecViolation),
    #[error("session environment refused: {0}")]
    Environment(#[from] CampusSandboxError),
    #[error("session envelope does not fit the lab slot: {0}")]
    Capacity(String),
    #[error("{field} value {value:?} is not a valid Kubernetes label value")]
    Label { field: &'static str, value: String },
    #[error("session deadline must be between 1 and {max} seconds, got {got}")]
    Deadline { got: i64, max: i64 },
}

/// Everything the Job template is derived from.
#[derive(Debug, Clone, Copy)]
pub struct CampusJobTemplateInput<'a> {
    pub config: &'a KubernetesCampusProviderConfig,
    pub namespace: &'a str,
    pub workload: &'a CampusWorkloadRef,
    pub tenant_id: &'a str,
    pub course_id: &'a str,
    pub op_key: &'a str,
    pub spec_sha256: &'a str,
    pub profile: &'a CampusProfile,
    pub constraints: &'a CampusEffectiveConstraints,
    pub network: &'a CampusSandboxNetwork,
    /// The broker URL, token Secret and (with approved egress) the proxy.
    pub binding: &'a CampusSandboxBrokerBinding,
    pub effective_policy_hash: &'a str,
    pub expires_at: i64,
    pub active_deadline_seconds: i64,
}

/// The resource sizes a session gets, derived from its effective policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CampusSessionSizing {
    pub cpu_millicores: u32,
    /// Container memory limit: process memory plus the memory-backed
    /// `/workspace` and `/tmp` volumes, whose pages count against it.
    pub memory_limit_mib: u32,
    pub workspace_mib: u32,
    pub tmp_mib: u32,
    pub ephemeral_storage_mib: u32,
    pub pids: u32,
}

impl CampusSessionSizing {
    pub fn derive(
        constraints: &CampusEffectiveConstraints,
        config: &KubernetesCampusProviderConfig,
    ) -> Result<Self, CampusTemplateError> {
        let storage = constraints.ephemeral_storage_mib;
        let tmp_mib = (storage / 4).clamp(MIN_TMP_MIB, MAX_TMP_MIB);
        let workspace_mib = storage
            .checked_sub(tmp_mib)
            .filter(|mib| *mib > 0)
            .ok_or_else(|| {
                CampusTemplateError::Capacity(format!("{storage} MiB of storage is too small"))
            })?;
        let memory_limit_mib = constraints
            .memory_mib
            .checked_add(storage)
            .ok_or_else(|| CampusTemplateError::Capacity("memory overflows".into()))?;
        let sizing = Self {
            cpu_millicores: constraints.cpu_millicores,
            memory_limit_mib,
            workspace_mib,
            tmp_mib,
            ephemeral_storage_mib: config.log_storage_mib,
            pids: constraints.pids,
        };
        sizing.check_fits(config)?;
        Ok(sizing)
    }

    fn check_fits(
        &self,
        config: &KubernetesCampusProviderConfig,
    ) -> Result<(), CampusTemplateError> {
        let capacity = &config.slot_capacity;
        let overhead = &config.runtime.overhead;
        let cpu = u64::from(self.cpu_millicores) + u64::from(overhead.cpu_millicores);
        let memory = u64::from(self.memory_limit_mib) + u64::from(overhead.memory_mib);
        if self.cpu_millicores == 0 || cpu > u64::from(capacity.cpu_millicores) {
            return Err(CampusTemplateError::Capacity(format!(
                "cpu {}m plus {}m runtime overhead exceeds the slot's {}m",
                self.cpu_millicores, overhead.cpu_millicores, capacity.cpu_millicores
            )));
        }
        if memory > u64::from(capacity.memory_mib) {
            return Err(CampusTemplateError::Capacity(format!(
                "memory {} MiB (processes plus memory-backed workspace) plus {} MiB runtime \
                 overhead exceeds the slot's {} MiB",
                self.memory_limit_mib, overhead.memory_mib, capacity.memory_mib
            )));
        }
        if self.ephemeral_storage_mib > capacity.ephemeral_storage_mib {
            return Err(CampusTemplateError::Capacity(format!(
                "ephemeral storage {} MiB exceeds the slot's {} MiB",
                self.ephemeral_storage_mib, capacity.ephemeral_storage_mib
            )));
        }
        if self.pids == 0 {
            return Err(CampusTemplateError::Capacity(
                "pids must be positive".into(),
            ));
        }
        Ok(())
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Deterministic name shared by a workload's Job and its anchor ConfigMap, so
/// a repeated create for the same occupancy finds the existing objects.
pub fn session_resource_name(workload: &CampusWorkloadRef) -> String {
    let digest = sha256_hex(
        format!(
            "{}\n{}\n{}",
            workload.session_id, workload.slot_id, workload.slot_generation
        )
        .as_bytes(),
    );
    format!("campus-{}", &digest[..24])
}

pub fn op_label(op_key: &str) -> String {
    sha256_hex(op_key.as_bytes())[..40].to_string()
}

/// Kubernetes label values: at most 63 characters, alphanumeric at both ends,
/// and `-`, `_` or `.` inside.
pub fn is_label_value(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_alphanumeric()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

pub(crate) fn label_value(field: &'static str, value: &str) -> Result<String, CampusTemplateError> {
    if is_label_value(value) {
        Ok(value.to_string())
    } else {
        Err(CampusTemplateError::Label {
            field,
            value: value.to_string(),
        })
    }
}

/// The exact-match selector for one workload occupancy.
pub fn workload_selector(controller: &str, workload: &CampusWorkloadRef) -> String {
    format!(
        "{LABEL_CONTROLLER}={controller},{LABEL_SESSION}={},{LABEL_SLOT}={},{LABEL_GENERATION}={}",
        workload.session_id, workload.slot_id, workload.slot_generation
    )
}

pub fn record_selector(controller: &str, workload: &CampusWorkloadRef, record: &str) -> String {
    format!(
        "{},{LABEL_RECORD}={record}",
        workload_selector(controller, workload)
    )
}

fn ownership_labels(
    controller: &str,
    tenant_id: &str,
    workload: &CampusWorkloadRef,
    record: &str,
) -> Result<Map<String, Value>, CampusTemplateError> {
    let mut labels = Map::new();
    labels.insert(
        LABEL_CONTROLLER.into(),
        label_value("controller", controller)?.into(),
    );
    labels.insert(
        LABEL_TENANT.into(),
        label_value("tenant_id", tenant_id)?.into(),
    );
    labels.insert(
        LABEL_SESSION.into(),
        label_value("session_id", &workload.session_id)?.into(),
    );
    labels.insert(
        LABEL_SLOT.into(),
        label_value("slot_id", &workload.slot_id)?.into(),
    );
    if workload.slot_generation < 1 {
        return Err(CampusTemplateError::Label {
            field: "slot_generation",
            value: workload.slot_generation.to_string(),
        });
    }
    labels.insert(
        LABEL_GENERATION.into(),
        workload.slot_generation.to_string().into(),
    );
    labels.insert(LABEL_RECORD.into(), record.into());
    labels.insert("app.kubernetes.io/part-of".into(), "pharness-campus".into());
    Ok(labels)
}

/// Labels of the session's broker token Secret.
pub fn broker_token_labels(
    controller: &str,
    tenant_id: &str,
    workload: &CampusWorkloadRef,
) -> Result<std::collections::BTreeMap<String, String>, CampusTemplateError> {
    Ok(
        ownership_labels(controller, tenant_id, workload, RECORD_BROKER_TOKEN)?
            .into_iter()
            .filter_map(|(key, value)| value.as_str().map(|value| (key, value.to_string())))
            .collect(),
    )
}

/// The anchor every workload resource is owned by. Deleting it lets the
/// garbage collector remove any Job created against it, including one a
/// stale worker creates after the delete.
pub fn render_anchor(
    controller: &str,
    namespace: &str,
    tenant_id: &str,
    workload: &CampusWorkloadRef,
    op_key: &str,
    spec_sha256: &str,
) -> Result<Value, CampusTemplateError> {
    let mut labels = ownership_labels(controller, tenant_id, workload, RECORD_ANCHOR)?;
    labels.insert(LABEL_OP.into(), op_label(op_key).into());
    Ok(json!({
        "apiVersion": "v1",
        "kind": "ConfigMap",
        "metadata": {
            "name": session_resource_name(workload),
            "namespace": namespace,
            "labels": labels,
            "annotations": {
                ANNOTATION_OP_KEY: op_key,
                ANNOTATION_SPEC_SHA256: spec_sha256,
            },
        },
        "immutable": true,
        "data": { "state": "active" },
    }))
}

/// The fence a delete leaves behind: it holds the workload's name so no new
/// anchor, and therefore no new Job, can be created for that occupancy.
pub fn render_tombstone(
    controller: &str,
    namespace: &str,
    tenant_id: &str,
    workload: &CampusWorkloadRef,
    op_key: &str,
) -> Result<Value, CampusTemplateError> {
    let labels = ownership_labels(controller, tenant_id, workload, RECORD_TOMBSTONE)?;
    Ok(json!({
        "apiVersion": "v1",
        "kind": "ConfigMap",
        "metadata": {
            "name": session_resource_name(workload),
            "namespace": namespace,
            "labels": labels,
            "annotations": { ANNOTATION_OP_KEY: op_key },
        },
        "immutable": true,
        "data": { "state": "tombstone" },
    }))
}

/// The argv that runs `command` under the session's process limit, with its
/// output discarded so a student cannot fill the node's container log
/// directory through it. gVisor enforces RLIMIT_NPROC per UID inside the
/// sandbox; Kubernetes has no per-Pod PID limit on the lab node.
///
/// It is the session container's entrypoint. A process started in the
/// sandbox any other way (for example `kubectl exec`) does not inherit the
/// limit, so every such path must wrap its command with this as well.
pub fn session_command(pids: u32, command: &[String]) -> Vec<String> {
    let mut argv = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        // bash and busybox name the process limit -u; dash (Debian's
        // /bin/sh) names it -p. Without a flag each sets both soft and hard
        // limits, and if neither works the chain stops before the exec.
        format!(
            "{{ ulimit -u {pids} || ulimit -p {pids}; }} 2>/dev/null && ulimit -c 0 && \
             exec \"$@\" </dev/null >/dev/null 2>&1"
        ),
        "campus-workspace".to_string(),
    ];
    argv.extend(command.iter().cloned());
    argv
}

fn base_environment(config: &KubernetesCampusProviderConfig) -> Vec<(String, String)> {
    let mut environment = vec![
        ("HOME".to_string(), WORKSPACE_MOUNT.to_string()),
        ("TMPDIR".to_string(), TMP_MOUNT.to_string()),
        ("LANG".to_string(), "C.UTF-8".to_string()),
    ];
    environment.extend(
        config
            .environment
            .iter()
            .map(|(name, value)| (name.clone(), value.clone())),
    );
    environment
}

/// The approved egress hosts of a session network. Every destination must be
/// an approved Campus egress route on port 443; anything else refuses the
/// whole session rather than being dropped.
pub fn approved_egress_hosts(
    network: &CampusSandboxNetwork,
) -> Result<BTreeSet<&'static str>, CampusTemplateError> {
    if network.egress.is_empty() {
        return Ok(BTreeSet::new());
    }
    if network.mode != CampusNetworkMode::ApprovedEgress {
        return Err(CampusTemplateError::UnapprovedEgress(
            "(network mode is denied)".into(),
        ));
    }
    network
        .egress
        .iter()
        .map(|destination| {
            CAMPUS_EGRESS_ROUTES
                .iter()
                .find(|(host, _)| *host == destination.host)
                .filter(|_| destination.port == CAMPUS_EGRESS_PORT)
                .map(|(host, _)| *host)
                .ok_or_else(|| {
                    CampusTemplateError::UnapprovedEgress(format!(
                        "{}:{}",
                        destination.host, destination.port
                    ))
                })
        })
        .collect()
}

/// Refuse what the slots cannot provide, before anything is rendered.
/// `egress_proxy` says whether the Campus egress proxy is configured; without
/// it a session with approved egress cannot be enforced and is refused.
pub fn check_launchable_in_slot(
    profile: &CampusProfile,
    network: &CampusSandboxNetwork,
    egress_proxy: bool,
) -> Result<(), CampusTemplateError> {
    if profile.kind == CampusProfileKind::Security {
        return Err(CampusTemplateError::SecurityProfile(profile.id.clone()));
    }
    if profile.isolation != CampusIsolation::Gvisor {
        return Err(CampusTemplateError::Isolation(
            profile.id.clone(),
            profile.isolation,
        ));
    }
    if profile.launch_gate != CampusLaunchGate::Enabled {
        return Err(CampusTemplateError::LaunchGate(profile.id.clone()));
    }
    let hosts = approved_egress_hosts(network)?;
    if !hosts.is_empty() && !egress_proxy {
        return Err(CampusTemplateError::ApprovedEgress(hosts.len()));
    }
    Ok(())
}

/// The init container's script: wait until none of the canaries (addresses
/// the slot policy denies, such as the API server) accepts a TCP connection
/// on two consecutive rounds, and fail the Pod if that does not happen in
/// time. The network-policy controller programs a new Pod's rules shortly
/// after the Pod gets its address, and a Pod is briefly unconfined until
/// then. The probe uses whichever of python3 or node the image has; an
/// image with neither fails closed.
const NETWORK_GATE_SCRIPT: &str = r#"deadline=$(( $(date +%s) + $1 )); shift
probe() {
  if command -v python3 >/dev/null 2>&1; then
    python3 -c 'import socket, sys
s = socket.socket()
s.settimeout(1)
try:
    s.connect((sys.argv[1], int(sys.argv[2])))
except Exception:
    sys.exit(1)
sys.exit(0)' "$1" "$2"
  elif command -v node >/dev/null 2>&1; then
    node -e 'const s = require("net").connect({ host: process.argv[1], port: Number(process.argv[2]), timeout: 1000 });
s.on("connect", () => process.exit(0)); s.on("error", () => process.exit(1)); s.on("timeout", () => process.exit(1));' "$1" "$2"
  else
    echo "campus network gate: the image has no python3 or node to probe with" >&2
    exit 3
  fi
}
quiet=0
while [ "$(date +%s)" -lt "$deadline" ]; do
  open=0
  for canary in "$@"; do
    if probe "${canary%:*}" "${canary##*:}"; then open=1; fi
  done
  if [ "$open" = 0 ]; then
    quiet=$((quiet + 1))
    if [ "$quiet" -ge 2 ]; then exit 0; fi
  else
    quiet=0
  fi
  sleep 1
done
echo "campus network gate: network isolation is not in force" >&2
exit 1"#;

/// The argv of the network gate init container.
pub fn network_gate_command(timeout_seconds: u32, canaries: &[String]) -> Vec<String> {
    let mut argv = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        NETWORK_GATE_SCRIPT.to_string(),
        "campus-network-gate".to_string(),
        timeout_seconds.to_string(),
    ];
    argv.extend(canaries.iter().cloned());
    argv
}

/// Render the session Job. The environment check runs on the exact
/// environment in the rendered container.
pub fn render_session_job(
    input: &CampusJobTemplateInput<'_>,
) -> Result<Value, CampusTemplateError> {
    let config = input.config;
    let binding = input.binding;
    check_launchable_in_slot(input.profile, input.network, binding.egress().is_some())?;
    let egress_hosts = approved_egress_hosts(input.network)?;
    if egress_hosts.is_empty() == binding.egress().is_some() {
        return Err(CampusTemplateError::ApprovedEgress(egress_hosts.len()));
    }
    let max = i64::from(crate::campus_profile::MAX_TTL_SECONDS);
    if !(1..=max).contains(&input.active_deadline_seconds) {
        return Err(CampusTemplateError::Deadline {
            got: input.active_deadline_seconds,
            max,
        });
    }
    let sizing = CampusSessionSizing::derive(input.constraints, config)?;
    let labels = ownership_labels(
        &config.controller,
        input.tenant_id,
        input.workload,
        RECORD_WORKLOAD,
    )?;
    let mut labels = labels;
    labels.insert(LABEL_OP.into(), op_label(input.op_key).into());
    labels.insert(
        LABEL_PROFILE.into(),
        label_value("profile_id", &input.profile.id)?.into(),
    );
    let mut annotations = json!({
        ANNOTATION_OP_KEY: input.op_key,
        ANNOTATION_SPEC_SHA256: input.spec_sha256,
        ANNOTATION_PROFILE_HASH: input.profile.profile_hash(),
        ANNOTATION_POLICY_HASH: input.effective_policy_hash,
        ANNOTATION_COURSE: input.course_id,
        ANNOTATION_EXPIRES_AT: input.expires_at.to_string(),
    });
    // The label admits the Pod to the egress proxy in the slot NetworkPolicy;
    // the proxy then allows only the annotated hosts.
    if !egress_hosts.is_empty() {
        labels.insert(CAMPUS_EGRESS_LABEL.into(), CAMPUS_EGRESS_LABEL_VALUE.into());
        annotations[CAMPUS_EGRESS_HOSTS_ANNOTATION] =
            format_campus_egress_hosts(egress_hosts.iter().copied()).into();
    }
    let resources = json!({
        "cpu": format!("{}m", sizing.cpu_millicores),
        "memory": format!("{}Mi", sizing.memory_limit_mib),
        "ephemeral-storage": format!("{}Mi", sizing.ephemeral_storage_mib),
    });
    let environment = base_environment(config);
    let security_context = json!({
        "allowPrivilegeEscalation": false,
        "privileged": false,
        "readOnlyRootFilesystem": true,
        "runAsNonRoot": true,
        "capabilities": { "drop": ["ALL"] },
    });
    let gate = json!({
        "name": NETWORK_GATE_CONTAINER,
        "image": input.profile.image,
        "imagePullPolicy": "IfNotPresent",
        "command": network_gate_command(
            config.network_gate.timeout_seconds,
            &config.network_gate.canaries,
        ),
        "env": [{ "name": "LANG", "value": "C.UTF-8" }],
        "securityContext": security_context,
        "resources": { "requests": resources, "limits": resources },
    });
    let mut env = environment
        .iter()
        .map(|(name, value)| json!({ "name": name, "value": value }))
        .collect::<Vec<_>>();
    env.extend(binding.env());
    let container = json!({
        "name": SESSION_CONTAINER,
        "image": input.profile.image,
        "imagePullPolicy": "IfNotPresent",
        "command": session_command(sizing.pids, &config.workspace_command),
        "workingDir": WORKSPACE_MOUNT,
        "env": env,
        "securityContext": security_context,
        "resources": { "requests": resources, "limits": resources },
        "volumeMounts": [
            { "name": "workspace", "mountPath": WORKSPACE_MOUNT },
            { "name": "tmp", "mountPath": TMP_MOUNT },
            binding.volume_mount(),
        ],
    });
    let volumes = json!([
        { "name": "workspace", "emptyDir": {
            "medium": "Memory", "sizeLimit": format!("{}Mi", sizing.workspace_mib),
        } },
        { "name": "tmp", "emptyDir": {
            "medium": "Memory", "sizeLimit": format!("{}Mi", sizing.tmp_mib),
        } },
        binding.volume(),
    ]);
    let pod_spec = json!({
        "runtimeClassName": config.runtime.class_name,
        "nodeSelector": config.runtime.node_selector,
        "restartPolicy": "Never",
        "activeDeadlineSeconds": input.active_deadline_seconds,
        "terminationGracePeriodSeconds": TERMINATION_GRACE_SECONDS,
        "automountServiceAccountToken": false,
        "enableServiceLinks": false,
        "hostNetwork": false,
        "hostPID": false,
        "hostIPC": false,
        "shareProcessNamespace": false,
        "securityContext": {
            "runAsNonRoot": true,
            "runAsUser": SESSION_UID,
            "runAsGroup": SESSION_UID,
            "fsGroup": SESSION_UID,
            "seccompProfile": { "type": "RuntimeDefault" },
        },
        "initContainers": [gate],
        "containers": [container],
        "volumes": volumes,
    });
    let job = json!({
        "apiVersion": "batch/v1",
        "kind": "Job",
        "metadata": {
            "name": session_resource_name(input.workload),
            "namespace": input.namespace,
            "labels": labels,
            "annotations": annotations,
        },
        "spec": {
            "backoffLimit": 0,
            "completions": 1,
            "parallelism": 1,
            "activeDeadlineSeconds": input.active_deadline_seconds,
            "ttlSecondsAfterFinished": config.finished_job_ttl_seconds,
            "template": {
                "metadata": { "labels": labels, "annotations": annotations },
                "spec": pod_spec,
            },
        },
    });
    check_rendered_environment(&job)?;
    check_campus_sandbox_pod_spec(&job["spec"]["template"]["spec"], binding)?;
    Ok(job)
}

/// Run the sandbox environment check on what the Pod will actually receive.
fn check_rendered_environment(job: &Value) -> Result<(), CampusTemplateError> {
    let containers = job
        .pointer("/spec/template/spec/containers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    for container in containers {
        let pairs = container
            .get("env")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|entry| {
                (
                    entry
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    entry
                        .get("value")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>();
        check_sandbox_environment(pairs)?;
    }
    Ok(())
}
