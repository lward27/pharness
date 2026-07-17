//! Local, allowlisted source workspace provisioning for the coding alpha.
//!
//! This module intentionally accepts only local Git repositories. Remote clone
//! credentials, Git writes, and Kubernetes workspaces are separate capability
//! boundaries and are not smuggled into the first coding loop.

use pharness_runhost::WorkspaceSourceSpec;
use std::path::{Path, PathBuf};
use tokio::process::Command;

#[derive(Clone, Debug)]
pub(crate) struct WorkspaceProvisioner {
    root: PathBuf,
    allowed_repos: Vec<PathBuf>,
    allowed_remote_repos: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct ProvisionedWorkspace {
    pub cwd: PathBuf,
    pub resolved_commit: String,
    pub branch: String,
}

#[derive(Clone, Debug)]
pub(crate) struct GitEvidence {
    pub status: String,
    pub diff: String,
    pub changed_paths: Vec<String>,
}

impl WorkspaceProvisioner {
    #[cfg(test)]
    pub(crate) fn new(root: PathBuf, allowed_repos: Vec<PathBuf>) -> Self {
        Self {
            root,
            allowed_repos,
            allowed_remote_repos: Vec::new(),
        }
    }

    pub(crate) fn with_remote_repos(
        root: PathBuf,
        allowed_repos: Vec<PathBuf>,
        allowed_remote_repos: Vec<String>,
    ) -> Self {
        Self {
            root,
            allowed_repos,
            allowed_remote_repos,
        }
    }

    pub(crate) async fn provision(
        &self,
        work_item_id: &str,
        attempt: u32,
        source_repo: &str,
        source_ref: &str,
    ) -> Result<ProvisionedWorkspace, WorkspaceError> {
        let source = self.allowed_source(source_repo).await?;
        let target = self.root.join(work_item_id).join(attempt.to_string());
        if tokio::fs::try_exists(&target).await.unwrap_or(false) {
            return Err(WorkspaceError::new("workspace target already exists"));
        }
        let parent = target
            .parent()
            .ok_or_else(|| WorkspaceError::new("workspace target has no parent"))?;
        tokio::fs::create_dir_all(parent).await.map_err(|error| {
            WorkspaceError::new(format!("failed to create workspace root: {error}"))
        })?;

        run_git(&[
            "clone",
            "--no-checkout",
            "--no-local",
            source.to_string_lossy().as_ref(),
            target.to_string_lossy().as_ref(),
        ])
        .await?;
        let target_str = target.to_string_lossy().to_string();
        let resolved_commit = run_git(&[
            "-C",
            &target_str,
            "rev-parse",
            "--verify",
            &format!("{source_ref}^{{commit}}"),
        ])
        .await?;
        let branch = format!("pharness/{work_item_id}/attempt-{attempt}");
        run_git(&[
            "-C",
            &target_str,
            "switch",
            "--create",
            &branch,
            &resolved_commit,
        ])
        .await?;
        run_git(&["-C", &target_str, "config", "user.name", "pharness-local"]).await?;
        run_git(&[
            "-C",
            &target_str,
            "config",
            "user.email",
            "pharness@local.invalid",
        ])
        .await?;

        Ok(ProvisionedWorkspace {
            cwd: target,
            resolved_commit,
            branch,
        })
    }

    pub(crate) async fn ensure_managed(&self, path: &Path) -> Result<(), WorkspaceError> {
        let root = tokio::fs::canonicalize(&self.root)
            .await
            .map_err(|_| WorkspaceError::new("workspace root does not exist"))?;
        let path = tokio::fs::canonicalize(path)
            .await
            .map_err(|_| WorkspaceError::new("workspace path does not exist"))?;
        if path.starts_with(root) {
            Ok(())
        } else {
            Err(WorkspaceError::new(
                "workspace path is outside the managed root",
            ))
        }
    }

    pub(crate) fn configured(&self) -> bool {
        !self.allowed_repos.is_empty()
    }

    pub(crate) fn allowed_repo_count(&self) -> usize {
        self.allowed_repos.len()
    }

    pub(crate) fn remote_configured(&self) -> bool {
        !self.allowed_remote_repos.is_empty()
    }

    pub(crate) fn remote_source_allowed(
        &self,
        source: &WorkspaceSourceSpec,
    ) -> Result<(), WorkspaceError> {
        source
            .validate()
            .map_err(|error| WorkspaceError::new(error.to_string()))?;
        let source_repo = normalized_remote_repo(&source.source_repo);
        if self
            .allowed_remote_repos
            .iter()
            .map(|repo| normalized_remote_repo(repo))
            .any(|allowed| allowed == source_repo)
        {
            return Ok(());
        }
        Err(WorkspaceError::new(
            "source repository is not in PHARNESS_WORKSPACE_ALLOWED_REMOTE_REPOS",
        ))
    }

    async fn allowed_source(&self, source_repo: &str) -> Result<PathBuf, WorkspaceError> {
        let candidate = tokio::fs::canonicalize(source_repo)
            .await
            .map_err(|_| WorkspaceError::new("source repository must be an existing local path"))?;
        let candidate_str = candidate.to_string_lossy().to_string();
        let repo_root =
            PathBuf::from(run_git(&["-C", &candidate_str, "rev-parse", "--show-toplevel"]).await?);
        let repo_root = tokio::fs::canonicalize(repo_root)
            .await
            .map_err(|_| WorkspaceError::new("could not canonicalize Git repository root"))?;
        for allowed in &self.allowed_repos {
            if let Ok(allowed) = tokio::fs::canonicalize(allowed).await {
                if allowed == repo_root {
                    return Ok(repo_root);
                }
            }
        }
        Err(WorkspaceError::new(
            "source repository is not in PHARNESS_WORKSPACE_ALLOWED_REPOS",
        ))
    }
}

fn normalized_remote_repo(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

pub(crate) async fn collect_git_evidence(
    cwd: &Path,
    base_commit: &str,
) -> Result<GitEvidence, WorkspaceError> {
    let cwd = cwd.to_string_lossy().to_string();
    let untracked = run_git(&["-C", &cwd, "ls-files", "--others", "--exclude-standard"]).await?;
    let untracked_paths = lines(&untracked);
    if untracked_paths.iter().any(|path| secret_shaped_path(path)) {
        return Err(WorkspaceError::new(
            "workspace contains an untracked secret-shaped path; refusing to capture it",
        ));
    }
    if !untracked_paths.is_empty() {
        let mut args = vec!["-C", cwd.as_str(), "add", "--intent-to-add", "--"];
        args.extend(untracked_paths.iter().map(String::as_str));
        run_git(&args).await?;
    }
    let status = run_git(&["-C", &cwd, "status", "--short"]).await?;
    let changed_paths = lines(&run_git(&["-C", &cwd, "diff", "--name-only", base_commit]).await?);
    if changed_paths.iter().any(|path| secret_shaped_path(path)) {
        return Err(WorkspaceError::new(
            "workspace diff includes a secret-shaped path; refusing to capture it",
        ));
    }
    let diff = run_git(&["-C", &cwd, "diff", "--no-ext-diff", "--binary", base_commit]).await?;
    if diff.len() > 512 * 1024 {
        return Err(WorkspaceError::new(
            "workspace Git diff exceeds the 512 KiB capture limit",
        ));
    }
    Ok(GitEvidence {
        status,
        diff,
        changed_paths,
    })
}

fn lines(value: &str) -> Vec<String> {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn secret_shaped_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    name == ".env"
        || name.starts_with(".env.")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.contains("kubeconfig")
        || name.contains("credential")
        || name.contains("secret")
        || name.contains("token")
}

#[derive(Debug)]
pub(crate) struct WorkspaceError(String);

impl WorkspaceError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for WorkspaceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for WorkspaceError {}

async fn run_git(args: &[&str]) -> Result<String, WorkspaceError> {
    let output = Command::new("git")
        .args(args)
        .output()
        .await
        .map_err(|error| WorkspaceError::new(format!("could not execute git: {error}")))?;
    if !output.status.success() {
        return Err(WorkspaceError::new("Git workspace operation failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::WorkspaceProvisioner;
    use pharness_runhost::WorkspaceSourceSpec;
    use std::path::Path;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    #[tokio::test]
    async fn provisions_an_independent_clone_pinned_to_the_requested_commit() {
        let root = std::env::temp_dir().join(format!(
            "pharness-workspace-test-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let source = root.join("source");
        std::fs::create_dir_all(&source).unwrap();
        git(&source, &["init"]);
        git(&source, &["config", "user.email", "test@example.invalid"]);
        git(&source, &["config", "user.name", "test"]);
        std::fs::write(source.join("README.md"), "base\n").unwrap();
        git(&source, &["add", "README.md"]);
        git(&source, &["commit", "-m", "base"]);
        let base = git(&source, &["rev-parse", "HEAD"]);

        let provisioner = WorkspaceProvisioner::new(root.join("workspaces"), vec![source.clone()]);
        let workspace = provisioner
            .provision("witem_test", 1, source.to_str().unwrap(), "HEAD")
            .await
            .unwrap();
        assert_eq!(workspace.resolved_commit, base);
        assert_eq!(git(&workspace.cwd, &["rev-parse", "HEAD"]), base);
        assert!(workspace.branch.starts_with("pharness/witem_test/"));
        provisioner.ensure_managed(&workspace.cwd).await.unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn rejects_repositories_outside_the_allowlist() {
        let root = std::env::temp_dir().join(format!(
            "pharness-workspace-test-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let source = root.join("source");
        std::fs::create_dir_all(&source).unwrap();
        git(&source, &["init"]);
        let provisioner = WorkspaceProvisioner::new(root.join("workspaces"), vec![]);
        assert!(provisioner
            .provision("witem_test", 1, source.to_str().unwrap(), "HEAD")
            .await
            .is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    fn git(cwd: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(["-C", cwd.to_str().unwrap()])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[test]
    fn permits_only_exact_configured_remote_source() {
        let provisioner = WorkspaceProvisioner::with_remote_repos(
            std::env::temp_dir(),
            Vec::new(),
            vec!["https://github.com/example/finance-app.git".to_string()],
        );
        let source = WorkspaceSourceSpec {
            workspace_id: "ws_test".to_string(),
            source_repo: "https://github.com/example/finance-app.git".to_string(),
            source_ref: "main".to_string(),
            branch: "pharness/test/attempt-1".to_string(),
            resolved_commit: None,
        };
        provisioner.remote_source_allowed(&source).unwrap();

        let source = WorkspaceSourceSpec {
            source_repo: "https://github.com/example/other-app.git".to_string(),
            ..source
        };
        assert!(provisioner.remote_source_allowed(&source).is_err());
    }
}
