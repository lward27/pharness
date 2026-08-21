use super::*;
use axum::routing::{get, post};
use axum::Router;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/approval-gates", get(list_approval_gates))
        .route("/api/approval-gates/summary", get(approval_gate_summary))
        .route(
            "/api/approval-gates/batch-decide",
            post(batch_decide_approval_gates),
        )
        .route("/api/approval-gates/:gate_id", get(get_approval_gate))
        .route(
            "/api/approval-gates/:gate_id/satisfy",
            post(satisfy_approval_gate),
        )
        .route(
            "/api/approval-gates/:gate_id/waive",
            post(waive_approval_gate),
        )
        .route(
            "/api/approval-gates/:gate_id/reject",
            post(reject_approval_gate),
        )
        .route("/api/approvals", get(list_approvals))
        .route("/api/approvals/summary", get(approval_summary))
        .route("/api/approvals/:approval_id", get(get_approval))
        .route(
            "/api/approvals/:approval_id/approve",
            post(approve_approval),
        )
        .route("/api/approvals/:approval_id/deny", post(deny_approval))
        .route(
            "/api/permission-grants",
            get(list_permission_grants).post(create_permission_grant),
        )
        .route(
            "/api/permission-grants/:grant_id",
            get(get_permission_grant),
        )
        .route(
            "/api/permission-grants/:grant_id/revoke",
            post(revoke_permission_grant),
        )
}
