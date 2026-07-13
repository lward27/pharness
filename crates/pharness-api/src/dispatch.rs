//! Run dispatch across execution targets.
//!
//! `RunDispatcher` decides where a run attempt executes: in-process through
//! the existing local worker, or in an isolated Kubernetes Job per attempt.
//! Job orchestration shells out to kubectl with the pod service account,
//! matching how the typed read-only cluster capabilities already execute.

use crate::worker::{fail_run_from_dispatch, LocalWorker};
use pharness_config::WorkerKubernetesConfig;
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
const TEKTON_EXECUTOR_JOB_NAME_VALUE: &str = "pharness-tekton-executor";
const PIPELINE_INTENT_LABEL: &str = "pharness.lucas.engineering/pipeline-intent";
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
            }),
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
        });
    }

    async fn create_job(
        &self,
        run: &StoredRun,
        approval: Option<&StoredApproval>,
    ) -> anyhow::Result<()> {
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
        let mut env = vec![
            serde_json::json!({ "name": "PHARNESS_API_URL", "value": self.config.api_url }),
            serde_json::json!({ "name": "PHARNESS_RUN_ID", "value": run.id.as_str() }),
            serde_json::json!({ "name": "HOME", "value": self.config.workspace_dir }),
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

        serde_json::json!({
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
                "activeDeadlineSeconds": self.config.active_deadline_seconds,
                "ttlSecondsAfterFinished": self.config.ttl_seconds_after_finished,
                "template": {
                    "metadata": {
                        "labels": {
                            JOB_NAME_LABEL: JOB_NAME_VALUE,
                            RUN_ID_LABEL: job_label_value(run.id.as_str()),
                        },
                    },
                    "spec": {
                        "serviceAccountName": self.config.service_account,
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
                            "image": self.config.image,
                            "imagePullPolicy": "Always",
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
                                "requests": { "cpu": "100m", "memory": "256Mi" },
                                "limits": { "cpu": "1", "memory": "1Gi" },
                            },
                        }],
                        "volumes": [
                            { "name": "workspace", "emptyDir": {} },
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
        self.reap_tekton_executor_jobs().await
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

fn tekton_executor_job_name(execution_id: &str) -> String {
    let digest = Sha256::digest(execution_id.as_bytes());
    let suffix = digest
        .iter()
        .take(9)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("pharness-tekton-{suffix}")
}

#[cfg(test)]
mod tests {
    use super::{
        executor_job_terminal_state, job_label_value, job_name, run_label_to_run_id,
        ExecutorJobTerminalState,
    };
    use serde_json::json;

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
}
