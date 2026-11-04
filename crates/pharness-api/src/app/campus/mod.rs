//! Campus: the student-facing boundary under `/api/campus/*`.
//!
//! Campus has its own state, middleware and error type and is merged into
//! the application after the operator routes, so none of the operator,
//! SDLC or worker middleware applies to it and none of its credentials apply
//! to them. Campus is disabled unless `PHARNESS_CAMPUS_ENABLED` is true.

mod authz;
mod config;
mod error;
mod handlers;
mod oidc;
mod session;
mod state;
mod tokens;

#[cfg(test)]
mod tests;

pub(crate) use config::{load_campus_config_from_env, validate_campus_startup};
pub(crate) use state::{CampusMount, CampusRuntime};

use axum::body::Body;
use axum::http::{header, HeaderValue, Request, StatusCode};
use axum::middleware::{from_fn_with_state, map_response};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::Router;
use error::CampusError;
use pharness_store::SqliteStore;
use state::CampusState;
use std::sync::Arc;
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;

pub(crate) fn router(store: Arc<SqliteStore>, mount: CampusMount) -> Router {
    let routes = match mount {
        CampusMount::Disabled => closed_router(StatusCode::NOT_FOUND, "campus is disabled"),
        CampusMount::Unavailable(reason) => closed_router(StatusCode::SERVICE_UNAVAILABLE, reason),
        CampusMount::Enabled(runtime) => enabled_router(CampusState {
            store,
            runtime: Arc::from(runtime),
        }),
    };
    // The callback URL carries the authorization code and state, so the span
    // records only the path, never the query.
    routes.layer(map_response(no_store)).layer(
        TraceLayer::new_for_http()
            .make_span_with(|request: &Request<Body>| {
                tracing::info_span!(
                    "campus_request",
                    method = %request.method(),
                    path = %request.uri().path(),
                )
            })
            .on_response(DefaultOnResponse::new().level(Level::INFO)),
    )
}

async fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn closed_router(status: StatusCode, message: &'static str) -> Router {
    let handler = move || async move { CampusError::new(status, message).into_response() };
    Router::new()
        .route("/api/campus", any(handler))
        .route("/api/campus/", any(handler))
        .route("/api/campus/*rest", any(handler))
}

fn enabled_router(state: CampusState) -> Router {
    let session_routes = Router::new()
        .route("/api/campus/me", get(handlers::me))
        .route("/api/campus/auth/logout", post(handlers::logout))
        .route("/api/campus/courses/:course_id", get(handlers::get_course))
        .route(
            "/api/campus/courses/:course_id/members",
            get(handlers::list_members)
                .put(handlers::put_member)
                .delete(handlers::delete_member),
        )
        .route_layer(from_fn_with_state(
            state.clone(),
            session::require_campus_session,
        ));
    Router::new()
        .route("/api/campus/auth/login", get(handlers::login))
        .route("/api/campus/auth/callback", get(handlers::callback))
        .merge(session_routes)
        .with_state(state)
}
