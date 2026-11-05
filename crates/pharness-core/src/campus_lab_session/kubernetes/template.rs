//! The immutable session Job template.
//!
//! Everything a session Pod runs with is fixed here and derived only from the
//! approved profile, the session's effective policy and the admin-owned
//! provider configuration. Nothing comes from the student or the request.

use super::{
    KubernetesCampusProviderConfig, ANNOTATION_COURSE, ANNOTATION_EXPIRES_AT, ANNOTATION_OP_KEY,
    ANNOTATION_POLICY_HASH, ANNOTATION_PROFILE_HASH, ANNOTATION_SPEC_SHA256, LABEL_CONTROLLER,
    LABEL_GENERATION, LABEL_OP, LABEL_PROFILE, LABEL_RECORD, LABEL_SESSION, LABEL_SLOT,
    LABEL_TENANT, RECORD_ANCHOR, RECORD_TOMBSTONE, RECORD_WORKLOAD,
};
use crate::campus_capability::{
    check_sandbox_environment, CampusSandboxError, CampusSandboxNetwork,
};
use crate::policy::CampusEffectiveConstraints;
use crate::{
    CampusIsolation, CampusLaunchGate, CampusNetworkMode, CampusProfile, CampusProfileKind,
    CampusWorkloadRef,
};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

/// UID and GID of the unprivileged user in the approved runner images.
pub const SESSION_UID: i64 = 65532;
pub const WORKSPACE_MOUNT: &str = "/workspace";
pub const TMP_MOUNT: &str = "/tmp";
pub const SESSION_CONTAINER: &str = "workspace";
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
        "the session network allows {0} egress destinations; lab slots are default-deny and \
         have no admin-owned egress enforcer for approved destinations"
    )]
    ApprovedEgress(usize),
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

/// Refuse what the slots cannot provide, before anything is rendered.
pub fn check_launchable_in_slot(
    profile: &CampusProfile,
    network: &CampusSandboxNetwork,
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
    if network.mode != CampusNetworkMode::Denied || !network.egress.is_empty() {
        return Err(CampusTemplateError::ApprovedEgress(network.egress.len()));
    }
    Ok(())
}

/// Render the session Job. The environment check runs on the exact
/// environment in the rendered container.
pub fn render_session_job(
    input: &CampusJobTemplateInput<'_>,
) -> Result<Value, CampusTemplateError> {
    let config = input.config;
    check_launchable_in_slot(input.profile, input.network)?;
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
    let annotations = json!({
        ANNOTATION_OP_KEY: input.op_key,
        ANNOTATION_SPEC_SHA256: input.spec_sha256,
        ANNOTATION_PROFILE_HASH: input.profile.profile_hash(),
        ANNOTATION_POLICY_HASH: input.effective_policy_hash,
        ANNOTATION_COURSE: input.course_id,
        ANNOTATION_EXPIRES_AT: input.expires_at.to_string(),
    });
    let resources = json!({
        "cpu": format!("{}m", sizing.cpu_millicores),
        "memory": format!("{}Mi", sizing.memory_limit_mib),
        "ephemeral-storage": format!("{}Mi", sizing.ephemeral_storage_mib),
    });
    let environment = base_environment(config);
    let container = json!({
        "name": SESSION_CONTAINER,
        "image": input.profile.image,
        "imagePullPolicy": "IfNotPresent",
        "command": session_command(sizing.pids, &config.workspace_command),
        "workingDir": WORKSPACE_MOUNT,
        "env": environment
            .iter()
            .map(|(name, value)| json!({ "name": name, "value": value }))
            .collect::<Vec<_>>(),
        "securityContext": {
            "allowPrivilegeEscalation": false,
            "privileged": false,
            "readOnlyRootFilesystem": true,
            "runAsNonRoot": true,
            "capabilities": { "drop": ["ALL"] },
        },
        "resources": { "requests": resources, "limits": resources },
        "volumeMounts": [
            { "name": "workspace", "mountPath": WORKSPACE_MOUNT },
            { "name": "tmp", "mountPath": TMP_MOUNT },
        ],
    });
    let volumes = json!([
        { "name": "workspace", "emptyDir": {
            "medium": "Memory", "sizeLimit": format!("{}Mi", sizing.workspace_mib),
        } },
        { "name": "tmp", "emptyDir": {
            "medium": "Memory", "sizeLimit": format!("{}Mi", sizing.tmp_mib),
        } },
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
