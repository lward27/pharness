//! Run dispatch across execution targets.
//!
//! `RunDispatcher` decides where a run attempt executes: in-process through
//! the existing local worker, or in an isolated Kubernetes Job per attempt.
//! Job orchestration shells out to kubectl with the pod service account,
//! matching how the typed read-only cluster capabilities already execute.

use crate::worker::{fail_run_from_dispatch, LocalWorker};
use pharness_config::WorkerKubernetesConfig;
use pharness_core::EnvironmentProfile;
use pharness_store::{
    CreateAuditEvent, PipelineIntentListFilter, SqliteStore, StoredApproval, StoredPipelineIntent,
    StoredRun, UpdatePipelineIntentExecution,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const REAPER_INTERVAL: Duration = Duration::from_secs(30);
pub(crate) const RUN_ID_LABEL: &str = "pharness.lucas.engineering/run-id";
const JOB_NAME_LABEL: &str = "app.kubernetes.io/name";
const JOB_NAME_VALUE: &str = "pharness-run";
const WORKSPACE_CLAIM_NAME_VALUE: &str = "pharness-run-workspace";
const TEKTON_EXECUTOR_JOB_NAME_VALUE: &str = "pharness-tekton-executor";
const ARGO_EXECUTOR_JOB_NAME_VALUE: &str = "pharness-argo-executor";
const GIT_WRITER_JOB_NAME_VALUE: &str = "pharness-git-writer";
const GITOPS_WRITER_JOB_NAME_VALUE: &str = "pharness-gitops-writer";
const GIT_OBSERVER_JOB_NAME_VALUE: &str = "pharness-git-observer";
const GITOPS_REVISION_RESOLVER_JOB_NAME_VALUE: &str = "pharness-gitops-revision-resolver";
const CAPABILITY_PREFLIGHT_JOB_NAME_VALUE: &str = "pharness-capability-preflight";
const ENVIRONMENT_PREPARATION_JOB_NAME_VALUE: &str = "pharness-environment-preparation";
const PIPELINE_INTENT_LABEL: &str = "pharness.lucas.engineering/pipeline-intent";
const DEPLOYMENT_INTENT_LABEL: &str = "pharness.lucas.engineering/deployment-intent";
const CHANGE_SET_LABEL: &str = "pharness.lucas.engineering/change-set";
const GITOPS_CHANGE_SET_LABEL: &str = "pharness.lucas.engineering/gitops-change-set";
const PIPELINE_INTENT_ID_ANNOTATION: &str = "pharness.lucas.engineering/pipeline-intent-id";
const EXECUTION_ID_ANNOTATION: &str = "pharness.lucas.engineering/execution-id";

#[derive(Debug, Clone)]
pub struct TektonExecutionRequest {
    pub pipeline_intent_id: String,
    pub execution_id: String,
    pub target_namespace: String,
    pub pipeline_run_manifest: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct TektonExecutionReceipt {
    pub job_name: String,
}

#[derive(Debug, Clone)]
pub struct ArgoSyncExecutionRequest {
    pub deployment_intent_id: String,
    pub execution_id: String,
}

#[derive(Debug, Clone)]
pub struct ArgoSyncExecutionReceipt {
    pub job_name: String,
}

#[derive(Debug, Clone)]
pub struct GitDeliveryExecutionRequest {
    pub change_set_id: String,
    pub execution_id: String,
}

#[derive(Debug, Clone)]
pub struct GitDeliveryExecutionReceipt {
    pub job_name: String,
}

#[derive(Debug, Clone)]
pub struct GitDeliveryObservationRequest {
    pub change_set_id: String,
    pub execution_id: String,
}

#[derive(Debug, Clone)]
pub struct GitDeliveryObservationReceipt {
    pub job_name: String,
}

#[derive(Debug, Clone)]
pub struct GitOpsRevisionResolutionRequest {
    pub gitops_change_set_id: String,
    pub execution_id: String,
}

#[derive(Debug, Clone)]
pub struct GitOpsRevisionResolutionReceipt {
    pub job_name: String,
}

#[derive(Debug, Clone)]
pub struct GitOpsDeliveryExecutionRequest {
    pub gitops_change_set_id: String,
    pub execution_id: String,
}

#[derive(Debug, Clone)]
pub struct GitOpsDeliveryExecutionReceipt {
    pub job_name: String,
}

#[derive(Debug, Clone)]
pub struct GitOpsDeliveryObservationRequest {
    pub gitops_change_set_id: String,
    pub execution_id: String,
}

#[derive(Debug, Clone)]
pub struct GitOpsDeliveryObservationReceipt {
    pub job_name: String,
}

#[derive(Debug, Clone)]
pub struct GitWriterSettings {
    pub allowed_repos: Vec<String>,
    pub github_api_url: String,
    pub author_name: String,
    pub author_email: String,
}

#[derive(Debug, Clone)]
pub struct GitObserverSettings {
    pub allowed_repos: Vec<String>,
    pub github_api_url: String,
}

#[derive(Debug, Clone)]
pub struct CapabilityVerificationOutcome {
    pub available: bool,
    pub principal: Option<String>,
    pub repository: Option<String>,
    pub permission: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EnvironmentPreparationReceipt {
    pub job_name: String,
}

#[derive(Clone)]
pub enum RunDispatcher {
    Disabled,
    Local(Box<LocalWorker>),
    Kubernetes(Arc<KubernetesJobDispatcher>),
}

impl RunDispatcher {
    pub fn enabled(&self) -> bool {
        !matches!(self, Self::Disabled)
    }

    pub fn supports_local_workspace(&self) -> bool {
        matches!(self, Self::Local(_))
    }

    pub fn supports_remote_workspace(&self) -> bool {
        matches!(self, Self::Kubernetes(_))
    }

    pub fn mode(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Local(_) => "local",
            Self::Kubernetes(_) => "kubernetes_job",
        }
    }

    pub fn execution_target_kind(&self) -> &'static str {
        match self {
            Self::Kubernetes(_) => "kubernetes_job",
            _ => "local_process",
        }
    }

    /// The workspace the run actually executes in. Kubernetes attempts run
    /// in the Job workspace volume, not in an operator-local path.
    pub fn effective_cwd(&self, requested: &str) -> String {
        match self {
            Self::Kubernetes(dispatcher) => dispatcher.config.workspace_dir.clone(),
            _ => requested.to_string(),
        }
    }

    pub fn config_json(&self) -> serde_json::Value {
        match self {
            Self::Disabled => serde_json::json!({
                "enabled": false,
                "mode": self.mode(),
                "provider": null,
                "model": null,
                "base_url": null,
            }),
            Self::Local(worker) => {
                let config = worker.config();
                serde_json::json!({
                    "enabled": config.enabled,
                    "mode": self.mode(),
                    "provider": config.provider,
                    "model": config.model,
                    "base_url": config.base_url,
                })
            }
            Self::Kubernetes(dispatcher) => serde_json::json!({
                "enabled": true,
                "mode": self.mode(),
                "provider": "fireworks",
                "model": dispatcher.model,
                "base_url": dispatcher.base_url,
                "namespace": dispatcher.config.namespace,
                "image": dispatcher.config.image,
                "git_writer": {
                    "enabled": dispatcher.config.git_writer_enabled,
                    "available": dispatcher.git_writer_available(),
                    "service_account": dispatcher.config.git_writer_service_account,
                    "allowed_repos": dispatcher.config.git_writer_allowed_repos,
                    "github_api_url": dispatcher.config.git_writer_github_api_url,
                },
                "gitops_writer": {
                    "enabled": dispatcher.config.gitops_writer_enabled,
                    "available": dispatcher.gitops_writer_available(),
                    "service_account": dispatcher.config.gitops_writer_service_account,
                    "allowed_repos": dispatcher.config.gitops_writer_allowed_repos,
                    "github_api_url": dispatcher.config.gitops_writer_github_api_url,
                },
                "git_observer": {
                    "enabled": dispatcher.config.git_observer_enabled,
                    "available": dispatcher.git_observer_available(),
                    "service_account": dispatcher.config.git_observer_service_account,
                    "allowed_repos": dispatcher.config.git_observer_allowed_repos,
                    "github_api_url": dispatcher.config.git_observer_github_api_url,
                },
                "gitops_observer": {
                    "enabled": dispatcher.config.gitops_observer_enabled,
                    "available": dispatcher.gitops_observer_available(),
                    "service_account": dispatcher.config.gitops_observer_service_account,
                    "allowed_repos": dispatcher.config.gitops_observer_allowed_repos,
                    "github_api_url": dispatcher.config.gitops_observer_github_api_url,
                },
                "argo_executor": {
                    "enabled": dispatcher.config.argo_executor_enabled,
                    "available": dispatcher.argo_executor_available(),
                    "service_account": dispatcher.config.argo_executor_service_account,
                    "namespace": dispatcher.config.argo_executor_namespace,
                    "allowed_applications": dispatcher.config.argo_executor_allowed_applications,
                    "poll_seconds": dispatcher.config.argo_executor_poll_seconds,
                },
            }),
        }
    }

    pub async fn verify_capability(
        &self,
        capability: &str,
        repository: Option<&str>,
    ) -> anyhow::Result<CapabilityVerificationOutcome> {
        match self {
            Self::Kubernetes(dispatcher) => {
                dispatcher.verify_capability(capability, repository).await
            }
            _ => anyhow::bail!(
                "isolated capability verification requires kubernetes_job worker mode"
            ),
        }
    }

    pub async fn verify_environment_profile(
        &self,
        profile: &EnvironmentProfile,
    ) -> anyhow::Result<CapabilityVerificationOutcome> {
        match self {
            Self::Kubernetes(dispatcher) => dispatcher.verify_environment_profile(profile).await,
            _ => anyhow::bail!(
                "environment profile verification requires kubernetes_job worker mode"
            ),
        }
    }

    pub fn spawn_run(&self, run: StoredRun, cwd: String) {
        match self {
            Self::Disabled => {}
            Self::Local(worker) => worker.spawn_run(run, cwd),
            Self::Kubernetes(dispatcher) => dispatcher.clone().launch(run, None),
        }
    }

    pub fn resume_run(&self, run: StoredRun, approval: StoredApproval) {
        match self {
            Self::Disabled => {}
            Self::Local(worker) => worker.resume_run(run, approval),
            Self::Kubernetes(dispatcher) => dispatcher.clone().launch(run, Some(approval)),
        }
    }

    pub async fn dispatch_environment_preparation(
        &self,
        run: &StoredRun,
        profile: &EnvironmentProfile,
    ) -> anyhow::Result<EnvironmentPreparationReceipt> {
        match self {
            Self::Kubernetes(dispatcher) => {
                dispatcher
                    .create_environment_preparation_job(run, profile)
                    .await
            }
            Self::Disabled => {
                anyhow::bail!("environment preparation requires kubernetes_job worker mode")
            }
            Self::Local(_) => {
                anyhow::bail!("immutable environment profiles are unavailable in local worker mode")
            }
        }
    }

    pub fn cancel(&self, run_id: &pharness_core::RunId) -> bool {
        match self {
            Self::Disabled => false,
            Self::Local(worker) => worker.cancel(run_id),
            Self::Kubernetes(dispatcher) => {
                dispatcher.clone().delete_jobs_for_run(run_id.as_str());
                true
            }
        }
    }

    /// Create a purpose-built executor Job. Unlike a run worker, this Job has
    /// no model credentials and can submit exactly one validated PipelineRun.
    pub async fn dispatch_tekton_execution(
        &self,
        request: TektonExecutionRequest,
    ) -> anyhow::Result<TektonExecutionReceipt> {
        match self {
            Self::Kubernetes(dispatcher) => dispatcher.create_tekton_executor_job(&request).await,
            Self::Disabled => anyhow::bail!("Tekton execution requires kubernetes_job worker mode"),
            Self::Local(_) => anyhow::bail!("Tekton execution is unavailable in local worker mode"),
        }
    }

    pub fn argo_executor_available(&self) -> bool {
        matches!(self, Self::Kubernetes(dispatcher) if dispatcher.argo_executor_available())
    }

    pub fn argo_executor_allows_application(&self, application: &str) -> bool {
        matches!(self, Self::Kubernetes(dispatcher)
            if dispatcher.argo_executor_available()
                && dispatcher.config.argo_executor_allowed_applications.iter()
                    .any(|allowed| allowed == application))
    }

    /// Create an isolated Argo worker Job. The worker obtains its exact target
    /// only through the internal context route after the API has revalidated
    /// the DeploymentIntent's contract and supervised-autonomy envelope.
    pub async fn dispatch_argo_sync_execution(
        &self,
        request: ArgoSyncExecutionRequest,
    ) -> anyhow::Result<ArgoSyncExecutionReceipt> {
        match self {
            Self::Kubernetes(dispatcher) => dispatcher.create_argo_executor_job(&request).await,
            Self::Disabled => anyhow::bail!("Argo execution requires kubernetes_job worker mode"),
            Self::Local(_) => anyhow::bail!("Argo execution is unavailable in local worker mode"),
        }
    }

    pub fn git_writer_available(&self) -> bool {
        matches!(self, Self::Kubernetes(dispatcher) if dispatcher.git_writer_available())
    }

    pub fn git_writer_settings(&self) -> Option<GitWriterSettings> {
        match self {
            Self::Kubernetes(dispatcher) if dispatcher.git_writer_available() => {
                Some(GitWriterSettings {
                    allowed_repos: dispatcher.config.git_writer_allowed_repos.clone(),
                    github_api_url: dispatcher.config.git_writer_github_api_url.clone(),
                    author_name: dispatcher.config.git_writer_author_name.clone(),
                    author_email: dispatcher.config.git_writer_author_email.clone(),
                })
            }
            _ => None,
        }
    }

    pub fn gitops_writer_settings(&self) -> Option<GitWriterSettings> {
        match self {
            Self::Kubernetes(dispatcher) if dispatcher.gitops_writer_available() => {
                Some(GitWriterSettings {
                    allowed_repos: dispatcher.config.gitops_writer_allowed_repos.clone(),
                    github_api_url: dispatcher.config.gitops_writer_github_api_url.clone(),
                    author_name: dispatcher.config.gitops_writer_author_name.clone(),
                    author_email: dispatcher.config.gitops_writer_author_email.clone(),
                })
            }
            _ => None,
        }
    }

    pub async fn dispatch_git_delivery(
        &self,
        request: GitDeliveryExecutionRequest,
    ) -> anyhow::Result<GitDeliveryExecutionReceipt> {
        match self {
            Self::Kubernetes(dispatcher) => dispatcher.create_git_writer_job(&request).await,
            Self::Disabled => anyhow::bail!("Git delivery requires kubernetes_job worker mode"),
            Self::Local(_) => anyhow::bail!("Git delivery is unavailable in local worker mode"),
        }
    }

    /// Dispatch a separate GitOps writer Job. It is intentionally distinct
    /// from the application source writer and can only receive its own token.
    pub async fn dispatch_gitops_delivery(
        &self,
        request: GitOpsDeliveryExecutionRequest,
    ) -> anyhow::Result<GitOpsDeliveryExecutionReceipt> {
        match self {
            Self::Kubernetes(dispatcher) => dispatcher.create_gitops_writer_job(&request).await,
            Self::Disabled => anyhow::bail!("GitOps delivery requires kubernetes_job worker mode"),
            Self::Local(_) => anyhow::bail!("GitOps delivery is unavailable in local worker mode"),
        }
    }

    pub async fn dispatch_gitops_delivery_observation(
        &self,
        request: GitOpsDeliveryObservationRequest,
    ) -> anyhow::Result<GitOpsDeliveryObservationReceipt> {
        match self {
            Self::Kubernetes(dispatcher) => dispatcher.create_gitops_observer_job(&request).await,
            Self::Disabled => {
                anyhow::bail!("GitOps observation requires kubernetes_job worker mode")
            }
            Self::Local(_) => {
                anyhow::bail!("GitOps observation is unavailable in local worker mode")
            }
        }
    }

    pub fn git_observer_settings(&self) -> Option<GitObserverSettings> {
        match self {
            Self::Kubernetes(dispatcher) if dispatcher.git_observer_available() => {
                Some(GitObserverSettings {
                    allowed_repos: dispatcher.config.git_observer_allowed_repos.clone(),
                    github_api_url: dispatcher.config.git_observer_github_api_url.clone(),
                })
            }
            _ => None,
        }
    }

    pub fn gitops_observer_settings(&self) -> Option<GitObserverSettings> {
        match self {
            Self::Kubernetes(dispatcher) if dispatcher.gitops_observer_available() => {
                Some(GitObserverSettings {
                    allowed_repos: dispatcher.config.gitops_observer_allowed_repos.clone(),
                    github_api_url: dispatcher.config.gitops_observer_github_api_url.clone(),
                })
            }
            _ => None,
        }
    }

    pub async fn dispatch_git_delivery_observation(
        &self,
        request: GitDeliveryObservationRequest,
    ) -> anyhow::Result<GitDeliveryObservationReceipt> {
        match self {
            Self::Kubernetes(dispatcher) => dispatcher.create_git_observer_job(&request).await,
            Self::Disabled => anyhow::bail!("Git observation requires kubernetes_job worker mode"),
            Self::Local(_) => anyhow::bail!("Git observation is unavailable in local worker mode"),
        }
    }

    /// Dispatch a read-only GitHub ref resolver through the observer identity.
    /// This identity has no model or Git write credentials.
    pub async fn dispatch_gitops_revision_resolution(
        &self,
        request: GitOpsRevisionResolutionRequest,
    ) -> anyhow::Result<GitOpsRevisionResolutionReceipt> {
        match self {
            Self::Kubernetes(dispatcher) => {
                dispatcher
                    .create_gitops_revision_resolver_job(&request)
                    .await
            }
            Self::Disabled => {
                anyhow::bail!("GitOps revision resolution requires kubernetes_job worker mode")
            }
            Self::Local(_) => {
                anyhow::bail!("GitOps revision resolution is unavailable in local worker mode")
            }
        }
    }
}

pub struct KubernetesJobDispatcher {
    store: Arc<SqliteStore>,
    kubectl_bin: String,
    config: WorkerKubernetesConfig,
    model: String,
    base_url: String,
    worker_env: Vec<(String, String)>,
}

impl KubernetesJobDispatcher {
    pub fn new(
        store: Arc<SqliteStore>,
        kubectl_bin: String,
        config: WorkerKubernetesConfig,
        model: String,
        base_url: String,
        worker_env: Vec<(String, String)>,
    ) -> Arc<Self> {
        let dispatcher = Arc::new(Self {
            store,
            kubectl_bin,
            config,
            model,
            base_url,
            worker_env,
        });
        dispatcher.clone().spawn_reaper();
        dispatcher
    }

    fn launch(self: Arc<Self>, run: StoredRun, approval: Option<StoredApproval>) {
        tokio::spawn(async move {
            let run_id = run.id.clone();
            if let Err(error) = self.create_job(&run, approval.as_ref()).await {
                tracing::error!(run_id = %run_id, %error, "failed to launch worker job");
                let _ = fail_run_from_dispatch(
                    &self.store,
                    &run_id,
                    format!("failed to launch worker job: {error}"),
                )
                .await;
            }
        });
    }

    fn git_writer_available(&self) -> bool {
        self.config.git_writer_enabled
            && self.config.git_writer_token_secret_name.is_some()
            && !self.config.git_writer_allowed_repos.is_empty()
    }

    fn gitops_writer_available(&self) -> bool {
        self.config.gitops_writer_enabled
            && self.config.gitops_writer_token_secret_name.is_some()
            && !self.config.gitops_writer_allowed_repos.is_empty()
    }

    fn argo_executor_available(&self) -> bool {
        self.config.argo_executor_enabled
            && !self.config.argo_executor_allowed_applications.is_empty()
    }

    fn git_observer_available(&self) -> bool {
        self.config.git_observer_enabled
            && self.config.git_observer_token_secret_name.is_some()
            && !self.config.git_observer_allowed_repos.is_empty()
    }

    fn gitops_observer_available(&self) -> bool {
        self.config.gitops_observer_enabled
            && self.config.gitops_observer_token_secret_name.is_some()
            && !self.config.gitops_observer_allowed_repos.is_empty()
    }

    async fn verify_capability(
        &self,
        capability: &str,
        repository: Option<&str>,
    ) -> anyhow::Result<CapabilityVerificationOutcome> {
        let (principal, permission, repository, manifest) =
            self.capability_preflight_manifest(capability, repository)?;
        let job_name = manifest
            .pointer("/metadata/name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("capability preflight Job has no name"))?
            .to_string();
        create_job_from_manifest(&self.kubectl_bin, &self.config.namespace, &manifest).await?;
        let mut available = false;
        for _ in 0..60 {
            let output = tokio::process::Command::new(&self.kubectl_bin)
                .args([
                    "get",
                    "job",
                    &job_name,
                    "-n",
                    &self.config.namespace,
                    "-o",
                    "json",
                ])
                .output()
                .await?;
            if !output.status.success() {
                break;
            }
            let job: serde_json::Value = serde_json::from_slice(&output.stdout)?;
            if job
                .pointer("/status/succeeded")
                .and_then(serde_json::Value::as_u64)
                == Some(1)
            {
                available = true;
                break;
            }
            if job
                .pointer("/status/failed")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0
            {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        let _ = tokio::process::Command::new(&self.kubectl_bin)
            .args([
                "delete",
                "job",
                &job_name,
                "-n",
                &self.config.namespace,
                "--ignore-not-found=true",
                "--wait=false",
            ])
            .output()
            .await;
        Ok(CapabilityVerificationOutcome {
            available,
            principal: Some(principal),
            repository,
            permission: Some(permission),
        })
    }

    async fn verify_environment_profile(
        &self,
        profile: &EnvironmentProfile,
    ) -> anyhow::Result<CapabilityVerificationOutcome> {
        profile.validate().map_err(|error| anyhow::anyhow!(error))?;
        let suffix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        let job_name = format!("pharness-profile-preflight-{suffix}");
        let executable_checks = profile
            .required_executables
            .iter()
            .map(|executable| format!("command -v {executable} >/dev/null"))
            .collect::<Vec<_>>()
            .join(" && ");
        let script = format!(
            "set -eu; test \"$(uname -s)\" = Linux; test \"$(uname -m)\" = x86_64; test \"$PHARNESS_BUILD_REVISION\" = \"$EXPECTED_REVISION\"; {executable_checks}; python -m venv /tmp/profile-venv; /tmp/profile-venv/bin/pip --version >/dev/null; git ls-remote --exit-code \"$PROFILE_REPOSITORY\" HEAD >/dev/null; /tmp/profile-venv/bin/python -c \"import urllib.request; urllib.request.urlopen('https://pypi.org/simple/', timeout=15).read(1)\""
        );
        let proxy_url = format!(
            "http://pharness-preparation-egress-proxy.{}.svc.cluster.local:8080",
            self.config.namespace
        );
        let repository = profile
            .repository_allowlist
            .first()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("environment profile repository allowlist is empty"))?;
        let manifest = serde_json::json!({
            "apiVersion": "batch/v1",
            "kind": "Job",
            "metadata": { "name": job_name, "namespace": self.config.namespace },
            "spec": {
                "backoffLimit": 0,
                "activeDeadlineSeconds": 120,
                "ttlSecondsAfterFinished": 300,
                "template": {
                    "metadata": { "labels": {
                        "app": "pharness-runner",
                        "agentic.lucas.engineering/phase": "preparation",
                        "agentic.lucas.engineering/environment-profile": profile.id,
                    }},
                    "spec": {
                        "serviceAccountName": profile.service_account,
                        "restartPolicy": "Never",
                        "securityContext": {
                            "runAsNonRoot": true,
                            "runAsUser": 65532,
                            "runAsGroup": 65532,
                            "seccompProfile": { "type": "RuntimeDefault" },
                        },
                        "containers": [{
                            "name": "verify",
                            "image": profile.image,
                            "imagePullPolicy": "IfNotPresent",
                            "command": ["/bin/sh", "-c", script],
                            "env": [
                                { "name": "EXPECTED_REVISION", "value": profile.revision },
                                { "name": "PROFILE_REPOSITORY", "value": repository },
                                { "name": "HTTPS_PROXY", "value": proxy_url },
                                { "name": "https_proxy", "value": proxy_url },
                                { "name": "NO_PROXY", "value": ".svc,.cluster.local,127.0.0.1,localhost" },
                                { "name": "no_proxy", "value": ".svc,.cluster.local,127.0.0.1,localhost" }
                            ],
                            "securityContext": {
                                "allowPrivilegeEscalation": false,
                                "readOnlyRootFilesystem": true,
                                "capabilities": { "drop": ["ALL"] },
                            },
                            "volumeMounts": [{ "name": "tmp", "mountPath": "/tmp" }],
                            "resources": {
                                "requests": { "cpu": "50m", "memory": "128Mi" },
                                "limits": { "cpu": profile.limits.cpu, "memory": profile.limits.memory },
                            },
                        }],
                        "volumes": [{ "name": "tmp", "emptyDir": {} }],
                    },
                },
            },
        });
        create_job_from_manifest(&self.kubectl_bin, &self.config.namespace, &manifest).await?;
        let mut available = false;
        for _ in 0..120 {
            let output = tokio::process::Command::new(&self.kubectl_bin)
                .args([
                    "get",
                    "job",
                    &job_name,
                    "-n",
                    &self.config.namespace,
                    "-o",
                    "json",
                ])
                .output()
                .await?;
            if !output.status.success() {
                break;
            }
            let job: serde_json::Value = serde_json::from_slice(&output.stdout)?;
            if job
                .pointer("/status/succeeded")
                .and_then(serde_json::Value::as_u64)
                == Some(1)
            {
                available = true;
                break;
            }
            if job
                .pointer("/status/failed")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0
            {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        let _ = tokio::process::Command::new(&self.kubectl_bin)
            .args([
                "delete",
                "job",
                &job_name,
                "-n",
                &self.config.namespace,
                "--ignore-not-found=true",
                "--wait=false",
            ])
            .output()
            .await;
        Ok(CapabilityVerificationOutcome {
            available,
            principal: Some(format!(
                "system:serviceaccount:{}:{}",
                self.config.namespace, profile.service_account
            )),
            repository: Some(repository),
            permission: Some(
                "runner_revision_platform_executables_venv_and_preparation_egress".to_string(),
            ),
        })
    }

    fn capability_preflight_manifest(
        &self,
        capability: &str,
        requested_repository: Option<&str>,
    ) -> anyhow::Result<(String, String, Option<String>, serde_json::Value)> {
        let suffix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        let job_name = format!("pharness-cap-preflight-{suffix}");
        let mut env = Vec::new();
        let proxy_phase = match capability {
            "model_provider" => Some("coding"),
            "source_workspace" => Some("preparation"),
            _ => None,
        };
        if let Some(phase) = proxy_phase {
            let proxy_url = format!(
                "http://pharness-{phase}-egress-proxy.{}.svc.cluster.local:8080",
                self.config.namespace
            );
            env.extend([
                serde_json::json!({"name":"HTTPS_PROXY","value":proxy_url}),
                serde_json::json!({"name":"https_proxy","value":proxy_url}),
                serde_json::json!({"name":"NO_PROXY","value":".svc,.cluster.local,127.0.0.1,localhost"}),
                serde_json::json!({"name":"no_proxy","value":".svc,.cluster.local,127.0.0.1,localhost"}),
            ]);
        }
        let (service_account, principal, permission, repository, script) = match capability {
            "model_provider" => {
                env.push(serde_json::json!({"name":"FIREWORKS_API_KEY","valueFrom":{"secretKeyRef":{"name":self.config.fireworks_secret_name,"key":"api-key"}}}));
                env.push(serde_json::json!({"name":"ENDPOINT","value":format!("{}/models", self.base_url.trim_end_matches('/'))}));
                (self.config.service_account.clone(), format!("system:serviceaccount:{}:{}", self.config.namespace, self.config.service_account), "provider_authenticate".to_string(), None, "curl -fsS -o /dev/null -H \"Authorization: Bearer $FIREWORKS_API_KEY\" \"$ENDPOINT\"".to_string())
            }
            "source_workspace" => {
                let repo = requested_repository.ok_or_else(|| anyhow::anyhow!("source workspace repository is unavailable"))?;
                env.push(serde_json::json!({"name":"REPOSITORY","value":repo}));
                (self.config.service_account.clone(), format!("system:serviceaccount:{}:{}", self.config.namespace, self.config.service_account), "repository_read".to_string(), Some(repo.to_string()), "git ls-remote --exit-code \"$REPOSITORY\" HEAD >/dev/null".to_string())
            }
            "source_writer" | "source_observer" | "gitops_writer" | "gitops_observer" => {
                let (enabled, service_account, secret, repos, required_permission) = match capability {
                    "source_writer" => (self.git_writer_available(), &self.config.git_writer_service_account, self.config.git_writer_token_secret_name.as_deref(), &self.config.git_writer_allowed_repos, "push"),
                    "source_observer" => (self.git_observer_available(), &self.config.git_observer_service_account, self.config.git_observer_token_secret_name.as_deref(), &self.config.git_observer_allowed_repos, "pull"),
                    "gitops_writer" => (self.gitops_writer_available(), &self.config.gitops_writer_service_account, self.config.gitops_writer_token_secret_name.as_deref(), &self.config.gitops_writer_allowed_repos, "push"),
                    _ => (self.gitops_observer_available(), &self.config.gitops_observer_service_account, self.config.gitops_observer_token_secret_name.as_deref(), &self.config.gitops_observer_allowed_repos, "pull"),
                };
                if !enabled { anyhow::bail!("{capability} is not configured"); }
                let repo = repos.first().ok_or_else(|| anyhow::anyhow!("{capability} repository allowlist is empty"))?;
                let repo_path = repo.trim_end_matches('/').trim_end_matches(".git").strip_prefix("https://github.com/").ok_or_else(|| anyhow::anyhow!("{capability} repository is not a safe GitHub HTTPS URL"))?;
                env.push(serde_json::json!({"name":"GITHUB_TOKEN","valueFrom":{"secretKeyRef":{"name":secret.expect("availability requires Secret"),"key":"token"}}}));
                env.push(serde_json::json!({"name":"REPOSITORY_API","value":format!("https://api.github.com/repos/{repo_path}")}));
                env.push(serde_json::json!({"name":"REQUIRED_PERMISSION","value":required_permission}));
                (service_account.clone(), format!("system:serviceaccount:{}:{}", self.config.namespace, service_account), format!("repository_{required_permission}"), Some(repo.clone()), "curl -fsS -H \"Authorization: Bearer $GITHUB_TOKEN\" -H \"Accept: application/vnd.github+json\" \"$REPOSITORY_API\" -o /tmp/repository.json && grep -Eq \"\\\"$REQUIRED_PERMISSION\\\"[[:space:]]*:[[:space:]]*true\" /tmp/repository.json".to_string())
            }
            "tekton" => (self.config.tekton_executor_service_account.clone(), format!("system:serviceaccount:{}:{}", self.config.namespace, self.config.tekton_executor_service_account), "create_pipelineruns".to_string(), None, format!("kubectl auth can-i create pipelineruns.tekton.dev -n {} | grep -qx yes", self.config.tekton_allowed_namespaces.first().ok_or_else(|| anyhow::anyhow!("Tekton namespace allowlist is empty"))?)),
            "argo" => (self.config.argo_executor_service_account.clone(), format!("system:serviceaccount:{}:{}", self.config.namespace, self.config.argo_executor_service_account), "get_and_patch_application".to_string(), None, format!("kubectl auth can-i get applications.argoproj.io/{} -n {} | grep -qx yes && kubectl auth can-i patch applications.argoproj.io/{} -n {} | grep -qx yes", self.config.argo_executor_allowed_applications.first().ok_or_else(|| anyhow::anyhow!("Argo Application allowlist is empty"))?, self.config.argo_executor_namespace, self.config.argo_executor_allowed_applications.first().unwrap(), self.config.argo_executor_namespace)),
            "observability" => {
                let url = self.worker_env.iter().find(|(name, _)| name == "PHARNESS_PROMETHEUS_URL").map(|(_, value)| value).ok_or_else(|| anyhow::anyhow!("Prometheus endpoint is not configured"))?;
                env.push(serde_json::json!({"name":"PROMETHEUS_URL","value":url}));
                (self.config.service_account.clone(), format!("system:serviceaccount:{}:{}", self.config.namespace, self.config.service_account), "prometheus_ready".to_string(), None, "curl -fsS -o /dev/null \"${PROMETHEUS_URL%/}/-/ready\"".to_string())
            }
            "yfinance_healthz" => (
                self.config.service_account.clone(),
                format!("system:serviceaccount:{}:{}", self.config.namespace, self.config.service_account),
                "get_apps_prod_yfinance_wrapper_healthz".to_string(),
                None,
                "curl -fsS -o /dev/null http://yfinance-wrapper.apps-prod.svc.cluster.local:8090/healthz".to_string(),
            ),
            _ => anyhow::bail!("unsupported capability {capability}"),
        };
        let mut pod_labels =
            serde_json::json!({JOB_NAME_LABEL:CAPABILITY_PREFLIGHT_JOB_NAME_VALUE});
        if let Some(phase) = proxy_phase {
            pod_labels["app"] = serde_json::json!("pharness-runner");
            pod_labels["agentic.lucas.engineering/phase"] = serde_json::json!(phase);
        }
        let manifest = serde_json::json!({
            "apiVersion":"batch/v1","kind":"Job",
            "metadata":{"name":job_name,"namespace":self.config.namespace,"labels":{JOB_NAME_LABEL:CAPABILITY_PREFLIGHT_JOB_NAME_VALUE}},
            "spec":{"backoffLimit":0,"activeDeadlineSeconds":75,"ttlSecondsAfterFinished":300,
                "template":{"metadata":{"labels":pod_labels},
                    "spec":{"serviceAccountName":service_account,"restartPolicy":"Never",
                        "securityContext":{"runAsNonRoot":true,"runAsUser":65532,"runAsGroup":65532,"seccompProfile":{"type":"RuntimeDefault"}},
                        "containers":[{"name":"preflight","image":self.config.image,"imagePullPolicy":"IfNotPresent","command":["/bin/sh","-ec"],"args":[script],"env":env,
                            "securityContext":{"allowPrivilegeEscalation":false,"readOnlyRootFilesystem":true,"capabilities":{"drop":["ALL"]}},
                            "volumeMounts":[{"name":"tmp","mountPath":"/tmp"}],
                            "resources":{"requests":{"cpu":"25m","memory":"64Mi"},"limits":{"cpu":"200m","memory":"128Mi"}}}],
                        "volumes":[{"name":"tmp","emptyDir":{}}]}}}
        });
        Ok((principal, permission, repository, manifest))
    }

    fn delete_jobs_for_run(self: Arc<Self>, run_id: &str) {
        let run_id = run_id.to_string();
        tokio::spawn(async move {
            let selector = format!("{RUN_ID_LABEL}={}", job_label_value(&run_id));
            let result = tokio::process::Command::new(&self.kubectl_bin)
                .args([
                    "delete",
                    "job",
                    "-n",
                    &self.config.namespace,
                    "-l",
                    &selector,
                    "--ignore-not-found=true",
                    "--wait=false",
                ])
                .output()
                .await;
            match result {
                Ok(output) if output.status.success() => {}
                Ok(output) => {
                    tracing::warn!(
                        run_id = %run_id,
                        stderr = %String::from_utf8_lossy(&output.stderr),
                        "failed to delete worker job"
                    );
                }
                Err(error) => {
                    tracing::warn!(run_id = %run_id, %error, "failed to spawn kubectl delete");
                }
            }
            if let Err(error) = self.delete_workspace_claim(&run_id).await {
                tracing::warn!(run_id = %run_id, %error, "failed to delete run workspace claim");
            }
        });
    }

    async fn create_job(
        &self,
        run: &StoredRun,
        approval: Option<&StoredApproval>,
    ) -> anyhow::Result<()> {
        self.ensure_run_job_capacity().await?;
        self.ensure_workspace_claim(run).await?;
        let manifest = self.job_manifest(run, approval);
        let payload = serde_json::to_vec(&manifest)?;

        let mut child = tokio::process::Command::new(&self.kubectl_bin)
            .args(["create", "-n", &self.config.namespace, "-f", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(&payload).await?;
        }
        let output = child.wait_with_output().await?;
        if !output.status.success() {
            anyhow::bail!(
                "kubectl create job failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        tracing::info!(
            run_id = %run.id,
            job = %job_name(run.id.as_str(), approval),
            resume = approval.is_some(),
            "created worker job"
        );

        Ok(())
    }

    async fn create_environment_preparation_job(
        &self,
        run: &StoredRun,
        profile: &EnvironmentProfile,
    ) -> anyhow::Result<EnvironmentPreparationReceipt> {
        profile
            .validate()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        self.ensure_run_job_capacity().await?;
        self.ensure_workspace_claim(run).await?;
        let manifest = self.environment_preparation_job_manifest(run, profile);
        create_job_from_manifest(&self.kubectl_bin, &self.config.namespace, &manifest).await?;
        let job_name = environment_preparation_job_name(run.id.as_str());
        tracing::info!(run_id = %run.id, job = %job_name, profile = %profile.id, "created isolated environment preparation job");
        Ok(EnvironmentPreparationReceipt { job_name })
    }

    async fn ensure_workspace_claim(&self, run: &StoredRun) -> anyhow::Result<()> {
        let name = workspace_claim_name(run.id.as_str());
        let output = tokio::process::Command::new(&self.kubectl_bin)
            .args([
                "get",
                "persistentvolumeclaim",
                &name,
                "-n",
                &self.config.namespace,
                "--ignore-not-found=true",
                "-o",
                "name",
            ])
            .output()
            .await?;
        if !output.status.success() {
            anyhow::bail!(
                "kubectl get workspace claim failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        if !output.stdout.is_empty() {
            return Ok(());
        }

        let payload = serde_json::to_vec(&self.workspace_claim_manifest(run))?;
        let mut child = tokio::process::Command::new(&self.kubectl_bin)
            .args(["create", "-n", &self.config.namespace, "-f", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(&payload).await?;
        }
        let output = child.wait_with_output().await?;
        if !output.status.success() {
            anyhow::bail!(
                "kubectl create workspace claim failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        tracing::info!(run_id = %run.id, claim = %name, "created durable run workspace claim");
        Ok(())
    }

    async fn delete_workspace_claim(&self, run_id: &str) -> anyhow::Result<()> {
        let output = tokio::process::Command::new(&self.kubectl_bin)
            .args([
                "delete",
                "persistentvolumeclaim",
                &workspace_claim_name(run_id),
                "-n",
                &self.config.namespace,
                "--ignore-not-found=true",
                "--wait=false",
            ])
            .output()
            .await?;
        if !output.status.success() {
            anyhow::bail!(
                "kubectl delete workspace claim failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    fn workspace_claim_manifest(&self, run: &StoredRun) -> serde_json::Value {
        let mut manifest = serde_json::json!({
            "apiVersion": "v1",
            "kind": "PersistentVolumeClaim",
            "metadata": {
                "name": workspace_claim_name(run.id.as_str()),
                "namespace": self.config.namespace,
                "labels": {
                    JOB_NAME_LABEL: WORKSPACE_CLAIM_NAME_VALUE,
                    RUN_ID_LABEL: job_label_value(run.id.as_str()),
                },
            },
            "spec": {
                "accessModes": ["ReadWriteOnce"],
                "resources": {
                    "requests": { "storage": self.config.workspace_size_limit },
                },
            },
        });
        if let Some(storage_class) = self.config.workspace_storage_class.as_deref() {
            manifest["spec"]["storageClassName"] = serde_json::json!(storage_class);
        }
        manifest
    }

    /// The API is deliberately single-replica while SQLite is the durable
    /// store, so this read-before-create admission check is sufficient for the
    /// first bounded coding-worker pool. A future multi-worker controller must
    /// replace it with durable queue admission.
    async fn ensure_run_job_capacity(&self) -> anyhow::Result<()> {
        let selector = format!("{JOB_NAME_LABEL}={JOB_NAME_VALUE}");
        let output = tokio::process::Command::new(&self.kubectl_bin)
            .args([
                "get",
                "jobs",
                "-n",
                &self.config.namespace,
                "-l",
                &selector,
                "-o",
                "json",
            ])
            .output()
            .await?;
        if !output.status.success() {
            anyhow::bail!(
                "kubectl get worker jobs for capacity check failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let jobs: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        enforce_run_job_capacity(&jobs, self.config.max_concurrent_run_jobs)
    }

    async fn create_tekton_executor_job(
        &self,
        request: &TektonExecutionRequest,
    ) -> anyhow::Result<TektonExecutionReceipt> {
        if self.config.tekton_allowed_namespaces.is_empty()
            || !self
                .config
                .tekton_allowed_namespaces
                .iter()
                .any(|namespace| namespace == &request.target_namespace)
        {
            anyhow::bail!(
                "Tekton execution target namespace {} is not allowlisted",
                request.target_namespace
            );
        }

        let job_name = tekton_executor_job_name(&request.execution_id);
        let manifest = self.tekton_executor_job_manifest(request, &job_name);
        let payload = serde_json::to_vec(&manifest)?;
        let mut child = tokio::process::Command::new(&self.kubectl_bin)
            .args(["create", "-n", &self.config.namespace, "-f", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(&payload).await?;
        }
        let output = child.wait_with_output().await?;
        if !output.status.success() {
            anyhow::bail!(
                "kubectl create Tekton executor Job failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        tracing::info!(
            pipeline_intent_id = %request.pipeline_intent_id,
            execution_id = %request.execution_id,
            namespace = %request.target_namespace,
            job = %job_name,
            "created Tekton executor job"
        );
        Ok(TektonExecutionReceipt { job_name })
    }

    async fn create_git_writer_job(
        &self,
        request: &GitDeliveryExecutionRequest,
    ) -> anyhow::Result<GitDeliveryExecutionReceipt> {
        if !self.git_writer_available() {
            anyhow::bail!("Git writer executor is not configured");
        }
        let job_name = git_writer_job_name(&request.execution_id);
        let manifest = self.git_writer_job_manifest(request, &job_name);
        create_job_from_manifest(&self.kubectl_bin, &self.config.namespace, &manifest).await?;
        tracing::info!(
            change_set_id = %request.change_set_id,
            execution_id = %request.execution_id,
            job = %job_name,
            "created Git writer job"
        );
        Ok(GitDeliveryExecutionReceipt { job_name })
    }

    async fn create_argo_executor_job(
        &self,
        request: &ArgoSyncExecutionRequest,
    ) -> anyhow::Result<ArgoSyncExecutionReceipt> {
        if !self.argo_executor_available() {
            anyhow::bail!("Argo executor is not configured");
        }
        let job_name = argo_executor_job_name(&request.execution_id);
        let manifest = self.argo_executor_job_manifest(request, &job_name);
        create_job_from_manifest(&self.kubectl_bin, &self.config.namespace, &manifest).await?;
        tracing::info!(
            deployment_intent_id = %request.deployment_intent_id,
            execution_id = %request.execution_id,
            job = %job_name,
            "created Argo sync executor job"
        );
        Ok(ArgoSyncExecutionReceipt { job_name })
    }

    async fn create_git_observer_job(
        &self,
        request: &GitDeliveryObservationRequest,
    ) -> anyhow::Result<GitDeliveryObservationReceipt> {
        if !self.git_observer_available() {
            anyhow::bail!("Git observer executor is not configured");
        }
        let job_name = git_observer_job_name(&request.execution_id);
        let manifest = self.git_observer_job_manifest(request, &job_name);
        create_job_from_manifest(&self.kubectl_bin, &self.config.namespace, &manifest).await?;
        tracing::info!(
            change_set_id = %request.change_set_id,
            execution_id = %request.execution_id,
            job = %job_name,
            "created Git observer job"
        );
        Ok(GitDeliveryObservationReceipt { job_name })
    }

    async fn create_gitops_revision_resolver_job(
        &self,
        request: &GitOpsRevisionResolutionRequest,
    ) -> anyhow::Result<GitOpsRevisionResolutionReceipt> {
        if !self.gitops_observer_available() {
            anyhow::bail!(
                "GitOps observer identity is not configured for read-only GitOps resolution"
            );
        }
        let job_name = gitops_revision_resolver_job_name(&request.execution_id);
        let manifest = self.gitops_revision_resolver_job_manifest(request, &job_name);
        create_job_from_manifest(&self.kubectl_bin, &self.config.namespace, &manifest).await?;
        tracing::info!(
            gitops_change_set_id = %request.gitops_change_set_id,
            execution_id = %request.execution_id,
            job = %job_name,
            "created read-only GitOps revision resolver job"
        );
        Ok(GitOpsRevisionResolutionReceipt { job_name })
    }

    async fn create_gitops_writer_job(
        &self,
        request: &GitOpsDeliveryExecutionRequest,
    ) -> anyhow::Result<GitOpsDeliveryExecutionReceipt> {
        if !self.gitops_writer_available() {
            anyhow::bail!("GitOps writer executor is not configured");
        }
        let job_name = gitops_writer_job_name(&request.execution_id);
        let manifest = self.gitops_writer_job_manifest(request, &job_name);
        create_job_from_manifest(&self.kubectl_bin, &self.config.namespace, &manifest).await?;
        tracing::info!(
            gitops_change_set_id = %request.gitops_change_set_id,
            execution_id = %request.execution_id,
            job = %job_name,
            "created GitOps writer job"
        );
        Ok(GitOpsDeliveryExecutionReceipt { job_name })
    }

    async fn create_gitops_observer_job(
        &self,
        request: &GitOpsDeliveryObservationRequest,
    ) -> anyhow::Result<GitOpsDeliveryObservationReceipt> {
        if !self.gitops_observer_available() {
            anyhow::bail!("GitOps observer executor is not configured");
        }
        let job_name = gitops_observer_job_name(&request.execution_id);
        let manifest = self.gitops_observer_job_manifest(request, &job_name);
        create_job_from_manifest(&self.kubectl_bin, &self.config.namespace, &manifest).await?;
        Ok(GitOpsDeliveryObservationReceipt { job_name })
    }

    fn gitops_observer_job_manifest(
        &self,
        request: &GitOpsDeliveryObservationRequest,
        job_name: &str,
    ) -> serde_json::Value {
        let token_secret = self
            .config
            .gitops_observer_token_secret_name
            .as_deref()
            .expect("GitOps observer availability validates token Secret");
        serde_json::json!({
            "apiVersion":"batch/v1", "kind":"Job",
            "metadata":{"name":job_name,"namespace":self.config.namespace,
                "labels":{JOB_NAME_LABEL:GIT_OBSERVER_JOB_NAME_VALUE,GITOPS_CHANGE_SET_LABEL:job_label_value(&request.gitops_change_set_id)},
                "annotations":{EXECUTION_ID_ANNOTATION:request.execution_id}},
            "spec":{"backoffLimit":0,"activeDeadlineSeconds":self.config.gitops_observer_active_deadline_seconds,"ttlSecondsAfterFinished":self.config.gitops_observer_ttl_seconds_after_finished,
                "template":{"metadata":{"labels":{JOB_NAME_LABEL:GIT_OBSERVER_JOB_NAME_VALUE,GITOPS_CHANGE_SET_LABEL:job_label_value(&request.gitops_change_set_id)}},
                    "spec":{"serviceAccountName":self.config.gitops_observer_service_account,"restartPolicy":"Never",
                        "securityContext":{"runAsNonRoot":true,"runAsUser":65532,"runAsGroup":65532,"seccompProfile":{"type":"RuntimeDefault"}},
                        "containers":[{"name":"gitops-observer","image":self.config.image,"imagePullPolicy":"Always","command":["pharness-worker"],
                            "env":[
                                {"name":"PHARNESS_EXECUTION_KIND","value":"gitops_delivery_observe"},
                                {"name":"PHARNESS_API_URL","value":self.config.api_url},
                                {"name":"PHARNESS_GITOPS_CHANGE_SET_ID","value":request.gitops_change_set_id},
                                {"name":"PHARNESS_GITOPS_DELIVERY_OBSERVATION_EXECUTION_ID","value":request.execution_id},
                                {"name":"PHARNESS_WORKER_TOKEN","valueFrom":{"secretKeyRef":{"name":self.config.worker_token_secret_name,"key":"token"}}},
                                {"name":"PHARNESS_GIT_OBSERVER_TOKEN","valueFrom":{"secretKeyRef":{"name":token_secret,"key":"token"}}}
                            ],
                            "securityContext":{"allowPrivilegeEscalation":false,"readOnlyRootFilesystem":true,"capabilities":{"drop":["ALL"]}},
                            "resources":{"requests":{"cpu":"50m","memory":"128Mi","ephemeral-storage":"128Mi"},"limits":{"cpu":"250m","memory":"256Mi","ephemeral-storage":"256Mi"}}
                        }]}}}
        })
    }

    fn gitops_writer_job_manifest(
        &self,
        request: &GitOpsDeliveryExecutionRequest,
        job_name: &str,
    ) -> serde_json::Value {
        let token_secret = self
            .config
            .gitops_writer_token_secret_name
            .as_deref()
            .expect("GitOps writer availability validates token Secret");
        serde_json::json!({
            "apiVersion": "batch/v1",
            "kind": "Job",
            "metadata": {
                "name": job_name,
                "namespace": self.config.namespace,
                "labels": {
                    JOB_NAME_LABEL: GITOPS_WRITER_JOB_NAME_VALUE,
                    GITOPS_CHANGE_SET_LABEL: job_label_value(&request.gitops_change_set_id),
                },
                "annotations": { EXECUTION_ID_ANNOTATION: request.execution_id },
            },
            "spec": {
                "backoffLimit": 0,
                "activeDeadlineSeconds": self.config.gitops_writer_active_deadline_seconds,
                "ttlSecondsAfterFinished": self.config.gitops_writer_ttl_seconds_after_finished,
                "template": {
                    "metadata": { "labels": {
                        JOB_NAME_LABEL: GITOPS_WRITER_JOB_NAME_VALUE,
                        GITOPS_CHANGE_SET_LABEL: job_label_value(&request.gitops_change_set_id),
                    }},
                    "spec": {
                        "serviceAccountName": self.config.gitops_writer_service_account,
                        "restartPolicy": "Never",
                        "securityContext": {
                            "runAsNonRoot": true,
                            "runAsUser": 65532,
                            "runAsGroup": 65532,
                            "fsGroup": 65532,
                            "seccompProfile": { "type": "RuntimeDefault" },
                        },
                        "containers": [{
                            "name": "gitops-writer",
                            "image": self.config.image,
                            "imagePullPolicy": "Always",
                            "command": ["pharness-worker"],
                            "env": [
                                { "name": "PHARNESS_EXECUTION_KIND", "value": "gitops_delivery" },
                                { "name": "PHARNESS_API_URL", "value": self.config.api_url },
                                { "name": "PHARNESS_GITOPS_CHANGE_SET_ID", "value": request.gitops_change_set_id },
                                { "name": "PHARNESS_GITOPS_DELIVERY_EXECUTION_ID", "value": request.execution_id },
                                { "name": "HOME", "value": "/work" },
                                { "name": "TMPDIR", "value": "/work/tmp" },
                                { "name": "PHARNESS_WORKER_TOKEN", "valueFrom": {
                                    "secretKeyRef": { "name": self.config.worker_token_secret_name, "key": "token" }
                                }},
                                { "name": "PHARNESS_GIT_WRITER_TOKEN", "valueFrom": {
                                    "secretKeyRef": { "name": token_secret, "key": "token" }
                                }},
                            ],
                            "volumeMounts": [{ "name": "work", "mountPath": "/work" }],
                            "securityContext": {
                                "allowPrivilegeEscalation": false,
                                "readOnlyRootFilesystem": true,
                                "capabilities": { "drop": ["ALL"] },
                            },
                            "resources": {
                                "requests": { "cpu": "100m", "memory": "256Mi", "ephemeral-storage": "1Gi" },
                                "limits": { "cpu": "1", "memory": "1Gi", "ephemeral-storage": "2Gi" },
                            },
                        }],
                        "volumes": [{ "name": "work", "emptyDir": { "sizeLimit": "2Gi" } }],
                    },
                },
            },
        })
    }

    fn git_writer_job_manifest(
        &self,
        request: &GitDeliveryExecutionRequest,
        job_name: &str,
    ) -> serde_json::Value {
        let token_secret = self
            .config
            .git_writer_token_secret_name
            .as_deref()
            .expect("Git writer availability validates token Secret");
        serde_json::json!({
            "apiVersion": "batch/v1",
            "kind": "Job",
            "metadata": {
                "name": job_name,
                "namespace": self.config.namespace,
                "labels": {
                    JOB_NAME_LABEL: GIT_WRITER_JOB_NAME_VALUE,
                    CHANGE_SET_LABEL: job_label_value(&request.change_set_id),
                },
                "annotations": { EXECUTION_ID_ANNOTATION: request.execution_id },
            },
            "spec": {
                "backoffLimit": 0,
                "activeDeadlineSeconds": self.config.git_writer_active_deadline_seconds,
                "ttlSecondsAfterFinished": self.config.git_writer_ttl_seconds_after_finished,
                "template": {
                    "metadata": { "labels": {
                        JOB_NAME_LABEL: GIT_WRITER_JOB_NAME_VALUE,
                        CHANGE_SET_LABEL: job_label_value(&request.change_set_id),
                    }},
                    "spec": {
                        "serviceAccountName": self.config.git_writer_service_account,
                        "restartPolicy": "Never",
                        "securityContext": {
                            "runAsNonRoot": true,
                            "runAsUser": 65532,
                            "runAsGroup": 65532,
                            "fsGroup": 65532,
                            "seccompProfile": { "type": "RuntimeDefault" },
                        },
                        "containers": [{
                            "name": "git-writer",
                            "image": self.config.image,
                            "imagePullPolicy": "Always",
                            "command": ["pharness-worker"],
                            "env": [
                                { "name": "PHARNESS_EXECUTION_KIND", "value": "git_delivery" },
                                { "name": "PHARNESS_API_URL", "value": self.config.api_url },
                                { "name": "PHARNESS_CHANGE_SET_ID", "value": request.change_set_id },
                                { "name": "PHARNESS_GIT_DELIVERY_EXECUTION_ID", "value": request.execution_id },
                                { "name": "HOME", "value": "/work" },
                                { "name": "TMPDIR", "value": "/work/tmp" },
                                { "name": "PHARNESS_WORKER_TOKEN", "valueFrom": {
                                    "secretKeyRef": { "name": self.config.worker_token_secret_name, "key": "token" }
                                }},
                                { "name": "PHARNESS_GIT_WRITER_TOKEN", "valueFrom": {
                                    "secretKeyRef": { "name": token_secret, "key": "token" }
                                }},
                            ],
                            "volumeMounts": [{ "name": "work", "mountPath": "/work" }],
                            "securityContext": {
                                "allowPrivilegeEscalation": false,
                                "readOnlyRootFilesystem": true,
                                "capabilities": { "drop": ["ALL"] },
                            },
                            "resources": {
                                "requests": { "cpu": "100m", "memory": "256Mi", "ephemeral-storage": "1Gi" },
                                "limits": { "cpu": "1", "memory": "1Gi", "ephemeral-storage": "2Gi" },
                            },
                        }],
                        "volumes": [{ "name": "work", "emptyDir": { "sizeLimit": "2Gi" } }],
                    },
                },
            },
        })
    }

    fn argo_executor_job_manifest(
        &self,
        request: &ArgoSyncExecutionRequest,
        job_name: &str,
    ) -> serde_json::Value {
        serde_json::json!({
            "apiVersion": "batch/v1",
            "kind": "Job",
            "metadata": {
                "name": job_name,
                "namespace": self.config.namespace,
                "labels": {
                    JOB_NAME_LABEL: ARGO_EXECUTOR_JOB_NAME_VALUE,
                    DEPLOYMENT_INTENT_LABEL: job_label_value(&request.deployment_intent_id),
                },
                "annotations": { EXECUTION_ID_ANNOTATION: request.execution_id },
            },
            "spec": {
                "backoffLimit": 0,
                "activeDeadlineSeconds": self.config.argo_executor_active_deadline_seconds,
                "ttlSecondsAfterFinished": self.config.argo_executor_ttl_seconds_after_finished,
                "template": {
                    "metadata": { "labels": {
                        JOB_NAME_LABEL: ARGO_EXECUTOR_JOB_NAME_VALUE,
                        DEPLOYMENT_INTENT_LABEL: job_label_value(&request.deployment_intent_id),
                    }},
                    "spec": {
                        "serviceAccountName": self.config.argo_executor_service_account,
                        "restartPolicy": "Never",
                        "securityContext": {
                            "runAsNonRoot": true,
                            "runAsUser": 65532,
                            "runAsGroup": 65532,
                            "seccompProfile": { "type": "RuntimeDefault" },
                        },
                        "containers": [{
                            "name": "argo-executor",
                            "image": self.config.image,
                            "imagePullPolicy": "Always",
                            "command": ["pharness-worker"],
                            "env": [
                                { "name": "PHARNESS_EXECUTION_KIND", "value": "argo_sync" },
                                { "name": "PHARNESS_API_URL", "value": self.config.api_url },
                                { "name": "PHARNESS_DEPLOYMENT_INTENT_ID", "value": request.deployment_intent_id },
                                { "name": "PHARNESS_ARGO_EXECUTION_ID", "value": request.execution_id },
                                { "name": "PHARNESS_ARGOCD_NAMESPACE", "value": self.config.argo_executor_namespace },
                                { "name": "PHARNESS_ARGO_EXECUTOR_POLL_SECONDS", "value": self.config.argo_executor_poll_seconds.to_string() },
                                { "name": "HOME", "value": "/tmp" },
                                { "name": "PHARNESS_WORKER_TOKEN", "valueFrom": {
                                    "secretKeyRef": {
                                        "name": self.config.worker_token_secret_name,
                                        "key": "token",
                                    }
                                }},
                            ],
                            "volumeMounts": [{ "name": "tmp", "mountPath": "/tmp" }],
                            "securityContext": {
                                "allowPrivilegeEscalation": false,
                                "readOnlyRootFilesystem": true,
                                "capabilities": { "drop": ["ALL"] },
                            },
                            "resources": {
                                "requests": { "cpu": "50m", "memory": "64Mi" },
                                "limits": { "cpu": "250m", "memory": "256Mi" },
                            },
                        }],
                        "volumes": [{ "name": "tmp", "emptyDir": {} }],
                    },
                },
            },
        })
    }

    fn git_observer_job_manifest(
        &self,
        request: &GitDeliveryObservationRequest,
        job_name: &str,
    ) -> serde_json::Value {
        let token_secret = self
            .config
            .git_observer_token_secret_name
            .as_deref()
            .expect("Git observer availability validates token Secret");
        serde_json::json!({
            "apiVersion": "batch/v1",
            "kind": "Job",
            "metadata": {
                "name": job_name,
                "namespace": self.config.namespace,
                "labels": {
                    JOB_NAME_LABEL: GIT_OBSERVER_JOB_NAME_VALUE,
                    CHANGE_SET_LABEL: job_label_value(&request.change_set_id),
                },
                "annotations": { EXECUTION_ID_ANNOTATION: request.execution_id },
            },
            "spec": {
                "backoffLimit": 0,
                "activeDeadlineSeconds": self.config.git_observer_active_deadline_seconds,
                "ttlSecondsAfterFinished": self.config.git_observer_ttl_seconds_after_finished,
                "template": {
                    "metadata": { "labels": {
                        JOB_NAME_LABEL: GIT_OBSERVER_JOB_NAME_VALUE,
                        CHANGE_SET_LABEL: job_label_value(&request.change_set_id),
                    }},
                    "spec": {
                        "serviceAccountName": self.config.git_observer_service_account,
                        "restartPolicy": "Never",
                        "securityContext": {
                            "runAsNonRoot": true,
                            "runAsUser": 65532,
                            "runAsGroup": 65532,
                            "seccompProfile": { "type": "RuntimeDefault" },
                        },
                        "containers": [{
                            "name": "git-observer",
                            "image": self.config.image,
                            "imagePullPolicy": "Always",
                            "command": ["pharness-worker"],
                            "env": [
                                { "name": "PHARNESS_EXECUTION_KIND", "value": "git_delivery_observe" },
                                { "name": "PHARNESS_API_URL", "value": self.config.api_url },
                                { "name": "PHARNESS_CHANGE_SET_ID", "value": request.change_set_id },
                                { "name": "PHARNESS_GIT_DELIVERY_EXECUTION_ID", "value": request.execution_id },
                                { "name": "PHARNESS_WORKER_TOKEN", "valueFrom": {
                                    "secretKeyRef": { "name": self.config.worker_token_secret_name, "key": "token" }
                                }},
                                { "name": "PHARNESS_GIT_OBSERVER_TOKEN", "valueFrom": {
                                    "secretKeyRef": { "name": token_secret, "key": "token" }
                                }},
                            ],
                            "securityContext": {
                                "allowPrivilegeEscalation": false,
                                "readOnlyRootFilesystem": true,
                                "capabilities": { "drop": ["ALL"] },
                            },
                            "resources": {
                                "requests": { "cpu": "50m", "memory": "128Mi", "ephemeral-storage": "128Mi" },
                                "limits": { "cpu": "250m", "memory": "256Mi", "ephemeral-storage": "256Mi" },
                            },
                        }],
                    },
                },
            },
        })
    }

    fn gitops_revision_resolver_job_manifest(
        &self,
        request: &GitOpsRevisionResolutionRequest,
        job_name: &str,
    ) -> serde_json::Value {
        let token_secret = self
            .config
            .gitops_observer_token_secret_name
            .as_deref()
            .expect("GitOps observer availability validates token Secret");
        serde_json::json!({
            "apiVersion": "batch/v1",
            "kind": "Job",
            "metadata": {
                "name": job_name,
                "namespace": self.config.namespace,
                "labels": {
                    JOB_NAME_LABEL: GITOPS_REVISION_RESOLVER_JOB_NAME_VALUE,
                    GITOPS_CHANGE_SET_LABEL: job_label_value(&request.gitops_change_set_id),
                },
                "annotations": { EXECUTION_ID_ANNOTATION: request.execution_id },
            },
            "spec": {
                "backoffLimit": 0,
                "activeDeadlineSeconds": self.config.gitops_observer_active_deadline_seconds,
                "ttlSecondsAfterFinished": self.config.gitops_observer_ttl_seconds_after_finished,
                "template": {
                    "metadata": { "labels": {
                        JOB_NAME_LABEL: GITOPS_REVISION_RESOLVER_JOB_NAME_VALUE,
                        GITOPS_CHANGE_SET_LABEL: job_label_value(&request.gitops_change_set_id),
                    }},
                    "spec": {
                        "serviceAccountName": self.config.gitops_observer_service_account,
                        "restartPolicy": "Never",
                        "securityContext": {
                            "runAsNonRoot": true,
                            "runAsUser": 65532,
                            "runAsGroup": 65532,
                            "seccompProfile": { "type": "RuntimeDefault" },
                        },
                        "containers": [{
                            "name": "gitops-revision-resolver",
                            "image": self.config.image,
                            "imagePullPolicy": "Always",
                            "command": ["pharness-worker"],
                            "env": [
                                { "name": "PHARNESS_EXECUTION_KIND", "value": "gitops_base_revision" },
                                { "name": "PHARNESS_API_URL", "value": self.config.api_url },
                                { "name": "PHARNESS_GITOPS_CHANGE_SET_ID", "value": request.gitops_change_set_id },
                                { "name": "PHARNESS_GITOPS_REVISION_EXECUTION_ID", "value": request.execution_id },
                                { "name": "PHARNESS_WORKER_TOKEN", "valueFrom": {
                                    "secretKeyRef": { "name": self.config.worker_token_secret_name, "key": "token" }
                                }},
                                { "name": "PHARNESS_GIT_OBSERVER_TOKEN", "valueFrom": {
                                    "secretKeyRef": { "name": token_secret, "key": "token" }
                                }},
                            ],
                            "securityContext": {
                                "allowPrivilegeEscalation": false,
                                "readOnlyRootFilesystem": true,
                                "capabilities": { "drop": ["ALL"] },
                            },
                            "resources": {
                                "requests": { "cpu": "50m", "memory": "128Mi", "ephemeral-storage": "128Mi" },
                                "limits": { "cpu": "250m", "memory": "256Mi", "ephemeral-storage": "256Mi" },
                            },
                        }],
                    },
                },
            },
        })
    }

    fn tekton_executor_job_manifest(
        &self,
        request: &TektonExecutionRequest,
        job_name: &str,
    ) -> serde_json::Value {
        serde_json::json!({
            "apiVersion": "batch/v1",
            "kind": "Job",
            "metadata": {
                "name": job_name,
                "namespace": self.config.namespace,
                "labels": {
                    JOB_NAME_LABEL: TEKTON_EXECUTOR_JOB_NAME_VALUE,
                    PIPELINE_INTENT_LABEL: job_label_value(&request.pipeline_intent_id),
                },
                "annotations": {
                    PIPELINE_INTENT_ID_ANNOTATION: request.pipeline_intent_id,
                    EXECUTION_ID_ANNOTATION: request.execution_id,
                },
            },
            "spec": {
                "backoffLimit": 0,
                "activeDeadlineSeconds": self.config.active_deadline_seconds,
                "ttlSecondsAfterFinished": self.config.ttl_seconds_after_finished,
                "template": {
                    "metadata": { "labels": {
                        JOB_NAME_LABEL: TEKTON_EXECUTOR_JOB_NAME_VALUE,
                        PIPELINE_INTENT_LABEL: job_label_value(&request.pipeline_intent_id),
                    }},
                    "spec": {
                        "serviceAccountName": self.config.tekton_executor_service_account,
                        "restartPolicy": "Never",
                        "securityContext": {
                            "runAsNonRoot": true,
                            "runAsUser": 65532,
                            "runAsGroup": 65532,
                            "seccompProfile": { "type": "RuntimeDefault" },
                        },
                        "containers": [{
                            "name": "tekton-executor",
                            "image": self.config.image,
                            "imagePullPolicy": "Always",
                            "command": ["pharness-worker"],
                            "env": [
                                { "name": "PHARNESS_EXECUTION_KIND", "value": "tekton_trigger" },
                                { "name": "PHARNESS_API_URL", "value": self.config.api_url },
                                { "name": "PHARNESS_PIPELINE_INTENT_ID", "value": request.pipeline_intent_id },
                                { "name": "PHARNESS_EXECUTION_ID", "value": request.execution_id },
                                { "name": "PHARNESS_TEKTON_PIPELINERUN_JSON", "value": request.pipeline_run_manifest.to_string() },
                                { "name": "PHARNESS_TEKTON_EXECUTOR_POLL_SECONDS", "value": self.config.tekton_executor_poll_seconds.to_string() },
                                { "name": "HOME", "value": "/tmp" },
                                { "name": "PHARNESS_WORKER_TOKEN", "valueFrom": {
                                    "secretKeyRef": {
                                        "name": self.config.worker_token_secret_name,
                                        "key": "token",
                                    }
                                }},
                            ],
                            "volumeMounts": [{ "name": "tmp", "mountPath": "/tmp" }],
                            "securityContext": {
                                "allowPrivilegeEscalation": false,
                                "readOnlyRootFilesystem": true,
                                "capabilities": { "drop": ["ALL"] },
                            },
                            "resources": {
                                "requests": { "cpu": "50m", "memory": "64Mi" },
                                "limits": { "cpu": "250m", "memory": "256Mi" },
                            },
                        }],
                        "volumes": [{ "name": "tmp", "emptyDir": {} }],
                    },
                },
            },
        })
    }

    fn job_manifest(
        &self,
        run: &StoredRun,
        approval: Option<&StoredApproval>,
    ) -> serde_json::Value {
        let job_name = job_name(run.id.as_str(), approval);
        let runner_profile = run.execution_target_json.get("runner_profile");
        let runner_image = runner_profile
            .and_then(|profile| profile.get("image"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&self.config.image);
        let service_account = runner_profile
            .and_then(|profile| profile.get("service_account"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&self.config.service_account);
        let cpu_limit = runner_profile
            .and_then(|profile| profile.pointer("/limits/cpu"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("1");
        let memory_limit = runner_profile
            .and_then(|profile| profile.pointer("/limits/memory"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("1Gi");
        let ephemeral_limit = runner_profile
            .and_then(|profile| profile.pointer("/limits/ephemeral_storage"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&self.config.workspace_ephemeral_storage_limit);
        let remaining_active_seconds = run
            .run_budget
            .active_execution_seconds
            .saturating_sub(run.budget_consumption.active_execution_seconds_used)
            .max(1);
        let active_deadline_seconds = self
            .config
            .active_deadline_seconds
            .min(remaining_active_seconds);
        let mut env = vec![
            serde_json::json!({ "name": "PHARNESS_API_URL", "value": self.config.api_url }),
            serde_json::json!({ "name": "PHARNESS_RUN_ID", "value": run.id.as_str() }),
            serde_json::json!({ "name": "HOME", "value": self.config.workspace_dir }),
            serde_json::json!({
                "name": "PATH",
                "value": format!("{}/.pharness-runtime/venv/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin", self.config.workspace_dir),
            }),
            serde_json::json!({
                "name": "HTTPS_PROXY",
                "value": format!("http://pharness-coding-egress-proxy.{}.svc.cluster.local:8080", self.config.namespace),
            }),
            serde_json::json!({
                "name": "https_proxy",
                "value": format!("http://pharness-coding-egress-proxy.{}.svc.cluster.local:8080", self.config.namespace),
            }),
            serde_json::json!({ "name": "NO_PROXY", "value": ".svc,.cluster.local,127.0.0.1,localhost" }),
            serde_json::json!({ "name": "no_proxy", "value": ".svc,.cluster.local,127.0.0.1,localhost" }),
            serde_json::json!({
                "name": "PHARNESS_WORKER_TOKEN",
                "valueFrom": {
                    "secretKeyRef": {
                        "name": self.config.worker_token_secret_name,
                        "key": "token",
                    }
                }
            }),
            serde_json::json!({
                "name": "FIREWORKS_API_KEY",
                "valueFrom": {
                    "secretKeyRef": {
                        "name": self.config.fireworks_secret_name,
                        "key": "api-key",
                    }
                }
            }),
        ];
        if let Some(approval) = approval {
            env.push(serde_json::json!({
                "name": "PHARNESS_APPROVAL_ID",
                "value": approval.id,
            }));
        }
        for (name, value) in &self.worker_env {
            env.push(serde_json::json!({ "name": name, "value": value }));
        }

        let mut manifest = serde_json::json!({
            "apiVersion": "batch/v1",
            "kind": "Job",
            "metadata": {
                "name": job_name,
                "namespace": self.config.namespace,
                "labels": {
                    JOB_NAME_LABEL: JOB_NAME_VALUE,
                    RUN_ID_LABEL: job_label_value(run.id.as_str()),
                },
            },
            "spec": {
                "backoffLimit": 0,
                "activeDeadlineSeconds": active_deadline_seconds,
                "ttlSecondsAfterFinished": self.config.ttl_seconds_after_finished,
                "template": {
                    "metadata": {
                        "labels": {
                            "app": "pharness-runner",
                            "agentic.lucas.engineering/phase": "coding",
                            JOB_NAME_LABEL: JOB_NAME_VALUE,
                            RUN_ID_LABEL: job_label_value(run.id.as_str()),
                        },
                    },
                    "spec": {
                        "serviceAccountName": service_account,
                        "restartPolicy": "Never",
                        "securityContext": {
                            "runAsNonRoot": true,
                            "runAsUser": 65532,
                            "runAsGroup": 65532,
                            "fsGroup": 65532,
                            "seccompProfile": { "type": "RuntimeDefault" },
                        },
                        "containers": [{
                            "name": "worker",
                            "image": runner_image,
                            "imagePullPolicy": "IfNotPresent",
                            "command": ["pharness-worker"],
                            "env": env,
                            "volumeMounts": [
                                {
                                    "name": "workspace",
                                    "mountPath": self.config.workspace_dir,
                                },
                                { "name": "tmp", "mountPath": "/tmp" },
                            ],
                            "securityContext": {
                                "allowPrivilegeEscalation": false,
                                "readOnlyRootFilesystem": true,
                                "capabilities": { "drop": ["ALL"] },
                            },
                            "resources": {
                                "requests": {
                                    "cpu": "100m",
                                    "memory": "256Mi",
                                    "ephemeral-storage": self.config.workspace_ephemeral_storage_request,
                                },
                                "limits": {
                                    "cpu": cpu_limit,
                                    "memory": memory_limit,
                                    "ephemeral-storage": ephemeral_limit,
                                },
                            },
                        }],
                        "volumes": [
                            {
                                "name": "workspace",
                                "persistentVolumeClaim": {
                                    "claimName": workspace_claim_name(run.id.as_str()),
                                },
                            },
                            { "name": "tmp", "emptyDir": {} },
                        ],
                    },
                },
            },
        });
        if let Some(hostname) = self.config.workspace_node_hostname.as_deref() {
            manifest["spec"]["template"]["spec"]["affinity"] = serde_json::json!({
                "nodeAffinity": {
                    "requiredDuringSchedulingIgnoredDuringExecution": {
                        "nodeSelectorTerms": [{
                            "matchExpressions": [{
                                "key": "kubernetes.io/hostname",
                                "operator": "In",
                                "values": [hostname],
                            }],
                        }],
                    },
                },
            });
        }
        manifest
    }

    fn environment_preparation_job_manifest(
        &self,
        run: &StoredRun,
        profile: &EnvironmentProfile,
    ) -> serde_json::Value {
        let job_name = environment_preparation_job_name(run.id.as_str());
        serde_json::json!({
            "apiVersion": "batch/v1",
            "kind": "Job",
            "metadata": {
                "name": job_name,
                "namespace": self.config.namespace,
                "labels": {
                    JOB_NAME_LABEL: ENVIRONMENT_PREPARATION_JOB_NAME_VALUE,
                    RUN_ID_LABEL: job_label_value(run.id.as_str()),
                },
            },
            "spec": {
                "backoffLimit": 0,
                "activeDeadlineSeconds": 1800,
                "ttlSecondsAfterFinished": self.config.ttl_seconds_after_finished,
                "template": {
                    "metadata": { "labels": {
                        "app": "pharness-runner",
                        "agentic.lucas.engineering/phase": "preparation",
                        JOB_NAME_LABEL: ENVIRONMENT_PREPARATION_JOB_NAME_VALUE,
                        RUN_ID_LABEL: job_label_value(run.id.as_str()),
                    }},
                    "spec": {
                        "serviceAccountName": profile.service_account,
                        "restartPolicy": "Never",
                        "securityContext": {
                            "runAsNonRoot": true,
                            "runAsUser": 65532,
                            "runAsGroup": 65532,
                            "fsGroup": 65532,
                            "seccompProfile": { "type": "RuntimeDefault" },
                        },
                        "containers": [{
                            "name": "prepare",
                            "image": profile.image,
                            "imagePullPolicy": "IfNotPresent",
                            "command": ["pharness-worker"],
                            "env": [
                                { "name": "PHARNESS_EXECUTION_KIND", "value": "environment_prepare" },
                                { "name": "PHARNESS_API_URL", "value": self.config.api_url },
                                { "name": "PHARNESS_RUN_ID", "value": run.id.as_str() },
                                { "name": "PHARNESS_ENVIRONMENT_PROFILE_ID", "value": profile.id },
                                { "name": "PHARNESS_RUNNER_IMAGE", "value": profile.image },
                                { "name": "PHARNESS_RUNNER_REVISION", "value": profile.revision },
                                { "name": "PHARNESS_RUNNER_PLATFORM", "value": profile.platform },
                                { "name": "PHARNESS_REQUIRED_EXECUTABLES_JSON", "value": serde_json::to_string(&profile.required_executables).unwrap_or_else(|_| "[]".to_string()) },
                                { "name": "HTTPS_PROXY", "value": format!("http://pharness-preparation-egress-proxy.{}.svc.cluster.local:8080", self.config.namespace) },
                                { "name": "https_proxy", "value": format!("http://pharness-preparation-egress-proxy.{}.svc.cluster.local:8080", self.config.namespace) },
                                { "name": "NO_PROXY", "value": ".svc,.cluster.local,127.0.0.1,localhost" },
                                { "name": "no_proxy", "value": ".svc,.cluster.local,127.0.0.1,localhost" },
                                { "name": "HOME", "value": self.config.workspace_dir },
                                { "name": "PHARNESS_WORKER_TOKEN", "valueFrom": {
                                    "secretKeyRef": { "name": self.config.worker_token_secret_name, "key": "token" }
                                }},
                            ],
                            "volumeMounts": [
                                { "name": "workspace", "mountPath": self.config.workspace_dir },
                                { "name": "tmp", "mountPath": "/tmp" },
                            ],
                            "securityContext": {
                                "allowPrivilegeEscalation": false,
                                "readOnlyRootFilesystem": true,
                                "capabilities": { "drop": ["ALL"] },
                            },
                            "resources": {
                                "requests": { "cpu": "100m", "memory": "256Mi", "ephemeral-storage": "512Mi" },
                                "limits": { "cpu": profile.limits.cpu, "memory": profile.limits.memory, "ephemeral-storage": profile.limits.ephemeral_storage },
                            },
                        }],
                        "volumes": [
                            { "name": "workspace", "persistentVolumeClaim": { "claimName": workspace_claim_name(run.id.as_str()) } },
                            { "name": "tmp", "emptyDir": {} },
                        ],
                    },
                },
            },
        })
    }

    /// Reconcile worker and executor jobs that stopped without reporting a
    /// durable outcome. The API remains the only SQLite writer.
    fn spawn_reaper(self: Arc<Self>) {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(REAPER_INTERVAL).await;
                if let Err(error) = self.reap_once().await {
                    tracing::warn!(%error, "worker job reaper pass failed");
                }
            }
        });
    }

    async fn reap_once(&self) -> anyhow::Result<()> {
        self.reap_run_jobs().await?;
        self.reap_run_workspace_claims().await?;
        self.reap_tekton_executor_jobs().await
    }

    async fn reap_run_workspace_claims(&self) -> anyhow::Result<()> {
        let selector = format!("{JOB_NAME_LABEL}={WORKSPACE_CLAIM_NAME_VALUE}");
        let output = tokio::process::Command::new(&self.kubectl_bin)
            .args([
                "get",
                "persistentvolumeclaims",
                "-n",
                &self.config.namespace,
                "-l",
                &selector,
                "-o",
                "json",
            ])
            .output()
            .await?;
        if !output.status.success() {
            anyhow::bail!(
                "kubectl get workspace claims failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let claims: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        for claim in claims
            .get("items")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(run_label) = claim
                .pointer("/metadata/labels")
                .and_then(|labels| labels.get(RUN_ID_LABEL))
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            let run_id_text = run_label_to_run_id(run_label);
            let run_id = pharness_core::RunId::new(run_id_text.clone());
            let terminal = self.store.get_run(&run_id).await?.map_or(true, |run| {
                matches!(run.status.as_str(), "completed" | "failed" | "cancelled")
            });
            if terminal {
                self.delete_workspace_claim(&run_id_text).await?;
            }
        }
        Ok(())
    }

    async fn reap_run_jobs(&self) -> anyhow::Result<()> {
        let selector = format!("{JOB_NAME_LABEL}={JOB_NAME_VALUE}");
        let output = tokio::process::Command::new(&self.kubectl_bin)
            .args([
                "get",
                "jobs",
                "-n",
                &self.config.namespace,
                "-l",
                &selector,
                "-o",
                "json",
            ])
            .output()
            .await?;
        if !output.status.success() {
            anyhow::bail!(
                "kubectl get jobs failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let jobs: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        let items = jobs
            .get("items")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();

        for job in items {
            let Some(run_label) = job
                .pointer("/metadata/labels")
                .and_then(|labels| labels.get(RUN_ID_LABEL))
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            let failed = job
                .pointer("/status/failed")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            if failed == 0 {
                continue;
            }

            let run_id = pharness_core::RunId::new(run_label_to_run_id(run_label));
            let Some(run) = self.store.get_run(&run_id).await? else {
                continue;
            };
            if matches!(run.status.as_str(), "queued" | "running") {
                tracing::warn!(run_id = %run_id, "worker job failed without durable outcome");
                fail_run_from_dispatch(
                    &self.store,
                    &run_id,
                    "worker job failed before reporting a durable outcome".to_string(),
                )
                .await?;
            }
        }

        Ok(())
    }

    async fn reap_tekton_executor_jobs(&self) -> anyhow::Result<()> {
        let selector = format!("{JOB_NAME_LABEL}={TEKTON_EXECUTOR_JOB_NAME_VALUE}");
        let output = tokio::process::Command::new(&self.kubectl_bin)
            .args([
                "get",
                "jobs",
                "-n",
                &self.config.namespace,
                "-l",
                &selector,
                "-o",
                "json",
            ])
            .output()
            .await?;
        if !output.status.success() {
            anyhow::bail!(
                "kubectl get Tekton executor jobs failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let jobs: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        let jobs = jobs
            .get("items")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut visible_job_names = std::collections::BTreeSet::new();
        for job in &jobs {
            if let Some(name) = job
                .pointer("/metadata/name")
                .and_then(serde_json::Value::as_str)
            {
                visible_job_names.insert(name.to_string());
            }
            let Some(intent_id) = job
                .pointer("/metadata/annotations")
                .and_then(|annotations| annotations.get(PIPELINE_INTENT_ID_ANNOTATION))
                .and_then(serde_json::Value::as_str)
            else {
                tracing::warn!("Tekton executor Job is missing PipelineIntent annotation");
                continue;
            };
            let Some(execution_id) = job
                .pointer("/metadata/annotations")
                .and_then(|annotations| annotations.get(EXECUTION_ID_ANNOTATION))
                .and_then(serde_json::Value::as_str)
            else {
                tracing::warn!(pipeline_intent_id = %intent_id, "Tekton executor Job is missing execution annotation");
                continue;
            };
            let Some(job_name) = job
                .pointer("/metadata/name")
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            let terminal = executor_job_terminal_state(job);
            if terminal == ExecutorJobTerminalState::Active {
                continue;
            }
            let reason = match terminal {
                ExecutorJobTerminalState::Failed => {
                    "Tekton executor Job failed before reporting a durable outcome"
                }
                ExecutorJobTerminalState::Succeeded => {
                    "Tekton executor Job completed without reporting a durable outcome"
                }
                ExecutorJobTerminalState::Active => unreachable!(),
            };
            self.fail_pipeline_intent_execution_if_current(
                intent_id,
                execution_id,
                job_name,
                reason,
            )
            .await?;
        }

        // A TTL controller or manual deletion can remove the Job before the
        // reaper sees its terminal state. Reconcile only executions already
        // dispatched to an executor Job; a freshly dispatching intent is not
        // considered missing.
        let executing = self
            .store
            .list_pipeline_intents(PipelineIntentListFilter {
                status: Some("executing".to_string()),
                limit: 200,
                ..PipelineIntentListFilter::default()
            })
            .await?;
        for intent in executing {
            let Some(job_name) = intent
                .intent_json
                .pointer("/execution_state/executor_job_name")
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            if visible_job_names.contains(job_name) {
                continue;
            }
            let Some(execution_id) = intent
                .intent_json
                .pointer("/execution_state/execution_id")
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            self.fail_pipeline_intent_execution_if_current(
                &intent.id,
                execution_id,
                job_name,
                "Tekton executor Job disappeared before reporting a durable outcome",
            )
            .await?;
        }

        Ok(())
    }

    async fn fail_pipeline_intent_execution_if_current(
        &self,
        pipeline_intent_id: &str,
        execution_id: &str,
        executor_job_name: &str,
        reason: &str,
    ) -> anyhow::Result<()> {
        let Some(intent) = self.store.get_pipeline_intent(pipeline_intent_id).await? else {
            tracing::warn!(
                pipeline_intent_id,
                "Tekton executor Job references an unknown PipelineIntent"
            );
            return Ok(());
        };
        if !execution_is_current(&intent, execution_id, executor_job_name) {
            return Ok(());
        }
        let mut intent_json = intent.intent_json.clone();
        replace_execution_state(
            &mut intent_json,
            serde_json::json!({
                "execution_id": execution_id,
                "state": "executor_job_lost",
                "executor_job_name": executor_job_name,
                "pipeline_run_namespace": intent.intent_json.pointer("/execution_state/pipeline_run_namespace"),
                "pipeline_run_name": intent.intent_json.pointer("/execution_state/pipeline_run_name"),
                "permission_grant_id": intent.intent_json.pointer("/execution_state/permission_grant_id"),
                "error": reason,
            }),
        );
        let intent = self
            .store
            .update_pipeline_intent_execution(
                &intent.id,
                UpdatePipelineIntentExecution {
                    status: "failed".to_string(),
                    intent_json,
                    actor: Some("system:executor-reaper".to_string()),
                    reason: Some(reason.to_string()),
                },
            )
            .await?;
        self.store
            .create_audit_event(CreateAuditEvent {
                id: format!("aud_{}_reaper_{}", intent.id, time_suffix()),
                kind: "pipeline_intent.execution_executor_lost".to_string(),
                actor: Some("system:executor-reaper".to_string()),
                resource_kind: "pipeline_intent".to_string(),
                resource_id: intent.id.clone(),
                run_id: intent.run_id.clone(),
                payload_json: serde_json::json!({
                    "pipeline_intent_id": intent.id,
                    "execution_id": execution_id,
                    "executor_job_name": executor_job_name,
                    "status": "failed",
                    "reason": reason,
                }),
            })
            .await?;
        tracing::warn!(
            pipeline_intent_id,
            execution_id,
            executor_job_name,
            reason,
            "reconciled missing Tekton executor outcome"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutorJobTerminalState {
    Active,
    Failed,
    Succeeded,
}

fn executor_job_terminal_state(job: &serde_json::Value) -> ExecutorJobTerminalState {
    if job
        .pointer("/status/failed")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0
    {
        ExecutorJobTerminalState::Failed
    } else if job
        .pointer("/status/succeeded")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0
    {
        ExecutorJobTerminalState::Succeeded
    } else {
        ExecutorJobTerminalState::Active
    }
}

fn active_run_job_count(jobs: &serde_json::Value) -> u64 {
    jobs.get("items")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|job| {
            job.pointer("/status/active")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
        })
        .sum()
}

fn enforce_run_job_capacity(
    jobs: &serde_json::Value,
    max_concurrent_jobs: u32,
) -> anyhow::Result<()> {
    let active = active_run_job_count(jobs);
    if active >= max_concurrent_jobs as u64 {
        anyhow::bail!(
            "worker Job concurrency limit reached: {active} active, limit {max_concurrent_jobs}"
        );
    }
    Ok(())
}

fn execution_is_current(
    intent: &StoredPipelineIntent,
    execution_id: &str,
    executor_job_name: &str,
) -> bool {
    intent.status == "executing"
        && intent
            .intent_json
            .pointer("/execution_state/execution_id")
            .and_then(serde_json::Value::as_str)
            == Some(execution_id)
        && intent
            .intent_json
            .pointer("/execution_state/executor_job_name")
            .and_then(serde_json::Value::as_str)
            == Some(executor_job_name)
}

fn replace_execution_state(
    intent_json: &mut serde_json::Value,
    execution_state: serde_json::Value,
) {
    if let Some(object) = intent_json.as_object_mut() {
        object.insert("execution_state".to_string(), execution_state);
    }
}

fn time_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

/// Kubernetes object names and label values must be DNS-safe; run ids use
/// underscores. The mapping must stay reversible for the reaper.
fn job_label_value(run_id: &str) -> String {
    run_id.replace('_', "-")
}

fn run_label_to_run_id(label: &str) -> String {
    // run ids are `run_<digits>`; the label form is `run-<digits>`.
    match label.strip_prefix("run-") {
        Some(rest) => format!("run_{rest}"),
        None => label.to_string(),
    }
}

fn job_name(run_id: &str, approval: Option<&StoredApproval>) -> String {
    let base = job_label_value(run_id);
    match approval {
        None => format!("pharness-{base}-i"),
        Some(approval) => {
            let digest = Sha256::digest(approval.id.as_bytes());
            format!(
                "pharness-{base}-r{:02x}{:02x}{:02x}{:02x}",
                digest[0], digest[1], digest[2], digest[3]
            )
        }
    }
}

fn workspace_claim_name(run_id: &str) -> String {
    format!("pharness-{}-ws", job_label_value(run_id))
}

fn environment_preparation_job_name(run_id: &str) -> String {
    format!("pharness-{}-prepare", job_label_value(run_id))
}

fn tekton_executor_job_name(execution_id: &str) -> String {
    let digest = Sha256::digest(execution_id.as_bytes());
    let suffix = digest
        .iter()
        .take(9)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("pharness-tekton-{suffix}")
}

fn argo_executor_job_name(execution_id: &str) -> String {
    let digest = Sha256::digest(execution_id.as_bytes());
    let suffix = digest
        .iter()
        .take(9)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("pharness-argo-{suffix}")
}

fn git_writer_job_name(execution_id: &str) -> String {
    let digest = Sha256::digest(execution_id.as_bytes());
    let suffix = digest
        .iter()
        .take(9)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("pharness-git-writer-{suffix}")
}

fn gitops_writer_job_name(execution_id: &str) -> String {
    let digest = Sha256::digest(execution_id.as_bytes());
    let suffix = digest
        .iter()
        .take(9)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("pharness-gitops-writer-{suffix}")
}

fn gitops_observer_job_name(execution_id: &str) -> String {
    let digest = Sha256::digest(execution_id.as_bytes());
    let suffix = digest
        .iter()
        .take(9)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("pharness-gitops-observer-{suffix}")
}

fn git_observer_job_name(execution_id: &str) -> String {
    let digest = Sha256::digest(execution_id.as_bytes());
    let suffix = digest
        .iter()
        .take(9)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("pharness-git-observer-{suffix}")
}

fn gitops_revision_resolver_job_name(execution_id: &str) -> String {
    let digest = Sha256::digest(execution_id.as_bytes());
    let suffix = digest
        .iter()
        .take(9)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("pharness-gitops-revision-{suffix}")
}

async fn create_job_from_manifest(
    kubectl_bin: &str,
    namespace: &str,
    manifest: &serde_json::Value,
) -> anyhow::Result<()> {
    let payload = serde_json::to_vec(manifest)?;
    let mut child = tokio::process::Command::new(kubectl_bin)
        .args(["create", "-n", namespace, "-f", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(&payload).await?;
    }
    let output = child.wait_with_output().await?;
    if !output.status.success() {
        anyhow::bail!(
            "kubectl create Job failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        active_run_job_count, argo_executor_job_name, enforce_run_job_capacity,
        executor_job_terminal_state, git_observer_job_name, git_writer_job_name,
        gitops_observer_job_name, gitops_writer_job_name, job_label_value, job_name,
        run_label_to_run_id, workspace_claim_name, ArgoSyncExecutionRequest,
        ExecutorJobTerminalState, GitDeliveryExecutionRequest, GitDeliveryObservationRequest,
        GitOpsDeliveryExecutionRequest, GitOpsDeliveryObservationRequest, KubernetesJobDispatcher,
    };
    use pharness_config::WorkerKubernetesConfig;
    use pharness_core::{
        EnvironmentProfile, EnvironmentProfileLimits, PreparationStrategy, RunId, SessionId,
    };
    use pharness_store::{SqliteStore, StoredRun};
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn job_label_round_trips_run_id() {
        let run_id = "run_1781521948426738000";
        let label = job_label_value(run_id);
        assert_eq!(label, "run-1781521948426738000");
        assert_eq!(run_label_to_run_id(&label), run_id);
    }

    #[test]
    fn job_names_are_dns_safe_and_attempt_scoped() {
        let initial = job_name("run_123", None);
        assert_eq!(initial, "pharness-run-123-i");
        assert!(initial.len() <= 63);
        assert!(!initial.contains('_'));
    }

    #[test]
    fn recognizes_terminal_executor_job_states() {
        assert_eq!(
            executor_job_terminal_state(&json!({ "status": { "failed": 1 } })),
            ExecutorJobTerminalState::Failed
        );
        assert_eq!(
            executor_job_terminal_state(&json!({ "status": { "succeeded": 1 } })),
            ExecutorJobTerminalState::Succeeded
        );
        assert_eq!(
            executor_job_terminal_state(&json!({ "status": { "active": 1 } })),
            ExecutorJobTerminalState::Active
        );
    }

    #[test]
    fn counts_only_active_worker_jobs() {
        assert_eq!(
            active_run_job_count(&json!({
                "items": [
                    { "status": { "active": 1 } },
                    { "status": { "active": 2 } },
                    { "status": { "succeeded": 1 } },
                    { "status": {} },
                ],
            })),
            3
        );
    }

    #[test]
    fn rejects_worker_job_at_concurrency_limit() {
        let jobs = json!({ "items": [{ "status": { "active": 1 } }] });

        let error = enforce_run_job_capacity(&jobs, 1).unwrap_err();
        assert!(error.to_string().contains("concurrency limit reached"));
        assert!(enforce_run_job_capacity(&jobs, 2).is_ok());
    }

    #[tokio::test]
    async fn worker_manifest_mounts_durable_workspace_and_pins_node() {
        let dispatcher = test_dispatcher(Some("roomier-node".to_string())).await;
        let run = test_run();
        let manifest = dispatcher.job_manifest(&run, None);

        assert_eq!(
            manifest.pointer("/spec/template/spec/volumes/0/persistentVolumeClaim/claimName"),
            Some(&json!(workspace_claim_name(run.id.as_str())))
        );
        let claim = dispatcher.workspace_claim_manifest(&run);
        assert_eq!(
            claim.pointer("/spec/accessModes/0"),
            Some(&json!("ReadWriteOnce"))
        );
        assert_eq!(
            claim.pointer("/spec/resources/requests/storage"),
            Some(&json!("4Gi"))
        );
        assert_eq!(
            claim.pointer("/spec/storageClassName"),
            Some(&json!("local-path"))
        );
        assert_eq!(
            manifest
                .pointer("/spec/template/spec/containers/0/resources/requests/ephemeral-storage"),
            Some(&json!("2Gi"))
        );
        assert_eq!(
            manifest.pointer("/spec/template/spec/containers/0/resources/limits/ephemeral-storage"),
            Some(&json!("4Gi"))
        );
        assert_eq!(
            manifest.pointer("/spec/template/spec/affinity/nodeAffinity/requiredDuringSchedulingIgnoredDuringExecution/nodeSelectorTerms/0/matchExpressions/0/key"),
            Some(&json!("kubernetes.io/hostname"))
        );
        assert_eq!(
            manifest.pointer("/spec/template/spec/affinity/nodeAffinity/requiredDuringSchedulingIgnoredDuringExecution/nodeSelectorTerms/0/matchExpressions/0/values/0"),
            Some(&json!("roomier-node"))
        );
        let env = manifest
            .pointer("/spec/template/spec/containers/0/env")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(env.iter().any(|entry| {
            entry["name"] == "HTTPS_PROXY"
                && entry["value"]
                    .as_str()
                    .is_some_and(|value| value.contains("pharness-coding-egress-proxy"))
        }));
    }

    #[tokio::test]
    async fn preparation_manifest_uses_only_the_preparation_proxy() {
        let dispatcher = test_dispatcher(None).await;
        let profile = EnvironmentProfile {
            id: "python-3.11".to_string(),
            active: true,
            image: format!("example.test/python@sha256:{}", "a".repeat(64)),
            revision: "b".repeat(40),
            platform: "linux/amd64".to_string(),
            required_executables: vec!["pharness-worker".to_string(), "python".to_string()],
            preparation_strategy: PreparationStrategy::PythonHashedRequirements,
            service_account: "pharness-python-runner".to_string(),
            repository_allowlist: vec!["https://github.com/example/test.git".to_string()],
            limits: EnvironmentProfileLimits {
                cpu: "1".to_string(),
                memory: "1Gi".to_string(),
                ephemeral_storage: "2Gi".to_string(),
            },
        };
        let manifest = dispatcher.environment_preparation_job_manifest(&test_run(), &profile);
        let env = manifest
            .pointer("/spec/template/spec/containers/0/env")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(env.iter().any(|entry| {
            entry["name"] == "HTTPS_PROXY"
                && entry["value"]
                    .as_str()
                    .is_some_and(|value| value.contains("pharness-preparation-egress-proxy"))
        }));
        assert!(env.iter().all(|entry| {
            !entry["value"]
                .as_str()
                .is_some_and(|value| value.contains("pharness-coding-egress-proxy"))
        }));
        assert!(env.iter().all(|entry| entry["name"] != "FIREWORKS_API_KEY"));
    }

    #[tokio::test]
    async fn worker_manifest_omits_affinity_when_not_configured() {
        let dispatcher = test_dispatcher(None).await;
        let manifest = dispatcher.job_manifest(&test_run(), None);

        assert!(manifest.pointer("/spec/template/spec/affinity").is_none());
    }

    #[tokio::test]
    async fn git_writer_manifest_isolated_from_model_credentials() {
        let dispatcher = test_dispatcher(None).await;
        let request = GitDeliveryExecutionRequest {
            change_set_id: "cset_123".to_string(),
            execution_id: "gexec_123".to_string(),
        };
        let job_name = git_writer_job_name(&request.execution_id);
        let manifest = dispatcher.git_writer_job_manifest(&request, &job_name);
        let env = manifest
            .pointer("/spec/template/spec/containers/0/env")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(env
            .iter()
            .any(|entry| entry["name"] == "PHARNESS_GIT_WRITER_TOKEN"));
        assert!(env.iter().all(|entry| entry["name"] != "FIREWORKS_API_KEY"));
        assert_eq!(
            manifest.pointer("/spec/template/spec/serviceAccountName"),
            Some(&json!("pharness-git-writer"))
        );
        assert_eq!(manifest.pointer("/spec/backoffLimit"), Some(&json!(0)));
    }

    #[tokio::test]
    async fn git_observer_manifest_has_only_its_read_only_credential() {
        let mut dispatcher = test_dispatcher(None).await;
        dispatcher.config.git_observer_enabled = true;
        dispatcher.config.git_observer_token_secret_name =
            Some("pharness-git-observer-token".to_string());
        dispatcher.config.git_observer_allowed_repos =
            vec!["https://github.com/example/test.git".to_string()];
        let request = GitDeliveryObservationRequest {
            change_set_id: "cset_123".to_string(),
            execution_id: "gobs_123".to_string(),
        };
        let job_name = git_observer_job_name(&request.execution_id);
        let manifest = dispatcher.git_observer_job_manifest(&request, &job_name);
        let env = manifest
            .pointer("/spec/template/spec/containers/0/env")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(env
            .iter()
            .any(|entry| entry["name"] == "PHARNESS_GIT_OBSERVER_TOKEN"));
        assert!(env
            .iter()
            .all(|entry| entry["name"] != "PHARNESS_GIT_WRITER_TOKEN"));
        assert!(env.iter().all(|entry| entry["name"] != "FIREWORKS_API_KEY"));
        assert_eq!(
            manifest.pointer("/spec/template/spec/serviceAccountName"),
            Some(&json!("pharness-git-observer"))
        );
        assert!(manifest.pointer("/spec/template/spec/volumes").is_none());
    }

    #[tokio::test]
    async fn gitops_writer_manifest_isolated_from_source_and_model_credentials() {
        let mut dispatcher = test_dispatcher(None).await;
        dispatcher.config.gitops_writer_enabled = true;
        dispatcher.config.gitops_writer_token_secret_name =
            Some("pharness-gitops-writer-token".to_string());
        dispatcher.config.gitops_writer_allowed_repos =
            vec!["https://github.com/example/finance-gitops.git".to_string()];
        let request = GitOpsDeliveryExecutionRequest {
            gitops_change_set_id: "gset_123".to_string(),
            execution_id: "gopsexec_123".to_string(),
        };
        let job_name = gitops_writer_job_name(&request.execution_id);
        let manifest = dispatcher.gitops_writer_job_manifest(&request, &job_name);
        let env = manifest
            .pointer("/spec/template/spec/containers/0/env")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(env
            .iter()
            .any(|entry| entry["name"] == "PHARNESS_GITOPS_CHANGE_SET_ID"));
        assert!(env
            .iter()
            .any(|entry| entry["name"] == "PHARNESS_GIT_WRITER_TOKEN"));
        assert!(env.iter().all(|entry| entry["name"] != "FIREWORKS_API_KEY"));
        assert!(env
            .iter()
            .all(|entry| entry["name"] != "PHARNESS_GIT_OBSERVER_TOKEN"));
        assert_eq!(
            manifest.pointer("/spec/template/spec/serviceAccountName"),
            Some(&json!("pharness-gitops-writer"))
        );
        assert_eq!(manifest.pointer("/spec/backoffLimit"), Some(&json!(0)));
    }

    #[tokio::test]
    async fn gitops_observer_manifest_has_only_read_only_observer_credentials() {
        let mut dispatcher = test_dispatcher(None).await;
        dispatcher.config.gitops_observer_enabled = true;
        dispatcher.config.gitops_observer_token_secret_name =
            Some("pharness-gitops-observer-token".to_string());
        dispatcher.config.gitops_observer_allowed_repos =
            vec!["https://github.com/example/finance-gitops.git".to_string()];
        let request = GitOpsDeliveryObservationRequest {
            gitops_change_set_id: "gset_123".to_string(),
            execution_id: "gopsobs_123".to_string(),
        };
        let job_name = gitops_observer_job_name(&request.execution_id);
        let manifest = dispatcher.gitops_observer_job_manifest(&request, &job_name);
        let env = manifest
            .pointer("/spec/template/spec/containers/0/env")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(env
            .iter()
            .any(|entry| entry["name"] == "PHARNESS_GIT_OBSERVER_TOKEN"));
        assert!(env
            .iter()
            .all(|entry| entry["name"] != "PHARNESS_GIT_WRITER_TOKEN"));
        assert!(env.iter().all(|entry| entry["name"] != "FIREWORKS_API_KEY"));
        assert_eq!(
            manifest.pointer("/spec/template/spec/serviceAccountName"),
            Some(&json!("pharness-gitops-observer"))
        );
        assert!(manifest.pointer("/spec/template/spec/volumes").is_none());
    }

    #[tokio::test]
    async fn argo_executor_manifest_isolated_and_sync_scoped() {
        let mut dispatcher = test_dispatcher(None).await;
        dispatcher.config.argo_executor_enabled = true;
        dispatcher.config.argo_executor_allowed_applications = vec!["finance-app-dev".to_string()];
        let request = ArgoSyncExecutionRequest {
            deployment_intent_id: "dint_123".to_string(),
            execution_id: "aexec_123".to_string(),
        };
        let job_name = argo_executor_job_name(&request.execution_id);
        let manifest = dispatcher.argo_executor_job_manifest(&request, &job_name);
        let env = manifest
            .pointer("/spec/template/spec/containers/0/env")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(env
            .iter()
            .any(|entry| entry["name"] == "PHARNESS_DEPLOYMENT_INTENT_ID"));
        assert!(env.iter().all(|entry| entry["name"] != "FIREWORKS_API_KEY"));
        assert!(env
            .iter()
            .all(|entry| entry["name"] != "PHARNESS_GIT_WRITER_TOKEN"));
        assert_eq!(
            manifest.pointer("/spec/template/spec/serviceAccountName"),
            Some(&json!("pharness-argo-runner"))
        );
        assert_eq!(manifest.pointer("/spec/backoffLimit"), Some(&json!(0)));
        assert_eq!(
            manifest.pointer("/spec/activeDeadlineSeconds"),
            Some(&json!(600))
        );
    }

    async fn test_dispatcher(node_hostname: Option<String>) -> KubernetesJobDispatcher {
        KubernetesJobDispatcher {
            store: Arc::new(SqliteStore::connect_in_memory().await.unwrap()),
            kubectl_bin: "kubectl".to_string(),
            config: WorkerKubernetesConfig {
                namespace: "pharness".to_string(),
                image: "example.test/pharness:latest".to_string(),
                service_account: "pharness-worker".to_string(),
                tekton_executor_service_account: "pharness-tekton-runner".to_string(),
                tekton_allowed_namespaces: Vec::new(),
                tekton_executor_poll_seconds: 5,
                argo_executor_enabled: false,
                argo_executor_service_account: "pharness-argo-runner".to_string(),
                argo_executor_namespace: "argocd".to_string(),
                argo_executor_allowed_applications: Vec::new(),
                argo_executor_poll_seconds: 5,
                argo_executor_active_deadline_seconds: 600,
                argo_executor_ttl_seconds_after_finished: 3600,
                git_writer_enabled: true,
                git_writer_service_account: "pharness-git-writer".to_string(),
                git_writer_token_secret_name: Some("pharness-git-writer-token".to_string()),
                git_writer_allowed_repos: vec!["https://github.com/example/test.git".to_string()],
                git_writer_github_api_url: "https://api.github.com".to_string(),
                git_writer_author_name: "Pharness".to_string(),
                git_writer_author_email: "pharness@example.test".to_string(),
                git_writer_active_deadline_seconds: 900,
                git_writer_ttl_seconds_after_finished: 3600,
                gitops_writer_enabled: false,
                gitops_writer_service_account: "pharness-gitops-writer".to_string(),
                gitops_writer_token_secret_name: None,
                gitops_writer_allowed_repos: Vec::new(),
                gitops_writer_github_api_url: "https://api.github.com".to_string(),
                gitops_writer_author_name: "Pharness".to_string(),
                gitops_writer_author_email: "pharness@example.test".to_string(),
                gitops_writer_active_deadline_seconds: 900,
                gitops_writer_ttl_seconds_after_finished: 3600,
                git_observer_enabled: false,
                git_observer_service_account: "pharness-git-observer".to_string(),
                git_observer_token_secret_name: None,
                git_observer_allowed_repos: Vec::new(),
                git_observer_github_api_url: "https://api.github.com".to_string(),
                git_observer_active_deadline_seconds: 300,
                git_observer_ttl_seconds_after_finished: 3600,
                gitops_observer_enabled: true,
                gitops_observer_service_account: "pharness-gitops-observer".to_string(),
                gitops_observer_token_secret_name: Some(
                    "pharness-gitops-observer-token".to_string(),
                ),
                gitops_observer_allowed_repos: vec![
                    "https://github.com/example/finance-gitops.git".to_string(),
                ],
                gitops_observer_github_api_url: "https://api.github.com".to_string(),
                gitops_observer_active_deadline_seconds: 300,
                gitops_observer_ttl_seconds_after_finished: 3600,
                api_url: "http://pharness-api:4777".to_string(),
                workspace_dir: "/workspace".to_string(),
                workspace_size_limit: "4Gi".to_string(),
                workspace_storage_class: Some("local-path".to_string()),
                workspace_ephemeral_storage_request: "2Gi".to_string(),
                workspace_ephemeral_storage_limit: "4Gi".to_string(),
                workspace_node_hostname: node_hostname,
                max_concurrent_run_jobs: 1,
                fireworks_secret_name: "pharness-fireworks".to_string(),
                worker_token_secret_name: "pharness-worker-token".to_string(),
                active_deadline_seconds: 3600,
                ttl_seconds_after_finished: 3600,
            },
            model: "accounts/fireworks/models/test".to_string(),
            base_url: "https://example.test/v1".to_string(),
            worker_env: Vec::new(),
        }
    }

    fn test_run() -> StoredRun {
        StoredRun {
            id: RunId::new("run_123"),
            session_id: SessionId::new("ses_123"),
            cwd: "/workspace".to_string(),
            status: "queued".to_string(),
            user_task: "test".to_string(),
            max_turns: 1,
            run_budget: Default::default(),
            budget_consumption: Default::default(),
            stop_reason: None,
            started_at: "0".to_string(),
            finished_at: None,
            cancel_requested_at: None,
            error: None,
            result_json: None,
            execution_target_json: json!({}),
            origin: "legacy".to_string(),
            created_by: None,
        }
    }
}
