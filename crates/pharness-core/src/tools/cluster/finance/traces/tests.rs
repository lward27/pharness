use super::*;
use crate::tools::FinanceEnvironment;
use base64::{engine::general_purpose::STANDARD, Engine};

fn expected(application: FinanceApplication) -> FinanceDeploymentExpectation {
    FinanceDeploymentExpectation {
        application,
        environment: FinanceEnvironment::Staging,
        gitops_commit_sha: "a".repeat(40),
        image_digest: format!("sha256:{}", "b".repeat(64)),
    }
}
fn window() -> FinanceRuntimeWindow {
    let end = timestamp().unwrap() / 30_000 * 30;
    FinanceRuntimeWindow {
        start_unix_seconds: end - 300,
        end_unix_seconds: end,
    }
}
fn probes(e: &FinanceDeploymentExpectation, w: &FinanceRuntimeWindow) -> Value {
    let start = w.start_unix_seconds * 1000 + 1000;
    let correlation = format!(
        "{:x}",
        Sha256::digest(json!([e, start, "/healthz"]).to_string().as_bytes())
    );
    json!({"schema_version":"pharness.dev/finance-functional-probes/v1alpha1","expected":e,
        "probe_state":"failed","started_at_unix_ms":start,"completed_at_unix_ms":start+400,
        "probes":[{"path":"/healthz","method":"GET","state":"passed","http_status":200,
            "observed_at_unix_ms":start+300,"backend_trace_id":&correlation[..32],"backend_parent_span_id":&correlation[32..48]},
            {"path":"/history?ticker_name=%2A%2A%2A","state":"failed"},{"path":"/markets/not-a-market","state":"failed"}]})
}
fn identity(e: &FinanceDeploymentExpectation, start: u64, end: u64) -> Value {
    json!({"schema_version":"pharness.dev/finance-deployment-observation/v1alpha1","cluster":"lucas_engineering","expected":e,
        "identity_state":"verified","started_at_unix_ms":start,"completed_at_unix_ms":end,
        "identity":{"image_ref":e.image_ref(),"gitops_revision":e.gitops_commit_sha,"generation":1,"replicas":1,
            "template_hash":"template","deployment":{"uid":"deployment-uid"},"service":{"uid":"service-uid"},
            "pods":[{"name":format!("{}-abc-one",e.workload()),"uid":"e763137d-8b31-411c-b27d-127166e0de98","ip":"10.42.0.84","restart_count":0,"image_id":e.image_ref()}]}})
}
fn identities(e: &FinanceDeploymentExpectation, w: &FinanceRuntimeWindow) -> (Value, Value) {
    (
        identity(
            e,
            w.start_unix_seconds * 1000 - 1000,
            w.start_unix_seconds * 1000 - 500,
        ),
        identity(e, w.end_unix_seconds * 1000, w.end_unix_seconds * 1000),
    )
}
fn string_attribute(key: &str, value: &str) -> Value {
    json!({"key":key,"value":{"stringValue":value}})
}
fn base64_id(hex: &str) -> String {
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect::<Vec<_>>();
    STANDARD.encode(bytes)
}
fn body(e: &FinanceDeploymentExpectation, h: &HealthRequest) -> Value {
    json!({"metrics":{"inspectedBytes":"2360"},"trace":{"resourceSpans":[{
        "resource":{"attributes":[string_attribute("service.name","yfinance-wrapper"),string_attribute("service.namespace",e.namespace())]},
        "scopeSpans":[{"spans":[{"traceId":base64_id(&h.trace_id),"spanId":STANDARD.encode([3_u8;8]),
            "parentSpanId":base64_id(&h.parent_span_id),"kind":"SPAN_KIND_SERVER","name":"GET /healthz",
            "startTimeUnixNano":(h.started_ms*1_000_000+100_000_000).to_string(),
            "endTimeUnixNano":(h.completed_ms*1_000_000+100_000_000).to_string(),"status":{},
            "attributes":[string_attribute("http.method","GET"),string_attribute("http.route","/healthz"),string_attribute("http.target","/healthz"),
                {"key":"http.status_code","value":{"intValue":"200"}}]}]}]}]}})
}
const SPAN: &str = "/trace/resourceSpans/0/scopeSpans/0/spans/0";

#[test]
fn correlation_requires_the_native_health_receipt_and_keeps_other_failures() {
    let e = expected(FinanceApplication::Yfinance);
    let w = window();
    let p = probes(&e, &w);
    assert!(HealthRequest::from_probes(&e, &w, &p).is_ok());
    assert_eq!(p["probe_state"], "failed");
    for (pointer, value) in [
        ("/schema_version", json!("agent_claim")),
        ("/expected/image_digest", json!("latest")),
        (
            "/started_at_unix_ms",
            json!(w.start_unix_seconds * 1000 - 1),
        ),
        ("/probes/0/backend_parent_span_id", json!("f".repeat(16))),
        ("/probes/0/backend_trace_id", json!("f".repeat(32))),
        ("/probes/0/http_status", json!(500)),
        ("/probes/0/state", json!("inconclusive")),
        ("/probes/0/path", json!("/history")),
    ] {
        let mut changed = p.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(
            HealthRequest::from_probes(&e, &w, &changed).is_err(),
            "{pointer}"
        );
    }
}

#[test]
fn complete_tempo_trace_matches_request_without_inventing_image_labels() {
    let e = expected(FinanceApplication::Yfinance);
    let w = window();
    let h = HealthRequest::from_probes(&e, &w, &probes(&e, &w)).unwrap();
    let mut b = body(&e, &h);
    let s = analysis::summarize(&b, &e, &w, &h).unwrap();
    assert_eq!(s["trace_id"], h.trace_id);
    assert_eq!(s["parent_span_id"], h.parent_span_id);
    assert_eq!(
        s["image_identity_source"],
        "separate native deployment/Service observations"
    );
    for status in [json!(0), json!("COMPLETE")] {
        b["status"] = status;
        assert!(analysis::summarize(&b, &e, &w, &h).is_ok());
    }
    *b.pointer_mut(&(SPAN.to_string() + "/traceId")).unwrap() = json!(h.trace_id);
    assert!(analysis::summarize(&b, &e, &w, &h).is_ok());
}

#[test]
fn unrelated_or_contradictory_server_spans_never_correlate() {
    let e = expected(FinanceApplication::Yfinance);
    let w = window();
    let h = HealthRequest::from_probes(&e, &w, &probes(&e, &w)).unwrap();
    let b = body(&e, &h);
    for (field, value) in [
        ("traceId", json!(STANDARD.encode([7_u8; 16]))),
        ("parentSpanId", json!(STANDARD.encode([7_u8; 8]))),
        ("spanId", json!(STANDARD.encode([0_u8; 8]))),
        ("kind", json!("SPAN_KIND_INTERNAL")),
        ("name", json!("GET /different")),
        ("status", json!({"code":"STATUS_CODE_ERROR"})),
    ] {
        let mut changed = b.clone();
        *changed.pointer_mut(SPAN).unwrap().get_mut(field).unwrap() = value;
        assert!(
            analysis::summarize(&changed, &e, &w, &h).is_err(),
            "{field}"
        );
    }
    for pointer in [
        "/trace/resourceSpans/0/resource/attributes/0/value/stringValue",
        "/trace/resourceSpans/0/resource/attributes/1/value/stringValue",
    ] {
        let mut changed = b.clone();
        *changed.pointer_mut(pointer).unwrap() = json!("unrelated");
        assert!(analysis::summarize(&changed, &e, &w, &h).is_err());
    }
    let mut changed = b.clone();
    *changed
        .pointer_mut(&(SPAN.to_string() + "/attributes/3/value/intValue"))
        .unwrap() = json!("500");
    assert!(analysis::summarize(&changed, &e, &w, &h).is_err());
}

#[test]
fn trace_times_are_bound_to_the_actual_request_with_finite_clock_skew() {
    let e = expected(FinanceApplication::Yfinance);
    let w = window();
    let h = HealthRequest::from_probes(&e, &w, &probes(&e, &w)).unwrap();
    let b = body(&e, &h);
    for (field, value) in [
        ("startTimeUnixNano", 0),
        ("endTimeUnixNano", 0),
        (
            "endTimeUnixNano",
            h.completed_ms * 1_000_000 + 1_000_000_001,
        ),
        (
            "startTimeUnixNano",
            (w.end_unix_seconds + 100) * 1_000_000_000,
        ),
    ] {
        let mut changed = b.clone();
        *changed.pointer_mut(SPAN).unwrap().get_mut(field).unwrap() = json!(value.to_string());
        assert!(
            analysis::summarize(&changed, &e, &w, &h).is_err(),
            "{field}"
        );
    }
}

#[test]
fn partial_missing_duplicate_and_oversized_traces_are_inconclusive() {
    let e = expected(FinanceApplication::Yfinance);
    let w = window();
    let h = HealthRequest::from_probes(&e, &w, &probes(&e, &w)).unwrap();
    let b = body(&e, &h);
    for (key, value) in [
        ("status", json!("PARTIAL")),
        ("status", json!(1)),
        ("message", json!("incomplete")),
        ("error", json!("failed")),
        ("metrics", Value::Null),
    ] {
        let mut changed = b.clone();
        changed[key] = value;
        assert!(analysis::summarize(&changed, &e, &w, &h).is_err());
    }
    let mut changed = b.clone();
    changed["trace"]["resourceSpans"] = json!([]);
    assert!(analysis::summarize(&changed, &e, &w, &h).is_err());
    let mut changed = b.clone();
    changed["trace"]["resourceSpans"] = json!(vec![b["trace"]["resourceSpans"][0].clone(); 9]);
    assert!(analysis::summarize(&changed, &e, &w, &h).is_err());
    let mut changed = b.clone();
    let span = changed.pointer(SPAN).unwrap().clone();
    changed["trace"]["resourceSpans"][0]["scopeSpans"][0]["spans"]
        .as_array_mut()
        .unwrap()
        .push(span);
    assert!(analysis::summarize(&changed, &e, &w, &h).is_err());
}

#[tokio::test]
async fn unavailable_tempo_and_frontend_do_not_create_success_or_leak_credentials() {
    let e = expected(FinanceApplication::Yfinance);
    let w = window();
    let (b, a) = identities(&e, &w);
    let p = probes(&e, &w);
    let tools = ReadOnlyClusterTools::default()
        .with_tempo_url_option(Some("http://user:credential-canary@127.0.0.1:1".into()));
    let r = tools
        .observe_finance_health_trace(&e, &w, &b, &a, &p)
        .await
        .unwrap()
        .content;
    assert_eq!(r["trace_state"], "inconclusive");
    assert_eq!(r["functional_probe_state"], "failed");
    assert!(!r.to_string().contains("credential-canary"));
    let e = expected(FinanceApplication::Frontend);
    let (b, a) = identities(&e, &w);
    let r = tools
        .observe_finance_health_trace(&e, &w, &b, &a, &Value::Null)
        .await
        .unwrap()
        .content;
    assert_eq!(r["trace_state"], "not_instrumented");
    assert_eq!(r["limits"]["requests"], 0);
    assert_eq!(r["runtime_verification"], "not_evaluated");
}

#[tokio::test]
async fn trace_transport_does_not_follow_redirects_or_accept_unbounded_responses() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let w = window();
    for (response,reason) in [
        ("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/credential-canary\r\nContent-Length: 0\r\nConnection: close\r\n\r\n","tempo_trace_unavailable"),
        ("HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\n","tempo_response_too_large"),
        ("HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsecret","tempo_response_not_json"),
    ] {
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let endpoint=format!("http://{}",listener.local_addr().unwrap());
        let server=tokio::spawn(async move {let (mut socket,_)=listener.accept().await.unwrap();let mut bytes=[0;4096];let n=socket.read(&mut bytes).await.unwrap();let request=std::str::from_utf8(&bytes[..n]).unwrap();assert!(request.starts_with("GET /api/v2/traces/"));assert!(request.contains("?start="));assert!(request.contains("&end="));socket.write_all(response.as_bytes()).await.unwrap();});
        assert_eq!(request::read(Some(&endpoint),&"a".repeat(32),&w,1000,128).await.unwrap_err(),reason);server.await.unwrap();
    }
}
