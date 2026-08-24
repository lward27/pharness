use crate::dispatch::RunDispatcher;
use crate::workspace::WorkspaceProvisioner;
use axum::http::StatusCode;
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use pharness_core::{ReadOnlyClusterTools, SafetyPolicy};
use pharness_store::{SqliteStore, StoreError};
use serde_json::json;
use std::sync::Arc;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

mod approval_policy;
mod approvals;
mod audit;
mod auth;
mod capabilities;
mod clock;
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
mod internal;
mod json_values;
mod operator;
mod pipeline;
mod policy;
mod principals;
mod products;
mod releases;
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
    organization: pharness_store::BootstrapOrganization,
}

impl RepoModeConfiguration {
    fn from_env() -> Self {
        let enabled = std::env::var("PHARNESS_REPO_MODE_V1_ENABLED")
            .ok()
            .is_some_and(|value| {
                matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
            });
        Self {
            enabled,
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
            organization: pharness_store::BootstrapOrganization {
                id: "org_test".into(),
                organization_key: "test".into(),
                display_name: "PHarness Test".into(),
            },
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
}

pub fn router(
    store: Arc<SqliteStore>,
    worker: RunDispatcher,
    cluster_tools: ReadOnlyClusterTools,
    policy: SafetyPolicy,
    worker_token: Option<String>,
    operator_tokens: Vec<(String, String)>,
    workspace: WorkspaceProvisioner,
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
    };

    Router::new()
        .merge(runs::router())
        .merge(system::router())
        .merge(evidence::router())
        .merge(work_items::router())
        .merge(operator::router())
        .merge(products::router())
        .merge(source::router())
        .merge(gitops::router())
        .merge(pipeline::router())
        .merge(deployment::router())
        .merge(releases::router())
        .merge(approvals::router())
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_operator_token,
        ))
        .merge(internal::router(state.clone()))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
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
