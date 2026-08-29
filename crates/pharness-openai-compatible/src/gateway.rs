use crate::{
    aggregate_to_model_turn, build_chat_request, OpenAiCompatibleError, OpenAiStreamAggregate,
    SseDecoder, StreamChunk,
};
use async_trait::async_trait;
use futures::StreamExt;
use pharness_core::{
    canonical_json_sha256, ModelCapabilities, ModelProvider, ModelRequest, ModelTurn,
    ProviderError, ResolvedInferenceBinding,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::time::timeout;
use url::Url;

#[derive(Clone)]
pub struct GatewayClientConfig {
    pub api_base_url: String,
    pub gateway_base_url: String,
    pub worker_token: SecretString,
    pub selection_id: String,
    pub stage_execution_id: String,
    pub binding: ResolvedInferenceBinding,
    pub next_request_sequence: u32,
}

#[derive(Clone)]
pub struct GatewayModelClient {
    http: reqwest::Client,
    api_base_url: Url,
    gateway_base_url: Url,
    worker_token: SecretString,
    selection_id: String,
    stage_execution_id: String,
    binding: ResolvedInferenceBinding,
    next_request_sequence: Arc<AtomicU32>,
}

#[derive(Debug, Serialize)]
struct ModelGrantRequest {
    selection_id: String,
    stage_execution_id: String,
    request_sequence: u32,
    request_body_hash: String,
}

#[derive(Debug, Deserialize)]
struct ModelGrantResponse {
    token: String,
    expires_at_epoch_seconds: u64,
}

impl GatewayModelClient {
    pub fn new(config: GatewayClientConfig) -> Result<Self, OpenAiCompatibleError> {
        config
            .binding
            .validate()
            .map_err(|error| OpenAiCompatibleError::InvalidConfiguration(error.to_string()))?;
        let api_base_url = internal_base_url(&config.api_base_url)?;
        let gateway_base_url = internal_base_url(&config.gateway_base_url)?;
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(OpenAiCompatibleError::ClientBuild)?;
        Ok(Self {
            http,
            api_base_url,
            gateway_base_url,
            worker_token: config.worker_token,
            selection_id: config.selection_id,
            stage_execution_id: config.stage_execution_id,
            binding: config.binding,
            next_request_sequence: Arc::new(AtomicU32::new(config.next_request_sequence.max(1))),
        })
    }

    fn gateway_request(&self, request: ModelRequest) -> crate::ChatRequest {
        let mut wire = build_chat_request(
            self.binding.target.backend_kind,
            self.binding.target.upstream_model.clone(),
            request,
            &self.binding.policy,
            self.binding.target.capabilities.stream_options,
            self.binding
                .target
                .openrouter
                .as_ref()
                .map(|route| route.provider_slug.as_str()),
        );
        wire.model = format!(
            "{}@{}",
            self.binding.target.target_id, self.binding.target.revision
        );
        wire
    }

    async fn request_grant(
        &self,
        run_id: &str,
        request_sequence: u32,
        request_body_hash: String,
    ) -> Result<ModelGrantResponse, ProviderError> {
        let url = self
            .api_base_url
            .join(&format!("api/internal/runs/{run_id}/model-grants"))
            .map_err(|error| ProviderError::MalformedResponse {
                message: format!("invalid internal model-grant endpoint: {error}"),
            })?;
        let response = timeout(
            Duration::from_secs(10),
            self.http
                .post(url)
                .bearer_auth(self.worker_token.expose_secret())
                .json(&ModelGrantRequest {
                    selection_id: self.selection_id.clone(),
                    stage_execution_id: self.stage_execution_id.clone(),
                    request_sequence,
                    request_body_hash,
                })
                .send(),
        )
        .await
        .map_err(|_| ProviderError::RequestFailed {
            message: "model-grant request timed out".into(),
            retryable: false,
        })?
        .map_err(|error| ProviderError::RequestFailed {
            message: format!("model-grant request failed: {error}"),
            retryable: false,
        })?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::RequestFailed {
                message: format!(
                    "model-grant request returned {status}: {}",
                    body.chars().take(500).collect::<String>()
                ),
                retryable: false,
            });
        }
        response
            .json()
            .await
            .map_err(|error| ProviderError::MalformedResponse {
                message: format!("invalid model-grant response: {error}"),
            })
    }

    async fn complete_gateway_stream(
        &self,
        request: &serde_json::Value,
        grant: &str,
    ) -> Result<OpenAiStreamAggregate, ProviderError> {
        let url = self
            .gateway_base_url
            .join("chat/completions")
            .map_err(|error| ProviderError::MalformedResponse {
                message: format!("invalid model-gateway endpoint: {error}"),
            })?;
        let response = timeout(
            Duration::from_secs(self.binding.target.transport.first_response_timeout_seconds),
            self.http.post(url).bearer_auth(grant).json(request).send(),
        )
        .await
        .map_err(|_| ProviderError::RequestFailed {
            message: "model gateway first response timed out".into(),
            retryable: false,
        })?
        .map_err(|error| ProviderError::RequestFailed {
            message: format!("model gateway request failed: {error}"),
            retryable: false,
        })?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::RequestFailed {
                message: format!(
                    "model gateway returned {status}: {}",
                    body.chars().take(500).collect::<String>()
                ),
                retryable: false,
            });
        }
        let mut stream = response.bytes_stream();
        let mut decoder = SseDecoder::default();
        let mut aggregate = OpenAiStreamAggregate::default();
        aggregate.metadata.backend =
            Some(format!("{:?}", self.binding.target.backend_kind).to_ascii_lowercase());
        loop {
            let next = timeout(
                Duration::from_secs(self.binding.target.transport.stream_idle_timeout_seconds),
                stream.next(),
            )
            .await
            .map_err(|_| ProviderError::RequestFailed {
                message: "model gateway stream became idle".into(),
                retryable: false,
            })?;
            let Some(next) = next else { break };
            let bytes = next.map_err(|error| ProviderError::RequestFailed {
                message: format!("model gateway stream failed: {error}"),
                retryable: false,
            })?;
            let text =
                std::str::from_utf8(&bytes).map_err(|error| ProviderError::MalformedResponse {
                    message: format!("model gateway stream was not UTF-8: {error}"),
                })?;
            for payload in decoder.push_str(text) {
                let chunk: StreamChunk = serde_json::from_str(&payload).map_err(|error| {
                    ProviderError::MalformedResponse {
                        message: format!("model gateway stream contained invalid JSON: {error}"),
                    }
                })?;
                aggregate.push_chunk(chunk);
            }
        }
        for payload in decoder.finish() {
            let chunk: StreamChunk = serde_json::from_str(&payload).map_err(|error| {
                ProviderError::MalformedResponse {
                    message: format!("model gateway stream contained invalid JSON: {error}"),
                }
            })?;
            aggregate.push_chunk(chunk);
        }
        Ok(aggregate)
    }
}

#[async_trait]
impl ModelProvider for GatewayModelClient {
    async fn complete_action(&self, request: ModelRequest) -> Result<ModelTurn, ProviderError> {
        let run_id = request.run_id.as_str().to_string();
        let mode = request.mode;
        let request = self.gateway_request(request);
        let value =
            serde_json::to_value(&request).map_err(|error| ProviderError::MalformedResponse {
                message: format!("model gateway request could not be serialized: {error}"),
            })?;
        let request_body_hash =
            canonical_json_sha256(&value).map_err(|error| ProviderError::MalformedResponse {
                message: format!("model gateway request could not be hashed: {error}"),
            })?;
        let sequence = self.next_request_sequence.fetch_add(1, Ordering::SeqCst);
        let grant = self
            .request_grant(&run_id, sequence, request_body_hash)
            .await?;
        if grant.expires_at_epoch_seconds == 0 || grant.token.is_empty() {
            return Err(ProviderError::MalformedResponse {
                message: "model-grant response is incomplete".into(),
            });
        }
        let aggregate = self.complete_gateway_stream(&value, &grant.token).await?;
        aggregate_to_model_turn(aggregate, mode)
    }

    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            native_tool_calling: self.binding.target.capabilities.native_tools,
            streaming: self.binding.target.capabilities.streaming,
            json_schema_response_format: self.binding.target.capabilities.json_schema,
        }
    }
}

fn internal_base_url(input: &str) -> Result<Url, OpenAiCompatibleError> {
    let mut url = Url::parse(input).map_err(|source| OpenAiCompatibleError::InvalidBaseUrl {
        input: input.to_string(),
        source,
    })?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(OpenAiCompatibleError::InvalidConfiguration(
            "internal model endpoint must be an HTTP(S) base URL without credentials, query, or fragment"
                .into(),
        ));
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::internal_base_url;
    use pharness_core::canonical_json_sha256;
    use serde::Serialize;

    #[test]
    fn internal_urls_reject_embedded_credentials() {
        assert!(internal_base_url("http://token@pharness-api:4777/").is_err());
        assert!(internal_base_url("http://pharness-api:4777/").is_ok());
    }

    #[test]
    fn hashed_gateway_value_is_the_transmitted_value() {
        #[derive(Serialize)]
        struct Request {
            temperature: f32,
        }

        let request = Request { temperature: 0.1 };
        let hashed_value = serde_json::to_value(&request).unwrap();
        let transmitted = serde_json::to_vec(&hashed_value).unwrap();
        let received: serde_json::Value = serde_json::from_slice(&transmitted).unwrap();

        assert_eq!(
            canonical_json_sha256(&received).unwrap(),
            canonical_json_sha256(&hashed_value).unwrap()
        );
        assert_ne!(
            canonical_json_sha256(
                &serde_json::from_slice(&serde_json::to_vec(&request).unwrap()).unwrap()
            )
            .unwrap(),
            canonical_json_sha256(&hashed_value).unwrap()
        );
    }
}
