//! Prometheus exposition for the control plane.
//!
//! The endpoint is served on its own listener (`PHARNESS_METRICS_BIND`) so a
//! scrape never needs an operator credential and the public ingress, which
//! forwards the whole operator port, never exposes it.

use super::campus::CAMPUS_JANITOR;
use super::{AppState, OperationalMode};
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use pharness_store::{CampusLabSlotCondition, CampusTenantLabMetrics};
use std::fmt::Write;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};

/// Process-local counters maintained by the hosted scheduler loop.
pub(super) struct ControllerTelemetry {
    enabled: AtomicBool,
    last_tick_millis: AtomicI64,
    ticks: AtomicU64,
    reconciliations: AtomicU64,
    errors: AtomicU64,
}

pub(super) static CONTROLLER: ControllerTelemetry = ControllerTelemetry {
    enabled: AtomicBool::new(false),
    last_tick_millis: AtomicI64::new(0),
    ticks: AtomicU64::new(0),
    reconciliations: AtomicU64::new(0),
    errors: AtomicU64::new(0),
};

impl ControllerTelemetry {
    pub(super) fn mark_enabled(&self) {
        self.enabled.store(true, Ordering::Relaxed);
    }

    pub(super) fn record_tick(&self, now_millis: i64, outcome: Result<bool, ()>) {
        self.last_tick_millis.store(now_millis, Ordering::Relaxed);
        self.ticks.fetch_add(1, Ordering::Relaxed);
        match outcome {
            Ok(true) => {
                self.reconciliations.fetch_add(1, Ordering::Relaxed);
            }
            Ok(false) => {}
            Err(()) => {
                self.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

pub(super) fn router(state: AppState) -> Router {
    Router::new()
        .route("/metrics", get(metrics))
        .with_state(state)
}

async fn metrics(State(state): State<AppState>) -> Response {
    match render(&state).await {
        Ok(body) => (
            StatusCode::OK,
            [(
                header::CONTENT_TYPE,
                "text/plain; version=0.0.4; charset=utf-8",
            )],
            body,
        )
            .into_response(),
        Err(error) => {
            tracing::warn!(%error, "metrics collection failed");
            (StatusCode::SERVICE_UNAVAILABLE, error).into_response()
        }
    }
}

async fn render(state: &AppState) -> Result<String, String> {
    let now = super::clock::current_millis() as i64;
    let controller = state
        .store
        .hosted_controller_metrics(now)
        .await
        .map_err(|error| error.to_string())?;
    let database = state
        .store
        .database_size()
        .await
        .map_err(|error| error.to_string())?;
    let mut out = String::with_capacity(4096);

    gauge(
        &mut out,
        "pharness_build_info",
        "Build identity of the running API; always 1.",
        &[(
            &[
                ("revision", state.build.api_revision.as_str()),
                ("image_digest", state.build.runtime_image_digest.as_str()),
            ],
            1.0,
        )],
    );
    let mode = match OperationalMode::from_env() {
        OperationalMode::Normal => "normal",
        OperationalMode::Draining => "draining",
        OperationalMode::ReadOnly => "read_only",
    };
    gauge(
        &mut out,
        "pharness_operational_mode",
        "Current PHARNESS_OPERATIONAL_MODE; the active mode is 1.",
        &[(&[("mode", mode)], 1.0)],
    );

    let enabled = CONTROLLER.enabled.load(Ordering::Relaxed);
    gauge(
        &mut out,
        "pharness_controller_enabled",
        "1 when this process runs the hosted controller loop.",
        &[(&[], if enabled { 1.0 } else { 0.0 })],
    );
    let last_tick = CONTROLLER.last_tick_millis.load(Ordering::Relaxed);
    gauge(
        &mut out,
        "pharness_controller_last_tick_timestamp_seconds",
        "Unix time of the most recent hosted controller tick; 0 before the first tick.",
        &[(&[], last_tick as f64 / 1000.0)],
    );
    gauge(
        &mut out,
        "pharness_controller_last_tick_age_seconds",
        "Seconds since the most recent hosted controller tick.",
        &[(
            &[],
            if last_tick > 0 {
                (now - last_tick).max(0) as f64 / 1000.0
            } else {
                f64::NAN
            },
        )],
    );
    counter(
        &mut out,
        "pharness_controller_ticks_total",
        "Hosted controller loop iterations since process start.",
        CONTROLLER.ticks.load(Ordering::Relaxed),
    );
    counter(
        &mut out,
        "pharness_controller_reconciliations_total",
        "Hosted reconciliations that claimed a WorkItem since process start.",
        CONTROLLER.reconciliations.load(Ordering::Relaxed),
    );
    counter(
        &mut out,
        "pharness_controller_reconcile_errors_total",
        "Hosted reconciliation passes that ended in an error since process start.",
        CONTROLLER.errors.load(Ordering::Relaxed),
    );

    let conditions = controller
        .reconciliations_by_condition
        .iter()
        .map(|(condition, count)| (vec![("condition", condition.as_str())], *count as f64))
        .collect::<Vec<_>>();
    gauge_owned(
        &mut out,
        "pharness_hosted_reconciliations",
        "Open hosted WorkItems by controller condition.",
        &conditions,
    );
    gauge(
        &mut out,
        "pharness_hosted_reconciliations_due_unclaimed",
        "Open hosted WorkItems whose reconciliation is due and not claimed.",
        &[(&[], controller.due_unclaimed as f64)],
    );
    gauge(
        &mut out,
        "pharness_hosted_reconciliations_oldest_due_age_seconds",
        "Age of the oldest due, unclaimed reconciliation; NaN when none is due.",
        &[(
            &[],
            controller
                .oldest_due_age_millis
                .map(|millis| millis as f64 / 1000.0)
                .unwrap_or(f64::NAN),
        )],
    );
    gauge(
        &mut out,
        "pharness_hosted_reconciliations_claimed",
        "Hosted reconciliations currently held under a live claim.",
        &[(&[], controller.claimed as f64)],
    );
    let statuses = controller
        .operations_by_status
        .iter()
        .map(|(status, count)| (vec![("status", status.as_str())], *count as f64))
        .collect::<Vec<_>>();
    gauge_owned(
        &mut out,
        "pharness_hosted_operations_active",
        "Hosted operations that have not succeeded, by status.",
        &statuses,
    );
    gauge(
        &mut out,
        "pharness_hosted_operations_oldest_active_age_seconds",
        "Age of the oldest unfinished hosted operation; NaN when none is active.",
        &[(
            &[],
            controller
                .oldest_active_operation_age_millis
                .map(|millis| millis as f64 / 1000.0)
                .unwrap_or(f64::NAN),
        )],
    );

    gauge(
        &mut out,
        "pharness_sqlite_database_bytes",
        "SQLite database size (page_size * page_count).",
        &[(&[], database.total_bytes as f64)],
    );
    gauge(
        &mut out,
        "pharness_sqlite_freelist_bytes",
        "SQLite bytes held in free pages that a VACUUM would release.",
        &[(&[], database.freelist_bytes as f64)],
    );

    let campus = state
        .store
        .campus_lab_metrics(now / 1000)
        .await
        .map_err(|error| error.to_string())?;
    render_campus(&mut out, &campus, now / 1000);

    Ok(out)
}

/// Campus lab gauges from the journal, so every replica reports the same
/// values, plus this process's most recent janitor pass.
fn render_campus(out: &mut String, tenants: &[CampusTenantLabMetrics], now: i64) {
    let mut sessions = Vec::new();
    let mut slots = Vec::new();
    let mut oldest = Vec::new();
    let mut expired = Vec::new();
    for tenant in tenants {
        let id = tenant.tenant_id.as_str();
        for (state, count) in &tenant.sessions {
            sessions.push((
                vec![("tenant", id), ("state", state.as_str())],
                *count as f64,
            ));
        }
        for condition in CampusLabSlotCondition::ALL {
            let count = tenant.slots.get(&condition).copied().unwrap_or(0);
            slots.push((
                vec![("tenant", id), ("condition", condition.as_str())],
                count as f64,
            ));
        }
        oldest.push((
            vec![("tenant", id)],
            tenant
                .oldest_quarantined_at
                .map(|since| (now - since).max(0) as f64)
                .unwrap_or(f64::NAN),
        ));
        expired.push((vec![("tenant", id)], tenant.expired_live as f64));
    }
    gauge_owned(
        out,
        "pharness_campus_lab_sessions",
        "Campus lab sessions that are not terminal, by journal state.",
        &sessions,
    );
    gauge_owned(
        out,
        "pharness_campus_lab_slots",
        "Campus lab slots by condition; cleanup_blocked and quarantined slots take no new session.",
        &slots,
    );
    gauge_owned(
        out,
        "pharness_campus_lab_oldest_quarantine_age_seconds",
        "Age of the oldest blocked cleanup or quarantined slot; NaN when there is none.",
        &oldest,
    );
    gauge_owned(
        out,
        "pharness_campus_lab_sessions_expired_live",
        "Campus lab sessions past their absolute expiry that are neither terminal nor quarantined.",
        &expired,
    );
    let last_pass = CAMPUS_JANITOR.last_pass.load(Ordering::Relaxed);
    gauge(
        out,
        "pharness_campus_lab_janitor_last_pass_timestamp_seconds",
        "Unix time of this process's most recent Campus lab janitor pass; 0 before the first.",
        &[(&[], last_pass as f64)],
    );
    gauge(
        out,
        "pharness_campus_lab_janitor_orphaned_workloads",
        "Provider workloads no live session accounted for at the most recent janitor pass.",
        &[(
            &[],
            CAMPUS_JANITOR.orphaned_workloads.load(Ordering::Relaxed) as f64,
        )],
    );
    counter(
        out,
        "pharness_campus_lab_janitor_errors_total",
        "Campus lab janitor inventory, delete or journal errors since process start.",
        CAMPUS_JANITOR.errors.load(Ordering::Relaxed),
    );
}

fn gauge(out: &mut String, name: &str, help: &str, samples: &[(&[(&str, &str)], f64)]) {
    header_lines(out, name, help, "gauge");
    for (labels, value) in samples {
        sample(out, name, labels, *value);
    }
}

fn gauge_owned(out: &mut String, name: &str, help: &str, samples: &[(Vec<(&str, &str)>, f64)]) {
    header_lines(out, name, help, "gauge");
    for (labels, value) in samples {
        sample(out, name, labels, *value);
    }
}

fn counter(out: &mut String, name: &str, help: &str, value: u64) {
    header_lines(out, name, help, "counter");
    sample(out, name, &[], value as f64);
}

fn header_lines(out: &mut String, name: &str, help: &str, kind: &str) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} {kind}");
}

fn sample(out: &mut String, name: &str, labels: &[(&str, &str)], value: f64) {
    out.push_str(name);
    if !labels.is_empty() {
        out.push('{');
        for (index, (key, raw)) in labels.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let _ = write!(out, "{key}=\"{}\"", escape_label(raw));
        }
        out.push('}');
    }
    let _ = writeln!(out, " {}", format_value(value));
}

fn escape_label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn format_value(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value > 0.0 { "+Inf" } else { "-Inf" }.to_string()
    } else if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::{escape_label, format_value, gauge, render_campus, sample};
    use pharness_core::CampusLabSessionState;
    use pharness_store::{CampusLabSlotCondition, CampusTenantLabMetrics};

    #[test]
    fn exposition_lines_follow_the_text_format() {
        let mut out = String::new();
        gauge(
            &mut out,
            "pharness_example",
            "An example.",
            &[(&[("kind", "a\"b")], 2.0), (&[], 0.5)],
        );
        assert_eq!(
            out,
            "# HELP pharness_example An example.\n# TYPE pharness_example gauge\npharness_example{kind=\"a\\\"b\"} 2\npharness_example 0.5\n"
        );
        let mut nan = String::new();
        sample(&mut nan, "x", &[], f64::NAN);
        assert_eq!(nan, "x NaN\n");
        assert_eq!(format_value(1234567.0), "1234567");
        assert_eq!(format_value(f64::INFINITY), "+Inf");
        assert_eq!(escape_label("line\nbreak"), "line\\nbreak");
    }

    #[test]
    fn campus_gauges_report_every_slot_condition_and_the_quarantine_age() {
        let tenant = CampusTenantLabMetrics {
            tenant_id: "tenant-a".into(),
            sessions: [(CampusLabSessionState::CleanupBlocked, 1)].into(),
            slots: [
                (CampusLabSlotCondition::CleanupBlocked, 1),
                (CampusLabSlotCondition::Free, 4),
            ]
            .into(),
            oldest_quarantined_at: Some(900),
            expired_live: 2,
        };
        let mut out = String::new();
        render_campus(&mut out, &[tenant], 1_000);
        for line in [
            "pharness_campus_lab_sessions{tenant=\"tenant-a\",state=\"cleanup_blocked\"} 1",
            "pharness_campus_lab_slots{tenant=\"tenant-a\",condition=\"free\"} 4",
            "pharness_campus_lab_slots{tenant=\"tenant-a\",condition=\"cleanup_blocked\"} 1",
            "pharness_campus_lab_slots{tenant=\"tenant-a\",condition=\"quarantined\"} 0",
            "pharness_campus_lab_slots{tenant=\"tenant-a\",condition=\"retired\"} 0",
            "pharness_campus_lab_oldest_quarantine_age_seconds{tenant=\"tenant-a\"} 100",
            "pharness_campus_lab_sessions_expired_live{tenant=\"tenant-a\"} 2",
            "# TYPE pharness_campus_lab_janitor_errors_total counter",
        ] {
            assert!(
                out.lines().any(|candidate| candidate == line),
                "{line} in\n{out}"
            );
        }
        let mut none = String::new();
        render_campus(&mut none, &[], 1_000);
        assert!(none.contains("# TYPE pharness_campus_lab_slots gauge"));
        assert!(!none.contains("pharness_campus_lab_slots{"));
    }
}
