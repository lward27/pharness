//! Fixed Mimir/Loki queries for an elapsed window bracketed by native identity reads.
use super::{timestamp, FinanceDeploymentExpectation, ReadOnlyClusterTools, ToolError, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

mod analysis;
#[cfg(test)]
mod live;
mod query;
mod request;
#[cfg(test)]
mod tests;
mod window;
pub(super) use window::Context as WindowContext;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinanceRuntimeWindow {
    pub start_unix_seconds: u64,
    pub end_unix_seconds: u64,
}

impl FinanceRuntimeWindow {
    /// Start on the next 30-second query grid boundary after the native identity
    /// observation. Loki aligns range queries to this grid; rounding an existing
    /// window would include unobserved time or shorten its required duration.
    pub fn starting_after(
        observed_at_unix_ms: u64,
        duration_seconds: u64,
    ) -> Result<Self, ToolError> {
        let invalid = || ToolError::InvalidArguments {
            message: "Finance requires a representable five- or ten-minute observation window"
                .into(),
        };
        if observed_at_unix_ms == 0 || !matches!(duration_seconds, 300 | 600) {
            return Err(invalid());
        }
        let start = (observed_at_unix_ms / 30_000 + 1)
            .checked_mul(30)
            .ok_or_else(invalid)?;
        let end = start.checked_add(duration_seconds).ok_or_else(invalid)?;
        end.checked_mul(1_000_000_000).ok_or_else(invalid)?;
        Ok(Self {
            start_unix_seconds: start,
            end_unix_seconds: end,
        })
    }
}

impl ReadOnlyClusterTools {
    /// No arbitrary queries or agent-facing tool. The caller retains both native
    /// deployment observations and obtains expected identities from durable delivery.
    pub async fn observe_finance_signals(
        &self,
        expected: &FinanceDeploymentExpectation,
        window: &FinanceRuntimeWindow,
        before: &Value,
        after: &Value,
    ) -> Result<ToolResult, ToolError> {
        let now = timestamp()?;
        let mut evidence = json!({
            "schema_version":"pharness.dev/finance-window-signals/v1alpha1",
            "expected":expected,"window":window,"observed_at_unix_ms":now,
            "signal_state":"inconclusive","reasons":[],"queries":[],
            "runtime_verification":"not_evaluated","regression_causality":"not_established",
            "limits":{"request_timeout_ms":self.timeout_ms.clamp(1,15_000),
                "response_bytes_per_query":self.max_output_bytes.clamp(1,256*1024),
                "sample_step_seconds":30,"maximum_sample_age_seconds":60,
                "redirects":false,"retries":0},
            "scope":"sampled readiness, restarts, request counters and known log-error patterns; functional and trace evidence remain separate",
        });
        let context = match window::Context::validate(expected, window, before, after, now) {
            Ok(context) => context,
            Err(reason) => {
                evidence["reasons"] = json!([reason]);
                return Ok(ToolResult::ok(
                    "Finance window identity is inconclusive",
                    evidence,
                ));
            }
        };
        let hash = |value| {
            crate::canonical_json_sha256(value).map_err(|_| ToolError::InvalidArguments {
                message: "Finance identity cannot be hashed".into(),
            })
        };
        evidence["identity_before_sha256"] = json!(hash(before)?);
        evidence["identity_after_sha256"] = json!(hash(after)?);
        let queries = query::build(expected, window, &context);
        evidence["limits"]["requests"] = json!(queries.len());
        let mut tasks = tokio::task::JoinSet::new();
        for query in queries {
            let base = if query.source == "mimir" {
                self.prometheus_url.clone()
            } else {
                self.loki_url.clone()
            };
            let timeout = self.timeout_ms.clamp(1, 15_000);
            let bytes = self.max_output_bytes.clamp(1, 256 * 1024);
            tasks.spawn(async move {
                let response = request::read(base.as_deref(), &query, timeout, bytes).await;
                (query, response)
            });
        }
        let mut outputs = Vec::new();
        let mut reasons = Vec::new();
        let mut failed = false;
        while let Some(result) = tasks.join_next().await {
            let Ok((query, response)) = result else {
                reasons.push("signal_collection_interrupted".to_string());
                continue;
            };
            let mut output = json!({"name":query.name,"source":query.source,"query":query.expression,
                "parameters":query.parameters,"state":"inconclusive"});
            match response {
                Ok((body, receipt)) => {
                    output["receipt"] = receipt;
                    match analysis::summarize(&query, &context, &body) {
                        Ok(summary) => {
                            failed |= summary["anomaly_observed"] == true;
                            output["state"] = json!("observed");
                            output["summary"] = summary;
                        }
                        Err(reason) => {
                            output["reason"] = json!(reason);
                            reasons.push(format!("{}:{reason}", query.name));
                        }
                    }
                }
                Err(reason) => {
                    output["reason"] = json!(reason);
                    reasons.push(format!("{}:{reason}", query.name));
                }
            }
            outputs.push(output);
        }
        outputs.sort_by_key(|v| v["name"].as_str().unwrap_or_default().to_string());
        reasons.sort();
        evidence["queries"] = json!(outputs);
        evidence["reasons"] = json!(reasons);
        evidence["signal_state"] = json!(if failed {
            "anomaly_observed"
        } else if reasons.is_empty() {
            "observed"
        } else {
            "inconclusive"
        });
        evidence["completed_at_unix_ms"] = json!(timestamp()?);
        // Collection consumes part of the freshness budget; a timely request is
        // not sufficient if its result is stale when it becomes available.
        if timestamp()? / 1000 > window.end_unix_seconds.saturating_add(60) {
            evidence["signal_state"] = json!("inconclusive");
            evidence["reasons"]
                .as_array_mut()
                .unwrap()
                .push(json!("window_stale_at_completion"));
        }
        Ok(ToolResult::ok(
            "Observed bounded Finance window signals",
            evidence,
        ))
    }
}
