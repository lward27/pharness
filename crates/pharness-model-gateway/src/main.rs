#![forbid(unsafe_code)]

use anyhow::Context;
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures::StreamExt;
use pharness_core::{
    canonical_json_sha256, verify_model_grant, InferenceBackendKind, InferenceRegistry,
    InferenceTargetRevision, ModelGrantClaims, StageInferencePolicyRevision,
};
use pharness_openai_compatible::ChatRequest;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{sync::Mutex, time::timeout};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialFiles(BTreeMap<String, PathBuf>);

#[derive(Clone)]
struct GatewayState {
    registry: Arc<InferenceRegistry>,
    signing_key: Arc<Vec<u8>>,
    credentials: Arc<BTreeMap<String, SecretString>>,
    clients: Arc<BTreeMap<(String, String), reqwest::Client>>,
    replayed_nonces: Arc<Mutex<BTreeMap<String, u64>>>,
}

#[derive(Debug)]
enum GatewayError {
    Unauthorized(&'static str),
    InvalidRequest(String),
    Unavailable(String),
    Upstream(StatusCode, String),
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::Unauthorized(message) => {
                (StatusCode::UNAUTHORIZED, "unauthorized", message.into())
            }
            Self::InvalidRequest(message) => (StatusCode::BAD_REQUEST, "invalid_request", message),
            Self::Unavailable(message) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "provider_unavailable",
                message,
            ),
            Self::Upstream(status, message) => (status, "upstream_error", message),
        };
        (
            status,
            Json(serde_json::json!({
                "error": {"type": code, "message": message}
            })),
        )
            .into_response()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing()?;
    let bind: SocketAddr = std::env::var("PHARNESS_MODEL_GATEWAY_BIND")
        .unwrap_or_else(|_| "0.0.0.0:4780".into())
        .parse()
        .context("PHARNESS_MODEL_GATEWAY_BIND must be a socket address")?;
    let registry_path = required_path("PHARNESS_INFERENCE_REGISTRY_FILE")?;
    let signing_key_path = required_path("PHARNESS_MODEL_GRANT_HMAC_FILE")?;
    let credential_files: CredentialFiles = std::env::var("PHARNESS_INFERENCE_CREDENTIAL_FILES")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .context("PHARNESS_INFERENCE_CREDENTIAL_FILES must be a JSON object")?
        .unwrap_or_default();

    let mut registry: InferenceRegistry = serde_json::from_slice(
        &tokio::fs::read(&registry_path)
            .await
            .with_context(|| format!("failed to read {}", registry_path.display()))?,
    )
    .context("failed to parse inference registry")?;
    registry.finalize_hashes()?;
    let signing_key = tokio::fs::read(&signing_key_path)
        .await
        .with_context(|| format!("failed to read {}", signing_key_path.display()))?;
    anyhow::ensure!(
        signing_key.len() >= 32,
        "model-grant signing key is too short"
    );
    let credentials = load_credentials(&registry, &credential_files.0).await?;
    let clients = build_clients(&registry)?;

    tracing::info!(registry_hash = %registry.config_hash, targets = registry.targets.len(), "model gateway configured");
    let state = GatewayState {
        registry: Arc::new(registry),
        signing_key: Arc::new(signing_key),
        credentials: Arc::new(credentials),
        clients: Arc::new(clients),
        replayed_nonces: Arc::new(Mutex::new(BTreeMap::new())),
    };
    let app = router(state).layer(TraceLayer::new_for_http());
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "pharness-model-gateway listening");
    axum::serve(listener, app).await?;
    Ok(())
}

fn router(state: GatewayState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(ready))
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat_completions))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn ready(State(state): State<GatewayState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status":"ready",
        "registry_hash":state.registry.config_hash,
        "targets":state.registry.targets.len()
    }))
}

async fn models(State(state): State<GatewayState>) -> Json<serde_json::Value> {
    let data = state
        .registry
        .targets
        .iter()
        .map(|target| {
            serde_json::json!({
                "id":format!("{}@{}", target.target_id, target.revision),
                "object":"model",
                "owned_by":"pharness",
                "selectable":target.selectable
            })
        })
        .collect::<Vec<_>>();
    Json(
        serde_json::json!({"object":"list","data":data,"registry_hash":state.registry.config_hash}),
    )
}

async fn chat_completions(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, GatewayError> {
    const MAX_REQUEST_BYTES: usize = 4 * 1024 * 1024;
    if body.len() > MAX_REQUEST_BYTES {
        return Err(GatewayError::InvalidRequest("request is too large".into()));
    }
    let token = bearer_token(&headers)?;
    let now = epoch_seconds();
    let claims = verify_model_grant(token, &state.signing_key, now)
        .map_err(|_| GatewayError::Unauthorized("invalid model grant"))?;
    let value: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|_| GatewayError::InvalidRequest("body must be valid JSON".into()))?;
    let request_hash = canonical_json_sha256(&value)
        .map_err(|_| GatewayError::InvalidRequest("request cannot be canonicalized".into()))?;
    if request_hash != claims.request_body_hash {
        return Err(GatewayError::Unauthorized(
            "model grant request hash mismatch",
        ));
    }
    claim_nonce(&state, &claims, now).await?;
    let mut request: ChatRequest = serde_json::from_value(value)
        .map_err(|error| GatewayError::InvalidRequest(format!("invalid chat request: {error}")))?;
    let (target, policy) = resolve_binding(&state.registry, &claims)?;
    validate_gateway_request(&request, target, policy, &claims)?;
    request.model = target.upstream_model.clone();
    apply_backend_policy(&mut request, target, policy)?;
    forward_request(&state, target, request).await
}

fn resolve_binding<'a>(
    registry: &'a InferenceRegistry,
    claims: &ModelGrantClaims,
) -> Result<
    (
        &'a InferenceTargetRevision,
        &'a StageInferencePolicyRevision,
    ),
    GatewayError,
> {
    let target = registry
        .targets
        .iter()
        .find(|target| {
            target.target_id == claims.target.target_id && target.revision == claims.target.revision
        })
        .ok_or(GatewayError::Unauthorized("unknown target revision"))?;
    if target.config_hash != claims.target_hash {
        return Err(GatewayError::Unauthorized("target hash mismatch"));
    }
    let policy = registry
        .policies
        .iter()
        .find(|policy| {
            policy.policy_id == claims.policy.policy_id && policy.revision == claims.policy.revision
        })
        .ok_or(GatewayError::Unauthorized("unknown policy revision"))?;
    if policy.policy_hash != claims.policy_hash || policy.target_hash != target.config_hash {
        return Err(GatewayError::Unauthorized("policy hash mismatch"));
    }
    Ok((target, policy))
}

fn validate_gateway_request(
    request: &ChatRequest,
    target: &InferenceTargetRevision,
    policy: &StageInferencePolicyRevision,
    claims: &ModelGrantClaims,
) -> Result<(), GatewayError> {
    if request.model != claims.target_alias() {
        return Err(GatewayError::Unauthorized("target alias mismatch"));
    }
    if request.max_tokens != policy.max_output_tokens || request.temperature != policy.temperature()
    {
        return Err(GatewayError::Unauthorized("generation policy mismatch"));
    }
    if !request.stream {
        return Err(GatewayError::InvalidRequest("streaming is required".into()));
    }
    if policy.tool_protocol == pharness_core::ToolProtocolMode::NativeTools
        && (request.tools.is_empty()
            || request.tool_choice
                != Some(match policy.tool_choice {
                    pharness_core::ToolChoiceMode::Auto => {
                        pharness_openai_compatible::ToolChoice::Auto
                    }
                    pharness_core::ToolChoiceMode::Required => {
                        pharness_openai_compatible::ToolChoice::Required
                    }
                    pharness_core::ToolChoiceMode::Specific => {
                        return Err(GatewayError::InvalidRequest(
                            "specific tool choice requires a named-tool policy revision".into(),
                        ));
                    }
                })
            || request.parallel_tool_calls != Some(false))
    {
        return Err(GatewayError::Unauthorized("native tool policy mismatch"));
    }
    if request.max_tokens > target.output_limit_tokens {
        return Err(GatewayError::InvalidRequest(
            "output limit exceeds target capability".into(),
        ));
    }
    Ok(())
}

fn apply_backend_policy(
    request: &mut ChatRequest,
    target: &InferenceTargetRevision,
    policy: &StageInferencePolicyRevision,
) -> Result<(), GatewayError> {
    let effort = policy
        .reasoning
        .effort
        .map(|effort| effort.as_str().to_string());
    match target.backend_kind {
        InferenceBackendKind::Openrouter => {
            let route = target.openrouter.as_ref().ok_or_else(|| {
                GatewayError::InvalidRequest("OpenRouter route is missing".into())
            })?;
            request.provider = Some(pharness_openai_compatible::OpenRouterProviderPreferences {
                order: vec![route.provider_slug.clone()],
                only: vec![route.provider_slug.clone()],
                allow_fallbacks: false,
                require_parameters: true,
                data_collection: "deny".into(),
            });
            request.reasoning_effort = None;
            request.reasoning_history = None;
            request.reasoning = Some(pharness_openai_compatible::ReasoningOptions {
                effort,
                context: match policy.reasoning.context_mode {
                    pharness_core::ReasoningContextMode::CurrentTurn => Some("current_turn".into()),
                    pharness_core::ReasoningContextMode::AllTurns => Some("all_turns".into()),
                    _ => None,
                },
                exclude: false,
            });
            for message in &mut request.messages {
                message.reasoning_content = None;
            }
        }
        InferenceBackendKind::Fireworks => {
            request.reasoning_effort = effort;
            request.reasoning_history = match policy.reasoning.context_mode {
                pharness_core::ReasoningContextMode::CurrentTurn => Some("interleaved".into()),
                pharness_core::ReasoningContextMode::AllTurns => Some("preserved".into()),
                _ => None,
            };
            request.reasoning = None;
            request.provider = None;
            for message in &mut request.messages {
                // Fireworks replays interleaved reasoning only through the
                // assistant message `reasoning_content` field. Strip the
                // OpenRouter variants even if a caller supplied them.
                message.reasoning = None;
                message.reasoning_details = None;
            }
        }
        InferenceBackendKind::LmStudio
        | InferenceBackendKind::LlamaCpp
        | InferenceBackendKind::OpenaiCompatible => {
            request.provider = None;
            request.reasoning = None;
            request.reasoning_history = None;
            request.reasoning_effort = effort;
            for message in &mut request.messages {
                message.reasoning = None;
            }
        }
    }
    Ok(())
}

async fn forward_request(
    state: &GatewayState,
    target: &InferenceTargetRevision,
    request: ChatRequest,
) -> Result<Response, GatewayError> {
    let key = (target.target_id.clone(), target.revision.clone());
    let client = state
        .clients
        .get(&key)
        .ok_or_else(|| GatewayError::Unavailable("target transport is unavailable".into()))?;
    let base = normalize_base_url(&target.upstream_base_url)
        .map_err(|error| GatewayError::Unavailable(error.to_string()))?;
    let url = base
        .join("chat/completions")
        .map_err(|error| GatewayError::Unavailable(error.to_string()))?;
    let mut attempt = 1;
    loop {
        let mut builder = client.post(url.clone()).json(&request);
        if let Some(binding) = &target.authentication_binding {
            let credential = state.credentials.get(binding).ok_or_else(|| {
                GatewayError::Unavailable("target credential is unavailable".into())
            })?;
            builder = builder.bearer_auth(credential.expose_secret());
        }
        let result = timeout(
            Duration::from_secs(target.transport.first_response_timeout_seconds),
            builder.send(),
        )
        .await;
        let response = match result {
            Ok(Ok(response)) => response,
            Ok(Err(error))
                if (error.is_connect() || error.is_timeout())
                    && attempt < target.transport.max_attempts =>
            {
                attempt += 1;
                continue;
            }
            Ok(Err(_)) => {
                return Err(GatewayError::Unavailable(
                    "upstream connection failed".into(),
                ))
            }
            Err(_) if attempt < target.transport.max_attempts => {
                attempt += 1;
                continue;
            }
            Err(_) => {
                return Err(GatewayError::Unavailable(
                    "upstream first response timed out".into(),
                ))
            }
        };
        let status = response.status();
        if !status.is_success() {
            let retryable = status == reqwest::StatusCode::REQUEST_TIMEOUT
                || status == reqwest::StatusCode::TOO_MANY_REQUESTS
                || status.is_server_error();
            if retryable && attempt < target.transport.max_attempts {
                attempt += 1;
                continue;
            }
            let body = response.text().await.unwrap_or_default();
            return Err(GatewayError::Upstream(
                status,
                sanitize_upstream_error(&body),
            ));
        }
        let idle_timeout = Duration::from_secs(target.transport.stream_idle_timeout_seconds);
        let upstream = Box::pin(response.bytes_stream());
        let stream = futures::stream::unfold(
            (upstream, false),
            move |(mut upstream, terminated)| async move {
                if terminated {
                    return None;
                }
                match timeout(idle_timeout, upstream.next()).await {
                    Ok(Some(chunk)) => {
                        Some((chunk.map_err(std::io::Error::other), (upstream, false)))
                    }
                    Ok(None) => None,
                    Err(_) => Some((
                        Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "upstream inference stream became idle",
                        )),
                        (upstream, true),
                    )),
                }
            },
        );
        let mut gateway_response = Response::new(Body::from_stream(stream));
        *gateway_response.status_mut() = StatusCode::OK;
        gateway_response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream"),
        );
        gateway_response.headers_mut().insert(
            "x-pharness-target",
            HeaderValue::from_str(&format!("{}@{}", target.target_id, target.revision))
                .map_err(|_| GatewayError::Unavailable("invalid target identity".into()))?,
        );
        return Ok(gateway_response);
    }
}

async fn claim_nonce(
    state: &GatewayState,
    claims: &ModelGrantClaims,
    now: u64,
) -> Result<(), GatewayError> {
    let mut replayed = state.replayed_nonces.lock().await;
    replayed.retain(|_, expiry| *expiry > now);
    if replayed.contains_key(&claims.nonce) {
        return Err(GatewayError::Unauthorized("model grant was already used"));
    }
    replayed.insert(claims.nonce.clone(), claims.expires_at_epoch_seconds);
    Ok(())
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, GatewayError> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty())
        .ok_or(GatewayError::Unauthorized("model grant is required"))
}

fn required_path(name: &str) -> anyhow::Result<PathBuf> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .with_context(|| format!("{name} is required"))
}

async fn load_credentials(
    registry: &InferenceRegistry,
    paths: &BTreeMap<String, PathBuf>,
) -> anyhow::Result<BTreeMap<String, SecretString>> {
    let required = registry
        .targets
        .iter()
        .filter(|target| target.selectable)
        .filter_map(|target| target.authentication_binding.clone())
        .collect::<BTreeSet<_>>();
    anyhow::ensure!(
        paths.keys().all(|binding| required.contains(binding)),
        "credential file mapping contains an unused binding"
    );
    let mut credentials = BTreeMap::new();
    for binding in required {
        let path = paths
            .get(&binding)
            .with_context(|| format!("credential binding {binding} has no mounted file"))?;
        let value = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("failed to read credential binding {binding}"))?;
        let value = value.trim().to_string();
        anyhow::ensure!(!value.is_empty(), "credential binding {binding} is empty");
        credentials.insert(binding, SecretString::new(value));
    }
    Ok(credentials)
}

fn build_clients(
    registry: &InferenceRegistry,
) -> anyhow::Result<BTreeMap<(String, String), reqwest::Client>> {
    registry
        .targets
        .iter()
        .map(|target| {
            let client = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(
                    target.transport.connect_timeout_seconds,
                ))
                .redirect(reqwest::redirect::Policy::none())
                .build()?;
            Ok(((target.target_id.clone(), target.revision.clone()), client))
        })
        .collect()
}

fn normalize_base_url(input: &str) -> anyhow::Result<url::Url> {
    let mut url = url::Url::parse(input)?;
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

fn sanitize_upstream_error(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        for pointer in ["/error/code", "/error/type"] {
            if let Some(code) = value.pointer(pointer).and_then(|value| value.as_str()) {
                let code = code
                    .chars()
                    .filter(|character| {
                        character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
                    })
                    .take(64)
                    .collect::<String>();
                if !code.is_empty() {
                    return format!("upstream rejected the request ({code})");
                }
            }
        }
    }
    "upstream rejected the request".into()
}

fn epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn init_tracing() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("pharness_model_gateway=info,tower_http=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .compact()
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to initialize tracing: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pharness_core::{
        InferenceCapabilities, InferencePolicyRef, InferenceStage, InferenceTargetRef,
        InferenceTransportPolicy, ModelGrantClaims, OpenRouterRoutePolicy, ReasoningContextMode,
        ReasoningEffort, ReasoningRequestPolicy, ToolProtocolMode, INFERENCE_POLICY_SCHEMA,
        INFERENCE_TARGET_SCHEMA, MODEL_GRANT_SCHEMA,
    };

    fn target() -> InferenceTargetRevision {
        let mut target = InferenceTargetRevision {
            schema_version: INFERENCE_TARGET_SCHEMA.into(),
            target_id: "openrouter-test".into(),
            revision: "v1".into(),
            display_name: "OpenRouter test".into(),
            backend_kind: InferenceBackendKind::Openrouter,
            protocol: "openai_chat_completions_v1".into(),
            upstream_base_url: "https://openrouter.ai/api/v1".into(),
            upstream_model: "model/name".into(),
            authentication_binding: Some("openrouter-token".into()),
            transport: InferenceTransportPolicy::default(),
            capabilities: InferenceCapabilities {
                native_tools: true,
                streaming: true,
                json_schema: true,
                stream_options: true,
                reasoning_efforts: vec![ReasoningEffort::High],
                reasoning_context_modes: vec![ReasoningContextMode::CurrentTurn],
                tool_choice_modes: vec![
                    pharness_core::ToolChoiceMode::Auto,
                    pharness_core::ToolChoiceMode::Required,
                ],
            },
            context_limit_tokens: 32_768,
            output_limit_tokens: 8_192,
            allowed_stages: vec![InferenceStage::Plan],
            selectable: false,
            openrouter: Some(OpenRouterRoutePolicy {
                provider_slug: "deepinfra/turbo".into(),
                require_parameters: true,
                allow_fallbacks: false,
                data_collection: "deny".into(),
            }),
            config_hash: String::new(),
        };
        target.config_hash = target.computed_hash().unwrap();
        target
    }

    fn policy(target: &InferenceTargetRevision) -> StageInferencePolicyRevision {
        let mut policy = StageInferencePolicyRevision {
            schema_version: INFERENCE_POLICY_SCHEMA.into(),
            policy_id: "planner-openrouter".into(),
            revision: "v1".into(),
            display_name: "Planner OpenRouter".into(),
            eligible_profiles: vec!["repo-planner".into()],
            eligible_stages: vec![InferenceStage::Plan],
            target: InferenceTargetRef {
                target_id: target.target_id.clone(),
                revision: target.revision.clone(),
            },
            target_hash: target.config_hash.clone(),
            reasoning: ReasoningRequestPolicy {
                effort: Some(ReasoningEffort::High),
                context_mode: ReasoningContextMode::CurrentTurn,
                expose_replay: true,
            },
            temperature_milli: Some(100),
            max_output_tokens: 8_192,
            max_input_tokens: 16_000,
            tool_protocol: ToolProtocolMode::NativeTools,
            tool_choice: pharness_core::ToolChoiceMode::Required,
            transport_max_attempts: 3,
            selectable: false,
            policy_hash: String::new(),
        };
        policy.policy_hash = policy.computed_hash().unwrap();
        policy
    }

    fn fireworks_target() -> InferenceTargetRevision {
        let mut target = target();
        target.target_id = "fireworks-test".into();
        target.display_name = "Fireworks test".into();
        target.backend_kind = InferenceBackendKind::Fireworks;
        target.upstream_base_url = "https://api.fireworks.ai/inference/v1".into();
        target.authentication_binding = Some("fireworks-token".into());
        target.openrouter = None;
        target.config_hash = String::new();
        target.config_hash = target.computed_hash().unwrap();
        target
    }

    #[test]
    fn backend_translation_overwrites_openrouter_routing() {
        let target = target();
        let policy = policy(&target);
        let mut request: ChatRequest = serde_json::from_value(serde_json::json!({
            "model":"openrouter-test@v1","messages":[],"tools":[{"type":"function","function":{"name":"finish","description":"finish","parameters":{"type":"object"}}}],"tool_choice":"required","parallel_tool_calls":false,"stream":true,"temperature":0.1,"max_tokens":8192
        })).unwrap();
        apply_backend_policy(&mut request, &target, &policy).unwrap();
        let provider = request.provider.unwrap();
        assert_eq!(provider.only, vec!["deepinfra/turbo"]);
        assert!(!provider.allow_fallbacks);
        assert!(provider.require_parameters);
    }

    #[test]
    fn backend_translation_strips_non_fireworks_reasoning_fields() {
        let target = fireworks_target();
        let policy = policy(&target);
        let mut request: ChatRequest = serde_json::from_value(serde_json::json!({
            "model":"fireworks-test@v1",
            "messages":[{
                "role":"assistant",
                "content":"",
                "reasoning_content":"keep this",
                "reasoning":"reject this",
                "reasoning_details":{"type":"reasoning.encrypted","data":"reject this too"}
            }],
            "tools":[{"type":"function","function":{"name":"finish","description":"finish","parameters":{"type":"object"}}}],
            "tool_choice":"required",
            "parallel_tool_calls":false,
            "stream":true,
            "temperature":0.1,
            "max_tokens":8192
        })).unwrap();
        apply_backend_policy(&mut request, &target, &policy).unwrap();
        let value = serde_json::to_value(request).unwrap();
        assert_eq!(value["messages"][0]["reasoning_content"], "keep this");
        assert!(value["messages"][0].get("reasoning").is_none());
        assert!(value["messages"][0].get("reasoning_details").is_none());
    }

    #[test]
    fn request_validation_binds_alias_and_generation_settings() {
        let target = target();
        let policy = policy(&target);
        let claims = ModelGrantClaims {
            schema_version: MODEL_GRANT_SCHEMA.into(),
            run_id: "run".into(),
            stage_execution_id: "stage".into(),
            selection_id: "selection".into(),
            target: InferenceTargetRef {
                target_id: target.target_id.clone(),
                revision: target.revision.clone(),
            },
            target_hash: target.config_hash.clone(),
            policy: InferencePolicyRef {
                policy_id: policy.policy_id.clone(),
                revision: policy.revision.clone(),
            },
            policy_hash: policy.policy_hash.clone(),
            request_sequence: 1,
            request_body_hash: "c".repeat(64),
            nonce: "nonce_nonce_nonce".into(),
            issued_at_epoch_seconds: 1,
            expires_at_epoch_seconds: 61,
        };
        let request: ChatRequest = serde_json::from_value(serde_json::json!({
            "model":"openrouter-test@v1","messages":[],"tools":[{"type":"function","function":{"name":"finish","description":"finish","parameters":{"type":"object"}}}],"tool_choice":"required","parallel_tool_calls":false,"stream":true,"temperature":0.1,"max_tokens":8192
        })).unwrap();
        validate_gateway_request(&request, &target, &policy, &claims).unwrap();
    }
}
