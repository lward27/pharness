use super::hosted_pipeline::{build_intent, merged_finance_source};
use crate::app::{deployment, gitops};
use crate::dispatch::KubectlFixture;
use axum::extract::{Path, State};
use axum::Json;
use pharness_store::{
    CreateArtifact, CreateDeploymentIntent, CreateGitOpsChangeSet, DeliveryStage,
};
use serde_json::{json, Value};

fn request<T: serde::de::DeserializeOwned>(value: Value) -> Json<T> {
    Json(serde_json::from_value(value).unwrap())
}

#[tokio::test]
async fn hosted_deployment_records_are_readable_but_legacy_routes_cannot_advance_them() {
    let fake = KubectlFixture::new(false);
    let f = merged_finance_source("stage_records", &fake).await;
    let pipeline = build_intent(&f).await;
    assert!(pipeline.run_id.is_none());
    let state = || State(f.state.clone());
    let baseline_creates = fake.creates();
    let err = deployment::intents::create_deployment_intent_from_pipeline_intent(
        state(),
        request(json!({
            "pipeline_intent_id":pipeline.id,"target_environment":"production"
        })),
    )
    .await
    .unwrap_err();
    assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
    assert!(err.message.contains("saved workflow"));
    let err = gitops::change_sets::create_gitops_change_set(
        state(),
        request(json!({
            "pipeline_intent_id":pipeline.id,"gitops_update_plan_artifact_id":"not-created"
        })),
    )
    .await
    .unwrap_err();
    assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
    assert!(err.message.contains("saved workflow"));

    // Storage scaffolding deliberately does not claim a real staging deployment.
    // The controller must bind build results, operation, authority and observations.
    for stage in [DeliveryStage::Staging, DeliveryStage::Production] {
        let id = format!("d_{}", stage.as_str());
        let intent = f
            .state
            .store
            .create_deployment_intent_for_stage(
                CreateDeploymentIntent {
                    id: id.clone(),
                    pipeline_intent_id: pipeline.id.clone(),
                    change_set_id: pipeline.change_set_id.clone(),
                    work_plan_id: pipeline.work_plan_id.clone(),
                    remediation_plan_id: None,
                    incident_id: None,
                    session_id: pipeline.session_id.clone(),
                    run_id: None,
                    status: "proposed".into(),
                    title: "Delivery fixture".into(),
                    summary: "Not deployment evidence".into(),
                    risk_level: "medium".into(),
                    intent_kind: "gitops_deploy".into(),
                    target_environment: Some(stage.as_str().into()),
                    target_namespace: Some(
                        if stage == DeliveryStage::Staging {
                            "apps-staging"
                        } else {
                            "apps"
                        }
                        .into(),
                    ),
                    argo_application: Some(id.clone()),
                    resource_namespace: None,
                    resource_kind: None,
                    resource_name: None,
                    intent_json: json!({}),
                },
                stage,
            )
            .await
            .unwrap();
        let Json(read) = deployment::intents::get_deployment_intent(state(), Path(id.clone()))
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(read).unwrap()["delivery_stage"],
            stage.as_str()
        );
        assert!(deployment::target::deployment_target(&intent)
            .unwrap_err()
            .message
            .contains("legacy Argo"));
        let err = deployment::intents::transition_deployment_intent(
            state(),
            Path(id.clone()),
            request(json!({"target_status":"approved"})),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
        let err = deployment::intents::attach_deployment_intent_evidence(
            state(),
            Path(id.clone()),
            request(json!({"observation_id":"no-observation"})),
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
        f.state
            .store
            .create_artifact(CreateArtifact {
                id: format!("a_{id}"),
                session_id: pipeline.session_id.clone(),
                run_id: None,
                kind: "gitops_update_plan".into(),
                label: "Fixture".into(),
                mime_type: Some("application/json".into()),
                path: None,
                content_text: None,
                content_json: Some(json!({"deployment_intent_id":id})),
            })
            .await
            .unwrap();
        let change = f
            .state
            .store
            .create_gitops_change_set(CreateGitOpsChangeSet {
                id: format!("g_{id}"),
                work_item_id: f.work_item_id.clone(),
                work_plan_id: pipeline.work_plan_id.clone(),
                source_change_set_id: pipeline.change_set_id.clone(),
                pipeline_intent_id: pipeline.id.clone(),
                deployment_intent_id: id,
                gitops_update_plan_artifact_id: format!("a_d_{}", stage.as_str()),
                session_id: pipeline.session_id.clone(),
                run_id: None,
                status: "proposed".into(),
                title: "Fixture".into(),
                summary: "No approval or merge".into(),
                risk_level: "medium".into(),
                material_hash: "sha256:fixture".into(),
                gitops_repo: "https://github.com/lward27/lucas_engineering.git".into(),
                gitops_ref: "main".into(),
                head_branch: format!("fixture-{}", stage.as_str()),
                kustomization_path: "fixture/kustomization.yaml".into(),
                image_name: "fixture/app".into(),
                image_ref: format!("fixture/app@sha256:{}", "a".repeat(64)),
                gitops_change_set_json: json!({}),
            })
            .await
            .unwrap();
        let Json(read) =
            gitops::change_sets::get_gitops_change_set(state(), Path(change.id.clone()))
                .await
                .unwrap();
        assert!(serde_json::to_value(read).unwrap()["run_id"].is_null());
        assert!(
            gitops::delivery_flow::gitops_delivery_flow(&f.state.store, Some(&change))
                .await
                .unwrap()
                .is_none()
        );
        for action in [
            "transition",
            "resolve",
            "prepare",
            "authorize",
            "preflight",
            "execute",
        ] {
            let path = Path(change.id.clone());
            let err = match action {
                "resolve" => gitops::delivery::resolve_gitops_base_revision(
                    state(),
                    None,
                    path,
                    request(json!({"reason":"Must be rejected"})),
                )
                .await
                .unwrap_err(),
                "transition" => gitops::change_sets::transition_gitops_change_set(
                    state(),
                    path,
                    request(json!({"target_status":"approved"})),
                )
                .await
                .unwrap_err(),
                "prepare" => gitops::delivery::prepare_gitops_change_set_delivery(
                    state(),
                    path,
                    request(json!({})),
                )
                .await
                .unwrap_err(),
                "authorize" => gitops::delivery::authorize_gitops_change_set_delivery(
                    state(),
                    None,
                    path,
                    request(json!({"reason":"Must be rejected"})),
                )
                .await
                .unwrap_err(),
                "preflight" => gitops::delivery::preflight_gitops_change_set_delivery(
                    state(),
                    None,
                    path,
                    request(json!({})),
                )
                .await
                .unwrap_err(),
                "execute" => gitops::delivery::execute_gitops_change_set_delivery(
                    state(),
                    None,
                    path,
                    request(json!({"reason":"Must be rejected"})),
                )
                .await
                .unwrap_err(),
                _ => unreachable!(),
            };
            assert_eq!(err.status, axum::http::StatusCode::CONFLICT, "{action}");
            assert!(
                err.message.contains("saved workflow"),
                "{action}: {}",
                err.message
            );
        }
        let retained = f
            .state
            .store
            .get_gitops_change_set(&change.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retained, change);
    }
    assert_eq!(
        fake.creates(),
        baseline_creates,
        "read and rejected legacy actions must not dispatch Jobs"
    );
    assert!(f
        .state
        .store
        .get_deployment_intent_by_pipeline_intent(&pipeline.id)
        .await
        .unwrap()
        .is_none());
    assert!(f
        .state
        .store
        .get_gitops_change_set_by_pipeline_intent(&pipeline.id)
        .await
        .unwrap()
        .is_none());
    assert!(!f
        .state
        .store
        .list_effective_stage_outcomes(&f.work_item_id)
        .await
        .unwrap()
        .iter()
        .any(|o| matches!(o.stage_key.as_str(), "release" | "observe")));
}
