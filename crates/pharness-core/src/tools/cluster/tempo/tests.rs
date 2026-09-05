use super::{
    summarize, trace_identifier, unix_seconds, FinanceTraceWindow, ReadOnlyClusterTools,
    TRACE_LIMIT,
};
use crate::tools::ToolResultStatus;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn window() -> FinanceTraceWindow {
    let now = unix_seconds().unwrap();
    FinanceTraceWindow {
        namespace: "apps-staging".into(),
        start_unix_seconds: now - 300,
        end_unix_seconds: now,
    }
}

fn sample(window: &FinanceTraceWindow) -> Value {
    let start = (window.end_unix_seconds - 5) * 1_000_000_000;
    json!({"traces":[{"traceID":"1234567890abcdef1234567890abcdef",
        "rootTraceName":"secret-body-canary", "startTimeUnixNano":start.to_string(),"durationMs":2,
        "spanSets":[{"spans":[{"spanID":"1234567890abcdef","startTimeUnixNano":start.to_string(),
            "durationNanos":"2000000","attributes":[
                {"key":"service.name","value":{"stringValue":"yfinance-wrapper"}},
                {"key":"service.namespace","value":{"stringValue":window.namespace}},
                {"key":"http.request.header.authorization","value":{"stringValue":"secret-body-canary"}}
            ]}]}]}],"metrics":{"completedJobs":1,"totalJobs":3,"inspectedBytes":"4012"}})
}

async fn server(response: String, delay: Duration) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut bytes = [0; 1024];
        while !request.windows(4).any(|v| v == b"\r\n\r\n") {
            let n = socket.read(&mut bytes).await.unwrap();
            if n == 0 {
                break;
            }
            request.extend_from_slice(&bytes[..n]);
            assert!(request.len() < 8192);
        }
        if delay.is_zero() {
            let _ = socket.write_all(response.as_bytes()).await;
        } else {
            // Send headers immediately so the test exercises the body deadline.
            let (headers, body) = response.split_once("\r\n\r\n").unwrap();
            let _ = socket
                .write_all(format!("{headers}\r\n\r\n").as_bytes())
                .await;
            tokio::time::sleep(delay).await;
            let _ = socket.write_all(body.as_bytes()).await;
        }
        String::from_utf8(request).unwrap()
    });
    (format!("http://{address}"), task)
}

fn response(body: &str) -> String {
    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
}

#[tokio::test]
async fn reads_only_the_fixed_finance_query_and_retains_sampling_limits() {
    let window = window();
    let body = sample(&window).to_string();
    let (url, task) = server(response(&body), Duration::ZERO).await;
    let output = ReadOnlyClusterTools::default()
        .with_tempo_url_option(Some(url))
        .observe_finance_traces(&window)
        .await
        .unwrap();
    assert_eq!(output.status, ToolResultStatus::Ok);
    assert_eq!(output.content["sample_state"], "sample_available");
    assert_eq!(output.content["release_verification"], "not_evaluated");
    assert_eq!(output.content["deployment_correlation"], "not_established");
    assert_eq!(output.content["search_is_exhaustive"], false);
    assert_eq!(output.content["sample"]["search_jobs_complete"], false);
    assert_eq!(
        output.content["sample"]["absence_of_errors_established"],
        false
    );
    assert_eq!(output.content["response_bytes"], body.len());
    assert!(output.content["response_sha256"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert!(!serde_json::to_string(&output)
        .unwrap()
        .contains("secret-body-canary"));
    let request = task.await.unwrap();
    assert!(request.starts_with("GET /api/search?"));
    assert!(!request.to_ascii_lowercase().contains("authorization:"));
    let path = request.split_whitespace().nth(1).unwrap();
    let parsed = reqwest::Url::parse(&format!("http://local{path}")).unwrap();
    let params = parsed
        .query_pairs()
        .into_owned()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(params.len(), 4);
    assert_eq!(params["q"], window.query());
    assert_eq!(params["start"], window.start_unix_seconds.to_string());
    assert_eq!(params["end"], window.end_unix_seconds.to_string());
    assert_eq!(params["limit"], TRACE_LIMIT.to_string());
}

#[tokio::test]
async fn unavailable_empty_malformed_and_unrelated_evidence_is_inconclusive() {
    let window = window();
    let mut unrelated = sample(&window);
    unrelated["traces"][0]["spanSets"][0]["spans"][0]["attributes"][1]["value"]["stringValue"] =
        json!("apps-prod");
    for body in [
        "{}".to_string(),
        json!({"traces":[]}).to_string(),
        "invalid-json".into(),
        unrelated.to_string(),
    ] {
        let (url, task) = server(response(&body), Duration::ZERO).await;
        let output = ReadOnlyClusterTools::default()
            .with_tempo_url_option(Some(url))
            .observe_finance_traces(&window)
            .await
            .unwrap();
        assert_eq!(output.status, ToolResultStatus::Error);
        assert_eq!(output.content["sample_state"], "inconclusive");
        assert_eq!(output.content["release_verification"], "not_evaluated");
        task.await.unwrap();
    }
    for endpoint in [
        None,
        Some(String::new()),
        Some("https://user:secret-body-canary@example.test".into()),
        Some("https://example.test?token=secret-body-canary".into()),
    ] {
        let output = ReadOnlyClusterTools::default()
            .with_tempo_url_option(endpoint)
            .observe_finance_traces(&window)
            .await
            .unwrap();
        assert_eq!(output.content["sample_state"], "inconclusive");
        assert!(!serde_json::to_string(&output)
            .unwrap()
            .contains("secret-body-canary"));
    }
}

#[tokio::test]
async fn enforces_the_body_cap_and_deadline_and_never_follows_redirects() {
    let window = window();
    let redirected = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let redirect = format!("HTTP/1.1 302 Found\r\nLocation: http://{}/secret-body-canary\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", redirected.local_addr().unwrap());
    let large = "x".repeat(512);
    for (reply, delay, reason) in [
        (response(&large), Duration::ZERO, "tempo_response_too_large"),
        (format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n200\r\n{large}\r\n0\r\n\r\n"), Duration::ZERO, "tempo_response_too_large"),
        (response("{}"), Duration::from_millis(150), "deadline"),
        ("HTTP/1.1 503 Unavailable\r\nContent-Length: 18\r\nConnection: close\r\n\r\nsecret-body-canary".into(), Duration::ZERO, "tempo_http_error"),
        (redirect, Duration::ZERO, "tempo_http_error"),
    ] {
        let (url, task) = server(reply, delay).await;
        let output = ReadOnlyClusterTools::default().with_tempo_url_option(Some(url))
            .with_max_output_bytes(128).with_timeout_ms(75)
            .observe_finance_traces(&window).await.unwrap();
        assert_eq!(output.content["sample_state"], "inconclusive");
        let actual = output.content["reasons"][0].as_str().unwrap();
        if reason == "deadline" {
            assert!(["tempo_request_timed_out","tempo_body_failed"].contains(&actual), "{actual}");
        } else { assert_eq!(actual, reason); }
        assert!(!serde_json::to_string(&output).unwrap().contains("secret-body-canary"));
        task.await.unwrap();
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(50), redirected.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn rejects_invalid_targets_and_elapsed_window_bounds_before_network_access() {
    let base = window();
    for window in [
        FinanceTraceWindow {
            namespace: "monitoring".into(),
            ..base.clone()
        },
        FinanceTraceWindow {
            namespace: "apps-staging\" || true".into(),
            ..base.clone()
        },
        FinanceTraceWindow {
            end_unix_seconds: base.start_unix_seconds,
            ..base.clone()
        },
        FinanceTraceWindow {
            start_unix_seconds: base.end_unix_seconds - 601,
            ..base.clone()
        },
        FinanceTraceWindow {
            end_unix_seconds: base.end_unix_seconds + 300,
            start_unix_seconds: base.end_unix_seconds,
            ..base.clone()
        },
    ] {
        assert!(ReadOnlyClusterTools::default()
            .observe_finance_traces(&window)
            .await
            .is_err());
    }
    let stale = FinanceTraceWindow {
        start_unix_seconds: base.start_unix_seconds - 120,
        end_unix_seconds: base.end_unix_seconds - 120,
        ..base.clone()
    };
    let result = ReadOnlyClusterTools::default()
        .observe_finance_traces(&stale)
        .await
        .unwrap();
    assert_eq!(result.content["reasons"][0], "stale_query_window");
    let production = FinanceTraceWindow {
        namespace: "apps-prod".into(),
        ..base
    };
    assert!(production.validate(unix_seconds().unwrap()).is_ok());
}

#[test]
fn rejects_wrong_scope_timing_duplicate_identities_and_malformed_data_without_claiming_health() {
    let window = window();
    let baseline = sample(&window);
    let mut mutations = Vec::new();
    for (pointer, value) in [
        ("/traces/0/traceID", json!("not-a-trace")),
        (
            "/traces/0/spanSets/0/spans/0/startTimeUnixNano",
            json!((window.start_unix_seconds - 1) * 1_000_000_000),
        ),
        (
            "/traces/0/spanSets/0/spans/0/startTimeUnixNano",
            json!(window.end_unix_seconds * 1_000_000_000),
        ),
        (
            "/traces/0/spanSets/0/spans/0/attributes/0/value/stringValue",
            json!("finance-frontend"),
        ),
        ("/traces/0/spanSets/0/spans/0/attributes", json!([])),
    ] {
        let mut changed = baseline.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        mutations.push(changed);
    }
    let mut duplicate = baseline.clone();
    duplicate["traces"]
        .as_array_mut()
        .unwrap()
        .push(baseline["traces"][0].clone());
    mutations.push(duplicate);
    let mut excessive = baseline.clone();
    excessive["traces"] = json!(vec![baseline["traces"][0].clone(); TRACE_LIMIT + 1]);
    mutations.push(excessive);
    let mut ambiguous = baseline.clone();
    ambiguous["traces"][0]["spanSets"][0]["spans"][0]["attributes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"key":"service.namespace","value":{"stringValue":"apps-prod"}}));
    mutations.push(ambiguous);
    for changed in mutations {
        let result = summarize(&changed, &window, window.end_unix_seconds);
        assert_eq!(result["malformed"], true);
        assert_eq!(result["absence_of_errors_established"], false);
    }
}

#[test]
fn normalizes_tempos_unpadded_trace_ids_before_detecting_duplicate_identities() {
    for (raw, expected) in [
        ("123", "00000000000000000000000000000123"),
        (
            "ABCDEF1234567890ABCDEF123456789",
            "0abcdef1234567890abcdef123456789",
        ),
        (
            "abcdef1234567890abcdef12345678",
            "00abcdef1234567890abcdef12345678",
        ),
    ] {
        assert_eq!(trace_identifier(&json!(raw)).as_deref(), Some(expected));
    }
    for raw in [
        "",
        "0",
        "00000000000000000000000000000000",
        "not-hex",
        "0x123",
        "12345678901234567890123456789012345",
    ] {
        assert!(trace_identifier(&json!(raw)).is_none());
    }
    let window = window();
    let mut body = sample(&window);
    body["traces"][0]["traceID"] = json!("123");
    let mut duplicate = body["traces"][0].clone();
    duplicate["traceID"] = json!("00000000000000000000000000000123");
    body["traces"].as_array_mut().unwrap().push(duplicate);
    assert_eq!(
        summarize(&body, &window, window.end_unix_seconds)["malformed"],
        true
    );
}

#[test]
fn captured_m02_tempo_shape_preserves_the_partial_search_and_cannot_be_current_acceptance() {
    let captured: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../planning/evidence/autonomous-sdlc/ASTRA-M02-STAGING-TELEMETRY-VERIFIED.json"
    )))
    .unwrap();
    let entry = captured["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["source"] == "tempo_backend")
        .unwrap();
    let window = FinanceTraceWindow {
        namespace: "apps-staging".into(),
        start_unix_seconds: entry["query"]["start"].as_str().unwrap().parse().unwrap(),
        end_unix_seconds: entry["query"]["end"].as_str().unwrap().parse().unwrap(),
    };
    let output = summarize(&entry["result"], &window, window.end_unix_seconds);
    assert_eq!(output["malformed"], false);
    assert_eq!(output["traces"].as_array().unwrap().len(), 3);
    assert_eq!(output["search_jobs_complete"], false);
    assert_eq!(output["search_metrics"]["completed_jobs"], 1);
    assert_eq!(output["search_metrics"]["total_jobs"], 3);
    assert_eq!(output["absence_of_errors_established"], false);
    assert!(
        summarize(&entry["result"], &window, window.end_unix_seconds + 61)["window_age_seconds"]
            .as_u64()
            .unwrap()
            > 60
    );
}

#[tokio::test]
#[ignore = "explicitly authorized read-only Tempo access is required"]
async fn live_finance_tempo_sample() {
    let endpoint = std::env::var("PHARNESS_TEMPO_LIVE_TEST_URL")
        .expect("explicit local Tempo forwarding URL required");
    let output = ReadOnlyClusterTools::default()
        .with_tempo_url_option(Some(endpoint))
        .observe_finance_traces(&window())
        .await
        .unwrap();
    println!("{}", serde_json::to_string(&output).unwrap());
    assert_eq!(output.content["sample_state"], "sample_available");
    assert_eq!(output.content["release_verification"], "not_evaluated");
}
