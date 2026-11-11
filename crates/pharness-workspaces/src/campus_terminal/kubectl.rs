//! The Kubernetes terminal: `kubectl exec` of the fixed terminal command
//! into the one session Pod of a workload, under a local pty so that
//! kubectl allocates a remote TTY and forwards window-size changes.
//!
//! kubectl runs under its own identity (`global_args`: a kubeconfig,
//! context or impersonation), which needs only `get`/`list` on Pods and
//! `create` on `pods/exec` in the lab slots. The lifecycle controller's
//! identity is not used and has no `pods/exec`.

use super::{
    terminal_command, CampusTerminalAttacher, CampusTerminalError, CampusTerminalInput,
    CampusTerminalStream, CampusTerminalTarget, STREAM_CAPACITY, TERMINAL_OUTPUT_CHUNK_BYTES,
};
use crate::campus_lab_session::kubernetes::{
    is_label_value, record_selector, CampusKubeApi, CampusKubeError, CampusKubeKind,
    LABEL_CONTROLLER, LABEL_GENERATION, LABEL_RECORD, LABEL_SESSION, LABEL_SLOT, RECORD_WORKLOAD,
    SESSION_CONTAINER,
};
use crate::CampusWorkloadRef;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KubectlCampusTerminalConfig {
    pub kubectl: String,
    /// Arguments before every kubectl command, selecting the terminal
    /// identity, e.g. `--kubeconfig=/var/run/pharness/campus-terminal/kubeconfig`.
    pub global_args: Vec<String>,
    /// Value of `pharness.dev/campus-controller` on session resources.
    pub controller: String,
    /// Lab slot id (as in the journal) to its namespace.
    pub slots: BTreeMap<String, String>,
    /// What the wrapper execs after setting the process limit.
    pub shell: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid Campus terminal configuration: {0}")]
pub struct CampusTerminalConfigError(pub String);

impl KubectlCampusTerminalConfig {
    pub fn validate(&self) -> Result<(), CampusTerminalConfigError> {
        let invalid = |reason: &str| Err(CampusTerminalConfigError(reason.to_string()));
        if self.kubectl.trim().is_empty() {
            return invalid("kubectl is required");
        }
        if !is_label_value(&self.controller) {
            return invalid("controller must be a label value");
        }
        if self.slots.is_empty() {
            return invalid("at least one lab slot is required");
        }
        if !self
            .slots
            .iter()
            .all(|(slot, namespace)| is_label_value(slot) && is_dns_label(namespace))
        {
            return invalid("slot ids must be label values and namespaces DNS labels");
        }
        if !self
            .shell
            .first()
            .is_some_and(|program| program.starts_with('/'))
            || self.shell.iter().any(String::is_empty)
        {
            return invalid("shell must start with an absolute program path");
        }
        // The global arguments select an identity; they must not smuggle in
        // a subcommand or a second command line.
        if self
            .global_args
            .iter()
            .any(|arg| !arg.starts_with('-') || arg == "--")
        {
            return invalid("global_args may contain only flags");
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

fn is_dns_subdomain(value: &str) -> bool {
    !value.is_empty() && value.len() <= 253 && value.split('.').all(is_dns_label)
}

/// The kubectl argv for one terminal. Only `namespace` and `pod` vary, and
/// both come from trusted provider state.
pub fn kubectl_exec_args(
    global_args: &[String],
    namespace: &str,
    pod: &str,
    command: &[String],
) -> Vec<String> {
    let mut args = global_args.to_vec();
    args.extend([
        "exec".to_string(),
        "--stdin".to_string(),
        "--tty".to_string(),
        format!("--namespace={namespace}"),
        format!("--container={SESSION_CONTAINER}"),
        pod.to_string(),
        "--".to_string(),
    ]);
    args.extend(command.iter().cloned());
    args
}

fn label<'a>(object: &'a Value, key: &str) -> Option<&'a str> {
    object
        .pointer("/metadata/labels")
        .and_then(|labels| labels.get(key))
        .and_then(Value::as_str)
}

fn is_ready(pod: &Value) -> bool {
    pod.pointer("/status/phase").and_then(Value::as_str) == Some("Running")
        && pod
            .pointer("/status/conditions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|condition| {
                condition.get("type").and_then(Value::as_str) == Some("Ready")
                    && condition.get("status").and_then(Value::as_str) == Some("True")
            })
}

/// The one Pod a terminal may attach to: in the slot's namespace, labelled
/// for exactly this occupancy, running, ready, not being deleted, with the
/// session container. Anything else, including two candidates, is refused.
pub fn select_session_pod(
    controller: &str,
    namespace: &str,
    workload: &CampusWorkloadRef,
    pods: &[Value],
) -> Result<String, CampusTerminalError> {
    let generation = workload.slot_generation.to_string();
    let candidates = pods
        .iter()
        .filter(|pod| {
            pod.pointer("/metadata/namespace").and_then(Value::as_str) == Some(namespace)
                && label(pod, LABEL_CONTROLLER) == Some(controller)
                && label(pod, LABEL_SESSION) == Some(workload.session_id.as_str())
                && label(pod, LABEL_SLOT) == Some(workload.slot_id.as_str())
                && label(pod, LABEL_GENERATION) == Some(generation.as_str())
                && label(pod, LABEL_RECORD) == Some(RECORD_WORKLOAD)
                && pod.pointer("/metadata/deletionTimestamp").is_none()
        })
        .collect::<Vec<_>>();
    let [pod] = candidates.as_slice() else {
        return Err(CampusTerminalError::NotRunning(format!(
            "{} session Pods match the workload",
            candidates.len()
        )));
    };
    if !is_ready(pod) {
        return Err(CampusTerminalError::NotRunning(
            "the session Pod is not running and ready".into(),
        ));
    }
    let has_container = pod
        .pointer("/spec/containers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|container| container.get("name").and_then(Value::as_str) == Some(SESSION_CONTAINER));
    if !has_container {
        return Err(CampusTerminalError::Refused(
            "the session Pod has no session container".into(),
        ));
    }
    pod.pointer("/metadata/name")
        .and_then(Value::as_str)
        .filter(|name| is_dns_subdomain(name))
        .map(str::to_string)
        .ok_or_else(|| CampusTerminalError::Refused("the session Pod name is invalid".into()))
}

pub struct KubectlCampusTerminal {
    config: KubectlCampusTerminalConfig,
    /// Pod lookups, under the same terminal identity as the exec.
    api: Arc<dyn CampusKubeApi>,
}

impl KubectlCampusTerminal {
    pub fn new(
        config: KubectlCampusTerminalConfig,
        api: Arc<dyn CampusKubeApi>,
    ) -> Result<Self, CampusTerminalConfigError> {
        config.validate()?;
        Ok(Self { config, api })
    }

    pub fn config(&self) -> &KubectlCampusTerminalConfig {
        &self.config
    }

    /// The namespace and Pod of a workload, from the slot map and the
    /// cluster's own records.
    pub async fn resolve(
        &self,
        workload: &CampusWorkloadRef,
    ) -> Result<(String, String), CampusTerminalError> {
        let namespace = self.config.slots.get(&workload.slot_id).ok_or_else(|| {
            CampusTerminalError::Refused(format!(
                "slot {} is not a configured lab slot",
                workload.slot_id
            ))
        })?;
        if !is_label_value(&workload.session_id) || workload.slot_generation < 1 {
            return Err(CampusTerminalError::Refused(
                "the workload identity is invalid".into(),
            ));
        }
        let selector = record_selector(&self.config.controller, workload, RECORD_WORKLOAD);
        let pods = self
            .api
            .list(CampusKubeKind::Pod, namespace, &selector)
            .await
            .map_err(|error| match error {
                CampusKubeError::Rejected(message) => CampusTerminalError::Refused(message),
                other => CampusTerminalError::Unavailable(other.to_string()),
            })?;
        let pod = select_session_pod(&self.config.controller, namespace, workload, &pods)?;
        Ok((namespace.clone(), pod))
    }
}

#[async_trait]
impl CampusTerminalAttacher for KubectlCampusTerminal {
    async fn attach(
        &self,
        target: &CampusTerminalTarget,
    ) -> Result<CampusTerminalStream, CampusTerminalError> {
        if target.pids == 0 {
            return Err(CampusTerminalError::Refused(
                "the session has no process limit".into(),
            ));
        }
        let (namespace, pod) = self.resolve(&target.workload).await?;
        let args = kubectl_exec_args(
            &self.config.global_args,
            &namespace,
            &pod,
            &terminal_command(target.pids, &self.config.shell),
        );
        spawn_under_pty(&self.config.kubectl, &args, target)
    }
}

fn unavailable(error: impl std::fmt::Display) -> CampusTerminalError {
    CampusTerminalError::Unavailable(error.to_string())
}

/// Run `program` with a pty as its controlling terminal and connect the pty
/// to a [`CampusTerminalStream`].
pub(crate) fn spawn_under_pty(
    program: &str,
    args: &[String],
    target: &CampusTerminalTarget,
) -> Result<CampusTerminalStream, CampusTerminalError> {
    let (pty, pts) = pty_process::open().map_err(unavailable)?;
    pty.resize(pty_process::Size::new(target.size.rows, target.size.cols))
        .map_err(unavailable)?;
    let mut child = pty_process::Command::new(program)
        .args(args)
        .kill_on_drop(true)
        .spawn(pts)
        .map_err(unavailable)?;
    let (mut read, mut write) = pty.into_split();
    let (input_tx, mut input_rx) = mpsc::channel::<CampusTerminalInput>(STREAM_CAPACITY);
    let (output_tx, output_rx) = mpsc::channel(STREAM_CAPACITY);
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        while let Some(message) = input_rx.recv().await {
            let written = match message {
                CampusTerminalInput::Data(bytes) => write.write_all(&bytes).await.is_ok(),
                CampusTerminalInput::Resize(size) => write
                    .resize(pty_process::Size::new(size.rows, size.cols))
                    .is_ok(),
            };
            if !written {
                break;
            }
        }
    });
    tokio::spawn(async move {
        let mut buffer = vec![0_u8; TERMINAL_OUTPUT_CHUNK_BYTES];
        loop {
            // Linux reports EIO on the pty once the process side is closed.
            match read.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if output_tx.send(buffer[..read].to_vec()).await.is_err() {
                        break;
                    }
                }
            }
        }
    });
    tokio::spawn(async move {
        tokio::select! {
            _ = child.wait() => {}
            _ = stop_rx => {
                let _ = child.kill().await;
            }
        }
    });
    Ok(CampusTerminalStream::new(input_tx, output_rx, stop_tx))
}
