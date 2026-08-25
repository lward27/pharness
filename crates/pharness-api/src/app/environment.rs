use crate::dto::EnvironmentProfileResponse;
use pharness_core::{EnvironmentProfile, RepositoryContract};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;

pub fn load_environment_profiles() -> Vec<EnvironmentProfile> {
    let Some(raw) = std::env::var("PHARNESS_ENVIRONMENT_PROFILES_JSON")
        .ok()
        .filter(|value| !value.trim().is_empty())
    else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<EnvironmentProfile>>(&raw).unwrap_or_else(|error| {
        tracing::error!(%error, "environment profile registry is invalid");
        Vec::new()
    })
}

pub fn profile_response(profile: &EnvironmentProfile) -> EnvironmentProfileResponse {
    let mut blockers = Vec::new();
    if let Err(error) = profile.validate() {
        blockers.push(error.to_string());
    }
    if !profile.active {
        blockers.push("profile is inactive".to_string());
    }
    EnvironmentProfileResponse {
        id: profile.id.clone(),
        status: if blockers.is_empty() {
            "configured_unverified"
        } else {
            "unavailable"
        }
        .to_string(),
        image: profile.image.clone(),
        revision: profile.revision.clone(),
        platform: profile.platform.clone(),
        required_executables: profile.required_executables.clone(),
        preparation_strategy: serde_json::to_value(profile.preparation_strategy)
            .ok()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| "unknown".to_string()),
        service_account: profile.service_account.clone(),
        repository_allowlist: profile.repository_allowlist.clone(),
        blockers,
    }
}

pub fn select_profile<'a>(
    profiles: &'a [EnvironmentProfile],
    id: &str,
    repository: &str,
) -> Result<&'a EnvironmentProfile, String> {
    let profile = profiles
        .iter()
        .find(|profile| profile.id == id)
        .ok_or_else(|| format!("environment profile {id} does not exist"))?;
    profile.validate().map_err(|error| error.to_string())?;
    if !profile.active {
        return Err(format!("environment profile {id} is inactive"));
    }
    if !profile.repository_allowlist.is_empty()
        && !profile
            .repository_allowlist
            .iter()
            .any(|allowed| allowed.trim_end_matches('/') == repository.trim_end_matches('/'))
    {
        return Err(format!(
            "environment profile {id} is not allowed for repository {repository}"
        ));
    }
    Ok(profile)
}

pub async fn inspect_remote_project_contract(
    repository: &str,
    commit: &str,
) -> Result<(RepositoryContract, String), String> {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "pharness-project-preflight-{}-{suffix}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let result = inspect_remote_project_contract_inner(&root, repository, commit).await;
    let _ = std::fs::remove_dir_all(&root);
    result
}

async fn inspect_remote_project_contract_inner(
    root: &Path,
    repository: &str,
    commit: &str,
) -> Result<(RepositoryContract, String), String> {
    command(root, &["init", "--quiet"]).await?;
    command(root, &["remote", "add", "origin", repository]).await?;
    command(
        root,
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--quiet",
            "--depth=1",
            "--filter=blob:none",
            "origin",
            commit,
        ],
    )
    .await?;
    command(root, &["checkout", "--quiet", "--detach", "FETCH_HEAD"]).await?;
    let resolved = command(root, &["rev-parse", "HEAD"]).await?;
    if resolved.trim() != commit {
        return Err(format!(
            "repository resolved {} instead of requested immutable commit {commit}",
            resolved.trim()
        ));
    }
    RepositoryContract::load(root).map_err(|error| error.to_string())
}

async fn command(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(args)
        .output()
        .await
        .map_err(|error| format!("failed to invoke git: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
