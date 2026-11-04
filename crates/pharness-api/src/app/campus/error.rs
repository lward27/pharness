use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use pharness_store::StoreError;
use serde_json::json;

/// Campus responses use their own error type so that no operator error path
/// (or its messages) leaks into the student-facing boundary.
#[derive(Debug)]
pub(crate) struct CampusError {
    pub(crate) status: StatusCode,
    message: &'static str,
}

impl CampusError {
    pub(crate) fn new(status: StatusCode, message: &'static str) -> Self {
        Self { status, message }
    }

    pub(crate) fn bad_request(message: &'static str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    pub(crate) fn unauthorized(message: &'static str) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, message)
    }

    pub(crate) fn forbidden(message: &'static str) -> Self {
        Self::new(StatusCode::FORBIDDEN, message)
    }

    pub(crate) fn not_found(message: &'static str) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    pub(crate) fn unavailable(message: &'static str) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, message)
    }

    pub(crate) fn bad_gateway(message: &'static str) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, message)
    }
}

impl From<StoreError> for CampusError {
    fn from(error: StoreError) -> Self {
        tracing::error!(%error, "campus store operation failed");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "campus storage is unavailable",
        )
    }
}

impl IntoResponse for CampusError {
    fn into_response(self) -> Response {
        let mut response = (self.status, Json(json!({ "error": self.message }))).into_response();
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
    }
}
