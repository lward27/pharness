use super::super::{signals, traces, FinanceApplication, FinanceEnvironment};
use super::FinanceRuntimeEvidence;
use serde_json::{json, Value};
use std::collections::BTreeSet;

fn digest(v: &Value) -> bool {
    v.as_str()
        .and_then(|s| s.strip_prefix("sha256:"))
        .is_some_and(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
}

fn receipt(v: &Value, maximum: u64) -> bool {
    v["http_status"] == 200
        && digest(&v["sha256"])
        && v["bytes"].as_u64().is_some_and(|n| n > 0 && n <= maximum)
}

fn hash_matches(v: &Value, key: &str, material: &Value) -> bool {
    crate::canonical_json_sha256(material).is_ok_and(|hash| v[key] == hash)
}

fn collection_time(v: &Value, first: u64, now: u64) -> bool {
    let start = v["observed_at_unix_ms"].as_u64();
    let end = v["completed_at_unix_ms"].as_u64().or(start);
    start.zip(end).is_some_and(|(start, end)| {
        start >= first && end >= start && end <= now && end - start <= 30_000
    })
}

pub(super) fn validate(e: &FinanceRuntimeEvidence, now: u64) -> Result<(), &'static str> {
    for (v, schema) in [
        (
            &e.functional_probes,
            "pharness.dev/finance-functional-probes/v1alpha1",
        ),
        (&e.signals, "pharness.dev/finance-window-signals/v1alpha1"),
        (
            &e.health_trace,
            "pharness.dev/finance-health-trace/v1alpha1",
        ),
    ] {
        if v["schema_version"] != schema
            || v["expected"] != json!(e.expected)
            || v["runtime_verification"] != "not_evaluated"
        {
            return Err("native_collection_schema_or_identity_mismatch");
        }
    }
    let start = e.window.start_unix_seconds * 1000;
    let end = e.window.end_unix_seconds * 1000;
    for v in [&e.signals, &e.health_trace] {
        if v["window"] != json!(e.window) || !collection_time(v, end, now) {
            return Err("native_collection_window_or_time_mismatch");
        }
    }
    let probes = &e.functional_probes;
    if !probes["started_at_unix_ms"]
        .as_u64()
        .zip(probes["completed_at_unix_ms"].as_u64())
        .is_some_and(|(began, finished)| {
            began >= start && finished <= end && finished >= began && finished - began <= 30_000
        })
    {
        return Err("functional_probes_outside_native_window");
    }
    let expected_paths = match e.expected.application {
        FinanceApplication::Yfinance => vec![
            "/healthz",
            "/history?ticker_name=%2A%2A%2A",
            "/markets/not-a-market",
        ],
        FinanceApplication::Frontend if e.expected.environment == FinanceEnvironment::Staging => {
            vec!["/", "/runtime-config.json", "/api/yfinance/healthz"]
        }
        FinanceApplication::Frontend => vec!["/", "/runtime-config.json"],
    };
    let rows = probes["probes"]
        .as_array()
        .ok_or("functional_probe_set_missing")?;
    let mut found = BTreeSet::new();
    if rows.len() != expected_paths.len() {
        return Err("functional_probe_set_incomplete");
    }
    for row in rows {
        let path = row["path"]
            .as_str()
            .ok_or("functional_probe_path_missing")?;
        if !expected_paths.contains(&path)
            || !found.insert(path)
            || row["method"] != "GET"
            || !row["observed_at_unix_ms"].as_u64().is_some_and(|time| {
                time >= probes["started_at_unix_ms"].as_u64().unwrap()
                    && time <= probes["completed_at_unix_ms"].as_u64().unwrap()
            })
        {
            return Err("functional_probe_identity_mismatch");
        }
        if row["state"] == "passed" {
            let status =
                if path == "/history?ticker_name=%2A%2A%2A" || path == "/markets/not-a-market" {
                    422
                } else {
                    200
                };
            if row["http_status"] != status
                || !row["reason"].is_null()
                || !digest(&row["response_sha256"])
                || !row["response_bytes"]
                    .as_u64()
                    .is_some_and(|n| n > 0 && n <= 65_536)
            {
                return Err("functional_probe_pass_contradicts_receipt");
            }
        }
    }
    let signals = &e.signals;
    if signals["completed_at_unix_ms"].as_u64().is_none()
        || !hash_matches(signals, "identity_before_sha256", &e.identity_before)
        || !hash_matches(signals, "identity_after_sha256", &e.identity_after)
    {
        return Err("signal_identity_fingerprint_mismatch");
    }
    let bindings = signals::query_bindings(
        &e.expected,
        &e.window,
        &e.identity_before,
        &e.identity_after,
        now,
    )?;
    let queries = signals["queries"]
        .as_array()
        .ok_or("signal_queries_missing")?;
    if queries.len() != bindings.len() {
        return Err("signal_query_set_incomplete");
    }
    let mut found = BTreeSet::new();
    for q in queries {
        let name = q["name"].as_str().ok_or("signal_query_name_missing")?;
        let binding = bindings
            .iter()
            .find(|b| b["name"] == name)
            .ok_or("unrelated_signal_query")?;
        if !found.insert(name)
            || ["source", "query", "parameters"]
                .iter()
                .any(|key| q[*key] != binding[*key])
        {
            return Err("signal_query_binding_mismatch");
        }
        if q["state"] == "observed"
            && (!receipt(&q["receipt"], 256 * 1024) || !q["reason"].is_null())
        {
            return Err("signal_query_pass_contradicts_receipt");
        }
    }
    let trace = &e.health_trace;
    if trace["functional_probe_state"] != probes["probe_state"] {
        return Err("trace_functional_result_mismatch");
    }
    if trace["trace_state"] == "observed"
        && (e.expected.application != FinanceApplication::Yfinance
            || trace["completed_at_unix_ms"].as_u64().is_none()
            || !hash_matches(trace, "identity_before_sha256", &e.identity_before)
            || !hash_matches(trace, "identity_after_sha256", &e.identity_after)
            || !hash_matches(trace, "functional_probes_sha256", probes)
            || !receipt(&trace["receipt"], 512 * 1024))
    {
        return Err("trace_identity_or_probe_fingerprint_mismatch");
    }
    Ok(())
}

pub(super) fn trace_passes(e: &FinanceRuntimeEvidence) -> bool {
    traces::collected_trace_passes(
        &e.expected,
        &e.window,
        &e.functional_probes,
        &e.health_trace,
    )
}
