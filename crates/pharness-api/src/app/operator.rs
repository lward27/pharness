use super::*;
use axum::routing::get;
use axum::Router;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/triage", get(list_triage))
        .route("/api/triage/summary", get(triage_summary))
        .route("/api/scopes/options", get(scope_options))
}
