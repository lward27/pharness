use super::characterization::test_state;
use crate::app::{inference, AppState};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use pharness_store::CreateInferenceTargetVerification;
use serde_json::{json, Value};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tower::ServiceExt;

async fn state() -> AppState {
    let mut state = test_state().await;
    let config = Arc::make_mut(&mut state.inference);
    config.enabled = false; // Never contact a provider from this integration test.
    config.registry = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../deploy/helm/pharness/files/inference-registry.json"
    )))
    .unwrap();
    config.registry.finalize_hashes().unwrap();
    state
}

async fn request(state: &AppState, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let response = inference::router()
        .with_state(state.clone())
        .oneshot(
            Request::builder()
                .method(if body.is_some() { "POST" } else { "GET" })
                .uri(path)
                .header("content-type", "application/json")
                .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn action(state: &AppState) -> Value {
    json!({"actor":"test","reason":"local protocol gate regression","config_hash":state.inference.registry.config_hash})
}

#[tokio::test]
async fn protocol_preflight_rejects_ambiguity_before_recording_and_records_explicit_policy_on_failure(
) {
    let state = state().await;
    let path = "/api/inference-targets/fireworks-kimi-k3/revisions/v1/preflight";
    assert_eq!(
        request(&state, path, Some(action(&state))).await.0,
        StatusCode::CONFLICT
    );
    assert!(state
        .store
        .list_inference_target_verifications("fireworks-kimi-k3", "v1")
        .await
        .unwrap()
        .is_empty());
    let mut body = action(&state);
    body["policy"] = json!({"policy_id":"planner-kimi-k3-v2","revision":"v1"});
    let (status, record) = request(&state, path, Some(body)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(record["status"], "failed");
    assert_eq!(record["sanitized_failure"], "model gateway is disabled");
    assert_eq!(
        record["observed_capabilities"]["policy"]["policy_id"],
        "planner-kimi-k3-v2"
    );
    assert_eq!(
        record["observed_capabilities"]["policy"]["policy_hash"],
        state
            .inference
            .registry
            .policy("planner-kimi-k3-v2", "v1")
            .unwrap()
            .policy_hash
    );
    let (_, policies) = request(&state, "/api/inference-policies", None).await;
    let planner = policies["policies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["policy_id"] == "planner-kimi-k3-v2")
        .unwrap();
    assert_eq!(planner["protocol_ready"], false);
    assert_eq!(planner["latest_protocol_verification"]["id"], record["id"]);
    let onboarding = policies["policies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["policy_id"] == "onboarding-kimi-k3-v2")
        .unwrap();
    assert!(onboarding["latest_protocol_verification"].is_null());
}

async fn seed_pass(state: &AppState, policy_id: &str, id: &str) {
    let policy = state.inference.registry.policy(policy_id, "v1").unwrap();
    let expiry = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 900;
    state.store.create_inference_target_verification(serde_json::from_value::<CreateInferenceTargetVerification>(json!({
        "id":id,"target_id":policy.target.target_id,"target_revision":policy.target.revision,
        "target_hash":policy.target_hash,"status":"passed","reachability":"reachable",
        "model_visible":true,"streaming_compatible":true,"tool_compatible":true,
        "observed_capabilities":{"registry_hash":state.inference.registry.config_hash,"runtime_revision":state.build.api_revision,
            "policy":{"policy_id":policy.policy_id,"revision":policy.revision,"policy_hash":policy.policy_hash},
            "protocol_calibration":{"passed":30,"required":30}},
        "sanitized_failure":null,"actor":"test","reason":"deterministic fixture only",
        "config_hash":state.inference.registry.config_hash,"expires_at":expiry.to_string()
    })).unwrap()).await.unwrap();
}

#[tokio::test]
async fn protocol_qualification_admission_and_readiness_use_the_exact_policy_receipt() {
    let state = state().await;
    let path = "/api/inference-policies/planner-kimi-k3-v2/revisions/v1/qualifications";
    let mut body = action(&state);
    body["attempts"] = json!(1);
    body["scope"] = json!({"kind":"diagnostic","case_ids":["acceptance-boundary"]});
    seed_pass(&state, "onboarding-kimi-k3-v2", "verify_onboarding").await;
    assert_eq!(
        request(&state, path, Some(body.clone())).await.0,
        StatusCode::CONFLICT
    );
    assert!(state
        .store
        .list_inference_evaluations("planner-kimi-k3-v2", "v1")
        .await
        .unwrap()
        .is_empty());
    seed_pass(&state, "planner-kimi-k3-v2", "verify_planner").await;
    let (_, policies) = request(&state, "/api/inference-policies", None).await;
    assert_eq!(
        policies["policies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["policy_id"] == "planner-kimi-k3-v2")
            .unwrap()["protocol_ready"],
        true
    );
    let (status, evaluation) = request(&state, path, Some(body)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(evaluation["policy_id"], "planner-kimi-k3-v2");
    assert_eq!(
        evaluation["status"], "failed",
        "disabled worker proves admission without dispatching external work"
    );
    assert_eq!(
        state
            .store
            .list_inference_evaluations("planner-kimi-k3-v2", "v1")
            .await
            .unwrap()
            .len(),
        1
    );
}
