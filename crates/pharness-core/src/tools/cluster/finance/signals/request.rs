use super::query::Query;
use reqwest::{redirect::Policy, Client, Url};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;

pub(super) async fn read(
    base: Option<&str>,
    query: &Query,
    timeout_ms: u64,
    byte_limit: usize,
) -> Result<(Value, Value), &'static str> {
    let Some(base) = base.filter(|s| !s.trim().is_empty()) else {
        return Err("provider_not_configured");
    };
    let mut url = Url::parse(base).map_err(|_| "invalid_provider_endpoint")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("invalid_provider_endpoint");
    }
    let endpoint = if query.range { "query_range" } else { "query" };
    let prefix = if query.source == "mimir" {
        "api/v1"
    } else {
        "loki/api/v1"
    };
    url.set_path(&format!(
        "{}/{prefix}/{endpoint}",
        url.path().trim_end_matches('/')
    ));
    url.query_pairs_mut().extend_pairs(&query.parameters);
    let client = Client::builder()
        .redirect(Policy::none())
        .no_proxy()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .map_err(|_| "provider_client_unavailable")?;
    let bytes = tokio::time::timeout(Duration::from_millis(timeout_ms), async {
        let mut response = client
            .get(url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|_| "provider_request_failed")?;
        if response.status().as_u16() != 200 {
            return Err("provider_http_error");
        }
        if response
            .content_length()
            .is_some_and(|n| n > byte_limit as u64)
        {
            return Err("provider_response_too_large");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "provider_body_failed")? {
            if chunk.len() > byte_limit.saturating_sub(bytes.len()) {
                return Err("provider_response_too_large");
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    })
    .await
    .map_err(|_| "provider_request_timed_out")??;
    let body = serde_json::from_slice(&bytes).map_err(|_| "provider_response_not_json")?;
    let receipt = json!({"http_status":200,"bytes":bytes.len(),"sha256":format!("sha256:{:x}",Sha256::digest(&bytes))});
    Ok((body, receipt))
}
