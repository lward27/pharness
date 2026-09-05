//! Run dispatch across execution targets.
//!
//! `RunDispatcher` decides where a run attempt executes: in-process through
//! the existing local worker, or in an isolated Kubernetes Job per attempt.
//! Job orchestration shells out to kubectl with the pod service account,
//! matching how the typed read-only cluster capabilities already execute.

use crate::worker::{
    fail_run_from_dispatch, fail_run_from_job_creation, sync_repo_stage_run, LocalWorker,
};
use pharness_config::WorkerKubernetesConfig;
use pharness_core::{EnvironmentProfile, PreparationStrategy};
use pharness_store::{
    CreateAuditEvent, PipelineIntentListFilter, SqliteStore, StoredApproval, StoredPipelineIntent,
    StoredRun, UpdatePipelineIntentExecution,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod recovery;

const REAPER_INTERVAL: Duration = Duration::from_secs(30);
const CHAINED_RUN_HANDOFF_TIMEOUT: Duration = Duration::from_secs(60);
const CHAINED_RUN_HANDOFF_POLL_INTERVAL: Duration = Duration::from_millis(250);
static RUN_ADMISSION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
pub(crate) const RUN_ID_LABEL: &str = "pharness.lucas.engineering/run-id";
const WORKSPACE_ID_LABEL: &str = "pharness.lucas.engineering/workspace-id";
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
const REPOSITORY_DISCOVERY_JOB_NAME_VALUE: &str = "pharness-repository-discovery";
const ONBOARDING_PATCH_JOB_NAME_VALUE: &str = "pharness-onboarding-patch";
const ONBOARDING_VALIDATION_JOB_NAME_VALUE: &str = "pharness-onboarding-validation";
const REPOSITORY_READINESS_JOB_NAME_VALUE: &str = "pharness-repository-readiness";
const INFERENCE_EVALUATION_JOB_NAME_VALUE: &str = "pharness-inference-evaluation";
// K3s's selector-backed NetworkPolicy rules converge asynchronously after a
// Pod receives its IP. Short-lived Jobs otherwise race the policy controller
// and fail their first API or proxy connection before the rule includes them.
// Keep this delay outside the worker/model process so no model turn or active
// execution budget is consumed while the dataplane catches up.
const NETWORK_POLICY_STABILIZATION_SECONDS: u64 = 15;
const PIPELINE_INTENT_LABEL: &str = "pharness.lucas.engineering/pipeline-intent";
const DEPLOYMENT_INTENT_LABEL: &str = "pharness.lucas.engineering/deployment-intent";
const CHANGE_SET_LABEL: &str = "pharness.lucas.engineering/change-set";
const SOURCE_DELIVERY_INTENT_LABEL: &str = "pharness.lucas.engineering/source-delivery-intent";
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
pub struct SourceDeliveryExecutionRequest {
    pub source_delivery_intent_id: String,
    pub execution_id: String,
}

#[derive(Debug, Clone)]
pub struct SourceDeliveryObservationRequest {
    pub source_delivery_intent_id: String,
    pub execution_id: String,
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

#[derive(Debug, Clone)]
pub struct EnvironmentPreparationJobObservation {
    pub job_name: String,
    pub status: &'static str,
}

#[derive(Debug, Clone)]
pub struct RepositoryDiscoveryRequest {
    pub discovery_id: String,
}

#[derive(Debug, Clone)]
pub struct RepositoryDiscoveryReceipt {
    pub job_name: String,
}

#[derive(Debug, Clone)]
pub struct OnboardingPatchRequest {
    pub onboarding_id: String,
    pub execution_id: String,
}

#[derive(Debug, Clone)]
pub struct OnboardingPatchReceipt {
    pub job_name: String,
}

#[derive(Debug, Clone)]
pub struct OnboardingContractValidationRequest {
    pub onboarding_id: String,
    pub execution_id: String,
}

#[derive(Debug, Clone)]
pub struct OnboardingContractValidationReceipt {
    pub job_name: String,
}

#[derive(Debug, Clone)]
pub struct RepositoryReadinessExecutionRequest {
    pub preparation_id: String,
    pub profile: EnvironmentProfile,
}

#[derive(Debug, Clone)]
pub struct RepositoryReadinessExecutionReceipt {
    pub job_name: String,
}

#[derive(Debug, Clone)]
pub struct InferenceEvaluationExecutionRequest {
    pub evaluation_id: String,
    pub gateway_url: String,
}

#[derive(Debug, Clone)]
pub struct InferenceEvaluationExecutionReceipt {
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

    pub async fn cleanup_retention_resources(
        &self,
        resources: &[serde_json::Value],
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        if resources.is_empty() {
            return Ok(Vec::new());
        }
        match self {
            Self::Kubernetes(dispatcher) => dispatcher.cleanup_retention_resources(resources).await,
            Self::Disabled | Self::Local(_) => {
                anyhow::bail!("Kubernetes retention resources require kubernetes_job worker mode")
            }
        }
    }

    pub async fn delete_archive_claims(
        &self,
        archive_id: &str,
        archived_generation_id: &str,
        database_claim: &str,
        archive_claim: &str,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        match self {
            Self::Kubernetes(dispatcher) => {
                dispatcher
                    .delete_archive_claims(
                        archive_id,
                        archived_generation_id,
                        database_claim,
                        archive_claim,
                    )
                    .await
            }
            Self::Disabled | Self::Local(_) => {
                anyhow::bail!("archive deletion requires kubernetes_job worker mode")
            }
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
                "source_reader": {
                    "enabled": dispatcher.config.source_reader_enabled,
                    "available": dispatcher.source_reader_available(),
                    "service_account": dispatcher.config.source_reader_service_account,
                    "allowed_repos": dispatcher.config.source_reader_allowed_repos,
                    "private_credential_configured": dispatcher.config.source_reader_token_secret_name.is_some(),
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
            Self::Kubernetes(dispatcher) => dispatcher.clone().launch(run, None, None),
        }
    }

    /// Dispatch a controller-authorized follow-up Run without racing the
    /// predecessor Kubernetes Job's terminal status update. The predecessor
    /// has already reported a durable successful outcome, but Kubernetes can
    /// still count its Job as active for a short interval while the worker
    /// process exits. Keep the next Run queued during that interval rather
    /// than weakening the global worker concurrency limit.
    pub fn spawn_chained_run(&self, run: StoredRun, cwd: String, predecessor_run_id: &str) {
        match self {
            Self::Disabled => {}
            Self::Local(worker) => worker.spawn_run(run, cwd),
            Self::Kubernetes(dispatcher) => {
                dispatcher
                    .clone()
                    .launch(run, None, Some(predecessor_run_id.to_string()))
            }
        }
    }

    pub fn resume_run(&self, run: StoredRun, approval: StoredApproval) {
        match self {
            Self::Disabled => {}
            Self::Local(worker) => worker.resume_run(run, approval),
            Self::Kubernetes(dispatcher) => dispatcher.clone().launch(run, Some(approval), None),
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

    /// Observe only the exact immutable preparation Job for an existing Run.
    /// A missing Job is distinct from an unavailable or conflicting observation.
    pub async fn observe_hosted_preparation(
        &self,
        run: &StoredRun,
        profile: &EnvironmentProfile,
    ) -> anyhow::Result<Option<EnvironmentPreparationJobObservation>> {
        let Self::Kubernetes(dispatcher) = self else {
            anyhow::bail!("hosted preparation observation requires kubernetes_job worker mode");
        };
        if !recovery::is_hosted(run) {
            anyhow::bail!("preparation recovery requires hosted authority");
        }
        let manifest =
            recovery::bind_manifest(dispatcher.environment_preparation_job_manifest(run, profile));
        Ok(recovery::find_exact_job(
            &dispatcher.kubectl_bin,
            &dispatcher.config.namespace,
            &manifest,
        )
        .await?
        .map(|job| {
            let terminal = job
                .pointer("/status/conditions")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .find(|c| {
                    c["status"] == "True"
                        && matches!(c["type"].as_str(), Some("Complete" | "Failed"))
                });
            EnvironmentPreparationJobObservation {
                job_name: manifest["metadata"]["name"]
                    .as_str()
                    .unwrap_or_default()
                    .into(),
                status: match terminal.and_then(|c| c["type"].as_str()) {
                    Some("Failed") => "failed",
                    Some("Complete") => "completed",
                    _ => "active",
                },
            }
        }))
    }

    pub fn source_reader_available(&self) -> bool {
        matches!(self, Self::Kubernetes(dispatcher) if dispatcher.source_reader_available())
    }

    pub fn source_reader_allows_repository(&self, repository: &str) -> bool {
        matches!(self, Self::Kubernetes(dispatcher)
            if dispatcher.source_reader_available()
                && dispatcher.config.source_reader_allowed_repos.iter()
                    .any(|allowed| allowed == repository))
    }

    pub fn source_reader_allowed_repos(&self) -> Vec<String> {
        match self {
            Self::Kubernetes(dispatcher) if dispatcher.source_reader_available() => {
                dispatcher.config.source_reader_allowed_repos.clone()
            }
            _ => Vec::new(),
        }
    }

    pub async fn dispatch_repository_discovery(
        &self,
        request: RepositoryDiscoveryRequest,
    ) -> anyhow::Result<RepositoryDiscoveryReceipt> {
        match self {
            Self::Kubernetes(dispatcher) => {
                dispatcher.create_repository_discovery_job(&request).await
            }
            Self::Disabled => {
                anyhow::bail!("repository discovery requires kubernetes_job worker mode")
            }
            Self::Local(_) => {
                anyhow::bail!("isolated repository discovery is unavailable in local worker mode")
            }
        }
    }

    pub async fn dispatch_onboarding_patch(
        &self,
        request: OnboardingPatchRequest,
    ) -> anyhow::Result<OnboardingPatchReceipt> {
        match self {
            Self::Kubernetes(dispatcher) => dispatcher.create_onboarding_patch_job(&request).await,
            Self::Disabled => {
                anyhow::bail!("onboarding patch requires kubernetes_job worker mode")
            }
            Self::Local(_) => {
                anyhow::bail!("onboarding patch is unavailable in local worker mode")
            }
        }
    }

    pub async fn dispatch_onboarding_contract_validation(
        &self,
        request: OnboardingContractValidationRequest,
    ) -> anyhow::Result<OnboardingContractValidationReceipt> {
        match self {
            Self::Kubernetes(dispatcher) => {
                dispatcher
                    .create_onboarding_contract_validation_job(&request)
                    .await
            }
            Self::Disabled => {
                anyhow::bail!("onboarding contract validation requires kubernetes_job worker mode")
            }
            Self::Local(_) => {
                anyhow::bail!("onboarding contract validation is unavailable in local worker mode")
            }
        }
    }

    pub async fn dispatch_repository_readiness(
        &self,
        request: RepositoryReadinessExecutionRequest,
    ) -> anyhow::Result<RepositoryReadinessExecutionReceipt> {
        match self {
            Self::Kubernetes(dispatcher) => {
                dispatcher.create_repository_readiness_job(&request).await
            }
            Self::Disabled => {
                anyhow::bail!("repository readiness requires kubernetes_job worker mode")
            }
            Self::Local(_) => {
                anyhow::bail!("repository readiness is unavailable in local worker mode")
            }
        }
    }

    pub async fn dispatch_inference_evaluation(
        &self,
        request: InferenceEvaluationExecutionRequest,
    ) -> anyhow::Result<InferenceEvaluationExecutionReceipt> {
        match self {
            Self::Kubernetes(dispatcher) => {
                dispatcher.create_inference_evaluation_job(&request).await
            }
            Self::Disabled => {
                anyhow::bail!("inference qualification requires kubernetes_job worker mode")
            }
            Self::Local(_) => {
                anyhow::bail!("inference qualification is unavailable in local worker mode")
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

    pub async fn dispatch_source_delivery(
        &self,
        request: SourceDeliveryExecutionRequest,
    ) -> anyhow::Result<GitDeliveryExecutionReceipt> {
        match self {
            Self::Kubernetes(dispatcher) => {
                dispatcher.create_source_delivery_writer_job(&request).await
            }
            Self::Disabled => anyhow::bail!("source delivery requires kubernetes_job worker mode"),
            Self::Local(_) => anyhow::bail!("source delivery is unavailable in local worker mode"),
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

    pub async fn dispatch_source_delivery_observation(
        &self,
        request: SourceDeliveryObservationRequest,
    ) -> anyhow::Result<GitDeliveryObservationReceipt> {
        match self {
            Self::Kubernetes(dispatcher) => {
                dispatcher
                    .create_source_delivery_observer_job(&request)
                    .await
            }
            Self::Disabled => {
                anyhow::bail!("source delivery observation requires kubernetes_job worker mode")
            }
            Self::Local(_) => {
                anyhow::bail!("source delivery observation is unavailable in local worker mode")
            }
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

fn network_policy_stabilization_container(image: &str) -> serde_json::Value {
    serde_json::json!({
        "name": "network-policy-stabilization",
        "image": image,
        "imagePullPolicy": "IfNotPresent",
        "command": ["/bin/sh", "-ec"],
        "args": [format!("sleep {NETWORK_POLICY_STABILIZATION_SECONDS}")],
        "securityContext": {
            "allowPrivilegeEscalation": false,
            "readOnlyRootFilesystem": true,
            "capabilities": { "drop": ["ALL"] },
        },
        "resources": {
            "requests": { "cpu": "1m", "memory": "4Mi" },
            "limits": { "cpu": "10m", "memory": "16Mi" },
        },
    })
}

fn required_node_affinity(hostname: &str) -> serde_json::Value {
    serde_json::json!({
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
    })
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

    fn launch(
        self: Arc<Self>,
        run: StoredRun,
        approval: Option<StoredApproval>,
        predecessor_run_id: Option<String>,
    ) {
        tokio::spawn(async move {
            let run_id = run.id.clone();
            if let Err(error) = self
                .create_job(&run, approval.as_ref(), predecessor_run_id.as_deref())
                .await
            {
                tracing::error!(run_id = %run_id, %error, "failed to launch worker job");
                if recovery::is_hosted(&run) {
                    // A failed response does not prove that Kubernetes rejected
                    // the request. Keep the durable Run for exact-Job recovery.
                    tracing::warn!(run_id = %run_id, "hosted dispatch awaits reconciliation; Run was not sealed failed");
                    return;
                }
                let _ = fail_run_from_job_creation(
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

    fn source_reader_available(&self) -> bool {
        self.config.source_reader_enabled && !self.config.source_reader_allowed_repos.is_empty()
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
        for _ in 0..(60 + NETWORK_POLICY_STABILIZATION_SECONDS) {
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
        let repository_checks = profile
            .repository_allowlist
            .iter()
            .map(|repository| format!("git ls-remote --exit-code '{repository}' HEAD >/dev/null"))
            .collect::<Vec<_>>()
            .join(" && ");
        let strategy_check = match profile.preparation_strategy {
            PreparationStrategy::PythonHashedRequirements => "python -m venv /tmp/profile-venv; /tmp/profile-venv/bin/pip --version >/dev/null; /tmp/profile-venv/bin/python -c \"import urllib.request; urllib.request.urlopen('https://pypi.org/simple/', timeout=15).read(1)\"".to_string(),
            PreparationStrategy::NodeNpmCi => "node --version >/dev/null; npm --version >/dev/null; npm ping --registry=https://registry.npmjs.org/ --ignore-scripts >/dev/null".to_string(),
        };
        let script = format!(
            "set -eu; test \"$(uname -s)\" = Linux; test \"$(uname -m)\" = x86_64; test \"$PHARNESS_BUILD_REVISION\" = \"$EXPECTED_REVISION\"; {executable_checks}; {repository_checks}; {strategy_check}"
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
                "activeDeadlineSeconds": 120 + NETWORK_POLICY_STABILIZATION_SECONDS,
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
                        "initContainers": [network_policy_stabilization_container(&profile.image)],
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
        for _ in 0..(120 + NETWORK_POLICY_STABILIZATION_SECONDS) {
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
            "source_workspace" | "source_reader" => Some("preparation"),
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
            "source_reader" => {
                if !self.source_reader_available() {
                    anyhow::bail!("source reader is not configured");
                }
                let repo = requested_repository.ok_or_else(|| anyhow::anyhow!("source reader repository is unavailable"))?;
                if !self.config.source_reader_allowed_repos.iter().any(|allowed| allowed == repo) {
                    anyhow::bail!("source reader repository is not allowlisted");
                }
                if let Some(secret) = self.config.source_reader_token_secret_name.as_deref() {
                    env.push(serde_json::json!({"name":"GITHUB_TOKEN","valueFrom":{"secretKeyRef":{"name":secret,"key":"token"}}}));
                }
                env.push(serde_json::json!({"name":"REPOSITORY","value":repo}));
                let script = r#"if [ -n "${GITHUB_TOKEN:-}" ]; then
cat > /tmp/askpass <<'EOF'
#!/bin/sh
case "$1" in
  *Username*) printf '%s\n' x-access-token ;;
  *) printf '%s\n' "$GITHUB_TOKEN" ;;
esac
EOF
chmod 700 /tmp/askpass
GIT_ASKPASS_VALUE=/tmp/askpass
else
GIT_ASKPASS_VALUE=/bin/false
fi
GIT_TERMINAL_PROMPT=0 GIT_ASKPASS="$GIT_ASKPASS_VALUE" GIT_CONFIG_NOSYSTEM=1 git ls-remote --exit-code "$REPOSITORY" HEAD >/dev/null"#.to_string();
                (self.config.source_reader_service_account.clone(), format!("system:serviceaccount:{}:{}", self.config.namespace, self.config.source_reader_service_account), "repository_read".to_string(), Some(repo.to_string()), script)
            }
            "source_writer" | "source_observer" | "gitops_writer" | "gitops_observer" => {
                let (enabled, service_account, secret, repos, required_permission) = match capability {
                    "source_writer" => (self.git_writer_available(), &self.config.git_writer_service_account, self.config.git_writer_token_secret_name.as_deref(), &self.config.git_writer_allowed_repos, "push"),
                    "source_observer" => (self.git_observer_available(), &self.config.git_observer_service_account, self.config.git_observer_token_secret_name.as_deref(), &self.config.git_observer_allowed_repos, "pull_rules_checks_statuses"),
                    "gitops_writer" => (self.gitops_writer_available(), &self.config.gitops_writer_service_account, self.config.gitops_writer_token_secret_name.as_deref(), &self.config.gitops_writer_allowed_repos, "push"),
                    _ => (self.gitops_observer_available(), &self.config.gitops_observer_service_account, self.config.gitops_observer_token_secret_name.as_deref(), &self.config.gitops_observer_allowed_repos, "pull"),
                };
                if !enabled { anyhow::bail!("{capability} is not configured"); }
                let repo = match requested_repository {
                    Some(repository) => {
                        if !repos.iter().any(|allowed| allowed == repository) {
                            anyhow::bail!("{capability} repository is not allowlisted");
                        }
                        repository
                    }
                    None => repos
                        .first()
                        .map(String::as_str)
                        .ok_or_else(|| anyhow::anyhow!("{capability} repository allowlist is empty"))?,
                };
                let repo_path = repo.trim_end_matches('/').trim_end_matches(".git").strip_prefix("https://github.com/").ok_or_else(|| anyhow::anyhow!("{capability} repository is not a safe GitHub HTTPS URL"))?;
                env.push(serde_json::json!({"name":"GITHUB_TOKEN","valueFrom":{"secretKeyRef":{"name":secret.expect("availability requires Secret"),"key":"token"}}}));
                env.push(serde_json::json!({"name":"REPOSITORY","value":repo}));
                env.push(serde_json::json!({"name":"REPOSITORY_API","value":format!("https://api.github.com/repos/{repo_path}")}));
                env.push(serde_json::json!({"name":"REQUIRED_PERMISSION","value":required_permission}));
                let script = if capability == "source_observer" {
                    env.push(serde_json::json!({
                        "name":"PHARNESS_EXECUTION_KIND",
                        "value":"source_observer_capability_preflight"
                    }));
                    env.push(serde_json::json!({
                        "name":"GITHUB_API_URL",
                        "value":self.config.git_observer_github_api_url
                    }));
                    "exec /usr/local/bin/pharness-worker".to_string()
                } else if required_permission == "push" {
                    env.push(serde_json::json!({
                        "name":"PREFLIGHT_REF",
                        "value":format!("refs/heads/pharness/capability-preflight-{suffix}")
                    }));
                    r#"curl -fsS -H "Authorization: Bearer $GITHUB_TOKEN" -H "Accept: application/vnd.github+json" "$REPOSITORY_API" -o /tmp/repository.json
grep -Eq "\"$REQUIRED_PERMISSION\"[[:space:]]*:[[:space:]]*true" /tmp/repository.json
cat > /tmp/askpass <<'EOF'
#!/bin/sh
case "$1" in
  *Username*) printf '%s\n' x-access-token ;;
  *) printf '%s\n' "$GITHUB_TOKEN" ;;
esac
EOF
chmod 700 /tmp/askpass
git init -q /tmp/push-preflight
git -C /tmp/push-preflight config user.name Pharness
git -C /tmp/push-preflight config user.email pharness@example.invalid
printf '%s\n' capability-preflight > /tmp/push-preflight/probe
git -C /tmp/push-preflight add probe
git -C /tmp/push-preflight commit -qm capability-preflight
GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=/tmp/askpass GIT_CONFIG_NOSYSTEM=1 git -C /tmp/push-preflight push --dry-run "$REPOSITORY" "HEAD:$PREFLIGHT_REF" >/dev/null 2>&1"#.to_string()
                } else {
                    "curl -fsS -H \"Authorization: Bearer $GITHUB_TOKEN\" -H \"Accept: application/vnd.github+json\" \"$REPOSITORY_API\" -o /tmp/repository.json && grep -Eq \"\\\"$REQUIRED_PERMISSION\\\"[[:space:]]*:[[:space:]]*true\" /tmp/repository.json".to_string()
                };
                (service_account.clone(), format!("system:serviceaccount:{}:{}", self.config.namespace, service_account), format!("repository_{required_permission}"), Some(repo.to_string()), script)
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
        let mut manifest = serde_json::json!({
            "apiVersion":"batch/v1","kind":"Job",
            "metadata":{"name":job_name,"namespace":self.config.namespace,"labels":{JOB_NAME_LABEL:CAPABILITY_PREFLIGHT_JOB_NAME_VALUE}},
            "spec":{"backoffLimit":0,"activeDeadlineSeconds":75 + if proxy_phase.is_some() { NETWORK_POLICY_STABILIZATION_SECONDS } else { 0 },"ttlSecondsAfterFinished":300,
                "template":{"metadata":{"labels":pod_labels},
                    "spec":{"serviceAccountName":service_account,"restartPolicy":"Never",
                        "securityContext":{"runAsNonRoot":true,"runAsUser":65532,"runAsGroup":65532,"seccompProfile":{"type":"RuntimeDefault"}},
                        "containers":[{"name":"preflight","image":self.config.image,"imagePullPolicy":"IfNotPresent","command":["/bin/sh","-ec"],"args":[script],"env":env,
                            "securityContext":{"allowPrivilegeEscalation":false,"readOnlyRootFilesystem":true,"capabilities":{"drop":["ALL"]}},
                            "volumeMounts":[{"name":"tmp","mountPath":"/tmp"}],
                            "resources":{"requests":{"cpu":"25m","memory":"64Mi"},"limits":{"cpu":"200m","memory":"128Mi"}}}],
                        "volumes":[{"name":"tmp","emptyDir":{}}]}}}
        });
        if proxy_phase.is_some() {
            manifest["spec"]["template"]["spec"]["initContainers"] =
                serde_json::json!([network_policy_stabilization_container(&self.config.image)]);
        }
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
        predecessor_run_id: Option<&str>,
    ) -> anyhow::Result<()> {
        // SQLite is single-writer, but HTTP handlers and spawned dispatch tasks
        // are concurrent. Keep capacity observation and Job creation together.
        let _admission = RUN_ADMISSION.lock().await;
        let hosted_manifest = recovery::is_hosted(run)
            .then(|| recovery::bind_manifest(self.job_manifest(run, approval)));
        if let Some(manifest) = &hosted_manifest {
            if recovery::find_exact_job(&self.kubectl_bin, &self.config.namespace, manifest)
                .await?
                .is_some()
            {
                return Ok(());
            }
        }
        if let Some(predecessor_run_id) = predecessor_run_id {
            self.await_chained_run_capacity(predecessor_run_id).await?;
        } else {
            self.ensure_run_job_capacity().await?;
        }
        self.ensure_workspace_claim(run).await?;
        if let Some(manifest) = hosted_manifest {
            return recovery::create_or_reconcile_job(
                &self.kubectl_bin,
                &self.config.namespace,
                &manifest,
            )
            .await;
        }
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
            job = %job_name(run, approval),
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
        let _admission = RUN_ADMISSION.lock().await;
        let job_name = environment_preparation_job_name(run.id.as_str());
        let manifest = self.environment_preparation_job_manifest(run, profile);
        let hosted = recovery::is_hosted(run);
        let manifest = if hosted {
            recovery::bind_manifest(manifest)
        } else {
            manifest
        };
        if hosted
            && recovery::find_exact_job(&self.kubectl_bin, &self.config.namespace, &manifest)
                .await?
                .is_some()
        {
            return Ok(EnvironmentPreparationReceipt { job_name });
        }
        self.ensure_run_job_capacity().await?;
        self.ensure_workspace_claim(run).await?;
        if hosted {
            recovery::create_or_reconcile_job(&self.kubectl_bin, &self.config.namespace, &manifest)
                .await?;
        } else {
            create_job_from_manifest(&self.kubectl_bin, &self.config.namespace, &manifest).await?;
        }
        tracing::info!(run_id = %run.id, job = %job_name, profile = %profile.id, "created isolated environment preparation job");
        Ok(EnvironmentPreparationReceipt { job_name })
    }

    async fn create_repository_discovery_job(
        &self,
        request: &RepositoryDiscoveryRequest,
    ) -> anyhow::Result<RepositoryDiscoveryReceipt> {
        if !self.source_reader_available() {
            anyhow::bail!("source reader capability is unavailable");
        }
        let manifest = self.repository_discovery_job_manifest(request);
        create_job_from_manifest(&self.kubectl_bin, &self.config.namespace, &manifest).await?;
        let job_name = repository_discovery_job_name(&request.discovery_id);
        tracing::info!(discovery_id = %request.discovery_id, job = %job_name, "created isolated repository discovery job");
        Ok(RepositoryDiscoveryReceipt { job_name })
    }

    async fn create_onboarding_patch_job(
        &self,
        request: &OnboardingPatchRequest,
    ) -> anyhow::Result<OnboardingPatchReceipt> {
        if !self.source_reader_available() {
            anyhow::bail!("source reader is not configured");
        }
        let manifest = self.onboarding_patch_job_manifest(request);
        let job_name = onboarding_patch_job_name(&request.execution_id);
        create_job_from_manifest(&self.kubectl_bin, &self.config.namespace, &manifest).await?;
        tracing::info!(onboarding_id=%request.onboarding_id, execution_id=%request.execution_id, job=%job_name, "created onboarding patch materializer job");
        Ok(OnboardingPatchReceipt { job_name })
    }

    async fn create_onboarding_contract_validation_job(
        &self,
        request: &OnboardingContractValidationRequest,
    ) -> anyhow::Result<OnboardingContractValidationReceipt> {
        if !self.source_reader_available() {
            anyhow::bail!("source reader is not configured");
        }
        let manifest = self.onboarding_contract_validation_job_manifest(request);
        let job_name = onboarding_contract_validation_job_name(&request.execution_id);
        create_job_from_manifest(&self.kubectl_bin, &self.config.namespace, &manifest).await?;
        tracing::info!(onboarding_id=%request.onboarding_id, execution_id=%request.execution_id, job=%job_name, "created merged onboarding contract validation job");
        Ok(OnboardingContractValidationReceipt { job_name })
    }

    async fn create_repository_readiness_job(
        &self,
        request: &RepositoryReadinessExecutionRequest,
    ) -> anyhow::Result<RepositoryReadinessExecutionReceipt> {
        if !self.source_reader_available() {
            anyhow::bail!("source reader is not configured");
        }
        request
            .profile
            .validate()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let manifest = self.repository_readiness_job_manifest(request);
        let job_name = repository_readiness_job_name(&request.preparation_id);
        create_job_from_manifest(&self.kubectl_bin, &self.config.namespace, &manifest).await?;
        tracing::info!(preparation_id=%request.preparation_id, job=%job_name, profile=%request.profile.id, "created repository coding-readiness job");
        Ok(RepositoryReadinessExecutionReceipt { job_name })
    }

    async fn create_inference_evaluation_job(
        &self,
        request: &InferenceEvaluationExecutionRequest,
    ) -> anyhow::Result<InferenceEvaluationExecutionReceipt> {
        let manifest = self.inference_evaluation_job_manifest(request);
        let job_name = inference_evaluation_job_name(&request.evaluation_id);
        create_job_from_manifest(&self.kubectl_bin, &self.config.namespace, &manifest).await?;
        tracing::info!(evaluation_id=%request.evaluation_id, job=%job_name, "created isolated inference qualification job");
        Ok(InferenceEvaluationExecutionReceipt { job_name })
    }

    async fn ensure_workspace_claim(&self, run: &StoredRun) -> anyhow::Result<()> {
        let name = workspace_claim_name_for_run(run);
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
        let claim_name = workspace_claim_name_for_run(run);
        let mut manifest = serde_json::json!({
            "apiVersion": "v1",
            "kind": "PersistentVolumeClaim",
            "metadata": {
                "name": claim_name,
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
        if let Some(workspace_id) = shared_repo_workspace_id(run) {
            if let Some(labels) = manifest
                .pointer_mut("/metadata/labels")
                .and_then(serde_json::Value::as_object_mut)
            {
                labels.remove(RUN_ID_LABEL);
                labels.insert(
                    WORKSPACE_ID_LABEL.into(),
                    serde_json::Value::String(job_label_value(workspace_id)),
                );
            }
        }
        if let Some(storage_class) = self.config.workspace_storage_class.as_deref() {
            manifest["spec"]["storageClassName"] = serde_json::json!(storage_class);
        }
        manifest
    }

    /// The caller holds RUN_ADMISSION across this observation and creation.
    /// Persisted hosted operation locks survive restarts; this process lock also
    /// serializes legacy dispatches sharing the same single-replica worker pool.
    async fn list_run_jobs_for_capacity(&self) -> anyhow::Result<serde_json::Value> {
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
        Ok(serde_json::from_slice(&output.stdout)?)
    }

    async fn ensure_run_job_capacity(&self) -> anyhow::Result<()> {
        let jobs = self.list_run_jobs_for_capacity().await?;
        enforce_run_job_capacity(&jobs, self.config.max_concurrent_run_jobs)
    }

    async fn await_chained_run_capacity(&self, predecessor_run_id: &str) -> anyhow::Result<()> {
        let predecessor = self
            .store
            .get_run(&pharness_core::RunId::new(predecessor_run_id))
            .await?
            .ok_or_else(|| anyhow::anyhow!("predecessor Run {predecessor_run_id} is missing"))?;
        if predecessor.status != "completed" {
            anyhow::bail!(
                "predecessor Run {predecessor_run_id} is {}, not durably completed",
                predecessor.status
            );
        }
        let predecessor_label = job_label_value(predecessor_run_id);
        let started = std::time::Instant::now();
        loop {
            let jobs = self.list_run_jobs_for_capacity().await?;
            match chained_run_capacity(
                &jobs,
                self.config.max_concurrent_run_jobs,
                &predecessor_label,
            )? {
                ChainedRunCapacity::Available => return Ok(()),
                ChainedRunCapacity::WaitingForPredecessor { active } => {
                    if started.elapsed() >= CHAINED_RUN_HANDOFF_TIMEOUT {
                        anyhow::bail!(
                            "timed out waiting for predecessor Run {predecessor_run_id} Job to release worker capacity ({active} active)"
                        );
                    }
                    tokio::time::sleep(CHAINED_RUN_HANDOFF_POLL_INTERVAL).await;
                }
            }
        }
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

    async fn create_source_delivery_writer_job(
        &self,
        request: &SourceDeliveryExecutionRequest,
    ) -> anyhow::Result<GitDeliveryExecutionReceipt> {
        if !self.git_writer_available() {
            anyhow::bail!("Git writer executor is not configured");
        }
        let legacy = GitDeliveryExecutionRequest {
            change_set_id: request.source_delivery_intent_id.clone(),
            execution_id: request.execution_id.clone(),
        };
        let job_name = git_writer_job_name(&request.execution_id);
        let mut manifest = self.git_writer_job_manifest(&legacy, &job_name);
        bind_source_delivery_manifest(
            &mut manifest,
            &request.source_delivery_intent_id,
            "PHARNESS_SOURCE_DELIVERY_INTENT_ID",
        )?;
        create_job_from_manifest(&self.kubectl_bin, &self.config.namespace, &manifest).await?;
        tracing::info!(
            source_delivery_intent_id = %request.source_delivery_intent_id,
            execution_id = %request.execution_id,
            job = %job_name,
            "created source delivery writer job"
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

    async fn create_source_delivery_observer_job(
        &self,
        request: &SourceDeliveryObservationRequest,
    ) -> anyhow::Result<GitDeliveryObservationReceipt> {
        if !self.git_observer_available() {
            anyhow::bail!("Git observer executor is not configured");
        }
        let legacy = GitDeliveryObservationRequest {
            change_set_id: request.source_delivery_intent_id.clone(),
            execution_id: request.execution_id.clone(),
        };
        let job_name = git_observer_job_name(&request.execution_id);
        let mut manifest = self.git_observer_job_manifest(&legacy, &job_name);
        bind_source_delivery_manifest(
            &mut manifest,
            &request.source_delivery_intent_id,
            "PHARNESS_SOURCE_DELIVERY_INTENT_ID",
        )?;
        create_job_from_manifest(&self.kubectl_bin, &self.config.namespace, &manifest).await?;
        tracing::info!(
            source_delivery_intent_id = %request.source_delivery_intent_id,
            execution_id = %request.execution_id,
            job = %job_name,
            "created source delivery observer job"
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
            "spec":{"backoffLimit":0,"activeDeadlineSeconds":self.config.gitops_observer_active_deadline_seconds + NETWORK_POLICY_STABILIZATION_SECONDS,"ttlSecondsAfterFinished":self.config.gitops_observer_ttl_seconds_after_finished,
                "template":{"metadata":{"labels":{JOB_NAME_LABEL:GIT_OBSERVER_JOB_NAME_VALUE,GITOPS_CHANGE_SET_LABEL:job_label_value(&request.gitops_change_set_id)}},
                    "spec":{"serviceAccountName":self.config.gitops_observer_service_account,"restartPolicy":"Never","initContainers":[network_policy_stabilization_container(&self.config.image)],
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
                "activeDeadlineSeconds": self.config.gitops_writer_active_deadline_seconds + NETWORK_POLICY_STABILIZATION_SECONDS,
                "ttlSecondsAfterFinished": self.config.gitops_writer_ttl_seconds_after_finished,
                "template": {
                    "metadata": { "labels": {
                        JOB_NAME_LABEL: GITOPS_WRITER_JOB_NAME_VALUE,
                        GITOPS_CHANGE_SET_LABEL: job_label_value(&request.gitops_change_set_id),
                    }},
                    "spec": {
                        "serviceAccountName": self.config.gitops_writer_service_account,
                        "restartPolicy": "Never",
                        "initContainers": [network_policy_stabilization_container(&self.config.image)],
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
                                { "name": "TMPDIR", "value": "/work" },
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
                "activeDeadlineSeconds": self.config.git_writer_active_deadline_seconds + NETWORK_POLICY_STABILIZATION_SECONDS,
                "ttlSecondsAfterFinished": self.config.git_writer_ttl_seconds_after_finished,
                "template": {
                    "metadata": { "labels": {
                        JOB_NAME_LABEL: GIT_WRITER_JOB_NAME_VALUE,
                        CHANGE_SET_LABEL: job_label_value(&request.change_set_id),
                    }},
                    "spec": {
                        "serviceAccountName": self.config.git_writer_service_account,
                        "restartPolicy": "Never",
                        "initContainers": [network_policy_stabilization_container(&self.config.image)],
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
                                { "name": "TMPDIR", "value": "/work" },
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
                "activeDeadlineSeconds": self.config.argo_executor_active_deadline_seconds + NETWORK_POLICY_STABILIZATION_SECONDS,
                "ttlSecondsAfterFinished": self.config.argo_executor_ttl_seconds_after_finished,
                "template": {
                    "metadata": { "labels": {
                        JOB_NAME_LABEL: ARGO_EXECUTOR_JOB_NAME_VALUE,
                        DEPLOYMENT_INTENT_LABEL: job_label_value(&request.deployment_intent_id),
                    }},
                    "spec": {
                        "serviceAccountName": self.config.argo_executor_service_account,
                        "restartPolicy": "Never",
                        "initContainers": [network_policy_stabilization_container(&self.config.image)],
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
                "activeDeadlineSeconds": self.config.git_observer_active_deadline_seconds + NETWORK_POLICY_STABILIZATION_SECONDS,
                "ttlSecondsAfterFinished": self.config.git_observer_ttl_seconds_after_finished,
                "template": {
                    "metadata": { "labels": {
                        JOB_NAME_LABEL: GIT_OBSERVER_JOB_NAME_VALUE,
                        CHANGE_SET_LABEL: job_label_value(&request.change_set_id),
                    }},
                    "spec": {
                        "serviceAccountName": self.config.git_observer_service_account,
                        "restartPolicy": "Never",
                        "initContainers": [network_policy_stabilization_container(&self.config.image)],
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
                "activeDeadlineSeconds": self.config.gitops_observer_active_deadline_seconds + NETWORK_POLICY_STABILIZATION_SECONDS,
                "ttlSecondsAfterFinished": self.config.gitops_observer_ttl_seconds_after_finished,
                "template": {
                    "metadata": { "labels": {
                        JOB_NAME_LABEL: GITOPS_REVISION_RESOLVER_JOB_NAME_VALUE,
                        GITOPS_CHANGE_SET_LABEL: job_label_value(&request.gitops_change_set_id),
                    }},
                    "spec": {
                        "serviceAccountName": self.config.gitops_observer_service_account,
                        "restartPolicy": "Never",
                        "initContainers": [network_policy_stabilization_container(&self.config.image)],
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
                "activeDeadlineSeconds": self.config.active_deadline_seconds + NETWORK_POLICY_STABILIZATION_SECONDS,
                "ttlSecondsAfterFinished": self.config.ttl_seconds_after_finished,
                "template": {
                    "metadata": { "labels": {
                        JOB_NAME_LABEL: TEKTON_EXECUTOR_JOB_NAME_VALUE,
                        PIPELINE_INTENT_LABEL: job_label_value(&request.pipeline_intent_id),
                    }},
                    "spec": {
                        "serviceAccountName": self.config.tekton_executor_service_account,
                        "restartPolicy": "Never",
                        "initContainers": [network_policy_stabilization_container(&self.config.image)],
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
        let job_name = job_name(run, approval);
        let agent_profile_id = run
            .execution_target_json
            .pointer("/agent_profile/id")
            .and_then(serde_json::Value::as_str);
        let onboarding_proposer = agent_profile_id == Some("repository-onboarding-proposer");
        let runner_profile = run.execution_target_json.get("runner_profile");
        let runner_image = runner_profile
            .and_then(|profile| profile.get("image"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&self.config.image);
        let service_account = if onboarding_proposer {
            self.config.source_reader_service_account.as_str()
        } else {
            runner_profile
                .and_then(|profile| profile.get("service_account"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&self.config.service_account)
        };
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
        let active_execution_deadline_seconds = self
            .config
            .active_deadline_seconds
            .min(remaining_active_seconds);
        let active_deadline_seconds =
            active_execution_deadline_seconds.saturating_add(NETWORK_POLICY_STABILIZATION_SECONDS);
        let gateway_bound = run
            .execution_target_json
            .pointer("/inference/mode")
            .and_then(serde_json::Value::as_str)
            == Some("gateway");
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
        ];
        if !gateway_bound {
            env.push(serde_json::json!({
                "name": "FIREWORKS_API_KEY",
                "valueFrom": {
                    "secretKeyRef": {
                        "name": self.config.fireworks_secret_name,
                        "key": "api-key",
                    }
                }
            }));
        }
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
                            "agentic.lucas.engineering/inference-mode": if gateway_bound { "gateway" } else { "direct-fireworks" },
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
                        "initContainers": [network_policy_stabilization_container(runner_image)],
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
                                    "claimName": workspace_claim_name_for_run(run),
                                },
                            },
                            { "name": "tmp", "emptyDir": {} },
                        ],
                    },
                },
            },
        });
        if onboarding_proposer {
            bind_onboarding_proposer_workspace(
                &mut manifest,
                run,
                self.config.source_reader_token_secret_name.as_deref(),
                &self.config.namespace,
                runner_image,
                &self.config.workspace_dir,
            );
        }
        match run
            .execution_target_json
            .pointer("/repo_mode/workspace_access")
            .and_then(serde_json::Value::as_str)
        {
            Some("ephemeral_copy") => {
                manifest["spec"]["template"]["spec"]["initContainers"] = serde_json::json!([
                    network_policy_stabilization_container(runner_image),
                    {
                        "name":"copy-authorized-workspace",
                        "image":runner_image,
                        "imagePullPolicy":"IfNotPresent",
                        "command":["/bin/sh","-ec"],
                        "args":["find /source-workspace -mindepth 1 -maxdepth 1 -exec cp -a --no-preserve=ownership,timestamps -t /workspace -- {} +"],
                        "volumeMounts":[
                            {"name":"source-workspace","mountPath":"/source-workspace","readOnly":true},
                            {"name":"workspace","mountPath":self.config.workspace_dir},
                        ],
                        "securityContext":{
                            "allowPrivilegeEscalation":false,
                            "readOnlyRootFilesystem":true,
                            "capabilities":{"drop":["ALL"]},
                        },
                        "resources":{
                            "requests":{"cpu":"50m","memory":"128Mi","ephemeral-storage":"256Mi"},
                            "limits":{"cpu":"500m","memory":"512Mi","ephemeral-storage":ephemeral_limit},
                        },
                    }
                ]);
                manifest["spec"]["template"]["spec"]["volumes"] = serde_json::json!([
                    {"name":"workspace","emptyDir":{"sizeLimit":self.config.workspace_size_limit}},
                    {"name":"source-workspace","persistentVolumeClaim":{"claimName":workspace_claim_name_for_run(run),"readOnly":true}},
                    {"name":"tmp","emptyDir":{}},
                ]);
            }
            Some("read_only") => {
                manifest["spec"]["template"]["spec"]["containers"][0]["volumeMounts"][0]
                    ["readOnly"] = serde_json::json!(true);
                manifest["spec"]["template"]["spec"]["volumes"][0]["persistentVolumeClaim"]
                    ["readOnly"] = serde_json::json!(true);
            }
            _ => {}
        }
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
        let mut environment = vec![
            serde_json::json!({ "name": "PHARNESS_EXECUTION_KIND", "value": "environment_prepare" }),
            serde_json::json!({ "name": "PHARNESS_API_URL", "value": self.config.api_url }),
            serde_json::json!({ "name": "PHARNESS_RUN_ID", "value": run.id.as_str() }),
            serde_json::json!({ "name": "PHARNESS_ENVIRONMENT_PROFILE_ID", "value": profile.id }),
            serde_json::json!({ "name": "PHARNESS_RUNNER_IMAGE", "value": profile.image }),
            serde_json::json!({ "name": "PHARNESS_RUNNER_REVISION", "value": profile.revision }),
            serde_json::json!({ "name": "PHARNESS_RUNNER_PLATFORM", "value": profile.platform }),
            serde_json::json!({ "name": "PHARNESS_PREPARATION_STRATEGY", "value": serde_json::to_value(profile.preparation_strategy).ok().and_then(|value| value.as_str().map(str::to_owned)).unwrap_or_else(|| "unknown".into()) }),
            serde_json::json!({ "name": "PHARNESS_REQUIRED_EXECUTABLES_JSON", "value": serde_json::to_string(&profile.required_executables).unwrap_or_else(|_| "[]".to_string()) }),
            serde_json::json!({ "name": "HTTPS_PROXY", "value": format!("http://pharness-preparation-egress-proxy.{}.svc.cluster.local:8080", self.config.namespace) }),
            serde_json::json!({ "name": "https_proxy", "value": format!("http://pharness-preparation-egress-proxy.{}.svc.cluster.local:8080", self.config.namespace) }),
            serde_json::json!({ "name": "NO_PROXY", "value": ".svc,.cluster.local,127.0.0.1,localhost" }),
            serde_json::json!({ "name": "no_proxy", "value": ".svc,.cluster.local,127.0.0.1,localhost" }),
            serde_json::json!({ "name": "HOME", "value": self.config.workspace_dir }),
            serde_json::json!({ "name": "PHARNESS_WORKER_TOKEN", "valueFrom": {
                "secretKeyRef": { "name": self.config.worker_token_secret_name, "key": "token" }
            }}),
        ];
        if let Some(secret) = self.config.source_reader_token_secret_name.as_deref() {
            environment.push(
                serde_json::json!({ "name": "PHARNESS_SOURCE_READER_TOKEN", "valueFrom": {
                    "secretKeyRef": { "name": secret, "key": "token" }
                }}),
            );
        }
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
                "activeDeadlineSeconds": 1800 + NETWORK_POLICY_STABILIZATION_SECONDS,
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
                        "initContainers": [network_policy_stabilization_container(&profile.image)],
                        "containers": [{
                            "name": "prepare",
                            "image": profile.image,
                            "imagePullPolicy": "IfNotPresent",
                            "command": ["pharness-worker"],
                            "env": environment,
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
                            { "name": "workspace", "persistentVolumeClaim": { "claimName": workspace_claim_name_for_run(run) } },
                            { "name": "tmp", "emptyDir": {} },
                        ],
                    },
                },
            },
        })
    }

    fn repository_discovery_job_manifest(
        &self,
        request: &RepositoryDiscoveryRequest,
    ) -> serde_json::Value {
        let mut env = vec![
            serde_json::json!({ "name": "PHARNESS_EXECUTION_KIND", "value": "repository_discovery" }),
            serde_json::json!({ "name": "PHARNESS_API_URL", "value": self.config.api_url }),
            serde_json::json!({ "name": "PHARNESS_REPOSITORY_DISCOVERY_ID", "value": request.discovery_id }),
            serde_json::json!({ "name": "PHARNESS_WORKER_TOKEN", "valueFrom": {
                "secretKeyRef": { "name": self.config.worker_token_secret_name, "key": "token" }
            }}),
            serde_json::json!({ "name": "HTTPS_PROXY", "value": format!("http://pharness-preparation-egress-proxy.{}.svc.cluster.local:8080", self.config.namespace) }),
            serde_json::json!({ "name": "https_proxy", "value": format!("http://pharness-preparation-egress-proxy.{}.svc.cluster.local:8080", self.config.namespace) }),
            serde_json::json!({ "name": "NO_PROXY", "value": ".svc,.cluster.local,127.0.0.1,localhost" }),
            serde_json::json!({ "name": "no_proxy", "value": ".svc,.cluster.local,127.0.0.1,localhost" }),
            serde_json::json!({ "name": "HOME", "value": "/work" }),
        ];
        if let Some(secret) = self.config.source_reader_token_secret_name.as_deref() {
            env.push(
                serde_json::json!({ "name": "PHARNESS_SOURCE_READER_TOKEN", "valueFrom": {
                    "secretKeyRef": { "name": secret, "key": "token" }
                }}),
            );
        }
        serde_json::json!({
            "apiVersion": "batch/v1",
            "kind": "Job",
            "metadata": {
                "name": repository_discovery_job_name(&request.discovery_id),
                "namespace": self.config.namespace,
                "labels": {
                    JOB_NAME_LABEL: REPOSITORY_DISCOVERY_JOB_NAME_VALUE,
                    "pharness.lucas.engineering/discovery-id": job_label_value(&request.discovery_id),
                },
            },
            "spec": {
                "backoffLimit": 0,
                "activeDeadlineSeconds": self.config.source_reader_active_deadline_seconds + NETWORK_POLICY_STABILIZATION_SECONDS,
                "ttlSecondsAfterFinished": self.config.source_reader_ttl_seconds_after_finished,
                "template": {
                    "metadata": { "labels": {
                        "app": "pharness-source-reader",
                        "agentic.lucas.engineering/phase": "discovery",
                        JOB_NAME_LABEL: REPOSITORY_DISCOVERY_JOB_NAME_VALUE,
                    }},
                    "spec": {
                        "serviceAccountName": self.config.source_reader_service_account,
                        "restartPolicy": "Never",
                        "automountServiceAccountToken": false,
                        "securityContext": {
                            "runAsNonRoot": true,
                            "runAsUser": 65532,
                            "runAsGroup": 65532,
                            "fsGroup": 65532,
                            "seccompProfile": { "type": "RuntimeDefault" },
                        },
                        "initContainers": [network_policy_stabilization_container(&self.config.image)],
                        "containers": [{
                            "name": "discover",
                            "image": self.config.image,
                            "imagePullPolicy": "IfNotPresent",
                            "command": ["pharness-worker"],
                            "env": env,
                            "volumeMounts": [
                                { "name": "work", "mountPath": "/work" },
                                { "name": "tmp", "mountPath": "/tmp" },
                            ],
                            "securityContext": {
                                "allowPrivilegeEscalation": false,
                                "readOnlyRootFilesystem": true,
                                "capabilities": { "drop": ["ALL"] },
                            },
                            "resources": {
                                "requests": { "cpu": "100m", "memory": "256Mi", "ephemeral-storage": "512Mi" },
                                "limits": { "cpu": "1", "memory": "1Gi", "ephemeral-storage": "2Gi" },
                            },
                        }],
                        "volumes": [
                            { "name": "work", "emptyDir": { "sizeLimit": "2Gi" } },
                            { "name": "tmp", "emptyDir": { "sizeLimit": "128Mi" } },
                        ],
                    },
                },
            },
        })
    }

    fn onboarding_patch_job_manifest(&self, request: &OnboardingPatchRequest) -> serde_json::Value {
        let mut env = vec![
            serde_json::json!({"name":"PHARNESS_EXECUTION_KIND","value":"onboarding_patch"}),
            serde_json::json!({"name":"PHARNESS_API_URL","value":self.config.api_url}),
            serde_json::json!({"name":"PHARNESS_REPOSITORY_ONBOARDING_ID","value":request.onboarding_id}),
            serde_json::json!({"name":"PHARNESS_ONBOARDING_PATCH_EXECUTION_ID","value":request.execution_id}),
            serde_json::json!({"name":"PHARNESS_WORKER_TOKEN","valueFrom":{"secretKeyRef":{"name":self.config.worker_token_secret_name,"key":"token"}}}),
            serde_json::json!({"name":"HTTPS_PROXY","value":format!("http://pharness-preparation-egress-proxy.{}.svc.cluster.local:8080",self.config.namespace)}),
            serde_json::json!({"name":"https_proxy","value":format!("http://pharness-preparation-egress-proxy.{}.svc.cluster.local:8080",self.config.namespace)}),
            serde_json::json!({"name":"NO_PROXY","value":".svc,.cluster.local,127.0.0.1,localhost"}),
            serde_json::json!({"name":"no_proxy","value":".svc,.cluster.local,127.0.0.1,localhost"}),
            serde_json::json!({"name":"HOME","value":"/work"}),
        ];
        if let Some(secret) = self.config.source_reader_token_secret_name.as_deref() {
            env.push(serde_json::json!({"name":"PHARNESS_SOURCE_READER_TOKEN","valueFrom":{"secretKeyRef":{"name":secret,"key":"token"}}}));
        }
        serde_json::json!({
            "apiVersion":"batch/v1","kind":"Job",
            "metadata":{"name":onboarding_patch_job_name(&request.execution_id),"namespace":self.config.namespace,
                "labels":{JOB_NAME_LABEL:ONBOARDING_PATCH_JOB_NAME_VALUE,"pharness.lucas.engineering/onboarding-id":job_label_value(&request.onboarding_id)}},
            "spec":{"backoffLimit":0,"activeDeadlineSeconds":self.config.source_reader_active_deadline_seconds + NETWORK_POLICY_STABILIZATION_SECONDS,
                "ttlSecondsAfterFinished":self.config.source_reader_ttl_seconds_after_finished,
                "template":{"metadata":{"labels":{"app":"pharness-source-reader","agentic.lucas.engineering/phase":"discovery",JOB_NAME_LABEL:ONBOARDING_PATCH_JOB_NAME_VALUE}},
                    "spec":{"serviceAccountName":self.config.source_reader_service_account,"restartPolicy":"Never","automountServiceAccountToken":false,
                        "securityContext":{"runAsNonRoot":true,"runAsUser":65532,"runAsGroup":65532,"fsGroup":65532,"seccompProfile":{"type":"RuntimeDefault"}},
                        "initContainers":[network_policy_stabilization_container(&self.config.image)],
                        "containers":[{"name":"materialize","image":self.config.image,"imagePullPolicy":"IfNotPresent","command":["pharness-worker"],"env":env,
                            "volumeMounts":[{"name":"work","mountPath":"/work"},{"name":"tmp","mountPath":"/tmp"}],
                            "securityContext":{"allowPrivilegeEscalation":false,"readOnlyRootFilesystem":true,"capabilities":{"drop":["ALL"]}},
                            "resources":{"requests":{"cpu":"100m","memory":"256Mi","ephemeral-storage":"512Mi"},"limits":{"cpu":"1","memory":"1Gi","ephemeral-storage":"2Gi"}}}],
                        "volumes":[{"name":"work","emptyDir":{"sizeLimit":"2Gi"}},{"name":"tmp","emptyDir":{"sizeLimit":"128Mi"}}]
                    }
                }
            }
        })
    }

    fn onboarding_contract_validation_job_manifest(
        &self,
        request: &OnboardingContractValidationRequest,
    ) -> serde_json::Value {
        let mut env = vec![
            serde_json::json!({"name":"PHARNESS_EXECUTION_KIND","value":"onboarding_contract_validate"}),
            serde_json::json!({"name":"PHARNESS_API_URL","value":self.config.api_url}),
            serde_json::json!({"name":"PHARNESS_REPOSITORY_ONBOARDING_ID","value":request.onboarding_id}),
            serde_json::json!({"name":"PHARNESS_ONBOARDING_VALIDATION_EXECUTION_ID","value":request.execution_id}),
            serde_json::json!({"name":"PHARNESS_WORKER_TOKEN","valueFrom":{"secretKeyRef":{"name":self.config.worker_token_secret_name,"key":"token"}}}),
            serde_json::json!({"name":"HTTPS_PROXY","value":format!("http://pharness-preparation-egress-proxy.{}.svc.cluster.local:8080",self.config.namespace)}),
            serde_json::json!({"name":"https_proxy","value":format!("http://pharness-preparation-egress-proxy.{}.svc.cluster.local:8080",self.config.namespace)}),
            serde_json::json!({"name":"NO_PROXY","value":".svc,.cluster.local,127.0.0.1,localhost"}),
            serde_json::json!({"name":"no_proxy","value":".svc,.cluster.local,127.0.0.1,localhost"}),
            serde_json::json!({"name":"HOME","value":"/work"}),
        ];
        if let Some(secret) = self.config.source_reader_token_secret_name.as_deref() {
            env.push(serde_json::json!({"name":"PHARNESS_SOURCE_READER_TOKEN","valueFrom":{"secretKeyRef":{"name":secret,"key":"token"}}}));
        }
        serde_json::json!({
            "apiVersion":"batch/v1","kind":"Job",
            "metadata":{"name":onboarding_contract_validation_job_name(&request.execution_id),"namespace":self.config.namespace,
                "labels":{JOB_NAME_LABEL:ONBOARDING_VALIDATION_JOB_NAME_VALUE,"pharness.lucas.engineering/onboarding-id":job_label_value(&request.onboarding_id)}},
            "spec":{"backoffLimit":0,"activeDeadlineSeconds":self.config.source_reader_active_deadline_seconds + NETWORK_POLICY_STABILIZATION_SECONDS,
                "ttlSecondsAfterFinished":self.config.source_reader_ttl_seconds_after_finished,
                "template":{"metadata":{"labels":{"app":"pharness-source-reader","agentic.lucas.engineering/phase":"validation",JOB_NAME_LABEL:ONBOARDING_VALIDATION_JOB_NAME_VALUE}},
                    "spec":{"serviceAccountName":self.config.source_reader_service_account,"restartPolicy":"Never","automountServiceAccountToken":false,
                        "securityContext":{"runAsNonRoot":true,"runAsUser":65532,"runAsGroup":65532,"fsGroup":65532,"seccompProfile":{"type":"RuntimeDefault"}},
                        "initContainers":[network_policy_stabilization_container(&self.config.image)],
                        "containers":[{"name":"validate","image":self.config.image,"imagePullPolicy":"IfNotPresent","command":["pharness-worker"],"env":env,
                            "volumeMounts":[{"name":"work","mountPath":"/work"},{"name":"tmp","mountPath":"/tmp"}],
                            "securityContext":{"allowPrivilegeEscalation":false,"readOnlyRootFilesystem":true,"capabilities":{"drop":["ALL"]}},
                            "resources":{"requests":{"cpu":"100m","memory":"256Mi","ephemeral-storage":"512Mi"},"limits":{"cpu":"1","memory":"1Gi","ephemeral-storage":"2Gi"}}}],
                        "volumes":[{"name":"work","emptyDir":{"sizeLimit":"2Gi"}},{"name":"tmp","emptyDir":{"sizeLimit":"128Mi"}}]
                    }
                }
            }
        })
    }

    fn repository_readiness_job_manifest(
        &self,
        request: &RepositoryReadinessExecutionRequest,
    ) -> serde_json::Value {
        let profile = &request.profile;
        let mut env = vec![
            serde_json::json!({"name":"PHARNESS_EXECUTION_KIND","value":"repository_readiness"}),
            serde_json::json!({"name":"PHARNESS_API_URL","value":self.config.api_url}),
            serde_json::json!({"name":"PHARNESS_REPOSITORY_PREPARATION_ID","value":request.preparation_id}),
            serde_json::json!({"name":"PHARNESS_ENVIRONMENT_PROFILE_ID","value":profile.id}),
            serde_json::json!({"name":"PHARNESS_RUNNER_IMAGE","value":profile.image}),
            serde_json::json!({"name":"PHARNESS_RUNNER_REVISION","value":profile.revision}),
            serde_json::json!({"name":"PHARNESS_RUNNER_PLATFORM","value":profile.platform}),
            serde_json::json!({"name":"PHARNESS_PREPARATION_STRATEGY","value":serde_json::to_value(profile.preparation_strategy).ok().and_then(|value|value.as_str().map(str::to_owned)).unwrap_or_else(||"unknown".into())}),
            serde_json::json!({"name":"PHARNESS_REQUIRED_EXECUTABLES_JSON","value":serde_json::to_string(&profile.required_executables).unwrap_or_else(|_| "[]".into())}),
            serde_json::json!({"name":"PHARNESS_WORKER_TOKEN","valueFrom":{"secretKeyRef":{"name":self.config.worker_token_secret_name,"key":"token"}}}),
            serde_json::json!({"name":"HTTPS_PROXY","value":format!("http://pharness-preparation-egress-proxy.{}.svc.cluster.local:8080",self.config.namespace)}),
            serde_json::json!({"name":"https_proxy","value":format!("http://pharness-preparation-egress-proxy.{}.svc.cluster.local:8080",self.config.namespace)}),
            serde_json::json!({"name":"NO_PROXY","value":".svc,.cluster.local,127.0.0.1,localhost"}),
            serde_json::json!({"name":"no_proxy","value":".svc,.cluster.local,127.0.0.1,localhost"}),
            serde_json::json!({"name":"HOME","value":"/work"}),
        ];
        if let Some(secret) = self.config.source_reader_token_secret_name.as_deref() {
            env.push(serde_json::json!({"name":"PHARNESS_SOURCE_READER_TOKEN","valueFrom":{"secretKeyRef":{"name":secret,"key":"token"}}}));
        }
        serde_json::json!({
            "apiVersion":"batch/v1","kind":"Job",
            "metadata":{"name":repository_readiness_job_name(&request.preparation_id),"namespace":self.config.namespace,
                "labels":{JOB_NAME_LABEL:REPOSITORY_READINESS_JOB_NAME_VALUE,"pharness.lucas.engineering/preparation-id":job_label_value(&request.preparation_id)}},
            "spec":{"backoffLimit":0,"activeDeadlineSeconds":1800 + NETWORK_POLICY_STABILIZATION_SECONDS,
                "ttlSecondsAfterFinished":self.config.source_reader_ttl_seconds_after_finished,
                "template":{"metadata":{"labels":{"app":"pharness-runner","agentic.lucas.engineering/phase":"preparation",JOB_NAME_LABEL:REPOSITORY_READINESS_JOB_NAME_VALUE}},
                    "spec":{"serviceAccountName":profile.service_account,"restartPolicy":"Never","automountServiceAccountToken":false,
                        "securityContext":{"runAsNonRoot":true,"runAsUser":65532,"runAsGroup":65532,"fsGroup":65532,"seccompProfile":{"type":"RuntimeDefault"}},
                        "initContainers":[network_policy_stabilization_container(&profile.image)],
                        "containers":[{"name":"prepare","image":profile.image,"imagePullPolicy":"IfNotPresent","command":["pharness-worker"],"env":env,
                            "volumeMounts":[{"name":"work","mountPath":"/work"},{"name":"tmp","mountPath":"/tmp"}],
                            "securityContext":{"allowPrivilegeEscalation":false,"readOnlyRootFilesystem":true,"capabilities":{"drop":["ALL"]}},
                            "resources":{"requests":{"cpu":"100m","memory":"256Mi","ephemeral-storage":"512Mi"},"limits":{"cpu":profile.limits.cpu,"memory":profile.limits.memory,"ephemeral-storage":profile.limits.ephemeral_storage}}}],
                        "volumes":[{"name":"work","emptyDir":{"sizeLimit":"2Gi"}},{"name":"tmp","emptyDir":{"sizeLimit":"128Mi"}}]
                    }
                }
            }
        })
    }

    fn inference_evaluation_job_manifest(
        &self,
        request: &InferenceEvaluationExecutionRequest,
    ) -> serde_json::Value {
        let job_name = inference_evaluation_job_name(&request.evaluation_id);
        let mut manifest = serde_json::json!({
            "apiVersion":"batch/v1",
            "kind":"Job",
            "metadata":{
                "name":job_name,
                "namespace":self.config.namespace,
                "labels":{
                    JOB_NAME_LABEL:INFERENCE_EVALUATION_JOB_NAME_VALUE,
                    "pharness.lucas.engineering/inference-evaluation-id":job_label_value(&request.evaluation_id),
                },
            },
            "spec":{
                "backoffLimit":0,
                "activeDeadlineSeconds":7200 + NETWORK_POLICY_STABILIZATION_SECONDS,
                "ttlSecondsAfterFinished":self.config.ttl_seconds_after_finished,
                "template":{
                    "metadata":{"labels":{
                        "app":"pharness-inference-evaluator",
                        "agentic.lucas.engineering/phase":"inference-evaluation",
                        "agentic.lucas.engineering/inference-mode":"gateway",
                        JOB_NAME_LABEL:INFERENCE_EVALUATION_JOB_NAME_VALUE,
                    }},
                    "spec":{
                        "serviceAccountName":self.config.service_account,
                        "restartPolicy":"Never",
                        "automountServiceAccountToken":false,
                        "securityContext":{
                            "runAsNonRoot":true,
                            "runAsUser":65532,
                            "runAsGroup":65532,
                            "fsGroup":65532,
                            "seccompProfile":{"type":"RuntimeDefault"},
                        },
                        "initContainers":[network_policy_stabilization_container(&self.config.image)],
                        "containers":[{
                            "name":"evaluate",
                            "image":self.config.inference_evaluation_image,
                            "imagePullPolicy":"IfNotPresent",
                            "command":["pharness-eval"],
                            "args":["execute-qualification","--evaluation-id",request.evaluation_id],
                            "env":[
                                {"name":"PHARNESS_API_URL","value":self.config.api_url},
                                {"name":"PHARNESS_INFERENCE_GATEWAY_URL","value":request.gateway_url},
                                {"name":"PHARNESS_INFERENCE_GATEWAY_ENABLED","value":"true"},
                                {"name":"PHARNESS_MODEL_GRANT_SIGNER_ENABLED","value":"false"},
                                {"name":"PHARNESS_EVAL_ARTIFACT_DIR","value":"/work/artifacts"},
                                {"name":"PHARNESS_INFERENCE_REGISTRY_JSON","valueFrom":{"configMapKeyRef":{"name":"pharness-inference-registry","key":"registry.json"}}},
                                {"name":"PHARNESS_WORKER_TOKEN","valueFrom":{"secretKeyRef":{"name":self.config.worker_token_secret_name,"key":"token"}}},
                                {"name":"HOME","value":"/work"},
                                {"name":"TMPDIR","value":"/tmp"},
                                {"name":"NO_PROXY","value":".svc,.cluster.local,127.0.0.1,localhost"},
                                {"name":"no_proxy","value":".svc,.cluster.local,127.0.0.1,localhost"}
                            ],
                            "volumeMounts":[{"name":"work","mountPath":"/work"},{"name":"tmp","mountPath":"/tmp"}],
                            "securityContext":{"allowPrivilegeEscalation":false,"readOnlyRootFilesystem":true,"capabilities":{"drop":["ALL"]}},
                            "resources":{
                                "requests":{"cpu":"250m","memory":"512Mi","ephemeral-storage":"1Gi"},
                                "limits":{"cpu":"2","memory":"2Gi","ephemeral-storage":"4Gi"}
                            }
                        }],
                        "volumes":[{"name":"work","emptyDir":{"sizeLimit":"4Gi"}},{"name":"tmp","emptyDir":{"sizeLimit":"1Gi"}}]
                    }
                }
            }
        });
        if let Some(hostname) = self.config.inference_evaluation_node_hostname.as_deref() {
            manifest["spec"]["template"]["spec"]["affinity"] = required_node_affinity(hostname);
        }
        manifest
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
        self.reap_onboarding_jobs().await?;
        self.reap_inference_evaluation_jobs().await?;
        self.reap_tekton_executor_jobs().await
    }

    async fn reap_inference_evaluation_jobs(&self) -> anyhow::Result<()> {
        let evaluations = self.store.list_active_inference_evaluations().await?;
        for evaluation in evaluations {
            let Some(job_name) = evaluation.job_name.as_deref() else {
                continue;
            };
            let output = tokio::process::Command::new(&self.kubectl_bin)
                .args([
                    "get",
                    "job",
                    job_name,
                    "-n",
                    &self.config.namespace,
                    "--ignore-not-found=true",
                    "-o",
                    "json",
                ])
                .output()
                .await?;
            if !output.status.success() {
                anyhow::bail!(
                    "kubectl get inference evaluation Job failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            if output.stdout.is_empty() {
                let refreshed = self.store.get_inference_evaluation(&evaluation.id).await?;
                if !refreshed
                    .as_ref()
                    .is_some_and(|value| matches!(value.status.as_str(), "queued" | "running"))
                {
                    continue;
                }
                let reason = "inference evaluation Job is missing before a durable outcome";
                self.store
                    .fail_inference_evaluation(&evaluation.id, reason)
                    .await?;
                tracing::warn!(evaluation_id=%evaluation.id, job=%job_name, %reason, "sealed missing inference evaluation Job");
                continue;
            }
            let job: serde_json::Value = serde_json::from_slice(&output.stdout)?;
            let terminal = executor_job_terminal_state(&job);
            if terminal == ExecutorJobTerminalState::Active {
                continue;
            }
            let refreshed = self.store.get_inference_evaluation(&evaluation.id).await?;
            if refreshed
                .as_ref()
                .is_some_and(|value| value.status == "completed")
            {
                continue;
            }
            let reason = match terminal {
                ExecutorJobTerminalState::Succeeded => {
                    "inference evaluation Job completed without a durable report"
                }
                ExecutorJobTerminalState::Failed => "inference evaluation Job failed",
                ExecutorJobTerminalState::Active => unreachable!(),
            };
            self.store
                .fail_inference_evaluation(&evaluation.id, reason)
                .await?;
            tracing::warn!(evaluation_id=%evaluation.id, job=%job_name, %reason, "sealed missing inference evaluation outcome");
        }
        Ok(())
    }

    async fn reap_onboarding_jobs(&self) -> anyhow::Result<()> {
        let selector = format!(
            "{JOB_NAME_LABEL} in ({ONBOARDING_PATCH_JOB_NAME_VALUE},{ONBOARDING_VALIDATION_JOB_NAME_VALUE})"
        );
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
                "kubectl get onboarding jobs failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let jobs: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        let items = jobs
            .get("items")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let onboardings = self
            .store
            .list_repository_onboardings_awaiting_isolated_job()
            .await?;
        for onboarding in onboardings {
            let (execution_id, job_name) = match onboarding.status.as_str() {
                "patch_queued" => {
                    let Some(execution_id) = onboarding.patch_execution_id.as_deref() else {
                        continue;
                    };
                    (execution_id, onboarding_patch_job_name(execution_id))
                }
                "validation_queued" => {
                    let Some(execution_id) = onboarding.validation_execution_id.as_deref() else {
                        continue;
                    };
                    (
                        execution_id,
                        onboarding_contract_validation_job_name(execution_id),
                    )
                }
                _ => continue,
            };
            let terminal = items.iter().find(|job| {
                job.pointer("/metadata/name")
                    .and_then(serde_json::Value::as_str)
                    == Some(job_name.as_str())
            });
            let Some(job) = terminal else {
                continue;
            };
            if executor_job_terminal_state(job) == ExecutorJobTerminalState::Active {
                continue;
            }
            match onboarding.status.as_str() {
                "patch_queued" => {
                    self.store
                        .fail_repository_onboarding_patch(
                            &onboarding.id,
                            execution_id,
                            "onboarding_patch_job_terminated_without_outcome",
                        )
                        .await?;
                }
                "validation_queued" => {
                    self.store
                        .fail_repository_onboarding_contract_validation(
                            &onboarding.id,
                            execution_id,
                            "onboarding contract validation Job terminated without a durable outcome",
                        )
                        .await?;
                }
                _ => {}
            }
            tracing::warn!(
                onboarding_id = %onboarding.id,
                execution_id,
                job = %job_name,
                "reconciled missing repository onboarding Job outcome"
            );
        }
        Ok(())
    }

    async fn cleanup_retention_resources(
        &self,
        resources: &[serde_json::Value],
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        let pods_output = tokio::process::Command::new(&self.kubectl_bin)
            .args(["get", "pods", "-n", &self.config.namespace, "-o", "json"])
            .output()
            .await?;
        if !pods_output.status.success() {
            anyhow::bail!(
                "kubectl get pods failed during retention precondition: {}",
                String::from_utf8_lossy(&pods_output.stderr)
            );
        }
        let pods: serde_json::Value = serde_json::from_slice(&pods_output.stdout)?;
        let mut cleaned = Vec::new();
        for resource in resources {
            let workspace_id = resource
                .get("workspace_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("retention workspace has no workspace_id"))?;
            let claim_name = resource
                .get("pvc_name")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("retention workspace has no pvc_name"))?;
            let pvc_identity = resource
                .get("pvc_identity")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("retention workspace has no PVC identity"))?;
            let pvc_identity_kind = resource
                .get("pvc_identity_kind")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("retention workspace has no PVC identity kind"))?;
            if claim_name != workspace_claim_name(pvc_identity) {
                anyhow::bail!(
                    "retention PVC name does not match its server-derived workspace identity"
                );
            }
            let pvc_output = tokio::process::Command::new(&self.kubectl_bin)
                .args([
                    "get",
                    "persistentvolumeclaim",
                    claim_name,
                    "-n",
                    &self.config.namespace,
                    "--ignore-not-found=true",
                    "-o",
                    "json",
                ])
                .output()
                .await?;
            if !pvc_output.status.success() {
                anyhow::bail!(
                    "kubectl get retention PVC failed: {}",
                    String::from_utf8_lossy(&pvc_output.stderr)
                );
            }
            if !pvc_output.stdout.is_empty() {
                let pvc: serde_json::Value = serde_json::from_slice(&pvc_output.stdout)?;
                let labels = pvc
                    .pointer("/metadata/labels")
                    .and_then(serde_json::Value::as_object)
                    .ok_or_else(|| anyhow::anyhow!("retention PVC has no PHarness labels"))?;
                let expected_identity_label = if pvc_identity_kind == "workspace" {
                    WORKSPACE_ID_LABEL
                } else if pvc_identity_kind == "run" {
                    RUN_ID_LABEL
                } else {
                    anyhow::bail!("retention PVC identity kind is unsupported");
                };
                if labels
                    .get(JOB_NAME_LABEL)
                    .and_then(serde_json::Value::as_str)
                    != Some(WORKSPACE_CLAIM_NAME_VALUE)
                    || labels
                        .get(expected_identity_label)
                        .and_then(serde_json::Value::as_str)
                        != Some(job_label_value(pvc_identity).as_str())
                {
                    anyhow::bail!("retention PVC is not labeled for the exact PHarness workspace");
                }
                let mounted = pods
                    .get("items")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .any(|pod| {
                        let phase = pod
                            .pointer("/status/phase")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("Unknown");
                        !matches!(phase, "Succeeded" | "Failed")
                            && pod
                                .pointer("/spec/volumes")
                                .and_then(serde_json::Value::as_array)
                                .into_iter()
                                .flatten()
                                .any(|volume| {
                                    volume
                                        .pointer("/persistentVolumeClaim/claimName")
                                        .and_then(serde_json::Value::as_str)
                                        == Some(claim_name)
                                })
                    });
                if mounted {
                    anyhow::bail!("retention PVC is still mounted by a nonterminal Pod");
                }
                let deleted = tokio::process::Command::new(&self.kubectl_bin)
                    .args([
                        "delete",
                        "persistentvolumeclaim",
                        claim_name,
                        "-n",
                        &self.config.namespace,
                        "--ignore-not-found=true",
                        "--wait=false",
                    ])
                    .output()
                    .await?;
                if !deleted.status.success() {
                    anyhow::bail!(
                        "kubectl delete retention PVC failed: {}",
                        String::from_utf8_lossy(&deleted.stderr)
                    );
                }
            }
            for run_id in resource
                .get("run_ids")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
            {
                let selector = format!(
                    "{RUN_ID_LABEL}={},{}",
                    job_label_value(run_id),
                    JOB_NAME_LABEL
                );
                let deleted = tokio::process::Command::new(&self.kubectl_bin)
                    .args([
                        "delete",
                        "jobs",
                        "-n",
                        &self.config.namespace,
                        "-l",
                        &selector,
                        "--ignore-not-found=true",
                        "--wait=false",
                    ])
                    .output()
                    .await?;
                if !deleted.status.success() {
                    anyhow::bail!(
                        "kubectl delete retention Jobs failed: {}",
                        String::from_utf8_lossy(&deleted.stderr)
                    );
                }
            }
            cleaned.push(serde_json::json!({
                "workspace_id":workspace_id,
                "pvc_name":claim_name,
                "namespace":self.config.namespace,
                "status":"deleted_or_already_absent",
            }));
        }
        Ok(cleaned)
    }

    async fn delete_archive_claims(
        &self,
        archive_id: &str,
        archived_generation_id: &str,
        database_claim: &str,
        archive_claim: &str,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        let pods_output = tokio::process::Command::new(&self.kubectl_bin)
            .args(["get", "pods", "-n", &self.config.namespace, "-o", "json"])
            .output()
            .await?;
        if !pods_output.status.success() {
            anyhow::bail!("could not verify archive PVC mount state");
        }
        let pods: serde_json::Value = serde_json::from_slice(&pods_output.stdout)?;
        let mut deleted = Vec::new();
        for (claim_name, role) in [(database_claim, "database"), (archive_claim, "archive")] {
            let output = tokio::process::Command::new(&self.kubectl_bin)
                .args([
                    "get",
                    "persistentvolumeclaim",
                    claim_name,
                    "-n",
                    &self.config.namespace,
                    "--ignore-not-found=true",
                    "-o",
                    "json",
                ])
                .output()
                .await?;
            if !output.status.success() {
                anyhow::bail!("could not inspect exact archive PVC {claim_name}");
            }
            if output.stdout.is_empty() {
                deleted.push(
                    serde_json::json!({"claim":claim_name,"role":role,"status":"already_absent"}),
                );
                continue;
            }
            let pvc: serde_json::Value = serde_json::from_slice(&output.stdout)?;
            let labels = pvc
                .pointer("/metadata/labels")
                .and_then(serde_json::Value::as_object)
                .ok_or_else(|| anyhow::anyhow!("archive PVC {claim_name} has no labels"))?;
            if labels
                .get("app.kubernetes.io/part-of")
                .and_then(serde_json::Value::as_str)
                != Some("pharness")
                || labels
                    .get("pharness.dev/data-generation")
                    .and_then(serde_json::Value::as_str)
                    != Some(archived_generation_id)
            {
                anyhow::bail!(
                    "archive PVC {claim_name} is not bound to the archived PHarness generation"
                );
            }
            if role == "archive" {
                if labels
                    .get("pharness.dev/archive-role")
                    .and_then(serde_json::Value::as_str)
                    != Some("database-backup")
                {
                    anyhow::bail!("archive backup PVC does not have the database-backup role");
                }
                if let Some(label_archive_id) = labels
                    .get("pharness.dev/archive-id")
                    .and_then(serde_json::Value::as_str)
                {
                    if label_archive_id != archive_id {
                        anyhow::bail!("archive backup PVC is bound to a different ArchiveRecord");
                    }
                }
            }
            let mounted = pods
                .get("items")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .any(|pod| {
                    pod.pointer("/spec/volumes")
                        .and_then(serde_json::Value::as_array)
                        .into_iter()
                        .flatten()
                        .any(|volume| {
                            volume
                                .pointer("/persistentVolumeClaim/claimName")
                                .and_then(serde_json::Value::as_str)
                                == Some(claim_name)
                        })
                });
            if mounted {
                anyhow::bail!("archive PVC {claim_name} is still mounted");
            }
            let output = tokio::process::Command::new(&self.kubectl_bin)
                .args([
                    "delete",
                    "persistentvolumeclaim",
                    claim_name,
                    "-n",
                    &self.config.namespace,
                    "--ignore-not-found=true",
                    "--wait=false",
                ])
                .output()
                .await?;
            if !output.status.success() {
                anyhow::bail!("could not delete exact archive PVC {claim_name}");
            }
            deleted.push(serde_json::json!({"claim":claim_name,"role":role,"status":"deleted"}));
        }
        Ok(deleted)
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
            let failure = "worker job failed before reporting a durable outcome";
            if matches!(run.status.as_str(), "queued" | "running") {
                tracing::warn!(run_id = %run_id, "worker job failed without durable outcome");
                fail_run_from_dispatch(&self.store, &run_id, failure.to_string()).await?;
            } else if run.status == "failed" {
                // Recover runs that were made terminal by an earlier API
                // version before the corresponding Repo Mode StageExecution
                // could be sealed. The stage finalizer is idempotent and is a
                // no-op for non-Repo-Mode runs.
                if run
                    .execution_target_json
                    .get("hosted_workflow_policy_hash")
                    .is_some()
                {
                    crate::worker::reconcile_terminal_hosted_run(&self.store, &run).await?;
                } else {
                    let error = run.error.clone().unwrap_or_else(|| failure.to_string());
                    sync_repo_stage_run(
                        &self.store,
                        &run,
                        &pharness_runhost::AttemptOutcome::failed(error),
                    )
                    .await?;
                }
            }

            // A proposer can fail in an init container before its worker can
            // report an outcome through the internal API. Keep the parent
            // onboarding recoverable even when the Run was already sealed by
            // an earlier reaper pass (for example, across an API rollout).
            if let Some(onboarding_id) = onboarding_proposer_id(&run) {
                if let Some(onboarding) =
                    self.store.get_repository_onboarding(onboarding_id).await?
                {
                    if onboarding.status == "proposal_running"
                        && onboarding.proposer_run_id.as_deref() == Some(run.id.as_str())
                    {
                        self.store
                            .fail_repository_onboarding_proposer(
                                onboarding_id,
                                run.id.as_str(),
                                failure,
                            )
                            .await?;
                    }
                }
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

fn onboarding_proposer_id(run: &StoredRun) -> Option<&str> {
    run.execution_target_json
        .pointer("/onboarding/onboarding_id")
        .and_then(serde_json::Value::as_str)
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

#[derive(Debug, PartialEq, Eq)]
enum ChainedRunCapacity {
    Available,
    WaitingForPredecessor { active: u64 },
}

fn chained_run_capacity(
    jobs: &serde_json::Value,
    max_concurrent_jobs: u32,
    predecessor_run_label: &str,
) -> anyhow::Result<ChainedRunCapacity> {
    let active = active_run_job_count(jobs);
    if active < max_concurrent_jobs as u64 {
        return Ok(ChainedRunCapacity::Available);
    }

    let only_predecessor_is_active = jobs
        .get("items")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|job| {
            job.pointer("/status/active")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0
        })
        .all(|job| {
            job.pointer("/metadata/labels")
                .and_then(|labels| labels.get(RUN_ID_LABEL))
                .and_then(serde_json::Value::as_str)
                == Some(predecessor_run_label)
        });
    if only_predecessor_is_active {
        return Ok(ChainedRunCapacity::WaitingForPredecessor { active });
    }

    anyhow::bail!(
        "worker Job concurrency limit reached: {active} active, limit {max_concurrent_jobs}; active capacity is not owned exclusively by predecessor Run {predecessor_run_label}"
    )
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

fn bind_source_delivery_manifest(
    manifest: &mut serde_json::Value,
    intent_id: &str,
    env_name: &str,
) -> anyhow::Result<()> {
    for pointer in ["/metadata/labels", "/spec/template/metadata/labels"] {
        let labels = manifest
            .pointer_mut(pointer)
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| anyhow::anyhow!("source delivery Job has no labels at {pointer}"))?;
        labels.remove(CHANGE_SET_LABEL);
        labels.insert(
            SOURCE_DELIVERY_INTENT_LABEL.into(),
            serde_json::Value::String(job_label_value(intent_id)),
        );
    }
    let env = manifest
        .pointer_mut("/spec/template/spec/containers/0/env")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| anyhow::anyhow!("source delivery Job has no executor environment"))?;
    env.retain(|entry| {
        entry.get("name").and_then(serde_json::Value::as_str) != Some("PHARNESS_CHANGE_SET_ID")
    });
    env.push(serde_json::json!({"name":env_name,"value":intent_id}));
    Ok(())
}

fn bind_onboarding_proposer_workspace(
    manifest: &mut serde_json::Value,
    run: &StoredRun,
    source_reader_secret: Option<&str>,
    namespace: &str,
    image: &str,
    workspace_dir: &str,
) {
    let source = run
        .execution_target_json
        .get("workspace_source")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    for pointer in ["/metadata/labels", "/spec/template/metadata/labels"] {
        if let Some(labels) = manifest
            .pointer_mut(pointer)
            .and_then(serde_json::Value::as_object_mut)
        {
            labels.insert(
                "agentic.lucas.engineering/onboarding-proposer".into(),
                serde_json::Value::String("true".into()),
            );
        }
    }
    manifest["spec"]["template"]["spec"]["automountServiceAccountToken"] = serde_json::json!(false);
    let mut env = vec![
        serde_json::json!({"name":"PHARNESS_ONBOARDING_REPOSITORY","value":source.get("source_repo").and_then(serde_json::Value::as_str).unwrap_or("")}),
        serde_json::json!({"name":"PHARNESS_ONBOARDING_SOURCE_COMMIT","value":source.get("source_commit").and_then(serde_json::Value::as_str).unwrap_or("")}),
        serde_json::json!({"name":"PHARNESS_ONBOARDING_BRANCH","value":source.get("branch").and_then(serde_json::Value::as_str).unwrap_or("")}),
        serde_json::json!({"name":"HTTPS_PROXY","value":format!("http://pharness-preparation-egress-proxy.{namespace}.svc.cluster.local:8080")}),
        serde_json::json!({"name":"https_proxy","value":format!("http://pharness-preparation-egress-proxy.{namespace}.svc.cluster.local:8080")}),
        serde_json::json!({"name":"NO_PROXY","value":".svc,.cluster.local,127.0.0.1,localhost"}),
        serde_json::json!({"name":"no_proxy","value":".svc,.cluster.local,127.0.0.1,localhost"}),
        serde_json::json!({"name":"HOME","value":"/tmp"}),
    ];
    if let Some(secret) = source_reader_secret {
        env.push(serde_json::json!({"name":"PHARNESS_SOURCE_READER_TOKEN","valueFrom":{"secretKeyRef":{"name":secret,"key":"token"}}}));
    }
    manifest["spec"]["template"]["spec"]["initContainers"] = serde_json::json!([
        network_policy_stabilization_container(image),
        {
            "name":"prepare-onboarding-source",
            "image":image,
            "imagePullPolicy":"IfNotPresent",
            "command":["/bin/sh","-ec"],
            "args":[r#"
test -n "$PHARNESS_ONBOARDING_REPOSITORY"
test -n "$PHARNESS_ONBOARDING_SOURCE_COMMIT"
test -n "$PHARNESS_ONBOARDING_BRANCH"
cat > /tmp/askpass <<'EOF'
#!/bin/sh
case "$1" in
  *Username*) printf '%s\n' x-access-token ;;
  *) printf '%s\n' "${PHARNESS_SOURCE_READER_TOKEN:-}" ;;
esac
EOF
chmod 700 /tmp/askpass
git init -q "$PHARNESS_WORKSPACE_DIR"
git -c safe.directory="$PHARNESS_WORKSPACE_DIR" -C "$PHARNESS_WORKSPACE_DIR" remote add origin "$PHARNESS_ONBOARDING_REPOSITORY"
GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=/tmp/askpass git -c safe.directory="$PHARNESS_WORKSPACE_DIR" -C "$PHARNESS_WORKSPACE_DIR" fetch --depth=1 origin "$PHARNESS_ONBOARDING_SOURCE_COMMIT"
git -c safe.directory="$PHARNESS_WORKSPACE_DIR" -C "$PHARNESS_WORKSPACE_DIR" checkout -q -b "$PHARNESS_ONBOARDING_BRANCH" FETCH_HEAD
test "$(git -c safe.directory="$PHARNESS_WORKSPACE_DIR" -C "$PHARNESS_WORKSPACE_DIR" rev-parse HEAD)" = "$PHARNESS_ONBOARDING_SOURCE_COMMIT"
"#],
            "env":env,
            "volumeMounts":[
                {"name":"workspace","mountPath":workspace_dir},
                {"name":"tmp","mountPath":"/tmp"},
            ],
            "securityContext":{"allowPrivilegeEscalation":false,"readOnlyRootFilesystem":true,"capabilities":{"drop":["ALL"]}},
            "resources":{"requests":{"cpu":"50m","memory":"128Mi","ephemeral-storage":"256Mi"},"limits":{"cpu":"500m","memory":"512Mi","ephemeral-storage":"1Gi"}},
        }
    ]);
    manifest["spec"]["template"]["spec"]["initContainers"][1]["env"]
        .as_array_mut()
        .expect("onboarding proposer env is an array")
        .push(serde_json::json!({"name":"PHARNESS_WORKSPACE_DIR","value":workspace_dir}));
}

fn run_label_to_run_id(label: &str) -> String {
    // run ids are `run_<digits>`; the label form is `run-<digits>`.
    match label.strip_prefix("run-") {
        Some(rest) => format!("run_{rest}"),
        None => label.to_string(),
    }
}

fn job_name(run: &StoredRun, approval: Option<&StoredApproval>) -> String {
    let base = job_label_value(run.id.as_str());
    match approval {
        None if run.budget_consumption.extensions == 0 => format!("pharness-{base}-i"),
        None => format!("pharness-{base}-b{:02}", run.budget_consumption.extensions),
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

fn shared_repo_workspace_id(run: &StoredRun) -> Option<&str> {
    run.execution_target_json
        .pointer("/repo_mode/chain_authorization_id")?;
    run.execution_target_json
        .pointer("/workspace_source/workspace_id")
        .and_then(serde_json::Value::as_str)
}

fn workspace_claim_name_for_run(run: &StoredRun) -> String {
    shared_repo_workspace_id(run)
        .map(workspace_claim_name)
        .unwrap_or_else(|| workspace_claim_name(run.id.as_str()))
}

fn environment_preparation_job_name(run_id: &str) -> String {
    format!("pharness-{}-prepare", job_label_value(run_id))
}

fn repository_discovery_job_name(discovery_id: &str) -> String {
    let digest = Sha256::digest(discovery_id.as_bytes());
    let suffix = digest
        .iter()
        .take(9)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("pharness-discovery-{suffix}")
}

fn onboarding_patch_job_name(execution_id: &str) -> String {
    let digest = Sha256::digest(execution_id.as_bytes());
    format!(
        "pharness-onboarding-patch-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5]
    )
}

fn onboarding_contract_validation_job_name(execution_id: &str) -> String {
    let digest = Sha256::digest(execution_id.as_bytes());
    format!(
        "pharness-onboarding-validate-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5]
    )
}

fn repository_readiness_job_name(preparation_id: &str) -> String {
    let digest = Sha256::digest(preparation_id.as_bytes());
    format!(
        "pharness-repository-ready-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5]
    )
}

fn inference_evaluation_job_name(evaluation_id: &str) -> String {
    let digest = Sha256::digest(evaluation_id.as_bytes());
    format!(
        "pharness-inference-eval-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5]
    )
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
        active_run_job_count, argo_executor_job_name, chained_run_capacity,
        enforce_run_job_capacity, executor_job_terminal_state, git_observer_job_name,
        git_writer_job_name, gitops_observer_job_name, gitops_writer_job_name, job_label_value,
        job_name, onboarding_proposer_id, run_label_to_run_id, workspace_claim_name,
        workspace_claim_name_for_run, ArgoSyncExecutionRequest, ChainedRunCapacity,
        ExecutorJobTerminalState, GitDeliveryExecutionRequest, GitDeliveryObservationRequest,
        GitOpsDeliveryExecutionRequest, GitOpsDeliveryObservationRequest,
        InferenceEvaluationExecutionRequest, KubernetesJobDispatcher,
        OnboardingContractValidationRequest, OnboardingPatchRequest, RepositoryDiscoveryRequest,
        RepositoryReadinessExecutionRequest, NETWORK_POLICY_STABILIZATION_SECONDS, RUN_ID_LABEL,
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

    #[tokio::test]
    async fn inference_evaluation_manifest_has_only_internal_worker_identity() {
        let mut dispatcher = test_dispatcher(None).await;
        dispatcher.config.inference_evaluation_node_hostname =
            Some("evaluation-worker".to_string());
        let manifest =
            dispatcher.inference_evaluation_job_manifest(&InferenceEvaluationExecutionRequest {
                evaluation_id: "infeval_test".into(),
                gateway_url: "http://pharness-model-gateway:4780/v1/".into(),
            });
        assert_eq!(
            manifest.pointer("/spec/template/spec/automountServiceAccountToken"),
            Some(&json!(false))
        );
        assert_eq!(
            manifest.pointer("/spec/template/spec/containers/0/command/0"),
            Some(&json!("pharness-eval"))
        );
        assert_eq!(
            manifest.pointer("/spec/template/spec/containers/0/image"),
            Some(&json!(dispatcher
                .config
                .inference_evaluation_image
                .as_str()))
        );
        assert_eq!(
            manifest.pointer(
                "/spec/template/metadata/labels/agentic.lucas.engineering~1inference-mode"
            ),
            Some(&json!("gateway"))
        );
        let env = manifest
            .pointer("/spec/template/spec/containers/0/env")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(env
            .iter()
            .any(|entry| entry["name"] == "PHARNESS_WORKER_TOKEN"));
        assert!(env.iter().any(|entry| {
            entry["name"] == "PHARNESS_MODEL_GRANT_SIGNER_ENABLED" && entry["value"] == "false"
        }));
        assert!(env.iter().any(|entry| {
            entry["name"] == "PHARNESS_EVAL_ARTIFACT_DIR" && entry["value"] == "/work/artifacts"
        }));
        assert_eq!(
            manifest.pointer("/spec/template/spec/affinity/nodeAffinity/requiredDuringSchedulingIgnoredDuringExecution/nodeSelectorTerms/0/matchExpressions/0/values/0"),
            Some(&json!("evaluation-worker"))
        );
        for forbidden in [
            "FIREWORKS_API_KEY",
            "OPENROUTER_API_KEY",
            "PHARNESS_MODEL_GRANT_HMAC_KEY",
            "PHARNESS_GIT_WRITER_TOKEN",
            "PHARNESS_SOURCE_READER_TOKEN",
        ] {
            assert!(env.iter().all(|entry| entry["name"] != forbidden));
        }
    }

    #[test]
    fn job_names_are_dns_safe_and_attempt_scoped() {
        let mut run = test_run();
        run.id = RunId::new("run_123");
        let initial = job_name(&run, None);
        assert_eq!(initial, "pharness-run-123-i");
        assert!(initial.len() <= 63);
        assert!(!initial.contains('_'));

        run.budget_consumption.extensions = 1;
        let budget_resume = job_name(&run, None);
        assert_eq!(budget_resume, "pharness-run-123-b01");
        assert_ne!(budget_resume, initial);
    }

    #[test]
    fn recognizes_onboarding_proposer_runs_for_parent_recovery() {
        let mut run = test_run();
        assert_eq!(onboarding_proposer_id(&run), None);
        run.execution_target_json = json!({
            "onboarding": {"onboarding_id": "ronb_test"}
        });
        assert_eq!(onboarding_proposer_id(&run), Some("ronb_test"));
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

    #[test]
    fn chained_run_waits_only_for_its_exact_predecessor_job() {
        let predecessor_label = job_label_value("run_predecessor");
        let jobs = json!({
            "items": [{
                "metadata": {"labels": {RUN_ID_LABEL: predecessor_label}},
                "status": {"active": 1}
            }]
        });

        assert_eq!(
            chained_run_capacity(&jobs, 1, &job_label_value("run_predecessor")).unwrap(),
            ChainedRunCapacity::WaitingForPredecessor { active: 1 }
        );
        assert!(chained_run_capacity(&jobs, 1, &job_label_value("run_unrelated")).is_err());
    }

    #[test]
    fn chained_run_never_ignores_unrelated_active_capacity() {
        let jobs = json!({
            "items": [
                {
                    "metadata": {"labels": {RUN_ID_LABEL: job_label_value("run_predecessor")}},
                    "status": {"active": 1}
                },
                {
                    "metadata": {"labels": {RUN_ID_LABEL: job_label_value("run_unrelated")}},
                    "status": {"active": 1}
                }
            ]
        });

        let error =
            chained_run_capacity(&jobs, 1, &job_label_value("run_predecessor")).unwrap_err();
        assert!(error.to_string().contains("not owned exclusively"));
    }

    #[test]
    fn chained_run_dispatches_after_predecessor_releases_capacity() {
        let jobs = json!({
            "items": [{
                "metadata": {"labels": {RUN_ID_LABEL: job_label_value("run_predecessor")}},
                "status": {"succeeded": 1}
            }]
        });

        assert_eq!(
            chained_run_capacity(&jobs, 1, &job_label_value("run_predecessor")).unwrap(),
            ChainedRunCapacity::Available
        );
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
        assert_eq!(
            manifest.pointer("/spec/template/spec/initContainers/0/name"),
            Some(&json!("network-policy-stabilization"))
        );
        assert_eq!(
            manifest.pointer("/spec/template/spec/initContainers/0/args/0"),
            Some(&json!(format!(
                "sleep {NETWORK_POLICY_STABILIZATION_SECONDS}"
            )))
        );
    }

    #[tokio::test]
    async fn repo_stage_manifests_reuse_workspace_and_isolate_test_writes() {
        let dispatcher = test_dispatcher(None).await;
        let mut tester = test_run();
        tester.execution_target_json = json!({
            "repo_mode": {
                "chain_authorization_id": "chain_test",
                "workspace_access": "ephemeral_copy"
            },
            "workspace_source": {
                "workspace_id": "ws_shared",
                "source_repo": "https://github.com/example/repo.git",
                "source_ref": "main",
                "source_commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "branch": "pharness/test",
                "resolved_commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }
        });
        let shared_claim = workspace_claim_name("ws_shared");
        assert_eq!(workspace_claim_name_for_run(&tester), shared_claim);
        let claim = dispatcher.workspace_claim_manifest(&tester);
        assert_eq!(
            claim.pointer("/metadata/labels/pharness.lucas.engineering~1workspace-id"),
            Some(&json!(job_label_value("ws_shared")))
        );
        assert!(claim
            .pointer("/metadata/labels/pharness.lucas.engineering~1run-id")
            .is_none());

        let manifest = dispatcher.job_manifest(&tester, None);
        assert_eq!(
            manifest.pointer("/spec/template/spec/volumes/0/emptyDir/sizeLimit"),
            Some(&json!("4Gi"))
        );
        assert_eq!(
            manifest.pointer("/spec/template/spec/volumes/1/persistentVolumeClaim/claimName"),
            Some(&json!(shared_claim))
        );
        assert_eq!(
            manifest.pointer("/spec/template/spec/volumes/1/persistentVolumeClaim/readOnly"),
            Some(&json!(true))
        );
        assert_eq!(
            manifest.pointer("/spec/template/spec/initContainers/1/name"),
            Some(&json!("copy-authorized-workspace"))
        );
        assert_eq!(
            manifest.pointer("/spec/template/spec/initContainers/1/args/0"),
            Some(&json!(
                "find /source-workspace -mindepth 1 -maxdepth 1 -exec cp -a --no-preserve=ownership,timestamps -t /workspace -- {} +"
            ))
        );

        let mut verifier = tester;
        verifier.execution_target_json["repo_mode"]["workspace_access"] = json!("read_only");
        let manifest = dispatcher.job_manifest(&verifier, None);
        assert_eq!(
            manifest.pointer("/spec/template/spec/containers/0/volumeMounts/0/readOnly"),
            Some(&json!(true))
        );
        assert_eq!(
            manifest.pointer("/spec/template/spec/volumes/0/persistentVolumeClaim/readOnly"),
            Some(&json!(true))
        );
    }

    #[tokio::test]
    async fn onboarding_proposer_reader_credential_is_init_only() {
        let mut dispatcher = test_dispatcher(None).await;
        dispatcher.config.source_reader_token_secret_name =
            Some("pharness-source-reader-token".into());
        let mut run = test_run();
        run.execution_target_json = json!({
            "agent_profile":{"id":"repository-onboarding-proposer"},
            "workspace_source":{
                "workspace_id":"onboarding-ronb-test",
                "source_repo":"https://github.com/example/test.git",
                "source_ref":"main",
                "source_commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "branch":"pharness/onboarding/ronb-test",
                "resolved_commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }
        });
        let manifest = dispatcher.job_manifest(&run, None);
        assert_eq!(
            manifest.pointer("/spec/template/spec/serviceAccountName"),
            Some(&json!("pharness-source-reader"))
        );
        assert_eq!(
            manifest.pointer("/spec/template/spec/automountServiceAccountToken"),
            Some(&json!(false))
        );
        assert_eq!(
            manifest.pointer("/spec/template/spec/initContainers/1/name"),
            Some(&json!("prepare-onboarding-source"))
        );
        let init_script = manifest
            .pointer("/spec/template/spec/initContainers/1/args/0")
            .and_then(serde_json::Value::as_str)
            .unwrap();
        assert!(init_script.contains("git -c safe.directory=\"$PHARNESS_WORKSPACE_DIR\""));
        assert!(!init_script.contains("git config --global"));
        let init_env = manifest
            .pointer("/spec/template/spec/initContainers/1/env")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(init_env
            .iter()
            .any(|entry| entry["name"] == "PHARNESS_SOURCE_READER_TOKEN"));
        let model_env = manifest
            .pointer("/spec/template/spec/containers/0/env")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(model_env
            .iter()
            .all(|entry| entry["name"] != "PHARNESS_SOURCE_READER_TOKEN"));
        assert_eq!(
            manifest.pointer(
                "/spec/template/metadata/labels/agentic.lucas.engineering~1onboarding-proposer"
            ),
            Some(&json!("true"))
        );
    }

    #[tokio::test]
    async fn hosted_preparation_recovers_lost_ack_and_observes_terminal_job_without_recreating_it()
    {
        let fixture = super::recovery::tests::KubectlFixture::new(false);
        let mut dispatcher = test_dispatcher(None).await;
        dispatcher.kubectl_bin = fixture.command.clone();
        let mut run = test_run();
        run.execution_target_json["hosted_workflow_policy_hash"] = json!("sha256:fixture");
        let profile = EnvironmentProfile {
            id: "python-3.11".into(),
            active: true,
            image: format!("example.test/python@sha256:{}", "a".repeat(64)),
            revision: "b".repeat(40),
            platform: "linux/amd64".into(),
            required_executables: vec![
                "pharness-worker".into(),
                "git".into(),
                "python".into(),
                "pip".into(),
            ],
            preparation_strategy: PreparationStrategy::PythonHashedRequirements,
            service_account: "pharness-python-runner".into(),
            repository_allowlist: vec!["https://github.com/example/test.git".into()],
            limits: EnvironmentProfileLimits {
                cpu: "1".into(),
                memory: "1Gi".into(),
                ephemeral_storage: "2Gi".into(),
            },
        };
        let first = dispatcher
            .create_environment_preparation_job(&run, &profile)
            .await
            .unwrap();
        let second = dispatcher
            .create_environment_preparation_job(&run, &profile)
            .await
            .unwrap();
        assert_eq!(first.job_name, second.job_name);
        assert_eq!(fixture.creates(), 1);
        let mut job: serde_json::Value =
            serde_json::from_slice(&std::fs::read(fixture.dir.join("job.json")).unwrap()).unwrap();
        job["status"] = json!({"conditions":[{"type":"Complete","status":"True"}]});
        std::fs::write(fixture.dir.join("job.json"), job.to_string()).unwrap();
        let worker = super::RunDispatcher::Kubernetes(std::sync::Arc::new(dispatcher));
        let observed = worker
            .observe_hosted_preparation(&run, &profile)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(observed.status, "completed");
        assert_eq!(observed.job_name, first.job_name);
        assert_eq!(fixture.creates(), 1);
        job["spec"]["template"]["spec"]["containers"][0]["image"] = json!("changed-image");
        std::fs::write(fixture.dir.join("job.json"), job.to_string()).unwrap();
        assert!(worker
            .observe_hosted_preparation(&run, &profile)
            .await
            .is_err());
        assert_eq!(fixture.creates(), 1);
    }

    #[tokio::test]
    async fn preparation_manifest_uses_only_the_preparation_proxy() {
        let mut dispatcher = test_dispatcher(None).await;
        dispatcher.config.source_reader_token_secret_name = Some("source-reader-token".into());
        let profile = EnvironmentProfile {
            id: "python-3.11".to_string(),
            active: true,
            image: format!("example.test/python@sha256:{}", "a".repeat(64)),
            revision: "b".repeat(40),
            platform: "linux/amd64".to_string(),
            required_executables: vec![
                "pharness-worker".to_string(),
                "git".to_string(),
                "python".to_string(),
                "pip".to_string(),
            ],
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
        assert!(env
            .iter()
            .any(|entry| entry["name"] == "PHARNESS_SOURCE_READER_TOKEN"));
        assert!(env
            .iter()
            .all(|entry| entry["name"] != "PHARNESS_GIT_WRITER_TOKEN"));
        assert_eq!(
            manifest.pointer("/spec/template/spec/initContainers/0/image"),
            Some(&json!(profile.image))
        );
        assert_eq!(
            manifest.pointer("/spec/template/spec/initContainers/0/args/0"),
            Some(&json!(format!(
                "sleep {NETWORK_POLICY_STABILIZATION_SECONDS}"
            )))
        );
    }

    #[tokio::test]
    async fn discovery_manifest_uses_the_isolated_reader_without_model_or_writer_credentials() {
        let dispatcher = test_dispatcher(None).await;
        let manifest = dispatcher.repository_discovery_job_manifest(&RepositoryDiscoveryRequest {
            discovery_id: "rdisc_test".to_string(),
        });
        assert_eq!(
            manifest.pointer("/spec/template/spec/serviceAccountName"),
            Some(&json!("pharness-source-reader"))
        );
        assert_eq!(
            manifest.pointer("/spec/template/spec/automountServiceAccountToken"),
            Some(&json!(false))
        );
        assert_eq!(
            manifest.pointer("/spec/template/spec/volumes/0/emptyDir/sizeLimit"),
            Some(&json!("2Gi"))
        );
        let env = manifest
            .pointer("/spec/template/spec/containers/0/env")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(env.iter().all(|entry| entry["name"] != "FIREWORKS_API_KEY"));
        assert!(env
            .iter()
            .all(|entry| entry["name"] != "PHARNESS_GIT_WRITER_TOKEN"));
        assert!(env
            .iter()
            .any(|entry| entry["name"] == "PHARNESS_WORKER_TOKEN"));
        assert!(env.iter().any(|entry| {
            entry["name"] == "HTTPS_PROXY"
                && entry["value"]
                    .as_str()
                    .is_some_and(|value| value.contains("pharness-preparation-egress-proxy"))
        }));
    }

    #[tokio::test]
    async fn onboarding_patch_manifest_has_reader_but_no_model_or_writer_credentials() {
        let mut dispatcher = test_dispatcher(None).await;
        dispatcher.config.source_reader_token_secret_name = Some("source-reader-token".into());
        let manifest = dispatcher.onboarding_patch_job_manifest(&OnboardingPatchRequest {
            onboarding_id: "ronb_test".into(),
            execution_id: "onbpatch_test".into(),
        });
        assert_eq!(
            manifest.pointer("/spec/template/spec/serviceAccountName"),
            Some(&json!("pharness-source-reader"))
        );
        assert_eq!(
            manifest.pointer("/spec/template/spec/automountServiceAccountToken"),
            Some(&json!(false))
        );
        assert_eq!(
            manifest.pointer("/spec/template/spec/volumes/0/emptyDir/sizeLimit"),
            Some(&json!("2Gi"))
        );
        let env = manifest
            .pointer("/spec/template/spec/containers/0/env")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(env
            .iter()
            .any(|entry| entry["name"] == "PHARNESS_SOURCE_READER_TOKEN"));
        assert!(env.iter().all(|entry| entry["name"] != "FIREWORKS_API_KEY"));
        assert!(env
            .iter()
            .all(|entry| entry["name"] != "PHARNESS_GIT_WRITER_TOKEN"));
        assert!(env.iter().any(|entry| {
            entry["name"] == "HTTPS_PROXY"
                && entry["value"]
                    .as_str()
                    .is_some_and(|value| value.contains("pharness-preparation-egress-proxy"))
        }));
        assert_eq!(
            manifest.pointer("/spec/template/spec/containers/0/env/0/value"),
            Some(&json!("onboarding_patch"))
        );
    }

    #[tokio::test]
    async fn onboarding_validation_manifest_is_an_isolated_exact_source_reader() {
        let mut dispatcher = test_dispatcher(None).await;
        dispatcher.config.source_reader_token_secret_name = Some("source-reader-token".into());
        let manifest = dispatcher.onboarding_contract_validation_job_manifest(
            &OnboardingContractValidationRequest {
                onboarding_id: "ronb_test".into(),
                execution_id: "onbvalidate_test".into(),
            },
        );
        assert_eq!(
            manifest.pointer("/spec/template/spec/serviceAccountName"),
            Some(&json!("pharness-source-reader"))
        );
        let env = manifest
            .pointer("/spec/template/spec/containers/0/env")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(env
            .iter()
            .any(|entry| entry["name"] == "PHARNESS_SOURCE_READER_TOKEN"));
        assert!(env.iter().all(|entry| entry["name"] != "FIREWORKS_API_KEY"));
        assert!(env
            .iter()
            .all(|entry| entry["name"] != "PHARNESS_GIT_WRITER_TOKEN"));
        assert!(env.iter().any(|entry| {
            entry["name"] == "HTTPS_PROXY"
                && entry["value"]
                    .as_str()
                    .is_some_and(|value| value.contains("pharness-preparation-egress-proxy"))
        }));
        assert!(env.iter().any(|entry| {
            entry["name"] == "PHARNESS_EXECUTION_KIND"
                && entry["value"] == "onboarding_contract_validate"
        }));
    }

    #[tokio::test]
    async fn repository_readiness_uses_pinned_runner_without_model_or_writer_credentials() {
        let mut dispatcher = test_dispatcher(None).await;
        dispatcher.config.source_reader_token_secret_name = Some("source-reader-token".into());
        let profile = EnvironmentProfile {
            id: "python-3.11".into(),
            active: true,
            image: format!("example.test/python@sha256:{}", "a".repeat(64)),
            revision: "b".repeat(40),
            platform: "linux/amd64".into(),
            required_executables: vec![
                "pharness-worker".into(),
                "git".into(),
                "python".into(),
                "pip".into(),
            ],
            preparation_strategy: PreparationStrategy::PythonHashedRequirements,
            service_account: "pharness-python-runner".into(),
            repository_allowlist: vec!["https://github.com/example/test.git".into()],
            limits: EnvironmentProfileLimits {
                cpu: "1".into(),
                memory: "1Gi".into(),
                ephemeral_storage: "2Gi".into(),
            },
        };
        let manifest =
            dispatcher.repository_readiness_job_manifest(&RepositoryReadinessExecutionRequest {
                preparation_id: "sprep_test".into(),
                profile: profile.clone(),
            });
        assert_eq!(
            manifest.pointer("/spec/template/spec/containers/0/image"),
            Some(&json!(profile.image))
        );
        assert_eq!(
            manifest.pointer("/spec/template/spec/serviceAccountName"),
            Some(&json!(profile.service_account))
        );
        let env = manifest
            .pointer("/spec/template/spec/containers/0/env")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(env
            .iter()
            .any(|entry| entry["name"] == "PHARNESS_SOURCE_READER_TOKEN"));
        assert!(env.iter().all(|entry| entry["name"] != "FIREWORKS_API_KEY"));
        assert!(env
            .iter()
            .all(|entry| entry["name"] != "PHARNESS_GIT_WRITER_TOKEN"));
        assert!(env.iter().any(|entry| {
            entry["name"] == "HTTPS_PROXY"
                && entry["value"]
                    .as_str()
                    .is_some_and(|value| value.contains("preparation-egress-proxy"))
        }));
    }

    #[tokio::test]
    async fn proxy_capability_preflights_wait_for_network_policy_convergence() {
        let dispatcher = test_dispatcher(None).await;
        let (_, _, _, source_manifest) = dispatcher
            .capability_preflight_manifest(
                "source_workspace",
                Some("https://github.com/example/test.git"),
            )
            .unwrap();
        assert_eq!(
            source_manifest.pointer("/spec/activeDeadlineSeconds"),
            Some(&json!(75 + NETWORK_POLICY_STABILIZATION_SECONDS))
        );
        assert_eq!(
            source_manifest.pointer("/spec/template/spec/initContainers/0/name"),
            Some(&json!("network-policy-stabilization"))
        );
        let (principal, permission, repository, reader_manifest) = dispatcher
            .capability_preflight_manifest(
                "source_reader",
                Some("https://github.com/example/test.git"),
            )
            .unwrap();
        assert_eq!(
            principal,
            "system:serviceaccount:pharness:pharness-source-reader"
        );
        assert_eq!(permission, "repository_read");
        assert_eq!(
            repository.as_deref(),
            Some("https://github.com/example/test.git")
        );
        assert_eq!(
            reader_manifest.pointer("/spec/template/spec/serviceAccountName"),
            Some(&json!("pharness-source-reader"))
        );
        assert_eq!(
            reader_manifest.pointer("/spec/activeDeadlineSeconds"),
            Some(&json!(75 + NETWORK_POLICY_STABILIZATION_SECONDS))
        );
        let reader_env = reader_manifest
            .pointer("/spec/template/spec/containers/0/env")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(reader_env.iter().all(|entry| {
            !matches!(
                entry.get("name").and_then(serde_json::Value::as_str),
                Some("FIREWORKS_API_KEY" | "GITHUB_TOKEN")
            )
        }));
        let reader_script = reader_manifest
            .pointer("/spec/template/spec/containers/0/args/0")
            .and_then(serde_json::Value::as_str)
            .unwrap();
        assert!(reader_script.contains("git ls-remote"));
        assert!(reader_script.contains("GIT_ASKPASS_VALUE=/bin/false"));

        let (_, _, _, writer_manifest) = dispatcher
            .capability_preflight_manifest(
                "source_writer",
                Some("https://github.com/example/test.git"),
            )
            .unwrap();
        assert_eq!(
            writer_manifest.pointer("/spec/activeDeadlineSeconds"),
            Some(&json!(75))
        );
        assert!(writer_manifest
            .pointer("/spec/template/spec/initContainers")
            .is_none());
    }

    #[tokio::test]
    async fn writer_capability_preflight_exercises_git_transport_without_mutating_a_ref() {
        let mut dispatcher = test_dispatcher(None).await;
        dispatcher.config.gitops_writer_enabled = true;
        dispatcher.config.gitops_writer_token_secret_name =
            Some("pharness-gitops-writer-token".to_string());
        dispatcher.config.gitops_writer_allowed_repos =
            vec!["https://github.com/example/test.git".to_string()];
        let (_, permission, repository, manifest) = dispatcher
            .capability_preflight_manifest(
                "gitops_writer",
                Some("https://github.com/example/test.git"),
            )
            .unwrap();
        let script = manifest
            .pointer("/spec/template/spec/containers/0/args/0")
            .and_then(serde_json::Value::as_str)
            .unwrap();
        let env = manifest
            .pointer("/spec/template/spec/containers/0/env")
            .and_then(serde_json::Value::as_array)
            .unwrap();

        assert_eq!(permission, "repository_push");
        assert_eq!(
            repository.as_deref(),
            Some("https://github.com/example/test.git")
        );
        assert!(script.contains("push --dry-run"));
        assert!(script.contains("GIT_ASKPASS=/tmp/askpass"));
        assert!(!script.contains("https://x-access-token:"));
        assert!(env.iter().any(|entry| {
            entry["name"] == "PREFLIGHT_REF"
                && entry["value"].as_str().is_some_and(|value| {
                    value.starts_with("refs/heads/pharness/capability-preflight-")
                })
        }));
    }

    #[tokio::test]
    async fn source_capability_preflight_honors_the_exact_requested_repository() {
        let mut dispatcher = test_dispatcher(None).await;
        let first = "https://github.com/example/first.git".to_string();
        let second = "https://github.com/example/second.git".to_string();
        dispatcher.config.git_writer_allowed_repos = vec![first.clone(), second.clone()];
        dispatcher.config.git_observer_enabled = true;
        dispatcher.config.git_observer_token_secret_name =
            Some("pharness-git-observer-token".to_string());
        dispatcher.config.git_observer_allowed_repos = vec![first, second.clone()];

        for capability in ["source_writer", "source_observer"] {
            let (_, _, repository, manifest) = dispatcher
                .capability_preflight_manifest(capability, Some(&second))
                .unwrap();
            assert_eq!(repository.as_deref(), Some(second.as_str()));
            let env = manifest
                .pointer("/spec/template/spec/containers/0/env")
                .and_then(serde_json::Value::as_array)
                .unwrap();
            assert!(env.iter().any(|entry| {
                entry["name"] == "REPOSITORY" && entry["value"] == second.as_str()
            }));
            assert!(env.iter().any(|entry| {
                entry["name"] == "REPOSITORY_API"
                    && entry["value"] == "https://api.github.com/repos/example/second"
            }));
        }

        let error = dispatcher
            .capability_preflight_manifest(
                "source_writer",
                Some("https://github.com/example/not-allowed.git"),
            )
            .unwrap_err();
        assert!(error.to_string().contains("repository is not allowlisted"));
    }

    #[tokio::test]
    async fn source_observer_preflight_exercises_rules_checks_and_statuses() {
        let mut dispatcher = test_dispatcher(None).await;
        dispatcher.config.git_observer_enabled = true;
        dispatcher.config.git_observer_token_secret_name =
            Some("pharness-git-observer-token".to_string());
        dispatcher.config.git_observer_allowed_repos =
            vec!["https://github.com/example/test.git".to_string()];
        let (_, permission, repository, manifest) = dispatcher
            .capability_preflight_manifest("source_observer", None)
            .unwrap();
        let script = manifest
            .pointer("/spec/template/spec/containers/0/args/0")
            .and_then(serde_json::Value::as_str)
            .unwrap();
        let env = manifest
            .pointer("/spec/template/spec/containers/0/env")
            .and_then(serde_json::Value::as_array)
            .unwrap();

        assert_eq!(permission, "repository_pull_rules_checks_statuses");
        assert_eq!(
            repository.as_deref(),
            Some("https://github.com/example/test.git")
        );
        assert_eq!(script, "exec /usr/local/bin/pharness-worker");
        assert!(env.iter().any(|entry| {
            entry["name"] == "PHARNESS_EXECUTION_KIND"
                && entry["value"] == "source_observer_capability_preflight"
        }));
        assert!(env.iter().any(|entry| {
            entry["name"] == "GITHUB_API_URL" && entry["value"] == "https://api.github.com"
        }));
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
        assert!(env
            .iter()
            .any(|entry| entry["name"] == "TMPDIR" && entry["value"] == "/work"));
        assert!(env.iter().all(|entry| entry["name"] != "FIREWORKS_API_KEY"));
        assert_eq!(
            manifest.pointer("/spec/template/spec/serviceAccountName"),
            Some(&json!("pharness-git-writer"))
        );
        assert_eq!(manifest.pointer("/spec/backoffLimit"), Some(&json!(0)));
        assert_eq!(
            manifest.pointer("/spec/template/spec/initContainers/0/name"),
            Some(&json!("network-policy-stabilization"))
        );
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
        assert!(env
            .iter()
            .any(|entry| entry["name"] == "TMPDIR" && entry["value"] == "/work"));
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
            Some(&json!(600 + NETWORK_POLICY_STABILIZATION_SECONDS))
        );
    }

    #[tokio::test]
    async fn simultaneous_legacy_and_hosted_dispatch_share_one_worker_slot() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "pharness-admission-{}",
            uuid::Uuid::now_v7().simple()
        ));
        std::fs::create_dir(&root).unwrap();
        let kubectl = root.join("kubectl");
        // Capture an empty capacity response before delaying it. Without an
        // admission lock, both callers observe zero Jobs and create two.
        let script = format!(
            r#"#!/usr/bin/env python3
import json,pathlib,sys,time
root=pathlib.Path({root:?})
args=sys.argv[1:]
if args[:2]==['get','jobs']:
    jobs=[json.loads(p.read_text()) for p in root.glob('job-*.json')]
    time.sleep(.1)
    print(json.dumps({{'items':jobs}}))
elif args[:2]==['get','job']:
    p=root/('job-'+args[2]+'.json')
    if p.exists():print(p.read_text())
elif args[0]=='get':
    print('persistentvolumeclaim/already-provisioned')
elif args[0]=='create':
    job=json.load(sys.stdin)
    assert job['kind']=='Job'
    job['status']={{'active':1}}
    (root/('job-'+job['metadata']['name']+'.json')).write_text(json.dumps(job))
    with (root/'creates').open('a') as f:f.write('x')
else:sys.exit(99)
"#,
            root = root.to_str().unwrap()
        );
        std::fs::write(&kubectl, script).unwrap();
        std::fs::set_permissions(&kubectl, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut dispatcher = test_dispatcher(None).await;
        dispatcher.kubectl_bin = kubectl.to_str().unwrap().into();
        dispatcher.config.max_concurrent_run_jobs = 1;
        let mut legacy = test_run();
        legacy.id = RunId::new("run_legacy_admission");
        let mut hosted = test_run();
        hosted.id = RunId::new("run_hosted_admission");
        hosted.execution_target_json["hosted_workflow_policy_hash"] = json!("sha256:fixture");
        let (a, b) = tokio::join!(
            dispatcher.create_job(&legacy, None, None),
            dispatcher.create_job(&hosted, None, None)
        );
        assert_ne!(
            a.is_ok(),
            b.is_ok(),
            "exactly one Run should be admitted: {a:?}, {b:?}"
        );
        assert_eq!(std::fs::read(root.join("creates")).unwrap().len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    async fn test_dispatcher(node_hostname: Option<String>) -> KubernetesJobDispatcher {
        KubernetesJobDispatcher {
            store: Arc::new(SqliteStore::connect_in_memory().await.unwrap()),
            kubectl_bin: "kubectl".to_string(),
            config: WorkerKubernetesConfig {
                namespace: "pharness".to_string(),
                image: "example.test/pharness:latest".to_string(),
                inference_evaluation_image: "example.test/pharness-eval:latest".to_string(),
                inference_evaluation_node_hostname: None,
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
                source_reader_enabled: true,
                source_reader_service_account: "pharness-source-reader".to_string(),
                source_reader_token_secret_name: None,
                source_reader_allowed_repos: vec!["https://github.com/example/test.git".to_string()],
                source_reader_active_deadline_seconds: 600,
                source_reader_ttl_seconds_after_finished: 3600,
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

    #[tokio::test]
    async fn gateway_bound_coding_job_has_no_upstream_provider_credential() {
        let dispatcher = test_dispatcher(None).await;
        let mut run = test_run();
        run.execution_target_json = json!({
            "inference": {
                "mode": "gateway",
                "selection_id": "infsel_test",
            }
        });
        let manifest = dispatcher.job_manifest(&run, None);
        let env = manifest
            .pointer("/spec/template/spec/containers/0/env")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(env.iter().all(|entry| entry["name"] != "FIREWORKS_API_KEY"));
        assert!(env
            .iter()
            .any(|entry| entry["name"] == "PHARNESS_WORKER_TOKEN"));
        assert_eq!(
            manifest.pointer(
                "/spec/template/metadata/labels/agentic.lucas.engineering~1inference-mode"
            ),
            Some(&json!("gateway"))
        );
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
            retention_state: "retained".into(),
            sealed_summary: None,
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
