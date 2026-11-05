use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use pharness_store::{CampusJournalError, StoreError};
use serde_json::json;

/// Campus responses use their own error type so that no operator error path
/// (or its messages) leaks into the student-facing boundary.
#[derive(Debug)]
pub(crate) struct CampusError {
    pub(crate) status: StatusCode,
    message: &'static str,
    code: Option<&'static str>,
    /// A session the caller may act on instead, such as its own live one.
    session_id: Option<String>,
}

impl CampusError {
    pub(crate) fn new(status: StatusCode, message: &'static str) -> Self {
        Self {
            status,
            message,
            code: None,
            session_id: None,
        }
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

    pub(crate) fn conflict(message: &'static str) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }

    pub(crate) fn unavailable(message: &'static str) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, message)
    }

    pub(crate) fn bad_gateway(message: &'static str) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, message)
    }

    /// A stable, machine-readable reason alongside the message.
    pub(crate) fn with_code(mut self, code: &'static str) -> Self {
        self.code = Some(code);
        self
    }

    pub(crate) fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
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

/// Journal errors carry session, slot and operation internals, so they are
/// logged and never returned.
impl From<CampusJournalError> for CampusError {
    fn from(error: CampusJournalError) -> Self {
        match error {
            CampusJournalError::NotFound(_) => {
                Self::not_found("campus lab session not found").with_code("session_not_found")
            }
            CampusJournalError::Fenced(_)
            | CampusJournalError::LeaseHeld { .. }
            | CampusJournalError::Conflict(_) => {
                tracing::warn!(%error, "campus lab request raced another change");
                Self::conflict("the lab session changed; retry the request")
                    .with_code("session_changed")
            }
            other => {
                tracing::error!(error = %other, "campus lab journal operation failed");
                Self::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "campus storage is unavailable",
                )
            }
        }
    }
}

impl IntoResponse for CampusError {
    fn into_response(self) -> Response {
        let mut body = json!({ "error": self.message });
        if let Some(code) = self.code {
            body["code"] = json!(code);
        }
        if let Some(session_id) = self.session_id {
            body["session_id"] = json!(session_id);
        }
        let mut response = (self.status, Json(body)).into_response();
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
    }
}
