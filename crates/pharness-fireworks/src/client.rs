use crate::{
    FireworksChatRequest, FireworksStreamAggregate, FireworksStreamChunk, SseDecoder,
    DEFAULT_FIREWORKS_BASE_URL,
};
use async_trait::async_trait;
use futures::StreamExt;
use pharness_core::{
    AgentAction, ModelCapabilities, ModelProvider, ModelRequest, ModelToolCall, ModelTurn,
    ProviderError, ToolProtocolMode,
};
use secrecy::{ExposeSecret, SecretString};
use thiserror::Error;
use tokio::time::{sleep, Duration};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FireworksProviderConfig {
    pub base_url: String,
    pub model: String,
}

impl FireworksProviderConfig {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            base_url: DEFAULT_FIREWORKS_BASE_URL.to_string(),
            model: model.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FireworksClient {
    http: reqwest::Client,
    api_key: SecretString,
    base_url: Url,
    model: String,
    retry_policy: RetryPolicy,
}

impl FireworksClient {
    pub fn new(
        api_key: SecretString,
        config: FireworksProviderConfig,
    ) -> Result<Self, FireworksClientError> {
        Ok(Self {
            http: reqwest::Client::new(),
            api_key,
            base_url: parse_base_url(&config.base_url)?,
            model: config.model,
            retry_policy: RetryPolicy::default(),
        })
    }

    pub fn chat_completions_url(&self) -> Url {
        self.base_url
            .join("chat/completions")
            .expect("normalized Fireworks base URL should accept relative endpoint")
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn build_request(&self, request: FireworksChatRequest) -> reqwest::RequestBuilder {
        self.http
            .post(self.chat_completions_url())
            .bearer_auth(self.api_key.expose_secret())
            .json(&request)
    }

    pub async fn complete_streaming(
        &self,
        request: FireworksChatRequest,
    ) -> Result<FireworksStreamAggregate, FireworksClientError> {
        let mut attempt = 1;
        let mut delay_ms = self.retry_policy.initial_delay_ms;

        loop {
            match self.complete_streaming_once(request.clone()).await {
                Ok(aggregate) => return Ok(aggregate),
                Err(error) if error.is_retryable() && attempt < self.retry_policy.max_attempts => {
                    sleep(Duration::from_millis(delay_ms)).await;
                    attempt += 1;
                    delay_ms = (delay_ms.saturating_mul(2)).min(self.retry_policy.max_delay_ms);
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn complete_streaming_once(
        &self,
        request: FireworksChatRequest,
    ) -> Result<FireworksStreamAggregate, FireworksClientError> {
        let response = self
            .build_request(request)
            .send()
            .await
            .map_err(FireworksClientError::Request)?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(FireworksClientError::Status {
                status,
                body,
                retryable: is_retryable_status(status),
            });
        }

        let mut byte_stream = response.bytes_stream();
        let mut decoder = SseDecoder::default();
        let mut aggregate = FireworksStreamAggregate::default();

        while let Some(next) = byte_stream.next().await {
            let bytes = next.map_err(FireworksClientError::Request)?;
            let text = std::str::from_utf8(&bytes).map_err(FireworksClientError::StreamUtf8)?;

            for payload in decoder.push_str(text) {
                let chunk: FireworksStreamChunk =
                    serde_json::from_str(&payload).map_err(FireworksClientError::StreamJson)?;
                aggregate.push_chunk(chunk);
            }
        }

        for payload in decoder.finish() {
            let chunk: FireworksStreamChunk =
                serde_json::from_str(&payload).map_err(FireworksClientError::StreamJson)?;
            aggregate.push_chunk(chunk);
        }

        Ok(aggregate)
    }
}

#[async_trait]
impl ModelProvider for FireworksClient {
    async fn complete_action(&self, request: ModelRequest) -> Result<ModelTurn, ProviderError> {
        let mode = request.mode;
        let fireworks_request = match mode {
            ToolProtocolMode::NativeTools => FireworksChatRequest::native_tools(
                self.model.clone(),
                request.messages,
                request.tools,
                request.temperature,
                request.max_tokens,
            ),
            ToolProtocolMode::JsonAction => FireworksChatRequest::json_action(
                self.model.clone(),
                request.messages,
                request.temperature,
                request.max_tokens,
            ),
        };

        let aggregate = self
            .complete_streaming(fireworks_request)
            .await
            .map_err(ProviderError::from)?;

        aggregate_to_model_turn(aggregate, mode)
    }

    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            native_tool_calling: true,
            streaming: true,
            json_schema_response_format: true,
        }
    }
}

fn parse_base_url(input: &str) -> Result<Url, FireworksClientError> {
    let mut url = Url::parse(input).map_err(|source| FireworksClientError::InvalidBaseUrl {
        input: input.to_string(),
        source,
    })?;

    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }

    Ok(url)
}

#[derive(Debug, Error)]
pub enum FireworksClientError {
    #[error("invalid Fireworks base URL {input:?}: {source}")]
    InvalidBaseUrl {
        input: String,
        source: url::ParseError,
    },
    #[error("Fireworks request failed: {0}")]
    Request(reqwest::Error),
    #[error("Fireworks returned HTTP {status}: {body}")]
    Status {
        status: reqwest::StatusCode,
        body: String,
        retryable: bool,
    },
    #[error("Fireworks stream chunk was not UTF-8: {0}")]
    StreamUtf8(std::str::Utf8Error),
    #[error("Fireworks stream payload was not JSON: {0}")]
    StreamJson(serde_json::Error),
    #[error("Fireworks stream did not contain a usable action")]
    MissingAction,
    #[error("Fireworks returned multiple tool calls, but Pharness V1 accepts one action per turn")]
    MultipleToolCalls,
    #[error("Fireworks returned a tool call without a function name")]
    MissingToolName,
    #[error("Fireworks returned invalid action payload: {0}")]
    InvalidAction(String),
}

impl FireworksClientError {
    fn is_retryable(&self) -> bool {
        match self {
            Self::Request(error) => error.is_timeout() || error.is_connect(),
            Self::Status { retryable, .. } => *retryable,
            _ => false,
        }
    }
}

impl From<FireworksClientError> for ProviderError {
    fn from(error: FireworksClientError) -> Self {
        match error {
            FireworksClientError::Request(error) => ProviderError::RequestFailed {
                message: error.to_string(),
                retryable: error.is_timeout() || error.is_connect(),
            },
            FireworksClientError::Status {
                status,
                body,
                retryable,
            } => ProviderError::RequestFailed {
                message: format!("HTTP {status}: {}", summarize_error_body(&body)),
                retryable,
            },
            FireworksClientError::InvalidBaseUrl { .. }
            | FireworksClientError::StreamUtf8(_)
            | FireworksClientError::StreamJson(_)
            | FireworksClientError::MissingAction
            | FireworksClientError::MultipleToolCalls
            | FireworksClientError::MissingToolName
            | FireworksClientError::InvalidAction(_) => ProviderError::MalformedResponse {
                message: error.to_string(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay_ms: 250,
            max_delay_ms: 2_000,
        }
    }
}

fn aggregate_to_model_turn(
    aggregate: FireworksStreamAggregate,
    mode: ToolProtocolMode,
) -> Result<ModelTurn, ProviderError> {
    let (action, assistant_tool_calls) = match aggregate.tool_calls.as_slice() {
        [tool_call, ..] => {
            let name = tool_call
                .name
                .as_deref()
                .ok_or(FireworksClientError::MissingToolName)?;
            let id = tool_call
                .id
                .clone()
                .unwrap_or_else(|| format!("tool_call_{}", tool_call.index));

            let action = AgentAction::from_tool_call(name, id.clone(), &tool_call.arguments)
                .map_err(|error| FireworksClientError::InvalidAction(error.to_string()))?;

            (
                action,
                vec![ModelToolCall {
                    id,
                    name: name.to_string(),
                    arguments: tool_call.arguments.clone(),
                }],
            )
        }
        [] => match mode {
            ToolProtocolMode::JsonAction => (
                AgentAction::from_json_text(&aggregate.content)
                    .map_err(|error| FireworksClientError::InvalidAction(error.to_string()))?,
                Vec::new(),
            ),
            ToolProtocolMode::NativeTools if !aggregate.content.trim().is_empty() => (
                AgentAction::provider_respond(
                    aggregate
                        .raw_provider_id
                        .clone()
                        .unwrap_or_else(|| "provider_response".to_string()),
                    aggregate.content.clone(),
                ),
                Vec::new(),
            ),
            ToolProtocolMode::NativeTools => return Err(FireworksClientError::MissingAction.into()),
        },
    };

    Ok(ModelTurn {
        raw_provider_id: aggregate.raw_provider_id,
        assistant_message: (!aggregate.content.is_empty()).then_some(aggregate.content),
        assistant_tool_calls,
        action,
        usage: None,
    })
}

fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::REQUEST_TIMEOUT
        || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

fn summarize_error_body(body: &str) -> String {
    #[derive(serde::Deserialize)]
    struct ErrorEnvelope {
        error: Option<ErrorPayload>,
    }

    #[derive(serde::Deserialize)]
    struct ErrorPayload {
        message: Option<String>,
        #[serde(rename = "type")]
        error_type: Option<String>,
        code: Option<String>,
    }

    if let Ok(envelope) = serde_json::from_str::<ErrorEnvelope>(body) {
        if let Some(error) = envelope.error {
            let mut parts = Vec::new();
            if let Some(code) = error.code {
                parts.push(code);
            }
            if let Some(error_type) = error.error_type {
                parts.push(error_type);
            }
            if let Some(message) = error.message {
                parts.push(message);
            }

            if !parts.is_empty() {
                return parts.join(": ");
            }
        }
    }

    const MAX_ERROR_BODY_CHARS: usize = 500;
    body.chars().take(MAX_ERROR_BODY_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        aggregate_to_model_turn, summarize_error_body, FireworksClient, FireworksClientError,
        FireworksProviderConfig,
    };
    use crate::{AccumulatedToolCall, FireworksStreamAggregate};
    use pharness_core::{AgentAction, ProviderError, ToolProtocolMode};
    use secrecy::SecretString;

    #[test]
    fn builds_chat_completions_url_from_default_base_url() {
        let client = FireworksClient::new(
            SecretString::new("test".to_string()),
            FireworksProviderConfig::new("accounts/fireworks/models/kimi-k2p5"),
        )
        .unwrap();

        assert_eq!(
            client.chat_completions_url().as_str(),
            "https://api.fireworks.ai/inference/v1/chat/completions"
        );
    }

    #[test]
    fn preserves_base_url_with_trailing_slash() {
        let client = FireworksClient::new(
            SecretString::new("test".to_string()),
            FireworksProviderConfig {
                base_url: "https://example.test/custom/v1/".to_string(),
                model: "model".to_string(),
            },
        )
        .unwrap();

        assert_eq!(
            client.chat_completions_url().as_str(),
            "https://example.test/custom/v1/chat/completions"
        );
    }

    #[test]
    fn maps_streamed_tool_call_to_model_turn() {
        let turn = aggregate_to_model_turn(
            FireworksStreamAggregate {
                raw_provider_id: Some("chatcmpl-test".to_string()),
                content: String::new(),
                tool_calls: vec![AccumulatedToolCall {
                    index: 0,
                    id: Some("call_read".to_string()),
                    tool_type: Some("function".to_string()),
                    name: Some("read_file".to_string()),
                    arguments:
                        r#"{"reason":"Inspect manifest","path":"Cargo.toml","max_bytes":2048}"#
                            .to_string(),
                }],
            },
            ToolProtocolMode::NativeTools,
        )
        .unwrap();

        match turn.action {
            AgentAction::ReadFile {
                id,
                path,
                max_bytes,
                ..
            } => {
                assert_eq!(id.as_str(), "call_read");
                assert_eq!(path.as_str(), "Cargo.toml");
                assert_eq!(max_bytes, Some(2048));
            }
            other => panic!("expected read_file action, got {other:?}"),
        }
        assert_eq!(turn.assistant_tool_calls.len(), 1);
        assert_eq!(turn.assistant_tool_calls[0].id, "call_read");
        assert_eq!(turn.assistant_tool_calls[0].name, "read_file");
    }

    #[test]
    fn maps_first_tool_call_when_provider_returns_parallel_calls() {
        let turn = aggregate_to_model_turn(
            FireworksStreamAggregate {
                raw_provider_id: Some("chatcmpl-test".to_string()),
                content: String::new(),
                tool_calls: vec![
                    AccumulatedToolCall {
                        index: 0,
                        id: Some("call_pipeline_runs".to_string()),
                        tool_type: Some("function".to_string()),
                        name: Some("tekton_get_pipeline_runs".to_string()),
                        arguments:
                            r#"{"reason":"Inspect PipelineRuns","namespace":null,"name":null,"all_namespaces":true,"label_selector":null}"#
                                .to_string(),
                    },
                    AccumulatedToolCall {
                        index: 1,
                        id: Some("call_task_runs".to_string()),
                        tool_type: Some("function".to_string()),
                        name: Some("tekton_get_task_runs".to_string()),
                        arguments:
                            r#"{"reason":"Inspect TaskRuns","namespace":null,"name":null,"all_namespaces":true,"label_selector":null}"#
                                .to_string(),
                    },
                ],
            },
            ToolProtocolMode::NativeTools,
        )
        .unwrap();

        assert_eq!(turn.assistant_tool_calls.len(), 1);
        assert_eq!(turn.assistant_tool_calls[0].id, "call_pipeline_runs");
        assert_eq!(turn.action.kind_name(), "tekton_get_pipeline_runs");
    }

    #[test]
    fn maps_json_action_content_to_model_turn() {
        let turn = aggregate_to_model_turn(
            FireworksStreamAggregate {
                raw_provider_id: Some("chatcmpl-test".to_string()),
                content:
                    r#"{"action":"finish","id":"act_done","reason":"Done","summary":"ok","success":true}"#
                        .to_string(),
                tool_calls: Vec::new(),
            },
            ToolProtocolMode::JsonAction,
        )
        .unwrap();

        match turn.action {
            AgentAction::Finish { success, .. } => assert!(success),
            other => panic!("expected finish action, got {other:?}"),
        }
        assert!(turn.assistant_tool_calls.is_empty());
    }

    #[test]
    fn maps_retryable_status_to_provider_error() {
        let error = ProviderError::from(FireworksClientError::Status {
            status: reqwest::StatusCode::TOO_MANY_REQUESTS,
            body:
                r#"{"error":{"code":"rate_limit","type":"rate_limit_error","message":"slow down"}}"#
                    .to_string(),
            retryable: true,
        });

        match error {
            ProviderError::RequestFailed { message, retryable } => {
                assert!(retryable);
                assert!(message.contains("rate_limit"));
                assert!(message.contains("slow down"));
            }
            other => panic!("expected request failure, got {other:?}"),
        }
    }

    #[test]
    fn summarizes_json_error_body() {
        let summary = summarize_error_body(
            r#"{"error":{"code":"bad_request","type":"invalid_request_error","message":"nope"}}"#,
        );

        assert_eq!(summary, "bad_request: invalid_request_error: nope");
    }
}
