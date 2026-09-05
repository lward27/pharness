//! Correlate the native health request with Tempo; a trace is not a release verdict.
use super::{
    signals::WindowContext, timestamp, FinanceApplication, FinanceDeploymentExpectation,
    FinanceRuntimeWindow, ReadOnlyClusterTools, ToolError, ToolResult,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

mod analysis;
mod request;
#[cfg(test)]
mod tests;

struct HealthRequest {
    trace_id: String,
    parent_span_id: String,
    started_ms: u64,
    completed_ms: u64,
}

impl HealthRequest {
    fn from_probes(
        expected: &FinanceDeploymentExpectation,
        window: &FinanceRuntimeWindow,
        probes: &Value,
    ) -> Result<Self, &'static str> {
        if probes["schema_version"] != "pharness.dev/finance-functional-probes/v1alpha1"
            || probes["expected"] != json!(expected)
            || !matches!(
                probes["probe_state"].as_str(),
                Some("passed" | "failed" | "inconclusive")
            )
        {
            return Err("native_functional_probe_binding_missing");
        }
        let started = probes["started_at_unix_ms"]
            .as_u64()
            .ok_or("probe_time_missing")?;
        let completed = probes["completed_at_unix_ms"]
            .as_u64()
            .ok_or("probe_time_missing")?;
        if started < window.start_unix_seconds * 1000
            || completed > window.end_unix_seconds * 1000
            || completed < started
            || completed - started > 30_000
        {
            return Err("probes_outside_observed_window");
        }
        let rows = probes["probes"]
            .as_array()
            .filter(|r| r.len() == 3)
            .ok_or("native_probe_set_incomplete")?;
        let health = rows
            .iter()
            .filter(|p| p["path"] == "/healthz")
            .collect::<Vec<_>>();
        if health.len() != 1 {
            return Err("health_probe_missing_or_duplicated");
        }
        let health = health[0];
        if health["method"] != "GET" || health["state"] != "passed" || health["http_status"] != 200
        {
            return Err("health_probe_did_not_pass");
        }
        let observed = health["observed_at_unix_ms"]
            .as_u64()
            .ok_or("probe_time_missing")?;
        if observed < started || observed > completed {
            return Err("health_probe_time_invalid");
        }
        let correlation = format!(
            "{:x}",
            Sha256::digest(
                json!([expected, started, "/healthz"])
                    .to_string()
                    .as_bytes()
            )
        );
        if health["backend_trace_id"] != correlation[..32]
            || health["backend_parent_span_id"] != correlation[32..48]
        {
            return Err("health_probe_correlation_mismatch");
        }
        Ok(Self {
            trace_id: correlation[..32].into(),
            parent_span_id: correlation[32..48].into(),
            started_ms: started,
            completed_ms: observed,
        })
    }
}

impl ReadOnlyClusterTools {
    /// Exactly one native health trace, absent from the agent tool schema.
    /// Deployment/service identity and other functional results remain separate.
    pub async fn observe_finance_health_trace(
        &self,
        expected: &FinanceDeploymentExpectation,
        window: &FinanceRuntimeWindow,
        before: &Value,
        after: &Value,
        probes: &Value,
    ) -> Result<ToolResult, ToolError> {
        let now = timestamp()?;
        let mut evidence = json!({
            "schema_version":"pharness.dev/finance-health-trace/v1alpha1",
            "expected":expected,"window":window,"observed_at_unix_ms":now,
            "trace_state":"inconclusive","reasons":[],"runtime_verification":"not_evaluated",
            "functional_probe_state":probes["probe_state"],
            "limits":{"requests":1,"timeout_ms":self.timeout_ms.clamp(1,15_000),
                "response_bytes":self.max_output_bytes.clamp(1,512*1024),
                "maximum_window_age_seconds":60,"maximum_clock_skew_ms":1000,
                "resource_batches":8,"spans":64,"redirects":false,"retries":0},
            "scope":"specific native health request only; not exhaustive trace search or regression causality",
            "trace_image_identity_verified":false,
            "deployment_correlation":"matching native deployment and Service endpoint observations bracket the request window",
        });
        if let Err(reason) = WindowContext::validate(expected, window, before, after, now) {
            return Ok(inconclusive(evidence, reason));
        }
        if expected.application == FinanceApplication::Frontend {
            evidence["trace_state"] = json!("not_instrumented");
            evidence["reasons"] = json!(["frontend_binding_has_no_application_trace_requirement"]);
            evidence["limits"]["requests"] = json!(0);
            return Ok(ToolResult::ok(
                "Frontend application traces are not instrumented",
                evidence,
            ));
        }
        let health = match HealthRequest::from_probes(expected, window, probes) {
            Ok(health) => health,
            Err(reason) => return Ok(inconclusive(evidence, reason)),
        };
        for (name, value) in [
            ("identity_before_sha256", before),
            ("identity_after_sha256", after),
            ("functional_probes_sha256", probes),
        ] {
            evidence[name] = json!(crate::canonical_json_sha256(value).map_err(|_| {
                ToolError::InvalidArguments {
                    message: "Finance evidence cannot be hashed".into(),
                }
            })?);
        }
        evidence["trace_id"] = json!(health.trace_id);
        evidence["parent_span_id"] = json!(health.parent_span_id);
        let received = request::read(
            self.tempo_url.as_deref(),
            &health.trace_id,
            window,
            self.timeout_ms.clamp(1, 15_000),
            self.max_output_bytes.clamp(1, 512 * 1024),
        )
        .await;
        let (body, receipt) = match received {
            Ok(received) => received,
            Err(reason) => return Ok(inconclusive(evidence, reason)),
        };
        evidence["receipt"] = receipt;
        match analysis::summarize(&body, expected, window, &health) {
            Ok(summary) => evidence["trace"] = summary,
            Err(reason) => return Ok(inconclusive(evidence, reason)),
        }
        let finished = timestamp()?;
        evidence["completed_at_unix_ms"] = json!(finished);
        if finished / 1000 > window.end_unix_seconds + 60 {
            return Ok(inconclusive(evidence, "trace_window_stale_at_completion"));
        }
        evidence["trace_state"] = json!("observed");
        Ok(ToolResult::ok(
            "The native health request has a matching Tempo server span",
            evidence,
        ))
    }
}

fn inconclusive(mut evidence: Value, reason: &str) -> ToolResult {
    evidence["reasons"] = json!([reason]);
    ToolResult::ok("Finance health-trace correlation is inconclusive", evidence)
}
