use crate::{build_chat_request, ChatRequest, OpenAiStreamAggregate, SseDecoder, StreamChunk};
use async_trait::async_trait;
use futures::StreamExt;
use pharness_core::{
    AgentAction, InferenceTargetRevision, ModelCapabilities, ModelProvider, ModelRequest,
    ModelToolCall, ModelTurn, ProviderError, ProviderProtocolErrorKind,
    StageInferencePolicyRevision, TokenUsage, ToolProtocolMode,
};
use secrecy::{ExposeSecret, SecretString};
use std::time::Instant;
use thiserror::Error;
use tokio::time::{sleep, timeout, Duration};
use url::Url;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenAiCompatibleTransportOptions {
    pub https_proxy_url: Option<String>,
    pub no_proxy: Option<String>,
}

impl OpenAiCompatibleTransportOptions {
    pub fn from_environment() -> Self {
        Self {
            https_proxy_url: first_nonempty_environment_value(&["HTTPS_PROXY", "https_proxy"]),
            no_proxy: first_nonempty_environment_value(&["NO_PROXY", "no_proxy"]),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleClient {
    http: reqwest::Client,
    credential: Option<SecretString>,
    target: InferenceTargetRevision,
    policy: StageInferencePolicyRevision,
    base_url: Url,
    retry_policy: RetryPolicy,
}

impl OpenAiCompatibleClient {
    pub fn new(
        target: InferenceTargetRevision,
        policy: StageInferencePolicyRevision,
        credential: Option<SecretString>,
    ) -> Result<Self, OpenAiCompatibleError> {
        Self::new_with_transport(
            target,
            policy,
            credential,
            OpenAiCompatibleTransportOptions::default(),
        )
    }

    pub fn new_with_transport(
        target: InferenceTargetRevision,
        policy: StageInferencePolicyRevision,
        credential: Option<SecretString>,
        transport_options: OpenAiCompatibleTransportOptions,
    ) -> Result<Self, OpenAiCompatibleError> {
        target
            .validate()
            .map_err(|error| OpenAiCompatibleError::InvalidConfiguration(error.to_string()))?;
        policy
            .validate_for_target(&target)
            .map_err(|error| OpenAiCompatibleError::InvalidConfiguration(error.to_string()))?;
        if target.authentication_binding.is_some() != credential.is_some() {
            return Err(OpenAiCompatibleError::CredentialBindingMismatch);
        }
        let retry_policy = RetryPolicy {
            max_attempts: policy.transport_max_attempts,
            ..RetryPolicy::default()
        };
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(
                target.transport.connect_timeout_seconds,
            ))
            .redirect(reqwest::redirect::Policy::none());
        if let Some(proxy_url) = transport_options.https_proxy_url.as_deref() {
            let proxy_url = parse_proxy_url(proxy_url)?;
            let mut proxy = reqwest::Proxy::https(proxy_url.as_str())
                .map_err(OpenAiCompatibleError::ProxyConfiguration)?;
            if let Some(no_proxy) = transport_options.no_proxy.as_deref() {
                proxy = proxy.no_proxy(reqwest::NoProxy::from_string(no_proxy));
            }
            builder = builder.proxy(proxy);
        }
        let http = builder
            .build()
            .map_err(OpenAiCompatibleError::ClientBuild)?;
        let base_url = parse_base_url(&target.upstream_base_url)?;
        Ok(Self {
            http,
            credential,
            target,
            policy,
            base_url,
            retry_policy,
        })
    }

    pub fn chat_completions_url(&self) -> Url {
        self.base_url
            .join("chat/completions")
            .expect("validated OpenAI-compatible base URL accepts relative endpoints")
    }

    pub fn models_url(&self) -> Url {
        self.base_url
            .join("models")
            .expect("validated OpenAI-compatible base URL accepts relative endpoint")
    }

    pub fn wire_request(&self, request: ModelRequest) -> ChatRequest {
        build_chat_request(
            self.target.backend_kind,
            self.target.upstream_model.clone(),
            request,
            &self.policy,
            self.target.capabilities.stream_options,
            self.target
                .openrouter
                .as_ref()
                .map(|route| route.provider_slug.as_str()),
        )
    }

    pub async fn list_models(&self) -> Result<Vec<String>, OpenAiCompatibleError> {
        let mut builder = self.http.get(self.models_url());
        if let Some(credential) = &self.credential {
            builder = builder.bearer_auth(credential.expose_secret());
        }
        let response = timeout(
            Duration::from_secs(self.target.transport.first_response_timeout_seconds),
            builder.send(),
        )
        .await
        .map_err(|_| OpenAiCompatibleError::FirstResponseTimeout)?
        .map_err(OpenAiCompatibleError::Request)?;
        if !response.status().is_success() {
            return Err(status_error(response).await);
        }
        let value: serde_json::Value = response
            .json()
            .await
            .map_err(OpenAiCompatibleError::Request)?;
        Ok(value["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|model| model["id"].as_str().map(ToOwned::to_owned))
            .collect())
    }

    pub async fn complete_streaming(
        &self,
        request: ChatRequest,
    ) -> Result<OpenAiStreamAggregate, OpenAiCompatibleError> {
        let mut attempt = 1;
        let mut delay_ms = self.retry_policy.initial_delay_ms;
        loop {
            match self.complete_streaming_once(request.clone()).await {
                Ok(aggregate) => return Ok(aggregate),
                Err(error) if error.is_retryable() && attempt < self.retry_policy.max_attempts => {
                    sleep(Duration::from_millis(delay_ms)).await;
                    attempt += 1;
                    delay_ms = delay_ms
                        .saturating_mul(2)
                        .min(self.retry_policy.max_delay_ms);
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn complete_streaming_once(
        &self,
        request: ChatRequest,
    ) -> Result<OpenAiStreamAggregate, OpenAiCompatibleError> {
        let mut builder = self.http.post(self.chat_completions_url()).json(&request);
        if let Some(credential) = &self.credential {
            builder = builder.bearer_auth(credential.expose_secret());
        }
        let started = Instant::now();
        let response = timeout(
            Duration::from_secs(self.target.transport.first_response_timeout_seconds),
            builder.send(),
        )
        .await
        .map_err(|_| OpenAiCompatibleError::FirstResponseTimeout)?
        .map_err(OpenAiCompatibleError::Request)?;
        if !response.status().is_success() {
            return Err(status_error(response).await);
        }

        let mut byte_stream = response.bytes_stream();
        let mut decoder = SseDecoder::default();
        let mut aggregate = OpenAiStreamAggregate::default();
        aggregate.metadata.backend = Some(format!("{:?}", self.target.backend_kind).to_lowercase());

        loop {
            let next = timeout(
                Duration::from_secs(self.target.transport.stream_idle_timeout_seconds),
                byte_stream.next(),
            )
            .await
            .map_err(|_| OpenAiCompatibleError::StreamIdleTimeout {
                after_usable_data: aggregate.usable_stream_data,
            })?;
            let Some(next) = next else { break };
            let bytes = next.map_err(|source| OpenAiCompatibleError::StreamRequest {
                source,
                after_usable_data: aggregate.usable_stream_data,
            })?;
            let text = std::str::from_utf8(&bytes).map_err(OpenAiCompatibleError::StreamUtf8)?;
            for payload in decoder.push_str(text) {
                let chunk: StreamChunk =
                    serde_json::from_str(&payload).map_err(OpenAiCompatibleError::StreamJson)?;
                aggregate.push_chunk(chunk);
            }
        }
        for payload in decoder.finish() {
            let chunk: StreamChunk =
                serde_json::from_str(&payload).map_err(OpenAiCompatibleError::StreamJson)?;
            aggregate.push_chunk(chunk);
        }
        aggregate.metadata.latency_ms =
            started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        Ok(aggregate)
    }
}

#[async_trait]
impl ModelProvider for OpenAiCompatibleClient {
    async fn complete_action(&self, request: ModelRequest) -> Result<ModelTurn, ProviderError> {
        let mode = request.mode;
        let aggregate = self
            .complete_streaming(self.wire_request(request))
            .await
            .map_err(ProviderError::from)?;
        aggregate_to_model_turn(aggregate, mode)
    }

    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            native_tool_calling: self.target.capabilities.native_tools,
            streaming: self.target.capabilities.streaming,
            json_schema_response_format: self.target.capabilities.json_schema,
        }
    }
}

pub fn aggregate_to_model_turn(
    aggregate: OpenAiStreamAggregate,
    mode: ToolProtocolMode,
) -> Result<ModelTurn, ProviderError> {
    if aggregate.tool_calls.len() > 1 {
        return Err(OpenAiCompatibleError::MultipleToolCalls.into());
    }
    let (action, assistant_tool_calls) = match aggregate.tool_calls.as_slice() {
        [tool_call] => {
            let name = tool_call
                .name
                .as_deref()
                .ok_or(OpenAiCompatibleError::MissingToolName)?;
            let id = tool_call
                .id
                .clone()
                .unwrap_or_else(|| format!("tool_call_{}", tool_call.index));
            let action = AgentAction::from_tool_call(name, id.clone(), &tool_call.arguments)
                .map_err(|error| OpenAiCompatibleError::InvalidAction(error.to_string()))?;
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
                    .map_err(|error| OpenAiCompatibleError::InvalidAction(error.to_string()))?,
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
            ToolProtocolMode::NativeTools => {
                return Err(OpenAiCompatibleError::MissingAction.into())
            }
        },
        _ => unreachable!("multiple tool calls were rejected above"),
    };
    let reasoning = aggregate.reasoning_replay();
    let usage = aggregate.usage.map(|usage| TokenUsage {
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
        reasoning_tokens: usage
            .completion_tokens_details
            .map(|details| details.reasoning_tokens)
            .unwrap_or_default(),
        cached_tokens: usage
            .prompt_tokens_details
            .map(|details| details.cached_tokens)
            .unwrap_or_default(),
        cost_microusd: usage
            .cost
            .filter(|cost| cost.is_finite() && *cost >= 0.0)
            .map(|cost| (cost * 1_000_000.0).round() as u64),
    });
    Ok(ModelTurn {
        raw_provider_id: aggregate.raw_provider_id,
        assistant_message: (!aggregate.content.is_empty()).then_some(aggregate.content),
        assistant_tool_calls,
        reasoning,
        action,
        usage,
        metadata: Some(aggregate.metadata),
    })
}

fn parse_base_url(input: &str) -> Result<Url, OpenAiCompatibleError> {
    let mut url = Url::parse(input).map_err(|source| OpenAiCompatibleError::InvalidBaseUrl {
        input: input.to_string(),
        source,
    })?;
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

fn parse_proxy_url(input: &str) -> Result<Url, OpenAiCompatibleError> {
    let url = Url::parse(input).map_err(|_| OpenAiCompatibleError::InvalidProxyConfiguration)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(OpenAiCompatibleError::InvalidProxyConfiguration);
    }
    Ok(url)
}

fn first_nonempty_environment_value(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    })
}

async fn status_error(response: reqwest::Response) -> OpenAiCompatibleError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    OpenAiCompatibleError::Status {
        status,
        body: summarize_error_body(&body),
        retryable: is_retryable_status(status),
    }
}

#[derive(Debug, Error)]
pub enum OpenAiCompatibleError {
    #[error("invalid OpenAI-compatible configuration: {0}")]
    InvalidConfiguration(String),
    #[error("credential presence does not match the target authentication binding")]
    CredentialBindingMismatch,
    #[error("failed to build OpenAI-compatible HTTP client: {0}")]
    ClientBuild(reqwest::Error),
    #[error("OpenAI-compatible proxy URL must be an HTTP(S) origin without credentials, path, query, or fragment")]
    InvalidProxyConfiguration,
    #[error("failed to configure the OpenAI-compatible HTTPS proxy: {0}")]
    ProxyConfiguration(reqwest::Error),
    #[error("invalid OpenAI-compatible base URL {input:?}: {source}")]
    InvalidBaseUrl {
        input: String,
        source: url::ParseError,
    },
    #[error("OpenAI-compatible request failed: {0}")]
    Request(reqwest::Error),
    #[error("OpenAI-compatible first response timed out")]
    FirstResponseTimeout,
    #[error("OpenAI-compatible stream was idle after usable data={after_usable_data}")]
    StreamIdleTimeout { after_usable_data: bool },
    #[error("OpenAI-compatible stream failed after usable data={after_usable_data}: {source}")]
    StreamRequest {
        source: reqwest::Error,
        after_usable_data: bool,
    },
    #[error("OpenAI-compatible upstream returned HTTP {status}: {body}")]
    Status {
        status: reqwest::StatusCode,
        body: String,
        retryable: bool,
    },
    #[error("OpenAI-compatible stream chunk was not UTF-8: {0}")]
    StreamUtf8(std::str::Utf8Error),
    #[error("OpenAI-compatible stream payload was not JSON: {0}")]
    StreamJson(serde_json::Error),
    #[error("OpenAI-compatible stream did not contain a usable action")]
    MissingAction,
    #[error("OpenAI-compatible provider returned multiple tool calls; PHarness accepts one action per turn")]
    MultipleToolCalls,
    #[error("OpenAI-compatible provider returned a tool call without a function name")]
    MissingToolName,
    #[error("OpenAI-compatible provider returned invalid action payload: {0}")]
    InvalidAction(String),
}

impl OpenAiCompatibleError {
    fn is_retryable(&self) -> bool {
        match self {
            Self::Request(error) => error.is_timeout() || error.is_connect(),
            Self::FirstResponseTimeout => true,
            Self::StreamIdleTimeout {
                after_usable_data: false,
            }
            | Self::StreamRequest {
                after_usable_data: false,
                ..
            } => true,
            Self::Status { retryable, .. } => *retryable,
            _ => false,
        }
    }
}

impl From<OpenAiCompatibleError> for ProviderError {
    fn from(error: OpenAiCompatibleError) -> Self {
        let retryable = error.is_retryable();
        match error {
            OpenAiCompatibleError::Request(_)
            | OpenAiCompatibleError::FirstResponseTimeout
            | OpenAiCompatibleError::StreamIdleTimeout { .. }
            | OpenAiCompatibleError::StreamRequest { .. }
            | OpenAiCompatibleError::Status { .. } => ProviderError::RequestFailed {
                message: error.to_string(),
                retryable,
            },
            OpenAiCompatibleError::MissingAction => ProviderError::Protocol {
                category: ProviderProtocolErrorKind::MissingAction,
                message: error.to_string(),
            },
            OpenAiCompatibleError::MultipleToolCalls => ProviderError::Protocol {
                category: ProviderProtocolErrorKind::MultipleActions,
                message: error.to_string(),
            },
            OpenAiCompatibleError::MissingToolName | OpenAiCompatibleError::InvalidAction(_) => {
                ProviderError::Protocol {
                    category: ProviderProtocolErrorKind::MalformedArguments,
                    message: error.to_string(),
                }
            }
            _ => ProviderError::MalformedResponse {
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

fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::REQUEST_TIMEOUT
        || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

fn summarize_error_body(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(error) = value.get("error") {
            let parts = ["code", "type", "message"]
                .into_iter()
                .filter_map(|field| error.get(field))
                .filter_map(|value| value.as_str())
                .collect::<Vec<_>>();
            if !parts.is_empty() {
                return parts.join(": ");
            }
        }
    }
    body.chars().take(500).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccumulatedToolCall, OpenAiStreamAggregate};
    use pharness_core::{
        InferenceBackendKind, InferenceCapabilities, InferenceStage, InferenceTargetRef,
        InferenceTransportPolicy, ReasoningRequestPolicy, StageInferencePolicyRevision,
        ToolProtocolMode, INFERENCE_POLICY_SCHEMA, INFERENCE_TARGET_SCHEMA,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn test_target() -> InferenceTargetRevision {
        let mut target = InferenceTargetRevision {
            schema_version: INFERENCE_TARGET_SCHEMA.into(),
            target_id: "test-target".into(),
            revision: "v1".into(),
            display_name: "Test target".into(),
            backend_kind: InferenceBackendKind::OpenaiCompatible,
            protocol: "openai_chat_completions_v1".into(),
            upstream_base_url: "https://models.example.test/v1".into(),
            upstream_model: "test-model".into(),
            authentication_binding: None,
            transport: InferenceTransportPolicy {
                connect_timeout_seconds: 1,
                first_response_timeout_seconds: 1,
                stream_idle_timeout_seconds: 1,
                max_attempts: 1,
                allow_insecure_private_http: false,
                private_cidr: None,
                private_port: None,
            },
            capabilities: InferenceCapabilities {
                native_tools: true,
                streaming: true,
                json_schema: true,
                stream_options: true,
                reasoning_efforts: Vec::new(),
                reasoning_context_modes: Vec::new(),
                tool_choice_modes: vec![
                    pharness_core::ToolChoiceMode::Auto,
                    pharness_core::ToolChoiceMode::Required,
                ],
            },
            context_limit_tokens: 32_768,
            output_limit_tokens: 8_192,
            allowed_stages: vec![InferenceStage::Implement],
            selectable: true,
            openrouter: None,
            config_hash: String::new(),
        };
        target.config_hash = target.computed_hash().unwrap();
        target
    }

    fn test_policy(target: &InferenceTargetRevision) -> StageInferencePolicyRevision {
        let mut policy = StageInferencePolicyRevision {
            schema_version: INFERENCE_POLICY_SCHEMA.into(),
            policy_id: "test-policy".into(),
            revision: "v1".into(),
            display_name: "Test policy".into(),
            eligible_profiles: vec!["repo-builder".into()],
            eligible_stages: vec![InferenceStage::Implement],
            target: InferenceTargetRef {
                target_id: target.target_id.clone(),
                revision: target.revision.clone(),
            },
            target_hash: target.config_hash.clone(),
            reasoning: ReasoningRequestPolicy::default(),
            temperature_milli: Some(100),
            max_output_tokens: 4_096,
            max_input_tokens: 16_000,
            tool_protocol: ToolProtocolMode::NativeTools,
            tool_choice: pharness_core::ToolChoiceMode::Required,
            transport_max_attempts: 1,
            selectable: true,
            policy_hash: String::new(),
        };
        policy.policy_hash = policy.computed_hash().unwrap();
        policy
    }

    #[test]
    fn malformed_current_actions_and_history_envelopes_remain_non_executable() {
        for arguments in [
            "{",
            "[]",
            r#"{"_pharness_protocol_error":{"kind":"invalid_tool_arguments","execution":"rejected","raw_arguments":"{"}}"#,
        ] {
            let aggregate = OpenAiStreamAggregate {
                tool_calls: vec![AccumulatedToolCall {
                    index: 0,
                    id: Some("invalid-call".into()),
                    name: Some("read_file".into()),
                    arguments: arguments.into(),
                    ..AccumulatedToolCall::default()
                }],
                ..OpenAiStreamAggregate::default()
            };
            let error =
                aggregate_to_model_turn(aggregate, ToolProtocolMode::NativeTools).unwrap_err();
            assert!(matches!(error, ProviderError::Protocol { .. }));
        }
    }

    #[test]
    fn rejects_multiple_actions_in_one_turn() {
        let aggregate = OpenAiStreamAggregate {
            tool_calls: vec![
                AccumulatedToolCall {
                    index: 0,
                    id: Some("one".into()),
                    name: Some("finish".into()),
                    arguments: "{}".into(),
                    ..AccumulatedToolCall::default()
                },
                AccumulatedToolCall {
                    index: 1,
                    id: Some("two".into()),
                    name: Some("finish".into()),
                    arguments: "{}".into(),
                    ..AccumulatedToolCall::default()
                },
            ],
            ..OpenAiStreamAggregate::default()
        };
        let error = aggregate_to_model_turn(aggregate, ToolProtocolMode::NativeTools).unwrap_err();
        assert!(matches!(
            error,
            ProviderError::Protocol {
                category: ProviderProtocolErrorKind::MultipleActions,
                ..
            }
        ));
    }

    #[test]
    fn rejects_proxy_urls_with_credentials_or_paths() {
        assert!(parse_proxy_url("http://user:pass@proxy.example.test:8080").is_err());
        assert!(parse_proxy_url("http://proxy.example.test:8080/connect").is_err());
        assert!(parse_proxy_url("http://proxy.example.test:8080").is_ok());
    }

    #[tokio::test]
    async fn routes_https_requests_through_the_explicit_connect_proxy() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let proxy = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0_u8; 2_048];
            let bytes = stream.read(&mut request).await.unwrap();
            stream
                .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
            String::from_utf8_lossy(&request[..bytes]).into_owned()
        });
        let target = test_target();
        let policy = test_policy(&target);
        let client = OpenAiCompatibleClient::new_with_transport(
            target,
            policy,
            None,
            OpenAiCompatibleTransportOptions {
                https_proxy_url: Some(format!("http://{address}")),
                no_proxy: None,
            },
        )
        .unwrap();

        assert!(client.list_models().await.is_err());
        let request = proxy.await.unwrap();
        assert!(request.starts_with("CONNECT models.example.test:443 HTTP/1.1\r\n"));
    }
}
