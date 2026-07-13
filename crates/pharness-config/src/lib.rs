#![forbid(unsafe_code)]

use anyhow::{bail, Context};
use pharness_core::{PolicyMode, ReadOnlyClusterTools, SafetyPolicy};
use secrecy::SecretString;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

const DEFAULT_BIND: &str = "127.0.0.1:4777";
const DEFAULT_DB_PATH: &str = ".pharness/pharness.db";
const DEFAULT_FIREWORKS_MODEL: &str = "accounts/fireworks/models/kimi-k2p6";
const DEFAULT_FIREWORKS_BASE_URL: &str = pharness_fireworks::DEFAULT_FIREWORKS_BASE_URL;
const DEFAULT_FIREWORKS_API_KEY_ENV: &str = "FIREWORKS_API_KEY";
const DEFAULT_KUBECTL_BIN: &str = "kubectl";
const DEFAULT_ARGOCD_NAMESPACE: &str = "argocd";
const DEFAULT_CLUSTER_TIMEOUT_MS: u64 = 15_000;
const DEFAULT_WORKER_K8S_NAMESPACE: &str = "pharness";
const DEFAULT_WORKER_K8S_IMAGE: &str = "registry.lucas.engineering/pharness-runtime:latest";
const DEFAULT_WORKER_K8S_SERVICE_ACCOUNT: &str = "pharness-worker";
const DEFAULT_TEKTON_EXECUTOR_SERVICE_ACCOUNT: &str = "pharness-tekton-runner";
const DEFAULT_WORKER_K8S_API_URL: &str = "http://pharness-api:4777";
const DEFAULT_WORKER_K8S_WORKSPACE_DIR: &str = "/workspace";
const DEFAULT_WORKER_K8S_FIREWORKS_SECRET: &str = "pharness-fireworks";
const DEFAULT_WORKER_K8S_TOKEN_SECRET: &str = "pharness-worker-token";
const DEFAULT_WORKER_K8S_ACTIVE_DEADLINE_SECONDS: u64 = 3_600;
const DEFAULT_WORKER_K8S_TTL_SECONDS: u64 = 3_600;
const DEFAULT_TEKTON_EXECUTOR_POLL_SECONDS: u64 = 5;
const DEFAULT_CLUSTER_MAX_OUTPUT_BYTES: usize = 512 * 1024;

#[derive(Clone)]
pub struct ApiRuntimeConfig {
    pub api: ApiConfig,
    pub storage: StorageConfig,
    pub model: ModelConfig,
    pub cluster: ClusterConfig,
    pub policy: SafetyPolicy,
    pub worker: WorkerConfig,
}

#[derive(Clone)]
pub struct WorkerConfig {
    pub mode: WorkerMode,
    pub kubernetes: WorkerKubernetesConfig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerMode {
    Local,
    KubernetesJob,
}

impl WorkerMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::KubernetesJob => "kubernetes_job",
        }
    }
}

impl std::str::FromStr for WorkerMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "kubernetes_job" => Ok(Self::KubernetesJob),
            other => Err(format!(
                "unsupported worker mode {other:?}; expected local or kubernetes_job"
            )),
        }
    }
}

#[derive(Clone)]
pub struct WorkerKubernetesConfig {
    pub namespace: String,
    pub image: String,
    pub service_account: String,
    pub tekton_executor_service_account: String,
    pub tekton_allowed_namespaces: Vec<String>,
    pub tekton_executor_poll_seconds: u64,
    pub api_url: String,
    pub workspace_dir: String,
    pub fireworks_secret_name: String,
    pub worker_token_secret_name: String,
    pub active_deadline_seconds: u64,
    pub ttl_seconds_after_finished: u64,
}

#[derive(Clone)]
pub struct ApiConfig {
    pub bind: SocketAddr,
}

#[derive(Clone)]
pub struct StorageConfig {
    pub path: PathBuf,
}

#[derive(Clone)]
pub struct ModelConfig {
    pub provider: String,
    pub model: String,
    pub api_key_env: String,
    pub api_key: Option<SecretString>,
    pub base_url: String,
}

#[derive(Clone)]
pub struct ClusterConfig {
    pub kubectl_bin: String,
    pub argocd_namespace: String,
    pub prometheus_url: Option<String>,
    pub loki_url: Option<String>,
    pub registry_aliases: Vec<String>,
    pub timeout_ms: u64,
    pub max_output_bytes: usize,
}

impl ApiRuntimeConfig {
    pub fn load_from_env() -> anyhow::Result<Self> {
        let env = capture_env();
        let explicit_path = env.get("PHARNESS_CONFIG").map(PathBuf::from);
        let config_path = explicit_path
            .clone()
            .or_else(|| default_config_path().exists().then(default_config_path));

        match (explicit_path, config_path) {
            (Some(path), _) => Self::from_sources(Some(&path), &env),
            (None, Some(path)) => Self::from_sources(Some(&path), &env),
            (None, None) => Self::from_sources(None, &env),
        }
    }

    pub fn load_path_with_env(path: &Path) -> anyhow::Result<Self> {
        Self::from_sources(Some(path), &capture_env())
    }

    pub fn cluster_tools(&self) -> ReadOnlyClusterTools {
        ReadOnlyClusterTools::default()
            .with_kubectl_bin(self.cluster.kubectl_bin.clone())
            .with_argocd_namespace(self.cluster.argocd_namespace.clone())
            .with_prometheus_url_option(self.cluster.prometheus_url.clone())
            .with_loki_url_option(self.cluster.loki_url.clone())
            .with_registry_aliases(self.cluster.registry_aliases.join(","))
            .with_timeout_ms(self.cluster.timeout_ms)
            .with_max_output_bytes(self.cluster.max_output_bytes)
    }

    pub fn from_sources(
        config_path: Option<&Path>,
        env: &BTreeMap<String, String>,
    ) -> anyhow::Result<Self> {
        let file = config_path
            .map(read_config_file)
            .transpose()?
            .unwrap_or_default();
        let mut config = Self::defaults()?;

        config.apply_file(file)?;
        config.apply_env(env)?;
        reject_non_fireworks(&config.model.provider)?;
        reject_blank_policy_identity(&config.policy)?;
        config.resolve_api_key(env);

        Ok(config)
    }

    fn defaults() -> anyhow::Result<Self> {
        Ok(Self {
            api: ApiConfig {
                bind: parse_socket_addr(DEFAULT_BIND, "default api.bind")?,
            },
            storage: StorageConfig {
                path: PathBuf::from(DEFAULT_DB_PATH),
            },
            model: ModelConfig {
                provider: "fireworks".to_string(),
                model: DEFAULT_FIREWORKS_MODEL.to_string(),
                api_key_env: DEFAULT_FIREWORKS_API_KEY_ENV.to_string(),
                api_key: None,
                base_url: DEFAULT_FIREWORKS_BASE_URL.to_string(),
            },
            cluster: ClusterConfig {
                kubectl_bin: DEFAULT_KUBECTL_BIN.to_string(),
                argocd_namespace: DEFAULT_ARGOCD_NAMESPACE.to_string(),
                prometheus_url: None,
                loki_url: None,
                registry_aliases: Vec::new(),
                timeout_ms: DEFAULT_CLUSTER_TIMEOUT_MS,
                max_output_bytes: DEFAULT_CLUSTER_MAX_OUTPUT_BYTES,
            },
            policy: SafetyPolicy::default(),
            worker: WorkerConfig {
                mode: WorkerMode::Local,
                kubernetes: WorkerKubernetesConfig {
                    namespace: DEFAULT_WORKER_K8S_NAMESPACE.to_string(),
                    image: DEFAULT_WORKER_K8S_IMAGE.to_string(),
                    service_account: DEFAULT_WORKER_K8S_SERVICE_ACCOUNT.to_string(),
                    tekton_executor_service_account: DEFAULT_TEKTON_EXECUTOR_SERVICE_ACCOUNT
                        .to_string(),
                    tekton_allowed_namespaces: Vec::new(),
                    tekton_executor_poll_seconds: DEFAULT_TEKTON_EXECUTOR_POLL_SECONDS,
                    api_url: DEFAULT_WORKER_K8S_API_URL.to_string(),
                    workspace_dir: DEFAULT_WORKER_K8S_WORKSPACE_DIR.to_string(),
                    fireworks_secret_name: DEFAULT_WORKER_K8S_FIREWORKS_SECRET.to_string(),
                    worker_token_secret_name: DEFAULT_WORKER_K8S_TOKEN_SECRET.to_string(),
                    active_deadline_seconds: DEFAULT_WORKER_K8S_ACTIVE_DEADLINE_SECONDS,
                    ttl_seconds_after_finished: DEFAULT_WORKER_K8S_TTL_SECONDS,
                },
            },
        })
    }

    fn apply_file(&mut self, file: FileConfig) -> anyhow::Result<()> {
        if let Some(api) = file.api {
            if let Some(bind) = api.bind {
                self.api.bind = parse_socket_addr(&bind, "api.bind")?;
            }
        }

        if let Some(storage) = file.storage {
            if let Some(path) = storage.path {
                self.storage.path = expand_tilde(PathBuf::from(path));
            }
        }

        if let Some(model) = file.model {
            if let Some(provider) = model.provider {
                self.model.provider = provider;
            }
            if let Some(value) = model.model {
                self.model.model = value;
            }
            if let Some(value) = model.api_key_env {
                self.model.api_key_env = value;
            }
            if let Some(value) = model.base_url {
                self.model.base_url = value;
            }
        }

        if let Some(cluster) = file.cluster {
            if let Some(value) = cluster.kubectl_bin {
                self.cluster.kubectl_bin = value;
            }
            if let Some(value) = cluster.argocd_namespace {
                self.cluster.argocd_namespace = value;
            }
            if let Some(value) = cluster.prometheus_url {
                self.cluster.prometheus_url = blank_to_none(value);
            }
            if let Some(value) = cluster.loki_url {
                self.cluster.loki_url = blank_to_none(value);
            }
            if let Some(value) = cluster.registry_aliases {
                self.cluster.registry_aliases = value;
            }
            if let Some(value) = cluster.tool_timeout_ms {
                self.cluster.timeout_ms = value;
            }
            if let Some(value) = cluster.tool_max_output_bytes {
                self.cluster.max_output_bytes = value;
            }
        }

        if let Some(worker) = file.worker {
            if let Some(value) = worker.mode {
                self.worker.mode = value
                    .parse()
                    .map_err(|error: String| anyhow::anyhow!("worker.mode {error}"))?;
            }
            if let Some(kubernetes) = worker.kubernetes {
                if let Some(value) = kubernetes.namespace {
                    self.worker.kubernetes.namespace = value;
                }
                if let Some(value) = kubernetes.image {
                    self.worker.kubernetes.image = value;
                }
                if let Some(value) = kubernetes.service_account {
                    self.worker.kubernetes.service_account = value;
                }
                if let Some(value) = kubernetes.tekton_executor_service_account {
                    self.worker.kubernetes.tekton_executor_service_account = value;
                }
                if let Some(value) = kubernetes.tekton_allowed_namespaces {
                    self.worker.kubernetes.tekton_allowed_namespaces = value;
                }
                if let Some(value) = kubernetes.tekton_executor_poll_seconds {
                    self.worker.kubernetes.tekton_executor_poll_seconds = value;
                }
                if let Some(value) = kubernetes.api_url {
                    self.worker.kubernetes.api_url = value;
                }
                if let Some(value) = kubernetes.workspace_dir {
                    self.worker.kubernetes.workspace_dir = value;
                }
                if let Some(value) = kubernetes.fireworks_secret_name {
                    self.worker.kubernetes.fireworks_secret_name = value;
                }
                if let Some(value) = kubernetes.worker_token_secret_name {
                    self.worker.kubernetes.worker_token_secret_name = value;
                }
                if let Some(value) = kubernetes.active_deadline_seconds {
                    self.worker.kubernetes.active_deadline_seconds = value;
                }
                if let Some(value) = kubernetes.ttl_seconds_after_finished {
                    self.worker.kubernetes.ttl_seconds_after_finished = value;
                }
            }
        }

        if let Some(policy) = file.policy {
            if let Some(value) = policy.subject {
                self.policy.subject = value;
            }
            if let Some(value) = policy.environment {
                self.policy.environment = value;
            }
            if let Some(value) = policy.mode {
                self.policy.mode = value;
            }
            if let Some(value) = policy.allow_read_only_shell {
                self.policy.allow_read_only_shell = value;
            }
            if let Some(value) = policy.require_approval_for_writes {
                self.policy.require_approval_for_writes = value;
            }
            if let Some(value) = policy.require_approval_for_network {
                self.policy.require_approval_for_network = value;
            }
            if let Some(value) = policy.require_approval_for_destructive {
                self.policy.require_approval_for_destructive = value;
            }
            if let Some(value) = policy.deny_privileged {
                self.policy.deny_privileged = value;
            }
            if let Some(value) = policy.deny_secret_access {
                self.policy.deny_secret_access = value;
            }
        }

        Ok(())
    }

    fn apply_env(&mut self, env: &BTreeMap<String, String>) -> anyhow::Result<()> {
        if let Some(value) = env.get("PHARNESS_BIND") {
            self.api.bind = parse_socket_addr(value, "PHARNESS_BIND")?;
        }
        if let Some(value) = env.get("PHARNESS_DB_PATH") {
            self.storage.path = expand_tilde(PathBuf::from(value));
        }
        if let Some(value) = env.get("PHARNESS_FIREWORKS_MODEL") {
            self.model.model = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_FIREWORKS_BASE_URL") {
            self.model.base_url = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_FIREWORKS_API_KEY_ENV") {
            self.model.api_key_env = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_KUBECTL_BIN") {
            self.cluster.kubectl_bin = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_ARGOCD_NAMESPACE") {
            self.cluster.argocd_namespace = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_PROMETHEUS_URL") {
            self.cluster.prometheus_url = blank_to_none(value.clone());
        }
        if let Some(value) = env.get("PHARNESS_LOKI_URL") {
            self.cluster.loki_url = blank_to_none(value.clone());
        }
        if let Some(value) = env.get("PHARNESS_REGISTRY_ALIASES") {
            self.cluster.registry_aliases = split_registry_aliases(value);
        }
        if let Some(value) = env.get("PHARNESS_CLUSTER_TOOL_TIMEOUT_MS") {
            self.cluster.timeout_ms = parse_u64(value, "PHARNESS_CLUSTER_TOOL_TIMEOUT_MS")?;
        }
        if let Some(value) = env.get("PHARNESS_CLUSTER_TOOL_MAX_OUTPUT_BYTES") {
            self.cluster.max_output_bytes =
                parse_usize(value, "PHARNESS_CLUSTER_TOOL_MAX_OUTPUT_BYTES")?;
        }
        if let Some(value) = env.get("PHARNESS_WORKER_MODE") {
            self.worker.mode = value
                .parse()
                .map_err(|error: String| anyhow::anyhow!("PHARNESS_WORKER_MODE {error}"))?;
        }
        if let Some(value) = env.get("PHARNESS_WORKER_K8S_NAMESPACE") {
            self.worker.kubernetes.namespace = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_WORKER_K8S_IMAGE") {
            self.worker.kubernetes.image = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_WORKER_K8S_SERVICE_ACCOUNT") {
            self.worker.kubernetes.service_account = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_TEKTON_EXECUTOR_SERVICE_ACCOUNT") {
            self.worker.kubernetes.tekton_executor_service_account = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_TEKTON_ALLOWED_NAMESPACES") {
            self.worker.kubernetes.tekton_allowed_namespaces = value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect();
        }
        if let Some(value) = env.get("PHARNESS_TEKTON_EXECUTOR_POLL_SECONDS") {
            self.worker.kubernetes.tekton_executor_poll_seconds =
                parse_u64(value, "PHARNESS_TEKTON_EXECUTOR_POLL_SECONDS")?;
        }
        if let Some(value) = env.get("PHARNESS_WORKER_K8S_API_URL") {
            self.worker.kubernetes.api_url = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_WORKER_K8S_WORKSPACE_DIR") {
            self.worker.kubernetes.workspace_dir = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_WORKER_K8S_FIREWORKS_SECRET") {
            self.worker.kubernetes.fireworks_secret_name = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_WORKER_K8S_TOKEN_SECRET") {
            self.worker.kubernetes.worker_token_secret_name = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_WORKER_K8S_ACTIVE_DEADLINE_SECONDS") {
            self.worker.kubernetes.active_deadline_seconds =
                parse_u64(value, "PHARNESS_WORKER_K8S_ACTIVE_DEADLINE_SECONDS")?;
        }
        if let Some(value) = env.get("PHARNESS_WORKER_K8S_TTL_SECONDS") {
            self.worker.kubernetes.ttl_seconds_after_finished =
                parse_u64(value, "PHARNESS_WORKER_K8S_TTL_SECONDS")?;
        }
        if let Some(value) = env.get("PHARNESS_POLICY_MODE") {
            self.policy.mode = parse_policy_mode(value, "PHARNESS_POLICY_MODE")?;
        }
        if let Some(value) = env.get("PHARNESS_POLICY_SUBJECT") {
            self.policy.subject = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_POLICY_ENVIRONMENT") {
            self.policy.environment = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_ALLOW_READ_ONLY_SHELL") {
            self.policy.allow_read_only_shell =
                parse_bool(value, "PHARNESS_ALLOW_READ_ONLY_SHELL")?;
        }
        if let Some(value) = env.get("PHARNESS_REQUIRE_APPROVAL_FOR_WRITES") {
            self.policy.require_approval_for_writes =
                parse_bool(value, "PHARNESS_REQUIRE_APPROVAL_FOR_WRITES")?;
        }
        if let Some(value) = env.get("PHARNESS_REQUIRE_APPROVAL_FOR_NETWORK") {
            self.policy.require_approval_for_network =
                parse_bool(value, "PHARNESS_REQUIRE_APPROVAL_FOR_NETWORK")?;
        }
        if let Some(value) = env.get("PHARNESS_REQUIRE_APPROVAL_FOR_DESTRUCTIVE") {
            self.policy.require_approval_for_destructive =
                parse_bool(value, "PHARNESS_REQUIRE_APPROVAL_FOR_DESTRUCTIVE")?;
        }
        if let Some(value) = env.get("PHARNESS_DENY_PRIVILEGED") {
            self.policy.deny_privileged = parse_bool(value, "PHARNESS_DENY_PRIVILEGED")?;
        }
        if let Some(value) = env.get("PHARNESS_DENY_SECRET_ACCESS") {
            self.policy.deny_secret_access = parse_bool(value, "PHARNESS_DENY_SECRET_ACCESS")?;
        }

        Ok(())
    }

    fn resolve_api_key(&mut self, env: &BTreeMap<String, String>) {
        self.model.api_key = env
            .get(DEFAULT_FIREWORKS_API_KEY_ENV)
            .or_else(|| env.get(&self.model.api_key_env))
            .cloned()
            .map(SecretString::new);
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileConfig {
    api: Option<FileApiConfig>,
    storage: Option<FileStorageConfig>,
    model: Option<FileModelConfig>,
    cluster: Option<FileClusterConfig>,
    policy: Option<FilePolicyConfig>,
    worker: Option<FileWorkerConfig>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileWorkerConfig {
    mode: Option<String>,
    kubernetes: Option<FileWorkerKubernetesConfig>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileWorkerKubernetesConfig {
    namespace: Option<String>,
    image: Option<String>,
    service_account: Option<String>,
    tekton_executor_service_account: Option<String>,
    tekton_allowed_namespaces: Option<Vec<String>>,
    tekton_executor_poll_seconds: Option<u64>,
    api_url: Option<String>,
    workspace_dir: Option<String>,
    fireworks_secret_name: Option<String>,
    worker_token_secret_name: Option<String>,
    active_deadline_seconds: Option<u64>,
    ttl_seconds_after_finished: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileApiConfig {
    bind: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileStorageConfig {
    path: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileModelConfig {
    provider: Option<String>,
    model: Option<String>,
    api_key_env: Option<String>,
    base_url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileClusterConfig {
    kubectl_bin: Option<String>,
    argocd_namespace: Option<String>,
    prometheus_url: Option<String>,
    loki_url: Option<String>,
    registry_aliases: Option<Vec<String>>,
    tool_timeout_ms: Option<u64>,
    tool_max_output_bytes: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FilePolicyConfig {
    subject: Option<String>,
    environment: Option<String>,
    mode: Option<PolicyMode>,
    allow_read_only_shell: Option<bool>,
    require_approval_for_writes: Option<bool>,
    require_approval_for_network: Option<bool>,
    require_approval_for_destructive: Option<bool>,
    deny_privileged: Option<bool>,
    deny_secret_access: Option<bool>,
}

fn capture_env() -> BTreeMap<String, String> {
    std::env::vars().collect()
}

fn read_config_file(path: &Path) -> anyhow::Result<FileConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    toml::from_str(&content)
        .with_context(|| format!("failed to parse config file {}", path.display()))
}

fn default_config_path() -> PathBuf {
    PathBuf::from("config/pharness.toml")
}

fn parse_socket_addr(value: &str, label: &str) -> anyhow::Result<SocketAddr> {
    value
        .parse()
        .with_context(|| format!("{label} must be a socket address"))
}

fn parse_u64(value: &str, label: &str) -> anyhow::Result<u64> {
    value
        .parse()
        .with_context(|| format!("{label} must be an unsigned integer"))
}

fn parse_usize(value: &str, label: &str) -> anyhow::Result<usize> {
    value
        .parse()
        .with_context(|| format!("{label} must be an unsigned integer"))
}

fn parse_bool(value: &str, label: &str) -> anyhow::Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "y" | "on" => Ok(true),
        "false" | "0" | "no" | "n" | "off" => Ok(false),
        _ => bail!("{label} must be a boolean"),
    }
}

fn parse_policy_mode(value: &str, label: &str) -> anyhow::Result<PolicyMode> {
    value
        .parse::<PolicyMode>()
        .map_err(|error| anyhow::anyhow!("{label} {error}"))
}

fn expand_tilde(path: PathBuf) -> PathBuf {
    let Some(path_str) = path.to_str() else {
        return path;
    };
    if path_str == "~" {
        return std::env::var("HOME").map(PathBuf::from).unwrap_or(path);
    }
    let Some(rest) = path_str.strip_prefix("~/") else {
        return path;
    };
    std::env::var("HOME")
        .map(|home| PathBuf::from(home).join(rest))
        .unwrap_or(path)
}

fn blank_to_none(value: String) -> Option<String> {
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn split_registry_aliases(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn reject_non_fireworks(provider: &str) -> anyhow::Result<()> {
    if provider == "fireworks" {
        return Ok(());
    }

    bail!("only the fireworks model provider is supported in V1")
}

fn reject_blank_policy_identity(policy: &SafetyPolicy) -> anyhow::Result<()> {
    if policy.subject.trim().is_empty() {
        bail!("policy.subject must not be blank");
    }
    if policy.environment.trim().is_empty() {
        bail!("policy.environment must not be blank");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{split_registry_aliases, ApiRuntimeConfig};
    use pharness_core::PolicyMode;
    use secrecy::ExposeSecret;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn missing_config_uses_defaults() {
        let config = ApiRuntimeConfig::from_sources(None, &BTreeMap::new()).unwrap();

        assert_eq!(config.api.bind.to_string(), "127.0.0.1:4777");
        assert_eq!(config.storage.path, PathBuf::from(".pharness/pharness.db"));
        assert_eq!(config.model.model, "accounts/fireworks/models/kimi-k2p6");
        assert_eq!(config.cluster.argocd_namespace, "argocd");
        assert!(config.cluster.prometheus_url.is_none());
        assert!(config.cluster.loki_url.is_none());
        assert!(config.cluster.registry_aliases.is_empty());
        assert_eq!(config.policy.mode, PolicyMode::Default);
        assert_eq!(config.policy.environment, "local");
        assert!(config.policy.require_approval_for_writes);
        assert!(config.model.api_key.is_none());
    }

    #[test]
    fn parses_toml_config_values() {
        let path = write_temp_config(
            r#"
[api]
bind = "127.0.0.1:4888"

[storage]
path = ".pharness/test.db"

[model]
model = "accounts/fireworks/models/test-model"
api_key_env = "CUSTOM_FIREWORKS_API_KEY"
base_url = "https://example.test/v1"

[cluster]
kubectl_bin = "kubectl-test"
argocd_namespace = "argo-system"
prometheus_url = "http://prometheus.test"
loki_url = "http://loki.test"
registry_aliases = ["internal.registry=external.registry"]
tool_timeout_ms = 2222
tool_max_output_bytes = 3333

[worker.kubernetes]
tekton_executor_poll_seconds = 9

[policy]
subject = "agent:config-test"
environment = "dev"
mode = "trusted_writes"
allow_read_only_shell = false
require_approval_for_writes = false
require_approval_for_network = false
require_approval_for_destructive = true
deny_privileged = true
deny_secret_access = true
"#,
        );
        let mut env = BTreeMap::new();
        env.insert(
            "CUSTOM_FIREWORKS_API_KEY".to_string(),
            "custom-key".to_string(),
        );

        let config = ApiRuntimeConfig::from_sources(Some(&path), &env).unwrap();

        assert_eq!(config.api.bind.to_string(), "127.0.0.1:4888");
        assert_eq!(config.storage.path, PathBuf::from(".pharness/test.db"));
        assert_eq!(config.model.model, "accounts/fireworks/models/test-model");
        assert_eq!(config.model.base_url, "https://example.test/v1");
        assert_eq!(
            config.model.api_key.as_ref().unwrap().expose_secret(),
            "custom-key"
        );
        assert_eq!(config.cluster.kubectl_bin, "kubectl-test");
        assert_eq!(config.cluster.argocd_namespace, "argo-system");
        assert_eq!(
            config.cluster.prometheus_url.as_deref(),
            Some("http://prometheus.test")
        );
        assert_eq!(config.cluster.loki_url.as_deref(), Some("http://loki.test"));
        assert_eq!(
            config.cluster.registry_aliases,
            vec!["internal.registry=external.registry"]
        );
        assert_eq!(config.cluster.timeout_ms, 2222);
        assert_eq!(config.cluster.max_output_bytes, 3333);
        assert_eq!(config.worker.kubernetes.tekton_executor_poll_seconds, 9);
        assert_eq!(config.policy.subject, "agent:config-test");
        assert_eq!(config.policy.environment, "dev");
        assert_eq!(config.policy.mode, PolicyMode::TrustedWrites);
        assert!(!config.policy.allow_read_only_shell);
        assert!(!config.policy.require_approval_for_writes);
        assert!(!config.policy.require_approval_for_network);
        assert!(config.policy.require_approval_for_destructive);
        assert!(config.policy.deny_privileged);
        assert!(config.policy.deny_secret_access);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn env_overrides_toml_config_values() {
        let path = write_temp_config(
            r#"
[api]
bind = "127.0.0.1:4888"

[storage]
path = ".pharness/from-file.db"

[model]
model = "accounts/fireworks/models/from-file"
base_url = "https://file.example/v1"

[cluster]
argocd_namespace = "from-file"
registry_aliases = ["file.registry=public.registry"]
"#,
        );
        let mut env = BTreeMap::new();
        env.insert("PHARNESS_BIND".to_string(), "127.0.0.1:4999".to_string());
        env.insert(
            "PHARNESS_DB_PATH".to_string(),
            ".pharness/from-env.db".to_string(),
        );
        env.insert(
            "PHARNESS_FIREWORKS_MODEL".to_string(),
            "accounts/fireworks/models/from-env".to_string(),
        );
        env.insert(
            "PHARNESS_FIREWORKS_BASE_URL".to_string(),
            "https://env.example/v1".to_string(),
        );
        env.insert(
            "PHARNESS_ARGOCD_NAMESPACE".to_string(),
            "from-env".to_string(),
        );
        env.insert(
            "PHARNESS_LOKI_URL".to_string(),
            "http://loki.env".to_string(),
        );
        env.insert(
            "PHARNESS_REGISTRY_ALIASES".to_string(),
            "env.registry=public.registry".to_string(),
        );
        env.insert(
            "PHARNESS_TEKTON_EXECUTOR_POLL_SECONDS".to_string(),
            "11".to_string(),
        );
        env.insert("PHARNESS_POLICY_MODE".to_string(), "plan".to_string());
        env.insert(
            "PHARNESS_POLICY_SUBJECT".to_string(),
            "agent:env".to_string(),
        );
        env.insert("PHARNESS_POLICY_ENVIRONMENT".to_string(), "ci".to_string());
        env.insert(
            "PHARNESS_ALLOW_READ_ONLY_SHELL".to_string(),
            "false".to_string(),
        );
        env.insert(
            "PHARNESS_REQUIRE_APPROVAL_FOR_WRITES".to_string(),
            "0".to_string(),
        );
        env.insert("FIREWORKS_API_KEY".to_string(), "env-key".to_string());

        let config = ApiRuntimeConfig::from_sources(Some(&path), &env).unwrap();

        assert_eq!(config.api.bind.to_string(), "127.0.0.1:4999");
        assert_eq!(config.storage.path, PathBuf::from(".pharness/from-env.db"));
        assert_eq!(config.model.model, "accounts/fireworks/models/from-env");
        assert_eq!(config.model.base_url, "https://env.example/v1");
        assert_eq!(
            config.model.api_key.as_ref().unwrap().expose_secret(),
            "env-key"
        );
        assert_eq!(config.cluster.argocd_namespace, "from-env");
        assert_eq!(config.cluster.loki_url.as_deref(), Some("http://loki.env"));
        assert_eq!(
            config.cluster.registry_aliases,
            vec!["env.registry=public.registry"]
        );
        assert_eq!(config.worker.kubernetes.tekton_executor_poll_seconds, 11);
        assert_eq!(config.policy.subject, "agent:env");
        assert_eq!(config.policy.environment, "ci");
        assert_eq!(config.policy.mode, PolicyMode::Plan);
        assert!(!config.policy.allow_read_only_shell);
        assert!(!config.policy.require_approval_for_writes);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_unsupported_provider() {
        let path = write_temp_config(
            r#"
[model]
provider = "not-fireworks"
"#,
        );

        let error = ApiRuntimeConfig::from_sources(Some(&path), &BTreeMap::new())
            .err()
            .unwrap();

        assert!(error.to_string().contains("only the fireworks"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_blank_policy_environment() {
        let path = write_temp_config(
            r#"
[policy]
environment = " "
"#,
        );

        let error = ApiRuntimeConfig::from_sources(Some(&path), &BTreeMap::new())
            .err()
            .unwrap();

        assert!(error.to_string().contains("policy.environment"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn splits_registry_alias_env_value() {
        assert_eq!(
            split_registry_aliases("one=two, three=four ,, five=six"),
            vec!["one=two", "three=four", "five=six"]
        );
    }

    fn write_temp_config(content: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("pharness-config-{suffix}-{sequence}.toml"));
        fs::write(&path, content).unwrap();
        path
    }
}
