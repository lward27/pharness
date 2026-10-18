use super::validation::clean_optional_text;
use super::{ApiError, AppState};
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Extension;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

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
    bool::from(provided.as_slice().ct_eq(expected.as_slice()))
}

/// Authenticated operator identity resolved from the bearer token.
#[derive(Debug, Clone)]
pub(super) struct OperatorIdentity(pub(super) String);

/// Actor attribution for an operator-initiated decision.
///
/// When operator tokens are configured the authenticated token name is the
/// only trusted actor; a client-supplied name is retained purely so the audit
/// trail can show that the request body disagreed with the credential. With
/// no tokens configured the API keeps trusting the supplied name as before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DecisionActor {
    pub(super) name: Option<String>,
    pub(super) claimed: Option<String>,
}

impl DecisionActor {
    pub(super) fn resolve(
        identity: Option<&Extension<OperatorIdentity>>,
        supplied: Option<String>,
    ) -> Self {
        let supplied = clean_optional_text(supplied);
        match identity {
            Some(Extension(OperatorIdentity(name))) => Self {
                name: Some(name.clone()),
                claimed: supplied.filter(|claimed| claimed != name),
            },
            None => Self {
                name: supplied,
                claimed: None,
            },
        }
    }

    pub(super) fn require(self, message: &str) -> Result<(String, Option<String>), ApiError> {
        match self.name {
            Some(name) => Ok((name, self.claimed)),
            None => Err(ApiError::bad_request(message)),
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::{token_matches, DecisionActor, OperatorIdentity};
    use axum::http::StatusCode;
    use axum::Extension;

    #[test]
    fn token_matching_accepts_only_the_exact_token() {
        assert!(token_matches("operator-token", "operator-token"));
        assert!(!token_matches("operator-token", "other-token"));
    }

    #[test]
    fn decision_actor_prefers_the_authenticated_identity() {
        let identity = Extension(OperatorIdentity("alice".to_string()));

        let matching = DecisionActor::resolve(Some(&identity), Some(" alice ".to_string()));
        assert_eq!(matching.name.as_deref(), Some("alice"));
        assert_eq!(matching.claimed, None);

        let spoofed = DecisionActor::resolve(Some(&identity), Some("bob".to_string()));
        assert_eq!(spoofed.name.as_deref(), Some("alice"));
        assert_eq!(spoofed.claimed.as_deref(), Some("bob"));

        let omitted = DecisionActor::resolve(Some(&identity), None);
        assert_eq!(omitted.name.as_deref(), Some("alice"));
        assert_eq!(omitted.claimed, None);
    }

    #[test]
    fn decision_actor_falls_back_to_the_supplied_name_without_tokens() {
        let supplied = DecisionActor::resolve(None, Some(" bob ".to_string()));
        assert_eq!(supplied.name.as_deref(), Some("bob"));
        assert_eq!(supplied.claimed, None);

        let blank = DecisionActor::resolve(None, Some("   ".to_string()));
        assert_eq!(blank.name, None);
        let error = blank.require("actor is required").unwrap_err();
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.message, "actor is required");
    }
}
