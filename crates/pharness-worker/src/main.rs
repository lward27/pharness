#![forbid(unsafe_code)]

//! One-attempt worker binary for cluster execution targets.
//!
//! The worker executes exactly one run attempt (initial or resume) against
//! the pharness API, which stays the sole store writer. The process exits 0
//! when the attempt reached a durable terminal or approval-paused state, and
//! non-zero only when the attempt could not be reported back to the API.

use anyhow::Context;
use hmac::{Hmac, Mac};
use pharness_config::ApiRuntimeConfig;
use pharness_core::{
    AgentAction, AgentEvent, CancellationFlag, EnvironmentSnapshot, ProjectContract,
    ReadOnlyClusterTools, ToolExecutor,
};
use pharness_fireworks::{FireworksClient, FireworksProviderConfig};
use pharness_runhost::{
    execute_attempt, AttemptBackend, AttemptHost, AttemptOutcome, AttemptSpec, WorkspaceSourceSpec,
};
use serde::de::DeserializeOwned;
use serde_yaml::Value as YamlValue;
use sha2::Sha256;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tracing_subscriber::EnvFilter;

const CONTROL_POLL_INTERVAL: Duration = Duration::from_secs(2);
const INGEST_ATTEMPTS: u32 = 5;
const INGEST_RETRY_DELAY: Duration = Duration::from_secs(2);
// Fresh pods can see transient connection refusals until the CNI's network
// policy state includes the new pod; the startup fetch must ride that out.
const CONTEXT_FETCH_ATTEMPTS: u32 = 5;
const CONTEXT_FETCH_RETRY_DELAY: Duration = Duration::from_secs(2);
const DEFAULT_TEKTON_EXECUTOR_POLL_SECONDS: u64 = 5;
const DEFAULT_ARGO_EXECUTOR_POLL_SECONDS: u64 = 5;

/// Update one declared Kustomize image entry to an immutable image digest.
///
/// This supports only the standard `images` list and requires exactly one
/// matching `name`. A GitOps writer stops for review rather than guessing
/// among aliases or rewriting an arbitrary manifest.
fn update_kustomization_image(
    source: &str,
    image_name: &str,
    image_ref: &str,
) -> anyhow::Result<String> {
    validate_kustomization_image_name(image_name)?;
    let (new_name, digest) = parse_digest_pinned_image_reference(image_ref)?;
    let mut document: YamlValue =
        serde_yaml::from_str(source).context("kustomization document is not valid YAML")?;
    let root = document
        .as_mapping_mut()
        .context("kustomization document must be a YAML mapping")?;
    let images_key = YamlValue::String("images".to_string());
    let images = root
        .get_mut(&images_key)
        .and_then(YamlValue::as_sequence_mut)
        .context("kustomization document must contain an images sequence")?;

    let matching = images
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            entry
                .as_mapping()
                .and_then(|mapping| mapping.get(YamlValue::String("name".to_string())))
                .and_then(YamlValue::as_str)
                .filter(|name| *name == image_name)
                .map(|_| index)
        })
        .collect::<Vec<_>>();
    let index = match matching.as_slice() {
        [index] => *index,
        [] => anyhow::bail!("kustomization image entry was not found"),
        _ => anyhow::bail!("kustomization image entry is ambiguous"),
    };
    let entry = images[index]
        .as_mapping_mut()
        .context("kustomization image entry must be a mapping")?;
    entry.insert(
        YamlValue::String("newName".to_string()),
        YamlValue::String(new_name),
    );
    entry.insert(
        YamlValue::String("digest".to_string()),
        YamlValue::String(digest),
    );
    // A mutable tag combined with a digest makes the desired image ambiguous.
    entry.remove(YamlValue::String("newTag".to_string()));

    serde_yaml::to_string(&document).context("failed to serialize updated kustomization")
}

fn validate_kustomization_image_name(image_name: &str) -> anyhow::Result<()> {
    if image_name.trim().is_empty()
        || image_name != image_name.trim()
        || image_name.contains(['\0', '\n', '\r', '@'])
    {
        anyhow::bail!("invalid kustomization image name");
    }
    Ok(())
}

fn parse_digest_pinned_image_reference(image_ref: &str) -> anyhow::Result<(String, String)> {
    let (repository, digest) = image_ref
        .split_once('@')
        .context("image reference must be digest pinned")?;
    if repository.trim().is_empty()
        || repository != repository.trim()
        || repository.contains(['\0', '\n', '\r', '@'])
        || !digest.starts_with("sha256:")
        || digest.len() != "sha256:".len() + 64
        || !digest["sha256:".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        anyhow::bail!("image reference must contain a valid sha256 digest");
    }
    Ok((repository.to_string(), digest.to_string()))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing()?;

    if std::env::var("PHARNESS_EXECUTION_KIND").ok().as_deref() == Some("egress_proxy") {
        return execute_allowlisted_egress_proxy().await;
    }

    if std::env::var("PHARNESS_EXECUTION_KIND").ok().as_deref() == Some("environment_prepare") {
        return execute_environment_preparation().await;
    }

    if std::env::var("PHARNESS_EXECUTION_KIND").ok().as_deref() == Some("tekton_trigger") {
        return execute_tekton_trigger().await;
    }
    if std::env::var("PHARNESS_EXECUTION_KIND").ok().as_deref() == Some("argo_sync") {
        return execute_argo_sync().await;
    }
    if std::env::var("PHARNESS_EXECUTION_KIND").ok().as_deref() == Some("git_delivery") {
        return execute_git_delivery().await;
    }
    if std::env::var("PHARNESS_EXECUTION_KIND").ok().as_deref() == Some("git_delivery_observe") {
        return execute_git_delivery_observation().await;
    }
    if std::env::var("PHARNESS_EXECUTION_KIND").ok().as_deref() == Some("gitops_base_revision") {
        return execute_gitops_base_revision_resolution().await;
    }
    if std::env::var("PHARNESS_EXECUTION_KIND").ok().as_deref() == Some("gitops_delivery") {
        return execute_gitops_delivery().await;
    }
    if std::env::var("PHARNESS_EXECUTION_KIND").ok().as_deref() == Some("gitops_delivery_observe") {
        return execute_gitops_delivery_observation().await;
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
        context_budget: config.model.context_budget.clone(),
    };

    let backend = Arc::new(HttpAttemptBackend::new(
        env.api_url.clone(),
        env.run_id.clone(),
        env.worker_token.clone(),
    )?);

    let mut spec = fetch_attempt_spec_with_retry(&backend, env.approval_id.as_deref())
        .await
        .context("failed to fetch attempt context from api")?;

    let provisioned = match prepare_workspace(&spec).await {
        Ok(provisioned) => provisioned,
        Err(error) => {
            tracing::error!(run_id = %env.run_id, %error, "workspace preparation failed");
            backend
                .finish(AttemptOutcome::failed(format!(
                    "workspace preparation failed: {error}"
                )))
                .await
                .context("failed to report workspace preparation failure")?;
            return Ok(());
        }
    };
    if let Some(provisioned) = provisioned {
        let source = spec
            .run
            .workspace_source
            .as_mut()
            .expect("workspace provisioning requires a source contract");
        source.resolved_commit = Some(provisioned.resolved_commit.clone());
        if let Err(error) = backend
            .report_workspace_provisioned(
                &source.workspace_id,
                &provisioned.resolved_commit,
                &source.branch,
            )
            .await
        {
            tracing::error!(run_id = %env.run_id, %error, "workspace provisioning report failed");
            backend
                .finish(AttemptOutcome::failed(
                    "workspace provisioning report could not be persisted",
                ))
                .await
                .context("failed to report workspace provisioning failure")?;
            return Ok(());
        }
    }

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

async fn execute_allowlisted_egress_proxy() -> anyhow::Result<()> {
    use std::collections::BTreeSet;
    use tokio::net::TcpListener;

    let bind =
        std::env::var("PHARNESS_EGRESS_PROXY_BIND").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let allowed = serde_json::from_str::<Vec<String>>(&required_env(
        "PHARNESS_EGRESS_PROXY_ALLOWED_HOSTS_JSON",
    )?)?
    .into_iter()
    .map(|host| host.trim().to_ascii_lowercase())
    .collect::<BTreeSet<_>>();
    if allowed.is_empty()
        || allowed.iter().any(|host| {
            host.is_empty()
                || host.len() > 253
                || host.starts_with('.')
                || host.ends_with('.')
                || host.contains(['/', ':', '*', '\\', '\0', '\n', '\r'])
                || !host
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        })
    {
        anyhow::bail!("egress proxy host allowlist is invalid");
    }
    let listener = TcpListener::bind(&bind).await?;
    let allowed = Arc::new(allowed);
    tracing::info!(%bind, host_count = allowed.len(), "allowlisted egress proxy ready");
    loop {
        let (stream, _) = listener.accept().await?;
        let allowed = Arc::clone(&allowed);
        tokio::spawn(async move {
            if let Err(error) = proxy_connect(stream, allowed.as_ref()).await {
                tracing::warn!(%error, "egress proxy rejected or lost a connection");
            }
        });
    }
}

async fn proxy_connect(
    mut client: tokio::net::TcpStream,
    allowed: &std::collections::BTreeSet<String>,
) -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut request = Vec::with_capacity(1024);
    let header_end = loop {
        if request.len() >= 16 * 1024 {
            client
                .write_all(b"HTTP/1.1 431 Request Header Fields Too Large\r\n\r\n")
                .await?;
            anyhow::bail!("proxy request header exceeded 16 KiB");
        }
        let mut chunk = [0_u8; 1024];
        let read =
            tokio::time::timeout(std::time::Duration::from_secs(10), client.read(&mut chunk))
                .await
                .context("proxy request header timed out")??;
        if read == 0 {
            anyhow::bail!("proxy client closed before sending CONNECT");
        }
        request.extend_from_slice(&chunk[..read]);
        if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let request_head = std::str::from_utf8(&request[..header_end])?;
    let (host, port) = match allowlisted_connect_target(request_head, allowed) {
        Ok(target) => target,
        Err(error) => {
            client.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await?;
            return Err(error);
        }
    };
    let mut upstream = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        tokio::net::TcpStream::connect((host.as_str(), port)),
    )
    .await
    .context("CONNECT target timed out")??;
    client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    if request.len() > header_end {
        upstream.write_all(&request[header_end..]).await?;
    }
    tokio::io::copy_bidirectional(&mut client, &mut upstream).await?;
    Ok(())
}

fn allowlisted_connect_target(
    request_head: &str,
    allowed: &std::collections::BTreeSet<String>,
) -> anyhow::Result<(String, u16)> {
    let first_line = request_head
        .lines()
        .next()
        .context("proxy request is missing a request line")?;
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let authority = parts.next().unwrap_or_default();
    let version = parts.next().unwrap_or_default();
    if method != "CONNECT" || !matches!(version, "HTTP/1.0" | "HTTP/1.1") || parts.next().is_some()
    {
        anyhow::bail!("egress proxy accepts only HTTP/1.0 or HTTP/1.1 CONNECT");
    }
    let (host, port) = authority
        .rsplit_once(':')
        .context("CONNECT authority must include port 443")?;
    let host = host.to_ascii_lowercase();
    if port != "443" || !allowed.contains(&host) {
        anyhow::bail!("CONNECT target is outside the exact host allowlist");
    }
    Ok((host, 443))
}

async fn execute_environment_preparation() -> anyhow::Result<()> {
    let env = WorkerEnv::from_env()?;
    let backend = HttpAttemptBackend::new(
        env.api_url.clone(),
        env.run_id.clone(),
        env.worker_token.clone(),
    )?;
    let mut spec = fetch_attempt_spec_with_retry(&backend, None)
        .await
        .context("failed to fetch preparation context")?;
    let result = async {
        let provisioned = prepare_workspace(&spec).await?;
        if let Some(provisioned) = provisioned {
            let source = spec
                .run
                .workspace_source
                .as_mut()
                .context("preparation requires a workspace source")?;
            source.resolved_commit = Some(provisioned.resolved_commit.clone());
            backend
                .report_workspace_provisioned(
                    &source.workspace_id,
                    &provisioned.resolved_commit,
                    &source.branch,
                )
                .await?;
        }
        prepare_project_environment(&spec, &env.worker_token).await
    }
    .await;

    match result {
        Ok(payload) => backend
            .post_json_with_retry("environment-preparation", &payload)
            .await
            .context("failed to report environment preparation"),
        Err(error) => {
            tracing::error!(run_id = %env.run_id, %error, "environment preparation failed");
            backend
                .post_json_with_retry(
                    "environment-preparation",
                    &serde_json::json!({
                        "status": "failed",
                        "error": error.to_string(),
                        "logs": [{"step":"preparation","status":"failed","summary":error.to_string()}],
                    }),
                )
                .await
                .context("failed to report environment preparation failure")
        }
    }
}

async fn prepare_project_environment(
    spec: &AttemptSpec,
    worker_token: &str,
) -> anyhow::Result<serde_json::Value> {
    let cwd = std::path::PathBuf::from(&spec.run.cwd);
    let source = spec
        .run
        .workspace_source
        .as_ref()
        .context("environment preparation requires typed workspace source")?;
    let source_sha = source
        .resolved_commit
        .as_deref()
        .or(source.source_commit.as_deref())
        .context("environment preparation requires immutable source commit")?;
    let profile_id = required_env("PHARNESS_ENVIRONMENT_PROFILE_ID")?;
    if spec
        .run
        .execution_target_json
        .get("environment_profile_id")
        .and_then(serde_json::Value::as_str)
        != Some(profile_id.as_str())
    {
        anyhow::bail!("runner profile does not match the server-selected run profile");
    }
    let (contract, contract_hash) = ProjectContract::load(&cwd)?;
    if contract.environment_profile != profile_id {
        anyhow::bail!(
            "repository contract selects {} rather than runner profile {profile_id}",
            contract.environment_profile
        );
    }
    let required_executables =
        serde_json::from_str::<Vec<String>>(&required_env("PHARNESS_REQUIRED_EXECUTABLES_JSON")?)
            .context("required executable inventory is invalid")?;
    let mut executable_paths = serde_json::Map::new();
    for executable in &required_executables {
        let path = executable_path(executable).await?;
        executable_paths.insert(executable.clone(), serde_json::Value::String(path));
    }
    let python = required_executables
        .iter()
        .find(|name| name.as_str() == "python" || name.as_str() == "python3")
        .context("python runner profile must declare python or python3")?;
    let python_path = executable_paths
        .get(python)
        .and_then(serde_json::Value::as_str)
        .context("python executable path was not recorded")?;
    let runtime_dir = cwd.join(".pharness-runtime");
    let venv = runtime_dir.join("venv");
    tokio::fs::create_dir_all(&runtime_dir).await?;
    exclude_runtime_from_git(&cwd).await?;
    run_checked(
        &cwd,
        python_path,
        &["-m", "venv", venv.to_string_lossy().as_ref()],
    )
    .await?;
    let venv_python = venv.join("bin/python");
    run_checked(
        &cwd,
        venv_python.to_string_lossy().as_ref(),
        &[
            "-m",
            "pip",
            "install",
            "--disable-pip-version-check",
            "--require-hashes",
            "--only-binary=:all:",
            "-r",
            &contract.dependency_lock.path,
        ],
    )
    .await?;
    let python_version =
        run_output(&cwd, venv_python.to_string_lossy().as_ref(), &["--version"]).await?;
    let effective_user = run_output(&cwd, "id", &["-u"]).await?;
    let mut unavailable_tools = Vec::new();
    for executable in ["docker", "podman", "apt", "apt-get", "apk"] {
        if executable_path_optional(executable).await.is_none() {
            unavailable_tools.push(executable.to_string());
        }
    }
    let snapshot = EnvironmentSnapshot {
        source_sha: source_sha.to_string(),
        manifest_sha256: contract_hash.clone(),
        dependency_lock_sha256: contract.dependency_lock.sha256.clone(),
        runner_image_digest: required_env("PHARNESS_RUNNER_IMAGE")?,
        runner_revision: required_env("PHARNESS_RUNNER_REVISION")?,
        os: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
        effective_user,
        python_version,
        python_path: venv_python.to_string_lossy().to_string(),
        writable_paths: contract.writable_paths.clone(),
        unavailable_tools,
        agent_network: contract.agent_network,
        package_installation: contract.package_installation,
        acceptance_commands: contract.acceptance_commands.clone(),
        preparation_evidence: serde_json::json!({
            "required_executables": executable_paths,
            "venv": venv,
            "dependency_install": "pip --require-hashes --only-binary=:all:",
            "platform": required_env("PHARNESS_RUNNER_PLATFORM")?,
        }),
    };
    let snapshot_json = serde_json::to_value(&snapshot)?;
    let signature = signed_payload(worker_token, &snapshot_json);
    Ok(serde_json::json!({
        "status": "succeeded",
        "project_contract": contract,
        "project_contract_hash": contract_hash,
        "environment_snapshot": snapshot_json,
        "snapshot_signature": signature,
        "logs": [
            {"step":"checkout","status":"succeeded","source_sha":source_sha},
            {"step":"contract","status":"succeeded","manifest_sha256":contract_hash},
            {"step":"executables","status":"succeeded","inventory":required_executables},
            {"step":"dependencies","status":"succeeded","lock_sha256":snapshot.dependency_lock_sha256},
        ],
    }))
}

async fn executable_path(executable: &str) -> anyhow::Result<String> {
    executable_path_optional(executable)
        .await
        .with_context(|| format!("runner is missing required executable {executable}"))
}

async fn executable_path_optional(executable: &str) -> Option<String> {
    let output = Command::new("/bin/sh")
        .args(["-c", "command -v \"$1\"", "pharness-executable", executable])
        .output()
        .await
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

async fn run_checked(cwd: &std::path::Path, program: &str, args: &[&str]) -> anyhow::Result<()> {
    let output = Command::new(program)
        .current_dir(cwd)
        .args(args)
        .output()
        .await?;
    if !output.status.success() {
        anyhow::bail!(
            "{} failed: {}",
            program,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

async fn run_output(cwd: &std::path::Path, program: &str, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new(program)
        .current_dir(cwd)
        .args(args)
        .output()
        .await?;
    if !output.status.success() {
        anyhow::bail!("{} failed", program);
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_string()
        .pipe_if_empty(|| String::from_utf8_lossy(&output.stderr).trim().to_string()))
}

trait NonEmptyString {
    fn pipe_if_empty(self, fallback: impl FnOnce() -> String) -> String;
}

impl NonEmptyString for String {
    fn pipe_if_empty(self, fallback: impl FnOnce() -> String) -> String {
        if self.is_empty() {
            fallback()
        } else {
            self
        }
    }
}

async fn exclude_runtime_from_git(cwd: &std::path::Path) -> anyhow::Result<()> {
    let exclude = cwd.join(".git/info/exclude");
    let mut content = tokio::fs::read_to_string(&exclude)
        .await
        .unwrap_or_default();
    if !content
        .lines()
        .any(|line| line.trim() == "/.pharness-runtime/")
    {
        if !content.ends_with('\n') && !content.is_empty() {
            content.push('\n');
        }
        content.push_str("/.pharness-runtime/\n");
        tokio::fs::write(exclude, content).await?;
    }
    Ok(())
}

fn signed_payload(token: &str, payload: &serde_json::Value) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(token.as_bytes())
        .expect("HMAC-SHA256 accepts keys of any size");
    mac.update(payload.to_string().as_bytes());
    format!("hmac-sha256:{:x}", mac.finalize().into_bytes())
}

async fn fetch_internal_context_with_retry<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    token: &str,
) -> anyhow::Result<T> {
    fetch_internal_context(
        client,
        url,
        token,
        CONTEXT_FETCH_ATTEMPTS,
        CONTEXT_FETCH_RETRY_DELAY,
    )
    .await
}

async fn fetch_internal_context<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    attempts: u32,
    retry_delay: Duration,
) -> anyhow::Result<T> {
    let attempts = attempts.max(1);
    let mut last_error = None;
    for attempt in 1..=attempts {
        match client.get(url).bearer_auth(token).send().await {
            Ok(response) if response.status().is_client_error() => {
                anyhow::bail!(
                    "internal context request was rejected with {}",
                    response.status()
                );
            }
            Ok(response) if !response.status().is_success() => {
                last_error = Some(anyhow::anyhow!(
                    "internal context request returned {}",
                    response.status()
                ));
            }
            Ok(response) => {
                return response
                    .json::<T>()
                    .await
                    .context("internal context response was invalid");
            }
            Err(error) => last_error = Some(error.into()),
        }
        if attempt < attempts {
            tracing::warn!(attempt, "internal context fetch failed; retrying");
            tokio::time::sleep(retry_delay).await;
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("internal context request failed")))
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

/// Execute one API-authorized Argo CD sync. The worker has no model, Git,
/// registry, database, or secret-reading capability; Kubernetes RBAC limits
/// it to `get` and `patch` on named Application resources.
async fn execute_argo_sync() -> anyhow::Result<()> {
    let api_url = required_env("PHARNESS_API_URL")?
        .trim_end_matches('/')
        .to_string();
    let deployment_intent_id = required_env("PHARNESS_DEPLOYMENT_INTENT_ID")?;
    let execution_id = required_env("PHARNESS_ARGO_EXECUTION_ID")?;
    let worker_token = required_env("PHARNESS_WORKER_TOKEN")?;
    let argo_namespace = required_env("PHARNESS_ARGOCD_NAMESPACE")?;
    let configured_poll_interval = argo_poll_interval()?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build Argo executor http client")?;
    let context_url = format!(
        "{api_url}/api/internal/deployment-intents/{deployment_intent_id}/argo-sync-context?execution_id={execution_id}"
    );
    let context =
        fetch_internal_context_with_retry::<ArgoSyncContext>(&client, &context_url, &worker_token)
            .await
            .context("failed to fetch Argo sync context")?;
    if context.execution_id != execution_id {
        anyhow::bail!("Argo sync context execution id did not match");
    }
    if context.poll_seconds != configured_poll_interval.as_secs() {
        anyhow::bail!("Argo sync context poll interval did not match executor configuration");
    }
    tracing::info!(
        deployment_intent_id = %deployment_intent_id,
        target_namespace = %context.target_namespace,
        argo_application = %context.argo_application,
        "starting bounded Argo sync execution"
    );
    let poll_interval = configured_poll_interval;

    if argo_sync_cancelled(
        &client,
        &api_url,
        &deployment_intent_id,
        &execution_id,
        &worker_token,
    )
    .await?
    {
        return post_argo_sync_outcome(
            &client,
            &api_url,
            &deployment_intent_id,
            &worker_token,
            &serde_json::json!({ "execution_id": execution_id, "status": "cancelled", "error_code": "cancelled_before_sync" }),
        )
        .await;
    }

    if let Err(error) = start_argo_sync(
        &argo_namespace,
        &context.argo_application,
        context.revision.as_deref(),
    )
    .await
    {
        tracing::warn!(deployment_intent_id = %deployment_intent_id, error = %error, "Argo sync patch failed");
        return post_argo_sync_outcome(
            &client,
            &api_url,
            &deployment_intent_id,
            &worker_token,
            &serde_json::json!({ "execution_id": execution_id, "status": "failed", "error_code": "argo_sync_patch_failed" }),
        )
        .await;
    }
    post_argo_sync_outcome(
        &client,
        &api_url,
        &deployment_intent_id,
        &worker_token,
        &serde_json::json!({ "execution_id": execution_id, "status": "submitted" }),
    )
    .await?;

    loop {
        if argo_sync_cancelled(
            &client,
            &api_url,
            &deployment_intent_id,
            &execution_id,
            &worker_token,
        )
        .await?
        {
            return post_argo_sync_outcome(
                &client,
                &api_url,
                &deployment_intent_id,
                &worker_token,
                &serde_json::json!({ "execution_id": execution_id, "status": "cancelled", "error_code": "cancelled_while_observing" }),
            )
            .await;
        }
        match observe_argo_application(
            &argo_namespace,
            &context.argo_application,
            context.revision.as_deref(),
        )
        .await
        {
            Ok(ArgoApplicationTerminal::Succeeded(state)) => {
                return post_argo_sync_outcome(
                    &client,
                    &api_url,
                    &deployment_intent_id,
                    &worker_token,
                    &serde_json::json!({
                        "execution_id": execution_id,
                        "status": "completed",
                        "sync_status": state.sync_status,
                        "health_status": state.health_status,
                        "operation_phase": state.operation_phase,
                        "revision": state.revision,
                    }),
                )
                .await;
            }
            Ok(ArgoApplicationTerminal::Failed(state)) => {
                return post_argo_sync_outcome(
                    &client,
                    &api_url,
                    &deployment_intent_id,
                    &worker_token,
                    &serde_json::json!({
                        "execution_id": execution_id,
                        "status": "failed",
                        "sync_status": state.sync_status,
                        "health_status": state.health_status,
                        "operation_phase": state.operation_phase,
                        "revision": state.revision,
                        "error_code": "argo_operation_failed",
                    }),
                )
                .await;
            }
            Ok(ArgoApplicationTerminal::Pending) => tokio::time::sleep(poll_interval).await,
            Err(error) => {
                tracing::warn!(deployment_intent_id = %deployment_intent_id, error = %error, "Argo Application observation failed");
                return post_argo_sync_outcome(
                    &client,
                    &api_url,
                    &deployment_intent_id,
                    &worker_token,
                    &serde_json::json!({ "execution_id": execution_id, "status": "failed", "error_code": "argo_observation_failed" }),
                )
                .await;
            }
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct ArgoSyncContext {
    execution_id: String,
    target_namespace: String,
    argo_application: String,
    revision: Option<String>,
    poll_seconds: u64,
}

#[derive(Debug, serde::Deserialize)]
struct ArgoSyncControl {
    cancelled: bool,
}

#[derive(Debug)]
struct ArgoApplicationState {
    sync_status: Option<String>,
    health_status: Option<String>,
    operation_phase: Option<String>,
    revision: Option<String>,
}

#[derive(Debug)]
enum ArgoApplicationTerminal {
    Pending,
    Succeeded(ArgoApplicationState),
    Failed(ArgoApplicationState),
}

fn argo_poll_interval() -> anyhow::Result<Duration> {
    let seconds = std::env::var("PHARNESS_ARGO_EXECUTOR_POLL_SECONDS")
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()
        .context("PHARNESS_ARGO_EXECUTOR_POLL_SECONDS must be an integer")?
        .unwrap_or(DEFAULT_ARGO_EXECUTOR_POLL_SECONDS);
    if seconds == 0 {
        anyhow::bail!("PHARNESS_ARGO_EXECUTOR_POLL_SECONDS must be greater than zero");
    }
    Ok(Duration::from_secs(seconds))
}

async fn argo_sync_cancelled(
    client: &reqwest::Client,
    api_url: &str,
    deployment_intent_id: &str,
    execution_id: &str,
    token: &str,
) -> anyhow::Result<bool> {
    let url = format!(
        "{api_url}/api/internal/deployment-intents/{deployment_intent_id}/argo-sync-control?execution_id={execution_id}"
    );
    let response = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .context("failed to fetch Argo sync control")?
        .error_for_status()
        .context("Argo sync control request was rejected")?
        .json::<ArgoSyncControl>()
        .await
        .context("Argo sync control response was invalid")?;
    Ok(response.cancelled)
}

async fn start_argo_sync(
    namespace: &str,
    application: &str,
    revision: Option<&str>,
) -> anyhow::Result<()> {
    let patch = argo_sync_patch_payload(revision).to_string();
    let output = tokio::process::Command::new("kubectl")
        .args([
            "patch",
            "applications.argoproj.io",
            application,
            "-n",
            namespace,
            "--type=merge",
            "-p",
            &patch,
        ])
        .output()
        .await
        .context("failed to spawn kubectl for Argo sync")?;
    if output.status.success() {
        Ok(())
    } else {
        anyhow::bail!("kubectl could not patch the approved Argo Application")
    }
}

fn argo_sync_patch_payload(revision: Option<&str>) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "operation": {
            "sync": {
                "prune": false,
            }
        }
    });
    if let Some(revision) = revision {
        payload["operation"]["sync"]["revision"] = serde_json::json!(revision);
    }
    payload
}

async fn observe_argo_application(
    namespace: &str,
    application: &str,
    expected_revision: Option<&str>,
) -> anyhow::Result<ArgoApplicationTerminal> {
    let output = tokio::process::Command::new("kubectl")
        .args([
            "get",
            "applications.argoproj.io",
            application,
            "-n",
            namespace,
            "-o",
            "json",
        ])
        .output()
        .await
        .context("failed to spawn kubectl for Argo Application observation")?;
    if !output.status.success() {
        anyhow::bail!("kubectl could not read the approved Argo Application");
    }
    let application: serde_json::Value = serde_json::from_slice(&output.stdout)
        .context("kubectl returned invalid Argo Application JSON")?;
    Ok(argo_application_terminal(&application, expected_revision))
}

fn argo_application_terminal(
    application: &serde_json::Value,
    expected_revision: Option<&str>,
) -> ArgoApplicationTerminal {
    let state = ArgoApplicationState {
        sync_status: application
            .pointer("/status/sync/status")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        health_status: application
            .pointer("/status/health/status")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        operation_phase: application
            .pointer("/status/operationState/phase")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        revision: application
            .pointer("/status/operationState/syncResult/revision")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
    };
    if expected_revision.is_some() && state.revision.as_deref() != expected_revision {
        return ArgoApplicationTerminal::Pending;
    }
    match state.operation_phase.as_deref() {
        Some("Succeeded") if state.sync_status.as_deref() == Some("Synced") => {
            ArgoApplicationTerminal::Succeeded(state)
        }
        Some("Failed") | Some("Error") | Some("Terminated") => {
            ArgoApplicationTerminal::Failed(state)
        }
        _ => ArgoApplicationTerminal::Pending,
    }
}

async fn post_argo_sync_outcome(
    client: &reqwest::Client,
    api_url: &str,
    deployment_intent_id: &str,
    token: &str,
    outcome: &serde_json::Value,
) -> anyhow::Result<()> {
    let url = format!(
        "{api_url}/api/internal/deployment-intents/{deployment_intent_id}/argo-sync-outcome"
    );
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
                anyhow::bail!("{url} rejected Argo sync outcome: {status} {body}");
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

/// Execute one API-issued GitHub branch-and-PR plan. This process never loads
/// Fireworks credentials and only receives the Git writer token through its
/// dedicated Job Secret mount.
async fn execute_git_delivery() -> anyhow::Result<()> {
    let api_url = required_env("PHARNESS_API_URL")?
        .trim_end_matches('/')
        .to_string();
    let change_set_id = required_env("PHARNESS_CHANGE_SET_ID")?;
    let execution_id = required_env("PHARNESS_GIT_DELIVERY_EXECUTION_ID")?;
    let worker_token = required_env("PHARNESS_WORKER_TOKEN")?;
    let git_token = required_env("PHARNESS_GIT_WRITER_TOKEN")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("failed to build Git writer http client")?;
    let context_url = format!("{api_url}/api/internal/change-sets/{change_set_id}/git-delivery-context?execution_id={execution_id}");
    let context = fetch_internal_context_with_retry::<GitDeliveryContext>(
        &client,
        &context_url,
        &worker_token,
    )
    .await
    .context("failed to fetch Git writer context")?;

    let result = execute_git_delivery_plan(&client, &context, &git_token, &execution_id).await;
    let outcome = match result {
        Ok(completed) => serde_json::json!({
            "execution_id": execution_id, "status": "completed", "branch": completed.branch,
            "commit_sha": completed.commit_sha, "pull_request_url": completed.pull_request_url,
            "pull_request_number": completed.pull_request_number,
        }),
        Err(error) => {
            tracing::warn!(change_set_id = %change_set_id, error = %error, "Git writer failed without exposing command output");
            serde_json::json!({
                "execution_id": execution_id, "status": "failed", "error_code": git_delivery_error_code(&error),
            })
        }
    };
    let outcome_url =
        format!("{api_url}/api/internal/change-sets/{change_set_id}/git-delivery-outcome");
    post_git_delivery_outcome(&client, &outcome_url, &worker_token, &outcome).await
}

/// Read one GitHub pull request through the dedicated observer identity. The
/// observer has no Git CLI, workspace, or model credentials and reports only
/// bounded merge provenance to the API.
async fn execute_git_delivery_observation() -> anyhow::Result<()> {
    let api_url = required_env("PHARNESS_API_URL")?
        .trim_end_matches('/')
        .to_string();
    let change_set_id = required_env("PHARNESS_CHANGE_SET_ID")?;
    let execution_id = required_env("PHARNESS_GIT_DELIVERY_EXECUTION_ID")?;
    let worker_token = required_env("PHARNESS_WORKER_TOKEN")?;
    let git_token = required_env("PHARNESS_GIT_OBSERVER_TOKEN")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build Git observer http client")?;
    let context_url = format!("{api_url}/api/internal/change-sets/{change_set_id}/git-delivery-observation-context?execution_id={execution_id}");
    let context = fetch_internal_context_with_retry::<GitDeliveryObservationContext>(
        &client,
        &context_url,
        &worker_token,
    )
    .await
    .context("failed to fetch Git observer context")?;
    let outcome = match observe_github_pull_request(&client, &context, &git_token).await {
        Ok(observation) => serde_json::json!({
            "execution_id": execution_id,
            "status": "observed",
            "pull_request_state": observation.pull_request_state,
            "merged": observation.merged,
            "merge_commit_sha": observation.merge_commit_sha,
            "head_branch": observation.head_branch,
            "head_commit_sha": observation.head_commit_sha,
        }),
        Err(error) => {
            tracing::warn!(change_set_id = %change_set_id, error = %error, "Git observer failed without exposing provider output");
            serde_json::json!({
                "execution_id": execution_id,
                "status": "failed",
                "error_code": git_observer_error_code(&error),
            })
        }
    };
    let outcome_url = format!(
        "{api_url}/api/internal/change-sets/{change_set_id}/git-delivery-observation-outcome"
    );
    post_git_delivery_outcome(&client, &outcome_url, &worker_token, &outcome).await
}

/// Resolve one GitOps base ref through the observer identity. This does not
/// clone a repository, inspect file content, or use a write-capable token.
async fn execute_gitops_base_revision_resolution() -> anyhow::Result<()> {
    let api_url = required_env("PHARNESS_API_URL")?
        .trim_end_matches('/')
        .to_string();
    let change_set_id = required_env("PHARNESS_GITOPS_CHANGE_SET_ID")?;
    let execution_id = required_env("PHARNESS_GITOPS_REVISION_EXECUTION_ID")?;
    let worker_token = required_env("PHARNESS_WORKER_TOKEN")?;
    let git_token = required_env("PHARNESS_GIT_OBSERVER_TOKEN")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build GitOps revision resolver http client")?;
    let context_url = format!(
        "{api_url}/api/internal/gitops-change-sets/{change_set_id}/base-revision-context?execution_id={execution_id}"
    );
    let context = fetch_internal_context_with_retry::<GitOpsBaseRevisionContext>(
        &client,
        &context_url,
        &worker_token,
    )
    .await
    .context("failed to fetch GitOps base revision context")?;
    let outcome = match resolve_github_base_revision(&client, &context, &git_token).await {
        Ok(base_commit) => serde_json::json!({
            "execution_id": execution_id,
            "status": "resolved",
            "base_commit": base_commit,
        }),
        Err(error) => {
            tracing::warn!(gitops_change_set_id = %change_set_id, error = %error, "GitOps base revision resolver failed without exposing provider output");
            serde_json::json!({
                "execution_id": execution_id,
                "status": "failed",
                "error_code": git_observer_error_code(&error),
            })
        }
    };
    let outcome_url =
        format!("{api_url}/api/internal/gitops-change-sets/{change_set_id}/base-revision-outcome");
    post_git_delivery_outcome(&client, &outcome_url, &worker_token, &outcome).await
}

/// Execute one GitOps branch-and-PR delivery. This process receives only the
/// dedicated GitOps token and one API-validated Kustomization image operation.
async fn execute_gitops_delivery() -> anyhow::Result<()> {
    let api_url = required_env("PHARNESS_API_URL")?
        .trim_end_matches('/')
        .to_string();
    let change_set_id = required_env("PHARNESS_GITOPS_CHANGE_SET_ID")?;
    let execution_id = required_env("PHARNESS_GITOPS_DELIVERY_EXECUTION_ID")?;
    let worker_token = required_env("PHARNESS_WORKER_TOKEN")?;
    let git_token = required_env("PHARNESS_GIT_WRITER_TOKEN")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("failed to build GitOps writer http client")?;
    let context_url = format!(
        "{api_url}/api/internal/gitops-change-sets/{change_set_id}/delivery-context?execution_id={execution_id}"
    );
    let context = fetch_internal_context_with_retry::<GitOpsDeliveryContext>(
        &client,
        &context_url,
        &worker_token,
    )
    .await
    .context("failed to fetch GitOps writer context")?;
    let outcome = match execute_gitops_delivery_plan(&client, &context, &git_token, &execution_id)
        .await
    {
        Ok(completed) => serde_json::json!({
            "execution_id": execution_id,
            "status": "completed",
            "branch": completed.branch,
            "commit_sha": completed.commit_sha,
            "pull_request_url": completed.pull_request_url,
            "pull_request_number": completed.pull_request_number,
        }),
        Err(error) => {
            tracing::warn!(gitops_change_set_id = %change_set_id, error = %error, "GitOps writer failed without exposing command output");
            serde_json::json!({
                "execution_id": execution_id,
                "status": "failed",
                "error_code": gitops_delivery_error_code(&error),
            })
        }
    };
    let outcome_url =
        format!("{api_url}/api/internal/gitops-change-sets/{change_set_id}/delivery-outcome");
    post_git_delivery_outcome(&client, &outcome_url, &worker_token, &outcome).await
}

async fn execute_gitops_delivery_observation() -> anyhow::Result<()> {
    let api_url = required_env("PHARNESS_API_URL")?
        .trim_end_matches('/')
        .to_string();
    let change_set_id = required_env("PHARNESS_GITOPS_CHANGE_SET_ID")?;
    let execution_id = required_env("PHARNESS_GITOPS_DELIVERY_OBSERVATION_EXECUTION_ID")?;
    let worker_token = required_env("PHARNESS_WORKER_TOKEN")?;
    let git_token = required_env("PHARNESS_GIT_OBSERVER_TOKEN")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build GitOps observer http client")?;
    let context_url = format!("{api_url}/api/internal/gitops-change-sets/{change_set_id}/delivery-observation-context?execution_id={execution_id}");
    let context = fetch_internal_context_with_retry::<GitOpsDeliveryObservationContext>(
        &client,
        &context_url,
        &worker_token,
    )
    .await
    .context("failed to fetch GitOps observer context")?;
    let source_context = GitDeliveryObservationContext {
        execution_id: context.execution_id,
        repository: context.repository,
        head_branch: context.head_branch,
        source_commit_sha: context.source_commit_sha,
        pull_request_url: context.pull_request_url,
        pull_request_number: context.pull_request_number,
        github_api_url: context.github_api_url,
    };
    let outcome = match observe_github_pull_request(&client, &source_context, &git_token).await {
        Ok(observation) => {
            serde_json::json!({"execution_id":execution_id,"status":"observed","pull_request_state":observation.pull_request_state,"merged":observation.merged,"merge_commit_sha":observation.merge_commit_sha,"head_branch":observation.head_branch,"head_commit_sha":observation.head_commit_sha})
        }
        Err(error) => {
            tracing::warn!(gitops_change_set_id=%change_set_id,error=%error,"GitOps observer failed without exposing provider output");
            serde_json::json!({"execution_id":execution_id,"status":"failed","error_code":git_observer_error_code(&error)})
        }
    };
    let outcome_url = format!(
        "{api_url}/api/internal/gitops-change-sets/{change_set_id}/delivery-observation-outcome"
    );
    post_git_delivery_outcome(&client, &outcome_url, &worker_token, &outcome).await
}

#[derive(Debug, serde::Deserialize)]
struct GitOpsBaseRevisionContext {
    execution_id: String,
    repository: String,
    base_ref: String,
    github_api_url: String,
}

#[derive(Debug, serde::Deserialize)]
struct GitOpsDeliveryContext {
    execution_id: String,
    repository: String,
    base_ref: String,
    base_commit: String,
    head_branch: String,
    kustomization_path: String,
    image_name: String,
    image_ref: String,
    commit_subject: String,
    commit_body: String,
    pull_request_title: String,
    pull_request_body: String,
    github_api_url: String,
    author_name: String,
    author_email: String,
}

#[derive(Debug, serde::Deserialize)]
struct GitOpsDeliveryObservationContext {
    execution_id: String,
    repository: String,
    head_branch: String,
    source_commit_sha: String,
    pull_request_url: String,
    pull_request_number: u64,
    github_api_url: String,
}

async fn resolve_github_base_revision(
    client: &reqwest::Client,
    context: &GitOpsBaseRevisionContext,
    token: &str,
) -> anyhow::Result<String> {
    let (owner, repo) = parse_github_repository(&context.repository)?;
    if context.execution_id.trim().is_empty()
        || context.base_ref.trim().is_empty()
        || context.base_ref.contains(['\0', '\r', '\n'])
        || context.github_api_url.trim().is_empty()
    {
        anyhow::bail!("invalid_gitops_base_revision_context");
    }
    let api = context.github_api_url.trim_end_matches('/');
    if !api.starts_with("https://") {
        anyhow::bail!("invalid_github_api_url");
    }
    let url = format!("{api}/repos/{owner}/{repo}/commits/{}", context.base_ref);
    let response = client
        .get(url)
        .bearer_auth(token)
        .header("accept", "application/vnd.github+json")
        .header("user-agent", "pharness-gitops-revision-resolver")
        .send()
        .await
        .context("GitHub base revision resolution request failed")?;
    if !response.status().is_success() {
        anyhow::bail!("github_base_revision_resolution_failed");
    }
    let value: serde_json::Value = response
        .json()
        .await
        .context("GitHub base revision response was invalid")?;
    let sha = value
        .get("sha")
        .and_then(serde_json::Value::as_str)
        .filter(|value| is_git_sha(value))
        .context("GitHub base revision response lacked a valid commit SHA")?;
    Ok(sha.to_string())
}

#[derive(Debug, serde::Deserialize)]
struct GitDeliveryObservationContext {
    execution_id: String,
    repository: String,
    head_branch: String,
    source_commit_sha: String,
    pull_request_url: String,
    pull_request_number: u64,
    github_api_url: String,
}

struct GitPullRequestObservation {
    pull_request_state: String,
    merged: bool,
    merge_commit_sha: Option<String>,
    head_branch: String,
    head_commit_sha: String,
}

async fn observe_github_pull_request(
    client: &reqwest::Client,
    context: &GitDeliveryObservationContext,
    token: &str,
) -> anyhow::Result<GitPullRequestObservation> {
    let (owner, repo) = parse_github_repository(&context.repository)?;
    if context.execution_id.trim().is_empty()
        || !is_git_sha(&context.source_commit_sha)
        || !is_github_pr_url(&context.pull_request_url)
        || context.github_api_url.trim().is_empty()
    {
        anyhow::bail!("invalid_git_observation_context");
    }
    let api = context.github_api_url.trim_end_matches('/');
    if !api.starts_with("https://") {
        anyhow::bail!("invalid_github_api_url");
    }
    let url = format!(
        "{api}/repos/{owner}/{repo}/pulls/{}",
        context.pull_request_number
    );
    let response = client
        .get(url)
        .bearer_auth(token)
        .header("accept", "application/vnd.github+json")
        .header("user-agent", "pharness-git-observer")
        .send()
        .await
        .context("GitHub pull request observation request failed")?;
    if !response.status().is_success() {
        anyhow::bail!("github_pull_request_observation_failed");
    }
    let value: serde_json::Value = response
        .json()
        .await
        .context("GitHub pull request observation response was invalid")?;
    let number = value.get("number").and_then(serde_json::Value::as_u64);
    let html_url = value.get("html_url").and_then(serde_json::Value::as_str);
    let head_branch = value
        .pointer("/head/ref")
        .and_then(serde_json::Value::as_str);
    let head_commit_sha = value
        .pointer("/head/sha")
        .and_then(serde_json::Value::as_str);
    let pull_request_state = value.get("state").and_then(serde_json::Value::as_str);
    if number != Some(context.pull_request_number)
        || html_url != Some(context.pull_request_url.as_str())
        || head_branch != Some(context.head_branch.as_str())
        || head_commit_sha != Some(context.source_commit_sha.as_str())
        || !matches!(pull_request_state, Some("open" | "closed"))
    {
        anyhow::bail!("github_pull_request_provenance_mismatch");
    }
    let merged = value
        .get("merged")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let merge_commit_sha = value
        .pointer("/merge_commit_sha")
        .and_then(serde_json::Value::as_str)
        .filter(|value| is_git_sha(value))
        .map(ToOwned::to_owned);
    if merged && merge_commit_sha.is_none() {
        anyhow::bail!("github_merge_commit_missing");
    }
    Ok(GitPullRequestObservation {
        pull_request_state: pull_request_state.expect("validated state").to_string(),
        merged,
        merge_commit_sha,
        head_branch: head_branch.expect("validated branch").to_string(),
        head_commit_sha: head_commit_sha.expect("validated sha").to_string(),
    })
}

#[derive(Debug, serde::Deserialize)]
struct GitDeliveryContext {
    execution_id: String,
    repository: String,
    base_ref: String,
    base_commit: String,
    head_branch: String,
    diff: String,
    commit_subject: String,
    commit_body: String,
    pull_request_title: String,
    pull_request_body: String,
    github_api_url: String,
    author_name: String,
    author_email: String,
}

struct GitDeliveryCompleted {
    branch: String,
    commit_sha: String,
    pull_request_url: String,
    pull_request_number: u64,
}

async fn execute_git_delivery_plan(
    client: &reqwest::Client,
    context: &GitDeliveryContext,
    token: &str,
    execution_id: &str,
) -> anyhow::Result<GitDeliveryCompleted> {
    let (owner, repo) = parse_github_repository(&context.repository)?;
    validate_git_delivery_context(context)?;
    let root = std::path::PathBuf::from("/work").join(format!("git-{execution_id}"));
    let checkout = root.join("repo");
    tokio::fs::create_dir_all(&root).await?;
    let askpass = root.join("askpass");
    tokio::fs::write(&askpass, "#!/bin/sh\ncase \"$1\" in\n  *Username*) printf '%s\\n' x-access-token ;;\n  *) printf '%s\\n' \"$PHARNESS_GIT_WRITER_TOKEN\" ;;\nesac\n").await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&askpass, std::fs::Permissions::from_mode(0o700)).await?;
    }
    let result = async {
        git_delivery_command(
            &[
                "clone",
                "--no-checkout",
                "--filter=blob:none",
                &context.repository,
                checkout.to_str().context("checkout path is invalid")?,
            ],
            &askpass,
        )
        .await?;
        let checkout_text = checkout.to_str().context("checkout path is invalid")?;
        git_delivery_command(
            &[
                "-C",
                checkout_text,
                "fetch",
                "--depth",
                "1",
                "origin",
                &context.base_commit,
            ],
            &askpass,
        )
        .await?;
        git_delivery_command(
            &[
                "-C",
                checkout_text,
                "checkout",
                "--detach",
                &context.base_commit,
            ],
            &askpass,
        )
        .await?;
        let head =
            git_delivery_stdout(&["-C", checkout_text, "rev-parse", "HEAD"], &askpass).await?;
        if head != context.base_commit {
            anyhow::bail!("base_commit_mismatch");
        }
        git_delivery_command(
            &[
                "-C",
                checkout_text,
                "switch",
                "--create",
                &context.head_branch,
            ],
            &askpass,
        )
        .await?;
        let patch = root.join("change.patch");
        tokio::fs::write(&patch, git_patch_for_apply(&context.diff)).await?;
        git_delivery_command(
            &[
                "-C",
                checkout_text,
                "apply",
                "--whitespace=nowarn",
                "--index",
                patch.to_str().context("patch path is invalid")?,
            ],
            &askpass,
        )
        .await?;
        if git_delivery_command_status(
            &["-C", checkout_text, "diff", "--cached", "--quiet"],
            &askpass,
        )
        .await?
        {
            anyhow::bail!("empty_change_set");
        }
        git_delivery_command(
            &[
                "-C",
                checkout_text,
                "config",
                "user.name",
                &context.author_name,
            ],
            &askpass,
        )
        .await?;
        git_delivery_command(
            &[
                "-C",
                checkout_text,
                "config",
                "user.email",
                &context.author_email,
            ],
            &askpass,
        )
        .await?;
        git_delivery_command(
            &[
                "-C",
                checkout_text,
                "commit",
                "-m",
                &context.commit_subject,
                "-m",
                &context.commit_body,
            ],
            &askpass,
        )
        .await?;
        let commit_sha =
            git_delivery_stdout(&["-C", checkout_text, "rev-parse", "HEAD"], &askpass).await?;
        if !is_git_sha(&commit_sha) {
            anyhow::bail!("invalid_commit_sha");
        }
        git_delivery_command(
            &[
                "-C",
                checkout_text,
                "push",
                "origin",
                &format!("HEAD:refs/heads/{}", context.head_branch),
            ],
            &askpass,
        )
        .await?;
        let pr = create_github_pull_request(client, context, token, &owner, &repo).await?;
        Ok(GitDeliveryCompleted {
            branch: context.head_branch.clone(),
            commit_sha,
            pull_request_url: pr.0,
            pull_request_number: pr.1,
        })
    }
    .await;
    let _ = tokio::fs::remove_dir_all(&root).await;
    result
}

async fn execute_gitops_delivery_plan(
    client: &reqwest::Client,
    context: &GitOpsDeliveryContext,
    token: &str,
    execution_id: &str,
) -> anyhow::Result<GitDeliveryCompleted> {
    let (owner, repo) = parse_github_repository(&context.repository)?;
    validate_gitops_delivery_context(context)?;
    let root = std::path::PathBuf::from("/work").join(format!("gitops-{execution_id}"));
    let checkout = root.join("repo");
    tokio::fs::create_dir_all(&root).await?;
    let askpass = root.join("askpass");
    tokio::fs::write(&askpass, "#!/bin/sh\ncase \"$1\" in\n  *Username*) printf '%s\\n' x-access-token ;;\n  *) printf '%s\\n' \"$PHARNESS_GIT_WRITER_TOKEN\" ;;\nesac\n").await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&askpass, std::fs::Permissions::from_mode(0o700)).await?;
    }
    let result = async {
        git_delivery_command(
            &[
                "clone",
                "--no-checkout",
                "--filter=blob:none",
                &context.repository,
                checkout.to_str().context("checkout path is invalid")?,
            ],
            &askpass,
        )
        .await?;
        let checkout_text = checkout.to_str().context("checkout path is invalid")?;
        git_delivery_command(
            &[
                "-C",
                checkout_text,
                "fetch",
                "--depth",
                "1",
                "origin",
                &context.base_commit,
            ],
            &askpass,
        )
        .await?;
        git_delivery_command(
            &[
                "-C",
                checkout_text,
                "checkout",
                "--detach",
                &context.base_commit,
            ],
            &askpass,
        )
        .await?;
        let head =
            git_delivery_stdout(&["-C", checkout_text, "rev-parse", "HEAD"], &askpass).await?;
        if head != context.base_commit {
            anyhow::bail!("base_commit_mismatch");
        }
        git_delivery_command(
            &[
                "-C",
                checkout_text,
                "switch",
                "--create",
                &context.head_branch,
            ],
            &askpass,
        )
        .await?;
        let target = checkout.join(&context.kustomization_path);
        let original = tokio::fs::read_to_string(&target)
            .await
            .context("kustomization file could not be read")?;
        let updated =
            update_kustomization_image(&original, &context.image_name, &context.image_ref)?;
        tokio::fs::write(&target, updated)
            .await
            .context("kustomization file could not be updated")?;
        git_delivery_command(
            &[
                "-C",
                checkout_text,
                "add",
                "--",
                &context.kustomization_path,
            ],
            &askpass,
        )
        .await?;
        if git_delivery_command_status(
            &["-C", checkout_text, "diff", "--cached", "--quiet"],
            &askpass,
        )
        .await?
        {
            anyhow::bail!("empty_change_set");
        }
        git_delivery_command(
            &[
                "-C",
                checkout_text,
                "config",
                "user.name",
                &context.author_name,
            ],
            &askpass,
        )
        .await?;
        git_delivery_command(
            &[
                "-C",
                checkout_text,
                "config",
                "user.email",
                &context.author_email,
            ],
            &askpass,
        )
        .await?;
        git_delivery_command(
            &[
                "-C",
                checkout_text,
                "commit",
                "-m",
                &context.commit_subject,
                "-m",
                &context.commit_body,
            ],
            &askpass,
        )
        .await?;
        let commit_sha =
            git_delivery_stdout(&["-C", checkout_text, "rev-parse", "HEAD"], &askpass).await?;
        if !is_git_sha(&commit_sha) {
            anyhow::bail!("invalid_commit_sha");
        }
        git_delivery_command(
            &[
                "-C",
                checkout_text,
                "push",
                "origin",
                &format!("HEAD:refs/heads/{}", context.head_branch),
            ],
            &askpass,
        )
        .await?;
        let pull_request_context = GitDeliveryContext {
            execution_id: context.execution_id.clone(),
            repository: context.repository.clone(),
            base_ref: context.base_ref.clone(),
            base_commit: context.base_commit.clone(),
            head_branch: context.head_branch.clone(),
            diff: "gitops_kustomization_image_update".to_string(),
            commit_subject: context.commit_subject.clone(),
            commit_body: context.commit_body.clone(),
            pull_request_title: context.pull_request_title.clone(),
            pull_request_body: context.pull_request_body.clone(),
            github_api_url: context.github_api_url.clone(),
            author_name: context.author_name.clone(),
            author_email: context.author_email.clone(),
        };
        let pr =
            create_github_pull_request(client, &pull_request_context, token, &owner, &repo).await?;
        Ok(GitDeliveryCompleted {
            branch: context.head_branch.clone(),
            commit_sha,
            pull_request_url: pr.0,
            pull_request_number: pr.1,
        })
    }
    .await;
    let _ = tokio::fs::remove_dir_all(&root).await;
    result
}

fn validate_git_delivery_context(context: &GitDeliveryContext) -> anyhow::Result<()> {
    for value in [
        &context.base_ref,
        &context.base_commit,
        &context.head_branch,
        &context.commit_subject,
        &context.pull_request_title,
        &context.author_name,
        &context.author_email,
    ] {
        if value.trim().is_empty() || value.contains(['\0', '\r', '\n']) {
            anyhow::bail!("invalid_git_delivery_context");
        }
    }
    for value in [&context.commit_body, &context.pull_request_body] {
        if value.trim().is_empty() || value.contains(['\0', '\r']) {
            anyhow::bail!("invalid_git_delivery_context");
        }
    }
    if context.execution_id.trim().is_empty()
        || !is_git_sha(&context.base_commit)
        || context.diff.is_empty()
        || context.github_api_url.trim().is_empty()
    {
        anyhow::bail!("invalid_git_delivery_context");
    }
    if context.head_branch.starts_with('-')
        || context
            .head_branch
            .contains([' ', '~', '^', ':', '?', '*', '[', '\\', '\n'])
        || context.head_branch.contains("..")
    {
        anyhow::bail!("invalid_branch");
    }
    Ok(())
}

fn validate_gitops_delivery_context(context: &GitOpsDeliveryContext) -> anyhow::Result<()> {
    let delivery_context = GitDeliveryContext {
        execution_id: context.execution_id.clone(),
        repository: context.repository.clone(),
        base_ref: context.base_ref.clone(),
        base_commit: context.base_commit.clone(),
        head_branch: context.head_branch.clone(),
        diff: "gitops_kustomization_image_update".to_string(),
        commit_subject: context.commit_subject.clone(),
        commit_body: context.commit_body.clone(),
        pull_request_title: context.pull_request_title.clone(),
        pull_request_body: context.pull_request_body.clone(),
        github_api_url: context.github_api_url.clone(),
        author_name: context.author_name.clone(),
        author_email: context.author_email.clone(),
    };
    validate_git_delivery_context(&delivery_context)?;
    let path = std::path::Path::new(&context.kustomization_path);
    if context.kustomization_path.trim().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        anyhow::bail!("invalid_kustomization_path");
    }
    validate_kustomization_image_name(&context.image_name)?;
    parse_digest_pinned_image_reference(&context.image_ref)?;
    Ok(())
}

fn parse_github_repository(repository: &str) -> anyhow::Result<(String, String)> {
    let rest = repository
        .strip_prefix("https://github.com/")
        .context("repository_not_github_https")?;
    if rest.contains(['?', '#', '@']) {
        anyhow::bail!("repository_not_github_https");
    }
    let mut parts = rest.trim_end_matches(".git").split('/');
    let owner = parts
        .next()
        .filter(|value| !value.is_empty())
        .context("repository_not_github_https")?;
    let repo = parts
        .next()
        .filter(|value| !value.is_empty())
        .context("repository_not_github_https")?;
    if parts.next().is_some() || !is_github_name(owner) || !is_github_name(repo) {
        anyhow::bail!("repository_not_github_https");
    }
    Ok((owner.to_string(), repo.to_string()))
}

fn is_github_name(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}
fn is_git_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_github_pr_url(value: &str) -> bool {
    let parts = value
        .strip_prefix("https://github.com/")
        .map(|value| value.split('/').collect::<Vec<_>>());
    matches!(parts, Some(parts) if parts.len() == 4 && parts[2] == "pull" && parts[3].parse::<u64>().is_ok())
}

fn git_patch_for_apply(diff: &str) -> String {
    if diff.ends_with('\n') {
        diff.to_string()
    } else {
        format!("{diff}\n")
    }
}

async fn git_delivery_command(args: &[&str], askpass: &std::path::Path) -> anyhow::Result<()> {
    if git_delivery_command_status(args, askpass).await? {
        Ok(())
    } else {
        anyhow::bail!(git_delivery_command_error_code(args))
    }
}

async fn git_delivery_stdout(args: &[&str], askpass: &std::path::Path) -> anyhow::Result<String> {
    let output = git_delivery_output(args, askpass).await?;
    if !output.status.success() {
        anyhow::bail!(git_delivery_command_error_code(args));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_delivery_command_error_code(args: &[&str]) -> &'static str {
    for (command, error_code) in [
        ("clone", "git_clone_failed"),
        ("fetch", "git_fetch_failed"),
        ("checkout", "git_checkout_failed"),
        ("switch", "git_switch_failed"),
        ("apply", "git_apply_failed"),
        ("add", "git_add_failed"),
        ("config", "git_config_failed"),
        ("commit", "git_commit_failed"),
        ("rev-parse", "git_revision_failed"),
        ("push", "git_push_failed"),
    ] {
        if args.contains(&command) {
            return error_code;
        }
    }
    "git_command_failed"
}

async fn git_delivery_command_status(
    args: &[&str],
    askpass: &std::path::Path,
) -> anyhow::Result<bool> {
    Ok(git_delivery_output(args, askpass).await?.status.success())
}

async fn git_delivery_output(
    args: &[&str],
    askpass: &std::path::Path,
) -> anyhow::Result<std::process::Output> {
    tokio::process::Command::new("git")
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", askpass)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .await
        .context("failed to spawn Git writer command")
}

async fn create_github_pull_request(
    client: &reqwest::Client,
    context: &GitDeliveryContext,
    token: &str,
    owner: &str,
    repo: &str,
) -> anyhow::Result<(String, u64)> {
    let api = context.github_api_url.trim_end_matches('/');
    if !api.starts_with("https://") {
        anyhow::bail!("invalid_github_api_url");
    }
    let url = format!("{api}/repos/{owner}/{repo}/pulls");
    let response = client.post(&url).bearer_auth(token).header("accept", "application/vnd.github+json")
        .header("user-agent", "pharness-git-writer")
        .json(&serde_json::json!({ "title": context.pull_request_title, "head": context.head_branch, "base": context.base_ref, "body": context.pull_request_body, "maintainer_can_modify": false }))
        .send().await.context("GitHub pull request request failed")?;
    if !response.status().is_success() {
        anyhow::bail!("github_pull_request_failed");
    }
    let value: serde_json::Value = response
        .json()
        .await
        .context("GitHub pull request response was invalid")?;
    let html_url = value
        .get("html_url")
        .and_then(serde_json::Value::as_str)
        .filter(|value| value.starts_with("https://github.com/"))
        .context("GitHub pull request response lacked html_url")?;
    let number = value
        .get("number")
        .and_then(serde_json::Value::as_u64)
        .context("GitHub pull request response lacked number")?;
    Ok((html_url.to_string(), number))
}

async fn post_git_delivery_outcome(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    outcome: &serde_json::Value,
) -> anyhow::Result<()> {
    let mut last_error = None;
    for attempt in 1..=INGEST_ATTEMPTS {
        match client
            .post(url)
            .bearer_auth(token)
            .json(outcome)
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => return Ok(()),
            Ok(response) if response.status().is_client_error() => {
                anyhow::bail!("Git delivery outcome was rejected: {}", response.status())
            }
            Ok(response) => {
                last_error = Some(anyhow::anyhow!(
                    "Git delivery outcome endpoint returned {}",
                    response.status()
                ))
            }
            Err(error) => last_error = Some(error.into()),
        }
        if attempt < INGEST_ATTEMPTS {
            tokio::time::sleep(INGEST_RETRY_DELAY).await;
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Git delivery outcome could not be reported")))
}

fn git_delivery_error_code(error: &anyhow::Error) -> &'static str {
    match error.to_string().as_str() {
        "base_commit_mismatch" => "base_commit_mismatch",
        "empty_change_set" => "empty_change_set",
        "invalid_branch" => "invalid_branch",
        "repository_not_github_https" => "repository_not_github_https",
        "github_pull_request_failed" => "github_pull_request_failed",
        "git_clone_failed" => "git_clone_failed",
        "git_fetch_failed" => "git_fetch_failed",
        "git_checkout_failed" => "git_checkout_failed",
        "git_switch_failed" => "git_switch_failed",
        "git_apply_failed" => "git_apply_failed",
        "git_config_failed" => "git_config_failed",
        "git_commit_failed" => "git_commit_failed",
        "git_revision_failed" => "git_revision_failed",
        "git_push_failed" => "git_push_failed",
        "git_command_failed" => "git_command_failed",
        _ => "git_writer_failed",
    }
}

fn gitops_delivery_error_code(error: &anyhow::Error) -> &'static str {
    match error.to_string().as_str() {
        "base_commit_mismatch" => "base_commit_mismatch",
        "empty_change_set" => "empty_change_set",
        "invalid_branch" => "invalid_branch",
        "invalid_kustomization_path" => "invalid_kustomization_path",
        "kustomization image entry was not found" => "kustomization_image_not_found",
        "kustomization image entry is ambiguous" => "kustomization_image_ambiguous",
        "repository_not_github_https" => "repository_not_github_https",
        "github_pull_request_failed" => "github_pull_request_failed",
        "git_clone_failed" => "git_clone_failed",
        "git_fetch_failed" => "git_fetch_failed",
        "git_checkout_failed" => "git_checkout_failed",
        "git_switch_failed" => "git_switch_failed",
        "git_add_failed" => "git_add_failed",
        "git_config_failed" => "git_config_failed",
        "git_commit_failed" => "git_commit_failed",
        "git_revision_failed" => "git_revision_failed",
        "git_push_failed" => "git_push_failed",
        "git_command_failed" => "git_command_failed",
        _ => "gitops_writer_failed",
    }
}

fn git_observer_error_code(error: &anyhow::Error) -> &'static str {
    match error.to_string().as_str() {
        "repository_not_github_https" => "repository_not_github_https",
        "invalid_git_observation_context" => "invalid_git_observation_context",
        "github_pull_request_observation_failed" => "github_pull_request_observation_failed",
        "github_pull_request_provenance_mismatch" => "github_pull_request_provenance_mismatch",
        "github_merge_commit_missing" => "github_merge_commit_missing",
        "invalid_gitops_base_revision_context" => "invalid_gitops_base_revision_context",
        "github_base_revision_resolution_failed" => "github_base_revision_resolution_failed",
        _ => "git_observer_failed",
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

/// Ensure the attempt workspace exists. The only remote source instructions
/// accepted here came from the typed API-issued attempt context; ambient
/// repository environment variables are intentionally ignored.
struct ProvisionedWorkspace {
    resolved_commit: String,
}

async fn prepare_workspace(spec: &AttemptSpec) -> anyhow::Result<Option<ProvisionedWorkspace>> {
    let cwd = std::path::PathBuf::from(&spec.run.cwd);
    tokio::fs::create_dir_all(&cwd)
        .await
        .with_context(|| format!("failed to create workspace {}", cwd.display()))?;

    let Some(source) = spec.run.workspace_source.as_ref() else {
        return Ok(None);
    };
    source.validate()?;

    if spec.resume.is_some() || source.resolved_commit.is_some() {
        let cwd_text = cwd.to_string_lossy().to_string();
        let resolved_commit = workspace_git_stdout(
            &cwd,
            &["-C", &cwd_text, "rev-parse", "--verify", "HEAD^{commit}"],
        )
        .await
        .context("durable workspace is missing its pinned Git checkout")?;
        let branch = workspace_git_stdout(&cwd, &["-C", &cwd_text, "branch", "--show-current"])
            .await
            .context("durable workspace branch could not be inspected")?;
        validate_resumed_workspace_identity(source, &resolved_commit, &branch)?;
        if spec.resume.is_none()
            && spec
                .run
                .execution_target_json
                .get("environment_snapshot")
                .is_some()
            && !cwd.join(".pharness-runtime/venv/bin/python").is_file()
        {
            anyhow::bail!("prepared workspace is missing its durable Python environment");
        }
        return Ok(None);
    }

    let mut entries = tokio::fs::read_dir(&cwd).await?;
    if entries.next_entry().await?.is_some() {
        anyhow::bail!("typed workspace must be empty before clone");
    }

    let resolved_commit = clone_workspace_source(source, &cwd).await?;

    Ok(Some(ProvisionedWorkspace { resolved_commit }))
}

fn validate_resumed_workspace_identity(
    source: &WorkspaceSourceSpec,
    resolved_commit: &str,
    branch: &str,
) -> anyhow::Result<()> {
    let expected_commit = source
        .resolved_commit
        .as_deref()
        .or(source.source_commit.as_deref())
        .ok_or_else(|| anyhow::anyhow!("resumed workspace has no immutable base commit"))?;
    if resolved_commit != expected_commit {
        anyhow::bail!(
            "durable workspace HEAD {resolved_commit} does not match pinned base {expected_commit}"
        );
    }
    if branch != source.branch {
        anyhow::bail!(
            "durable workspace branch {branch:?} does not match issued branch {:?}",
            source.branch
        );
    }
    Ok(())
}

async fn clone_workspace_source(
    source: &WorkspaceSourceSpec,
    cwd: &std::path::Path,
) -> anyhow::Result<String> {
    tracing::info!(workspace_id = %source.workspace_id, cwd = %cwd.display(), "cloning typed workspace source");
    let cwd_text = cwd.to_string_lossy().to_string();
    let checkout_ref = source
        .source_commit
        .as_deref()
        .unwrap_or(source.source_ref.as_str());
    run_workspace_git(
        cwd,
        &[
            "clone",
            "--no-checkout",
            "--depth",
            "1",
            &source.source_repo,
            &cwd_text,
        ],
    )
    .await?;
    run_workspace_git(
        cwd,
        &[
            "-C",
            &cwd_text,
            "fetch",
            "--depth",
            "1",
            "origin",
            checkout_ref,
        ],
    )
    .await?;
    run_workspace_git(cwd, &["-C", &cwd_text, "switch", "--detach", "FETCH_HEAD"]).await?;
    run_workspace_git(
        cwd,
        &["-C", &cwd_text, "switch", "--create", &source.branch],
    )
    .await?;
    let resolved_commit = workspace_git_stdout(
        cwd,
        &["-C", &cwd_text, "rev-parse", "--verify", "HEAD^{commit}"],
    )
    .await?;
    if let Some(expected) = source.source_commit.as_deref() {
        if resolved_commit != expected {
            anyhow::bail!(
                "workspace source resolved commit {resolved_commit} does not match requested immutable commit {expected}"
            );
        }
    }
    Ok(resolved_commit)
}

/// Execute Git only against the API-issued workspace. Kubernetes volume roots
/// can be owned by kubelet even when the worker runs as a non-root UID; scope
/// Git's safe-directory exception to this one isolated workspace.
async fn run_workspace_git(cwd: &std::path::Path, args: &[&str]) -> anyhow::Result<()> {
    workspace_git_output(cwd, args).await.map(|_| ())
}

async fn workspace_git_stdout(cwd: &std::path::Path, args: &[&str]) -> anyhow::Result<String> {
    let output = workspace_git_output(cwd, args).await?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

async fn workspace_git_output(
    cwd: &std::path::Path,
    args: &[&str],
) -> anyhow::Result<std::process::Output> {
    let command_args = workspace_git_args(cwd, args);
    let output = tokio::process::Command::new("git")
        .args(&command_args)
        .output()
        .await
        .context("failed to spawn typed Git workspace command")?;
    if !output.status.success() {
        anyhow::bail!(workspace_git_failure_summary(&output, args))
    }
    Ok(output)
}

fn workspace_git_args(cwd: &std::path::Path, args: &[&str]) -> Vec<String> {
    let mut command_args = vec![
        "-c".to_string(),
        format!("safe.directory={}", cwd.display()),
    ];
    command_args.extend(args.iter().map(|arg| (*arg).to_string()));
    command_args
}

fn workspace_git_failure_summary(output: &std::process::Output, args: &[&str]) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    if stderr.contains("dubious ownership") {
        return "Git rejected workspace ownership before clone".to_string();
    }
    if stderr.contains("could not resolve host") {
        return "Git could not resolve the configured repository host".to_string();
    }
    if stderr.contains("authentication failed") || stderr.contains("could not read username") {
        return "Git repository authentication failed".to_string();
    }

    let operation = args
        .iter()
        .copied()
        .find(|arg| !matches!(*arg, "-C") && !arg.starts_with('/'))
        .unwrap_or("operation");
    let exit_code = output.status.code().unwrap_or(-1);
    format!("Git {operation} failed with exit code {exit_code}")
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

    async fn report_workspace_provisioned(
        &self,
        workspace_id: &str,
        resolved_commit: &str,
        branch: &str,
    ) -> anyhow::Result<()> {
        self.post_json_with_retry(
            "workspace-provisioned",
            &serde_json::json!({
                "workspace_id": workspace_id,
                "resolved_commit": resolved_commit,
                "branch": branch,
            }),
        )
        .await
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
    use super::{
        allowlisted_connect_target, argo_application_terminal, argo_sync_patch_payload,
        fetch_internal_context, git_delivery_command_error_code, git_patch_for_apply,
        parse_github_repository, pipeline_run_terminal, update_kustomization_image,
        validate_git_delivery_context, validate_resumed_workspace_identity, workspace_git_args,
        ArgoApplicationTerminal, GitDeliveryContext, PipelineRunTerminal,
    };
    use pharness_runhost::WorkspaceSourceSpec;
    use serde_json::json;
    use std::path::Path;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn retries_transient_internal_context_fetches_for_fresh_executor_pods() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for response in [
                None,
                Some("HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()),
                Some({
                    let body = r#"{"execution_id":"gexec_retry"}"#;
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                }),
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0_u8; 1024];
                let _ = stream.read(&mut request).await.unwrap();
                if let Some(response) = response {
                    stream.write_all(response.as_bytes()).await.unwrap();
                }
            }
        });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();

        let context: serde_json::Value = fetch_internal_context(
            &client,
            &format!("http://{address}/context"),
            "worker-token",
            3,
            Duration::from_millis(1),
        )
        .await
        .unwrap();

        assert_eq!(context["execution_id"], "gexec_retry");
        server.await.unwrap();
    }

    #[test]
    fn connect_proxy_accepts_only_exact_allowlisted_https_hosts() {
        let allowed = ["api.fireworks.ai".to_string()].into_iter().collect();
        assert_eq!(
            allowlisted_connect_target(
                "CONNECT api.fireworks.ai:443 HTTP/1.1\r\nHost: api.fireworks.ai:443\r\n\r\n",
                &allowed,
            )
            .unwrap(),
            ("api.fireworks.ai".to_string(), 443)
        );
        assert_eq!(
            allowlisted_connect_target("CONNECT api.fireworks.ai:443 HTTP/1.0\r\n\r\n", &allowed,)
                .unwrap(),
            ("api.fireworks.ai".to_string(), 443)
        );
        assert!(
            allowlisted_connect_target("CONNECT evil.example:443 HTTP/1.1\r\n\r\n", &allowed,)
                .is_err()
        );
        assert!(allowlisted_connect_target(
            "GET https://api.fireworks.ai/ HTTP/1.1\r\n\r\n",
            &allowed,
        )
        .is_err());
        assert!(allowlisted_connect_target(
            "CONNECT api.fireworks.ai:80 HTTP/1.1\r\n\r\n",
            &allowed,
        )
        .is_err());
        assert!(allowlisted_connect_target(
            "CONNECT api.fireworks.ai:443 HTTP/2\r\n\r\n",
            &allowed,
        )
        .is_err());
    }

    #[test]
    fn workspace_git_commands_scope_safe_directory_to_the_issued_workspace() {
        assert_eq!(
            workspace_git_args(
                Path::new("/workspace"),
                &[
                    "-C",
                    "/workspace",
                    "fetch",
                    "--depth",
                    "1",
                    "origin",
                    "main"
                ],
            ),
            vec![
                "-c",
                "safe.directory=/workspace",
                "-C",
                "/workspace",
                "fetch",
                "--depth",
                "1",
                "origin",
                "main",
            ]
        );
    }

    #[test]
    fn resumed_workspace_must_match_the_pinned_commit_and_branch() {
        let source = WorkspaceSourceSpec {
            workspace_id: "ws_123".to_string(),
            source_repo: "https://github.com/example/repo.git".to_string(),
            source_ref: "main".to_string(),
            source_commit: Some("a".repeat(40)),
            branch: "pharness/witem_123/attempt-1".to_string(),
            resolved_commit: Some("a".repeat(40)),
        };

        validate_resumed_workspace_identity(
            &source,
            &"a".repeat(40),
            "pharness/witem_123/attempt-1",
        )
        .unwrap();
        assert!(validate_resumed_workspace_identity(
            &source,
            &"b".repeat(40),
            "pharness/witem_123/attempt-1",
        )
        .unwrap_err()
        .to_string()
        .contains("does not match pinned base"));
        assert!(
            validate_resumed_workspace_identity(&source, &"a".repeat(40), "other")
                .unwrap_err()
                .to_string()
                .contains("does not match issued branch")
        );
    }

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

    #[test]
    fn recognizes_only_a_successful_synced_argo_operation_as_terminal_success() {
        let application = json!({
            "status": {
                "sync": { "status": "Synced" },
                "health": { "status": "Progressing" },
                "operationState": { "phase": "Succeeded", "syncResult": { "revision": "abc123" } }
            }
        });
        match argo_application_terminal(&application, None) {
            ArgoApplicationTerminal::Succeeded(state) => {
                assert_eq!(state.sync_status.as_deref(), Some("Synced"));
                assert_eq!(state.operation_phase.as_deref(), Some("Succeeded"));
                assert_eq!(state.revision.as_deref(), Some("abc123"));
            }
            _ => panic!("expected successful Argo terminal state"),
        }

        let not_synced = json!({
            "status": {
                "sync": { "status": "OutOfSync" },
                "operationState": { "phase": "Succeeded" }
            }
        });
        assert!(matches!(
            argo_application_terminal(&not_synced, None),
            ArgoApplicationTerminal::Pending
        ));
    }

    #[test]
    fn recognizes_failed_argo_operation_without_returning_raw_resource_text() {
        let application = json!({
            "status": {
                "sync": { "status": "OutOfSync" },
                "operationState": { "phase": "Failed" }
            }
        });
        match argo_application_terminal(&application, None) {
            ArgoApplicationTerminal::Failed(state) => {
                assert_eq!(state.operation_phase.as_deref(), Some("Failed"));
                assert_eq!(state.health_status, None);
            }
            _ => panic!("expected failed Argo terminal state"),
        }
    }

    #[test]
    fn argo_sync_patch_is_minimal_and_never_requests_prune_or_force_true() {
        let patch = argo_sync_patch_payload(Some("0123456789abcdef0123456789abcdef01234567"));
        assert_eq!(patch.pointer("/operation/sync/prune"), Some(&json!(false)));
        assert!(patch.pointer("/operation/sync/force").is_none());
        assert!(patch.pointer("/spec").is_none());
        assert_eq!(
            patch.pointer("/operation/sync/revision"),
            Some(&json!("0123456789abcdef0123456789abcdef01234567"))
        );
    }

    #[test]
    fn git_writer_accepts_only_a_safe_github_repository_and_context() {
        assert_eq!(
            parse_github_repository("https://github.com/example/finance-app.git").unwrap(),
            ("example".to_string(), "finance-app".to_string())
        );
        assert!(
            parse_github_repository("https://token@github.com/example/finance-app.git").is_err()
        );
        let context = GitDeliveryContext {
            execution_id: "gexec_1".to_string(),
            repository: "https://github.com/example/finance-app.git".to_string(),
            base_ref: "main".to_string(),
            base_commit: "a".repeat(40),
            head_branch: "pharness/work-item-1".to_string(),
            diff: "diff --git a/a b/a\n".to_string(),
            commit_subject: "Change".to_string(),
            commit_body: "Body\n\nPharness ChangeSet: cset_1".to_string(),
            pull_request_title: "Change".to_string(),
            pull_request_body: "Body\n\nWorkItem: witem_1".to_string(),
            github_api_url: "https://api.github.com".to_string(),
            author_name: "Pharness".to_string(),
            author_email: "pharness@example.test".to_string(),
        };
        assert!(validate_git_delivery_context(&context).is_ok());

        let mut invalid = context;
        invalid.commit_subject = "Change\nInjected trailer".to_string();
        assert!(validate_git_delivery_context(&invalid).is_err());
    }

    #[test]
    fn git_writer_reports_only_the_failed_typed_git_stage() {
        assert_eq!(
            git_delivery_command_error_code(&[
                "-C",
                "/work/git-gexec/repo",
                "apply",
                "--index",
                "/work/change.patch",
            ]),
            "git_apply_failed"
        );
        assert_eq!(
            git_delivery_command_error_code(&[
                "-C",
                "/work/git-gexec/repo",
                "push",
                "origin",
                "HEAD:refs/heads/pharness/work-item-1",
            ]),
            "git_push_failed"
        );
        assert_eq!(
            git_delivery_command_error_code(&["-C", "/work/git-gexec/repo", "status"]),
            "git_command_failed"
        );
    }

    #[test]
    fn git_writer_repairs_only_a_missing_patch_terminator() {
        assert_eq!(
            git_patch_for_apply("diff --git a/a b/a"),
            "diff --git a/a b/a\n"
        );
        assert_eq!(
            git_patch_for_apply("diff --git a/a b/a\n"),
            "diff --git a/a b/a\n"
        );
    }

    #[test]
    fn updates_exactly_one_declared_kustomization_image_with_an_immutable_digest() {
        const DIGEST: &str =
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let output = update_kustomization_image(
            "apiVersion: kustomize.config.k8s.io/v1beta1\nkind: Kustomization\nimages:\n  - name: registry.example.test/team/api\n    newTag: old\n  - name: registry.example.test/team/worker\n    newName: registry.example.test/team/worker\n    digest: sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
            "registry.example.test/team/api",
            &format!("registry.dev.example/team/api@{DIGEST}"),
        )
        .unwrap();

        let document: serde_yaml::Value = serde_yaml::from_str(&output).unwrap();
        let images = document
            .get("images")
            .and_then(serde_yaml::Value::as_sequence)
            .unwrap();
        let api = images[0].as_mapping().unwrap();
        assert_eq!(
            api.get("name").and_then(serde_yaml::Value::as_str),
            Some("registry.example.test/team/api")
        );
        assert_eq!(
            api.get("newName").and_then(serde_yaml::Value::as_str),
            Some("registry.dev.example/team/api")
        );
        assert_eq!(
            api.get("digest").and_then(serde_yaml::Value::as_str),
            Some(DIGEST)
        );
        assert!(!api.contains_key("newTag"));
        assert_eq!(
            images[1].get("digest").and_then(serde_yaml::Value::as_str),
            Some("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
    }

    #[test]
    fn rejects_missing_or_ambiguous_kustomization_image_entries() {
        const DIGEST: &str =
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let missing = update_kustomization_image(
            "images:\n  - name: registry.example.test/team/other\n",
            "registry.example.test/team/api",
            &format!("registry.example.test/team/api@{DIGEST}"),
        )
        .unwrap_err();
        assert!(missing.to_string().contains("not found"));

        let ambiguous = update_kustomization_image(
            "images:\n  - name: registry.example.test/team/api\n  - name: registry.example.test/team/api\n",
            "registry.example.test/team/api",
            &format!("registry.example.test/team/api@{DIGEST}"),
        )
        .unwrap_err();
        assert!(ambiguous.to_string().contains("ambiguous"));
    }

    #[test]
    fn rejects_non_digest_pinned_kustomization_image_references() {
        let error = update_kustomization_image(
            "images:\n  - name: registry.example.test/team/api\n",
            "registry.example.test/team/api",
            "registry.example.test/team/api:latest",
        )
        .unwrap_err();

        assert!(error.to_string().contains("digest pinned"));
    }
}
