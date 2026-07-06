//! Run dispatch across execution targets.
//!
//! `RunDispatcher` decides where a run attempt executes: in-process through
//! the existing local worker, or in an isolated Kubernetes Job per attempt.
//! Job orchestration shells out to kubectl with the pod service account,
//! matching how the typed read-only cluster capabilities already execute.

use crate::worker::{fail_run_from_dispatch, LocalWorker};
use pharness_config::WorkerKubernetesConfig;
use pharness_store::{SqliteStore, StoredApproval, StoredRun};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;

const REAPER_INTERVAL: Duration = Duration::from_secs(30);
pub(crate) const RUN_ID_LABEL: &str = "pharness.lucas.engineering/run-id";
const JOB_NAME_LABEL: &str = "app.kubernetes.io/name";
const JOB_NAME_VALUE: &str = "pharness-run";

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

    /// Mark runs failed when their worker Job failed or disappeared without
    /// reporting a durable terminal state.
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

#[cfg(test)]
mod tests {
    use super::{job_label_value, job_name, run_label_to_run_id};

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
}
