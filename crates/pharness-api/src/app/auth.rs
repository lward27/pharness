use super::{ApiError, AppState};
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};

/// Gate `/api/internal/*` behind the configured worker token.
///
/// Worker ingest is disabled entirely when no token is configured, so a
/// loopback-only local deployment exposes no unauthenticated write surface
/// for remote workers.
pub(super) async fn require_worker_token(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let Some(expected) = state.worker_token.as_deref() else {
        return ApiError::conflict("worker ingest is disabled: no worker token is configured")
            .into_response();
    };

    let provided = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    match provided {
        Some(token) if token_matches(token, expected) => next.run(request).await,
        _ => ApiError::unauthorized("invalid or missing worker token").into_response(),
    }
}

fn token_matches(provided: &str, expected: &str) -> bool {
    let provided = Sha256::digest(provided.as_bytes());
    let expected = Sha256::digest(expected.as_bytes());
    provided == expected
}

/// Authenticated operator identity resolved from the bearer token.
#[derive(Debug, Clone)]
pub(super) struct OperatorIdentity(pub(super) String);

/// Gate operator routes behind `PHARNESS_OPERATOR_TOKENS` when configured.
///
/// `/health` stays open for probes. With no operator tokens configured the
/// API keeps its loopback-trusting local behavior.
pub(super) async fn require_operator_token(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    if state.operator_tokens.is_empty() || request.uri().path() == "/health" {
        return next.run(request).await;
    }

    let provided = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    let matched = provided.and_then(|token| {
        state
            .operator_tokens
            .iter()
            .find(|(_, expected)| token_matches(token, expected))
            .map(|(name, _)| name.clone())
    });

    match matched {
        Some(name) => {
            request.extensions_mut().insert(OperatorIdentity(name));
            next.run(request).await
        }
        None => ApiError::unauthorized("invalid or missing operator token").into_response(),
    }
}
