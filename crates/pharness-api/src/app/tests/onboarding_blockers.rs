use super::repo_mode_v1::repo_fixture_with_workflow;
use axum::{
    body::{to_bytes, Body},
    http::Request,
};
use serde_json::{json, Value};
use tower::ServiceExt;

async fn request(app: &axum::Router, method: &str, path: &str, body: Value) -> (u16, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(if method == "GET" {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn blocked_onboarding_round_trips_without_approval_or_implicit_progress() {
    let fixture = repo_fixture_with_workflow("onboarding_blockers", false, false).await;
    let store = &fixture.state.store;
    store
        .create_repository_onboarding(pharness_store::CreateRepositoryOnboarding {
            id: "onboard_blocked".into(),
            product_id: "prod_onboarding_blockers".into(),
            repository_id: "repo_onboarding_blockers".into(),
            binding_id: "rbind_onboarding_blockers".into(),
            onboarding_kind: "refresh".into(),
            registered_commit: "a".repeat(40),
            actor: "operator".into(),
            reason: "refresh exact source".into(),
        })
        .await
        .unwrap();
    store
        .create_repository_discovery("rdisc_blocked", "onboard_blocked", &"a".repeat(40))
        .await
        .unwrap();
    store
        .finish_repository_discovery(
            "rdisc_blocked",
            &"a".repeat(40),
            &json!({"files":[],"dependency_candidates":[]}),
            &format!("sha256:{}", "b".repeat(64)),
        )
        .await
        .unwrap();
    let app = crate::app::products::router().with_state(fixture.state.clone());
    let path = "/api/repository-onboardings/onboard_blocked";
    let (status, before) = request(&app, "GET", &format!("{path}/flow"), Value::Null).await;
    assert_eq!(status, 200);
    let proposal = json!({
        "schema_version":pharness_core::ONBOARDING_PROPOSAL_SCHEMA,"discovery_id":"rdisc_blocked","discovery_hash":format!("sha256:{}", "b".repeat(64)),
        "candidate_contract":null,"instructions":"Resolve the absent dependency lock before configuring execution.",
        "service_proposals":[],"binding_proposals":[],"assumptions":[],"conflicts":[],"blockers":["immutable_dependency_lock_missing"],"readiness_forecast":{}
    });
    let (status, _) = request(&app, "PUT", &format!("{path}/proposal"), json!({"actor":"operator","reason":"retain exact blocker","state_hash":before["onboarding"]["state_hash"],"proposal":proposal})).await;
    assert_eq!(status, 200);
    let persisted = store
        .get_repository_onboarding("onboard_blocked")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.status, "proposal_blocked");
    let (status, overview) = request(&app, "GET", "/api/organization/overview", Value::Null).await;
    assert_eq!(status, 200);
    assert!(overview["attention"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["resource_id"] == "onboard_blocked"
            && item["kind"] == "blocked"
            && item["status"] == "blocked"));
    for _ in 0..2 {
        let (status, flow) = request(&app, "GET", &format!("{path}/flow"), Value::Null).await;
        assert_eq!(status, 200);
        assert_eq!(flow["onboarding"]["status"], "proposal_blocked");
        assert!(flow["proposal"]["proposal"]["candidate_contract"].is_null());
        assert_eq!(flow["onboarding"]["actions"][0]["id"], "refresh_onboarding");
        assert_eq!(flow["onboarding"]["actions"][0]["status"], "blocked");
        for action in [
            "approve_proposal",
            "retry_proposer",
            "prepare_onboarding_patch",
            "authorize_onboarding_source_delivery",
        ] {
            let (status, _) = request(&app, "POST", &format!("{path}/actions/{action}/execute"), json!({"actor":"operator","reason":"must remain blocked","state_hash":flow["onboarding"]["state_hash"]})).await;
            assert!(matches!(status, 404 | 409), "{action}: {status}");
        }
        let mut invalid = proposal.clone();
        invalid["blockers"] = json!([]);
        let (status, _) = request(&app, "PUT", &format!("{path}/proposal"), json!({"actor":"operator","reason":"absence cannot be ready","state_hash":flow["onboarding"]["state_hash"],"proposal":invalid})).await;
        assert_eq!(status, 400);
    }
    assert_eq!(
        store
            .get_repository_onboarding("onboard_blocked")
            .await
            .unwrap()
            .unwrap(),
        persisted
    );
    assert!(persisted.source_delivery_intent_id.is_none());
    assert!(persisted.approved_proposal_hash.is_none());
}

#[tokio::test]
async fn onboarding_product_proposals_recheck_live_services_without_partial_writes() {
    let fixture = repo_fixture_with_workflow("product_recheck", false, false).await;
    let store = &fixture.state.store;
    store
        .create_repository_onboarding(pharness_store::CreateRepositoryOnboarding {
            id: "onboard_product_recheck_refresh".into(),
            product_id: "prod_product_recheck".into(),
            repository_id: "repo_product_recheck".into(),
            binding_id: "rbind_product_recheck".into(),
            onboarding_kind: "refresh".into(),
            registered_commit: "a".repeat(40),
            actor: "operator".into(),
            reason: "review original product context".into(),
        })
        .await
        .unwrap();
    store
        .create_repository_discovery(
            "rdisc_product_recheck_refresh",
            "onboard_product_recheck_refresh",
            &"a".repeat(40),
        )
        .await
        .unwrap();
    store
        .finish_repository_discovery(
            "rdisc_product_recheck_refresh",
            &"a".repeat(40),
            &json!({"files":[],"dependency_candidates":[]}),
            &format!("sha256:{}", "b".repeat(64)),
        )
        .await
        .unwrap();
    let app = crate::app::products::router().with_state(fixture.state.clone());
    let path = "/api/repository-onboardings/onboard_product_recheck_refresh";
    let (_, flow) = request(&app, "GET", &format!("{path}/flow"), Value::Null).await;
    let mut proposal = json!({
        "schema_version":pharness_core::ONBOARDING_PROPOSAL_SCHEMA,
        "discovery_id":"rdisc_product_recheck_refresh", "discovery_hash":format!("sha256:{}", "b".repeat(64)),
        "candidate_contract":null, "instructions":"Resolve the missing lock before execution.",
        "service_proposals":[{"service_key":"new-api", "display_name":"New API", "description":"Requested component"}],
        "binding_proposals":[{"service_keys":["new-api"], "scopes":["src/**"]}],
        "blockers":["immutable_dependency_lock_missing"], "readiness_forecast":{}
    });
    let (status, body) = request(&app, "PUT", &format!("{path}/proposal"), json!({
        "actor":"operator", "reason":"retain bounded proposal", "state_hash":flow["onboarding"]["state_hash"], "proposal":proposal
    })).await;
    assert_eq!(status, 200, "{body}");
    assert!(
        store
            .list_product_services("prod_product_recheck")
            .await
            .unwrap()
            .is_empty(),
        "proposal is not creation authority"
    );

    // Another reviewed operation creates the Service after the original proposal.
    let product = store
        .get_product("prod_product_recheck")
        .await
        .unwrap()
        .unwrap();
    store
        .apply_product_model_revision(pharness_store::ApplyProductModelRevision {
            product_id: product.id,
            expected_state_version: product.state_version,
            services: vec![pharness_store::ProductModelServiceRevision {
                id: "svc_concurrent".into(),
                service_key: "new-api".into(),
                display_name: "New API".into(),
                description: "Separately created component".into(),
                status: "active".into(),
            }],
            bindings: vec![],
            snapshot_id: "pmodel_concurrent".into(),
            snapshot_json: json!({"services":[{"id":"svc_concurrent","service_key":"new-api"}]}),
            snapshot_hash: format!("sha256:{}", "c".repeat(64)),
            actor: "operator".into(),
            reason: "separate reviewed creation".into(),
        })
        .await
        .unwrap();
    let before = store
        .get_repository_onboarding("onboard_product_recheck_refresh")
        .await
        .unwrap()
        .unwrap();
    let (_, flow) = request(&app, "GET", &format!("{path}/flow"), Value::Null).await;
    let (status, body) = request(&app, "PUT", &format!("{path}/proposal"), json!({
        "actor":"operator", "reason":"recheck existing proposal", "state_hash":flow["onboarding"]["state_hash"], "proposal":proposal
    })).await;
    assert_eq!(status, 409, "{body}");
    assert!(body
        .to_string()
        .contains("service_proposals[0].service_key"));
    assert_eq!(
        store
            .get_repository_onboarding("onboard_product_recheck_refresh")
            .await
            .unwrap()
            .unwrap(),
        before
    );

    proposal["service_proposals"] = json!([]);
    let (status, body) = request(&app, "PUT", &format!("{path}/proposal"), json!({
        "actor":"operator", "reason":"reuse existing Service", "state_hash":flow["onboarding"]["state_hash"], "proposal":proposal
    })).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        store
            .list_product_services("prod_product_recheck")
            .await
            .unwrap()
            .len(),
        1
    );
    let (_, flow) = request(&app, "GET", &format!("{path}/flow"), Value::Null).await;
    proposal["binding_proposals"][0]["service_keys"] = json!(["unknown"]);
    let (status, body) = request(&app, "PUT", &format!("{path}/proposal"), json!({
        "actor":"operator", "reason":"unknown reference must fail", "state_hash":flow["onboarding"]["state_hash"], "proposal":proposal
    })).await;
    assert_eq!(status, 400, "{body}");
    assert!(body
        .to_string()
        .contains("binding_proposals[0].service_keys[0]"));
}
