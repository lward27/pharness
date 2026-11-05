//! The Campus broker: the trusted services a lab sandbox may call, served
//! on a listener of their own (`PHARNESS_CAMPUS_BROKER_BIND`).
//!
//! The sandbox holds exactly one credential, a Campus broker token
//! ([`pharness_core::campus_broker_token`]). This listener accepts nothing
//! else: no Campus session cookie, no worker or operator token. Every
//! request is checked against the lab journal, so a stopped, reset, expired
//! or failed session loses access on its next request. The listener runs in
//! the control-plane process, never next to student code, and serves no
//! operator, worker or Campus browser route; a sandbox network policy should
//! allow egress to this port only.
//!
//! Inference here is a fake: it answers deterministically and never calls a
//! model provider. Real model routing is a later change.

use super::error::CampusError;
use super::state::CampusState;
use anyhow::bail;
use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use pharness_core::campus_broker_token::{
    campus_broker_token_hash, is_well_formed_campus_broker_token, CampusBrokerAudience,
    CampusBrokerDenial, CampusBrokerGrant,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;

pub(crate) const ENV_BROKER_BIND: &str = "PHARNESS_CAMPUS_BROKER_BIND";
pub(crate) const FAKE_INFERENCE_MODEL: &str = "campus-fake";
const MAX_BODY_BYTES: usize = 64 * 1024;
const MAX_MESSAGES: usize = 64;

/// The broker listener address, if one is configured. It must differ in
/// port from every other listener so that a sandbox network policy can
/// admit the broker port alone.
pub(crate) fn campus_broker_bind(
    raw: Option<&str>,
    campus_enabled: bool,
    other_listeners: &[SocketAddr],
) -> anyhow::Result<Option<SocketAddr>> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if !campus_enabled {
        bail!("{ENV_BROKER_BIND} requires PHARNESS_CAMPUS_ENABLED=true");
    }
    let bind: SocketAddr = raw.parse().map_err(|_| {
        anyhow::anyhow!("{ENV_BROKER_BIND} must be an address such as 0.0.0.0:4790")
    })?;
    if other_listeners
        .iter()
        .any(|other| other.port() == bind.port())
    {
        bail!("{ENV_BROKER_BIND} must use a port of its own, not the API or metrics port");
    }
    Ok(Some(bind))
}

pub(crate) fn load_campus_broker_bind_from_env(
    campus_enabled: bool,
    other_listeners: &[SocketAddr],
) -> anyhow::Result<Option<SocketAddr>> {
    campus_broker_bind(
        std::env::var(ENV_BROKER_BIND).ok().as_deref(),
        campus_enabled,
        other_listeners,
    )
}

pub(super) fn closed_router(status: StatusCode, message: &'static str) -> Router {
    traced(
        Router::new()
            .fallback(move || async move { CampusError::new(status, message).into_response() }),
    )
}

pub(super) fn router(state: CampusState) -> Router {
    let inference = Router::new()
        .route("/v1/token", get(describe_token))
        .route("/v1/inference/chat/completions", post(fake_chat_completion))
        .route_layer(from_fn_with_state(state.clone(), require_inference_token));
    traced(
        inference
            .fallback(|| async {
                CampusError::not_found("no such campus broker route").into_response()
            })
            .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
            .with_state(state),
    )
}

fn traced(routes: Router) -> Router {
    routes.layer(
        TraceLayer::new_for_http()
            .make_span_with(|request: &axum::http::Request<Body>| {
                tracing::info_span!(
                    "campus_broker_request",
                    method = %request.method(),
                    path = %request.uri().path(),
                )
            })
            .on_response(DefaultOnResponse::new().level(Level::INFO)),
    )
}

fn denied(denial: CampusBrokerDenial) -> CampusError {
    CampusError::unauthorized("the campus broker token was refused").with_code(denial.as_str())
}

/// The single bearer value, which must be a well-formed broker token.
fn presented_token(headers: &HeaderMap) -> Option<&str> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let (Some(value), None) = (values.next(), values.next()) else {
        return None;
    };
    value
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .filter(|token| is_well_formed_campus_broker_token(token))
}

async fn authenticate(
    state: &CampusState,
    headers: &HeaderMap,
    audience: CampusBrokerAudience,
) -> Result<CampusBrokerGrant, CampusError> {
    let Some(token) = presented_token(headers) else {
        return Err(denied(CampusBrokerDenial::Malformed));
    };
    let now = state.runtime.clock.now();
    match state
        .store
        .authenticate_campus_broker_token(&campus_broker_token_hash(token), audience, now)
        .await
    {
        Ok(Ok(grant)) => Ok(grant),
        Ok(Err(denial)) => {
            tracing::info!(denial = denial.as_str(), %audience, "campus broker token refused");
            Err(denied(denial))
        }
        Err(error) => {
            tracing::error!(%error, "campus broker token check failed");
            Err(CampusError::unavailable("the campus broker is unavailable"))
        }
    }
}

async fn require_inference_token(
    State(state): State<CampusState>,
    mut request: Request,
    next: Next,
) -> Response {
    match authenticate(&state, request.headers(), CampusBrokerAudience::Inference).await {
        Ok(grant) => {
            request.extensions_mut().insert(grant);
            next.run(request).await
        }
        Err(error) => error.into_response(),
    }
}

/// What the presented token is bound to, so a sandbox can tell when its
/// token will stop working. Everything comes from the journal.
async fn describe_token(Extension(grant): Extension<CampusBrokerGrant>) -> Json<Value> {
    Json(json!({
        "token_id": grant.token_id,
        "audience": grant.audience,
        "session_id": grant.session_id(),
        "lab_generation": grant.lab_generation,
        "expires_at": grant.expires_at,
    }))
}

#[derive(Debug, Deserialize)]
struct ChatCompletionRequest {
    #[serde(default)]
    model: Option<String>,
    messages: Vec<ChatMessage>,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

fn approximate_tokens(text: &str) -> usize {
    text.split_whitespace().count()
}

async fn fake_chat_completion(
    State(state): State<CampusState>,
    Extension(grant): Extension<CampusBrokerGrant>,
    body: Result<Json<ChatCompletionRequest>, JsonRejection>,
) -> Result<Json<Value>, CampusError> {
    let Json(request) = body.map_err(|_| {
        CampusError::bad_request("the request body must be a chat completion request")
    })?;
    if request.messages.is_empty() || request.messages.len() > MAX_MESSAGES {
        return Err(CampusError::bad_request(
            "a chat completion needs between 1 and 64 messages",
        ));
    }
    if request
        .messages
        .iter()
        .any(|message| !matches!(message.role.as_str(), "system" | "user" | "assistant"))
    {
        return Err(CampusError::bad_request(
            "message roles must be system, user or assistant",
        ));
    }
    if request
        .model
        .as_deref()
        .is_some_and(|model| model != FAKE_INFERENCE_MODEL)
    {
        return Err(CampusError::bad_request(
            "only the campus-fake model is available",
        ));
    }
    let content = format!(
        "PHarness Campus fake inference: received {} message(s); no model was called.",
        request.messages.len()
    );
    let prompt_tokens: usize = request
        .messages
        .iter()
        .map(|message| approximate_tokens(&message.content))
        .sum();
    let completion_tokens = approximate_tokens(&content);
    tracing::info!(
        session_id = %grant.session_id(),
        token_id = %grant.token_id,
        prompt_tokens,
        "campus fake inference served"
    );
    Ok(Json(json!({
        "id": format!("cbchat_{}", uuid::Uuid::now_v7().simple()),
        "object": "chat.completion",
        "created": state.runtime.clock.now(),
        "model": FAKE_INFERENCE_MODEL,
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": content },
            "finish_reason": "stop",
        }],
        "usage": {
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": prompt_tokens + completion_tokens,
        },
        "campus_session_id": grant.session_id(),
    })))
}
