#![forbid(unsafe_code)]

use anyhow::{bail, Context};
use pharness_core::{
    ContextBudget, InferenceBackendKind, InferenceCapabilities, InferencePolicyRef,
    InferenceRegistry, InferenceStage, InferenceTargetRef, InferenceTargetRevision,
    InferenceTransportPolicy, PolicyMode, ReadOnlyClusterTools, ReasoningContextMode,
    ReasoningEffort, ReasoningRequestPolicy, SafetyPolicy, StageInferencePolicyRevision,
    ToolProtocolMode, INFERENCE_POLICY_SCHEMA, INFERENCE_REGISTRY_SCHEMA, INFERENCE_TARGET_SCHEMA,
};
use secrecy::SecretString;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

const DEFAULT_BIND: &str = "127.0.0.1:4777";
const DEFAULT_DB_PATH: &str = ".pharness/pharness.db";
const DEFAULT_WORKSPACE_ROOT: &str = ".pharness/workspaces";
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
const DEFAULT_ARGO_EXECUTOR_SERVICE_ACCOUNT: &str = "pharness-argo-runner";
const DEFAULT_GIT_WRITER_SERVICE_ACCOUNT: &str = "pharness-git-writer";
const DEFAULT_GITOPS_WRITER_SERVICE_ACCOUNT: &str = "pharness-gitops-writer";
const DEFAULT_GIT_OBSERVER_SERVICE_ACCOUNT: &str = "pharness-git-observer";
const DEFAULT_GITOPS_OBSERVER_SERVICE_ACCOUNT: &str = "pharness-gitops-observer";
const DEFAULT_SOURCE_READER_SERVICE_ACCOUNT: &str = "pharness-source-reader";
const DEFAULT_GITHUB_API_URL: &str = "https://api.github.com";
const DEFAULT_GIT_WRITER_AUTHOR_NAME: &str = "Pharness";
const DEFAULT_GIT_WRITER_AUTHOR_EMAIL: &str = "pharness@localhost";
const DEFAULT_GIT_WRITER_ACTIVE_DEADLINE_SECONDS: u64 = 900;
const DEFAULT_GIT_WRITER_TTL_SECONDS: u64 = 3_600;
const DEFAULT_WORKER_K8S_API_URL: &str = "http://pharness-api:4777";
const DEFAULT_WORKER_K8S_WORKSPACE_DIR: &str = "/workspace";
const DEFAULT_WORKER_K8S_WORKSPACE_SIZE_LIMIT: &str = "4Gi";
const DEFAULT_WORKER_K8S_WORKSPACE_EPHEMERAL_STORAGE_REQUEST: &str = "2Gi";
const DEFAULT_WORKER_K8S_WORKSPACE_EPHEMERAL_STORAGE_LIMIT: &str = "4Gi";
const DEFAULT_WORKER_K8S_MAX_CONCURRENT_RUN_JOBS: u32 = 1;
const DEFAULT_WORKER_K8S_FIREWORKS_SECRET: &str = "pharness-fireworks";
const DEFAULT_WORKER_K8S_TOKEN_SECRET: &str = "pharness-worker-token";
const DEFAULT_WORKER_K8S_ACTIVE_DEADLINE_SECONDS: u64 = 3_600;
const DEFAULT_WORKER_K8S_TTL_SECONDS: u64 = 3_600;
const DEFAULT_TEKTON_EXECUTOR_POLL_SECONDS: u64 = 5;
const DEFAULT_ARGO_EXECUTOR_POLL_SECONDS: u64 = 5;
const DEFAULT_ARGO_EXECUTOR_ACTIVE_DEADLINE_SECONDS: u64 = 600;
const DEFAULT_ARGO_EXECUTOR_TTL_SECONDS: u64 = 3_600;
const DEFAULT_CLUSTER_MAX_OUTPUT_BYTES: usize = 512 * 1024;
const DEFAULT_INFERENCE_GATEWAY_URL: &str = "http://pharness-model-gateway:4780/v1";
const DEFAULT_MODEL_GRANT_HMAC_KEY_ENV: &str = "PHARNESS_MODEL_GRANT_HMAC_KEY";

#[derive(Clone)]
pub struct ApiRuntimeConfig {
    pub api: ApiConfig,
    pub storage: StorageConfig,
    pub model: ModelConfig,
    pub cluster: ClusterConfig,
    pub policy: SafetyPolicy,
    pub worker: WorkerConfig,
    pub inference: InferenceGatewayConfig,
}

#[derive(Clone)]
pub struct InferenceGatewayConfig {
    pub enabled: bool,
    pub gateway_url: String,
    pub direct_fireworks_enabled: bool,
    pub grant_signing_enabled: bool,
    pub grant_hmac_key_env: String,
    pub grant_hmac_key: Option<SecretString>,
    pub registry: InferenceRegistry,
}

impl InferenceGatewayConfig {
    pub fn legacy_default() -> Self {
        Self {
            enabled: false,
            gateway_url: DEFAULT_INFERENCE_GATEWAY_URL.to_string(),
            direct_fireworks_enabled: true,
            grant_signing_enabled: true,
            grant_hmac_key_env: DEFAULT_MODEL_GRANT_HMAC_KEY_ENV.to_string(),
            grant_hmac_key: None,
            registry: default_inference_registry()
                .expect("compiled legacy inference registry must be valid"),
        }
    }
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
    /// A separately scoped, opt-in Argo CD sync executor. It is disabled by
    /// default and accepts only exact application names named in configuration.
    pub argo_executor_enabled: bool,
    pub argo_executor_service_account: String,
    pub argo_executor_namespace: String,
    pub argo_executor_allowed_applications: Vec<String>,
    pub argo_executor_poll_seconds: u64,
    pub argo_executor_active_deadline_seconds: u64,
    pub argo_executor_ttl_seconds_after_finished: u64,
    /// A separately credentialed, opt-in GitHub branch/PR executor. The API
    /// never receives this credential; it only references the Secret name in
    /// a purpose-built Job manifest.
    pub git_writer_enabled: bool,
    pub git_writer_service_account: String,
    pub git_writer_token_secret_name: Option<String>,
    pub git_writer_allowed_repos: Vec<String>,
    pub git_writer_github_api_url: String,
    pub git_writer_author_name: String,
    pub git_writer_author_email: String,
    pub git_writer_active_deadline_seconds: u64,
    pub git_writer_ttl_seconds_after_finished: u64,
    /// A separately credentialed writer for GitOps manifests. It never shares
    /// the source-code writer identity or repository allowlist.
    pub gitops_writer_enabled: bool,
    pub gitops_writer_service_account: String,
    pub gitops_writer_token_secret_name: Option<String>,
    pub gitops_writer_allowed_repos: Vec<String>,
    pub gitops_writer_github_api_url: String,
    pub gitops_writer_author_name: String,
    pub gitops_writer_author_email: String,
    pub gitops_writer_active_deadline_seconds: u64,
    pub gitops_writer_ttl_seconds_after_finished: u64,
    pub git_observer_enabled: bool,
    pub git_observer_service_account: String,
    pub git_observer_token_secret_name: Option<String>,
    pub git_observer_allowed_repos: Vec<String>,
    pub git_observer_github_api_url: String,
    pub git_observer_active_deadline_seconds: u64,
    pub git_observer_ttl_seconds_after_finished: u64,
    pub gitops_observer_enabled: bool,
    pub gitops_observer_service_account: String,
    pub gitops_observer_token_secret_name: Option<String>,
    pub gitops_observer_allowed_repos: Vec<String>,
    pub gitops_observer_github_api_url: String,
    pub gitops_observer_active_deadline_seconds: u64,
    pub gitops_observer_ttl_seconds_after_finished: u64,
    /// An isolated clone/discovery identity. Public repositories may omit a
    /// token; private repositories mount only this reader-scoped Secret.
    pub source_reader_enabled: bool,
    pub source_reader_service_account: String,
    pub source_reader_token_secret_name: Option<String>,
    pub source_reader_allowed_repos: Vec<String>,
    pub source_reader_active_deadline_seconds: u64,
    pub source_reader_ttl_seconds_after_finished: u64,
    pub api_url: String,
    pub workspace_dir: String,
    /// Per-run source workspace quota, independent of the API's SQLite PVC.
    pub workspace_size_limit: String,
    /// StorageClass used by the durable per-run workspace PVC. `None` lets
    /// Kubernetes select the cluster default.
    pub workspace_storage_class: Option<String>,
    pub workspace_ephemeral_storage_request: String,
    pub workspace_ephemeral_storage_limit: String,
    /// Optional hostname pin for node-local workspace capacity.
    pub workspace_node_hostname: Option<String>,
    /// Conservative admission cap for model worker Jobs.
    pub max_concurrent_run_jobs: u32,
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
    /// Root for ephemeral local source clones. An empty repository allowlist
    /// deliberately disables autonomous source execution.
    pub workspace_root: PathBuf,
    pub workspace_allowed_repos: Vec<PathBuf>,
    /// Exact HTTPS source repositories permitted for future Kubernetes
    /// workspace attempts. Empty keeps remote source execution disabled.
    pub workspace_allowed_remote_repos: Vec<String>,
}

#[derive(Clone)]
pub struct ModelConfig {
    pub provider: String,
    pub model: String,
    pub api_key_env: String,
    pub api_key: Option<SecretString>,
    pub base_url: String,
    pub context_budget: ContextBudget,
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
        reject_invalid_context_budget(&config.model.context_budget)?;
        reject_blank_policy_identity(&config.policy)?;
        reject_invalid_kubernetes_workspace(&config.worker.kubernetes)?;
        reject_invalid_argo_executor(&config.worker.kubernetes)?;
        reject_invalid_git_writer(&config.worker.kubernetes)?;
        reject_invalid_gitops_writer(&config.worker.kubernetes)?;
        reject_invalid_git_observer(&config.worker.kubernetes)?;
        reject_invalid_gitops_observer(&config.worker.kubernetes)?;
        reject_invalid_source_reader(&config.worker.kubernetes)?;
        config.inference.registry.finalize_hashes()?;
        config.resolve_api_key(env);
        if config.inference.enabled
            && config.inference.grant_signing_enabled
            && config.inference.grant_hmac_key.is_none()
        {
            bail!("enabled inference gateway requires its model-grant HMAC key");
        }
        if config.worker.mode == WorkerMode::KubernetesJob {
            reject_mutable_worker_image(&config.worker.kubernetes.image)?;
        }
        Ok(config)
    }

    fn defaults() -> anyhow::Result<Self> {
        Ok(Self {
            api: ApiConfig {
                bind: parse_socket_addr(DEFAULT_BIND, "default api.bind")?,
            },
            storage: StorageConfig {
                path: PathBuf::from(DEFAULT_DB_PATH),
                workspace_root: PathBuf::from(DEFAULT_WORKSPACE_ROOT),
                workspace_allowed_repos: Vec::new(),
                workspace_allowed_remote_repos: Vec::new(),
            },
            model: ModelConfig {
                provider: "fireworks".to_string(),
                model: DEFAULT_FIREWORKS_MODEL.to_string(),
                api_key_env: DEFAULT_FIREWORKS_API_KEY_ENV.to_string(),
                api_key: None,
                base_url: DEFAULT_FIREWORKS_BASE_URL.to_string(),
                context_budget: ContextBudget::default(),
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
                    argo_executor_enabled: false,
                    argo_executor_service_account: DEFAULT_ARGO_EXECUTOR_SERVICE_ACCOUNT
                        .to_string(),
                    argo_executor_namespace: DEFAULT_ARGOCD_NAMESPACE.to_string(),
                    argo_executor_allowed_applications: Vec::new(),
                    argo_executor_poll_seconds: DEFAULT_ARGO_EXECUTOR_POLL_SECONDS,
                    argo_executor_active_deadline_seconds:
                        DEFAULT_ARGO_EXECUTOR_ACTIVE_DEADLINE_SECONDS,
                    argo_executor_ttl_seconds_after_finished: DEFAULT_ARGO_EXECUTOR_TTL_SECONDS,
                    git_writer_enabled: false,
                    git_writer_service_account: DEFAULT_GIT_WRITER_SERVICE_ACCOUNT.to_string(),
                    git_writer_token_secret_name: None,
                    git_writer_allowed_repos: Vec::new(),
                    git_writer_github_api_url: DEFAULT_GITHUB_API_URL.to_string(),
                    git_writer_author_name: DEFAULT_GIT_WRITER_AUTHOR_NAME.to_string(),
                    git_writer_author_email: DEFAULT_GIT_WRITER_AUTHOR_EMAIL.to_string(),
                    git_writer_active_deadline_seconds: DEFAULT_GIT_WRITER_ACTIVE_DEADLINE_SECONDS,
                    git_writer_ttl_seconds_after_finished: DEFAULT_GIT_WRITER_TTL_SECONDS,
                    gitops_writer_enabled: false,
                    gitops_writer_service_account: DEFAULT_GITOPS_WRITER_SERVICE_ACCOUNT
                        .to_string(),
                    gitops_writer_token_secret_name: None,
                    gitops_writer_allowed_repos: Vec::new(),
                    gitops_writer_github_api_url: DEFAULT_GITHUB_API_URL.to_string(),
                    gitops_writer_author_name: DEFAULT_GIT_WRITER_AUTHOR_NAME.to_string(),
                    gitops_writer_author_email: DEFAULT_GIT_WRITER_AUTHOR_EMAIL.to_string(),
                    gitops_writer_active_deadline_seconds:
                        DEFAULT_GIT_WRITER_ACTIVE_DEADLINE_SECONDS,
                    gitops_writer_ttl_seconds_after_finished: DEFAULT_GIT_WRITER_TTL_SECONDS,
                    git_observer_enabled: false,
                    git_observer_service_account: DEFAULT_GIT_OBSERVER_SERVICE_ACCOUNT.to_string(),
                    git_observer_token_secret_name: None,
                    git_observer_allowed_repos: Vec::new(),
                    git_observer_github_api_url: DEFAULT_GITHUB_API_URL.to_string(),
                    git_observer_active_deadline_seconds:
                        DEFAULT_GIT_WRITER_ACTIVE_DEADLINE_SECONDS,
                    git_observer_ttl_seconds_after_finished: DEFAULT_GIT_WRITER_TTL_SECONDS,
                    gitops_observer_enabled: false,
                    gitops_observer_service_account: DEFAULT_GITOPS_OBSERVER_SERVICE_ACCOUNT
                        .to_string(),
                    gitops_observer_token_secret_name: None,
                    gitops_observer_allowed_repos: Vec::new(),
                    gitops_observer_github_api_url: DEFAULT_GITHUB_API_URL.to_string(),
                    gitops_observer_active_deadline_seconds:
                        DEFAULT_GIT_WRITER_ACTIVE_DEADLINE_SECONDS,
                    gitops_observer_ttl_seconds_after_finished: DEFAULT_GIT_WRITER_TTL_SECONDS,
                    source_reader_enabled: false,
                    source_reader_service_account: DEFAULT_SOURCE_READER_SERVICE_ACCOUNT
                        .to_string(),
                    source_reader_token_secret_name: None,
                    source_reader_allowed_repos: Vec::new(),
                    source_reader_active_deadline_seconds: 600,
                    source_reader_ttl_seconds_after_finished: DEFAULT_GIT_WRITER_TTL_SECONDS,
                    api_url: DEFAULT_WORKER_K8S_API_URL.to_string(),
                    workspace_dir: DEFAULT_WORKER_K8S_WORKSPACE_DIR.to_string(),
                    workspace_size_limit: DEFAULT_WORKER_K8S_WORKSPACE_SIZE_LIMIT.to_string(),
                    workspace_storage_class: None,
                    workspace_ephemeral_storage_request:
                        DEFAULT_WORKER_K8S_WORKSPACE_EPHEMERAL_STORAGE_REQUEST.to_string(),
                    workspace_ephemeral_storage_limit:
                        DEFAULT_WORKER_K8S_WORKSPACE_EPHEMERAL_STORAGE_LIMIT.to_string(),
                    workspace_node_hostname: None,
                    max_concurrent_run_jobs: DEFAULT_WORKER_K8S_MAX_CONCURRENT_RUN_JOBS,
                    fireworks_secret_name: DEFAULT_WORKER_K8S_FIREWORKS_SECRET.to_string(),
                    worker_token_secret_name: DEFAULT_WORKER_K8S_TOKEN_SECRET.to_string(),
                    active_deadline_seconds: DEFAULT_WORKER_K8S_ACTIVE_DEADLINE_SECONDS,
                    ttl_seconds_after_finished: DEFAULT_WORKER_K8S_TTL_SECONDS,
                },
            },
            inference: InferenceGatewayConfig::legacy_default(),
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
            if let Some(path) = storage.workspace_root {
                self.storage.workspace_root = expand_tilde(PathBuf::from(path));
            }
            if let Some(repos) = storage.workspace_allowed_repos {
                self.storage.workspace_allowed_repos = repos
                    .into_iter()
                    .map(|repo| expand_tilde(PathBuf::from(repo)))
                    .collect();
            }
            if let Some(repos) = storage.workspace_allowed_remote_repos {
                self.storage.workspace_allowed_remote_repos = repos;
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
            if let Some(value) = model.context_max_input_tokens {
                self.model.context_budget.max_input_tokens = value;
            }
            if let Some(value) = model.context_recent_message_tokens {
                self.model.context_budget.recent_message_tokens = value;
            }
            if let Some(value) = model.context_max_tool_result_tokens {
                self.model.context_budget.max_tool_result_tokens = value;
            }
        }

        if let Some(inference) = file.inference {
            if let Some(value) = inference.enabled {
                self.inference.enabled = value;
            }
            if let Some(value) = inference.gateway_url {
                self.inference.gateway_url = value;
            }
            if let Some(value) = inference.direct_fireworks_enabled {
                self.inference.direct_fireworks_enabled = value;
            }
            if let Some(value) = inference.grant_signing_enabled {
                self.inference.grant_signing_enabled = value;
            }
            if let Some(value) = inference.grant_hmac_key_env {
                self.inference.grant_hmac_key_env = value;
            }
            if let Some(value) = inference.registry_json {
                self.inference.registry = serde_json::from_str(&value)
                    .context("inference.registry_json must contain a valid registry")?;
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
                if let Some(value) = kubernetes.argo_executor_enabled {
                    self.worker.kubernetes.argo_executor_enabled = value;
                }
                if let Some(value) = kubernetes.argo_executor_service_account {
                    self.worker.kubernetes.argo_executor_service_account = value;
                }
                if let Some(value) = kubernetes.argo_executor_namespace {
                    self.worker.kubernetes.argo_executor_namespace = value;
                }
                if let Some(value) = kubernetes.argo_executor_allowed_applications {
                    self.worker.kubernetes.argo_executor_allowed_applications = value;
                }
                if let Some(value) = kubernetes.argo_executor_poll_seconds {
                    self.worker.kubernetes.argo_executor_poll_seconds = value;
                }
                if let Some(value) = kubernetes.argo_executor_active_deadline_seconds {
                    self.worker.kubernetes.argo_executor_active_deadline_seconds = value;
                }
                if let Some(value) = kubernetes.argo_executor_ttl_seconds_after_finished {
                    self.worker
                        .kubernetes
                        .argo_executor_ttl_seconds_after_finished = value;
                }
                if let Some(value) = kubernetes.git_writer_enabled {
                    self.worker.kubernetes.git_writer_enabled = value;
                }
                if let Some(value) = kubernetes.git_writer_service_account {
                    self.worker.kubernetes.git_writer_service_account = value;
                }
                if let Some(value) = kubernetes.git_writer_token_secret_name {
                    self.worker.kubernetes.git_writer_token_secret_name = blank_to_none(value);
                }
                if let Some(value) = kubernetes.git_writer_allowed_repos {
                    self.worker.kubernetes.git_writer_allowed_repos = value;
                }
                if let Some(value) = kubernetes.git_writer_github_api_url {
                    self.worker.kubernetes.git_writer_github_api_url = value;
                }
                if let Some(value) = kubernetes.git_writer_author_name {
                    self.worker.kubernetes.git_writer_author_name = value;
                }
                if let Some(value) = kubernetes.git_writer_author_email {
                    self.worker.kubernetes.git_writer_author_email = value;
                }
                if let Some(value) = kubernetes.git_writer_active_deadline_seconds {
                    self.worker.kubernetes.git_writer_active_deadline_seconds = value;
                }
                if let Some(value) = kubernetes.git_writer_ttl_seconds_after_finished {
                    self.worker.kubernetes.git_writer_ttl_seconds_after_finished = value;
                }
                if let Some(value) = kubernetes.gitops_writer_enabled {
                    self.worker.kubernetes.gitops_writer_enabled = value;
                }
                if let Some(value) = kubernetes.gitops_writer_service_account {
                    self.worker.kubernetes.gitops_writer_service_account = value;
                }
                if let Some(value) = kubernetes.gitops_writer_token_secret_name {
                    self.worker.kubernetes.gitops_writer_token_secret_name = blank_to_none(value);
                }
                if let Some(value) = kubernetes.gitops_writer_allowed_repos {
                    self.worker.kubernetes.gitops_writer_allowed_repos = value;
                }
                if let Some(value) = kubernetes.gitops_writer_github_api_url {
                    self.worker.kubernetes.gitops_writer_github_api_url = value;
                }
                if let Some(value) = kubernetes.gitops_writer_author_name {
                    self.worker.kubernetes.gitops_writer_author_name = value;
                }
                if let Some(value) = kubernetes.gitops_writer_author_email {
                    self.worker.kubernetes.gitops_writer_author_email = value;
                }
                if let Some(value) = kubernetes.gitops_writer_active_deadline_seconds {
                    self.worker.kubernetes.gitops_writer_active_deadline_seconds = value;
                }
                if let Some(value) = kubernetes.gitops_writer_ttl_seconds_after_finished {
                    self.worker
                        .kubernetes
                        .gitops_writer_ttl_seconds_after_finished = value;
                }
                if let Some(value) = kubernetes.git_observer_enabled {
                    self.worker.kubernetes.git_observer_enabled = value;
                }
                if let Some(value) = kubernetes.git_observer_service_account {
                    self.worker.kubernetes.git_observer_service_account = value;
                }
                if let Some(value) = kubernetes.git_observer_token_secret_name {
                    self.worker.kubernetes.git_observer_token_secret_name = blank_to_none(value);
                }
                if let Some(value) = kubernetes.git_observer_allowed_repos {
                    self.worker.kubernetes.git_observer_allowed_repos = value;
                }
                if let Some(value) = kubernetes.git_observer_github_api_url {
                    self.worker.kubernetes.git_observer_github_api_url = value;
                }
                if let Some(value) = kubernetes.git_observer_active_deadline_seconds {
                    self.worker.kubernetes.git_observer_active_deadline_seconds = value;
                }
                if let Some(value) = kubernetes.git_observer_ttl_seconds_after_finished {
                    self.worker
                        .kubernetes
                        .git_observer_ttl_seconds_after_finished = value;
                }
                if let Some(value) = kubernetes.gitops_observer_enabled {
                    self.worker.kubernetes.gitops_observer_enabled = value;
                }
                if let Some(value) = kubernetes.gitops_observer_service_account {
                    self.worker.kubernetes.gitops_observer_service_account = value;
                }
                if let Some(value) = kubernetes.gitops_observer_token_secret_name {
                    self.worker.kubernetes.gitops_observer_token_secret_name = blank_to_none(value);
                }
                if let Some(value) = kubernetes.gitops_observer_allowed_repos {
                    self.worker.kubernetes.gitops_observer_allowed_repos = value;
                }
                if let Some(value) = kubernetes.gitops_observer_github_api_url {
                    self.worker.kubernetes.gitops_observer_github_api_url = value;
                }
                if let Some(value) = kubernetes.gitops_observer_active_deadline_seconds {
                    self.worker
                        .kubernetes
                        .gitops_observer_active_deadline_seconds = value;
                }
                if let Some(value) = kubernetes.gitops_observer_ttl_seconds_after_finished {
                    self.worker
                        .kubernetes
                        .gitops_observer_ttl_seconds_after_finished = value;
                }
                if let Some(value) = kubernetes.source_reader_enabled {
                    self.worker.kubernetes.source_reader_enabled = value;
                }
                if let Some(value) = kubernetes.source_reader_service_account {
                    self.worker.kubernetes.source_reader_service_account = value;
                }
                if let Some(value) = kubernetes.source_reader_token_secret_name {
                    self.worker.kubernetes.source_reader_token_secret_name = blank_to_none(value);
                }
                if let Some(value) = kubernetes.source_reader_allowed_repos {
                    self.worker.kubernetes.source_reader_allowed_repos = value;
                }
                if let Some(value) = kubernetes.source_reader_active_deadline_seconds {
                    self.worker.kubernetes.source_reader_active_deadline_seconds = value;
                }
                if let Some(value) = kubernetes.source_reader_ttl_seconds_after_finished {
                    self.worker
                        .kubernetes
                        .source_reader_ttl_seconds_after_finished = value;
                }
                if let Some(value) = kubernetes.api_url {
                    self.worker.kubernetes.api_url = value;
                }
                if let Some(value) = kubernetes.workspace_dir {
                    self.worker.kubernetes.workspace_dir = value;
                }
                if let Some(value) = kubernetes.workspace_size_limit {
                    self.worker.kubernetes.workspace_size_limit = value;
                }
                if let Some(value) = kubernetes.workspace_storage_class {
                    self.worker.kubernetes.workspace_storage_class = blank_to_none(value);
                }
                if let Some(value) = kubernetes.workspace_ephemeral_storage_request {
                    self.worker.kubernetes.workspace_ephemeral_storage_request = value;
                }
                if let Some(value) = kubernetes.workspace_ephemeral_storage_limit {
                    self.worker.kubernetes.workspace_ephemeral_storage_limit = value;
                }
                if let Some(value) = kubernetes.workspace_node_hostname {
                    self.worker.kubernetes.workspace_node_hostname = blank_to_none(value);
                }
                if let Some(value) = kubernetes.max_concurrent_run_jobs {
                    self.worker.kubernetes.max_concurrent_run_jobs = value;
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
        if let Some(value) = env.get("PHARNESS_WORKSPACE_ROOT") {
            self.storage.workspace_root = expand_tilde(PathBuf::from(value));
        }
        if let Some(value) = env.get("PHARNESS_WORKSPACE_ALLOWED_REPOS") {
            self.storage.workspace_allowed_repos = value
                .split(',')
                .map(str::trim)
                .filter(|repo| !repo.is_empty())
                .map(|repo| expand_tilde(PathBuf::from(repo)))
                .collect();
        }
        if let Some(value) = env.get("PHARNESS_WORKSPACE_ALLOWED_REMOTE_REPOS") {
            self.storage.workspace_allowed_remote_repos = value
                .split(',')
                .map(str::trim)
                .filter(|repo| !repo.is_empty())
                .map(ToOwned::to_owned)
                .collect();
        }
        if let Some(value) = env.get("PHARNESS_FIREWORKS_MODEL") {
            self.model.model = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_FIREWORKS_BASE_URL") {
            self.model.base_url = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_CONTEXT_MAX_INPUT_TOKENS") {
            self.model.context_budget.max_input_tokens =
                parse_u64(value, "PHARNESS_CONTEXT_MAX_INPUT_TOKENS")?
                    .try_into()
                    .map_err(|_| {
                        anyhow::anyhow!("PHARNESS_CONTEXT_MAX_INPUT_TOKENS is too large")
                    })?;
        }
        if let Some(value) = env.get("PHARNESS_CONTEXT_RECENT_MESSAGE_TOKENS") {
            self.model.context_budget.recent_message_tokens =
                parse_u64(value, "PHARNESS_CONTEXT_RECENT_MESSAGE_TOKENS")?
                    .try_into()
                    .map_err(|_| {
                        anyhow::anyhow!("PHARNESS_CONTEXT_RECENT_MESSAGE_TOKENS is too large")
                    })?;
        }
        if let Some(value) = env.get("PHARNESS_CONTEXT_MAX_TOOL_RESULT_TOKENS") {
            self.model.context_budget.max_tool_result_tokens =
                parse_u64(value, "PHARNESS_CONTEXT_MAX_TOOL_RESULT_TOKENS")?
                    .try_into()
                    .map_err(|_| {
                        anyhow::anyhow!("PHARNESS_CONTEXT_MAX_TOOL_RESULT_TOKENS is too large")
                    })?;
        }
        if let Some(value) = env.get("PHARNESS_FIREWORKS_API_KEY_ENV") {
            self.model.api_key_env = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_INFERENCE_GATEWAY_ENABLED") {
            self.inference.enabled = parse_bool(value, "PHARNESS_INFERENCE_GATEWAY_ENABLED")?;
        }
        if let Some(value) = env.get("PHARNESS_INFERENCE_GATEWAY_URL") {
            self.inference.gateway_url = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_DIRECT_FIREWORKS_ENABLED") {
            self.inference.direct_fireworks_enabled =
                parse_bool(value, "PHARNESS_DIRECT_FIREWORKS_ENABLED")?;
        }
        if let Some(value) = env.get("PHARNESS_MODEL_GRANT_SIGNER_ENABLED") {
            self.inference.grant_signing_enabled =
                parse_bool(value, "PHARNESS_MODEL_GRANT_SIGNER_ENABLED")?;
        }
        if let Some(value) = env.get("PHARNESS_MODEL_GRANT_HMAC_KEY_ENV") {
            self.inference.grant_hmac_key_env = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_INFERENCE_REGISTRY_JSON") {
            self.inference.registry = serde_json::from_str(value)
                .context("PHARNESS_INFERENCE_REGISTRY_JSON must contain a valid registry")?;
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
        if let Some(value) = env.get("PHARNESS_ARGO_EXECUTOR_ENABLED") {
            self.worker.kubernetes.argo_executor_enabled =
                parse_bool(value, "PHARNESS_ARGO_EXECUTOR_ENABLED")?;
        }
        if let Some(value) = env.get("PHARNESS_ARGO_EXECUTOR_SERVICE_ACCOUNT") {
            self.worker.kubernetes.argo_executor_service_account = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_ARGO_EXECUTOR_NAMESPACE") {
            self.worker.kubernetes.argo_executor_namespace = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_ARGO_EXECUTOR_ALLOWED_APPLICATIONS") {
            self.worker.kubernetes.argo_executor_allowed_applications = value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect();
        }
        if let Some(value) = env.get("PHARNESS_ARGO_EXECUTOR_POLL_SECONDS") {
            self.worker.kubernetes.argo_executor_poll_seconds =
                parse_u64(value, "PHARNESS_ARGO_EXECUTOR_POLL_SECONDS")?;
        }
        if let Some(value) = env.get("PHARNESS_ARGO_EXECUTOR_ACTIVE_DEADLINE_SECONDS") {
            self.worker.kubernetes.argo_executor_active_deadline_seconds =
                parse_u64(value, "PHARNESS_ARGO_EXECUTOR_ACTIVE_DEADLINE_SECONDS")?;
        }
        if let Some(value) = env.get("PHARNESS_ARGO_EXECUTOR_TTL_SECONDS") {
            self.worker
                .kubernetes
                .argo_executor_ttl_seconds_after_finished =
                parse_u64(value, "PHARNESS_ARGO_EXECUTOR_TTL_SECONDS")?;
        }
        if let Some(value) = env.get("PHARNESS_GIT_WRITER_ENABLED") {
            self.worker.kubernetes.git_writer_enabled =
                parse_bool(value, "PHARNESS_GIT_WRITER_ENABLED")?;
        }
        if let Some(value) = env.get("PHARNESS_GIT_WRITER_SERVICE_ACCOUNT") {
            self.worker.kubernetes.git_writer_service_account = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_GIT_WRITER_TOKEN_SECRET") {
            self.worker.kubernetes.git_writer_token_secret_name = blank_to_none(value.clone());
        }
        if let Some(value) = env.get("PHARNESS_GIT_WRITER_ALLOWED_REPOS") {
            self.worker.kubernetes.git_writer_allowed_repos = split_registry_aliases(value);
        }
        if let Some(value) = env.get("PHARNESS_GIT_WRITER_GITHUB_API_URL") {
            self.worker.kubernetes.git_writer_github_api_url = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_GIT_WRITER_AUTHOR_NAME") {
            self.worker.kubernetes.git_writer_author_name = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_GIT_WRITER_AUTHOR_EMAIL") {
            self.worker.kubernetes.git_writer_author_email = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_GIT_WRITER_ACTIVE_DEADLINE_SECONDS") {
            self.worker.kubernetes.git_writer_active_deadline_seconds =
                parse_u64(value, "PHARNESS_GIT_WRITER_ACTIVE_DEADLINE_SECONDS")?;
        }
        if let Some(value) = env.get("PHARNESS_GIT_WRITER_TTL_SECONDS") {
            self.worker.kubernetes.git_writer_ttl_seconds_after_finished =
                parse_u64(value, "PHARNESS_GIT_WRITER_TTL_SECONDS")?;
        }
        if let Some(value) = env.get("PHARNESS_GITOPS_WRITER_ENABLED") {
            self.worker.kubernetes.gitops_writer_enabled =
                parse_bool(value, "PHARNESS_GITOPS_WRITER_ENABLED")?;
        }
        if let Some(value) = env.get("PHARNESS_GITOPS_WRITER_SERVICE_ACCOUNT") {
            self.worker.kubernetes.gitops_writer_service_account = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_GITOPS_WRITER_TOKEN_SECRET") {
            self.worker.kubernetes.gitops_writer_token_secret_name = blank_to_none(value.clone());
        }
        if let Some(value) = env.get("PHARNESS_GITOPS_WRITER_ALLOWED_REPOS") {
            self.worker.kubernetes.gitops_writer_allowed_repos = split_registry_aliases(value);
        }
        if let Some(value) = env.get("PHARNESS_GITOPS_WRITER_GITHUB_API_URL") {
            self.worker.kubernetes.gitops_writer_github_api_url = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_GITOPS_WRITER_AUTHOR_NAME") {
            self.worker.kubernetes.gitops_writer_author_name = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_GITOPS_WRITER_AUTHOR_EMAIL") {
            self.worker.kubernetes.gitops_writer_author_email = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_GITOPS_WRITER_ACTIVE_DEADLINE_SECONDS") {
            self.worker.kubernetes.gitops_writer_active_deadline_seconds =
                parse_u64(value, "PHARNESS_GITOPS_WRITER_ACTIVE_DEADLINE_SECONDS")?;
        }
        if let Some(value) = env.get("PHARNESS_GITOPS_WRITER_TTL_SECONDS") {
            self.worker
                .kubernetes
                .gitops_writer_ttl_seconds_after_finished =
                parse_u64(value, "PHARNESS_GITOPS_WRITER_TTL_SECONDS")?;
        }
        if let Some(value) = env.get("PHARNESS_GIT_OBSERVER_ENABLED") {
            self.worker.kubernetes.git_observer_enabled =
                parse_bool(value, "PHARNESS_GIT_OBSERVER_ENABLED")?;
        }
        if let Some(value) = env.get("PHARNESS_GIT_OBSERVER_SERVICE_ACCOUNT") {
            self.worker.kubernetes.git_observer_service_account = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_GIT_OBSERVER_TOKEN_SECRET") {
            self.worker.kubernetes.git_observer_token_secret_name = blank_to_none(value.clone());
        }
        if let Some(value) = env.get("PHARNESS_GIT_OBSERVER_ALLOWED_REPOS") {
            self.worker.kubernetes.git_observer_allowed_repos = split_registry_aliases(value);
        }
        if let Some(value) = env.get("PHARNESS_GIT_OBSERVER_GITHUB_API_URL") {
            self.worker.kubernetes.git_observer_github_api_url = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_GIT_OBSERVER_ACTIVE_DEADLINE_SECONDS") {
            self.worker.kubernetes.git_observer_active_deadline_seconds =
                parse_u64(value, "PHARNESS_GIT_OBSERVER_ACTIVE_DEADLINE_SECONDS")?;
        }
        if let Some(value) = env.get("PHARNESS_GIT_OBSERVER_TTL_SECONDS") {
            self.worker
                .kubernetes
                .git_observer_ttl_seconds_after_finished =
                parse_u64(value, "PHARNESS_GIT_OBSERVER_TTL_SECONDS")?;
        }
        if let Some(value) = env.get("PHARNESS_GITOPS_OBSERVER_ENABLED") {
            self.worker.kubernetes.gitops_observer_enabled =
                parse_bool(value, "PHARNESS_GITOPS_OBSERVER_ENABLED")?;
        }
        if let Some(value) = env.get("PHARNESS_GITOPS_OBSERVER_SERVICE_ACCOUNT") {
            self.worker.kubernetes.gitops_observer_service_account = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_GITOPS_OBSERVER_TOKEN_SECRET") {
            self.worker.kubernetes.gitops_observer_token_secret_name = blank_to_none(value.clone());
        }
        if let Some(value) = env.get("PHARNESS_GITOPS_OBSERVER_ALLOWED_REPOS") {
            self.worker.kubernetes.gitops_observer_allowed_repos = split_registry_aliases(value);
        }
        if let Some(value) = env.get("PHARNESS_GITOPS_OBSERVER_GITHUB_API_URL") {
            self.worker.kubernetes.gitops_observer_github_api_url = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_GITOPS_OBSERVER_ACTIVE_DEADLINE_SECONDS") {
            self.worker
                .kubernetes
                .gitops_observer_active_deadline_seconds =
                parse_u64(value, "PHARNESS_GITOPS_OBSERVER_ACTIVE_DEADLINE_SECONDS")?;
        }
        if let Some(value) = env.get("PHARNESS_GITOPS_OBSERVER_TTL_SECONDS") {
            self.worker
                .kubernetes
                .gitops_observer_ttl_seconds_after_finished =
                parse_u64(value, "PHARNESS_GITOPS_OBSERVER_TTL_SECONDS")?;
        }
        if let Some(value) = env.get("PHARNESS_SOURCE_READER_ENABLED") {
            self.worker.kubernetes.source_reader_enabled =
                parse_bool(value, "PHARNESS_SOURCE_READER_ENABLED")?;
        }
        if let Some(value) = env.get("PHARNESS_SOURCE_READER_SERVICE_ACCOUNT") {
            self.worker.kubernetes.source_reader_service_account = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_SOURCE_READER_TOKEN_SECRET") {
            self.worker.kubernetes.source_reader_token_secret_name = blank_to_none(value.clone());
        }
        if let Some(value) = env.get("PHARNESS_SOURCE_READER_ALLOWED_REPOS") {
            self.worker.kubernetes.source_reader_allowed_repos = split_registry_aliases(value);
        }
        if let Some(value) = env.get("PHARNESS_SOURCE_READER_ACTIVE_DEADLINE_SECONDS") {
            self.worker.kubernetes.source_reader_active_deadline_seconds =
                parse_u64(value, "PHARNESS_SOURCE_READER_ACTIVE_DEADLINE_SECONDS")?;
        }
        if let Some(value) = env.get("PHARNESS_SOURCE_READER_TTL_SECONDS") {
            self.worker
                .kubernetes
                .source_reader_ttl_seconds_after_finished =
                parse_u64(value, "PHARNESS_SOURCE_READER_TTL_SECONDS")?;
        }
        if let Some(value) = env.get("PHARNESS_WORKER_K8S_API_URL") {
            self.worker.kubernetes.api_url = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_WORKER_K8S_WORKSPACE_DIR") {
            self.worker.kubernetes.workspace_dir = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_WORKER_K8S_WORKSPACE_SIZE_LIMIT") {
            self.worker.kubernetes.workspace_size_limit = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_WORKER_K8S_WORKSPACE_STORAGE_CLASS") {
            self.worker.kubernetes.workspace_storage_class = blank_to_none(value.clone());
        }
        if let Some(value) = env.get("PHARNESS_WORKER_K8S_WORKSPACE_EPHEMERAL_STORAGE_REQUEST") {
            self.worker.kubernetes.workspace_ephemeral_storage_request = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_WORKER_K8S_WORKSPACE_EPHEMERAL_STORAGE_LIMIT") {
            self.worker.kubernetes.workspace_ephemeral_storage_limit = value.clone();
        }
        if let Some(value) = env.get("PHARNESS_WORKER_K8S_WORKSPACE_NODE_HOSTNAME") {
            self.worker.kubernetes.workspace_node_hostname = blank_to_none(value.clone());
        }
        if let Some(value) = env.get("PHARNESS_WORKER_K8S_MAX_CONCURRENT_RUN_JOBS") {
            self.worker.kubernetes.max_concurrent_run_jobs =
                parse_u64(value, "PHARNESS_WORKER_K8S_MAX_CONCURRENT_RUN_JOBS")?
                    .try_into()
                    .map_err(|_| {
                        anyhow::anyhow!("PHARNESS_WORKER_K8S_MAX_CONCURRENT_RUN_JOBS is too large")
                    })?;
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
        self.inference.grant_hmac_key = env
            .get(&self.inference.grant_hmac_key_env)
            .cloned()
            .filter(|value| value.len() >= 32)
            .map(SecretString::new);
    }
}

fn default_inference_registry() -> anyhow::Result<InferenceRegistry> {
    let mut target = InferenceTargetRevision {
        schema_version: INFERENCE_TARGET_SCHEMA.into(),
        target_id: "fireworks-kimi-k2p6".into(),
        revision: "v1".into(),
        display_name: "Fireworks Kimi K2.6".into(),
        backend_kind: InferenceBackendKind::Fireworks,
        protocol: "openai_chat_completions_v1".into(),
        upstream_base_url: DEFAULT_FIREWORKS_BASE_URL.into(),
        upstream_model: DEFAULT_FIREWORKS_MODEL.into(),
        authentication_binding: Some("fireworks-api-key".into()),
        transport: InferenceTransportPolicy::default(),
        capabilities: InferenceCapabilities {
            native_tools: true,
            streaming: true,
            json_schema: true,
            stream_options: true,
            reasoning_efforts: vec![
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            reasoning_context_modes: vec![
                ReasoningContextMode::CurrentTurn,
                ReasoningContextMode::AllTurns,
            ],
        },
        context_limit_tokens: 262_144,
        output_limit_tokens: 16_384,
        allowed_stages: vec![
            InferenceStage::Onboarding,
            InferenceStage::Plan,
            InferenceStage::Implement,
            InferenceStage::Test,
            InferenceStage::Verify,
        ],
        selectable: true,
        openrouter: None,
        config_hash: String::new(),
    };
    target.config_hash = target.computed_hash()?;
    let mut policy = StageInferencePolicyRevision {
        schema_version: INFERENCE_POLICY_SCHEMA.into(),
        policy_id: "fireworks-legacy-v1".into(),
        revision: "v1".into(),
        display_name: "Fireworks legacy behavior".into(),
        eligible_profiles: vec![
            "repository-onboarding-proposer".into(),
            "repo-planner".into(),
            "repo-builder".into(),
            "repo-tester".into(),
            "repo-verifier".into(),
        ],
        eligible_stages: target.allowed_stages.clone(),
        target: InferenceTargetRef {
            target_id: target.target_id.clone(),
            revision: target.revision.clone(),
        },
        target_hash: target.config_hash.clone(),
        reasoning: ReasoningRequestPolicy::default(),
        temperature_milli: Some(100),
        max_output_tokens: 4_096,
        max_input_tokens: ContextBudget::default().max_input_tokens,
        tool_protocol: ToolProtocolMode::NativeTools,
        transport_max_attempts: 3,
        selectable: true,
        policy_hash: String::new(),
    };
    policy.policy_hash = policy.computed_hash()?;
    let policy_ref = InferencePolicyRef {
        policy_id: policy.policy_id.clone(),
        revision: policy.revision.clone(),
    };
    let defaults = target
        .allowed_stages
        .iter()
        .copied()
        .map(|stage| (stage, policy_ref.clone()))
        .collect();
    let candidates = [
        (
            "onboarding-kimi-k2p6-medium-v1",
            "Onboarding Kimi K2.6 medium",
            "repository-onboarding-proposer",
            InferenceStage::Onboarding,
            ReasoningEffort::Medium,
            100,
            4_096,
        ),
        (
            "planner-kimi-k2p6-high-v1",
            "Planner Kimi K2.6 high",
            "repo-planner",
            InferenceStage::Plan,
            ReasoningEffort::High,
            100,
            8_192,
        ),
        (
            "builder-kimi-k2p6-medium-v1",
            "Builder Kimi K2.6 medium",
            "repo-builder",
            InferenceStage::Implement,
            ReasoningEffort::Medium,
            100,
            8_192,
        ),
        (
            "tester-kimi-k2p6-low-v1",
            "Tester Kimi K2.6 low",
            "repo-tester",
            InferenceStage::Test,
            ReasoningEffort::Low,
            0,
            4_096,
        ),
        (
            "verifier-kimi-k2p6-high-v1",
            "Verifier Kimi K2.6 high",
            "repo-verifier",
            InferenceStage::Verify,
            ReasoningEffort::High,
            0,
            8_192,
        ),
    ]
    .into_iter()
    .map(
        |(id, display_name, profile, stage, effort, temperature_milli, output)| {
            let mut candidate = StageInferencePolicyRevision {
                schema_version: INFERENCE_POLICY_SCHEMA.into(),
                policy_id: id.into(),
                revision: "v1".into(),
                display_name: display_name.into(),
                eligible_profiles: vec![profile.into()],
                eligible_stages: vec![stage],
                target: InferenceTargetRef {
                    target_id: target.target_id.clone(),
                    revision: target.revision.clone(),
                },
                target_hash: target.config_hash.clone(),
                reasoning: ReasoningRequestPolicy {
                    effort: Some(effort),
                    context_mode: ReasoningContextMode::CurrentTurn,
                    expose_replay: true,
                },
                temperature_milli: Some(temperature_milli),
                max_output_tokens: output,
                max_input_tokens: ContextBudget::default().max_input_tokens,
                tool_protocol: ToolProtocolMode::NativeTools,
                transport_max_attempts: 3,
                selectable: false,
                policy_hash: String::new(),
            };
            candidate.policy_hash = candidate.computed_hash()?;
            Ok::<_, serde_json::Error>(candidate)
        },
    )
    .collect::<Result<Vec<_>, _>>()?;
    let mut policies = vec![policy];
    policies.extend(candidates);
    let mut registry = InferenceRegistry {
        schema_version: INFERENCE_REGISTRY_SCHEMA.into(),
        targets: vec![target],
        policies,
        defaults,
        config_hash: String::new(),
    };
    registry.config_hash = registry.computed_hash()?;
    Ok(registry)
}

fn reject_invalid_context_budget(budget: &ContextBudget) -> anyhow::Result<()> {
    if budget.max_input_tokens <= budget.reserved_output_tokens {
        anyhow::bail!("model context_max_input_tokens must exceed the reserved output budget");
    }
    if budget.recent_message_tokens == 0 {
        anyhow::bail!("model context_recent_message_tokens must be greater than zero");
    }
    if budget.max_tool_result_tokens == 0 {
        anyhow::bail!("model context_max_tool_result_tokens must be greater than zero");
    }
    Ok(())
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileConfig {
    api: Option<FileApiConfig>,
    storage: Option<FileStorageConfig>,
    model: Option<FileModelConfig>,
    inference: Option<FileInferenceConfig>,
    cluster: Option<FileClusterConfig>,
    policy: Option<FilePolicyConfig>,
    worker: Option<FileWorkerConfig>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileInferenceConfig {
    enabled: Option<bool>,
    gateway_url: Option<String>,
    direct_fireworks_enabled: Option<bool>,
    grant_signing_enabled: Option<bool>,
    grant_hmac_key_env: Option<String>,
    registry_json: Option<String>,
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
    argo_executor_enabled: Option<bool>,
    argo_executor_service_account: Option<String>,
    argo_executor_namespace: Option<String>,
    argo_executor_allowed_applications: Option<Vec<String>>,
    argo_executor_poll_seconds: Option<u64>,
    argo_executor_active_deadline_seconds: Option<u64>,
    argo_executor_ttl_seconds_after_finished: Option<u64>,
    git_writer_enabled: Option<bool>,
    git_writer_service_account: Option<String>,
    git_writer_token_secret_name: Option<String>,
    git_writer_allowed_repos: Option<Vec<String>>,
    git_writer_github_api_url: Option<String>,
    git_writer_author_name: Option<String>,
    git_writer_author_email: Option<String>,
    git_writer_active_deadline_seconds: Option<u64>,
    git_writer_ttl_seconds_after_finished: Option<u64>,
    gitops_writer_enabled: Option<bool>,
    gitops_writer_service_account: Option<String>,
    gitops_writer_token_secret_name: Option<String>,
    gitops_writer_allowed_repos: Option<Vec<String>>,
    gitops_writer_github_api_url: Option<String>,
    gitops_writer_author_name: Option<String>,
    gitops_writer_author_email: Option<String>,
    gitops_writer_active_deadline_seconds: Option<u64>,
    gitops_writer_ttl_seconds_after_finished: Option<u64>,
    git_observer_enabled: Option<bool>,
    git_observer_service_account: Option<String>,
    git_observer_token_secret_name: Option<String>,
    git_observer_allowed_repos: Option<Vec<String>>,
    git_observer_github_api_url: Option<String>,
    git_observer_active_deadline_seconds: Option<u64>,
    git_observer_ttl_seconds_after_finished: Option<u64>,
    gitops_observer_enabled: Option<bool>,
    gitops_observer_service_account: Option<String>,
    gitops_observer_token_secret_name: Option<String>,
    gitops_observer_allowed_repos: Option<Vec<String>>,
    gitops_observer_github_api_url: Option<String>,
    gitops_observer_active_deadline_seconds: Option<u64>,
    gitops_observer_ttl_seconds_after_finished: Option<u64>,
    source_reader_enabled: Option<bool>,
    source_reader_service_account: Option<String>,
    source_reader_token_secret_name: Option<String>,
    source_reader_allowed_repos: Option<Vec<String>>,
    source_reader_active_deadline_seconds: Option<u64>,
    source_reader_ttl_seconds_after_finished: Option<u64>,
    api_url: Option<String>,
    workspace_dir: Option<String>,
    workspace_size_limit: Option<String>,
    workspace_storage_class: Option<String>,
    workspace_ephemeral_storage_request: Option<String>,
    workspace_ephemeral_storage_limit: Option<String>,
    workspace_node_hostname: Option<String>,
    max_concurrent_run_jobs: Option<u32>,
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
    workspace_root: Option<String>,
    workspace_allowed_repos: Option<Vec<String>>,
    workspace_allowed_remote_repos: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileModelConfig {
    provider: Option<String>,
    model: Option<String>,
    api_key_env: Option<String>,
    base_url: Option<String>,
    context_max_input_tokens: Option<u32>,
    context_recent_message_tokens: Option<u32>,
    context_max_tool_result_tokens: Option<u32>,
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

fn reject_invalid_kubernetes_workspace(config: &WorkerKubernetesConfig) -> anyhow::Result<()> {
    for (label, value) in [
        ("worker.kubernetes.workspace_dir", &config.workspace_dir),
        (
            "worker.kubernetes.workspace_size_limit",
            &config.workspace_size_limit,
        ),
        (
            "worker.kubernetes.workspace_ephemeral_storage_request",
            &config.workspace_ephemeral_storage_request,
        ),
        (
            "worker.kubernetes.workspace_ephemeral_storage_limit",
            &config.workspace_ephemeral_storage_limit,
        ),
    ] {
        if value.trim().is_empty() {
            bail!("{label} must not be blank");
        }
    }
    if config.max_concurrent_run_jobs == 0 {
        bail!("worker.kubernetes.max_concurrent_run_jobs must be at least one");
    }
    Ok(())
}

fn reject_invalid_argo_executor(config: &WorkerKubernetesConfig) -> anyhow::Result<()> {
    if !config.argo_executor_enabled {
        return Ok(());
    }
    if config.argo_executor_allowed_applications.is_empty() {
        bail!("enabled Argo executor requires at least one allowed application");
    }
    if config.argo_executor_service_account.trim().is_empty()
        || config.argo_executor_service_account.contains(['\n', '\r'])
    {
        bail!("worker.kubernetes.argo_executor_service_account must be non-blank and single-line");
    }
    if config.argo_executor_namespace.trim().is_empty()
        || config.argo_executor_namespace.contains(['\n', '\r'])
    {
        bail!("worker.kubernetes.argo_executor_namespace must be non-blank and single-line");
    }
    if config.argo_executor_poll_seconds == 0 {
        bail!("worker.kubernetes.argo_executor_poll_seconds must be at least one");
    }
    if config.argo_executor_active_deadline_seconds == 0 {
        bail!("worker.kubernetes.argo_executor_active_deadline_seconds must be at least one");
    }
    if config.argo_executor_ttl_seconds_after_finished == 0 {
        bail!("worker.kubernetes.argo_executor_ttl_seconds_after_finished must be at least one");
    }
    for application in &config.argo_executor_allowed_applications {
        if application.trim().is_empty() || application.contains(['\n', '\r']) {
            bail!("worker.kubernetes.argo_executor_allowed_applications must be non-blank and single-line");
        }
    }
    Ok(())
}

fn reject_invalid_git_writer(config: &WorkerKubernetesConfig) -> anyhow::Result<()> {
    if !config.git_writer_enabled {
        return Ok(());
    }
    if config.git_writer_token_secret_name.is_none() || config.git_writer_allowed_repos.is_empty() {
        bail!(
            "enabled Git writer requires a token Secret name and at least one allowed repository"
        );
    }
    for (label, value) in [
        (
            "worker.kubernetes.git_writer_service_account",
            &config.git_writer_service_account,
        ),
        (
            "worker.kubernetes.git_writer_github_api_url",
            &config.git_writer_github_api_url,
        ),
        (
            "worker.kubernetes.git_writer_author_name",
            &config.git_writer_author_name,
        ),
        (
            "worker.kubernetes.git_writer_author_email",
            &config.git_writer_author_email,
        ),
    ] {
        if value.trim().is_empty() || value.contains(['\n', '\r']) {
            bail!("{label} must be non-blank and single-line");
        }
    }
    if !config.git_writer_github_api_url.starts_with("https://") {
        bail!("worker.kubernetes.git_writer_github_api_url must use HTTPS");
    }
    if config.git_writer_active_deadline_seconds == 0 {
        bail!("worker.kubernetes.git_writer_active_deadline_seconds must be at least one");
    }
    if config.git_writer_ttl_seconds_after_finished == 0 {
        bail!("worker.kubernetes.git_writer_ttl_seconds_after_finished must be at least one");
    }
    Ok(())
}

fn reject_invalid_gitops_writer(config: &WorkerKubernetesConfig) -> anyhow::Result<()> {
    if !config.gitops_writer_enabled {
        return Ok(());
    }
    if config.gitops_writer_token_secret_name.is_none()
        || config.gitops_writer_allowed_repos.is_empty()
    {
        bail!(
            "enabled GitOps writer requires a token Secret name and at least one allowed repository"
        );
    }
    for (label, value) in [
        (
            "worker.kubernetes.gitops_writer_service_account",
            &config.gitops_writer_service_account,
        ),
        (
            "worker.kubernetes.gitops_writer_github_api_url",
            &config.gitops_writer_github_api_url,
        ),
        (
            "worker.kubernetes.gitops_writer_author_name",
            &config.gitops_writer_author_name,
        ),
        (
            "worker.kubernetes.gitops_writer_author_email",
            &config.gitops_writer_author_email,
        ),
    ] {
        if value.trim().is_empty() || value.contains(['\n', '\r']) {
            bail!("{label} must be non-blank and single-line");
        }
    }
    if !config.gitops_writer_github_api_url.starts_with("https://") {
        bail!("worker.kubernetes.gitops_writer_github_api_url must use HTTPS");
    }
    if config.gitops_writer_active_deadline_seconds == 0 {
        bail!("worker.kubernetes.gitops_writer_active_deadline_seconds must be at least one");
    }
    if config.gitops_writer_ttl_seconds_after_finished == 0 {
        bail!("worker.kubernetes.gitops_writer_ttl_seconds_after_finished must be at least one");
    }
    Ok(())
}

fn reject_invalid_git_observer(config: &WorkerKubernetesConfig) -> anyhow::Result<()> {
    if !config.git_observer_enabled {
        return Ok(());
    }
    if config.git_observer_token_secret_name.is_none()
        || config.git_observer_allowed_repos.is_empty()
    {
        bail!(
            "enabled Git observer requires a token Secret name and at least one allowed repository"
        );
    }
    for (label, value) in [
        (
            "worker.kubernetes.git_observer_service_account",
            &config.git_observer_service_account,
        ),
        (
            "worker.kubernetes.git_observer_github_api_url",
            &config.git_observer_github_api_url,
        ),
    ] {
        if value.trim().is_empty() || value.contains(['\n', '\r']) {
            bail!("{label} must be non-blank and single-line");
        }
    }
    if !config.git_observer_github_api_url.starts_with("https://") {
        bail!("worker.kubernetes.git_observer_github_api_url must use HTTPS");
    }
    if config.git_observer_active_deadline_seconds == 0 {
        bail!("worker.kubernetes.git_observer_active_deadline_seconds must be at least one");
    }
    if config.git_observer_ttl_seconds_after_finished == 0 {
        bail!("worker.kubernetes.git_observer_ttl_seconds_after_finished must be at least one");
    }
    Ok(())
}

fn reject_invalid_gitops_observer(config: &WorkerKubernetesConfig) -> anyhow::Result<()> {
    if !config.gitops_observer_enabled {
        return Ok(());
    }
    if config.gitops_observer_token_secret_name.is_none()
        || config.gitops_observer_allowed_repos.is_empty()
    {
        bail!(
            "enabled GitOps observer requires a separate token Secret name and at least one allowed repository"
        );
    }
    for (label, value) in [
        (
            "worker.kubernetes.gitops_observer_service_account",
            &config.gitops_observer_service_account,
        ),
        (
            "worker.kubernetes.gitops_observer_github_api_url",
            &config.gitops_observer_github_api_url,
        ),
    ] {
        if value.trim().is_empty() || value.contains(['\n', '\r']) {
            bail!("{label} must be non-blank and single-line");
        }
    }
    if !config
        .gitops_observer_github_api_url
        .starts_with("https://")
    {
        bail!("worker.kubernetes.gitops_observer_github_api_url must use HTTPS");
    }
    if config.gitops_observer_active_deadline_seconds == 0 {
        bail!("worker.kubernetes.gitops_observer_active_deadline_seconds must be at least one");
    }
    if config.gitops_observer_ttl_seconds_after_finished == 0 {
        bail!("worker.kubernetes.gitops_observer_ttl_seconds_after_finished must be at least one");
    }
    Ok(())
}

fn reject_invalid_source_reader(config: &WorkerKubernetesConfig) -> anyhow::Result<()> {
    if !config.source_reader_enabled {
        return Ok(());
    }
    if config.source_reader_allowed_repos.is_empty() {
        bail!("enabled source reader requires at least one allowed repository");
    }
    if config.source_reader_service_account.trim().is_empty()
        || config.source_reader_service_account.contains(['\n', '\r'])
    {
        bail!("worker.kubernetes.source_reader_service_account must be non-blank and single-line");
    }
    if config.source_reader_active_deadline_seconds == 0 {
        bail!("worker.kubernetes.source_reader_active_deadline_seconds must be at least one");
    }
    if config.source_reader_ttl_seconds_after_finished == 0 {
        bail!("worker.kubernetes.source_reader_ttl_seconds_after_finished must be at least one");
    }
    for repository in &config.source_reader_allowed_repos {
        if !repository.starts_with("https://github.com/")
            || !repository.ends_with(".git")
            || repository.contains(['\n', '\r', '@', '?', '#'])
        {
            bail!("worker.kubernetes.source_reader_allowed_repos must contain safe GitHub HTTPS clone URLs");
        }
    }
    Ok(())
}

fn reject_mutable_worker_image(image: &str) -> anyhow::Result<()> {
    let Some((repository, digest)) = image.rsplit_once("@sha256:") else {
        bail!("worker.kubernetes.image must be pinned as repository@sha256:digest");
    };
    if repository.trim().is_empty()
        || digest.len() != 64
        || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("worker.kubernetes.image has an invalid immutable sha256 digest");
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
        assert_eq!(
            config.storage.workspace_root,
            PathBuf::from(".pharness/workspaces")
        );
        assert!(config.storage.workspace_allowed_repos.is_empty());
        assert!(config.storage.workspace_allowed_remote_repos.is_empty());
        assert_eq!(config.model.model, "accounts/fireworks/models/kimi-k2p6");
        assert_eq!(config.cluster.argocd_namespace, "argocd");
        assert!(config.cluster.prometheus_url.is_none());
        assert!(config.cluster.loki_url.is_none());
        assert!(config.cluster.registry_aliases.is_empty());
        assert_eq!(config.policy.mode, PolicyMode::Default);
        assert_eq!(config.policy.environment, "local");
        assert!(config.policy.require_approval_for_writes);
        assert!(config.model.api_key.is_none());
        assert_eq!(config.worker.kubernetes.workspace_size_limit, "4Gi");
        assert!(config.worker.kubernetes.workspace_storage_class.is_none());
        assert_eq!(
            config.worker.kubernetes.workspace_ephemeral_storage_request,
            "2Gi"
        );
        assert_eq!(
            config.worker.kubernetes.workspace_ephemeral_storage_limit,
            "4Gi"
        );
        assert_eq!(config.worker.kubernetes.max_concurrent_run_jobs, 1);
        assert!(!config.worker.kubernetes.argo_executor_enabled);
        assert!(config
            .worker
            .kubernetes
            .argo_executor_allowed_applications
            .is_empty());
    }

    #[test]
    fn rejects_an_unusable_context_budget_from_environment() {
        let mut env = BTreeMap::new();
        env.insert(
            "PHARNESS_CONTEXT_MAX_INPUT_TOKENS".to_string(),
            "4096".to_string(),
        );
        let error = ApiRuntimeConfig::from_sources(None, &env)
            .err()
            .expect("invalid context budget must fail");
        assert!(error.to_string().contains("reserved output budget"));
    }

    #[test]
    fn parses_toml_config_values() {
        let path = write_temp_config(
            r#"
[api]
bind = "127.0.0.1:4888"

[storage]
path = ".pharness/test.db"
workspace_root = ".pharness/workspaces-test"
workspace_allowed_repos = ["../finance-app"]
workspace_allowed_remote_repos = ["https://github.com/example/finance-app.git"]

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
workspace_size_limit = "3Gi"
workspace_storage_class = "local-path"
workspace_ephemeral_storage_request = "1500Mi"
workspace_ephemeral_storage_limit = "3Gi"
workspace_node_hostname = "worker-a"
max_concurrent_run_jobs = 2

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
        assert_eq!(
            config.storage.workspace_root,
            PathBuf::from(".pharness/workspaces-test")
        );
        assert_eq!(
            config.storage.workspace_allowed_repos,
            vec![PathBuf::from("../finance-app")]
        );
        assert_eq!(
            config.storage.workspace_allowed_remote_repos,
            vec!["https://github.com/example/finance-app.git"]
        );
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
        assert_eq!(config.worker.kubernetes.workspace_size_limit, "3Gi");
        assert_eq!(
            config.worker.kubernetes.workspace_storage_class.as_deref(),
            Some("local-path")
        );
        assert_eq!(
            config.worker.kubernetes.workspace_ephemeral_storage_request,
            "1500Mi"
        );
        assert_eq!(
            config.worker.kubernetes.workspace_ephemeral_storage_limit,
            "3Gi"
        );
        assert_eq!(
            config.worker.kubernetes.workspace_node_hostname.as_deref(),
            Some("worker-a")
        );
        assert_eq!(config.worker.kubernetes.max_concurrent_run_jobs, 2);
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
            "PHARNESS_WORKSPACE_ALLOWED_REPOS".to_string(),
            "../finance-app,../other-app".to_string(),
        );
        env.insert(
            "PHARNESS_WORKSPACE_ALLOWED_REMOTE_REPOS".to_string(),
            "https://github.com/example/finance-app.git,https://github.com/example/other-app.git"
                .to_string(),
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
        env.insert(
            "PHARNESS_WORKER_K8S_WORKSPACE_SIZE_LIMIT".to_string(),
            "5Gi".to_string(),
        );
        env.insert(
            "PHARNESS_WORKER_K8S_WORKSPACE_EPHEMERAL_STORAGE_REQUEST".to_string(),
            "3Gi".to_string(),
        );
        env.insert(
            "PHARNESS_WORKER_K8S_WORKSPACE_EPHEMERAL_STORAGE_LIMIT".to_string(),
            "5Gi".to_string(),
        );
        env.insert(
            "PHARNESS_WORKER_K8S_WORKSPACE_NODE_HOSTNAME".to_string(),
            "worker-b".to_string(),
        );
        env.insert(
            "PHARNESS_WORKER_K8S_MAX_CONCURRENT_RUN_JOBS".to_string(),
            "3".to_string(),
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
        assert_eq!(
            config.storage.workspace_allowed_repos,
            vec![
                PathBuf::from("../finance-app"),
                PathBuf::from("../other-app")
            ]
        );
        assert_eq!(
            config.storage.workspace_allowed_remote_repos,
            vec![
                "https://github.com/example/finance-app.git",
                "https://github.com/example/other-app.git"
            ]
        );
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
        assert_eq!(config.worker.kubernetes.workspace_size_limit, "5Gi");
        assert_eq!(
            config.worker.kubernetes.workspace_ephemeral_storage_request,
            "3Gi"
        );
        assert_eq!(
            config.worker.kubernetes.workspace_ephemeral_storage_limit,
            "5Gi"
        );
        assert_eq!(
            config.worker.kubernetes.workspace_node_hostname.as_deref(),
            Some("worker-b")
        );
        assert_eq!(config.worker.kubernetes.max_concurrent_run_jobs, 3);
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
    fn rejects_zero_kubernetes_worker_concurrency() {
        let path = write_temp_config(
            r#"
[worker.kubernetes]
max_concurrent_run_jobs = 0
"#,
        );

        let error = ApiRuntimeConfig::from_sources(Some(&path), &BTreeMap::new())
            .err()
            .unwrap();

        assert!(error.to_string().contains("max_concurrent_run_jobs"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn accepts_disabled_argo_executor_without_an_application_allowlist() {
        let path = write_temp_config(
            r#"
[worker.kubernetes]
argo_executor_enabled = false
argo_executor_allowed_applications = []
"#,
        );

        let config = ApiRuntimeConfig::from_sources(Some(&path), &BTreeMap::new()).unwrap();

        assert!(!config.worker.kubernetes.argo_executor_enabled);
        assert!(config
            .worker
            .kubernetes
            .argo_executor_allowed_applications
            .is_empty());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_enabled_argo_executor_without_an_application_allowlist() {
        let path = write_temp_config(
            r#"
[worker.kubernetes]
argo_executor_enabled = true
argo_executor_allowed_applications = []
"#,
        );

        let error = ApiRuntimeConfig::from_sources(Some(&path), &BTreeMap::new())
            .err()
            .unwrap();

        assert!(error
            .to_string()
            .contains("enabled Argo executor requires at least one allowed application"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_enabled_gitops_writer_without_a_scoped_credential() {
        let path = write_temp_config(
            r#"
[worker.kubernetes]
gitops_writer_enabled = true
gitops_writer_allowed_repos = ["https://github.com/example/finance-gitops.git"]
"#,
        );

        let error = ApiRuntimeConfig::from_sources(Some(&path), &BTreeMap::new())
            .err()
            .unwrap();

        assert!(error
            .to_string()
            .contains("enabled GitOps writer requires a token Secret name"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn accepts_a_scoped_enabled_gitops_writer_from_environment() {
        let mut env = BTreeMap::new();
        env.insert(
            "PHARNESS_GITOPS_WRITER_ENABLED".to_string(),
            "true".to_string(),
        );
        env.insert(
            "PHARNESS_GITOPS_WRITER_TOKEN_SECRET".to_string(),
            "finance-gitops-writer-token".to_string(),
        );
        env.insert(
            "PHARNESS_GITOPS_WRITER_ALLOWED_REPOS".to_string(),
            "https://github.com/example/finance-gitops.git".to_string(),
        );
        env.insert(
            "PHARNESS_GITOPS_WRITER_SERVICE_ACCOUNT".to_string(),
            "finance-gitops-writer".to_string(),
        );
        env.insert(
            "PHARNESS_GITOPS_WRITER_AUTHOR_NAME".to_string(),
            "Pharness GitOps".to_string(),
        );
        env.insert(
            "PHARNESS_GITOPS_WRITER_AUTHOR_EMAIL".to_string(),
            "pharness-gitops@example.test".to_string(),
        );

        let config = ApiRuntimeConfig::from_sources(None, &env).unwrap();

        assert!(config.worker.kubernetes.gitops_writer_enabled);
        assert_eq!(
            config
                .worker
                .kubernetes
                .gitops_writer_token_secret_name
                .as_deref(),
            Some("finance-gitops-writer-token")
        );
        assert_eq!(
            config.worker.kubernetes.gitops_writer_allowed_repos,
            vec!["https://github.com/example/finance-gitops.git"]
        );
        assert_eq!(
            config.worker.kubernetes.gitops_writer_service_account,
            "finance-gitops-writer"
        );
        assert_eq!(
            config.worker.kubernetes.gitops_writer_author_name,
            "Pharness GitOps"
        );
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
