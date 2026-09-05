//! Bounded evidence collection for the two native Finance environments.
//! A search sample is never a deployment identity or a release verdict.
use super::{
    ReadOnlyClusterTools, ToolError, ToolResult, DEFAULT_MAX_OUTPUT_BYTES, DEFAULT_TIMEOUT_MS,
};
use reqwest::{redirect::Policy, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TRACE_LIMIT: usize = 20;
const SPANS_PER_TRACE: usize = 3;
const MAX_WINDOW_SECONDS: u64 = 600;
const MAX_WINDOW_AGE_SECONDS: u64 = 60;
const SERVICE: &str = "yfinance-wrapper";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinanceTraceWindow {
    pub namespace: String,
    pub start_unix_seconds: u64,
    pub end_unix_seconds: u64,
}

impl FinanceTraceWindow {
    fn validate(&self, now: u64) -> Result<(), ToolError> {
        if !matches!(self.namespace.as_str(), "apps-staging" | "apps-prod")
            || self.start_unix_seconds == 0
            || self.end_unix_seconds <= self.start_unix_seconds
            || self.end_unix_seconds - self.start_unix_seconds > MAX_WINDOW_SECONDS
            || self.end_unix_seconds > now
            || self.end_unix_seconds.checked_mul(1_000_000_000).is_none()
        {
            return Err(ToolError::InvalidArguments {
                message: "Finance trace reads require apps-staging or apps-prod and an elapsed, nonempty window of at most 600 seconds".into(),
            });
        }
        Ok(())
    }

    fn query(&self) -> String {
        format!(
            "{{resource.service.name=\"{SERVICE}\" && resource.service.namespace=\"{}\"}}",
            self.namespace
        )
    }
}

impl ReadOnlyClusterTools {
    /// Native release evidence, deliberately absent from the agent tool schema.
    /// No retries, redirects, arbitrary TraceQL, trace contents, or frontend traces.
    pub async fn observe_finance_traces(
        &self,
        window: &FinanceTraceWindow,
    ) -> Result<ToolResult, ToolError> {
        let now = unix_seconds()?;
        window.validate(now)?;
        let query = window.query();
        let timeout_ms = self.timeout_ms.clamp(1, DEFAULT_TIMEOUT_MS);
        let byte_limit = self.max_output_bytes.clamp(1, DEFAULT_MAX_OUTPUT_BYTES);
        let mut evidence = json!({
            "schema_version": "pharness.dev/finance-trace-observation/v1alpha1",
            "source": "tempo", "service": SERVICE, "namespace": window.namespace,
            "query": query, "window": window, "observed_at_unix_seconds": now,
            "limits": {"traces": TRACE_LIMIT, "spans_per_trace": SPANS_PER_TRACE,
                "timeout_ms": timeout_ms, "response_bytes": byte_limit,
                "max_window_age_seconds": MAX_WINDOW_AGE_SECONDS},
            "sample_state": "inconclusive", "reasons": [],
            "release_verification": "not_evaluated",
            "deployment_correlation": "not_established",
            "search_is_exhaustive": false,
        });
        if now - window.end_unix_seconds > MAX_WINDOW_AGE_SECONDS {
            return Ok(inconclusive(evidence, "stale_query_window"));
        }
        let Some(base) = self.tempo_url.as_deref().filter(|v| !v.trim().is_empty()) else {
            return Ok(inconclusive(evidence, "tempo_not_configured"));
        };
        let Ok(mut url) = Url::parse(base) else {
            return Ok(inconclusive(evidence, "invalid_tempo_endpoint"));
        };
        // Configuration is operator-owned; credentials belong in neither URLs
        // nor diagnostics. A redirect must not broaden this read's destination.
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Ok(inconclusive(evidence, "invalid_tempo_endpoint"));
        }
        url.set_path(&format!("{}/api/search", url.path().trim_end_matches('/')));
        url.query_pairs_mut()
            .append_pair("q", &query)
            .append_pair("start", &window.start_unix_seconds.to_string())
            .append_pair("end", &window.end_unix_seconds.to_string())
            .append_pair("limit", &TRACE_LIMIT.to_string());
        let Ok(client) = reqwest::Client::builder()
            .redirect(Policy::none())
            .timeout(Duration::from_millis(timeout_ms))
            .build()
        else {
            return Ok(inconclusive(evidence, "tempo_client_unavailable"));
        };
        let result = tokio::time::timeout(Duration::from_millis(timeout_ms), async {
            let mut response = client
                .get(url)
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .await
                .map_err(|_| "tempo_request_failed")?;
            let status = response.status().as_u16();
            evidence["http_status"] = json!(status);
            if status != 200 {
                // Never include an error body, URL, headers or client error.
                return Err("tempo_http_error");
            }
            if response
                .content_length()
                .is_some_and(|n| n > byte_limit as u64)
            {
                return Err("tempo_response_too_large");
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| "tempo_body_failed")? {
                if chunk.len() > byte_limit.saturating_sub(bytes.len()) {
                    return Err("tempo_response_too_large");
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(bytes)
        })
        .await;
        let observed_at = unix_seconds()?;
        evidence["observed_at_unix_seconds"] = json!(observed_at);
        let bytes = match result {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(reason)) => return Ok(inconclusive(evidence, reason)),
            Err(_) => return Ok(inconclusive(evidence, "tempo_request_timed_out")),
        };
        evidence["response_bytes"] = json!(bytes.len());
        evidence["response_sha256"] = json!(format!("sha256:{:x}", Sha256::digest(&bytes)));
        let Ok(body) = serde_json::from_slice::<Value>(&bytes) else {
            return Ok(inconclusive(evidence, "malformed_tempo_response"));
        };
        evidence["sample"] = summarize(&body, window, observed_at);
        let summary = &evidence["sample"];
        let reason = if summary["malformed"] == true {
            Some("malformed_or_unrelated_trace_sample")
        } else if summary["traces"].as_array().map_or(true, Vec::is_empty) {
            Some("no_matching_traces")
        } else if summary["window_age_seconds"].as_u64().unwrap_or(u64::MAX)
            > MAX_WINDOW_AGE_SECONDS
        {
            Some("stale_query_window")
        } else {
            None
        };
        if let Some(reason) = reason {
            return Ok(inconclusive(evidence, reason));
        }
        evidence["sample_state"] = json!("sample_available");
        Ok(ToolResult::ok(
            "collected a bounded Finance trace sample; release verification remains separate",
            evidence,
        ))
    }
}

fn inconclusive(mut evidence: Value, reason: &str) -> ToolResult {
    evidence["reasons"] = json!([reason]);
    ToolResult::error("Finance trace observation is inconclusive", evidence)
}

fn unix_seconds() -> Result<u64, ToolError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs())
        .map_err(|_| ToolError::Io {
            message: "system time is before the Unix epoch".into(),
        })
}

fn number(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}

fn identifier(value: &Value, length: usize) -> Option<&str> {
    value.as_str().filter(|s| {
        s.len() == length
            && s.bytes().all(|c| c.is_ascii_hexdigit())
            && s.bytes().any(|c| c != b'0')
    })
}

fn trace_identifier(value: &Value) -> Option<String> {
    // Tempo search may omit leading zeroes. Normalize the numeric identity
    // before storing or comparing it; short IDs are not malformed traces.
    let raw = value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 32 && s.bytes().all(|c| c.is_ascii_hexdigit()))?;
    let id = u128::from_str_radix(raw, 16).ok().filter(|id| *id != 0)?;
    Some(format!("{id:032x}"))
}

fn resource_matches(span: &Value, namespace: &str) -> bool {
    let Some(attributes) = span["attributes"].as_array() else {
        return false;
    };
    [("service.name", SERVICE), ("service.namespace", namespace)]
        .iter()
        .all(|(key, expected)| {
            let values = attributes
                .iter()
                .filter(|v| v["key"] == *key)
                .collect::<Vec<_>>();
            values.len() == 1 && values[0]["value"]["stringValue"] == *expected
        })
}

fn compact_trace(trace: &Value, window: &FinanceTraceWindow) -> Option<Value> {
    let id = trace_identifier(&trace["traceID"])?;
    let trace_start = number(&trace["startTimeUnixNano"])?;
    let duration_ms = number(&trace["durationMs"])?;
    let legacy;
    let sets = if let Some(sets) = trace["spanSets"].as_array() {
        sets
    } else {
        legacy = vec![trace.get("spanSet")?.clone()];
        &legacy
    };
    let mut spans = Vec::new();
    let mut seen = BTreeSet::new();
    let mut newest = 0;
    for set in sets {
        for span in set["spans"].as_array()? {
            let span_id = identifier(&span["spanID"], 16)?;
            let start = number(&span["startTimeUnixNano"])?;
            let duration = number(&span["durationNanos"])?;
            if !resource_matches(span, &window.namespace)
                || start < window.start_unix_seconds * 1_000_000_000
                || start >= window.end_unix_seconds * 1_000_000_000
                || trace_start > start
                || start.checked_add(duration).is_none()
            {
                return None;
            }
            newest = newest.max(start);
            if seen.insert(span_id) && spans.len() < SPANS_PER_TRACE {
                spans.push(
                    json!({"span_id":span_id,"start_unix_nano":start,"duration_nanos":duration}),
                );
            }
        }
    }
    if spans.is_empty() {
        return None;
    }
    Some(
        json!({"trace_id": id, "start_unix_nano":trace_start, "duration_ms":duration_ms,
        "service": SERVICE, "namespace": window.namespace, "spans":spans,
        "returned_span_count":seen.len(), "spans_truncated":seen.len()>SPANS_PER_TRACE,
        "newest_matching_span_unix_nano": newest}),
    )
}

fn summarize(body: &Value, window: &FinanceTraceWindow, now: u64) -> Value {
    let Some(rows) = body["traces"].as_array() else {
        return json!({"malformed":true,"traces":[]});
    };
    let mut malformed = rows.len() > TRACE_LIMIT;
    let mut ids = BTreeSet::new();
    let traces = rows
        .iter()
        .take(TRACE_LIMIT)
        .filter_map(|row| match compact_trace(row, window) {
            Some(trace) if ids.insert(trace["trace_id"].as_str().unwrap().to_string()) => {
                Some(trace)
            }
            _ => {
                malformed = true;
                None
            }
        })
        .collect::<Vec<_>>();
    let completed_jobs = number(&body["metrics"]["completedJobs"]);
    let total_jobs = number(&body["metrics"]["totalJobs"]);
    let search_jobs_complete = match (completed_jobs, total_jobs) {
        (Some(done), Some(total)) if total > 0 && done <= total => Some(done == total),
        _ => None,
    };
    let newest = traces
        .iter()
        .filter_map(|v| v["newest_matching_span_unix_nano"].as_u64())
        .max();
    json!({"malformed":malformed,"traces":traces,"returned_trace_count":rows.len(),
        "limit_reached":rows.len()>=TRACE_LIMIT,"search_jobs_complete":search_jobs_complete,
        "search_metrics":{"completed_jobs":completed_jobs,"total_jobs":total_jobs,
            "inspected_bytes":number(&body["metrics"]["inspectedBytes"])},
        "window_age_seconds":now.saturating_sub(window.end_unix_seconds),
        "newest_matching_span_unix_nano":newest,
        "newest_match_age_seconds":newest.map(|n|now.saturating_sub(n/1_000_000_000)),
        "absence_of_errors_established":false})
}

#[cfg(test)]
mod tests;
