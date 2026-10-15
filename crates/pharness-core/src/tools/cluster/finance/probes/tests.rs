use super::*;
mod runtime_live;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[tokio::test]
#[ignore = "explicit staging access, three deterministic probes, Tempo and an actual five-minute window"]
async fn live_finance_health_trace_correlation() {
    let expected: FinanceDeploymentExpectation = serde_json::from_str(
        &std::env::var("PHARNESS_FINANCE_LIVE_EXPECTATION").expect("explicit expectation required"),
    )
    .unwrap();
    assert_eq!(expected.application, FinanceApplication::Yfinance);
    assert_eq!(expected.environment, FinanceEnvironment::Staging);
    let tools = ReadOnlyClusterTools::default()
        .with_kubectl_bin(
            std::env::var("PHARNESS_FINANCE_LIVE_KUBECTL")
                .expect("explicit cluster wrapper required"),
        )
        .with_tempo_url_option(Some(
            std::env::var("PHARNESS_FINANCE_LIVE_TEMPO").expect("explicit Tempo endpoint required"),
        ));
    let base = std::env::var("PHARNESS_FINANCE_LIVE_PROBE_BASE")
        .expect("explicit staging probe endpoint required");
    let before = tools
        .observe_finance_deployment(&expected)
        .await
        .unwrap()
        .content;
    assert_eq!(before["identity_state"], "verified");
    let window =
        crate::tools::FinanceRuntimeWindow::starting_after(timestamp().unwrap(), 300).unwrap();
    println!(
        "{}",
        json!({"identity_before":before,"planned_window":window})
    );
    async fn wait_until(target_ms: u64) {
        loop {
            let now = timestamp().unwrap();
            if now >= target_ms {
                break;
            }
            tokio::time::sleep(Duration::from_millis((target_ms - now).min(1000))).await;
        }
    }
    wait_until(window.start_unix_seconds * 1000).await;
    let probes = collect(&expected, &base, 15_000, MAX_BODY_BYTES)
        .await
        .unwrap();
    println!("{}", json!({"functional_probes":probes}));
    wait_until((window.end_unix_seconds + 10) * 1000).await;
    let after = tools
        .observe_finance_deployment(&expected)
        .await
        .unwrap()
        .content;
    let trace = tools
        .observe_finance_health_trace(&expected, &window, &before, &after, &probes)
        .await
        .unwrap();
    println!("{}", json!({"identity_after":after,"health_trace":trace}));
    assert_eq!(trace.content["trace_state"], "observed");
    assert_eq!(
        trace.content["functional_probe_state"],
        probes["probe_state"]
    );
    assert_eq!(trace.content["runtime_verification"], "not_evaluated");
}

fn expected(
    application: FinanceApplication,
    environment: FinanceEnvironment,
) -> FinanceDeploymentExpectation {
    FinanceDeploymentExpectation {
        application,
        environment,
        gitops_commit_sha: "a".repeat(40),
        image_digest: format!("sha256:{}", "b".repeat(64)),
    }
}

fn healthy_body(e: &FinanceDeploymentExpectation, path: &str) -> (u16, &'static str, String) {
    match path {
        "/" => (200,"text/html",r#"<!doctype html><div id="root"></div><script type="module" crossorigin src="/assets/index-abc.js"></script>"#.into()),
        "/healthz"|"/api/yfinance/healthz" => (200,"application/json",json!({"status":"ok"}).to_string()),
        "/history?ticker_name=%2A%2A%2A" => (422,"application/json",json!({"detail":"Ticker contains invalid characters"}).to_string()),
        "/markets/not-a-market" => (422,"application/json",json!({"detail":"Unsupported market 'not-a-market'. Supported markets: ASIA, COMMODITIES, CRYPTOCURRENCIES, CURRENCIES, EUROPE, GB, RATES, US."}).to_string()),
        "/runtime-config.json" => {
            let body=match e.environment {
                FinanceEnvironment::Staging => json!({"schemaVersion":1,"environment":"staging","services":{"database":null,"yfinance":"/api/yfinance","scraper":null}}),
                FinanceEnvironment::Production => json!({"schemaVersion":1,"environment":"production","services":{"database":"https://finance-db.lucas.engineering","yfinance":"https://yfinance.lucas.engineering","scraper":null}}),
            };(200,"application/json",body.to_string())
        },
        _=>panic!("unexpected test request"),
    }
}

async fn server(
    e: FinanceDeploymentExpectation,
    mode: &'static str,
) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let mut joins = Vec::new();
        for _ in paths(&e) {
            let (mut socket, _) = listener.accept().await.unwrap();
            let e = e.clone();
            joins.push(tokio::spawn(async move {
                let mut request=Vec::new();let mut buffer=[0;1024];
                while !request.windows(4).any(|b|b==b"\r\n\r\n") {
                    let n=socket.read(&mut buffer).await.unwrap();assert!(n>0);request.extend_from_slice(&buffer[..n]);assert!(request.len()<8192);
                }
                let request=String::from_utf8(request).unwrap();let path=request.split_whitespace().nth(1).unwrap();
                let (status,kind,body)=healthy_body(&e,path);
                let response=match mode {
                    "redirect"=>"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/credential-canary\r\nContent-Length: 17\r\n\r\ncredential-canary".into(),
                    "large"=>format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",MAX_BODY_BYTES+1,"x".repeat(MAX_BODY_BYTES+1)),
                    "chunked_large"=>format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: application/json\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n",MAX_BODY_BYTES+1,"x".repeat(MAX_BODY_BYTES+1)),
                    "slow_body"=>{
                        let _=socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 15\r\n\r\n").await;
                        tokio::time::sleep(Duration::from_millis(150)).await;"{\"status\":\"ok\"}".into()
                    },
                    "bad_body"=>"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 17\r\n\r\ncredential-canary".into(),
                    _=>format!("HTTP/1.1 {status} OK\r\nContent-Type: {kind}; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()),
                };
                let _=socket.write_all(response.as_bytes()).await;request
            }));
        }
        let mut requests = Vec::new();
        for join in joins {
            requests.push(join.await.unwrap());
        }
        requests
    });
    (format!("http://{address}"), handle)
}

#[tokio::test]
async fn deterministic_backend_probes_keep_validation_and_trace_correlation_explicit() {
    let e = expected(FinanceApplication::Yfinance, FinanceEnvironment::Staging);
    let (base, task) = server(e.clone(), "healthy").await;
    let result = collect(&e, &base, 1000, MAX_BODY_BYTES).await.unwrap();
    let requests = task.await.unwrap();
    assert_eq!(result["probe_state"], "passed");
    assert_eq!(result["runtime_verification"], "not_evaluated");
    assert_eq!(result["latency_slo"], "not_defined");
    assert_eq!(requests.len(), 3);
    for probe in result["probes"].as_array().unwrap() {
        let trace = probe["backend_trace_id"].as_str().unwrap();
        assert_eq!(trace.len(), 32);
        assert!(trace.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(probe["backend_parent_span_id"].as_str().unwrap().len(), 16);
        let request = requests
            .iter()
            .find(|r| r.starts_with(&format!("GET {} ", probe["path"].as_str().unwrap())))
            .unwrap();
        assert!(request.contains(&format!("traceparent: 00-{trace}-")));
        assert!(request.contains("cache-control: no-cache"));
        assert!(probe["elapsed_ms"].is_u64());
        assert_eq!(probe["response_sha256"].as_str().unwrap().len(), 71);
    }
}

#[tokio::test]
async fn frontend_probes_do_not_claim_browser_execution_or_fabricate_traces() {
    for environment in [FinanceEnvironment::Staging, FinanceEnvironment::Production] {
        let e = expected(FinanceApplication::Frontend, environment);
        let (base, task) = server(e.clone(), "healthy").await;
        let result = collect(&e, &base, 1000, MAX_BODY_BYTES).await.unwrap();
        let requests = task.await.unwrap();
        assert_eq!(result["probe_state"], "passed");
        assert!(requests.iter().all(|r| !r.contains("traceparent")));
        assert!(result["probes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["backend_trace_id"].is_null()));
        assert_eq!(
            requests.len(),
            if environment == FinanceEnvironment::Staging {
                3
            } else {
                2
            }
        );
        assert!(result["scope"]
            .as_str()
            .unwrap()
            .contains("no live market-data correctness or browser-initialization claim"));
    }
}

#[test]
fn validation_status_shapes_and_environment_config_cannot_silently_change() {
    for application in [FinanceApplication::Yfinance, FinanceApplication::Frontend] {
        let e = expected(application, FinanceEnvironment::Staging);
        for path in paths(&e) {
            let (status, kind, body) = healthy_body(&e, path);
            assert!(matches_response(&e, path, status, kind, body.as_bytes()));
            assert!(!matches_response(&e, path, 500, kind, body.as_bytes()));
            assert!(!matches_response(&e, path, status, kind, b"{}"));
            assert!(!matches_response(
                &e,
                path,
                status,
                "text/plain",
                body.as_bytes()
            ));
        }
    }
    let e = expected(FinanceApplication::Frontend, FinanceEnvironment::Staging);
    let (_, _, production) = healthy_body(
        &expected(FinanceApplication::Frontend, FinanceEnvironment::Production),
        "/runtime-config.json",
    );
    assert!(!matches_response(
        &e,
        "/runtime-config.json",
        200,
        "application/json",
        production.as_bytes()
    ));
    assert!(!matches_response(&e,"/",200,"text/html",b"<div id=\"root\"></div><script type=\"module\" src=\"https://other.example/script.js\"></script>"));
}

#[tokio::test]
async fn redirects_bad_bodies_oversize_and_slow_streams_never_pass() {
    let e = expected(FinanceApplication::Yfinance, FinanceEnvironment::Staging);
    for mode in [
        "redirect",
        "bad_body",
        "large",
        "chunked_large",
        "slow_body",
    ] {
        let (base, task) = server(e.clone(), mode).await;
        let result = collect(
            &e,
            &base,
            if mode == "slow_body" { 40 } else { 1000 },
            MAX_BODY_BYTES,
        )
        .await
        .unwrap();
        assert_ne!(result["probe_state"], "passed", "{mode}");
        assert!(!result.to_string().contains("credential-canary"));
        if mode == "large" || mode == "chunked_large" {
            assert_eq!(result["probe_state"], "inconclusive");
        }
        assert_eq!(task.await.unwrap().len(), 3);
    }
}

#[tokio::test]
async fn transport_loss_is_inconclusive_and_invalid_identity_prevents_requests() {
    let e = expected(FinanceApplication::Yfinance, FinanceEnvironment::Staging);
    let result = collect(&e, "http://127.0.0.1:1", 50, MAX_BODY_BYTES)
        .await
        .unwrap();
    assert_eq!(result["probe_state"], "inconclusive");
    let mut invalid = e;
    invalid.image_digest = "latest".into();
    assert!(ReadOnlyClusterTools::default()
        .observe_finance_probes(&invalid)
        .await
        .is_err());
}

#[tokio::test]
#[ignore = "explicit read-only Finance Service forwarding and exact expected identities required"]
async fn live_finance_deterministic_probes() {
    let e: FinanceDeploymentExpectation = serde_json::from_str(
        &std::env::var("PHARNESS_FINANCE_LIVE_EXPECTATION")
            .expect("expected identity JSON required"),
    )
    .unwrap();
    e.validate().unwrap();
    let base = std::env::var("PHARNESS_FINANCE_LIVE_PROBE_BASE")
        .expect("explicit local Service forwarding URL required");
    let url = Url::parse(&base).unwrap();
    assert_eq!(url.scheme(), "http");
    assert_eq!(url.host_str(), Some("127.0.0.1"));
    assert!(url.port().is_some());
    let evidence = collect(&e, &base, 15_000, MAX_BODY_BYTES).await.unwrap();
    println!("{}", serde_json::to_string(&evidence).unwrap());
    assert_eq!(evidence["probe_state"], "passed");
    assert_eq!(evidence["runtime_verification"], "not_evaluated");
}
