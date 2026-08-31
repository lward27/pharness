use anyhow::Context;
use pharness_core::AgentEvent;
use pharness_runhost::{AttemptOutcome, AttemptSpec};
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Duration;

const MAX_ERROR_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct EnrollmentExchange<'a> {
    pub enrollment_id: &'a str,
    pub enrollment_token: &'a str,
    pub platform: &'a str,
    pub architecture: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnrollmentResult {
    pub host: HostIdentityResponse,
    pub host_credential: String,
    pub heartbeat_interval_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HostIdentityResponse {
    pub id: String,
    pub display_name: String,
    pub host_pool: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostCapabilities {
    pub platform: String,
    pub architecture: String,
    pub codex_version: String,
    pub podman_version: Option<String>,
    pub execution_mode: String,
    pub authentication_class: String,
    pub authentication_ready: bool,
    pub supported_profiles: Vec<String>,
    pub runner_images: BTreeMap<String, String>,
    pub available_slots: u32,
    pub storage: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaimedLeaseEnvelope {
    pub lease: ClaimedLease,
    pub lease_token: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClaimedLease {
    pub id: String,
    pub run_id: String,
    pub stage_execution_id: String,
    pub host_pool: String,
    pub workspace_id: String,
    pub environment_profile_id: String,
    pub runner_image: String,
    pub binding_hash: String,
    pub remote_thread_id: Option<String>,
}

#[derive(Clone)]
pub struct HostApiClient {
    client: Client,
    api_url: String,
    host_id: String,
    credential: String,
}

impl HostApiClient {
    pub fn new(api_url: &str, host_id: String, credential: String) -> anyhow::Result<Self> {
        Ok(Self {
            client: client()?,
            api_url: api_url.trim_end_matches('/').to_string(),
            host_id,
            credential,
        })
    }

    pub async fn enroll(
        api_url: &str,
        request: &EnrollmentExchange<'_>,
    ) -> anyhow::Result<EnrollmentResult> {
        let client = client()?;
        let url = format!(
            "{}/api/internal/agent-hosts/enroll",
            api_url.trim_end_matches('/')
        );
        request_json(&client, reqwest::Method::POST, &url, None, Some(request)).await
    }

    pub async fn heartbeat(&self, capabilities: &HostCapabilities) -> anyhow::Result<Value> {
        self.post(
            &format!("/api/internal/agent-hosts/{}/heartbeat", self.host_id),
            capabilities,
        )
        .await
    }

    pub async fn claim(&self) -> anyhow::Result<Option<ClaimedLeaseEnvelope>> {
        self.post_value(
            &format!("/api/internal/agent-hosts/{}/leases/claim", self.host_id),
            &json!({}),
        )
        .await
    }

    async fn post<T: Serialize + ?Sized>(&self, path: &str, body: &T) -> anyhow::Result<Value> {
        request_json(
            &self.client,
            reqwest::Method::POST,
            &format!("{}{}", self.api_url, path),
            Some(&self.credential),
            Some(body),
        )
        .await
    }

    async fn post_value<T: DeserializeOwned>(&self, path: &str, body: &Value) -> anyhow::Result<T> {
        request_json(
            &self.client,
            reqwest::Method::POST,
            &format!("{}{}", self.api_url, path),
            Some(&self.credential),
            Some(body),
        )
        .await
    }
}

#[derive(Clone)]
pub struct LeaseApiClient {
    client: Client,
    api_url: String,
    host_id: String,
    lease_id: String,
    lease_token: String,
}

impl LeaseApiClient {
    pub fn new(
        api_url: &str,
        host_id: String,
        lease_id: String,
        lease_token: String,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            client: client()?,
            api_url: api_url.trim_end_matches('/').to_string(),
            host_id,
            lease_id,
            lease_token,
        })
    }

    fn path(&self, suffix: &str) -> String {
        format!(
            "{}/api/internal/agent-hosts/{}/leases/{}/{}",
            self.api_url, self.host_id, self.lease_id, suffix
        )
    }

    pub async fn context(&self) -> anyhow::Result<AttemptSpec> {
        request_json::<(), AttemptSpec>(
            &self.client,
            reqwest::Method::GET,
            &self.path("context"),
            Some(&self.lease_token),
            None,
        )
        .await
    }

    pub async fn mark_running(&self) -> anyhow::Result<()> {
        self.post_value("mark-running", &json!({})).await
    }

    pub async fn heartbeat(&self) -> anyhow::Result<Value> {
        self.post("heartbeat", &json!({})).await
    }

    pub async fn set_remote_thread(&self, remote_thread_id: &str) -> anyhow::Result<()> {
        self.post_value(
            "remote-thread",
            &json!({"remote_thread_id":remote_thread_id}),
        )
        .await
    }

    pub async fn workspace_provisioned(
        &self,
        workspace_id: &str,
        resolved_commit: &str,
        branch: &str,
    ) -> anyhow::Result<()> {
        self.post_value(
            "workspace-provisioned",
            &json!({
                "workspace_id":workspace_id,
                "resolved_commit":resolved_commit,
                "branch":branch,
            }),
        )
        .await
    }

    pub async fn environment_preparation(&self, payload: &Value) -> anyhow::Result<()> {
        self.post_value("environment-preparation", payload).await
    }

    pub async fn events(&self, events: &[AgentEvent]) -> anyhow::Result<()> {
        self.post_value("events", &json!({"events":events})).await
    }

    pub async fn outcome(&self, outcome: &AttemptOutcome) -> anyhow::Result<()> {
        self.post_value("outcome", &serde_json::to_value(outcome)?)
            .await
    }

    pub async fn complete(
        &self,
        state: &str,
        completion_hash: Option<&str>,
        error: Option<&str>,
    ) -> anyhow::Result<()> {
        self.post_value(
            "complete",
            &json!({
                "state":state,
                "completion_hash":completion_hash,
                "error":error,
            }),
        )
        .await
    }

    pub async fn pause(&self, stop_category: &str, detail: &str) -> anyhow::Result<()> {
        self.post_value(
            "pause",
            &json!({"stop_category":stop_category,"detail":detail}),
        )
        .await
    }

    async fn post(&self, suffix: &str, body: &Value) -> anyhow::Result<Value> {
        request_json(
            &self.client,
            reqwest::Method::POST,
            &self.path(suffix),
            Some(&self.lease_token),
            Some(body),
        )
        .await
    }

    async fn post_value(&self, suffix: &str, body: &Value) -> anyhow::Result<()> {
        let _: Value = self.post(suffix, body).await?;
        Ok(())
    }
}

fn client() -> anyhow::Result<Client> {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(45))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("failed to build agent-host HTTP client")
}

async fn request_json<B: Serialize + ?Sized, T: DeserializeOwned>(
    client: &Client,
    method: reqwest::Method,
    url: &str,
    bearer: Option<&str>,
    body: Option<&B>,
) -> anyhow::Result<T> {
    let mut request = client.request(method, url);
    if let Some(bearer) = bearer {
        request = request.bearer_auth(bearer);
    }
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request
        .send()
        .await
        .context("agent-host API request failed")?;
    let status = response.status();
    if !status.is_success() {
        let bytes = response.bytes().await.unwrap_or_default();
        let bounded = &bytes[..bytes.len().min(MAX_ERROR_BYTES)];
        let message = String::from_utf8_lossy(bounded);
        anyhow::bail!("agent-host API returned {status}: {message}");
    }
    response
        .json::<T>()
        .await
        .context("agent-host API returned invalid JSON")
}

pub fn subscription_quota_error(error: &str) -> bool {
    let normalized = error.to_ascii_lowercase();
    normalized.contains("usage limit")
        || normalized.contains("rate limit")
        || normalized.contains("quota")
        || normalized.contains("too many requests")
        || normalized.contains("http 429")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_subscription_quota_without_assuming_missing_usage_is_zero() {
        assert!(subscription_quota_error("You have hit your usage limit"));
        assert!(subscription_quota_error("HTTP 429 Too Many Requests"));
        assert!(!subscription_quota_error("invalid structured output"));
    }
}
