#![forbid(unsafe_code)]

//! One-attempt worker binary for cluster execution targets.
//!
//! The worker executes exactly one run attempt (initial or resume) against
//! the pharness API, which stays the sole store writer. The process exits 0
//! when the attempt reached a durable terminal or approval-paused state, and
//! non-zero only when the attempt could not be reported back to the API.

use anyhow::Context;
use pharness_config::ApiRuntimeConfig;
use pharness_core::{
    AgentAction, AgentEvent, CancellationFlag, ReadOnlyClusterTools, ToolExecutor,
};
use pharness_fireworks::{FireworksClient, FireworksProviderConfig};
use pharness_runhost::{execute_attempt, AttemptBackend, AttemptHost, AttemptOutcome, AttemptSpec};
use std::sync::Arc;
use std::time::Duration;
use tracing_subscriber::EnvFilter;

const CONTROL_POLL_INTERVAL: Duration = Duration::from_secs(2);
const INGEST_ATTEMPTS: u32 = 5;
const INGEST_RETRY_DELAY: Duration = Duration::from_secs(2);
// Fresh pods can see transient connection refusals until the CNI's network
// policy state includes the new pod; the startup fetch must ride that out.
const CONTEXT_FETCH_ATTEMPTS: u32 = 5;
const CONTEXT_FETCH_RETRY_DELAY: Duration = Duration::from_secs(2);
const DEFAULT_TEKTON_EXECUTOR_POLL_SECONDS: u64 = 5;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing()?;

    if std::env::var("PHARNESS_EXECUTION_KIND").ok().as_deref() == Some("tekton_trigger") {
        return execute_tekton_trigger().await;
    }

    let env = WorkerEnv::from_env()?;
    let config = ApiRuntimeConfig::load_from_env()?;
    let api_key = config
        .model
        .api_key
        .clone()
        .context("FIREWORKS_API_KEY is required for the worker attempt")?;
    let provider = FireworksClient::new(
        api_key,
        FireworksProviderConfig {
            base_url: config.model.base_url.clone(),
            model: config.model.model.clone(),
        },
    )?;
    let host = AttemptHost {
        provider,
        cluster_tools: config.cluster_tools(),
        default_policy: config.policy.clone(),
    };

    let backend = Arc::new(HttpAttemptBackend::new(
        env.api_url.clone(),
        env.run_id.clone(),
        env.worker_token.clone(),
    )?);

    let spec = fetch_attempt_spec_with_retry(&backend, env.approval_id.as_deref())
        .await
        .context("failed to fetch attempt context from api")?;

    prepare_workspace(&spec).await?;

    let cancellation = CancellationFlag::default();
    let control = tokio::spawn(poll_control(backend.clone(), cancellation.clone()));

    tracing::info!(
        run_id = %env.run_id,
        resume = spec.resume.is_some(),
        cwd = %spec.run.cwd,
        "starting run attempt"
    );

    let result = execute_attempt(host, backend.clone(), spec, cancellation).await;
    control.abort();

    match result {
        Ok(()) => {
            tracing::info!(run_id = %env.run_id, "attempt reported durable state");
            Ok(())
        }
        Err(error) => {
            tracing::error!(run_id = %env.run_id, %error, "attempt failed; reporting failure");
            backend
                .finish(AttemptOutcome::failed(error.to_string()))
                .await
                .context("failed to report attempt failure to api")?;
            Ok(())
        }
    }
}

/// Submit and observe one prevalidated PipelineRun. This mode deliberately
/// does not load model credentials or run an agent loop.
async fn execute_tekton_trigger() -> anyhow::Result<()> {
    let api_url = required_env("PHARNESS_API_URL")?
        .trim_end_matches('/')
        .to_string();
    let pipeline_intent_id = required_env("PHARNESS_PIPELINE_INTENT_ID")?;
    let execution_id = required_env("PHARNESS_EXECUTION_ID")?;
    let worker_token = required_env("PHARNESS_WORKER_TOKEN")?;
    let poll_interval = tekton_poll_interval()?;
    let manifest_text = required_env("PHARNESS_TEKTON_PIPELINERUN_JSON")?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text)
        .context("PHARNESS_TEKTON_PIPELINERUN_JSON must be valid JSON")?;
    let namespace = manifest
        .pointer("/metadata/namespace")
        .and_then(serde_json::Value::as_str)
        .context("PipelineRun manifest metadata.namespace is required")?
        .to_string();
    let name = manifest
        .pointer("/metadata/name")
        .and_then(serde_json::Value::as_str)
        .context("PipelineRun manifest metadata.name is required")?
        .to_string();

    match submit_pipeline_run(&manifest_text).await {
        Ok(()) => {
            post_tekton_outcome_with_retry(
                &api_url,
                &pipeline_intent_id,
                &worker_token,
                &serde_json::json!({
                    "execution_id": execution_id,
                    "status": "submitted",
                    "pipeline_run_namespace": namespace,
                    "pipeline_run_name": name,
                    "error": null,
                }),
            )
            .await
            .context("failed to report submitted PipelineRun to api")?;

            let outcome = match wait_for_pipeline_run(&namespace, &name, poll_interval).await {
                Ok(PipelineRunTerminal::Succeeded) => {
                    terminal_tekton_outcome(&execution_id, "completed", &namespace, &name, None)
                        .await
                }
                Ok(PipelineRunTerminal::Failed(reason)) => {
                    terminal_tekton_outcome(
                        &execution_id,
                        "failed",
                        &namespace,
                        &name,
                        Some(reason),
                    )
                    .await
                }
                Err(error) => {
                    tracing::error!(pipeline_intent_id = %pipeline_intent_id, %error, "Tekton PipelineRun observation failed");
                    serde_json::json!({
                        "execution_id": execution_id,
                        "status": "failed",
                        "pipeline_run_namespace": namespace,
                        "pipeline_run_name": name,
                        "error": "unable to observe PipelineRun to terminal state",
                    })
                }
            };
            post_tekton_outcome_with_retry(&api_url, &pipeline_intent_id, &worker_token, &outcome)
                .await
                .context("failed to report terminal PipelineRun outcome to api")
        }
        Err(error) => {
            tracing::error!(pipeline_intent_id = %pipeline_intent_id, %error, "Tekton execution failed");
            post_tekton_outcome_with_retry(
                &api_url,
                &pipeline_intent_id,
                &worker_token,
                &serde_json::json!({
                    "execution_id": execution_id,
                    "status": "failed",
                    "pipeline_run_namespace": namespace,
                    "pipeline_run_name": name,
                    "error": "unable to create PipelineRun",
                }),
            )
            .await
            .context("failed to report PipelineRun creation failure to api")
        }
    }
}

async fn terminal_tekton_outcome(
    execution_id: &str,
    status: &str,
    namespace: &str,
    name: &str,
    error: Option<String>,
) -> serde_json::Value {
    let mut outcome = serde_json::json!({
        "execution_id": execution_id,
        "status": status,
        "pipeline_run_namespace": namespace,
        "pipeline_run_name": name,
        "error": error,
    });

    match analyze_terminal_pipeline_run(namespace, name).await {
        Ok(analysis) => outcome["pipeline_run_analysis"] = analysis,
        Err(error) => {
            tracing::warn!(namespace, name, %error, "terminal PipelineRun analysis was not persisted");
            outcome["analysis_error"] = serde_json::Value::String(
                "unable to collect bounded PipelineRun analysis".to_string(),
            );
        }
    }

    outcome
}

async fn analyze_terminal_pipeline_run(
    namespace: &str,
    name: &str,
) -> anyhow::Result<serde_json::Value> {
    let tools = ReadOnlyClusterTools::from_env().without_related_resource_lookups();
    let result = tools
        .execute(&AgentAction::TektonAnalyzePipelineRun {
            id: "executor.pipeline_run_analysis".into(),
            reason: "persist terminal execution evidence".to_string(),
            namespace: namespace.to_string(),
            name: name.to_string(),
        })
        .await
        .context("failed to collect terminal PipelineRun analysis")?;
    result
        .content
        .get("analysis")
        .cloned()
        .context("terminal PipelineRun analysis result was missing analysis data")
}

#[derive(Debug, PartialEq, Eq)]
enum PipelineRunTerminal {
    Succeeded,
    Failed(String),
}

fn tekton_poll_interval() -> anyhow::Result<Duration> {
    let seconds = std::env::var("PHARNESS_TEKTON_EXECUTOR_POLL_SECONDS")
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()
        .context("PHARNESS_TEKTON_EXECUTOR_POLL_SECONDS must be an integer")?
        .unwrap_or(DEFAULT_TEKTON_EXECUTOR_POLL_SECONDS);
    if seconds == 0 {
        anyhow::bail!("PHARNESS_TEKTON_EXECUTOR_POLL_SECONDS must be greater than zero");
    }
    Ok(Duration::from_secs(seconds))
}

async fn wait_for_pipeline_run(
    namespace: &str,
    name: &str,
    poll_interval: Duration,
) -> anyhow::Result<PipelineRunTerminal> {
    loop {
        let output = tokio::process::Command::new("kubectl")
            .args(["get", "pipelinerun", name, "-n", namespace, "-o", "json"])
            .output()
            .await
            .context("failed to spawn kubectl while observing PipelineRun")?;
        if !output.status.success() {
            anyhow::bail!("kubectl could not read the submitted PipelineRun");
        }
        let pipeline_run: serde_json::Value = serde_json::from_slice(&output.stdout)
            .context("kubectl returned invalid PipelineRun JSON")?;
        if let Some(terminal) = pipeline_run_terminal(&pipeline_run) {
            return Ok(terminal);
        }
        tokio::time::sleep(poll_interval).await;
    }
}

fn pipeline_run_terminal(pipeline_run: &serde_json::Value) -> Option<PipelineRunTerminal> {
    let condition = pipeline_run
        .pointer("/status/conditions")
        .and_then(serde_json::Value::as_array)?
        .iter()
        .find(|condition| {
            condition.get("type").and_then(serde_json::Value::as_str) == Some("Succeeded")
        })?;
    match condition.get("status").and_then(serde_json::Value::as_str) {
        Some("True") => Some(PipelineRunTerminal::Succeeded),
        Some("False") => {
            let reason = condition
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("PipelineRunFailed");
            Some(PipelineRunTerminal::Failed(format!(
                "PipelineRun completed unsuccessfully: {reason}"
            )))
        }
        _ => None,
    }
}

async fn submit_pipeline_run(manifest: &str) -> anyhow::Result<()> {
    let mut child = tokio::process::Command::new("kubectl")
        .args(["create", "-f", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn kubectl for PipelineRun")?;
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(manifest.as_bytes()).await?;
    }
    let output = child.wait_with_output().await?;
    if output.status.success() {
        return Ok(());
    }
    anyhow::bail!(
        "kubectl create PipelineRun failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

async fn post_tekton_outcome_with_retry(
    api_url: &str,
    pipeline_intent_id: &str,
    token: &str,
    outcome: &serde_json::Value,
) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build Tekton executor http client")?;
    let url =
        format!("{api_url}/api/internal/pipeline-intents/{pipeline_intent_id}/execution-outcome");
    let mut last_error = None;
    for attempt in 1..=INGEST_ATTEMPTS {
        match client
            .post(&url)
            .bearer_auth(token)
            .json(outcome)
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => return Ok(()),
            Ok(response) if response.status().is_client_error() => {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                anyhow::bail!("{url} rejected execution outcome: {status} {body}");
            }
            Ok(response) => {
                last_error = Some(anyhow::anyhow!("{url} returned {}", response.status()))
            }
            Err(error) => last_error = Some(error.into()),
        }
        if attempt < INGEST_ATTEMPTS {
            tokio::time::sleep(INGEST_RETRY_DELAY).await;
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("{url} failed")))
}

struct WorkerEnv {
    api_url: String,
    run_id: String,
    approval_id: Option<String>,
    worker_token: String,
}

impl WorkerEnv {
    fn from_env() -> anyhow::Result<Self> {
        let api_url = required_env("PHARNESS_API_URL")?;
        let run_id = required_env("PHARNESS_RUN_ID")?;
        let worker_token = required_env("PHARNESS_WORKER_TOKEN")?;
        let approval_id = std::env::var("PHARNESS_APPROVAL_ID")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        Ok(Self {
            api_url: api_url.trim_end_matches('/').to_string(),
            run_id,
            approval_id,
            worker_token,
        })
    }
}

fn required_env(name: &str) -> anyhow::Result<String> {
    let value = std::env::var(name).with_context(|| format!("{name} is required"))?;
    let value = value.trim().to_string();
    if value.is_empty() {
        anyhow::bail!("{name} must not be empty");
    }
    Ok(value)
}

/// Ensure the attempt workspace exists. When `PHARNESS_WORKSPACE_REPO` is set
/// and the workspace is empty, clone that repo (optionally at
/// `PHARNESS_WORKSPACE_BRANCH`) before the attempt starts.
async fn prepare_workspace(spec: &AttemptSpec) -> anyhow::Result<()> {
    let cwd = std::path::PathBuf::from(&spec.run.cwd);
    tokio::fs::create_dir_all(&cwd)
        .await
        .with_context(|| format!("failed to create workspace {}", cwd.display()))?;

    let Some(repo) = std::env::var("PHARNESS_WORKSPACE_REPO")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };

    let mut entries = tokio::fs::read_dir(&cwd).await?;
    if entries.next_entry().await?.is_some() {
        tracing::info!(cwd = %cwd.display(), "workspace not empty; skipping clone");
        return Ok(());
    }

    let branch = std::env::var("PHARNESS_WORKSPACE_BRANCH")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let mut command = tokio::process::Command::new("git");
    command.arg("clone").arg("--depth").arg("1");
    if let Some(branch) = &branch {
        command.arg("--branch").arg(branch);
    }
    command.arg(&repo).arg(&cwd);

    tracing::info!(repo = %repo, cwd = %cwd.display(), "cloning workspace repo");
    let output = command
        .output()
        .await
        .context("failed to spawn git clone for workspace")?;
    if !output.status.success() {
        anyhow::bail!(
            "workspace clone failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

async fn fetch_attempt_spec_with_retry(
    backend: &HttpAttemptBackend,
    approval_id: Option<&str>,
) -> anyhow::Result<AttemptSpec> {
    let mut last_error = None;
    for attempt in 1..=CONTEXT_FETCH_ATTEMPTS {
        match backend.fetch_attempt_spec(approval_id).await {
            Ok(spec) => return Ok(spec),
            Err(error) => {
                tracing::warn!(attempt, %error, "attempt context fetch failed");
                last_error = Some(error);
            }
        }
        if attempt < CONTEXT_FETCH_ATTEMPTS {
            tokio::time::sleep(CONTEXT_FETCH_RETRY_DELAY).await;
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("attempt context fetch failed")))
}

async fn poll_control(backend: Arc<HttpAttemptBackend>, cancellation: CancellationFlag) {
    loop {
        tokio::time::sleep(CONTROL_POLL_INTERVAL).await;
        match backend.fetch_control().await {
            Ok(control) => {
                if control.cancel_requested {
                    tracing::info!("cancel requested through control plane");
                    cancellation.cancel();
                    return;
                }
            }
            Err(error) => {
                tracing::warn!(%error, "control poll failed; attempt continues");
            }
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct ControlResponse {
    cancel_requested: bool,
}

struct HttpAttemptBackend {
    client: reqwest::Client,
    base_url: String,
    run_id: String,
    token: String,
}

impl HttpAttemptBackend {
    fn new(base_url: String, run_id: String, token: String) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .context("failed to build worker http client")?;

        Ok(Self {
            client,
            base_url,
            run_id,
            token,
        })
    }

    fn internal_url(&self, suffix: &str) -> String {
        format!(
            "{}/api/internal/runs/{}/{suffix}",
            self.base_url, self.run_id
        )
    }

    async fn fetch_attempt_spec(&self, approval_id: Option<&str>) -> anyhow::Result<AttemptSpec> {
        let mut url = self.internal_url("attempt-context");
        if let Some(approval_id) = approval_id {
            url = format!("{url}?approval_id={approval_id}");
        }
        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await?;
        Self::ensure_success(&url, response.status())?;
        let response = response.error_for_status()?;

        Ok(response.json::<AttemptSpec>().await?)
    }

    async fn fetch_control(&self) -> anyhow::Result<ControlResponse> {
        let url = self.internal_url("control");
        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await?
            .error_for_status()?;

        Ok(response.json::<ControlResponse>().await?)
    }

    fn ensure_success(url: &str, status: reqwest::StatusCode) -> anyhow::Result<()> {
        if status.is_success() {
            Ok(())
        } else {
            anyhow::bail!("{url} returned {status}")
        }
    }

    async fn post_json_with_retry(
        &self,
        suffix: &str,
        body: &serde_json::Value,
    ) -> anyhow::Result<()> {
        let url = self.internal_url(suffix);
        let mut last_error: Option<anyhow::Error> = None;
        for attempt in 1..=INGEST_ATTEMPTS {
            let result = self
                .client
                .post(&url)
                .bearer_auth(&self.token)
                .json(body)
                .send()
                .await;
            match result {
                Ok(response) if response.status().is_success() => return Ok(()),
                Ok(response) if response.status().is_client_error() => {
                    let status = response.status();
                    let text = response.text().await.unwrap_or_default();
                    anyhow::bail!("{url} rejected request: {status} {text}");
                }
                Ok(response) => {
                    last_error = Some(anyhow::anyhow!("{url} returned {}", response.status()));
                }
                Err(error) => {
                    last_error = Some(error.into());
                }
            }
            if attempt < INGEST_ATTEMPTS {
                tokio::time::sleep(INGEST_RETRY_DELAY).await;
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("{url} failed")))
    }
}

#[async_trait::async_trait]
impl AttemptBackend for HttpAttemptBackend {
    async fn mark_running(&self) -> anyhow::Result<()> {
        self.post_json_with_retry("mark-running", &serde_json::json!({}))
            .await
    }

    async fn ingest_event(&self, event: &AgentEvent) -> anyhow::Result<()> {
        self.post_json_with_retry("events", &serde_json::json!({ "events": [event] }))
            .await
    }

    async fn finish(&self, outcome: AttemptOutcome) -> anyhow::Result<()> {
        self.post_json_with_retry("outcome", &serde_json::to_value(&outcome)?)
            .await
    }
}

fn init_tracing() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("pharness_worker=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .compact()
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to initialize tracing: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{pipeline_run_terminal, PipelineRunTerminal};
    use serde_json::json;

    #[test]
    fn recognizes_a_successful_pipeline_run() {
        let pipeline_run = json!({
            "status": {
                "conditions": [{ "type": "Succeeded", "status": "True" }]
            }
        });

        assert_eq!(
            pipeline_run_terminal(&pipeline_run),
            Some(PipelineRunTerminal::Succeeded)
        );
    }

    #[test]
    fn recognizes_a_failed_pipeline_run_with_a_safe_reason() {
        let pipeline_run = json!({
            "status": {
                "conditions": [{
                    "type": "Succeeded",
                    "status": "False",
                    "reason": "TasksFailed"
                }]
            }
        });

        assert_eq!(
            pipeline_run_terminal(&pipeline_run),
            Some(PipelineRunTerminal::Failed(
                "PipelineRun completed unsuccessfully: TasksFailed".to_string()
            ))
        );
    }

    #[test]
    fn keeps_observing_a_non_terminal_pipeline_run() {
        let pipeline_run = json!({
            "status": {
                "conditions": [{ "type": "Succeeded", "status": "Unknown" }]
            }
        });

        assert_eq!(pipeline_run_terminal(&pipeline_run), None);
    }
}
