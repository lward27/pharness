use super::FinanceRuntimeWindow;
use reqwest::{redirect::Policy, Client, Url};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;

pub(super) async fn read(
    base: Option<&str>,
    trace_id: &str,
    window: &FinanceRuntimeWindow,
    timeout_ms: u64,
    byte_limit: usize,
) -> Result<(Value, Value), &'static str> {
    let base = base
        .filter(|v| !v.trim().is_empty())
        .ok_or("tempo_not_configured")?;
    let mut url = Url::parse(base).map_err(|_| "invalid_tempo_endpoint")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("invalid_tempo_endpoint");
    }
    // The expectation is native and hexadecimal; no arbitrary path or TraceQL.
    if trace_id.len() != 32 || !trace_id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid_native_trace_id");
    }
    let path = format!(
        "{}/api/v2/traces/{trace_id}",
        url.path().trim_end_matches('/')
    );
    url.set_path(&path);
    url.query_pairs_mut()
        .append_pair("start", &window.start_unix_seconds.to_string())
        .append_pair("end", &window.end_unix_seconds.to_string());
    let client = Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .map_err(|_| "tempo_client_unavailable")?;
    let response = tokio::time::timeout(Duration::from_millis(timeout_ms), async {
        let mut response = client.get(url).header(reqwest::header::ACCEPT,"application/json")
            .send().await.map_err(|_| "tempo_request_failed")?;
        if response.status().as_u16()!=200 { return Err("tempo_trace_unavailable"); }
        if response.content_length().is_some_and(|n| n>byte_limit as u64) {
            return Err("tempo_response_too_large");
        }
        let mut bytes=Vec::new();
        while let Some(chunk)=response.chunk().await.map_err(|_| "tempo_body_unavailable")? {
            if chunk.len()>byte_limit.saturating_sub(bytes.len()) { return Err("tempo_response_too_large"); }
            bytes.extend_from_slice(&chunk);
        }
        let body=serde_json::from_slice(&bytes).map_err(|_| "tempo_response_not_json")?;
        Ok((body,json!({"http_status":200,"bytes":bytes.len(),"sha256":format!("sha256:{:x}",Sha256::digest(&bytes)),
            "path":path,"query_parameters":{"start":window.start_unix_seconds,"end":window.end_unix_seconds},"raw_trace_stored":false})))
    }).await.map_err(|_| "tempo_request_timed_out")?;
    response
}
