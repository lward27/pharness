//! Local HTTP fixtures exercise the gateway, not a real model qualification.
use super::*;
use pharness_core::{
    sign_model_grant, InferencePolicyRef, InferenceTargetRef, ReasoningRequestPolicy,
    INFERENCE_REGISTRY_SCHEMA, MODEL_GRANT_SCHEMA,
};
use serde_json::{json, Value};

struct Server(tokio::task::JoinHandle<()>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn serve(app: Router) -> (String, Server) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{address}/v1"), Server(task))
}

fn fixture(base: String, attempts: u32) -> (GatewayState, Value, HeaderMap) {
    let mut target = super::tests::target();
    target.target_id = "minisforum-test".into();
    target.backend_kind = InferenceBackendKind::LmStudio;
    target.upstream_base_url = base;
    target.upstream_model = "pharness-qwen-local".into();
    target.authentication_binding = Some("lm-studio-api-key".into());
    target.openrouter = None;
    target.capabilities.stream_options = false;
    target.capabilities.reasoning_efforts.clear();
    target.capabilities.reasoning_context_modes.clear();
    target.transport.stream_idle_timeout_seconds = 1;
    target.config_hash = target.computed_hash().unwrap();
    let mut policy = super::tests::policy(&target);
    policy.reasoning = ReasoningRequestPolicy::default();
    policy.transport_max_attempts = attempts;
    policy.policy_hash = policy.computed_hash().unwrap();
    let request = json!({
        "model":"minisforum-test@v1", "messages":[
            {"role":"system","content":"Bounded test instructions"},
            {"role":"assistant","content":"", "reasoning_details":[{"type":"reasoning.encrypted","data":"provider-specific"}]},
            {"role":"user","content":"Return one tool call"}
        ],
        "tools":[{"type":"function","function":{"name":"finish","description":"finish","parameters":{"type":"object"}}}],
        "tool_choice":"required","parallel_tool_calls":false,"stream":true,
        "stream_options":{"include_usage":true},"temperature":0.1,"max_tokens":8192
    });
    let key = vec![7_u8; 32];
    let claims = ModelGrantClaims {
        schema_version: MODEL_GRANT_SCHEMA.into(),
        run_id: "run_local".into(),
        stage_execution_id: "stage_local".into(),
        selection_id: "selection_local".into(),
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
        request_body_hash: canonical_json_sha256(&request).unwrap(),
        nonce: "local_test_nonce_0001".into(),
        issued_at_epoch_seconds: epoch_seconds(),
        expires_at_epoch_seconds: epoch_seconds() + 60,
    };
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!(
            "Bearer {}",
            sign_model_grant(&claims, &key).unwrap()
        ))
        .unwrap(),
    );
    // Loopback is only a test fixture. Production registry startup still rejects
    // loopback and requires the exact private IP/port opt-in for plain HTTP.
    let clients = BTreeMap::from([(
        (target.target_id.clone(), target.revision.clone()),
        reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap(),
    )]);
    let state = GatewayState {
        registry: Arc::new(InferenceRegistry {
            schema_version: INFERENCE_REGISTRY_SCHEMA.into(),
            targets: vec![target],
            policies: vec![policy],
            defaults: BTreeMap::new(),
            config_hash: String::new(),
        }),
        signing_key: Arc::new(key),
        credentials: Arc::new(BTreeMap::from([(
            "lm-studio-api-key".into(),
            SecretString::new("test-local-credential".into()),
        )])),
        clients: Arc::new(clients),
        replayed_nonces: Arc::new(Mutex::new(BTreeMap::new())),
    };
    (state, request, headers)
}

async fn call(state: GatewayState, request: &Value, headers: HeaderMap) -> Response {
    match chat_completions(
        State(state),
        headers,
        Bytes::from(serde_json::to_vec(request).unwrap()),
    )
    .await
    {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

#[tokio::test]
async fn local_auth_alias_tools_and_stream_survive_the_gateway() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let capture = captured.clone();
    let payload=concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"finish\",\"arguments\":\"{\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let (base, _server) = serve(Router::new().route(
        "/v1/chat/completions",
        post(move |headers: HeaderMap, Json(body): Json<Value>| {
            let capture = capture.clone();
            async move {
                capture.lock().await.push((headers, body));
                let chunks = vec![
                    Ok::<_, std::io::Error>(Bytes::from_static(&payload.as_bytes()[..41])),
                    Ok(Bytes::from_static(&payload.as_bytes()[41..])),
                ];
                (
                    [(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")],
                    Body::from_stream(futures::stream::iter(chunks)),
                )
            }
        }),
    ))
    .await;
    let (state, request, headers) = fixture(base, 1);
    let response = call(state.clone(), &request, headers.clone()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["x-pharness-target"],
        "minisforum-test@v1"
    );
    let bytes = axum::body::to_bytes(response.into_body(), 16384)
        .await
        .unwrap();
    assert_eq!(bytes, payload);
    let mut decoder = pharness_openai_compatible::SseDecoder::default();
    let mut aggregate = pharness_openai_compatible::OpenAiStreamAggregate::default();
    for frame in decoder.push_str(std::str::from_utf8(&bytes).unwrap()) {
        if frame != "[DONE]" {
            aggregate.push_chunk(serde_json::from_str(&frame).unwrap());
        }
    }
    assert_eq!(aggregate.tool_calls.len(), 1);
    let requests = captured.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].0[header::AUTHORIZATION],
        "Bearer test-local-credential"
    );
    assert_eq!(requests[0].1["model"], "pharness-qwen-local");
    assert_eq!(requests[0].1["tool_choice"], "required");
    assert_eq!(requests[0].1["parallel_tool_calls"], false);
    assert_eq!(requests[0].1["messages"][0], request["messages"][0]);
    assert!(requests[0].1.get("stream_options").is_none());
    assert!(requests[0].1["messages"][1]
        .get("reasoning_details")
        .is_none());
    drop(requests);
    assert_eq!(
        call(state.clone(), &request, headers.clone())
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        captured.lock().await.len(),
        1,
        "replayed grants must not dispatch"
    );
    let mut followup = request.clone();
    followup["messages"].as_array_mut().unwrap().extend([
        json!({"role":"assistant","content":"","tool_calls":[{"id":"call_1","type":"function","function":{"name":"finish","arguments":"{}"}}]}),
        json!({"role":"tool","tool_call_id":"call_1","content":"{\"accepted\":true}"}),
    ]);
    let mut claims = verify_model_grant(
        bearer_token(&headers).unwrap(),
        &state.signing_key,
        epoch_seconds(),
    )
    .unwrap();
    claims.request_sequence = 2;
    claims.nonce = "local_test_nonce_0002".into();
    claims.request_body_hash = canonical_json_sha256(&followup).unwrap();
    let mut next_headers = HeaderMap::new();
    next_headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!(
            "Bearer {}",
            sign_model_grant(&claims, &state.signing_key).unwrap()
        ))
        .unwrap(),
    );
    let response = call(state, &followup, next_headers).await;
    assert_eq!(response.status(), StatusCode::OK);
    axum::body::to_bytes(response.into_body(), 16384)
        .await
        .unwrap();
    let requests = captured.lock().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].1["messages"][3], followup["messages"][3]);
    assert_eq!(requests[1].1["messages"][4], followup["messages"][4]);
}

#[tokio::test]
async fn retry_count_obeys_the_stage_policy_and_errors_do_not_expose_upstream_text() {
    use std::sync::atomic::{AtomicU32, Ordering};
    let count = Arc::new(AtomicU32::new(0));
    let captured = count.clone();
    let (base, _server) = serve(Router::new().route(
        "/v1/chat/completions",
        post(move || {
            captured.fetch_add(1, Ordering::SeqCst);
            async {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error":{"message":"private prompt and credential"}})),
                )
            }
        }),
    ))
    .await;
    for attempts in [1, 2, 3] {
        count.store(0, Ordering::SeqCst);
        let (state, request, headers) = fixture(base.clone(), attempts);
        let response = call(state, &request, headers).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(count.load(Ordering::SeqCst), attempts);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert!(!String::from_utf8_lossy(&body).contains("private prompt"));
    }
}

#[tokio::test]
async fn successful_non_stream_response_is_rejected() {
    let (base, _server) = serve(Router::new().route(
        "/v1/chat/completions",
        post(|| async { Json(json!({"message":"model unloaded"})) }),
    ))
    .await;
    let (state, request, headers) = fixture(base, 1);
    assert_eq!(
        call(state, &request, headers).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn stalled_error_body_has_a_deadline() {
    let (base, _server) = serve(Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            (
                StatusCode::BAD_REQUEST,
                Body::from_stream(futures::stream::pending::<Result<Bytes, std::io::Error>>()),
            )
        }),
    ))
    .await;
    let (state, request, headers) = fixture(base, 1);
    let response = timeout(Duration::from_secs(3), call(state, &request, headers))
        .await
        .expect("error body must not hang the gateway");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn stalled_success_stream_terminates_without_retry() {
    let (base, _server) = serve(Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            (
                [(header::CONTENT_TYPE, "text/event-stream")],
                Body::from_stream(futures::stream::pending::<Result<Bytes, std::io::Error>>()),
            )
        }),
    ))
    .await;
    let (state, request, headers) = fixture(base, 3);
    let response = call(state, &request, headers).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(timeout(
        Duration::from_secs(3),
        axum::body::to_bytes(response.into_body(), 4096)
    )
    .await
    .unwrap()
    .is_err());
}

#[tokio::test]
async fn missing_local_credential_fails_before_upstream_dispatch() {
    let (mut state, request, headers) = fixture("http://127.0.0.1:1/v1".into(), 1);
    state.credentials = Arc::new(BTreeMap::new());
    assert_eq!(
        call(state, &request, headers).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}
