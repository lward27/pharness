use crate::api::{ClaimedLeaseEnvelope, HostApiClient, HostCapabilities, LeaseApiClient};
use crate::config::{
    ContextRepositoryMount, ExecutionMode, HostConfig, HostIdentity, LeaseExecutionConfig,
};
use crate::executor;
use crate::workspace::{checkout_context_repository, checkout_exact_source, writable_roots};
use anyhow::Context;
use pharness_core::RepositoryContract;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

pub async fn serve(config: HostConfig) -> anyhow::Result<()> {
    config.validate()?;
    ensure_directories(&config)?;
    ensure_authentication_boundary(&config).await?;
    let identity = HostIdentity::load(&config.identity_file)?;
    let api = HostApiClient::new(
        &config.api_url,
        identity.host_id.clone(),
        identity.host_credential.clone(),
    )?;
    resume_local_lease_if_present(&config, &api, &identity).await?;
    loop {
        ensure_authentication_boundary(&config).await?;
        api.heartbeat(&capabilities(&config, true).await?).await?;
        match api.claim().await? {
            Some(claimed) => {
                persist_and_execute_lease(&config, &api, &identity, claimed).await?;
            }
            None => tokio::time::sleep(Duration::from_secs(config.poll_seconds)).await,
        }
    }
}

pub async fn enroll(
    config_path: &Path,
    enrollment_id: &str,
    enrollment_token: &str,
) -> anyhow::Result<()> {
    let config = HostConfig::load(config_path)?;
    ensure_directories(&config)?;
    let response = HostApiClient::enroll(
        &config.api_url,
        &crate::api::EnrollmentExchange {
            enrollment_id,
            enrollment_token,
            platform: "linux",
            architecture: "amd64",
        },
    )
    .await?;
    let identity = HostIdentity {
        host_id: response.host.id,
        host_credential: response.host_credential,
        display_name: response.host.display_name,
        host_pool: response.host.host_pool,
    };
    write_secret_json(&config.identity_file, &identity)?;
    tracing::info!(
        heartbeat_interval_seconds = response.heartbeat_interval_seconds,
        "agent host enrollment accepted"
    );
    println!("enrolled agent host {}", identity.host_id);
    Ok(())
}

pub async fn check(config: &HostConfig) -> anyhow::Result<()> {
    config.validate()?;
    if config.execution_mode == ExecutionMode::Standalone {
        version(Path::new("slirp4netns"), &["--version"])
            .await
            .context("standalone Codex hosts require slirp4netns for rootless inference egress")?;
    }
    verify_authentication_boundary(config).await?;
    let caps = capabilities(config, true).await?;
    if !caps.authentication_ready {
        anyhow::bail!("Codex authentication is not ready");
    }
    println!(
        "Codex host is ready: codex={}, mode={}, profiles={}",
        caps.codex_version,
        caps.execution_mode,
        caps.supported_profiles.join(",")
    );
    Ok(())
}

pub async fn protocol_smoke(config: &HostConfig, model: &str, effort: &str) -> anyhow::Result<()> {
    config.validate()?;
    let probe_root = config
        .state_dir
        .join("protocol-smoke")
        .join(uuid::Uuid::now_v7().simple().to_string());
    let codex_home = probe_root.join("codex-home");
    let workspace = probe_root.join("workspace");
    std::fs::create_dir_all(&codex_home)?;
    std::fs::create_dir_all(&workspace)?;
    if let Some(auth_file) = config.codex_auth_file.as_deref() {
        copy_secret(auth_file, &codex_home.join("auth.json"))?;
    }
    let app_config = pharness_codex_host::app_server::AppServerConfig {
        codex_path: config.codex_path.clone(),
        codex_home,
        cwd: workspace,
        model: model.into(),
        reasoning_effort: effort.into(),
        prompt: "Return the exact structured object requested by the output schema. Do not run commands or inspect files.".into(),
        output_schema: serde_json::json!({
            "type":"object",
            "additionalProperties":false,
            "required":["ok","driver"],
            "properties":{
                "ok":{"type":"boolean","const":true},
                "driver":{"type":"string","const":"codex_app_server"}
            }
        }),
        workspace_write: false,
        writable_roots: Vec::new(),
        denied_read_paths: vec![credential_file(config)?.to_path_buf()],
        environment: BTreeMap::new(),
        upstream_api_key: if config.authentication_class == "api_key" {
            read_optional_secret(config.api_key_file.as_deref())?
        } else {
            None
        },
    };
    let mut app = pharness_codex_host::app_server::AppServerSession::start(&app_config).await?;
    let command = app
        .exec_sandboxed_command(
            &app_config.cwd,
            "printf pharness-command-sandbox",
            &BTreeMap::new(),
            Duration::from_secs(10),
        )
        .await?;
    if command.exit_code != Some(0) || command.stdout.trim() != "pharness-command-sandbox" {
        anyhow::bail!("Codex App Server command sandbox protocol smoke failed");
    }
    let thread_id = app.start_or_resume_thread(&app_config, None).await?;
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    let outcome = app
        .run_turn(&app_config, &thread_id, cancel_rx, Duration::from_secs(120))
        .await?;
    app.shutdown().await?;
    let _ = std::fs::remove_dir_all(probe_root);
    if outcome.status != "completed"
        || outcome
            .structured_output
            .as_ref()
            .and_then(|value| value.get("ok"))
            .and_then(serde_json::Value::as_bool)
            != Some(true)
    {
        anyhow::bail!("Codex App Server protocol smoke did not return the required output");
    }
    println!(
        "Codex protocol smoke passed: model={model}, effort={effort}, thread_id={}",
        outcome.thread_id
    );
    Ok(())
}

async fn persist_and_execute_lease(
    config: &HostConfig,
    api: &HostApiClient,
    identity: &HostIdentity,
    claimed: ClaimedLeaseEnvelope,
) -> anyhow::Result<()> {
    let local = local_lease_config(config, identity, &claimed)?;
    let path = lease_file(config, &claimed.lease.id);
    write_secret_json(&path, &local)?;
    let result = execute_claimed(config, api, identity, &claimed, &local).await;
    match result {
        Ok(()) => {
            let _ = std::fs::remove_file(path);
            Ok(())
        }
        Err(error) => {
            tracing::error!(lease_id=%claimed.lease.id,%error,"agent lease execution stopped");
            let lease_api = LeaseApiClient::new(
                &config.api_url,
                identity.host_id.clone(),
                claimed.lease.id.clone(),
                claimed.lease_token.clone(),
            )?;
            let _ = lease_api
                .pause(
                    "agent_host_unavailable",
                    &bounded(&error.to_string(), 2_000),
                )
                .await;
            Err(error)
        }
    }
}

async fn execute_claimed(
    config: &HostConfig,
    api: &HostApiClient,
    identity: &HostIdentity,
    claimed: &ClaimedLeaseEnvelope,
    local: &LeaseExecutionConfig,
) -> anyhow::Result<()> {
    let lease_api = LeaseApiClient::new(
        &config.api_url,
        identity.host_id.clone(),
        claimed.lease.id.clone(),
        claimed.lease_token.clone(),
    )?;
    let spec = lease_api.context().await?;
    let source = spec
        .run
        .workspace_source
        .as_ref()
        .context("claimed Run has no workspace source")?;
    let source_token = read_optional_secret(config.source_reader_token_file.as_deref())?;
    checkout_exact_source(&local.workspace_path, source, source_token.as_deref()).await?;
    let contract: RepositoryContract = serde_json::from_value(
        spec.run
            .execution_target_json
            .get("repository_contract")
            .cloned()
            .context("claimed Run has no RepositoryContract")?,
    )?;
    let heartbeat_api = api.clone();
    let heartbeat_config = config.clone();
    let heartbeat = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            if let Err(error) = heartbeat_api
                .heartbeat(
                    &capabilities(&heartbeat_config, false)
                        .await
                        .unwrap_or_else(|_| {
                            unavailable_capabilities(
                                &heartbeat_config,
                                "capability collection failed",
                            )
                        }),
                )
                .await
            {
                tracing::warn!(%error, "host heartbeat failed during an active lease");
            }
        }
    });
    let mut execution_local = local.clone();
    execution_local.remote_thread_id =
        read_resume_thread(&local.codex_home)?.or_else(|| local.remote_thread_id.clone());
    execution_local.context_repositories =
        prepare_context_repositories(config, &spec, source_token.as_deref()).await?;
    let deterministic_test = spec
        .run
        .execution_target_json
        .pointer("/repo_mode/deterministic_test")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let result = match config.execution_mode {
        ExecutionMode::Standalone => {
            run_in_podman(
                config,
                &execution_local,
                &contract,
                &claimed.lease.runner_image,
                deterministic_test,
            )
            .await
        }
        ExecutionMode::Kubernetes => executor::execute_lease(execution_local).await,
    };
    heartbeat.abort();
    result
}

async fn run_in_podman(
    config: &HostConfig,
    local: &LeaseExecutionConfig,
    contract: &RepositoryContract,
    image: &str,
    deterministic_test: bool,
) -> anyhow::Result<()> {
    let lease_dir = local
        .codex_home
        .parent()
        .context("lease Codex home has no parent")?;
    std::fs::create_dir_all(&local.codex_home)?;
    let transient_auth = local.codex_home.join("auth.json");
    if let Some(auth_file) = config.codex_auth_file.as_deref() {
        copy_secret(auth_file, &transient_auth)?;
    }
    let runtime = local.workspace_path.join(".pharness-runtime");
    std::fs::create_dir_all(&runtime)?;
    let node_modules = local.workspace_path.join("node_modules");
    std::fs::create_dir_all(&node_modules)?;
    let container_config = LeaseExecutionConfig {
        api_url: local.api_url.clone(),
        host_id: local.host_id.clone(),
        lease_id: local.lease_id.clone(),
        lease_token: local.lease_token.clone(),
        workspace_path: PathBuf::from("/workspace"),
        codex_path: PathBuf::from("/usr/local/bin/codex"),
        codex_home: PathBuf::from("/var/lib/pharness-codex"),
        authentication_class: local.authentication_class.clone(),
        remote_thread_id: local.remote_thread_id.clone(),
        protocol_restart_count: local.protocol_restart_count,
        api_key_file: config
            .api_key_file
            .as_ref()
            .map(|_| PathBuf::from("/run/secrets/pharness-openai-api-key")),
        context_repositories: local
            .context_repositories
            .iter()
            .map(|context| ContextRepositoryMount {
                repository_id: context.repository_id.clone(),
                source_commit: context.source_commit.clone(),
                path: PathBuf::from("/context").join(&context.repository_id),
            })
            .collect(),
    };
    let container_config_path = lease_dir.join("container-lease.json");
    write_secret_json(&container_config_path, &container_config)?;
    let mut command = Command::new(&config.podman_path);
    command
        .args(podman_isolation_args(deterministic_test))
        .arg("--volume")
        .arg(format!("{}:/workspace:ro", local.workspace_path.display()))
        .arg("--volume")
        .arg(format!(
            "{}:/workspace/.pharness-runtime:rw",
            runtime.display()
        ))
        .arg("--volume")
        .arg(format!(
            "{}:/workspace/node_modules:rw",
            node_modules.display()
        ))
        .arg("--volume")
        .arg(format!(
            "{}:/var/lib/pharness-codex:rw",
            local.codex_home.display()
        ))
        .arg("--volume")
        .arg(format!(
            "{}:/run/pharness/lease.json:ro",
            container_config_path.display()
        ));
    if let Some(api_key_file) = &config.api_key_file {
        command.arg("--secret").arg(format!(
            "id=pharness-openai-api-key,src={},target=pharness-openai-api-key,mode=0400",
            api_key_file.display()
        ));
    }
    for context in &local.context_repositories {
        command.arg("--volume").arg(format!(
            "{}:/context/{}:ro",
            context.path.display(),
            context.repository_id
        ));
    }
    for root in writable_roots(&local.workspace_path, contract)? {
        if !root.exists() {
            if root.extension().is_some() {
                if let Some(parent) = root.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&root, [])?;
            } else {
                std::fs::create_dir_all(&root)?;
            }
        }
        let relative = root.strip_prefix(&local.workspace_path)?;
        command.arg("--volume").arg(format!(
            "{}:/workspace/{}:rw",
            root.display(),
            relative.display()
        ));
    }
    command
        .arg(image)
        .args(["execute-lease", "--config", "/run/pharness/lease.json"])
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let status = command
        .status()
        .await
        .context("failed to start rootless Podman")?;
    let _ = std::fs::remove_file(&transient_auth);
    if !status.success() {
        anyhow::bail!("Codex runner container exited with {status}");
    }
    Ok(())
}

fn podman_isolation_args(deterministic_test: bool) -> Vec<&'static str> {
    let mut args = vec![
        "run",
        "--rm",
        "--read-only",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges",
        "--userns=keep-id:uid=65532,gid=65532",
        "--tmpfs=/tmp:rw,nosuid,nodev,noexec,size=512m",
        "--entrypoint=/usr/local/bin/pharness-codex-host",
    ];
    if deterministic_test {
        args.push("--network=none");
    } else {
        // Podman's newer `pasta` default cannot enter the rootless network
        // namespace when the host runs as a hardened system service without a
        // login session. Select the installed, userspace slirp backend
        // explicitly so only the App Server's inference channel has egress;
        // Codex command execution remains network-denied by its sandbox.
        args.push("--network=slirp4netns");
    }
    args
}

async fn resume_local_lease_if_present(
    config: &HostConfig,
    api: &HostApiClient,
    identity: &HostIdentity,
) -> anyhow::Result<()> {
    let leases = config.state_dir.join("leases");
    if !leases.exists() {
        return Ok(());
    }
    let mut files = std::fs::read_dir(&leases)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    files.sort();
    if let Some(path) = files.first() {
        let mut local: LeaseExecutionConfig = serde_json::from_slice(&std::fs::read(path)?)?;
        local.protocol_restart_count = local.protocol_restart_count.saturating_add(1);
        local.remote_thread_id =
            read_resume_thread(&local.codex_home)?.or_else(|| local.remote_thread_id.clone());
        write_secret_json(path, &local)?;
        let lease_api = LeaseApiClient::new(
            &local.api_url,
            local.host_id.clone(),
            local.lease_id.clone(),
            local.lease_token.clone(),
        )?;
        let spec = lease_api.context().await?;
        let claimed = ClaimedLeaseEnvelope {
            lease: crate::api::ClaimedLease {
                id: local.lease_id.clone(),
                run_id: spec.run.run_id.clone(),
                stage_execution_id: spec
                    .run
                    .execution_target_json
                    .pointer("/repo_mode/stage_execution_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .into(),
                host_pool: identity.host_pool.clone(),
                workspace_id: spec
                    .run
                    .workspace_source
                    .as_ref()
                    .map(|source| source.workspace_id.clone())
                    .unwrap_or_default(),
                environment_profile_id: spec
                    .run
                    .execution_target_json
                    .get("environment_profile_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .into(),
                runner_image: spec
                    .run
                    .execution_target_json
                    .pointer("/agent_execution/binding/runner_image")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .into(),
                binding_hash: spec
                    .run
                    .execution_target_json
                    .pointer("/agent_execution/binding/binding_hash")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("controller-deterministic-test")
                    .into(),
                remote_thread_id: local.remote_thread_id.clone(),
            },
            lease_token: local.lease_token.clone(),
        };
        execute_claimed(config, api, identity, &claimed, &local).await?;
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn local_lease_config(
    config: &HostConfig,
    identity: &HostIdentity,
    claimed: &ClaimedLeaseEnvelope,
) -> anyhow::Result<LeaseExecutionConfig> {
    let lease_root = config.state_dir.join("lease-state").join(&claimed.lease.id);
    std::fs::create_dir_all(&lease_root)?;
    Ok(LeaseExecutionConfig {
        api_url: config.api_url.clone(),
        host_id: identity.host_id.clone(),
        lease_id: claimed.lease.id.clone(),
        lease_token: claimed.lease_token.clone(),
        workspace_path: config.workspace_root.join(&claimed.lease.workspace_id),
        codex_path: config.codex_path.clone(),
        codex_home: lease_root.join("codex-home"),
        authentication_class: config.authentication_class.clone(),
        remote_thread_id: claimed.lease.remote_thread_id.clone(),
        protocol_restart_count: 0,
        api_key_file: config.api_key_file.clone(),
        context_repositories: Vec::new(),
    })
}

fn read_resume_thread(codex_home: &Path) -> anyhow::Result<Option<String>> {
    let path = codex_home.join("pharness-resume.json");
    if !path.exists() {
        return Ok(None);
    }
    let document: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let thread_id = document
        .get("remote_thread_id")
        .and_then(serde_json::Value::as_str)
        .context("Codex resume marker has no remote thread ID")?;
    if thread_id.is_empty()
        || thread_id.len() > 128
        || !thread_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        anyhow::bail!("Codex resume marker contains an invalid thread ID");
    }
    Ok(Some(thread_id.into()))
}

async fn prepare_context_repositories(
    config: &HostConfig,
    spec: &pharness_runhost::AttemptSpec,
    source_reader_token: Option<&str>,
) -> anyhow::Result<Vec<ContextRepositoryMount>> {
    let contexts = spec
        .run
        .execution_target_json
        .pointer("/agent_context/pinned_context_repositories")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if contexts.len() > 4 {
        anyhow::bail!("lease contains more than four context repositories");
    }
    let mut result = Vec::new();
    for context in contexts {
        let repository_id = context
            .get("repository_id")
            .and_then(serde_json::Value::as_str)
            .context("context repository has no identity")?;
        let repository_url = context
            .get("canonical_url")
            .and_then(serde_json::Value::as_str)
            .context("context repository has no canonical URL")?;
        let source_commit = context
            .get("source_commit")
            .and_then(serde_json::Value::as_str)
            .context("context repository has no immutable SHA")?;
        if repository_id.is_empty()
            || repository_id.len() > 128
            || !repository_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
            || repository_id == "."
            || repository_id == ".."
        {
            anyhow::bail!("context repository identity is not path safe");
        }
        let path = config
            .state_dir
            .join("context-repositories")
            .join(repository_id)
            .join(source_commit.to_ascii_lowercase());
        checkout_context_repository(&path, repository_url, source_commit, source_reader_token)
            .await?;
        result.push(ContextRepositoryMount {
            repository_id: repository_id.into(),
            source_commit: source_commit.to_ascii_lowercase(),
            path,
        });
    }
    result.sort_by(|left, right| left.repository_id.cmp(&right.repository_id));
    Ok(result)
}

async fn capabilities(config: &HostConfig, idle: bool) -> anyhow::Result<HostCapabilities> {
    let codex_version = version(&config.codex_path, &["--version"]).await?;
    let podman_version = if config.execution_mode == ExecutionMode::Standalone {
        Some(version(&config.podman_path, &["--version"]).await?)
    } else {
        None
    };
    Ok(HostCapabilities {
        platform: "linux".into(),
        architecture: "amd64".into(),
        codex_version: codex_version.clone(),
        podman_version,
        execution_mode: config.execution_mode.as_str().into(),
        authentication_class: config.authentication_class.clone(),
        authentication_ready: authentication_boundary_current(config, &codex_version),
        supported_profiles: config.runner_images.keys().cloned().collect(),
        runner_images: config.runner_images.clone(),
        available_slots: if idle { config.available_slots } else { 0 },
        storage: storage_summary(&config.workspace_root),
    })
}

fn unavailable_capabilities(config: &HostConfig, blocker: &str) -> HostCapabilities {
    HostCapabilities {
        platform: "linux".into(),
        architecture: "amd64".into(),
        codex_version: "unavailable".into(),
        podman_version: None,
        execution_mode: config.execution_mode.as_str().into(),
        authentication_class: config.authentication_class.clone(),
        authentication_ready: false,
        supported_profiles: Vec::new(),
        runner_images: BTreeMap::new(),
        available_slots: 0,
        storage: serde_json::json!({"status":"unavailable","blocker":blocker}),
    }
}

fn storage_summary(root: &Path) -> serde_json::Value {
    serde_json::json!({
        "workspace_root_ready":root.is_dir(),
        "persistent":true,
    })
}

async fn version(executable: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new(executable).args(args).output().await?;
    if !output.status.success() {
        anyhow::bail!("{} version check failed", executable.display());
    }
    Ok(String::from_utf8(output.stdout)?.trim().into())
}

fn ensure_directories(config: &HostConfig) -> anyhow::Result<()> {
    for path in [
        &config.state_dir,
        &config.workspace_root,
        &config.state_dir.join("leases"),
        &config.state_dir.join("lease-state"),
    ] {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

fn lease_file(config: &HostConfig, lease_id: &str) -> PathBuf {
    config
        .state_dir
        .join("leases")
        .join(format!("{lease_id}.json"))
}

fn write_secret_json(path: &Path, value: &impl serde::Serialize) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
    std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(temporary, path)?;
    Ok(())
}

async fn verify_authentication_boundary(config: &HostConfig) -> anyhow::Result<()> {
    let credential_file = credential_file(config)?;
    if !credential_file.is_file() {
        anyhow::bail!("Codex authentication material is unavailable");
    }
    let probe_root = config
        .state_dir
        .join("auth-boundary-probe")
        .join(uuid::Uuid::now_v7().simple().to_string());
    let codex_home = probe_root.join("codex-home");
    let workspace = probe_root.join("workspace");
    std::fs::create_dir_all(&codex_home)?;
    std::fs::create_dir_all(&workspace)?;
    let verification = async {
        if config.authentication_class == "chatgpt_session" {
            copy_secret(credential_file, &codex_home.join("auth.json"))?;
        }
        let app_config = pharness_codex_host::app_server::AppServerConfig {
            codex_path: config.codex_path.clone(),
            codex_home,
            cwd: workspace,
            model: "gpt-5.6-sol".into(),
            reasoning_effort: "low".into(),
            prompt: "PHarness authentication boundary probe".into(),
            output_schema: serde_json::json!({"type":"object"}),
            workspace_write: false,
            writable_roots: Vec::new(),
            denied_read_paths: vec![credential_file.to_path_buf()],
            environment: BTreeMap::new(),
            upstream_api_key: if config.authentication_class == "api_key" {
                read_optional_secret(Some(credential_file))?
            } else {
                None
            },
        };
        let app = pharness_codex_host::app_server::AppServerSession::start(&app_config).await?;
        app.shutdown().await?;
        let code_mode_host =
            pharness_codex_host::app_server::code_mode_host_path(&config.codex_path)?;
        Ok::<_, anyhow::Error>((
            version(&config.codex_path, &["--version"]).await?,
            file_sha256(&config.codex_path)?,
            file_sha256(&code_mode_host)?,
            authentication_metadata(credential_file)?,
        ))
    }
    .await;
    let cleanup = std::fs::remove_dir_all(&probe_root);
    let (codex_version, codex_sha256, code_mode_host_sha256, auth_metadata) = match verification {
        Ok(evidence) => {
            cleanup.context("failed to remove the Codex authentication boundary probe")?;
            evidence
        }
        Err(error) => {
            if let Err(cleanup_error) = cleanup {
                tracing::warn!(
                    error = %cleanup_error,
                    "failed to clean a rejected Codex authentication boundary probe"
                );
            }
            return Err(error);
        }
    };
    write_secret_json(
        &config.state_dir.join("auth-boundary.json"),
        &serde_json::json!({
            "schema_version":"pharness.dev/codex-auth-boundary/v1alpha2",
            "codex_version":codex_version,
            "codex_sha256":codex_sha256,
            "code_mode_host_sha256":code_mode_host_sha256,
            "authentication_metadata":auth_metadata,
            "verified_at":current_unix_seconds(),
        }),
    )?;
    Ok(())
}

async fn ensure_authentication_boundary(config: &HostConfig) -> anyhow::Result<()> {
    let codex_version = version(&config.codex_path, &["--version"]).await?;
    if authentication_boundary_current(config, &codex_version) {
        return Ok(());
    }
    verify_authentication_boundary(config).await
}

fn authentication_boundary_current(config: &HostConfig, codex_version: &str) -> bool {
    let Ok(credential_file) = credential_file(config) else {
        return false;
    };
    if !credential_file.is_file() {
        return false;
    }
    let marker = std::fs::read(config.state_dir.join("auth-boundary.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let current_codex_sha256 = file_sha256(&config.codex_path).ok();
    let current_code_mode_host_sha256 =
        pharness_codex_host::app_server::code_mode_host_path(&config.codex_path)
            .ok()
            .and_then(|path| file_sha256(&path).ok());
    let current_auth_metadata = authentication_metadata(credential_file).ok();
    marker.is_some_and(|marker| {
        marker
            .get("codex_version")
            .and_then(serde_json::Value::as_str)
            == Some(codex_version)
            && marker
                .get("codex_sha256")
                .and_then(serde_json::Value::as_str)
                == current_codex_sha256.as_deref()
            && marker
                .get("code_mode_host_sha256")
                .and_then(serde_json::Value::as_str)
                == current_code_mode_host_sha256.as_deref()
            && marker.get("authentication_metadata") == current_auth_metadata.as_ref()
    })
}

fn credential_file(config: &HostConfig) -> anyhow::Result<&Path> {
    match config.authentication_class.as_str() {
        "chatgpt_session" => config
            .codex_auth_file
            .as_deref()
            .context("chatgpt_session has no authentication file"),
        "api_key" => config
            .api_key_file
            .as_deref()
            .context("api_key authentication has no key file"),
        _ => anyhow::bail!("host authentication class is unsupported"),
    }
}

fn file_sha256(path: &Path) -> anyhow::Result<String> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read executable {}", path.display()))?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn authentication_metadata(path: &Path) -> anyhow::Result<serde_json::Value> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to inspect authentication file {}", path.display()))?;
    let modified_nanos = metadata
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string();
    Ok(serde_json::json!({
        "length": metadata.len(),
        "modified_unix_nanos": modified_nanos,
    }))
}

fn copy_secret(source: &Path, destination: &Path) -> anyhow::Result<()> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(source, destination)?;
    std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

fn current_unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn read_optional_secret(path: Option<&Path>) -> anyhow::Result<Option<String>> {
    path.map(|path| {
        std::fs::read_to_string(path)
            .with_context(|| format!("failed to read source-reader credential {}", path.display()))
            .map(|value| value.trim().to_string())
    })
    .transpose()
}

fn bounded(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_test_container_has_no_network_and_keeps_runner_uid() {
        let deterministic = podman_isolation_args(true);
        assert!(deterministic.contains(&"--network=none"));
        assert!(!deterministic.contains(&"--network=slirp4netns"));
        assert!(deterministic.contains(&"--userns=keep-id:uid=65532,gid=65532"));
        assert!(deterministic.contains(&"--read-only"));
        let inference = podman_isolation_args(false);
        assert!(!inference.contains(&"--network=none"));
        assert!(inference.contains(&"--network=slirp4netns"));
    }

    #[test]
    fn resume_marker_is_bounded_and_validated() {
        let root = std::env::temp_dir().join(format!(
            "pharness-codex-resume-{}",
            uuid::Uuid::now_v7().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("pharness-resume.json"),
            br#"{"remote_thread_id":"thread-valid_123"}"#,
        )
        .unwrap();
        assert_eq!(
            read_resume_thread(&root).unwrap().as_deref(),
            Some("thread-valid_123")
        );
        std::fs::write(
            root.join("pharness-resume.json"),
            br#"{"remote_thread_id":"../../unsafe"}"#,
        )
        .unwrap();
        assert!(read_resume_thread(&root).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
