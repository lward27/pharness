use super::{
    timestamp, FinanceApplication, FinanceDeploymentExpectation, FinanceEnvironment,
    ReadOnlyClusterTools, ToolError, ToolResult,
};
use reqwest::{redirect::Policy, Client, Url};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests;

const MAX_BODY_BYTES: usize = 65_536;

impl ReadOnlyClusterTools {
    /// Native, deterministic GET probes against fixed in-cluster Finance Services.
    /// Expected release identity is a binding, not evidence that a response came
    /// from that image; combine with the deployment reader before accepting it.
    pub async fn observe_finance_probes(
        &self,
        expected: &FinanceDeploymentExpectation,
    ) -> Result<ToolResult, ToolError> {
        expected
            .validate()
            .map_err(|message| ToolError::InvalidArguments { message })?;
        let base = format!(
            "http://{}.{}.svc.cluster.local:{}",
            expected.workload(),
            expected.namespace(),
            expected.service_port()
        );
        let evidence = collect(
            expected,
            &base,
            self.timeout_ms.clamp(1, 15_000),
            self.max_output_bytes.clamp(1, MAX_BODY_BYTES),
        )
        .await?;
        Ok(ToolResult::ok(
            "Observed deterministic Finance responses; complete runtime acceptance is separate",
            evidence,
        ))
    }
}

fn paths(e: &FinanceDeploymentExpectation) -> Vec<&'static str> {
    match e.application {
        FinanceApplication::Yfinance => vec![
            "/healthz",
            "/history?ticker_name=%2A%2A%2A",
            "/markets/not-a-market",
        ],
        FinanceApplication::Frontend if e.environment == FinanceEnvironment::Staging => {
            vec!["/", "/runtime-config.json", "/api/yfinance/healthz"]
        }
        FinanceApplication::Frontend => vec!["/", "/runtime-config.json"],
    }
}

async fn collect(
    e: &FinanceDeploymentExpectation,
    base: &str,
    timeout_ms: u64,
    byte_limit: usize,
) -> Result<Value, ToolError> {
    let started = timestamp()?;
    let client = Client::builder()
        .redirect(Policy::none())
        .no_proxy()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .map_err(|_| ToolError::InvalidArguments {
            message: "Finance probe client is unavailable".into(),
        })?;
    let mut tasks = tokio::task::JoinSet::new();
    for path in paths(e) {
        let (client, e, base) = (client.clone(), e.clone(), base.to_string());
        tasks.spawn(async move {
            probe(&client, &e, &base, path, started, timeout_ms, byte_limit).await
        });
    }
    let mut probes = Vec::new();
    while let Some(result) = tasks.join_next().await {
        probes.push(result.unwrap_or_else(
            |_| json!({"state":"inconclusive","reason":"probe_execution_interrupted"}),
        ));
    }
    probes.sort_by_key(|v| v["path"].as_str().unwrap_or_default().to_owned());
    let state = if probes.iter().any(|v| v["state"] == "failed") {
        "failed"
    } else if probes.iter().any(|v| v["state"] != "passed") {
        "inconclusive"
    } else {
        "passed"
    };
    Ok(
        json!({"schema_version":"pharness.dev/finance-functional-probes/v1alpha1","expected":e,"started_at_unix_ms":started,"completed_at_unix_ms":timestamp()?,"probe_state":state,"probes":probes,"limits":{"requests":paths(e).len(),"request_timeout_ms":timeout_ms,"response_bytes":byte_limit,"http_method":"GET","redirects":false,"retries":0},"latency_slo":"not_defined","deployment_identity":"must_be_verified_separately","runtime_verification":"not_evaluated","scope":"deterministic health, validation and frontend configuration responses; no live market-data correctness or browser-initialization claim"}),
    )
}

async fn probe(
    client: &Client,
    e: &FinanceDeploymentExpectation,
    base: &str,
    path: &str,
    started: u64,
    timeout_ms: u64,
    byte_limit: usize,
) -> Value {
    let clock = Instant::now();
    let material = json!([e, started, path]);
    let correlation = format!("{:x}", Sha256::digest(material.to_string().as_bytes()));
    let trace_id = &correlation[..32];
    let mut evidence = json!({"path":path,"method":"GET","state":"inconclusive","reason":null,"observed_at_unix_ms":started});
    let trace = e.application == FinanceApplication::Yfinance;
    evidence["backend_trace_id"] = if trace { json!(trace_id) } else { Value::Null };
    let result = tokio::time::timeout(Duration::from_millis(timeout_ms), async {
        let url = Url::parse(&format!("{base}{path}")).map_err(|_| "invalid_probe_destination")?;
        if !url.username().is_empty()
            || url.password().is_some()
            || !matches!(url.scheme(), "http" | "https")
        {
            return Err("invalid_probe_destination");
        }
        let mut request = client
            .get(url)
            .header("User-Agent", "pharness-native-sdlc-readiness/1")
            .header("Cache-Control", "no-cache");
        if trace {
            request = request.header(
                "traceparent",
                format!("00-{trace_id}-{}-01", &correlation[32..48]),
            );
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| "probe_transport_unavailable")?;
        let status = response.status().as_u16();
        evidence["http_status"] = json!(status);
        if response
            .content_length()
            .is_some_and(|size| size > byte_limit as u64)
        {
            return Err("probe_response_too_large");
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "probe_body_unavailable")?
        {
            if chunk.len() > byte_limit.saturating_sub(bytes.len()) {
                return Err("probe_response_too_large");
            }
            bytes.extend_from_slice(&chunk);
        }
        evidence["response_bytes"] = json!(bytes.len());
        evidence["response_sha256"] = json!(format!("sha256:{:x}", Sha256::digest(&bytes)));
        let matches = matches_response(e, path, status, &content_type, &bytes);
        Ok(matches)
    })
    .await;
    match result {
        Ok(Ok(true)) => evidence["state"] = json!("passed"),
        Ok(Ok(false)) => {
            evidence["state"] = json!("failed");
            evidence["reason"] = json!("unexpected_application_response");
        }
        Ok(Err(reason)) => evidence["reason"] = json!(reason),
        Err(_) => evidence["reason"] = json!("probe_deadline_exceeded"),
    }
    evidence["elapsed_ms"] = json!(clock.elapsed().as_millis());
    evidence["observed_at_unix_ms"] = json!(timestamp().ok());
    evidence
}

fn matches_response(
    e: &FinanceDeploymentExpectation,
    path: &str,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> bool {
    if path == "/" && e.application == FinanceApplication::Frontend {
        if status != 200 || content_type != "text/html" {
            return false;
        }
        return std::str::from_utf8(body).is_ok_and(|s| {
            s.contains("<div id=\"root\"")
                && s.split("<script").skip(1).any(|script| {
                    script.split('>').next().is_some_and(|tag| {
                        tag.contains("type=\"module\"") && tag.contains("src=\"/assets/")
                    })
                })
        });
    }
    if content_type != "application/json" {
        return false;
    }
    let Ok(document) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    match path {
        "/healthz" | "/api/yfinance/healthz" => status == 200 && document == json!({"status":"ok"}),
        "/history?ticker_name=%2A%2A%2A" => {
            status == 422 && document == json!({"detail":"Ticker contains invalid characters"})
        }
        "/markets/not-a-market" => {
            status == 422
                && document
                    == json!({"detail":"Unsupported market 'not-a-market'. Supported markets: ASIA, COMMODITIES, CRYPTOCURRENCIES, CURRENCIES, EUROPE, GB, RATES, US."})
        }
        "/runtime-config.json" => {
            let (environment, services) = match e.environment {
                FinanceEnvironment::Staging => (
                    "staging",
                    json!({"database":null,"yfinance":"/api/yfinance","scraper":null}),
                ),
                FinanceEnvironment::Production => (
                    "production",
                    json!({"database":"https://finance-db.lucas.engineering","yfinance":"https://yfinance.lucas.engineering","scraper":null}),
                ),
            };
            status == 200
                && document
                    == json!({"schemaVersion":1,"environment":environment,"services":services})
        }
        _ => false,
    }
}
