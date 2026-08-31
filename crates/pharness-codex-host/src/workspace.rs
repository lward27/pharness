use anyhow::Context;
use globset::{Glob, GlobSet, GlobSetBuilder};
use hmac::{Hmac, Mac};
use pharness_core::{
    EnvironmentProfile, EnvironmentRuntimeSnapshot, EnvironmentSnapshot, PreparationStrategy,
    RepositoryContract,
};
use pharness_runhost::{WorkspaceGitEvidence, WorkspaceSourceSpec};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

const OUTPUT_LIMIT: usize = 128 * 1024;

#[derive(Debug, Clone)]
pub struct WorkspaceBaseline {
    pub source_commit: String,
    pub git_metadata_hash: String,
}

#[derive(Debug, Clone)]
pub struct PreparedEnvironment {
    pub snapshot: EnvironmentSnapshot,
    pub contract_hash: String,
    pub logs: Value,
}

pub async fn checkout_exact_source(
    root: &Path,
    source: &WorkspaceSourceSpec,
    source_reader_token: Option<&str>,
) -> anyhow::Result<String> {
    source.validate()?;
    let expected = source
        .source_commit
        .as_deref()
        .context("portable Codex execution requires an immutable source commit")?;
    if root.join(".git").is_dir() {
        let actual = git_stdout(root, &["rev-parse", "HEAD"]).await?;
        if actual.trim() != expected {
            anyhow::bail!("sticky workspace source SHA does not match the lease");
        }
        configure_runtime_excludes(root)?;
        return Ok(actual.trim().to_string());
    }
    if root.exists() && std::fs::read_dir(root)?.next().is_some() {
        anyhow::bail!("new lease workspace is not empty");
    }
    std::fs::create_dir_all(root)?;
    git_status(root, &["init", "--initial-branch", "pharness-bootstrap"]).await?;
    git_status(root, &["remote", "add", "origin", &source.source_repo]).await?;
    let mut fetch = Command::new("git");
    fetch
        .args(["fetch", "--no-tags", "--depth", "1", "origin", expected])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_TERMINAL_PROMPT", "0");
    if let Some(token) = source_reader_token {
        fetch
            .env("PHARNESS_GIT_PASSWORD", token)
            .env("GIT_ASKPASS", "/usr/lib/pharness-codex-host/git-askpass");
    }
    let output = fetch
        .output()
        .await
        .context("failed to execute git fetch")?;
    if !output.status.success() {
        anyhow::bail!("exact source fetch failed: {}", bounded(&output.stderr));
    }
    git_status(root, &["checkout", "-B", &source.branch, expected]).await?;
    let actual = git_stdout(root, &["rev-parse", "HEAD"]).await?;
    if actual.trim() != expected {
        anyhow::bail!("resolved source SHA does not match the immutable lease");
    }
    configure_runtime_excludes(root)?;
    Ok(actual.trim().to_string())
}

fn configure_runtime_excludes(root: &Path) -> anyhow::Result<()> {
    let path = root.join(".git/info/exclude");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut entries = existing.lines().map(str::to_string).collect::<Vec<_>>();
    for required in ["/.pharness-runtime/", "/node_modules/"] {
        if !entries.iter().any(|entry| entry.trim() == required) {
            entries.push(required.into());
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, format!("{}\n", entries.join("\n")))?;
    Ok(())
}

pub async fn checkout_context_repository(
    root: &Path,
    repository_url: &str,
    source_commit: &str,
    source_reader_token: Option<&str>,
) -> anyhow::Result<()> {
    validate_context_source(repository_url, source_commit)?;
    if root.join(".git").is_dir() {
        let actual = git_stdout(root, &["rev-parse", "HEAD"]).await?;
        let status =
            git_stdout(root, &["status", "--porcelain=v1", "--untracked-files=all"]).await?;
        if actual.trim() != source_commit || !status.trim().is_empty() {
            anyhow::bail!("cached context repository does not match its immutable revision");
        }
        return Ok(());
    }
    if root.exists() && std::fs::read_dir(root)?.next().is_some() {
        anyhow::bail!("new context repository directory is not empty");
    }
    std::fs::create_dir_all(root)?;
    git_status(root, &["init", "--initial-branch", "pharness-context"]).await?;
    git_status(root, &["remote", "add", "origin", repository_url]).await?;
    let mut fetch = Command::new("git");
    fetch
        .args([
            "fetch",
            "--no-tags",
            "--depth",
            "1",
            "origin",
            source_commit,
        ])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_TERMINAL_PROMPT", "0");
    if let Some(token) = source_reader_token {
        fetch
            .env("PHARNESS_GIT_PASSWORD", token)
            .env("GIT_ASKPASS", "/usr/lib/pharness-codex-host/git-askpass");
    }
    let output = fetch.output().await?;
    if !output.status.success() {
        anyhow::bail!("exact context fetch failed: {}", bounded(&output.stderr));
    }
    git_status(root, &["checkout", "--detach", source_commit]).await?;
    let actual = git_stdout(root, &["rev-parse", "HEAD"]).await?;
    if actual.trim() != source_commit {
        anyhow::bail!("resolved context SHA does not match its immutable revision");
    }
    Ok(())
}

fn validate_context_source(repository_url: &str, source_commit: &str) -> anyhow::Result<()> {
    let parsed = reqwest::Url::parse(repository_url)?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some("github.com")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !repository_url.ends_with(".git")
        || source_commit.len() != 40
        || !source_commit.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        anyhow::bail!("context repository must use canonical GitHub HTTPS and a full commit SHA");
    }
    Ok(())
}

pub async fn capture_baseline(
    root: &Path,
    source_commit: &str,
) -> anyhow::Result<WorkspaceBaseline> {
    let head = git_stdout(root, &["rev-parse", "HEAD"]).await?;
    if head.trim() != source_commit {
        anyhow::bail!("workspace HEAD no longer matches the immutable source baseline");
    }
    Ok(WorkspaceBaseline {
        source_commit: source_commit.into(),
        git_metadata_hash: git_metadata_hash(root).await?,
    })
}

pub async fn collect_workspace_evidence(
    root: &Path,
    source: &WorkspaceSourceSpec,
    contract: &RepositoryContract,
    baseline: &WorkspaceBaseline,
) -> anyhow::Result<WorkspaceGitEvidence> {
    let head = git_stdout(root, &["rev-parse", "HEAD"]).await?;
    if head.trim() != baseline.source_commit {
        anyhow::bail!("Codex mutated the workspace source baseline");
    }
    let metadata_hash = git_metadata_hash(root).await?;
    if metadata_hash != baseline.git_metadata_hash {
        anyhow::bail!("Codex mutated Git refs or metadata");
    }
    let status = git_stdout(root, &["status", "--porcelain=v1", "--untracked-files=all"]).await?;
    let changed_paths = parse_status_paths(&status)?;
    let allowlist = writable_globset(contract)?;
    for path in &changed_paths {
        if !allowlist.is_match(path) {
            anyhow::bail!("changed path is outside the RepositoryContract: {path}");
        }
        reject_symlink_escape(root, path)?;
    }
    let diff = git_stdout(
        root,
        &["diff", "--binary", "--no-ext-diff", "--full-index", "--"],
    )
    .await?;
    let untracked_diff = untracked_files_as_patch(root, &status)?;
    Ok(WorkspaceGitEvidence {
        workspace_id: source.workspace_id.clone(),
        base_commit: baseline.source_commit.clone(),
        branch: source.branch.clone(),
        status,
        diff: format!("{diff}{untracked_diff}"),
        changed_paths,
    })
}

pub async fn prepare_environment(
    root: &Path,
    contract: &RepositoryContract,
    profile: &EnvironmentProfile,
    source_sha: &str,
) -> anyhow::Result<PreparedEnvironment> {
    contract.validate_for_profile(profile)?;
    let contract_path = root.join(".pharness/repository.yaml");
    let contract_bytes =
        std::fs::read(&contract_path).context("canonical RepositoryContract is missing")?;
    if contract_bytes.len() > 32 * 1024 {
        anyhow::bail!("RepositoryContract exceeds 32 KiB");
    }
    let loaded: RepositoryContract = serde_yaml::from_slice(&contract_bytes)?;
    if &loaded != contract {
        anyhow::bail!("checked-out RepositoryContract does not match the lease");
    }
    let contract_hash = format!("sha256:{:x}", Sha256::digest(&contract_bytes));
    let lock = root.join(&contract.dependency_lock.path);
    let lock_bytes = std::fs::read(&lock).context("declared dependency lock is missing")?;
    let lock_hash = format!("sha256:{:x}", Sha256::digest(&lock_bytes));
    let declared_lock = normalize_sha256(&contract.dependency_lock.sha256);
    if lock_hash != declared_lock {
        anyhow::bail!("dependency lock hash does not match the RepositoryContract");
    }
    let before = git_stdout(root, &["status", "--porcelain=v1", "--untracked-files=all"]).await?;
    let runtime_root = root.join(".pharness-runtime");
    std::fs::create_dir_all(&runtime_root)?;
    let (runtime, preparation_evidence) = match profile.preparation_strategy {
        PreparationStrategy::PythonHashedRequirements => {
            let venv = runtime_root.join("venv");
            command(root, "python", &["-m", "venv", path_str(&venv)?]).await?;
            let python = venv.join("bin/python");
            command(
                root,
                path_str(&python)?,
                &[
                    "-m",
                    "pip",
                    "install",
                    "--require-hashes",
                    "--only-binary=:all:",
                    "-r",
                    path_str(&lock)?,
                ],
            )
            .await?;
            let python_version = command_stdout(root, path_str(&python)?, &["--version"]).await?;
            (
                EnvironmentRuntimeSnapshot {
                    kind: "python".into(),
                    executable: python.display().to_string(),
                    version: python_version.trim().into(),
                    package_manager_executable: Some("pip".into()),
                    package_manager_version: None,
                    path_entries: vec![venv.join("bin").display().to_string()],
                },
                json!({"strategy":"python_hashed_requirements","venv":venv}),
            )
        }
        PreparationStrategy::NodeNpmCi => {
            let tracked_node_modules =
                git_stdout(root, &["ls-files", "--", "node_modules"]).await?;
            if !tracked_node_modules.trim().is_empty() {
                anyhow::bail!("tracked node_modules is forbidden");
            }
            let cache = runtime_root.join("npm-cache");
            std::fs::create_dir_all(&cache)?;
            command(
                root,
                "npm",
                &[
                    "ci",
                    "--ignore-scripts",
                    "--no-audit",
                    "--no-fund",
                    "--cache",
                    path_str(&cache)?,
                ],
            )
            .await?;
            let node_version = command_stdout(root, "node", &["--version"]).await?;
            let npm_version = command_stdout(root, "npm", &["--version"]).await?;
            (
                EnvironmentRuntimeSnapshot {
                    kind: "node".into(),
                    executable: "node".into(),
                    version: node_version.trim().into(),
                    package_manager_executable: Some("npm".into()),
                    package_manager_version: Some(npm_version.trim().into()),
                    path_entries: vec![root.join("node_modules/.bin").display().to_string()],
                },
                json!({"strategy":"node_npm_ci","cache":cache,"lifecycle_scripts":false}),
            )
        }
    };
    let after = git_stdout(root, &["status", "--porcelain=v1", "--untracked-files=all"]).await?;
    if normalize_preparation_status(&before) != normalize_preparation_status(&after) {
        anyhow::bail!("environment preparation modified tracked repository state");
    }
    let snapshot = EnvironmentSnapshot {
        source_sha: source_sha.into(),
        manifest_sha256: contract_hash.clone(),
        dependency_lock_sha256: lock_hash,
        runner_image_digest: profile.image.clone(),
        runner_revision: profile.revision.clone(),
        os: std::env::consts::OS.into(),
        architecture: normalize_arch(std::env::consts::ARCH).into(),
        effective_user: command_stdout(root, "id", &["-u"]).await?.trim().into(),
        runtime: Some(runtime.clone()),
        python_version: (runtime.kind == "python").then(|| runtime.version.clone()),
        python_path: (runtime.kind == "python").then(|| runtime.executable.clone()),
        writable_paths: contract.writable_paths.clone(),
        unavailable_tools: ["docker", "podman", "apt", "apt-get", "apk"]
            .into_iter()
            .filter(|tool| !executable_exists(tool))
            .map(str::to_string)
            .collect(),
        agent_network: contract.agent_network,
        package_installation: contract.package_installation,
        acceptance_commands: contract.acceptance_commands.clone(),
        preparation_evidence: json!({
            "runtime":preparation_evidence,
            "required_executables":profile.required_executables,
            "runner_platform":profile.platform,
            "lifecycle_scripts_allowed":profile.preparation_strategy.lifecycle_scripts_allowed(),
        }),
    };
    Ok(PreparedEnvironment {
        snapshot,
        contract_hash,
        logs: json!([
            {"step":"checkout","status":"succeeded","source_sha":source_sha},
            {"step":"contract","status":"succeeded"},
            {"step":"dependencies","status":"succeeded"},
            {"step":"tracked_state","status":"unchanged"}
        ]),
    })
}

pub fn signed_snapshot(token: &str, snapshot: &Value) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(token.as_bytes())
        .expect("HMAC-SHA256 accepts arbitrary key lengths");
    mac.update(snapshot.to_string().as_bytes());
    format!("hmac-sha256:{:x}", mac.finalize().into_bytes())
}

pub fn writable_roots(root: &Path, contract: &RepositoryContract) -> anyhow::Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    for pattern in &contract.writable_paths {
        let prefix = pattern.strip_suffix("/**").unwrap_or(pattern);
        let candidate = root.join(prefix);
        if !candidate.starts_with(root) || prefix.is_empty() || prefix.contains("..") {
            anyhow::bail!("invalid writable path {pattern}");
        }
        roots.push(candidate);
    }
    roots.sort();
    roots.dedup();
    Ok(roots)
}

async fn git_metadata_hash(root: &Path) -> anyhow::Result<String> {
    let material = format!(
        "{}\n{}",
        git_stdout(root, &["rev-parse", "HEAD"]).await?,
        git_stdout(
            root,
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/heads",
                "refs/tags",
            ],
        )
        .await?
    );
    Ok(format!("sha256:{:x}", Sha256::digest(material.as_bytes())))
}

fn writable_globset(contract: &RepositoryContract) -> anyhow::Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in &contract.writable_paths {
        builder
            .add(Glob::new(pattern).with_context(|| format!("invalid writable glob {pattern}"))?);
    }
    builder
        .build()
        .context("failed to build writable path matcher")
}

fn parse_status_paths(status: &str) -> anyhow::Result<Vec<String>> {
    let mut paths = Vec::new();
    for line in status.lines() {
        if line.len() < 4 {
            anyhow::bail!("git status emitted a malformed entry");
        }
        let path = line[3..].split(" -> ").last().unwrap_or_default();
        if path.is_empty() || path.starts_with('/') || path.split('/').any(|part| part == "..") {
            anyhow::bail!("git status emitted an unsafe path");
        }
        paths.push(path.into());
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn reject_symlink_escape(root: &Path, relative: &str) -> anyhow::Result<()> {
    let candidate = root.join(relative);
    let mut current = candidate.as_path();
    while let Some(parent) = current.parent() {
        if parent == root {
            break;
        }
        if let Ok(metadata) = std::fs::symlink_metadata(current) {
            if metadata.file_type().is_symlink() {
                let canonical = std::fs::canonicalize(current)?;
                if !canonical.starts_with(root) {
                    anyhow::bail!("changed path traverses a symlink outside the workspace");
                }
            }
        }
        current = parent;
    }
    Ok(())
}

fn untracked_files_as_patch(root: &Path, status: &str) -> anyhow::Result<String> {
    let mut result = String::new();
    for line in status.lines().filter(|line| line.starts_with("?? ")) {
        let path = &line[3..];
        let bytes = std::fs::read(root.join(path))?;
        if bytes.len() > 2 * 1024 * 1024 {
            anyhow::bail!("untracked file exceeds the 2 MiB evidence limit: {path}");
        }
        result.push_str(&format!(
            "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n"
        ));
        for (index, line) in String::from_utf8_lossy(&bytes).lines().enumerate() {
            if index == 0 {
                result.push_str(&format!(
                    "@@ -0,0 +1,{} @@\n",
                    String::from_utf8_lossy(&bytes).lines().count()
                ));
            }
            result.push('+');
            result.push_str(line);
            result.push('\n');
        }
    }
    Ok(result)
}

async fn git_status(root: &Path, args: &[&str]) -> anyhow::Result<()> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .await?;
    if !output.status.success() {
        anyhow::bail!("git command failed: {}", bounded(&output.stderr));
    }
    Ok(())
}

async fn git_stdout(root: &Path, args: &[&str]) -> anyhow::Result<String> {
    command_stdout(root, "git", args).await
}

async fn command(root: &Path, executable: &str, args: &[&str]) -> anyhow::Result<()> {
    let output = Command::new(executable)
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .await
        .with_context(|| format!("failed to start {executable}"))?;
    if !output.status.success() {
        anyhow::bail!("{executable} failed: {}", bounded(&output.stderr));
    }
    Ok(())
}

async fn command_stdout(root: &Path, executable: &str, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new(executable)
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .await
        .with_context(|| format!("failed to start {executable}"))?;
    if !output.status.success() {
        anyhow::bail!("{executable} failed: {}", bounded(&output.stderr));
    }
    String::from_utf8(output.stdout).context("command output was not UTF-8")
}

fn path_str(path: &Path) -> anyhow::Result<&str> {
    path.to_str().context("runtime path is not valid UTF-8")
}

fn normalize_sha256(value: &str) -> String {
    if value.starts_with("sha256:") {
        value.to_string()
    } else {
        format!("sha256:{value}")
    }
}

fn normalize_arch(value: &str) -> &str {
    match value {
        "x86_64" => "amd64",
        other => other,
    }
}

fn normalize_preparation_status(status: &str) -> Vec<&str> {
    status
        .lines()
        .filter(|line| !line.ends_with(" .pharness-runtime/") && !line.contains(" node_modules/"))
        .collect()
}

fn executable_exists(executable: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(executable).is_file()))
}

fn bounded(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(OUTPUT_LIMIT)]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_paths_are_deduplicated_and_safe() {
        assert_eq!(
            parse_status_paths(" M src/lib.rs\n?? tests/new.rs\n M src/lib.rs\n").unwrap(),
            vec!["src/lib.rs", "tests/new.rs"]
        );
        assert!(parse_status_paths("?? ../secret\n").is_err());
    }

    #[test]
    fn snapshot_signature_is_stable_and_token_bound() {
        let payload = json!({"source_sha":"abc"});
        assert_eq!(
            signed_snapshot("a", &payload),
            signed_snapshot("a", &payload)
        );
        assert_ne!(
            signed_snapshot("a", &payload),
            signed_snapshot("b", &payload)
        );
    }

    #[test]
    fn runtime_excludes_are_idempotent_and_do_not_hide_tracked_files() {
        let root = std::env::temp_dir().join(format!(
            "pharness-runtime-excludes-{}",
            uuid::Uuid::now_v7().simple()
        ));
        std::fs::create_dir_all(root.join(".git/info")).unwrap();
        std::fs::write(root.join(".git/info/exclude"), "# existing\n").unwrap();
        configure_runtime_excludes(&root).unwrap();
        configure_runtime_excludes(&root).unwrap();
        let document = std::fs::read_to_string(root.join(".git/info/exclude")).unwrap();
        assert_eq!(document.matches("/.pharness-runtime/").count(), 1);
        assert_eq!(document.matches("/node_modules/").count(), 1);
        assert!(document.contains("# existing"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
