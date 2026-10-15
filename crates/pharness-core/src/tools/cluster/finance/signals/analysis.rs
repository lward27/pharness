use super::{query::Query, window::Context};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

fn pair(value: &Value, time: u64) -> Result<f64, &'static str> {
    let pair = value
        .as_array()
        .filter(|v| v.len() == 2)
        .ok_or("malformed_sample")?;
    let timestamp = pair[0].as_f64().ok_or("malformed_sample_time")?;
    if !timestamp.is_finite() || (timestamp - time as f64).abs() > 0.001 {
        return Err("unexpected_sample_time");
    }
    let number = pair[1]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|n| n.is_finite() && *n >= 0.0)
        .ok_or("nonfinite_or_invalid_sample")?;
    Ok(number)
}

fn samples(row: &Value, query: &Query) -> Result<Vec<f64>, &'static str> {
    let count = ((query.last - query.first) / 30 + 1) as usize;
    let values = row["values"]
        .as_array()
        .filter(|v| v.len() == count)
        .ok_or("missing_window_samples")?;
    values
        .iter()
        .enumerate()
        .map(|(i, v)| pair(v, query.first + i as u64 * 30))
        .collect()
}

pub(super) fn summarize(
    query: &Query,
    context: &Context,
    body: &Value,
) -> Result<Value, &'static str> {
    if body["status"] != "success"
        || !(body["warnings"].is_null() || body["warnings"].as_array().is_some_and(Vec::is_empty))
        || !body["error"].is_null()
        || !body["errorType"].is_null()
        || body["data"]["resultType"] != if query.range { "matrix" } else { "vector" }
    {
        return Err("provider_result_incomplete_or_invalid");
    }
    let rows = body["data"]["result"]
        .as_array()
        .filter(|v| v.len() <= 128)
        .ok_or("result_missing_or_oversized")?;
    match query.name {
        "pod_readiness" | "pod_restarts" | "pod_metric_freshness" => {
            pod_matrix(query, context, rows)
        }
        "log_presence" => log_presence(query, context, rows),
        "log_errors" => log_errors(query, context, rows),
        "http_responses" => http_responses(query, rows),
        "http_metric_freshness" => {
            if rows.len() != 1 || !rows[0]["metric"].as_object().is_some_and(|m| m.is_empty()) {
                return Err("request_metric_freshness_missing");
            }
            let age = pair(&rows[0]["value"], query.last)?;
            if age > 60.0 {
                return Err("request_metrics_stale");
            }
            Ok(json!({"maximum_sample_age_seconds":age,"anomaly_observed":false}))
        }
        _ => Err("unknown_native_signal_query"),
    }
}

fn pod_matrix(query: &Query, context: &Context, rows: &[Value]) -> Result<Value, &'static str> {
    if rows.len() != context.pods.len() {
        return Err("pod_metric_coverage_incomplete");
    }
    let mut seen = BTreeSet::new();
    let mut output = Vec::new();
    let mut anomaly = false;
    for row in rows {
        let metric = &row["metric"];
        let pod = metric["pod"]
            .as_str()
            .and_then(|n| context.pods.get(n))
            .ok_or("unrelated_metric_pod")?;
        if metric["uid"] != pod.uid
            || metric["namespace"] != context.namespace
            || metric["container"] != context.workload
            || !seen.insert(pod.name.as_str())
        {
            return Err("metric_pod_uid_or_scope_mismatch");
        }
        let values = samples(row, query)?;
        let minimum = values.iter().copied().fold(f64::INFINITY, f64::min);
        let maximum = values.iter().copied().fold(0.0, f64::max);
        match query.name {
            "pod_readiness" => {
                if values.iter().any(|v| *v != 0.0 && *v != 1.0) {
                    return Err("invalid_readiness_value");
                }
                anomaly |= minimum != 1.0;
            }
            "pod_restarts" => {
                if values.iter().any(|v| v.fract() != 0.0) {
                    return Err("invalid_restart_value");
                }
                anomaly |= values.iter().any(|v| *v != pod.restarts as f64);
            }
            "pod_metric_freshness" if maximum > 60.0 => return Err("pod_metrics_stale"),
            _ => {}
        }
        output.push(json!({"pod":pod.name,"uid":pod.uid,"minimum":minimum,"maximum":maximum,"samples":values}));
    }
    Ok(json!({"pods":output,"anomaly_observed":anomaly}))
}

fn log_presence(query: &Query, context: &Context, rows: &[Value]) -> Result<Value, &'static str> {
    let mut seen = BTreeSet::new();
    let mut totals: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for row in rows {
        let pod = context.log_pod(&row["metric"])?;
        if !seen.insert(row["metric"]["filename"].as_str().unwrap()) {
            return Err("duplicate_log_stream");
        }
        let values = samples(row, query)?;
        let total = totals
            .entry(pod.name.clone())
            .or_insert_with(|| vec![0.0; values.len()]);
        for (sum, value) in total.iter_mut().zip(values) {
            if value.fract() != 0.0 {
                return Err("invalid_log_count");
            }
            *sum += value;
            if !sum.is_finite() {
                return Err("invalid_log_count");
            }
        }
    }
    if totals.len() != context.pods.len() || totals.values().any(|v| v.iter().any(|n| *n <= 0.0)) {
        return Err("log_presence_missing_or_stale");
    }
    Ok(
        json!({"counts_in_preceding_60_seconds":totals,"anomaly_observed":false,"raw_log_lines_stored":0}),
    )
}

fn log_errors(query: &Query, context: &Context, rows: &[Value]) -> Result<Value, &'static str> {
    let mut seen = BTreeSet::new();
    let mut total = 0.0;
    for row in rows {
        context.log_pod(&row["metric"])?;
        if !seen.insert(row["metric"]["filename"].as_str().unwrap()) {
            return Err("duplicate_log_stream");
        }
        let count = pair(&row["value"], query.last)?;
        if count.fract() != 0.0 {
            return Err("invalid_log_count");
        }
        total += count;
        if !total.is_finite() {
            return Err("invalid_log_count");
        }
    }
    // An empty error vector is usable only with the separately required,
    // successful log-presence query for every pod and sampled interval.
    Ok(
        json!({"known_error_pattern_matches":total,"anomaly_observed":total>0.0,
        "requires_log_presence":"every_pod_and_interval","raw_log_lines_stored":0,
        "classification_scope":"known application error and HTTP 5xx patterns; not exhaustive error detection"}),
    )
}

fn http_responses(query: &Query, rows: &[Value]) -> Result<Value, &'static str> {
    let mut counts = BTreeMap::new();
    let mut total = 0.0;
    let mut errors = 0.0;
    let mut unclassified = None;
    for row in rows {
        let count = pair(&row["value"], query.last)?;
        let metric = row["metric"]
            .as_object()
            .filter(|m| m.is_empty() || (m.len() == 1 && m.contains_key("http_status_code")))
            .ok_or("invalid_http_status_labels")?;
        if metric.is_empty() {
            if unclassified.replace(count).is_some() || count > 0.0 {
                return Err("unclassified_http_responses");
            }
            continue;
        }
        let status = row["metric"]["http_status_code"]
            .as_str()
            .filter(|s| s.len() == 3)
            .and_then(|s| s.parse::<u16>().ok())
            .filter(|n| (100..600).contains(n))
            .ok_or("invalid_http_status_label")?;
        if counts.insert(status, count).is_some() {
            return Err("duplicate_http_status_series");
        }
        total += count;
        if status >= 500 {
            errors += count;
        }
    }
    if !total.is_finite() || total <= 0.0 {
        return Err("request_counter_data_missing");
    }
    Ok(
        json!({"estimated_responses_by_status":counts,"estimated_5xx":errors,
        "counter_increase_is_extrapolated":true,"anomaly_observed":errors>0.0,
        "unclassified_responses":unclassified,"identity_scope":"application namespace and service; request metrics have no Pod UID label",
        "latency_slo":"not_defined"}),
    )
}
