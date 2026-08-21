use super::capabilities::execute_capability;
use super::clock::{current_millis, unique_suffix};
use super::identifiers::safe_id_fragment;
use super::policy::policy_json;
use super::{environment, ApiError, AppState};
use crate::dispatch::CapabilityVerificationOutcome;
use crate::dto::{
    CapabilityStatusResponse, EnvironmentProfileResponse, EnvironmentProfilesResponse,
    SystemReadinessResponse,
};
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use pharness_store::CreateCapabilityVerification;
use serde_json::{json, Value};

use super::auth::OperatorIdentity;

pub(super) const PROTECTED_ENVIRONMENT: &str = "production";
pub(super) const PROTECTED_NAMESPACE: &str = "apps-prod";
pub(super) const PROTECTED_ARGO_APPLICATION: &str = "yfinance-wrapper";
pub(super) const PROTECTED_WORKLOAD_KIND: &str = "Deployment";
pub(super) const PROTECTED_WORKLOAD_NAME: &str = "yfinance-wrapper";
pub(super) const PROTECTED_SOURCE_REPO: &str = "https://github.com/lward27/yfinance_wrapper.git";
pub(super) const PROTECTED_GITOPS_REPO: &str = "https://github.com/lward27/lucas_engineering.git";
pub(super) const PROTECTED_KUSTOMIZATION_PATH: &str = "charts/yfinance-wrapper/kustomization.yaml";
pub(super) const PROTECTED_IMAGE_NAME: &str = "registry.lucas.engineering/yfinance_wrapper";
pub(super) const PROTECTED_ROLLBACK_OWNER: &str = "lucas";
pub(super) const PROTECTED_PIPELINE_NAMESPACE: &str = "tekton-pipelines";
pub(super) const PROTECTED_PIPELINE_REF: &str = "pharness-yfinance-build";

#[derive(Clone)]
pub(super) struct BuildMetadata {
    pub(super) api_revision: String,
    pub(super) ui_revision: String,
    pub(super) runtime_image_digest: String,
    pub(super) ui_image_digest: String,
}

#[derive(Clone)]
pub(super) struct ProtectedTargetConfiguration {
    pub(super) exact_locked_match: bool,
}

impl ProtectedTargetConfiguration {
    pub(super) fn from_env() -> Self {
        let expected = [
            (
                "PHARNESS_PROTECTED_TARGET_ENVIRONMENT",
                PROTECTED_ENVIRONMENT,
            ),
            ("PHARNESS_PROTECTED_TARGET_NAMESPACE", PROTECTED_NAMESPACE),
            (
                "PHARNESS_PROTECTED_TARGET_ARGO_APPLICATION",
                PROTECTED_ARGO_APPLICATION,
            ),
            (
                "PHARNESS_PROTECTED_TARGET_WORKLOAD_KIND",
                PROTECTED_WORKLOAD_KIND,
            ),
            (
                "PHARNESS_PROTECTED_TARGET_WORKLOAD_NAME",
                PROTECTED_WORKLOAD_NAME,
            ),
            (
                "PHARNESS_PROTECTED_TARGET_SOURCE_REPO",
                PROTECTED_SOURCE_REPO,
            ),
            (
                "PHARNESS_PROTECTED_TARGET_GITOPS_REPO",
                PROTECTED_GITOPS_REPO,
            ),
            (
                "PHARNESS_PROTECTED_TARGET_KUSTOMIZATION_PATH",
                PROTECTED_KUSTOMIZATION_PATH,
            ),
            ("PHARNESS_PROTECTED_TARGET_IMAGE_NAME", PROTECTED_IMAGE_NAME),
            (
                "PHARNESS_PROTECTED_TARGET_ROLLBACK_OWNER",
                PROTECTED_ROLLBACK_OWNER,
            ),
        ];
        let configured = expected
            .iter()
            .any(|(name, _)| std::env::var_os(name).is_some());
        let enabled = std::env::var("PHARNESS_PROTECTED_TARGET_ENABLED")
            .ok()
            .map(|value| value == "true")
            .unwrap_or(!configured);
        Self {
            exact_locked_match: enabled
                && (!configured
                    || expected
                        .iter()
                        .all(|(name, value)| std::env::var(name).ok().as_deref() == Some(*value))),
        }
    }
}

impl BuildMetadata {
    pub(super) fn from_env() -> Self {
        let value = |name: &str, compiled: Option<&str>| {
            std::env::var(name)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    compiled
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToOwned::to_owned)
                })
                .unwrap_or_else(|| "unknown".to_string())
        };
        Self {
            api_revision: value(
                "PHARNESS_BUILD_REVISION",
                option_env!("PHARNESS_BUILD_REVISION"),
            ),
            ui_revision: value("PHARNESS_UI_BUILD_REVISION", None),
            runtime_image_digest: value("PHARNESS_RUNTIME_IMAGE_DIGEST", None),
            ui_image_digest: value("PHARNESS_UI_IMAGE_DIGEST", None),
        }
    }
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/api/config/effective", get(config_effective))
        .route("/api/system/readiness", get(system_readiness))
        .route("/api/environment-profiles", get(list_environment_profiles))
        .route(
            "/api/system/capabilities/:capability/preflight",
            post(preflight_system_capability),
        )
        .route("/api/capabilities/execute", post(execute_capability))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true }))
}

async fn list_environment_profiles(
    State(state): State<AppState>,
) -> Result<Json<EnvironmentProfilesResponse>, ApiError> {
    Ok(Json(EnvironmentProfilesResponse {
        profiles: environment_profile_responses(&state).await?,
        provider_transport_attempts: pharness_fireworks::DEFAULT_MAX_TRANSPORT_ATTEMPTS,
    }))
}

async fn environment_profile_responses(
    state: &AppState,
) -> Result<Vec<EnvironmentProfileResponse>, ApiError> {
    let now = current_millis();
    let mut responses = Vec::new();
    for profile in state.environment_profiles.iter() {
        let mut response = environment::profile_response(profile);
        if response.status == "configured_unverified" {
            let capability = format!("environment_profile:{}", profile.id);
            if let Some(verification) = state
                .store
                .latest_capability_verification(&capability)
                .await?
            {
                response.status =
                    if verification.expires_at.parse::<u128>().unwrap_or_default() <= now {
                        response
                            .blockers
                            .push("isolated runner verification expired".to_string());
                        "stale".to_string()
                    } else {
                        if verification.status != "available" {
                            response.blockers.push(verification.summary);
                        }
                        verification.status
                    };
            }
        }
        responses.push(response);
    }
    Ok(responses)
}

pub(super) fn environment_profile_readiness_blocker(
    profile: &EnvironmentProfileResponse,
) -> Option<String> {
    if profile.status == "available" {
        return None;
    }
    let detail = if profile.blockers.is_empty() {
        match profile.status.as_str() {
            "configured_unverified" => {
                "runner profile requires a fresh passing isolated verification"
            }
            "stale" => "isolated runner verification expired",
            _ => "runner profile is unavailable",
        }
        .to_string()
    } else {
        profile.blockers.join("; ")
    };
    Some(format!("environment_profile {}: {detail}", profile.id))
}

pub(super) fn capability_verification_summary(outcome: &CapabilityVerificationOutcome) -> String {
    let permission = outcome.permission.as_deref().unwrap_or("required access");
    let repository = outcome
        .repository
        .as_deref()
        .map(|repo| format!(" for {repo}"))
        .unwrap_or_default();
    if outcome.available {
        format!("Isolated identity verified {permission}{repository}")
    } else {
        format!("Isolated identity did not verify {permission}{repository}")
    }
}

pub(super) async fn config_effective(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
) -> Json<serde_json::Value> {
    let worker = state.worker.config_json();
    let operator = json!({
        "auth_required": !state.operator_tokens.is_empty(),
        "name": identity.map(|Extension(OperatorIdentity(name))| name),
    });

    Json(json!({
        "api": {
            "name": "pharness-api",
        },
        "cluster": {
            "kubectl_bin": state.cluster_tools.kubectl_bin(),
            "argocd_namespace": state.cluster_tools.argocd_namespace(),
            "prometheus_configured": state.cluster_tools.prometheus_configured(),
            "loki_configured": state.cluster_tools.loki_configured(),
            "registry_alias_count": state.cluster_tools.registry_alias_count(),
        },
        "policy": policy_json(&state.policy),
        "worker": worker,
        "workspace": {
            "local_coding_enabled": state.worker.supports_local_workspace() && state.workspace.configured(),
            "allowed_repo_count": state.workspace.allowed_repo_count(),
        },
        "operator": operator,
    }))
}

pub(super) async fn capability_statuses(
    state: &AppState,
) -> Result<Vec<CapabilityStatusResponse>, ApiError> {
    let worker = state.worker.config_json();
    let configured = |pointer: &str| {
        worker
            .pointer(pointer)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    let status = |capability: &str, is_configured: bool, configured_summary: &str| {
        CapabilityStatusResponse {
            capability: capability.to_string(),
            status: if is_configured {
                "configured_unverified"
            } else {
                "unavailable"
            }
            .to_string(),
            summary: if is_configured {
                configured_summary.to_string()
            } else {
                format!("{capability} is not configured for the isolated execution identity")
            },
            verified_at: None,
            expires_at: None,
        }
    };
    let mut statuses = vec![
        status(
            "model_provider",
            worker.get("enabled").and_then(Value::as_bool).unwrap_or(false),
            "Model provider is configured but has not passed a fresh isolated credential check.",
        ),
        status(
            "source_workspace",
            state.workspace.remote_configured() || state.workspace.configured(),
            "At least one workspace repository is allowlisted.",
        ),
        status(
            "source_writer",
            configured("/git_writer/available"),
            "Source writer identity and allowlist are configured but repository reachability is unverified.",
        ),
        status(
            "source_observer",
            configured("/git_observer/available"),
            "Source observer identity and allowlist are configured but repository reachability is unverified.",
        ),
        status(
            "gitops_writer",
            configured("/gitops_writer/available"),
            "GitOps writer identity and allowlist are configured but repository reachability is unverified.",
        ),
        status(
            "gitops_observer",
            configured("/gitops_observer/available"),
            "GitOps observer identity and allowlist are configured but repository reachability is unverified.",
        ),
        status(
            "tekton",
            state.worker.supports_remote_workspace(),
            "Tekton executor is configured but cluster authorization is unverified.",
        ),
        status(
            "argo",
            configured("/argo_executor/available"),
            "Argo runner identity and exact Application allowlist are configured but authorization is unverified.",
        ),
        status(
            "observability",
            state.cluster_tools.prometheus_configured(),
            "Prometheus endpoint is configured but target inventory has not been freshly verified.",
        ),
    ];
    let now = current_millis();
    for status in &mut statuses {
        if status.status != "configured_unverified" {
            continue;
        }
        let Some(verification) = state
            .store
            .latest_capability_verification(&status.capability)
            .await?
        else {
            continue;
        };
        let expires = verification.expires_at.parse::<u128>().unwrap_or_default();
        status.verified_at = Some(verification.verified_at.clone());
        status.expires_at = Some(verification.expires_at.clone());
        if expires <= now {
            status.status = "stale".to_string();
            status.summary = format!(
                "{} verification expired; run the isolated preflight again",
                status.capability
            );
        } else {
            status.status = verification.status;
            status.summary = verification.summary;
        }
    }
    Ok(statuses)
}

pub(super) async fn system_readiness(
    State(state): State<AppState>,
) -> Result<Json<SystemReadinessResponse>, ApiError> {
    let worker = state.worker.config_json();
    let capabilities = capability_statuses(&state).await?;
    let mut blockers = capabilities
        .iter()
        .filter(|capability| capability.status != "available")
        .map(|capability| format!("{}: {}", capability.capability, capability.summary))
        .collect::<Vec<_>>();
    if !state.protected_target.exact_locked_match {
        blockers.push(
            "protected_target: deployed configuration does not exactly match the locked yfinance-wrapper production target"
                .to_string(),
        );
    }
    let environment_profiles = environment_profile_responses(&state).await?;
    if environment_profiles.is_empty() {
        blockers
            .push("environment_profiles: no immutable runner profile is configured".to_string());
    }
    blockers.extend(
        environment_profiles
            .iter()
            .filter_map(environment_profile_readiness_blocker),
    );
    Ok(Json(SystemReadinessResponse {
        api_revision: state.build.api_revision.clone(),
        ui_revision: state.build.ui_revision.clone(),
        runtime_image_digest: state.build.runtime_image_digest.clone(),
        ui_image_digest: state.build.ui_image_digest.clone(),
        platform_versions_match: state.build.api_revision != "unknown"
            && state.build.api_revision == state.build.ui_revision
            && immutable_image_digest(&state.build.runtime_image_digest)
            && immutable_image_digest(&state.build.ui_image_digest),
        capabilities,
        repository_allowlists: json!({
            "workspace": state.workspace.allowed_remote_repos(),
            "source_writer": worker.pointer("/git_writer/allowed_repos").cloned().unwrap_or_else(|| json!([])),
            "source_observer": worker.pointer("/git_observer/allowed_repos").cloned().unwrap_or_else(|| json!([])),
            "gitops_writer": worker.pointer("/gitops_writer/allowed_repos").cloned().unwrap_or_else(|| json!([])),
            "gitops_observer": worker.pointer("/gitops_observer/allowed_repos").cloned().unwrap_or_else(|| json!([])),
        }),
        targets: protected_target_json(),
        environment_profiles,
        blockers,
    }))
}

async fn preflight_system_capability(
    State(state): State<AppState>,
    Path(capability): Path<String>,
) -> Result<Json<CapabilityStatusResponse>, ApiError> {
    let profile = capability
        .strip_prefix("environment_profile:")
        .and_then(|id| {
            state
                .environment_profiles
                .iter()
                .find(|profile| profile.id == id)
        });
    let configured = if let Some(profile) = profile {
        let response = environment::profile_response(profile);
        CapabilityStatusResponse {
            capability: capability.clone(),
            status: response.status,
            summary: if response.blockers.is_empty() {
                "Runner profile is configured but has not passed isolated verification.".to_string()
            } else {
                response.blockers.join("; ")
            },
            verified_at: None,
            expires_at: None,
        }
    } else {
        capability_statuses(&state)
            .await?
            .into_iter()
            .find(|entry| entry.capability == capability)
            .ok_or_else(|| ApiError::not_found("capability", &capability))?
    };
    if capability_preflight_is_statically_unavailable(&configured) {
        return Ok(Json(configured));
    }
    let repository = (capability == "source_workspace")
        .then(|| state.workspace.allowed_remote_repos().first().cloned())
        .flatten();
    let outcome = match profile {
        Some(profile) => state.worker.verify_environment_profile(profile).await,
        None => {
            state
                .worker
                .verify_capability(&capability, repository.as_deref())
                .await
        }
    };
    let now = current_millis();
    let (status, summary, principal, verified_repository, permission) = match outcome {
        Ok(outcome) => (
            if outcome.available {
                "available"
            } else {
                "unavailable"
            },
            capability_verification_summary(&outcome),
            outcome.principal,
            outcome.repository,
            outcome.permission,
        ),
        Err(_) => (
            "unavailable",
            "Isolated capability verification could not complete".to_string(),
            None,
            repository,
            None,
        ),
    };
    let verification = state
        .store
        .create_capability_verification(CreateCapabilityVerification {
            id: format!(
                "capverify_{}_{}",
                safe_id_fragment(&capability),
                unique_suffix()
            ),
            capability: capability.clone(),
            status: status.to_string(),
            summary: summary.clone(),
            principal,
            repository: verified_repository,
            permission,
            verified_at: now.to_string(),
            expires_at: (now + 15 * 60 * 1_000).to_string(),
        })
        .await?;
    Ok(Json(CapabilityStatusResponse {
        capability,
        status: verification.status,
        summary: verification.summary,
        verified_at: Some(verification.verified_at),
        expires_at: Some(verification.expires_at),
    }))
}

pub(super) fn capability_preflight_is_statically_unavailable(
    status: &CapabilityStatusResponse,
) -> bool {
    status.status == "unavailable" && status.verified_at.is_none()
}

pub(super) fn protected_target_json() -> Value {
    json!({
        "environment": PROTECTED_ENVIRONMENT,
        "namespace": PROTECTED_NAMESPACE,
        "argo_application": PROTECTED_ARGO_APPLICATION,
        "workload_kind": PROTECTED_WORKLOAD_KIND,
        "workload_name": PROTECTED_WORKLOAD_NAME,
        "source_repo": PROTECTED_SOURCE_REPO,
        "gitops_repo": PROTECTED_GITOPS_REPO,
        "kustomization_path": PROTECTED_KUSTOMIZATION_PATH,
        "image_name": PROTECTED_IMAGE_NAME,
        "rollback_owner": PROTECTED_ROLLBACK_OWNER,
    })
}

pub(super) fn immutable_image_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

pub(super) fn immutable_git_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
