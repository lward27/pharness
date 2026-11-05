//! The few Kubernetes operations the Campus provider needs, and their
//! kubectl implementation.

use async_trait::async_trait;
use serde_json::Value;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CampusKubeKind {
    Job,
    Pod,
    ConfigMap,
    /// Only the session's broker token Secret.
    Secret,
    /// Read only: the provider checks the admin-owned slot policies exist.
    NetworkPolicy,
    /// Cluster scoped; the namespace argument is ignored.
    RuntimeClass,
}

impl CampusKubeKind {
    pub fn resource(self) -> &'static str {
        match self {
            Self::Job => "jobs.batch",
            Self::Pod => "pods",
            Self::ConfigMap => "configmaps",
            Self::Secret => "secrets",
            Self::NetworkPolicy => "networkpolicies.networking.k8s.io",
            Self::RuntimeClass => "runtimeclasses.node.k8s.io",
        }
    }

    pub fn from_kind(kind: &str) -> Option<Self> {
        match kind {
            "Job" => Some(Self::Job),
            "Pod" => Some(Self::Pod),
            "ConfigMap" => Some(Self::ConfigMap),
            "Secret" => Some(Self::Secret),
            "NetworkPolicy" => Some(Self::NetworkPolicy),
            "RuntimeClass" => Some(Self::RuntimeClass),
            _ => None,
        }
    }

    fn namespaced(self) -> bool {
        self != Self::RuntimeClass
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CampusKubeError {
    #[error("already exists: {0}")]
    AlreadyExists(String),
    /// Refused by admission, RBAC or API validation. Retrying will not help.
    #[error("rejected by the API server: {0}")]
    Rejected(String),
    /// The API server could not be reached or did not answer; the outcome is
    /// unknown.
    #[error("Kubernetes API unreachable: {0}")]
    Unreachable(String),
    #[error("Kubernetes request failed: {0}")]
    Failed(String),
}

#[async_trait]
pub trait CampusKubeApi: Send + Sync {
    async fn get(
        &self,
        kind: CampusKubeKind,
        namespace: &str,
        name: &str,
    ) -> Result<Option<Value>, CampusKubeError>;

    async fn list(
        &self,
        kind: CampusKubeKind,
        namespace: &str,
        selector: &str,
    ) -> Result<Vec<Value>, CampusKubeError>;

    /// Create the object and return it as stored, with its `uid`.
    async fn create(&self, manifest: &Value) -> Result<Value, CampusKubeError>;

    /// Replace an existing object with the manifest, which must carry the
    /// stored `resourceVersion` so a concurrent change is refused rather than
    /// overwritten.
    async fn replace(&self, manifest: &Value) -> Result<Value, CampusKubeError>;

    /// Delete one object; an absent object is not an error. Jobs are deleted
    /// with background propagation, so their Pods follow.
    async fn delete(
        &self,
        kind: CampusKubeKind,
        namespace: &str,
        name: &str,
    ) -> Result<(), CampusKubeError>;
}

/// Shells out to kubectl, like the rest of PHarness's Kubernetes dispatch.
/// `global_args` go before every command (e.g. `--context`, or `--as` for an
/// operator running the provider under the controller's identity).
#[derive(Debug, Clone)]
pub struct KubectlCampusKubeApi {
    kubectl: String,
    global_args: Vec<String>,
    timeout: Duration,
}

impl KubectlCampusKubeApi {
    pub fn new(kubectl: impl Into<String>, global_args: Vec<String>) -> Self {
        Self {
            kubectl: kubectl.into(),
            global_args,
            timeout: Duration::from_secs(30),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    async fn run(
        &self,
        args: Vec<String>,
        stdin: Option<Vec<u8>>,
    ) -> Result<Vec<u8>, CampusKubeError> {
        let mut command = tokio::process::Command::new(&self.kubectl);
        command
            .args(&self.global_args)
            .arg(format!(
                "--request-timeout={}s",
                self.timeout.as_secs().max(1)
            ))
            .args(&args)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|error| {
            CampusKubeError::Unreachable(format!("cannot run kubectl: {error}"))
        })?;
        if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
            pipe.write_all(&input)
                .await
                .map_err(|error| CampusKubeError::Unreachable(format!("kubectl stdin: {error}")))?;
        }
        let output = tokio::time::timeout(
            self.timeout + Duration::from_secs(5),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| CampusKubeError::Unreachable("kubectl timed out".into()))?
        .map_err(|error| CampusKubeError::Unreachable(format!("kubectl failed: {error}")))?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(classify_kubectl_error(&String::from_utf8_lossy(
                &output.stderr,
            )))
        }
    }
}

impl KubectlCampusKubeApi {
    /// The manifest goes on stdin, never on the command line, because a token
    /// Secret carries the raw token.
    async fn submit(&self, verb: &str, manifest: &Value) -> Result<Value, CampusKubeError> {
        let body = serde_json::to_vec(manifest)
            .map_err(|error| CampusKubeError::Failed(error.to_string()))?;
        let stdout = self
            .run(
                vec![
                    verb.into(),
                    "-f".into(),
                    "-".into(),
                    "-o".into(),
                    "json".into(),
                ],
                Some(body),
            )
            .await?;
        parse_json(&stdout)
    }
}

pub fn get_args(kind: CampusKubeKind, namespace: &str, name: &str) -> Vec<String> {
    let mut args = vec!["get".into(), kind.resource().into(), name.into()];
    if kind.namespaced() {
        args.extend(["-n".into(), namespace.into()]);
    }
    args.extend(["-o".into(), "json".into(), "--ignore-not-found".into()]);
    args
}

pub fn list_args(kind: CampusKubeKind, namespace: &str, selector: &str) -> Vec<String> {
    vec![
        "get".into(),
        kind.resource().into(),
        "-n".into(),
        namespace.into(),
        "-l".into(),
        selector.into(),
        "-o".into(),
        "json".into(),
    ]
}

pub fn delete_args(kind: CampusKubeKind, namespace: &str, name: &str) -> Vec<String> {
    vec![
        "delete".into(),
        kind.resource().into(),
        name.into(),
        "-n".into(),
        namespace.into(),
        "--ignore-not-found".into(),
        "--wait=false".into(),
        "--cascade=background".into(),
    ]
}

/// Map kubectl's stderr to a typed error. Anything that does not prove the
/// server answered is treated as unreachable, so callers retry rather than
/// conclude.
pub fn classify_kubectl_error(stderr: &str) -> CampusKubeError {
    let message = stderr.trim().chars().take(2048).collect::<String>();
    if message.contains("(AlreadyExists)") || message.contains("already exists") {
        return CampusKubeError::AlreadyExists(message);
    }
    let rejected = [
        "(Forbidden)",
        "(Invalid)",
        "denied request",
        "is forbidden",
        "admission webhook",
        "ValidatingAdmissionPolicy",
        "exceeded quota",
    ];
    if rejected.iter().any(|needle| message.contains(needle)) {
        return CampusKubeError::Rejected(message);
    }
    let unreachable = [
        "Unable to connect to the server",
        "connection refused",
        "i/o timeout",
        "TLS handshake timeout",
        "context deadline exceeded",
        "Client.Timeout",
        "the server is currently unable to handle the request",
        "(ServiceUnavailable)",
        "(Timeout)",
        "no route to host",
        "net/http",
    ];
    if message.is_empty() || unreachable.iter().any(|needle| message.contains(needle)) {
        return CampusKubeError::Unreachable(message);
    }
    CampusKubeError::Failed(message)
}

fn parse_json(bytes: &[u8]) -> Result<Value, CampusKubeError> {
    serde_json::from_slice(bytes)
        .map_err(|error| CampusKubeError::Failed(format!("kubectl returned invalid JSON: {error}")))
}

#[async_trait]
impl CampusKubeApi for KubectlCampusKubeApi {
    async fn get(
        &self,
        kind: CampusKubeKind,
        namespace: &str,
        name: &str,
    ) -> Result<Option<Value>, CampusKubeError> {
        let stdout = self.run(get_args(kind, namespace, name), None).await?;
        if stdout.iter().all(u8::is_ascii_whitespace) {
            return Ok(None);
        }
        parse_json(&stdout).map(Some)
    }

    async fn list(
        &self,
        kind: CampusKubeKind,
        namespace: &str,
        selector: &str,
    ) -> Result<Vec<Value>, CampusKubeError> {
        let stdout = self.run(list_args(kind, namespace, selector), None).await?;
        let list = parse_json(&stdout)?;
        Ok(list
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    async fn create(&self, manifest: &Value) -> Result<Value, CampusKubeError> {
        self.submit("create", manifest).await
    }

    async fn replace(&self, manifest: &Value) -> Result<Value, CampusKubeError> {
        self.submit("replace", manifest).await
    }

    async fn delete(
        &self,
        kind: CampusKubeKind,
        namespace: &str,
        name: &str,
    ) -> Result<(), CampusKubeError> {
        self.run(delete_args(kind, namespace, name), None)
            .await
            .map(|_| ())
    }
}
