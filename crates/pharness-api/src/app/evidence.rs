use super::*;
use axum::routing::{get, post};
use axum::Router;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/artifacts/:artifact_id", get(get_artifact))
        .route(
            "/api/observations",
            get(list_observations).post(create_observation),
        )
        .route("/api/observations/:observation_id", get(get_observation))
        .route("/api/incidents", get(list_incidents).post(create_incident))
        .route("/api/incidents/:incident_id", get(get_incident))
        .route(
            "/api/remediation-plans",
            get(list_remediation_plans).post(create_remediation_plan),
        )
        .route("/api/remediation-plans/:plan_id", get(get_remediation_plan))
        .route(
            "/api/remediation-plans/:plan_id/transition",
            post(transition_remediation_plan),
        )
        .route("/api/audit-events", get(list_audit_events))
}
