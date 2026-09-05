use crate::app::ApiError;
use serde_json::{json, Value};

pub(super) fn validate_binding_scope(scope: &str) -> Result<(), ApiError> {
    if scope.is_empty()
        || scope.len() > 256
        || scope.starts_with(['/', '~'])
        || scope.contains(['\\', '\n', '\r', '\0'])
        || scope
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return Err(ApiError::bad_request(format!(
            "binding scope {scope:?} is not a normalized repository-relative glob"
        )));
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn onboarding_environment_profile_ids<'a>(
    profiles: impl IntoIterator<Item = (&'a str, bool)>,
) -> Vec<String> {
    let mut active = profiles
        .into_iter()
        .filter(|(_, is_active)| *is_active)
        .map(|(id, _)| id.to_string())
        .collect::<Vec<_>>();
    active.sort();
    active
}

pub(super) fn onboarding_environment_profile_descriptors(
    profiles: &[pharness_core::EnvironmentProfile],
    repository: &str,
    discovery: &Value,
) -> Vec<Value> {
    let dependency_candidates = discovery
        .get("dependency_candidates")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut compatible = profiles
        .iter()
        .filter(|profile| {
            profile.active
                && profile.validate().is_ok()
                && profile
                    .repository_allowlist
                    .iter()
                    .any(|allowed| allowed == repository)
                && dependency_candidates.iter().any(|candidate| {
                    candidate.get("kind").and_then(Value::as_str)
                        == Some(profile.preparation_strategy.accepted_dependency_lock_kind())
                })
        })
        .map(|profile| {
            json!({
                "id":profile.id,
                "runtime_kind":profile.preparation_strategy.runtime_kind(),
                "preparation_strategy":profile.preparation_strategy,
                "accepted_dependency_lock_kinds":[profile.preparation_strategy.accepted_dependency_lock_kind()],
                "repository_allowlist":profile.repository_allowlist,
                "lifecycle_scripts":if profile.preparation_strategy.lifecycle_scripts_allowed() {"allowed"} else {"denied"},
            })
        })
        .collect::<Vec<_>>();
    compatible.sort_by(|left, right| {
        left.get("id")
            .and_then(Value::as_str)
            .cmp(&right.get("id").and_then(Value::as_str))
    });
    compatible
}

pub(super) fn validate_onboarding_contract_compatibility(
    profiles: &[pharness_core::EnvironmentProfile],
    repository: &str,
    discovery: &Value,
    contract: &pharness_core::RepositoryContract,
) -> Result<(), ApiError> {
    let profile = profiles
        .iter()
        .find(|profile| profile.active && profile.id == contract.environment_profile)
        .ok_or_else(|| {
            ApiError::conflict("candidate contract selects an unavailable EnvironmentProfile")
        })?;
    contract
        .validate_for_profile(profile)
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    if !profile
        .repository_allowlist
        .iter()
        .any(|allowed| allowed == repository)
    {
        return Err(ApiError::conflict(
            "candidate EnvironmentProfile does not allow this exact repository",
        ));
    }
    let dependency_matches = discovery
        .get("dependency_candidates")
        .and_then(Value::as_array)
        .is_some_and(|candidates| {
            candidates.iter().any(|candidate| {
                candidate.get("path").and_then(Value::as_str)
                    == Some(contract.dependency_lock.path.as_str())
                    && candidate.get("kind").and_then(Value::as_str)
                        == Some(contract.dependency_lock.kind.as_str())
            })
        });
    if !dependency_matches {
        return Err(ApiError::conflict(
            "candidate dependency lock does not match deterministic discovery",
        ));
    }
    let files = discovery
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::conflict("deterministic discovery file inventory is missing"))?;
    for (label, roots) in [
        ("source", &contract.roots.source),
        ("test", &contract.roots.tests),
    ] {
        for root in roots {
            let prefix = format!("{}/", root.trim_end_matches('/'));
            if !files.iter().any(|file| {
                file.get("path")
                    .and_then(Value::as_str)
                    .is_some_and(|path| path == root || path.starts_with(&prefix))
            }) {
                return Err(ApiError::conflict(format!(
                    "candidate contract declares a {label} root absent from deterministic discovery: {root}"
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn onboarding_patch_paths(patch: &str) -> Result<Vec<String>, ApiError> {
    let allowed = [
        ".pharness/instructions.md",
        ".pharness/project.yaml",
        ".pharness/repository.yaml",
    ];
    let mut paths = Vec::new();
    for line in patch.lines() {
        let Some(header) = line.strip_prefix("diff --git a/") else {
            continue;
        };
        let (left, right) = header
            .split_once(" b/")
            .ok_or_else(|| ApiError::bad_request("onboarding patch has an invalid diff header"))?;
        if !allowed.contains(&left) || !allowed.contains(&right) {
            return Err(ApiError::conflict(
                "onboarding patch modifies a path outside the onboarding contract",
            ));
        }
        paths.push(left.to_string());
        if right != left {
            paths.push(right.to_string());
        }
    }
    paths.sort();
    paths.dedup();
    if paths.is_empty() || paths.len() > allowed.len() {
        return Err(ApiError::bad_request(
            "onboarding patch has no bounded contract changes",
        ));
    }
    Ok(paths)
}

pub(super) fn valid_prefixed_sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
