use super::{FinanceDeploymentExpectation, FinanceRuntimeWindow, HealthRequest};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

fn number(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str()?.parse().ok())
}

fn identifier(v: &Value, bytes: usize) -> Result<String, &'static str> {
    let s = v.as_str().ok_or("trace_identifier_missing")?;
    let hex = if s.len() == bytes * 2 && s.bytes().all(|c| c.is_ascii_hexdigit()) {
        s.to_ascii_lowercase()
    } else {
        let raw = STANDARD.decode(s).map_err(|_| "trace_identifier_invalid")?;
        if raw.len() != bytes {
            return Err("trace_identifier_invalid");
        }
        raw.iter().map(|b| format!("{b:02x}")).collect()
    };
    if hex.bytes().all(|c| c == b'0') {
        return Err("trace_identifier_invalid");
    }
    Ok(hex)
}

fn attributes(v: &Value) -> Result<BTreeMap<&str, &Value>, &'static str> {
    let rows = v
        .as_array()
        .filter(|r| r.len() <= 64)
        .ok_or("trace_attributes_missing_or_oversized")?;
    let mut attributes = BTreeMap::new();
    for row in rows {
        let key = row["key"]
            .as_str()
            .filter(|k| !k.is_empty() && k.len() <= 256)
            .ok_or("invalid_trace_attribute")?;
        if !row["value"].is_object() || attributes.insert(key, &row["value"]).is_some() {
            return Err("duplicate_or_invalid_trace_attribute");
        }
    }
    Ok(attributes)
}

fn text<'a>(attrs: &'a BTreeMap<&str, &Value>, key: &str) -> Option<&'a str> {
    attrs.get(key)?["stringValue"].as_str()
}

pub(super) fn summarize(
    body: &Value,
    expected: &FinanceDeploymentExpectation,
    window: &FinanceRuntimeWindow,
    health: &HealthRequest,
) -> Result<Value, &'static str> {
    // Tempo v2.9.0 TraceByIDResponse uses proto3 COMPLETE=0; JSON may omit
    // that default. Explicit PARTIAL, messages or error fields are not accepted.
    if !(body["status"].is_null() || body["status"] == 0 || body["status"] == "COMPLETE")
        || !(body["message"].is_null() || body["message"] == "")
        || !body["error"].is_null()
        || !body["warnings"].is_null()
        || !body["metrics"].is_object()
    {
        return Err("tempo_trace_response_partial_or_invalid");
    }
    let batches = body["trace"]["resourceSpans"]
        .as_array()
        .filter(|r| !r.is_empty() && r.len() <= 8)
        .ok_or("trace_missing_or_oversized")?;
    let mut server_spans = Vec::new();
    let mut ids = BTreeSet::new();
    for batch in batches {
        let resource = attributes(&batch["resource"]["attributes"])?;
        if text(&resource, "service.name") != Some("yfinance-wrapper")
            || text(&resource, "service.namespace") != Some(expected.namespace())
        {
            return Err("trace_service_or_namespace_mismatch");
        }
        let scopes = batch["scopeSpans"]
            .as_array()
            .filter(|s| !s.is_empty() && s.len() <= 16)
            .ok_or("trace_scopes_missing_or_oversized")?;
        for scope in scopes {
            let spans = scope["spans"]
                .as_array()
                .filter(|s| !s.is_empty() && s.len() <= 64)
                .ok_or("trace_spans_missing_or_oversized")?;
            for span in spans {
                if identifier(&span["traceId"], 16)? != health.trace_id {
                    return Err("unrelated_trace_identifier");
                }
                if !ids.insert(identifier(&span["spanId"], 8)?) || ids.len() > 64 {
                    return Err("trace_spans_duplicated_or_oversized");
                }
                if span["kind"] == "SPAN_KIND_SERVER" || span["kind"] == 2 {
                    server_spans.push(span);
                }
            }
        }
    }
    if server_spans.len() != 1 {
        return Err("health_server_span_missing_or_ambiguous");
    }
    let span = server_spans[0];
    if identifier(&span["parentSpanId"], 8)? != health.parent_span_id {
        return Err("health_request_parent_mismatch");
    }
    let attributes = attributes(&span["attributes"])?;
    if text(&attributes, "http.method") != Some("GET")
        || text(&attributes, "http.route") != Some("/healthz")
        || text(&attributes, "http.target") != Some("/healthz")
        || span["name"] != "GET /healthz"
    {
        return Err("health_request_route_mismatch");
    }
    if attributes
        .get("http.status_code")
        .and_then(|v| number(&v["intValue"]))
        != Some(200)
    {
        return Err("health_trace_status_missing_or_contradictory");
    }
    if !span["status"].is_null() && !span["status"].is_object() {
        return Err("health_span_status_invalid");
    }
    let code = &span["status"]["code"];
    if !(code.is_null()
        || code == 0
        || code == 1
        || code == "STATUS_CODE_UNSET"
        || code == "STATUS_CODE_OK")
    {
        return Err("health_trace_reports_error");
    }
    let start = number(&span["startTimeUnixNano"]).ok_or("trace_time_missing")?;
    let end = number(&span["endTimeUnixNano"]).ok_or("trace_time_missing")?;
    let skew = 1_000_000_000;
    if end < start
        || end - start > 32_000_000_000
        || start
            < window
                .start_unix_seconds
                .saturating_mul(1_000_000_000)
                .saturating_sub(skew)
        || end
            > window
                .end_unix_seconds
                .saturating_mul(1_000_000_000)
                .saturating_add(skew)
        || start
            < health
                .started_ms
                .saturating_mul(1_000_000)
                .saturating_sub(skew)
        || end
            > health
                .completed_ms
                .saturating_mul(1_000_000)
                .saturating_add(skew)
    {
        return Err("trace_outside_native_request_time");
    }
    Ok(
        json!({"trace_id":health.trace_id,"server_span_id":identifier(&span["spanId"],8)?,
        "parent_span_id":health.parent_span_id,"service":"yfinance-wrapper","namespace":expected.namespace(),
        "method":"GET","route":"/healthz","http_status":200,"start_unix_ns":start,"end_unix_ns":end,
        "duration_ms":(end-start) as f64/1_000_000.0,"spans_inspected":ids.len(),
        "tempo_partial_status":"complete","image_identity_source":"separate native deployment/Service observations",
        "raw_attributes_or_events_stored":false}),
    )
}
