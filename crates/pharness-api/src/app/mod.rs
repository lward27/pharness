use crate::dispatch::RunDispatcher;
use crate::workspace::WorkspaceProvisioner;
use axum::http::StatusCode;
use axum::http::{Method, Request};
use axum::middleware;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use pharness_core::{ReadOnlyClusterTools, SafetyPolicy};
use pharness_store::{SqliteStore, StoreError};
use serde_json::json;
use std::sync::Arc;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

mod agent_hosts;
mod approval_policy;
mod approvals;
mod audit;
mod auth;
mod capabilities;
mod clock;
mod data_lifecycle;
mod delivery_actions;
mod delivery_segments;
mod deployment;
mod environment;
pub(crate) mod event_evidence;
mod evidence;
mod execution_checks;
mod gitops;
mod hashing;
mod identifiers;
mod inference;
mod internal;
mod json_values;
mod lifecycle_timeline;
mod operator;
mod operator_experience;
mod pipeline;
mod policy;
mod principals;
mod products;
mod releases;
mod repo_mode;
mod risk;
mod runs;
mod sdlc;
mod sessions;
mod source;
mod system;
mod text;
mod validation;
mod work_items;

use auth::require_operator_token;
use system::{BuildMetadata, ProtectedTargetConfiguration};

#[derive(Debug, Clone)]
struct RepoModeConfiguration {
    enabled: bool,
    ui_enabled: bool,
    design_overhaul_enabled: bool,
    coding_reliability_v2_enabled: bool,
    legacy_work_item_creation_enabled: bool,
    organization: pharness_store::BootstrapOrganization,
}

impl RepoModeConfiguration {
    fn from_env() -> Self {
        let enabled = std::env::var("PHARNESS_REPO_MODE_V1_ENABLED")
            .ok()
            .is_some_and(|value| {
                matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
            });
        let ui_enabled = std::env::var("PHARNESS_REPO_MODE_V1_UI_ENABLED")
            .ok()
            .is_some_and(|value| {
                matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
            });
        let legacy_work_item_creation_enabled =
            std::env::var("PHARNESS_LEGACY_WORK_ITEM_CREATION_ENABLED")
                .ok()
                .map(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
                .unwrap_or(false);
        let coding_reliability_v2_enabled = std::env::var("PHARNESS_CODING_RELIABILITY_V2_ENABLED")
            .ok()
            .is_some_and(|value| {
                matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
            });
        Self {
            enabled,
            ui_enabled,
            design_overhaul_enabled: std::env::var("PHARNESS_REPO_MODE_V1_DESIGN_OVERHAUL_ENABLED")
                .ok()
                .is_some_and(|value| {
                    matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
                }),
            coding_reliability_v2_enabled,
            legacy_work_item_creation_enabled,
            organization: pharness_store::BootstrapOrganization {
                id: std::env::var("PHARNESS_ORGANIZATION_ID")
                    .unwrap_or_else(|_| "org_default".into()),
                organization_key: std::env::var("PHARNESS_ORGANIZATION_KEY")
                    .unwrap_or_else(|_| "default".into()),
                display_name: std::env::var("PHARNESS_ORGANIZATION_NAME")
                    .unwrap_or_else(|_| "PHarness".into()),
            },
        }
    }

    #[cfg(test)]
    fn test_enabled() -> Self {
        Self {
            enabled: true,
            ui_enabled: true,
            design_overhaul_enabled: false,
            coding_reliability_v2_enabled: true,
            legacy_work_item_creation_enabled: true,
            organization: pharness_store::BootstrapOrganization {
                id: "org_test".into(),
                organization_key: "test".into(),
                display_name: "PHarness Test".into(),
            },
        }
    }
}

impl AppState {
    fn compiled_agent_profiles(&self, model: &str) -> Vec<pharness_core::AgentProfile> {
        if self.repo_mode.coding_reliability_v2_enabled {
            pharness_core::compiled_reliability_v2_agent_profiles(
                model,
                pharness_runhost::RELIABILITY_V2_PROMPT_BUNDLE_VERSION,
            )
        } else {
            pharness_core::compiled_agent_profiles(model, pharness_runhost::SYSTEM_PROMPT_VERSION)
        }
    }
}

const DEFAULT_DIRECT_CAPABILITY_TIMEOUT_MS: u64 = 60_000;
const MAX_DIRECT_CAPABILITY_TIMEOUT_MS: u64 = 300_000;
const CONTROLLER_WAIT_INTERVAL_MS: u128 = 15_000;
const CONTROLLER_WAIT_MAX_CHECKS: u32 = 240;

#[derive(Clone)]
pub struct AppState {
    store: Arc<SqliteStore>,
    worker: RunDispatcher,
    cluster_tools: ReadOnlyClusterTools,
    policy: SafetyPolicy,
    worker_token: Option<String>,
    operator_tokens: Arc<Vec<(String, String)>>,
    workspace: WorkspaceProvisioner,
    build: BuildMetadata,
    protected_target: ProtectedTargetConfiguration,
    environment_profiles: Arc<Vec<pharness_core::EnvironmentProfile>>,
    repo_mode: RepoModeConfiguration,
    inference: Arc<pharness_config::InferenceGatewayConfig>,
    agent_execution: Arc<pharness_config::AgentExecutionBackendConfig>,
}

#[cfg(test)]
pub fn router(
    store: Arc<SqliteStore>,
    worker: RunDispatcher,
    cluster_tools: ReadOnlyClusterTools,
    policy: SafetyPolicy,
    worker_token: Option<String>,
    operator_tokens: Vec<(String, String)>,
    workspace: WorkspaceProvisioner,
) -> Router {
    router_with_inference(
        store,
        worker,
        cluster_tools,
        policy,
        worker_token,
        operator_tokens,
        workspace,
        pharness_config::InferenceGatewayConfig::legacy_default(),
    )
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub fn router_with_inference(
    store: Arc<SqliteStore>,
    worker: RunDispatcher,
    cluster_tools: ReadOnlyClusterTools,
    policy: SafetyPolicy,
    worker_token: Option<String>,
    operator_tokens: Vec<(String, String)>,
    workspace: WorkspaceProvisioner,
    inference: pharness_config::InferenceGatewayConfig,
) -> Router {
    router_with_runtime_configs(
        store,
        worker,
        cluster_tools,
        policy,
        worker_token,
        operator_tokens,
        workspace,
        inference,
        pharness_config::AgentExecutionBackendConfig::disabled_default(),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn router_with_runtime_configs(
    store: Arc<SqliteStore>,
    worker: RunDispatcher,
    cluster_tools: ReadOnlyClusterTools,
    policy: SafetyPolicy,
    worker_token: Option<String>,
    operator_tokens: Vec<(String, String)>,
    workspace: WorkspaceProvisioner,
    inference: pharness_config::InferenceGatewayConfig,
    agent_execution: pharness_config::AgentExecutionBackendConfig,
) -> Router {
    let state = AppState {
        store,
        worker,
        cluster_tools,
        policy,
        worker_token,
        operator_tokens: Arc::new(operator_tokens),
        workspace,
        build: BuildMetadata::from_env(),
        protected_target: ProtectedTargetConfiguration::from_env(),
        environment_profiles: Arc::new(environment::load_environment_profiles()),
        repo_mode: RepoModeConfiguration::from_env(),
        inference: Arc::new(inference),
        agent_execution: Arc::new(agent_execution),
    };
    data_lifecycle::spawn_retention_scheduler(state.clone());
    agent_hosts::spawn_lease_monitor(state.clone());

    let operator_routes = Router::new()
        .merge(runs::router())
        .merge(system::router())
        .merge(inference::router())
        .merge(agent_hosts::router())
        .merge(data_lifecycle::router())
        .merge(evidence::router())
        .merge(work_items::router())
        .merge(operator::router())
        .merge(operator_experience::router())
        .merge(products::router())
        .merge(repo_mode::router())
        .merge(source::router())
        .merge(gitops::router())
        .merge(pipeline::router())
        .merge(deployment::router())
        .merge(releases::router())
        .merge(approvals::router())
        .route_layer(middleware::from_fn(enforce_operational_mode))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_operator_token,
        ));

    operator_routes
        .merge(internal::router(state.clone()))
        .merge(agent_hosts::internal_router(state.clone()))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperationalMode {
    Normal,
    Draining,
    ReadOnly,
}

impl OperationalMode {
    pub(crate) fn from_env() -> Self {
        match std::env::var("PHARNESS_OPERATIONAL_MODE")
            .unwrap_or_else(|_| "normal".into())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "draining" => Self::Draining,
            "read_only" | "readonly" => Self::ReadOnly,
            _ => Self::Normal,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Draining => "draining",
            Self::ReadOnly => "read_only",
        }
    }
}

async fn enforce_operational_mode(request: Request<axum::body::Body>, next: Next) -> Response {
    let path = request.uri().path();
    let mode = OperationalMode::from_env();
    match operational_mutation_decision(
        mode,
        request.method(),
        path,
        RepoModeConfiguration::from_env().legacy_work_item_creation_enabled,
    ) {
        OperationalMutationDecision::Allowed => next.run(request).await,
        OperationalMutationDecision::Blocked => (
            StatusCode::LOCKED,
            Json(json!({
                "error":"PHarness is not accepting this mutation in the current operational mode",
                "code":"operational_mode_blocks_mutation",
                "operational_mode":mode.as_str(),
            })),
        )
            .into_response(),
        OperationalMutationDecision::LegacyCreationDisabled => (
            StatusCode::CONFLICT,
            Json(json!({
                "error":"legacy WorkItem creation is disabled; create a Product-scoped Repo Mode WorkItem",
                "code":"legacy_work_item_creation_disabled",
                "route":"/api/products/:product_id/work-items",
            })),
        )
            .into_response(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperationalMutationDecision {
    Allowed,
    Blocked,
    LegacyCreationDisabled,
}

fn operational_mutation_decision(
    mode: OperationalMode,
    method: &Method,
    path: &str,
    legacy_work_item_creation_enabled: bool,
) -> OperationalMutationDecision {
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return OperationalMutationDecision::Allowed;
    }
    let allowed = match mode {
        OperationalMode::Normal => true,
        OperationalMode::Draining => {
            path.starts_with("/api/internal/") || path.ends_with("/cancel")
        }
        OperationalMode::ReadOnly => false,
    };
    if !allowed {
        return OperationalMutationDecision::Blocked;
    }
    if method == Method::POST && path == "/api/work-items" && !legacy_work_item_creation_enabled {
        return OperationalMutationDecision::LegacyCreationDisabled;
    }
    OperationalMutationDecision::Allowed
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found(entity: &str, id: &str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: format!("{entity} not found: {id}"),
        }
    }

    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.into(),
        }
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::NotFound { entity, id } => Self::not_found(&entity, &id),
            StoreError::Conflict(message) => Self::conflict(message),
            other => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: other.to_string(),
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": self.message,
            })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod operational_mode_tests {
    use super::*;

    #[test]
    fn read_only_serves_reads_and_blocks_every_mutation() {
        assert_eq!(
            operational_mutation_decision(
                OperationalMode::ReadOnly,
                &Method::GET,
                "/api/products",
                false
            ),
            OperationalMutationDecision::Allowed
        );
        assert_eq!(
            operational_mutation_decision(
                OperationalMode::ReadOnly,
                &Method::POST,
                "/api/internal/runs/run/outcome",
                false
            ),
            OperationalMutationDecision::Blocked
        );
    }

    #[test]
    fn draining_allows_callbacks_and_cancellation_but_not_new_work() {
        assert_eq!(
            operational_mutation_decision(
                OperationalMode::Draining,
                &Method::POST,
                "/api/internal/runs/run/outcome",
                false
            ),
            OperationalMutationDecision::Allowed
        );
        assert_eq!(
            operational_mutation_decision(
                OperationalMode::Draining,
                &Method::POST,
                "/api/runs/run/cancel",
                false
            ),
            OperationalMutationDecision::Allowed
        );
        assert_eq!(
            operational_mutation_decision(
                OperationalMode::Draining,
                &Method::POST,
                "/api/products",
                false
            ),
            OperationalMutationDecision::Blocked
        );
    }

    #[test]
    fn legacy_work_item_creation_is_an_explicit_normal_mode_exception() {
        assert_eq!(
            operational_mutation_decision(
                OperationalMode::Normal,
                &Method::POST,
                "/api/work-items",
                false
            ),
            OperationalMutationDecision::LegacyCreationDisabled
        );
        assert_eq!(
            operational_mutation_decision(
                OperationalMode::Normal,
                &Method::POST,
                "/api/products/prod/work-items",
                false
            ),
            OperationalMutationDecision::Allowed
        );
    }
}
