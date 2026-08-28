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
    discover_repository, AgentAction, AgentEvent, CancellationFlag, EnvironmentRuntimeSnapshot,
    EnvironmentSnapshot, PreparationStrategy, ReadOnlyClusterTools, RepositoryContract,
    RepositoryContractSource, RepositoryDiscoveryIdentity, RepositoryDiscoveryLimits, ToolExecutor,
};
use pharness_fireworks::{FireworksClient, FireworksProviderConfig};
use pharness_runhost::{
    execute_attempt, AttemptBackend, AttemptHost, AttemptOutcome, AttemptSpec, WorkspaceSourceSpec,
};
use serde::de::DeserializeOwned;
use serde_yaml::Value as YamlValue;
use sha2::{Digest, Sha256};
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
    let (desired_reference, digest) = parse_digest_pinned_image_reference(image_ref)?;
    let desired_repository = image_repository_without_optional_tag(&desired_reference)?;
    if desired_repository != image_name {
        anyhow::bail!("kustomization image repository does not match the declared image name");
    }
    let document: YamlValue =
        serde_yaml::from_str(source).context("kustomization document is not valid YAML")?;
    let root = document
        .as_mapping()
        .context("kustomization document must be a YAML mapping")?;
    let images_key = YamlValue::String("images".to_string());
    let images = root
        .get(&images_key)
        .and_then(YamlValue::as_sequence)
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
        .as_mapping()
        .context("kustomization image entry must be a mapping")?;
    if entry
        .get(YamlValue::String("newName".to_string()))
        .and_then(YamlValue::as_str)
        .is_some_and(|new_name| new_name != image_name)
    {
        anyhow::bail!("kustomization image newName does not match the declared image name");
    }
    if entry.contains_key(YamlValue::String("newTag".to_string())) {
        anyhow::bail!("kustomization image entry must not contain newTag");
    }
    let existing_digest = entry
        .get(YamlValue::String("digest".to_string()))
        .and_then(YamlValue::as_str)
        .filter(|value| valid_sha256_digest(value))
        .context("kustomization image entry must already contain an immutable sha256 digest")?;
    let (digest_start, digest_end) =
        standard_kustomization_digest_span(source, image_name, existing_digest)?;

    // Preserve the reviewed Kustomization byte-for-byte except for the one
    // immutable digest scalar. Re-serializing YAML creates unrelated formatting
    // changes, and copying the convenience tag into `newName` broadens the
    // approved GitOps mutation beyond a digest-only promotion.
    let mut updated = String::with_capacity(source.len());
    updated.push_str(&source[..digest_start]);
    updated.push_str(&digest);
    updated.push_str(&source[digest_end..]);
    Ok(updated)
}

fn standard_kustomization_digest_span(
    source: &str,
    image_name: &str,
    existing_digest: &str,
) -> anyhow::Result<(usize, usize)> {
    let mut lines = Vec::new();
    let mut offset = 0;
    for raw_line in source.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let line = line.strip_suffix('\r').unwrap_or(line);
        let trimmed = line.trim_start_matches(' ');
        if trimmed.starts_with('\t') {
            anyhow::bail!("kustomization image entry must use standard space indentation");
        }
        lines.push((offset, line, line.len() - trimmed.len(), trimmed));
        offset += raw_line.len();
    }
    if source.is_empty() {
        anyhow::bail!("kustomization image entry was not found in standard YAML form");
    }

    let matching_entries = lines
        .iter()
        .enumerate()
        .filter_map(|(index, (_, _, indent, trimmed))| {
            trimmed
                .strip_prefix("- ")
                .and_then(|value| value.strip_prefix("name:"))
                .map(str::trim)
                .filter(|value| *value == image_name)
                .map(|_| (index, *indent))
        })
        .collect::<Vec<_>>();
    let (entry_index, entry_indent) = match matching_entries.as_slice() {
        [(index, indent)] => (*index, *indent),
        [] => anyhow::bail!("kustomization image entry was not found in standard YAML form"),
        _ => anyhow::bail!("kustomization image entry is ambiguous in source text"),
    };

    let mut digest_spans = Vec::new();
    for (line_offset, line, indent, trimmed) in lines.iter().skip(entry_index + 1) {
        if !trimmed.is_empty()
            && (*indent < entry_indent || (*indent == entry_indent && trimmed.starts_with("- ")))
        {
            break;
        }
        let Some(value) = trimmed.strip_prefix("digest:") else {
            continue;
        };
        if *indent <= entry_indent || !value.contains(existing_digest) {
            continue;
        }
        let local_start = line
            .find(existing_digest)
            .expect("digest presence checked above");
        digest_spans.push((
            line_offset + local_start,
            line_offset + local_start + existing_digest.len(),
        ));
    }
    match digest_spans.as_slice() {
        [span] => Ok(*span),
        [] => anyhow::bail!("kustomization image digest was not found in standard YAML form"),
        _ => anyhow::bail!("kustomization image digest occurrence is ambiguous"),
    }
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

fn image_repository_without_optional_tag(reference: &str) -> anyhow::Result<&str> {
    let last_slash = reference.rfind('/');
    let last_colon = reference.rfind(':');
    if last_colon.is_some_and(|colon| match last_slash {
        Some(slash) => colon > slash,
        None => true,
    }) {
        let colon = last_colon.expect("checked above");
        if colon + 1 == reference.len() {
            anyhow::bail!("image reference contains an empty tag");
        }
        Ok(&reference[..colon])
    } else {
        Ok(reference)
    }
}

fn valid_sha256_digest(value: &str) -> bool {
    value.starts_with("sha256:")
        && value.len() == "sha256:".len() + 64
        && value["sha256:".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
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

    if std::env::var("PHARNESS_EXECUTION_KIND").ok().as_deref() == Some("repository_discovery") {
        return execute_repository_discovery().await;
    }

    if std::env::var("PHARNESS_EXECUTION_KIND").ok().as_deref() == Some("onboarding_patch") {
        return execute_onboarding_patch().await;
    }

    if std::env::var("PHARNESS_EXECUTION_KIND").ok().as_deref()
        == Some("onboarding_contract_validate")
    {
        return execute_onboarding_contract_validation().await;
    }

    if std::env::var("PHARNESS_EXECUTION_KIND").ok().as_deref() == Some("repository_readiness") {
        return execute_repository_readiness().await;
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
    if std::env::var("PHARNESS_EXECUTION_KIND").ok().as_deref()
        == Some("source_observer_capability_preflight")
    {
        return execute_source_observer_capability_preflight().await;
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

#[derive(Debug, serde::Deserialize)]
struct RepositoryDiscoveryContext {
    discovery_id: String,
    onboarding_id: String,
    repository_id: String,
    provider: String,
    canonical_url: String,
    default_branch: String,
    source_commit: String,
    limits: RepositoryDiscoveryLimits,
}

#[derive(Debug, serde::Deserialize)]
struct OnboardingPatchContext {
    onboarding_id: String,
    execution_id: String,
    repository_id: String,
    provider: String,
    canonical_url: String,
    default_branch: String,
    source_commit: String,
    proposal_id: String,
    proposal_hash: String,
    candidate_contract: serde_json::Value,
    instructions: String,
    remove_alias: bool,
}

#[derive(Debug, serde::Deserialize)]
struct OnboardingContractValidationContext {
    onboarding_id: String,
    execution_id: String,
    repository_id: String,
    provider: String,
    canonical_url: String,
    source_commit: String,
    proposal_id: String,
    proposal_hash: String,
    expected_contract: serde_json::Value,
}

#[derive(Debug, serde::Deserialize)]
struct RepositoryReadinessContext {
    preparation_id: String,
    workspace_id: String,
    repository_id: String,
    provider: String,
    canonical_url: String,
    default_branch: String,
    source_commit: String,
    contract_version_id: String,
    contract_content_hash: String,
    contract: serde_json::Value,
    environment_profile_id: String,
}

async fn execute_repository_readiness() -> anyhow::Result<()> {
    let api_url = required_env("PHARNESS_API_URL")?
        .trim_end_matches('/')
        .to_string();
    let preparation_id = required_env("PHARNESS_REPOSITORY_PREPARATION_ID")?;
    let worker_token = required_env("PHARNESS_WORKER_TOKEN")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("failed to build repository readiness client")?;
    let context = fetch_internal_context_with_retry::<RepositoryReadinessContext>(
        &client,
        &format!(
            "{api_url}/api/internal/repository-readiness-preparations/{preparation_id}/context"
        ),
        &worker_token,
    )
    .await
    .context("failed to fetch repository readiness context")?;
    if context.preparation_id != preparation_id
        || context.provider != "github"
        || !is_git_sha(&context.source_commit)
        || context.repository_id.trim().is_empty()
        || context.default_branch.trim().is_empty()
        || context.environment_profile_id != required_env("PHARNESS_ENVIRONMENT_PROFILE_ID")?
    {
        anyhow::bail!("invalid_repository_readiness_context");
    }
    let outcome = match prepare_repository_readiness(&context, &worker_token).await {
        Ok(outcome) => outcome,
        Err(error) => {
            let code = repository_readiness_error_code(&error);
            tracing::warn!(
                preparation_id,
                error_code = code,
                "repository readiness preparation failed"
            );
            serde_json::json!({"status":"failed","error_code":code,"logs":[{"step":"readiness","status":"failed","summary":code}]})
        }
    };
    post_internal_json_with_retry(
        &client,
        &format!(
            "{api_url}/api/internal/repository-readiness-preparations/{preparation_id}/outcome"
        ),
        &worker_token,
        &outcome,
    )
    .await
}

async fn prepare_repository_readiness(
    context: &RepositoryReadinessContext,
    worker_token: &str,
) -> anyhow::Result<serde_json::Value> {
    parse_github_repository(&context.canonical_url)?;
    let expected: RepositoryContract =
        serde_json::from_value(context.contract.clone()).context("invalid_expected_contract")?;
    expected
        .validate_candidate()
        .map_err(|_| anyhow::anyhow!("invalid_expected_contract"))?;
    let root = std::path::PathBuf::from("/work/repository");
    checkout_exact_repository(&root, &context.canonical_url, &context.source_commit).await?;
    let loaded = RepositoryContract::load_for_repo_mode(&root)
        .map_err(|error| anyhow::anyhow!("repository_contract_invalid:{error}"))?;
    let loaded_hash = format!("sha256:{}", loaded.content_sha256);
    if loaded.contract != expected || loaded_hash != context.contract_content_hash {
        anyhow::bail!("repository_contract_provenance_mismatch");
    }
    let profile_id = required_env("PHARNESS_ENVIRONMENT_PROFILE_ID")?;
    if loaded.contract.environment_profile != profile_id {
        anyhow::bail!("runner_profile_mismatch");
    }
    let required_executables =
        serde_json::from_str::<Vec<String>>(&required_env("PHARNESS_REQUIRED_EXECUTABLES_JSON")?)
            .context("required_executable_inventory_invalid")?;
    let mut executable_paths = serde_json::Map::new();
    for executable in &required_executables {
        executable_paths.insert(
            executable.clone(),
            serde_json::Value::String(executable_path(executable).await?),
        );
    }
    let prepared = prepare_declared_runtime(
        &root,
        &loaded.contract,
        &executable_paths,
        configured_preparation_strategy()?,
    )
    .await?;
    let effective_user = run_output(&root, "id", &["-u"]).await?;
    let mut unavailable_tools = Vec::new();
    for executable in ["docker", "podman", "apt", "apt-get", "apk"] {
        if executable_path_optional(executable).await.is_none() {
            unavailable_tools.push(executable.into());
        }
    }
    let snapshot = EnvironmentSnapshot {
        source_sha: context.source_commit.clone(),
        manifest_sha256: loaded_hash.clone(),
        dependency_lock_sha256: loaded.contract.dependency_lock.sha256.clone(),
        runner_image_digest: required_env("PHARNESS_RUNNER_IMAGE")?,
        runner_revision: required_env("PHARNESS_RUNNER_REVISION")?,
        os: std::env::consts::OS.into(),
        architecture: std::env::consts::ARCH.into(),
        effective_user,
        runtime: Some(prepared.runtime.clone()),
        python_version: prepared.python_version.clone(),
        python_path: prepared.python_path.clone(),
        writable_paths: loaded.contract.writable_paths.clone(),
        unavailable_tools,
        agent_network: loaded.contract.agent_network,
        package_installation: loaded.contract.package_installation,
        acceptance_commands: loaded.contract.acceptance_commands.clone(),
        preparation_evidence: serde_json::json!({
            "required_executables":executable_paths,
            "runtime":prepared.evidence,
            "platform":required_env("PHARNESS_RUNNER_PLATFORM")?,
            "workspace_id":context.workspace_id,
            "contract_version_id":context.contract_version_id,
        }),
    };
    let inherited_path = std::env::var("PATH").unwrap_or_default();
    let readiness_path = effective_runtime_path(&prepared.runtime.path_entries, &inherited_path);
    let mut acceptance_results = Vec::new();
    for command in &loaded.contract.acceptance_commands {
        let output = Command::new("/bin/sh")
            .args(["-c", &command.command])
            .current_dir(&root)
            .env("PATH", &readiness_path)
            .envs(acceptance_environment(
                &root,
                &loaded.contract,
                &prepared.runtime,
            ))
            .output()
            .await
            .context("acceptance_command_structurally_unexecutable")?;
        if acceptance_command_is_structurally_unexecutable(
            &command.command,
            output.status.code(),
            &output.stderr,
        ) {
            anyhow::bail!(
                "acceptance_command_structurally_unexecutable:{}",
                command.name
            );
        }
        acceptance_results.push(serde_json::json!({
            "name":command.name,
            "command":command.command,
            "status":if output.status.success() {"passed"} else {"baseline_failed"},
            "exit_code":output.status.code(),
            "stdout":bounded_output(&output.stdout),
            "stderr":bounded_output(&output.stderr),
        }));
    }
    let snapshot_json = serde_json::to_value(&snapshot)?;
    Ok(serde_json::json!({
        "status":"succeeded",
        "resolved_commit":context.source_commit,
        "repository_contract":loaded.contract,
        "repository_contract_hash":loaded_hash,
        "environment_snapshot":snapshot_json,
        "snapshot_signature":signed_payload(worker_token,&snapshot_json),
        "acceptance_results":acceptance_results,
        "logs":[
            {"step":"checkout","status":"succeeded","source_sha":context.source_commit},
            {"step":"contract","status":"succeeded","contract_version_id":context.contract_version_id},
            {"step":"executables","status":"succeeded","inventory":required_executables},
            {"step":"dependencies","status":"succeeded","lock_sha256":snapshot.dependency_lock_sha256},
            {"step":"acceptance","status":"executed","commands":acceptance_results.len()},
        ],
    }))
}

fn acceptance_command_is_structurally_unexecutable(
    command: &str,
    exit_code: Option<i32>,
    stderr: &[u8],
) -> bool {
    if matches!(exit_code, Some(126 | 127)) {
        return true;
    }

    let words = command.split_ascii_whitespace().collect::<Vec<_>>();
    let Some([python, "-m", module, ..]) = words.get(..) else {
        return false;
    };
    let executable = python.rsplit('/').next().unwrap_or(python);
    if !matches!(executable, "python" | "python3" | "python3.11") {
        return false;
    }
    let missing_module = format!("No module named {module}");
    String::from_utf8_lossy(stderr)
        .lines()
        .any(|line| line.trim_end().ends_with(&missing_module))
}

fn bounded_output(bytes: &[u8]) -> String {
    const LIMIT: usize = 16 * 1024;
    let end = bytes.len().min(LIMIT);
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

fn repository_readiness_error_code(error: &anyhow::Error) -> &'static str {
    let text = error.to_string();
    for code in [
        "resolved_commit_mismatch",
        "invalid_expected_contract",
        "repository_contract_provenance_mismatch",
        "runner_profile_mismatch",
        "required_executable_inventory_invalid",
        "python_executable_missing",
        "acceptance_command_structurally_unexecutable",
        "git_fetch_failed",
        "git_checkout_failed",
        "git_revision_failed",
        "git_setup_failed",
        "repository_not_github_https",
    ] {
        if text.contains(code) {
            return code;
        }
    }
    if text.contains("dependency lock SHA-256") {
        "immutable_dependency_lock_mismatch"
    } else if text.contains("dependency lock") {
        "immutable_dependency_lock_invalid"
    } else if text.contains("runner is missing required executable") {
        "required_executable_missing"
    } else if text.contains("repository_contract_invalid") {
        "repository_contract_invalid"
    } else {
        "repository_readiness_failed"
    }
}

async fn execute_onboarding_contract_validation() -> anyhow::Result<()> {
    let api_url = required_env("PHARNESS_API_URL")?
        .trim_end_matches('/')
        .to_string();
    let onboarding_id = required_env("PHARNESS_REPOSITORY_ONBOARDING_ID")?;
    let execution_id = required_env("PHARNESS_ONBOARDING_VALIDATION_EXECUTION_ID")?;
    let worker_token = required_env("PHARNESS_WORKER_TOKEN")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("failed to build onboarding contract validation client")?;
    let context_url = format!(
        "{api_url}/api/internal/repository-onboardings/{onboarding_id}/contract-validation-context?execution_id={execution_id}"
    );
    let context = fetch_internal_context_with_retry::<OnboardingContractValidationContext>(
        &client,
        &context_url,
        &worker_token,
    )
    .await
    .context("failed to fetch onboarding contract validation context")?;
    if context.onboarding_id != onboarding_id
        || context.execution_id != execution_id
        || context.provider != "github"
        || !is_git_sha(&context.source_commit)
    {
        anyhow::bail!("invalid_onboarding_contract_validation_context");
    }
    let outcome = match validate_merged_onboarding_contract(&context).await {
        Ok(loaded) => serde_json::json!({
            "status":"succeeded",
            "contract":loaded.contract,
            "contract_content_hash":format!("sha256:{}", loaded.content_sha256),
            "contract_source":loaded.source,
            "warnings":loaded.warnings,
        }),
        Err(error) => {
            let code = onboarding_contract_validation_error_code(&error);
            tracing::warn!(
                onboarding_id,
                execution_id,
                error_code = code,
                "merged onboarding contract validation failed"
            );
            serde_json::json!({"status":"failed","error_code":code})
        }
    };
    post_internal_json_with_retry(
        &client,
        &format!("{api_url}/api/internal/repository-onboardings/{onboarding_id}/contract-validation-outcome"),
        &worker_token,
        &outcome,
    )
    .await
}

async fn validate_merged_onboarding_contract(
    context: &OnboardingContractValidationContext,
) -> anyhow::Result<pharness_core::LoadedRepositoryContract> {
    parse_github_repository(&context.canonical_url)?;
    let expected: RepositoryContract = serde_json::from_value(context.expected_contract.clone())
        .context("invalid_expected_contract")?;
    expected
        .validate_candidate()
        .map_err(|_| anyhow::anyhow!("invalid_expected_contract"))?;
    let root = std::path::PathBuf::from("/work/repository");
    checkout_exact_repository(&root, &context.canonical_url, &context.source_commit).await?;
    let loaded = RepositoryContract::load_for_repo_mode(&root)
        .map_err(|error| anyhow::anyhow!("merged_contract_invalid:{error}"))?;
    if loaded.contract != expected {
        anyhow::bail!("merged_contract_differs_from_approved_proposal");
    }
    if !matches!(
        loaded.source,
        RepositoryContractSource::Canonical | RepositoryContractSource::CanonicalWithMatchingAlias
    ) {
        anyhow::bail!("canonical_repository_contract_missing");
    }
    let _ = (
        &context.repository_id,
        &context.proposal_id,
        &context.proposal_hash,
    );
    Ok(loaded)
}

fn onboarding_contract_validation_error_code(error: &anyhow::Error) -> &'static str {
    let text = error.to_string();
    for code in [
        "resolved_commit_mismatch",
        "invalid_expected_contract",
        "merged_contract_differs_from_approved_proposal",
        "canonical_repository_contract_missing",
        "git_fetch_failed",
        "git_checkout_failed",
        "git_revision_failed",
        "git_setup_failed",
        "repository_not_github_https",
    ] {
        if text.contains(code) {
            return code;
        }
    }
    if text.contains("dependency lock SHA-256") {
        "immutable_dependency_lock_mismatch"
    } else if text.contains("dependency lock") {
        "immutable_dependency_lock_invalid"
    } else if text.contains("merged_contract_invalid") {
        "merged_contract_invalid"
    } else {
        "onboarding_contract_validation_failed"
    }
}

async fn execute_onboarding_patch() -> anyhow::Result<()> {
    let api_url = required_env("PHARNESS_API_URL")?
        .trim_end_matches('/')
        .to_string();
    let onboarding_id = required_env("PHARNESS_REPOSITORY_ONBOARDING_ID")?;
    let execution_id = required_env("PHARNESS_ONBOARDING_PATCH_EXECUTION_ID")?;
    let worker_token = required_env("PHARNESS_WORKER_TOKEN")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("failed to build onboarding patch client")?;
    let context_url = format!(
        "{api_url}/api/internal/repository-onboardings/{onboarding_id}/patch-context?execution_id={execution_id}"
    );
    let context = fetch_internal_context_with_retry::<OnboardingPatchContext>(
        &client,
        &context_url,
        &worker_token,
    )
    .await
    .context("failed to fetch onboarding patch context")?;
    if context.onboarding_id != onboarding_id
        || context.execution_id != execution_id
        || context.provider != "github"
        || !is_git_sha(&context.source_commit)
        || context.instructions.len() > 32 * 1024
    {
        anyhow::bail!("invalid_onboarding_patch_context");
    }
    let outcome = match materialize_onboarding_patch(&context).await {
        Ok((patch, patch_hash, changed_paths)) => serde_json::json!({
            "status": if changed_paths.is_empty() { "unchanged" } else { "succeeded" },
            "patch": patch,
            "patch_hash": patch_hash,
            "changed_paths": changed_paths,
        }),
        Err(error) => {
            let error_code = onboarding_patch_error_code(&error);
            tracing::warn!(
                onboarding_id,
                execution_id,
                error_code,
                "onboarding patch materialization failed"
            );
            serde_json::json!({
                "status": "failed",
                "error_code": error_code,
            })
        }
    };
    post_internal_json_with_retry(
        &client,
        &format!("{api_url}/api/internal/repository-onboardings/{onboarding_id}/patch-outcome"),
        &worker_token,
        &outcome,
    )
    .await
}

async fn materialize_onboarding_patch(
    context: &OnboardingPatchContext,
) -> anyhow::Result<(String, String, Vec<String>)> {
    parse_github_repository(&context.canonical_url)?;
    let contract: RepositoryContract = serde_json::from_value(context.candidate_contract.clone())
        .context("invalid_candidate_contract")?;
    contract
        .validate_candidate()
        .map_err(|_| anyhow::anyhow!("invalid_candidate_contract"))?;
    let root = std::path::PathBuf::from("/work/repository");
    checkout_exact_repository(&root, &context.canonical_url, &context.source_commit).await?;
    let pharness_dir = root.join(".pharness");
    ensure_onboarding_patch_path_safe(&root, &pharness_dir)?;
    tokio::fs::create_dir_all(&pharness_dir)
        .await
        .context("onboarding_patch_write_failed")?;
    let canonical_path = pharness_dir.join("repository.yaml");
    let instructions_path = pharness_dir.join("instructions.md");
    let alias_path = pharness_dir.join("project.yaml");
    ensure_onboarding_patch_path_safe(&root, &canonical_path)?;
    ensure_onboarding_patch_path_safe(&root, &instructions_path)?;
    ensure_onboarding_patch_path_safe(&root, &alias_path)?;
    let mut contract_yaml =
        serde_yaml::to_string(&contract).context("candidate_contract_serialization_failed")?;
    if !contract_yaml.ends_with('\n') {
        contract_yaml.push('\n');
    }
    tokio::fs::write(&canonical_path, contract_yaml)
        .await
        .context("onboarding_patch_write_failed")?;
    tokio::fs::write(&instructions_path, &context.instructions)
        .await
        .context("onboarding_patch_write_failed")?;
    if context.remove_alias && tokio::fs::try_exists(&alias_path).await? {
        tokio::fs::remove_file(&alias_path)
            .await
            .context("onboarding_patch_alias_removal_failed")?;
    }

    let askpass = source_reader_askpass().await?;
    let status = repository_git_stdout_preserve(
        &[
            "-C",
            root.to_str().context("invalid onboarding patch path")?,
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
        ],
        &askpass,
    )
    .await?;
    let changed_paths = parse_porcelain_paths(&status)?;
    validate_onboarding_patch_changed_paths(&changed_paths)?;
    if changed_paths.is_empty() {
        let _ = tokio::fs::remove_file(&askpass).await;
        let patch = String::new();
        let patch_hash = format!("sha256:{:x}", Sha256::digest(patch.as_bytes()));
        return Ok((patch, patch_hash, changed_paths));
    }
    repository_git_command(
        &[
            "-C",
            root.to_str().context("invalid onboarding patch path")?,
            "add",
            "--intent-to-add",
            "--",
            ".pharness/repository.yaml",
            ".pharness/instructions.md",
        ],
        &askpass,
    )
    .await?;
    let patch = repository_git_stdout_preserve(
        &[
            "-C",
            root.to_str().context("invalid onboarding patch path")?,
            "diff",
            "--binary",
            "--no-ext-diff",
            "--",
            ".pharness/repository.yaml",
            ".pharness/instructions.md",
            ".pharness/project.yaml",
        ],
        &askpass,
    )
    .await?;
    let _ = tokio::fs::remove_file(&askpass).await;
    if patch.is_empty() || patch.len() > 512 * 1024 {
        anyhow::bail!("onboarding_patch_size_invalid");
    }
    let patch_hash = format!("sha256:{:x}", Sha256::digest(patch.as_bytes()));
    let _ = (
        &context.repository_id,
        &context.default_branch,
        &context.proposal_id,
        &context.proposal_hash,
    );
    Ok((patch, patch_hash, changed_paths))
}

fn validate_onboarding_patch_changed_paths(changed_paths: &[String]) -> anyhow::Result<()> {
    let allowed = [
        ".pharness/repository.yaml",
        ".pharness/instructions.md",
        ".pharness/project.yaml",
    ];
    if changed_paths.len() > allowed.len()
        || changed_paths
            .iter()
            .any(|path| !allowed.contains(&path.as_str()))
    {
        anyhow::bail!("onboarding_patch_path_violation");
    }
    Ok(())
}

async fn checkout_exact_repository(
    root: &std::path::Path,
    canonical_url: &str,
    source_commit: &str,
) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(root).await?;
    let askpass = source_reader_askpass().await?;
    let root_text = root.to_str().context("invalid repository checkout path")?;
    repository_git_command(&["init", root_text], &askpass).await?;
    repository_git_command(
        &["-C", root_text, "remote", "add", "origin", canonical_url],
        &askpass,
    )
    .await?;
    repository_git_command(
        &[
            "-C",
            root_text,
            "fetch",
            "--depth",
            "1",
            "--no-tags",
            "--filter=blob:none",
            "--no-recurse-submodules",
            "origin",
            source_commit,
        ],
        &askpass,
    )
    .await?;
    repository_git_command(
        &["-C", root_text, "checkout", "--detach", "FETCH_HEAD"],
        &askpass,
    )
    .await?;
    let resolved = repository_git_stdout(&["-C", root_text, "rev-parse", "HEAD"], &askpass).await?;
    let _ = tokio::fs::remove_file(&askpass).await;
    if !resolved.eq_ignore_ascii_case(source_commit) {
        anyhow::bail!("resolved_commit_mismatch");
    }
    Ok(())
}

async fn source_reader_askpass() -> anyhow::Result<std::path::PathBuf> {
    let askpass = std::path::PathBuf::from("/tmp/source-reader-askpass");
    tokio::fs::write(
        &askpass,
        "#!/bin/sh\ncase \"$1\" in\n  *Username*) printf '%s\\n' x-access-token ;;\n  *) printf '%s\\n' \"${PHARNESS_SOURCE_READER_TOKEN:-}\" ;;\nesac\n",
    )
    .await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&askpass, std::fs::Permissions::from_mode(0o700)).await?;
    }
    Ok(askpass)
}

fn ensure_onboarding_patch_path_safe(
    root: &std::path::Path,
    path: &std::path::Path,
) -> anyhow::Result<()> {
    if !path.starts_with(root) {
        anyhow::bail!("onboarding_patch_path_violation");
    }
    let mut current = root.to_path_buf();
    let relative = path
        .strip_prefix(root)
        .map_err(|_| anyhow::anyhow!("onboarding_patch_path_violation"))?;
    for component in relative.components() {
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                anyhow::bail!("onboarding_patch_symlink_rejected")
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => anyhow::bail!("onboarding_patch_path_inspection_failed"),
        }
    }
    Ok(())
}

fn parse_porcelain_paths(status: &str) -> anyhow::Result<Vec<String>> {
    let mut paths = Vec::new();
    for entry in status.split('\0').filter(|entry| !entry.is_empty()) {
        if entry.len() < 4 || !entry.is_char_boundary(3) {
            anyhow::bail!("onboarding_patch_status_invalid");
        }
        let path = &entry[3..];
        if path.is_empty() || path.contains('\0') || path.starts_with('/') || path.contains("..") {
            anyhow::bail!("onboarding_patch_path_violation");
        }
        paths.push(path.to_string());
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

async fn repository_git_stdout_preserve(
    args: &[&str],
    askpass: &std::path::Path,
) -> anyhow::Result<String> {
    let output = repository_git_output(args, askpass).await?;
    if !output.status.success() {
        anyhow::bail!(repository_git_error_code(args));
    }
    String::from_utf8(output.stdout).context("repository Git output was not UTF-8")
}

fn onboarding_patch_error_code(error: &anyhow::Error) -> &'static str {
    let text = error.to_string();
    for code in [
        "resolved_commit_mismatch",
        "invalid_candidate_contract",
        "candidate_contract_serialization_failed",
        "onboarding_patch_path_violation",
        "onboarding_patch_symlink_rejected",
        "onboarding_patch_path_inspection_failed",
        "onboarding_patch_write_failed",
        "onboarding_patch_alias_removal_failed",
        "onboarding_patch_status_invalid",
        "onboarding_patch_size_invalid",
        "git_fetch_failed",
        "git_checkout_failed",
        "git_revision_failed",
        "git_setup_failed",
        "repository_not_github_https",
    ] {
        if text.contains(code) {
            return code;
        }
    }
    "onboarding_patch_failed"
}

async fn execute_repository_discovery() -> anyhow::Result<()> {
    let api_url = required_env("PHARNESS_API_URL")?
        .trim_end_matches('/')
        .to_string();
    let discovery_id = required_env("PHARNESS_REPOSITORY_DISCOVERY_ID")?;
    let worker_token = required_env("PHARNESS_WORKER_TOKEN")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("failed to build repository discovery client")?;
    let context_url =
        format!("{api_url}/api/internal/repository-discoveries/{discovery_id}/context");
    let context = fetch_internal_context_with_retry::<RepositoryDiscoveryContext>(
        &client,
        &context_url,
        &worker_token,
    )
    .await
    .context("failed to fetch repository discovery context")?;
    if context.discovery_id != discovery_id
        || context.provider != "github"
        || !is_git_sha(&context.source_commit)
    {
        anyhow::bail!("invalid_repository_discovery_context");
    }
    let outcome = match discover_repository_checkout(&context).await {
        Ok(discovery) => serde_json::json!({
            "status": "succeeded",
            "discovery": discovery,
        }),
        Err(error) => {
            tracing::warn!(
                discovery_id = %discovery_id,
                error_code = %repository_discovery_error_code(&error),
                "isolated repository discovery failed"
            );
            serde_json::json!({
                "status": "failed",
                "error_code": repository_discovery_error_code(&error),
                "error_summary": "isolated exact-revision repository discovery failed",
            })
        }
    };
    post_internal_json_with_retry(
        &client,
        &format!("{api_url}/api/internal/repository-discoveries/{discovery_id}/outcome"),
        &worker_token,
        &outcome,
    )
    .await
}

async fn discover_repository_checkout(
    context: &RepositoryDiscoveryContext,
) -> anyhow::Result<pharness_core::RepositoryDiscovery> {
    parse_github_repository(&context.canonical_url)?;
    let root = std::path::PathBuf::from("/work/repository");
    tokio::fs::create_dir_all(&root).await?;
    let askpass = std::path::PathBuf::from("/tmp/source-reader-askpass");
    tokio::fs::write(
        &askpass,
        "#!/bin/sh\ncase \"$1\" in\n  *Username*) printf '%s\\n' x-access-token ;;\n  *) printf '%s\\n' \"${PHARNESS_SOURCE_READER_TOKEN:-}\" ;;\nesac\n",
    )
    .await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&askpass, std::fs::Permissions::from_mode(0o700)).await?;
    }
    repository_git_command(
        &["init", root.to_str().context("invalid discovery path")?],
        &askpass,
    )
    .await?;
    repository_git_command(
        &[
            "-C",
            root.to_str().context("invalid discovery path")?,
            "remote",
            "add",
            "origin",
            &context.canonical_url,
        ],
        &askpass,
    )
    .await?;
    repository_git_command(
        &[
            "-C",
            root.to_str().context("invalid discovery path")?,
            "fetch",
            "--depth",
            "1",
            "--no-tags",
            "--filter=blob:none",
            "--no-recurse-submodules",
            "origin",
            &context.source_commit,
        ],
        &askpass,
    )
    .await?;
    repository_git_command(
        &[
            "-C",
            root.to_str().context("invalid discovery path")?,
            "checkout",
            "--detach",
            "FETCH_HEAD",
        ],
        &askpass,
    )
    .await?;
    let resolved = repository_git_stdout(
        &[
            "-C",
            root.to_str().context("invalid discovery path")?,
            "rev-parse",
            "HEAD",
        ],
        &askpass,
    )
    .await?;
    if !resolved.eq_ignore_ascii_case(&context.source_commit) {
        anyhow::bail!("resolved_commit_mismatch");
    }
    let discovery = discover_repository(
        &root,
        RepositoryDiscoveryIdentity {
            provider: context.provider.clone(),
            canonical_url: context.canonical_url.clone(),
            default_branch: context.default_branch.clone(),
            registered_commit: context.source_commit.to_ascii_lowercase(),
            resolved_commit: resolved.to_ascii_lowercase(),
        },
        context.limits.clone(),
    )?;
    let _ = tokio::fs::remove_file(&askpass).await;
    let _ = (&context.onboarding_id, &context.repository_id);
    Ok(discovery)
}

async fn repository_git_command(args: &[&str], askpass: &std::path::Path) -> anyhow::Result<()> {
    let output = repository_git_output(args, askpass).await?;
    if output.status.success() {
        Ok(())
    } else {
        anyhow::bail!(repository_git_error_code(args))
    }
}

async fn repository_git_stdout(args: &[&str], askpass: &std::path::Path) -> anyhow::Result<String> {
    let output = repository_git_output(args, askpass).await?;
    if !output.status.success() {
        anyhow::bail!(repository_git_error_code(args));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_string())
        .context("repository Git output was not UTF-8")
}

async fn repository_git_output(
    args: &[&str],
    askpass: &std::path::Path,
) -> anyhow::Result<std::process::Output> {
    Command::new("git")
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", askpass)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .await
        .context("failed to spawn repository Git command")
}

fn repository_git_error_code(args: &[&str]) -> &'static str {
    if args.contains(&"fetch") {
        "git_fetch_failed"
    } else if args.contains(&"checkout") {
        "git_checkout_failed"
    } else if args.contains(&"rev-parse") {
        "git_revision_failed"
    } else {
        "git_setup_failed"
    }
}

fn repository_discovery_error_code(error: &anyhow::Error) -> &'static str {
    let text = error.to_string();
    for code in [
        "resolved_commit_mismatch",
        "git_fetch_failed",
        "git_checkout_failed",
        "git_revision_failed",
        "git_setup_failed",
        "repository_not_github_https",
    ] {
        if text.contains(code) {
            return code;
        }
    }
    if text.contains("entry limit") {
        "repository_entry_limit_exceeded"
    } else if text.contains("inspected-text limit") {
        "repository_text_limit_exceeded"
    } else {
        "repository_discovery_failed"
    }
}

async fn post_internal_json_with_retry(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    payload: &serde_json::Value,
) -> anyhow::Result<()> {
    let mut last_error = None;
    for attempt in 1..=INGEST_ATTEMPTS {
        match client
            .post(url)
            .bearer_auth(token)
            .json(payload)
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => return Ok(()),
            Ok(response) if response.status().is_client_error() => {
                anyhow::bail!(
                    "internal outcome request was rejected with {}",
                    response.status()
                );
            }
            Ok(response) => {
                last_error = Some(anyhow::anyhow!(
                    "internal outcome request returned {}",
                    response.status()
                ));
            }
            Err(error) => last_error = Some(error.into()),
        }
        if attempt < INGEST_ATTEMPTS {
            tokio::time::sleep(INGEST_RETRY_DELAY).await;
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("internal outcome request failed")))
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
    let (contract, contract_hash) = load_preparation_contract(
        &cwd,
        spec.run.execution_target_json.get("repo_mode").is_some(),
    )?;
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
    let prepared = prepare_declared_runtime(
        &cwd,
        &contract,
        &executable_paths,
        configured_preparation_strategy()?,
    )
    .await?;
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
        runtime: Some(prepared.runtime.clone()),
        python_version: prepared.python_version.clone(),
        python_path: prepared.python_path.clone(),
        writable_paths: contract.writable_paths.clone(),
        unavailable_tools,
        agent_network: contract.agent_network,
        package_installation: contract.package_installation,
        acceptance_commands: contract.acceptance_commands.clone(),
        preparation_evidence: serde_json::json!({
            "required_executables": executable_paths,
            "runtime": prepared.evidence,
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

fn load_preparation_contract(
    workspace: &std::path::Path,
    repo_mode: bool,
) -> anyhow::Result<(RepositoryContract, String)> {
    if repo_mode {
        let loaded = RepositoryContract::load_for_repo_mode(workspace)?;
        return Ok((loaded.contract, format!("sha256:{}", loaded.content_sha256)));
    }
    RepositoryContract::load(workspace).map_err(Into::into)
}

struct PreparedRuntime {
    runtime: EnvironmentRuntimeSnapshot,
    python_version: Option<String>,
    python_path: Option<String>,
    evidence: serde_json::Value,
}

fn configured_preparation_strategy() -> anyhow::Result<PreparationStrategy> {
    match required_env("PHARNESS_PREPARATION_STRATEGY")?.as_str() {
        "python_hashed_requirements" => Ok(PreparationStrategy::PythonHashedRequirements),
        "node_npm_ci" => Ok(PreparationStrategy::NodeNpmCi),
        value => anyhow::bail!("unsupported preparation strategy {value}"),
    }
}

async fn prepare_declared_runtime(
    cwd: &std::path::Path,
    contract: &RepositoryContract,
    executable_paths: &serde_json::Map<String, serde_json::Value>,
    strategy: PreparationStrategy,
) -> anyhow::Result<PreparedRuntime> {
    let expected_lock = strategy.accepted_dependency_lock_kind();
    if contract.dependency_lock.kind != expected_lock {
        anyhow::bail!(
            "runner strategy requires dependency_lock.kind {expected_lock}, got {}",
            contract.dependency_lock.kind
        );
    }
    let runtime_dir = cwd.join(".pharness-runtime");
    tokio::fs::create_dir_all(&runtime_dir).await?;
    exclude_runtime_from_git(cwd).await?;
    match strategy {
        PreparationStrategy::PythonHashedRequirements => {
            let python_path = ["python", "python3"]
                .iter()
                .find_map(|name| {
                    executable_paths
                        .get(*name)
                        .and_then(serde_json::Value::as_str)
                })
                .context("python runner profile must declare python or python3")?;
            let venv = runtime_dir.join("venv");
            run_checked(
                cwd,
                python_path,
                &["-m", "venv", venv.to_string_lossy().as_ref()],
            )
            .await?;
            let venv_python = venv.join("bin/python");
            run_checked(
                cwd,
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
            let version =
                run_output(cwd, venv_python.to_string_lossy().as_ref(), &["--version"]).await?;
            let venv_bin = venv.join("bin").to_string_lossy().to_string();
            Ok(PreparedRuntime {
                runtime: EnvironmentRuntimeSnapshot {
                    kind: "python".into(),
                    executable: venv_python.to_string_lossy().to_string(),
                    version: version.clone(),
                    package_manager_executable: Some(format!("{venv_bin}/pip")),
                    package_manager_version: Some(
                        run_output(
                            cwd,
                            venv_python.to_string_lossy().as_ref(),
                            &["-m", "pip", "--version"],
                        )
                        .await?,
                    ),
                    path_entries: vec![venv_bin],
                },
                python_version: Some(version),
                python_path: Some(venv_python.to_string_lossy().to_string()),
                evidence: serde_json::json!({
                    "venv":venv,
                    "dependency_install":"pip --require-hashes --only-binary=:all:",
                }),
            })
        }
        PreparationStrategy::NodeNpmCi => {
            let node_path = executable_paths
                .get("node")
                .and_then(serde_json::Value::as_str)
                .context("node runner profile must declare node")?;
            let npm_path = executable_paths
                .get("npm")
                .and_then(serde_json::Value::as_str)
                .context("node runner profile must declare npm")?;
            reject_tracked_node_modules(cwd).await?;
            exclude_node_modules_from_git(cwd).await?;
            let lock_path = cwd.join(&contract.dependency_lock.path);
            let install_dir = lock_path
                .parent()
                .context("package-lock.json has no repository parent")?;
            let npm_cache = runtime_dir.join("npm-cache");
            tokio::fs::create_dir_all(&npm_cache).await?;
            let npm_cache_value = npm_cache.to_string_lossy().to_string();
            run_checked_with_env(
                install_dir,
                npm_path,
                &["ci", "--ignore-scripts", "--no-audit", "--no-fund"],
                &[("NPM_CONFIG_CACHE", npm_cache_value.as_str())],
            )
            .await?;
            ensure_tracked_workspace_clean(cwd).await?;
            let version = run_output(cwd, node_path, &["--version"]).await?;
            let npm_version = run_output(cwd, npm_path, &["--version"]).await?;
            let node_bin = install_dir
                .join("node_modules/.bin")
                .to_string_lossy()
                .to_string();
            Ok(PreparedRuntime {
                runtime: EnvironmentRuntimeSnapshot {
                    kind: "node".into(),
                    executable: node_path.to_string(),
                    version,
                    package_manager_executable: Some(npm_path.to_string()),
                    package_manager_version: Some(npm_version),
                    path_entries: vec![node_bin],
                },
                python_version: None,
                python_path: None,
                evidence: serde_json::json!({
                    "npm_cache":npm_cache,
                    "dependency_install":"npm ci --ignore-scripts --no-audit --no-fund",
                    "lifecycle_scripts":"denied",
                    "tracked_files_unchanged":true,
                }),
            })
        }
    }
}

fn effective_runtime_path(entries: &[String], inherited: &str) -> String {
    let mut parts = entries.to_vec();
    if !inherited.is_empty() {
        parts.push(inherited.to_string());
    }
    parts.join(":")
}

fn acceptance_environment(
    cwd: &std::path::Path,
    contract: &RepositoryContract,
    runtime: &EnvironmentRuntimeSnapshot,
) -> Vec<(String, String)> {
    if runtime.kind != "python" {
        return Vec::new();
    }
    let python_path = contract
        .roots
        .source
        .iter()
        .map(|path| cwd.join(path).to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(":");
    vec![("PYTHONPATH".into(), python_path)]
}

async fn reject_tracked_node_modules(cwd: &std::path::Path) -> anyhow::Result<()> {
    let tracked = run_output(
        cwd,
        "git",
        &["ls-files", "node_modules", ":(glob)**/node_modules/**"],
    )
    .await?;
    if !tracked.trim().is_empty() {
        anyhow::bail!("tracked_node_modules_not_supported");
    }
    Ok(())
}

async fn ensure_tracked_workspace_clean(cwd: &std::path::Path) -> anyhow::Result<()> {
    let status = run_output(
        cwd,
        "git",
        &["status", "--porcelain", "--untracked-files=no"],
    )
    .await?;
    if !status.trim().is_empty() {
        anyhow::bail!("preparation_modified_tracked_files");
    }
    Ok(())
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

async fn run_checked_with_env(
    cwd: &std::path::Path,
    program: &str,
    args: &[&str],
    env: &[(&str, &str)],
) -> anyhow::Result<()> {
    let output = Command::new(program)
        .current_dir(cwd)
        .args(args)
        .envs(env.iter().copied())
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

async fn exclude_node_modules_from_git(cwd: &std::path::Path) -> anyhow::Result<()> {
    let exclude = cwd.join(".git/info/exclude");
    let mut content = tokio::fs::read_to_string(&exclude)
        .await
        .unwrap_or_default();
    if !content.lines().any(|line| line.trim() == "node_modules/") {
        if !content.ends_with('\n') && !content.is_empty() {
            content.push('\n');
        }
        content.push_str("node_modules/\n");
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
    let source_delivery_intent_id = std::env::var("PHARNESS_SOURCE_DELIVERY_INTENT_ID").ok();
    let resource_id = match source_delivery_intent_id.as_deref() {
        Some(intent_id) => intent_id.to_string(),
        None => required_env("PHARNESS_CHANGE_SET_ID")?,
    };
    let execution_id = required_env("PHARNESS_GIT_DELIVERY_EXECUTION_ID")?;
    let worker_token = required_env("PHARNESS_WORKER_TOKEN")?;
    let git_token = required_env("PHARNESS_GIT_WRITER_TOKEN")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("failed to build Git writer http client")?;
    let context_url = match source_delivery_intent_id.as_deref() {
        Some(intent_id) => format!("{api_url}/api/internal/source-delivery-intents/{intent_id}/context?execution_id={execution_id}"),
        None => format!("{api_url}/api/internal/change-sets/{resource_id}/git-delivery-context?execution_id={execution_id}"),
    };
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
            tracing::warn!(delivery_resource_id = %resource_id, error = %error, "Git writer failed without exposing command output");
            serde_json::json!({
                "execution_id": execution_id, "status": "failed", "error_code": git_delivery_error_code(&error),
            })
        }
    };
    let outcome_url = match source_delivery_intent_id.as_deref() {
        Some(intent_id) => {
            format!("{api_url}/api/internal/source-delivery-intents/{intent_id}/writer-outcome")
        }
        None => format!("{api_url}/api/internal/change-sets/{resource_id}/git-delivery-outcome"),
    };
    post_git_delivery_outcome(&client, &outcome_url, &worker_token, &outcome).await
}

/// Read one GitHub pull request through the dedicated observer identity. The
/// observer has no Git CLI, workspace, or model credentials and reports only
/// bounded merge provenance to the API.
async fn execute_git_delivery_observation() -> anyhow::Result<()> {
    let api_url = required_env("PHARNESS_API_URL")?
        .trim_end_matches('/')
        .to_string();
    let source_delivery_intent_id = std::env::var("PHARNESS_SOURCE_DELIVERY_INTENT_ID").ok();
    let resource_id = match source_delivery_intent_id.as_deref() {
        Some(intent_id) => intent_id.to_string(),
        None => required_env("PHARNESS_CHANGE_SET_ID")?,
    };
    let execution_id = required_env("PHARNESS_GIT_DELIVERY_EXECUTION_ID")?;
    let worker_token = required_env("PHARNESS_WORKER_TOKEN")?;
    let git_token = required_env("PHARNESS_GIT_OBSERVER_TOKEN")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build Git observer http client")?;
    let context_url = match source_delivery_intent_id.as_deref() {
        Some(intent_id) => format!("{api_url}/api/internal/source-delivery-intents/{intent_id}/observation-context?execution_id={execution_id}"),
        None => format!("{api_url}/api/internal/change-sets/{resource_id}/git-delivery-observation-context?execution_id={execution_id}"),
    };
    let context = fetch_internal_context_with_retry::<GitDeliveryObservationContext>(
        &client,
        &context_url,
        &worker_token,
    )
    .await
    .context("failed to fetch Git observer context")?;
    let observed = if source_delivery_intent_id.is_some() {
        observe_github_source_delivery(&client, &context, &git_token).await
    } else {
        observe_github_pull_request(&client, &context, &git_token).await
    };
    let outcome = match observed {
        Ok(observation) => serde_json::json!({
            "execution_id": execution_id,
            "status": "observed",
            "pull_request_state": observation.pull_request_state,
            "merged": observation.merged,
            "merge_commit_sha": observation.merge_commit_sha,
            "head_branch": observation.head_branch,
            "head_commit_sha": observation.head_commit_sha,
            "authoritative_rules_succeeded": observation.authoritative_rules_succeeded,
            "required_checks": observation.required_checks,
            "check_runs": observation.check_runs,
            "commit_statuses": observation.commit_statuses,
            "provider_check_status": observation.provider_check_status,
        }),
        Err(error) => {
            tracing::warn!(delivery_resource_id = %resource_id, error = %error, "Git observer failed without exposing provider output");
            serde_json::json!({
                "execution_id": execution_id,
                "status": "failed",
                "error_code": git_observer_error_code(&error),
            })
        }
    };
    let outcome_url = match source_delivery_intent_id.as_deref() {
        Some(intent_id) => format!(
            "{api_url}/api/internal/source-delivery-intents/{intent_id}/observation-outcome"
        ),
        None => format!(
            "{api_url}/api/internal/change-sets/{resource_id}/git-delivery-observation-outcome"
        ),
    };
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
        base_ref: None,
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
    #[serde(default)]
    base_ref: Option<String>,
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
    authoritative_rules_succeeded: bool,
    required_checks: serde_json::Value,
    check_runs: serde_json::Value,
    commit_statuses: serde_json::Value,
    provider_check_status: String,
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
    parse_github_pull_request_observation(&value, context)
}

async fn observe_github_source_delivery(
    client: &reqwest::Client,
    context: &GitDeliveryObservationContext,
    token: &str,
) -> anyhow::Result<GitPullRequestObservation> {
    let mut observation = observe_github_pull_request(client, context, token).await?;
    let provider = observe_github_required_checks(client, context, token, None).await?;
    observation.authoritative_rules_succeeded = true;
    observation.required_checks = provider.required_checks;
    observation.check_runs = provider.check_runs;
    observation.commit_statuses = provider.commit_statuses;
    observation.provider_check_status = provider.status;
    Ok(observation)
}

fn parse_github_pull_request_observation(
    value: &serde_json::Value,
    context: &GitDeliveryObservationContext,
) -> anyhow::Result<GitPullRequestObservation> {
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
    // GitHub may return a synthetic test-merge SHA for a closed pull request
    // even when `merged` is false. It is not immutable merge provenance and
    // must not be forwarded as one.
    let merge_commit_sha = merged
        .then(|| {
            value
                .pointer("/merge_commit_sha")
                .and_then(serde_json::Value::as_str)
                .filter(|value| is_git_sha(value))
                .map(ToOwned::to_owned)
        })
        .flatten();
    if merged && merge_commit_sha.is_none() {
        anyhow::bail!("github_merge_commit_missing");
    }
    Ok(GitPullRequestObservation {
        pull_request_state: pull_request_state.expect("validated state").to_string(),
        merged,
        merge_commit_sha,
        head_branch: head_branch.expect("validated branch").to_string(),
        head_commit_sha: head_commit_sha.expect("validated sha").to_string(),
        authoritative_rules_succeeded: false,
        required_checks: serde_json::json!([]),
        check_runs: serde_json::json!([]),
        commit_statuses: serde_json::json!([]),
        provider_check_status: "unavailable".into(),
    })
}

struct GitHubRequiredCheckObservation {
    required_checks: serde_json::Value,
    check_runs: serde_json::Value,
    commit_statuses: serde_json::Value,
    status: String,
}

/// Exercise every read used by Repo Mode source delivery under the isolated
/// observer identity. Repository reachability alone is insufficient: an
/// observer that cannot authoritatively read branch rules, check runs, and
/// commit statuses must remain unavailable before a WorkItem reaches merge.
async fn execute_source_observer_capability_preflight() -> anyhow::Result<()> {
    let repository = required_env("REPOSITORY")?;
    let api = required_env("GITHUB_API_URL")?
        .trim_end_matches('/')
        .to_string();
    let token = required_env("GITHUB_TOKEN")?;
    if api != "https://api.github.com" {
        anyhow::bail!("invalid_github_api_url");
    }
    let (owner, repo) = parse_github_repository(&repository)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let metadata = github_observer_json(
        &client,
        &format!("{api}/repos/{owner}/{repo}"),
        Some(&token),
        false,
        "github_repository_metadata_query_unavailable",
    )
    .await?
    .ok_or_else(|| anyhow::anyhow!("github_repository_metadata_unavailable"))?;
    if metadata
        .pointer("/permissions/pull")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        anyhow::bail!("github_repository_pull_permission_unavailable");
    }
    let repository_is_public = metadata
        .get("private")
        .and_then(serde_json::Value::as_bool)
        .map(|private| !private)
        .ok_or_else(|| anyhow::anyhow!("github_repository_visibility_unavailable"))?;
    let base_ref = metadata
        .get("default_branch")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("github_default_branch_unavailable"))?
        .to_string();
    let branch = percent_encode_path_segment(&base_ref);
    let commit = github_observer_json(
        &client,
        &format!("{api}/repos/{owner}/{repo}/commits/{branch}"),
        Some(&token),
        false,
        "github_default_branch_commit_query_unavailable",
    )
    .await?
    .ok_or_else(|| anyhow::anyhow!("github_default_branch_commit_unavailable"))?;
    let source_commit_sha = commit
        .get("sha")
        .and_then(serde_json::Value::as_str)
        .filter(|value| is_git_sha(value))
        .ok_or_else(|| anyhow::anyhow!("github_default_branch_commit_invalid"))?
        .to_string();
    let context = GitDeliveryObservationContext {
        execution_id: "source-observer-capability-preflight".to_string(),
        repository,
        base_ref: Some(base_ref.clone()),
        head_branch: base_ref,
        source_commit_sha,
        pull_request_url: format!("https://github.com/{owner}/{repo}/pull/1"),
        pull_request_number: 1,
        github_api_url: api,
    };
    observe_github_required_checks(&client, &context, &token, Some(repository_is_public)).await?;
    tracing::info!(
        repository = %context.repository,
        "source observer verified repository, rules, checks, and statuses"
    );
    Ok(())
}

async fn observe_github_required_checks(
    client: &reqwest::Client,
    context: &GitDeliveryObservationContext,
    token: &str,
    repository_is_public: Option<bool>,
) -> anyhow::Result<GitHubRequiredCheckObservation> {
    let (owner, repo) = parse_github_repository(&context.repository)?;
    let api = context.github_api_url.trim_end_matches('/');
    let repository_is_public = match repository_is_public {
        Some(value) => value,
        None => {
            let metadata = github_observer_json(
                client,
                &format!("{api}/repos/{owner}/{repo}"),
                Some(token),
                false,
                "github_repository_metadata_query_unavailable",
            )
            .await?
            .ok_or_else(|| anyhow::anyhow!("github_repository_metadata_unavailable"))?;
            metadata
                .get("private")
                .and_then(serde_json::Value::as_bool)
                .map(|private| !private)
                .ok_or_else(|| anyhow::anyhow!("github_repository_visibility_unavailable"))?
        }
    };
    let branch = percent_encode_path_segment(
        context
            .base_ref
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("github_base_ref_unavailable"))?,
    );
    let rules = github_observer_json(
        client,
        &format!("{api}/repos/{owner}/{repo}/rules/branches/{branch}?per_page=100"),
        Some(token),
        false,
        "github_active_branch_rules_query_unavailable",
    )
    .await?
    .ok_or_else(|| anyhow::anyhow!("github_active_branch_rules_unavailable"))?;
    let classic = github_observer_json(
        client,
        &format!("{api}/repos/{owner}/{repo}/branches/{branch}/protection/required_status_checks"),
        Some(token),
        true,
        "github_classic_required_checks_query_unavailable",
    )
    .await?;
    let check_runs = github_observer_json_with_public_fallback(
        client,
        &format!(
            "{api}/repos/{owner}/{repo}/commits/{}/check-runs?per_page=100",
            context.source_commit_sha
        ),
        token,
        repository_is_public,
        "github_check_runs_query_unavailable",
    )
    .await?
    .ok_or_else(|| anyhow::anyhow!("github_check_runs_unavailable"))?;
    let statuses = github_observer_json(
        client,
        &format!(
            "{api}/repos/{owner}/{repo}/commits/{}/status?per_page=100",
            context.source_commit_sha
        ),
        Some(token),
        false,
        "github_commit_statuses_query_unavailable",
    )
    .await?
    .ok_or_else(|| anyhow::anyhow!("github_commit_statuses_unavailable"))?;
    evaluate_github_required_checks(&rules, classic.as_ref(), &check_runs, &statuses)
}

async fn github_observer_json(
    client: &reqwest::Client,
    url: &str,
    token: Option<&str>,
    not_found_is_empty: bool,
    unavailable_error: &'static str,
) -> anyhow::Result<Option<serde_json::Value>> {
    let mut request = client
        .get(url)
        .header("accept", "application/vnd.github+json")
        .header("x-github-api-version", "2022-11-28")
        .header("user-agent", "pharness-git-observer");
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .context("GitHub provider-check observation request failed")?;
    if response.status() == reqwest::StatusCode::NOT_FOUND && not_found_is_empty {
        return Ok(None);
    }
    if !response.status().is_success() {
        anyhow::bail!(unavailable_error);
    }
    if response
        .headers()
        .get(reqwest::header::LINK)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("rel=\"next\""))
    {
        anyhow::bail!("github_provider_check_query_exceeded_bound");
    }
    Ok(Some(
        response
            .json()
            .await
            .context("GitHub provider-check response was invalid")?,
    ))
}

async fn github_observer_json_with_public_fallback(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    repository_is_public: bool,
    unavailable_error: &'static str,
) -> anyhow::Result<Option<serde_json::Value>> {
    match github_observer_json(client, url, Some(token), false, unavailable_error).await {
        Ok(value) => Ok(value),
        Err(error) if repository_is_public && error.to_string().as_str() == unavailable_error => {
            github_observer_json(client, url, None, false, unavailable_error).await
        }
        Err(error) => Err(error),
    }
}

fn evaluate_github_required_checks(
    active_rules: &serde_json::Value,
    classic: Option<&serde_json::Value>,
    check_runs_response: &serde_json::Value,
    statuses_response: &serde_json::Value,
) -> anyhow::Result<GitHubRequiredCheckObservation> {
    let mut required =
        std::collections::BTreeMap::<(String, Option<i64>), serde_json::Value>::new();
    let rules = active_rules
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("github_active_branch_rules_invalid"))?;
    for rule in rules {
        if rule.get("type").and_then(serde_json::Value::as_str) != Some("required_status_checks") {
            continue;
        }
        let entries = rule
            .pointer("/parameters/required_status_checks")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("github_active_branch_rules_invalid"))?;
        for entry in entries {
            add_required_check(&mut required, entry, "integration_id")?;
        }
    }
    if let Some(classic) = classic {
        for context in classic
            .get("contexts")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = context
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("github_classic_branch_protection_invalid"))?;
            required
                .entry((name.to_string(), None))
                .or_insert_with(|| serde_json::json!({"name":name,"app_id":null}));
        }
        for entry in classic
            .get("checks")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            add_required_check(&mut required, entry, "app_id")?;
        }
    }
    let check_runs = check_runs_response
        .get("check_runs")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("github_check_runs_invalid"))?;
    let statuses = statuses_response
        .get("statuses")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("github_commit_statuses_invalid"))?;
    if check_runs.len() > 100 || statuses.len() > 100 || required.len() > 100 {
        anyhow::bail!("github_provider_check_query_exceeded_bound");
    }

    let bounded_runs = check_runs
        .iter()
        .filter_map(|run| {
            Some(serde_json::json!({
                "name":run.get("name")?.as_str()?,
                "status":run.get("status").and_then(serde_json::Value::as_str),
                "conclusion":run.get("conclusion").and_then(serde_json::Value::as_str),
                "app_id":run.pointer("/app/id").and_then(serde_json::Value::as_i64),
            }))
        })
        .collect::<Vec<_>>();
    let mut bounded_statuses = Vec::new();
    let mut seen_statuses = std::collections::BTreeSet::new();
    for status in statuses {
        let Some(name) = status.get("context").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if seen_statuses.insert(name.to_string()) {
            bounded_statuses.push(serde_json::json!({
                "name":name,
                "state":status.get("state").and_then(serde_json::Value::as_str),
            }));
        }
    }

    let mut overall = "passing";
    let mut required_output = Vec::new();
    for ((name, app_id), _) in required {
        let matching_runs = bounded_runs
            .iter()
            .filter(|run| {
                run.get("name").and_then(serde_json::Value::as_str) == Some(name.as_str())
                    && app_id.map_or(true, |expected| {
                        run.get("app_id").and_then(serde_json::Value::as_i64) == Some(expected)
                    })
            })
            .collect::<Vec<_>>();
        let matching_status = bounded_statuses.iter().find(|status| {
            status.get("name").and_then(serde_json::Value::as_str) == Some(name.as_str())
        });
        let run_state = if matching_runs.is_empty() {
            "missing"
        } else if matching_runs.iter().any(|run| {
            run.get("status").and_then(serde_json::Value::as_str) == Some("completed")
                && !matches!(
                    run.get("conclusion").and_then(serde_json::Value::as_str),
                    Some("success" | "skipped" | "neutral")
                )
        }) {
            "failed"
        } else if matching_runs.iter().all(|run| {
            run.get("status").and_then(serde_json::Value::as_str) == Some("completed")
                && matches!(
                    run.get("conclusion").and_then(serde_json::Value::as_str),
                    Some("success" | "skipped" | "neutral")
                )
        }) {
            "passing"
        } else {
            "pending"
        };
        let status_state = matching_status
            .and_then(|status| status.get("state"))
            .and_then(serde_json::Value::as_str)
            .map(|state| {
                if state == "success" {
                    "passing"
                } else if matches!(state, "pending" | "expected") {
                    "pending"
                } else {
                    "failed"
                }
            })
            .unwrap_or("missing");
        let effective = if app_id.is_some() {
            // App-bound requirements can only be satisfied by the bound check-run identity.
            // If a commit status of the same name also exists, both systems must pass.
            combine_provider_states(run_state, matching_status.map(|_| status_state))
        } else if matching_runs.is_empty() {
            status_state
        } else if matching_status.is_none() {
            run_state
        } else {
            combine_provider_states(run_state, Some(status_state))
        };
        overall = combine_overall_status(overall, effective);
        required_output.push(serde_json::json!({
            "name":name,
            "app_id":app_id,
            "check_run_state":run_state,
            "commit_status_state":status_state,
            "status":effective,
        }));
    }
    Ok(GitHubRequiredCheckObservation {
        required_checks: serde_json::Value::Array(required_output),
        check_runs: serde_json::Value::Array(bounded_runs),
        commit_statuses: serde_json::Value::Array(bounded_statuses),
        status: overall.into(),
    })
}

fn add_required_check(
    required: &mut std::collections::BTreeMap<(String, Option<i64>), serde_json::Value>,
    entry: &serde_json::Value,
    app_field: &str,
) -> anyhow::Result<()> {
    let name = entry
        .get("context")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("github_required_check_invalid"))?;
    let app_id = entry.get(app_field).and_then(serde_json::Value::as_i64);
    required
        .entry((name.to_string(), app_id))
        .or_insert_with(|| serde_json::json!({"name":name,"app_id":app_id}));
    Ok(())
}

fn combine_provider_states(first: &str, second: Option<&str>) -> &'static str {
    let second = second.unwrap_or("passing");
    if first == "failed" || second == "failed" {
        "failed"
    } else if first == "passing" && second == "passing" {
        "passing"
    } else {
        "pending"
    }
}

fn combine_overall_status(current: &str, next: &str) -> &'static str {
    if current == "failed" || next == "failed" {
        "failed"
    } else if current == "pending" || next != "passing" {
        "pending"
    } else {
        "passing"
    }
}

fn percent_encode_path_segment(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
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
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
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
    let output = git_delivery_output(args, askpass).await?;
    if output.status.success() {
        Ok(())
    } else {
        anyhow::bail!(git_delivery_command_error_code_for_stderr(
            args,
            &output.stderr
        ))
    }
}

async fn git_delivery_stdout(args: &[&str], askpass: &std::path::Path) -> anyhow::Result<String> {
    let output = git_delivery_output(args, askpass).await?;
    if !output.status.success() {
        anyhow::bail!(git_delivery_command_error_code_for_stderr(
            args,
            &output.stderr
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_delivery_command_error_code_for_stderr(args: &[&str], stderr: &[u8]) -> &'static str {
    let default = git_delivery_command_error_code(args);
    if default != "git_push_failed" {
        return default;
    }
    let stderr = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    if stderr.contains("authentication failed")
        || stderr.contains("invalid username or password")
        || stderr.contains("could not read username")
        || stderr.contains("authentication required")
    {
        "git_push_authentication_failed"
    } else if (stderr.contains("permission to") && stderr.contains("denied"))
        || stderr.contains("write access to repository not granted")
        || stderr.contains("requested url returned error: 403")
    {
        "git_push_permission_denied"
    } else if stderr.contains("non-fast-forward") || stderr.contains("fetch first") {
        "git_push_non_fast_forward"
    } else if stderr.contains("cannot lock ref") || stderr.contains("exists; cannot create") {
        "git_push_ref_conflict"
    } else if stderr.contains("repository rule violations")
        || stderr.contains("protected branch hook declined")
        || stderr.contains("push declined")
    {
        "git_push_policy_rejected"
    } else if stderr.contains("could not resolve host")
        || stderr.contains("failed to connect")
        || stderr.contains("connection timed out")
        || stderr.contains("network is unreachable")
    {
        "git_push_transport_failed"
    } else {
        default
    }
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
        "git_push_authentication_failed" => "git_push_authentication_failed",
        "git_push_permission_denied" => "git_push_permission_denied",
        "git_push_non_fast_forward" => "git_push_non_fast_forward",
        "git_push_ref_conflict" => "git_push_ref_conflict",
        "git_push_policy_rejected" => "git_push_policy_rejected",
        "git_push_transport_failed" => "git_push_transport_failed",
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
        "kustomization image repository does not match the declared image name" => {
            "kustomization_image_repository_mismatch"
        }
        "kustomization image newName does not match the declared image name" => {
            "kustomization_image_new_name_mismatch"
        }
        "kustomization image entry must not contain newTag" => "kustomization_image_mutable_tag",
        "kustomization image entry must already contain an immutable sha256 digest" => {
            "kustomization_image_not_digest_pinned"
        }
        "kustomization image digest occurrence is ambiguous" => {
            "kustomization_image_digest_ambiguous"
        }
        "kustomization image entry must use standard space indentation"
        | "kustomization image entry was not found in standard YAML form"
        | "kustomization image entry is ambiguous in source text"
        | "kustomization image digest was not found in standard YAML form" => {
            "kustomization_image_nonstandard_yaml"
        }
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
        "git_push_authentication_failed" => "git_push_authentication_failed",
        "git_push_permission_denied" => "git_push_permission_denied",
        "git_push_non_fast_forward" => "git_push_non_fast_forward",
        "git_push_ref_conflict" => "git_push_ref_conflict",
        "git_push_policy_rejected" => "git_push_policy_rejected",
        "git_push_transport_failed" => "git_push_transport_failed",
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
        "github_active_branch_rules_query_unavailable" => {
            "github_active_branch_rules_query_unavailable"
        }
        "github_classic_required_checks_query_unavailable" => {
            "github_classic_required_checks_query_unavailable"
        }
        "github_check_runs_query_unavailable" => "github_check_runs_query_unavailable",
        "github_commit_statuses_query_unavailable" => "github_commit_statuses_query_unavailable",
        "github_repository_metadata_query_unavailable" => {
            "github_repository_metadata_query_unavailable"
        }
        "github_repository_metadata_unavailable" => "github_repository_metadata_unavailable",
        "github_repository_visibility_unavailable" => "github_repository_visibility_unavailable",
        "github_provider_check_query_exceeded_bound" => {
            "github_provider_check_query_exceeded_bound"
        }
        "github_active_branch_rules_unavailable" => "github_active_branch_rules_unavailable",
        "github_check_runs_unavailable" => "github_check_runs_unavailable",
        "github_commit_statuses_unavailable" => "github_commit_statuses_unavailable",
        "github_active_branch_rules_invalid" => "github_active_branch_rules_invalid",
        "github_classic_branch_protection_invalid" => "github_classic_branch_protection_invalid",
        "github_check_runs_invalid" => "github_check_runs_invalid",
        "github_commit_statuses_invalid" => "github_commit_statuses_invalid",
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
        if spec.resume.is_none() {
            if let Some(snapshot) = spec.run.execution_target_json.get("environment_snapshot") {
                let runtime_ready = snapshot
                    .pointer("/runtime/path_entries/0")
                    .and_then(serde_json::Value::as_str)
                    .map(std::path::Path::new)
                    .is_some_and(std::path::Path::exists)
                    || snapshot.get("runtime").is_none()
                        && cwd.join(".pharness-runtime/venv/bin/python").is_file();
                if !runtime_ready {
                    anyhow::bail!("prepared workspace is missing its durable runtime environment");
                }
            }
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
        acceptance_command_is_structurally_unexecutable, allowlisted_connect_target,
        argo_application_terminal, argo_sync_patch_payload, evaluate_github_required_checks,
        fetch_internal_context, git_delivery_command_error_code,
        git_delivery_command_error_code_for_stderr, git_observer_error_code, git_patch_for_apply,
        github_observer_json, github_observer_json_with_public_fallback, load_preparation_contract,
        parse_github_pull_request_observation, parse_github_repository, pipeline_run_terminal,
        prepare_declared_runtime, update_kustomization_image, validate_git_delivery_context,
        validate_onboarding_patch_changed_paths, validate_resumed_workspace_identity,
        workspace_git_args, ArgoApplicationTerminal, GitDeliveryContext,
        GitDeliveryObservationContext, PipelineRunTerminal,
    };
    use pharness_core::{
        AcceptanceCommand, AgentNetworkPolicy, DependencyLock, PackageInstallationPolicy,
        PreparationStrategy, ProjectRoots, RepositoryContract,
    };
    use pharness_runhost::WorkspaceSourceSpec;
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use std::path::Path;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn onboarding_materializer_accepts_an_exact_no_change_result() {
        validate_onboarding_patch_changed_paths(&[]).unwrap();
        validate_onboarding_patch_changed_paths(&[".pharness/repository.yaml".into()]).unwrap();
        assert!(validate_onboarding_patch_changed_paths(&["src/main.rs".into()]).is_err());
    }

    #[test]
    fn readiness_classifies_missing_declared_python_module_as_structural() {
        assert!(acceptance_command_is_structurally_unexecutable(
            "python -m pytest -q",
            Some(1),
            b"/usr/local/bin/python: No module named pytest\n",
        ));
        assert!(acceptance_command_is_structurally_unexecutable(
            "python -m pytest -q",
            Some(127),
            b"python: not found\n",
        ));
    }

    #[test]
    fn readiness_keeps_executable_test_failures_as_baseline_failures() {
        assert!(!acceptance_command_is_structurally_unexecutable(
            "python -m pytest -q",
            Some(1),
            b"E   ModuleNotFoundError: No module named 'application_plugin'\n",
        ));
        assert!(!acceptance_command_is_structurally_unexecutable(
            "python -m pytest -q",
            Some(1),
            b"1 failed, 34 passed\n",
        ));
    }

    #[test]
    fn repo_mode_preparation_uses_the_canonical_prefixed_contract_hash() {
        let root = std::env::temp_dir().join(format!(
            "pharness-worker-repo-contract-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".pharness")).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(root.join("readme.md"), "# Fixture\n").unwrap();
        let lock = b"fastapi==1.0 --hash=sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n";
        std::fs::write(root.join("requirements.lock"), lock).unwrap();
        let lock_hash = format!("{:x}", Sha256::digest(lock));
        let contract = format!(
            r#"api_version: pharness.dev/v1alpha1
environment_profile: python-3.11
dependency_lock:
  kind: pip_requirements
  path: requirements.lock
  sha256: {lock_hash}
writable_paths: [src/**, tests/**, readme.md]
acceptance_commands:
  - name: unit
    command: python -m unittest discover -s tests -v
roots:
  source: [src]
  tests: [tests]
  documentation: [readme.md]
agent_network: denied
package_installation: preparation_only
"#
        );
        std::fs::write(root.join(".pharness/repository.yaml"), contract.as_bytes()).unwrap();

        let (_, repo_mode_hash) = load_preparation_contract(&root, true).unwrap();
        let (_, legacy_hash) = load_preparation_contract(&root, false).unwrap();
        assert_eq!(repo_mode_hash, format!("sha256:{legacy_hash}"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn node_preparation_ignores_lifecycle_scripts_and_keeps_tracked_files_clean() {
        use std::os::unix::fs::PermissionsExt;

        let suffix = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(format!("pharness-node-prep-{suffix}"));
        let tools = std::env::temp_dir().join(format!("pharness-node-tools-{suffix}"));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::write(
            root.join("package-lock.json"),
            r#"{"name":"fixture","lockfileVersion":3,"packages":{"":{"name":"fixture"},"node_modules/example":{"version":"1.0.0","resolved":"https://registry.npmjs.org/example/-/example-1.0.0.tgz","integrity":"sha512-YWJjZA=="}}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"scripts":{"install":"touch install-script-ran"}}"#,
        )
        .unwrap();
        std::fs::write(root.join("src/index.js"), "export {};\n").unwrap();
        std::fs::write(root.join("tests/index.test.js"), "// fixture\n").unwrap();
        let node = tools.join("node");
        let npm = tools.join("npm");
        std::fs::write(&node, "#!/bin/sh\necho v24.0.0\n").unwrap();
        std::fs::write(
            &npm,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 11.0.0; exit 0; fi\nfound=no\nfor arg in \"$@\"; do [ \"$arg\" = \"--ignore-scripts\" ] && found=yes; done\n[ \"$found\" = yes ] || touch install-script-ran\nmkdir -p node_modules/.bin .pharness-runtime/npm-cache\n",
        )
        .unwrap();
        std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&npm, std::fs::Permissions::from_mode(0o755)).unwrap();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.invalid"],
            vec!["config", "user.name", "PHarness test"],
            vec!["add", "."],
            vec!["commit", "-m", "fixture"],
        ] {
            assert!(std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .output()
                .unwrap()
                .status
                .success());
        }
        let contract = RepositoryContract {
            api_version: "pharness.dev/v1alpha1".into(),
            environment_profile: "node-24".into(),
            dependency_lock: DependencyLock {
                kind: "npm_package_lock".into(),
                path: "package-lock.json".into(),
                sha256: "a".repeat(64),
            },
            writable_paths: vec!["src/**".into(), "tests/**".into()],
            acceptance_commands: vec![AcceptanceCommand {
                name: "test".into(),
                command: "npm test".into(),
            }],
            roots: ProjectRoots {
                source: vec!["src".into()],
                tests: vec!["tests".into()],
                documentation: Vec::new(),
            },
            agent_network: AgentNetworkPolicy::Denied,
            package_installation: PackageInstallationPolicy::PreparationOnly,
        };
        let mut executables = serde_json::Map::new();
        executables.insert("node".into(), json!(node));
        executables.insert("npm".into(), json!(npm));
        let prepared = prepare_declared_runtime(
            &root,
            &contract,
            &executables,
            PreparationStrategy::NodeNpmCi,
        )
        .await
        .unwrap();
        assert_eq!(prepared.runtime.kind, "node");
        assert!(!root.join("install-script-ran").exists());
        assert!(root.join("node_modules/.bin").is_dir());
        assert!(std::process::Command::new("git")
            .args(["status", "--porcelain", "--untracked-files=no"])
            .current_dir(&root)
            .output()
            .unwrap()
            .stdout
            .is_empty());
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(tools);
    }

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

    #[tokio::test]
    async fn provider_check_queries_preserve_a_sanitized_endpoint_error_code() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await.unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
        });
        let client = reqwest::Client::builder().build().unwrap();

        let error = github_observer_json(
            &client,
            &format!("http://{address}/rules"),
            Some("redacted-test-token"),
            false,
            "github_active_branch_rules_query_unavailable",
        )
        .await
        .unwrap_err();

        assert_eq!(
            git_observer_error_code(&error),
            "github_active_branch_rules_query_unavailable"
        );
        assert!(!error.to_string().contains("redacted-test-token"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn public_check_runs_retry_without_the_fine_grained_pat() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for (index, response) in [
                "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_string(),
                {
                    let body = r#"{"check_runs":[]}"#;
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                },
            ]
            .into_iter()
            .enumerate()
            {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0_u8; 2048];
                let count = stream.read(&mut request).await.unwrap();
                let request = String::from_utf8_lossy(&request[..count]).to_ascii_lowercase();
                if index == 0 {
                    assert!(request.contains("authorization: bearer redacted-test-token"));
                } else {
                    assert!(!request.contains("authorization:"));
                }
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let client = reqwest::Client::builder().build().unwrap();

        let value = github_observer_json_with_public_fallback(
            &client,
            &format!("http://{address}/check-runs"),
            "redacted-test-token",
            true,
            "github_check_runs_query_unavailable",
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(value, json!({"check_runs":[]}));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn private_check_runs_never_fall_back_to_anonymous_access() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let count = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..count]).to_ascii_lowercase();
            assert!(request.contains("authorization: bearer redacted-test-token"));
            stream
                .write_all(
                    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
        });
        let client = reqwest::Client::builder().build().unwrap();

        let error = github_observer_json_with_public_fallback(
            &client,
            &format!("http://{address}/check-runs"),
            "redacted-test-token",
            false,
            "github_check_runs_query_unavailable",
        )
        .await
        .unwrap_err();

        assert_eq!(
            git_observer_error_code(&error),
            "github_check_runs_query_unavailable"
        );
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
    fn closed_unmerged_github_pull_request_discards_synthetic_merge_sha() {
        let context = GitDeliveryObservationContext {
            execution_id: "gobserve_1".to_string(),
            repository: "https://github.com/example/gitops.git".to_string(),
            base_ref: None,
            head_branch: "pharness/gitops/revision-2".to_string(),
            source_commit_sha: "a".repeat(40),
            pull_request_url: "https://github.com/example/gitops/pull/25".to_string(),
            pull_request_number: 25,
            github_api_url: "https://api.github.com".to_string(),
        };
        let value = json!({
            "number": 25,
            "html_url": "https://github.com/example/gitops/pull/25",
            "state": "closed",
            "merged": false,
            "merge_commit_sha": "b".repeat(40),
            "head": {
                "ref": "pharness/gitops/revision-2",
                "sha": "a".repeat(40),
            },
        });

        let observation = parse_github_pull_request_observation(&value, &context).unwrap();

        assert_eq!(observation.pull_request_state, "closed");
        assert!(!observation.merged);
        assert_eq!(observation.merge_commit_sha, None);
        assert_eq!(observation.head_branch, "pharness/gitops/revision-2");
        assert_eq!(observation.head_commit_sha, "a".repeat(40));
    }

    #[test]
    fn provider_checks_require_both_check_run_and_status_for_duplicate_names() {
        let result = evaluate_github_required_checks(
            &json!([{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"ci/test","integration_id":42}]}}]),
            Some(&json!({"contexts":["legacy/lint"],"checks":[]})),
            &json!({"check_runs":[
                {"name":"ci/test","status":"completed","conclusion":"success","app":{"id":42}},
            ]}),
            &json!({"statuses":[
                {"context":"ci/test","state":"pending"},
                {"context":"legacy/lint","state":"success"},
            ]}),
        )
        .unwrap();
        assert_eq!(result.status, "pending");
        assert_eq!(result.required_checks.as_array().unwrap().len(), 2);

        let passing = evaluate_github_required_checks(
            &json!([{"type":"required_status_checks","parameters":{"required_status_checks":[{"context":"ci/test","integration_id":42}]}}]),
            None,
            &json!({"check_runs":[
                {"name":"ci/test","status":"completed","conclusion":"neutral","app":{"id":42}},
            ]}),
            &json!({"statuses":[]}),
        )
        .unwrap();
        assert_eq!(passing.status, "passing");
    }

    #[test]
    fn authoritative_empty_required_set_is_passing() {
        let result = evaluate_github_required_checks(
            &json!([]),
            None,
            &json!({"check_runs":[]}),
            &json!({"statuses":[]}),
        )
        .unwrap();
        assert_eq!(result.status, "passing");
        assert_eq!(result.required_checks, json!([]));
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
    fn git_writer_classifies_push_failures_without_persisting_provider_output() {
        let push = [
            "-C",
            "/work/git-gexec/repo",
            "push",
            "origin",
            "HEAD:refs/heads/pharness/work-item-1",
        ];
        assert_eq!(
            git_delivery_command_error_code_for_stderr(
                &push,
                b"remote: Write access to repository not granted. requested URL returned error: 403",
            ),
            "git_push_permission_denied"
        );
        assert_eq!(
            git_delivery_command_error_code_for_stderr(
                &push,
                b"fatal: Authentication failed for a redacted repository",
            ),
            "git_push_authentication_failed"
        );
        assert_eq!(
            git_delivery_command_error_code_for_stderr(
                &push,
                b"! [rejected] HEAD -> branch (non-fast-forward)",
            ),
            "git_push_non_fast_forward"
        );
        assert_eq!(
            git_delivery_command_error_code_for_stderr(&push, b"unrecognized safe failure"),
            "git_push_failed"
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
        let source = "apiVersion: kustomize.config.k8s.io/v1beta1\nkind: Kustomization\n\nresources:\n  - deployment.yaml\n\nimages:\n  - name: registry.example.test/team/api\n    newName: registry.example.test/team/api\n    digest: sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n  - name: registry.example.test/team/worker\n    newName: registry.example.test/team/worker\n    digest: sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n";
        let output = update_kustomization_image(
            source,
            "registry.example.test/team/api",
            &format!("registry.example.test/team/api:git-0123456789abcdef@{DIGEST}"),
        )
        .unwrap();

        assert_eq!(
            output,
            source.replacen(
                "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                DIGEST,
                1,
            )
        );
        assert!(!output.contains("git-0123456789abcdef"));
    }

    #[test]
    fn rejects_missing_or_ambiguous_kustomization_image_entries() {
        const DIGEST: &str =
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let missing = update_kustomization_image(
            "images:\n  - name: registry.example.test/team/other\n    digest: sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
            "registry.example.test/team/api",
            &format!("registry.example.test/team/api@{DIGEST}"),
        )
        .unwrap_err();
        assert!(missing.to_string().contains("not found"));

        let ambiguous = update_kustomization_image(
            "images:\n  - name: registry.example.test/team/api\n    digest: sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n  - name: registry.example.test/team/api\n    digest: sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n",
            "registry.example.test/team/api",
            &format!("registry.example.test/team/api@{DIGEST}"),
        )
        .unwrap_err();
        assert!(ambiguous.to_string().contains("ambiguous"));
    }

    #[test]
    fn rejects_non_digest_pinned_kustomization_image_references() {
        let error = update_kustomization_image(
            "images:\n  - name: registry.example.test/team/api\n    digest: sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
            "registry.example.test/team/api",
            "registry.example.test/team/api:latest",
        )
        .unwrap_err();

        assert!(error.to_string().contains("digest pinned"));
    }

    #[test]
    fn rejects_kustomization_mutations_that_are_not_digest_only() {
        const DIGEST: &str =
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let mutable_tag = update_kustomization_image(
            "images:\n  - name: registry.example.test/team/api\n    newTag: latest\n    digest: sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
            "registry.example.test/team/api",
            &format!("registry.example.test/team/api@{DIGEST}"),
        )
        .unwrap_err();
        assert!(mutable_tag.to_string().contains("must not contain newTag"));

        let renamed = update_kustomization_image(
            "images:\n  - name: registry.example.test/team/api\n    newName: mirror.example.test/team/api\n    digest: sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
            "registry.example.test/team/api",
            &format!("registry.example.test/team/api@{DIGEST}"),
        )
        .unwrap_err();
        assert!(renamed.to_string().contains("newName does not match"));

        let missing_digest = update_kustomization_image(
            "images:\n  - name: registry.example.test/team/api\n",
            "registry.example.test/team/api",
            &format!("registry.example.test/team/api@{DIGEST}"),
        )
        .unwrap_err();
        assert!(missing_digest
            .to_string()
            .contains("already contain an immutable sha256 digest"));
    }

    #[test]
    fn scopes_a_shared_existing_digest_to_the_named_image_entry() {
        const OLD: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        const NEW: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let source = format!(
            "images:\n  - name: registry.example.test/team/worker\n    newName: registry.example.test/team/worker\n    digest: {OLD}\n  - name: registry.example.test/team/api\n    newName: registry.example.test/team/api\n    digest: {OLD}\n"
        );
        let output = update_kustomization_image(
            &source,
            "registry.example.test/team/api",
            &format!("registry.example.test/team/api:git-deadbeef@{NEW}"),
        )
        .unwrap();

        assert!(output.contains(&format!(
            "name: registry.example.test/team/worker\n    newName: registry.example.test/team/worker\n    digest: {OLD}"
        )));
        assert!(output.contains(&format!(
            "name: registry.example.test/team/api\n    newName: registry.example.test/team/api\n    digest: {NEW}"
        )));
    }
}
