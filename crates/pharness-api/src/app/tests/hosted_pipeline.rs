use super::hosted_source_merge::{admit, authority, fixture, fixture_with_policy, tick};
use super::repo_mode_v1::RepoDeliveryFixture;
use crate::app::pipeline::{hosted, intents::work_item_pipeline_source_provenance};
use crate::dispatch::KubectlFixture;
use axum::extract::{Path, State};
use axum::Json;
use pharness_store::StoredChangeSet;
use serde_json::{json, Value};

async fn merged_source(suffix: &str, fake: &KubectlFixture) -> RepoDeliveryFixture {
    finish_source(fixture(suffix, fake).await).await
}

pub(super) async fn merged_finance_source(
    suffix: &str,
    fake: &KubectlFixture,
) -> RepoDeliveryFixture {
    finish_source(fixture_with_policy(suffix, fake, Some(finance_policy())).await).await
}

async fn finish_source(f: RepoDeliveryFixture) -> RepoDeliveryFixture {
    let a = authority(&f).await;
    let _ = admit(&f, &a).await.unwrap();
    let intent = f
        .state
        .store
        .get_source_delivery_intent(&f.intent_id)
        .await
        .unwrap()
        .unwrap();
    f.state
        .store
        .update_source_delivery_intent(
            &intent.id,
            intent.state_version,
            "observer_dispatched",
            None,
            Some("observer_build"),
            None,
            None,
            None,
            "test-controller",
            "Observe admitted source before build",
        )
        .await
        .unwrap();
    let request = json!({"execution_id":"observer_build","status":"observed","pull_request_state":"closed","merged":true,"merge_commit_sha":"c".repeat(40),"head_commit_sha":a.head_commit_sha,"head_branch":a.head_branch,"merge_parent_shas":[a.base_commit_sha,a.head_commit_sha],"merge_tree_sha":"d".repeat(40),"authoritative_rules_succeeded":true,"required_checks":[{"name":"Source integrity","app_id":15368,"status":"passing"}],"check_runs":[],"commit_statuses":[],"provider_check_status":"passing"});
    let Json(result) = crate::app::repo_mode::internal_source_delivery_observation_outcome(
        State(f.state.clone()),
        Path(intent.id),
        Json(serde_json::from_value(request).unwrap()),
    )
    .await
    .unwrap();
    assert_eq!(result["delivery_status"], "succeeded");
    tick(&f).await;
    f
}

async fn change(f: &RepoDeliveryFixture) -> StoredChangeSet {
    let intent = f
        .state
        .store
        .get_source_delivery_intent(&f.intent_id)
        .await
        .unwrap()
        .unwrap();
    f.state
        .store
        .get_change_set(&intent.subject_id)
        .await
        .unwrap()
        .unwrap()
}

fn finance_policy() -> Value {
    let mut p: Value = serde_json::from_str(include_str!(
        "../../../../pharness-core/tests/fixtures/hosted-workflow.json"
    ))
    .unwrap();
    p["delivery_binding"]["image_name"] = json!("registry.lucas.engineering/yfinance_wrapper");
    p["delivery_binding"]["gitops_repo"] =
        json!("https://github.com/lward27/lucas_engineering.git");
    p["delivery_binding"]["staging"]["kustomization_path"] =
        json!("charts/finance-staging/yfinance/kustomization.yaml");
    p["delivery_binding"]["production"]["kustomization_path"] =
        json!("charts/yfinance-wrapper/kustomization.yaml");
    p["pipeline_contract"]["pipeline_ref"] = json!("pharness-yfinance-build");
    p["pipeline_contract"]["contract_json"] = json!({"params":[{"name":"revision","type":"scalar","required":true},{"name":"dockerfile","type":"scalar","required":false},{"name":"context","type":"scalar","required":false}],"source_revision_param":"revision","workspaces":[{"name":"shared-data","binding":"volume_claim_template","required":true}]});
    p["staging_contract"]["argo_application"] = json!("yfinance-staging");
    p["production_contract"]["argo_application"] = json!("yfinance-wrapper");
    for key in ["staging_contract", "production_contract"] {
        p[key]["contract_json"]["workload_name"] = json!("yfinance-wrapper");
        p[key]["contract_json"]["service_name"] = json!("yfinance-wrapper");
        p[key]["contract_json"]["service_port"] = json!(8090);
    }
    p
}

async fn build_intent(f: &RepoDeliveryFixture) -> pharness_store::StoredPipelineIntent {
    let request = json!({"pipeline_contract_id":"pipeline_test","intent_json":{"execution":{"enabled":true,"namespace":"tekton-pipelines","pipeline_ref":"pharness-yfinance-build","production_impacting":false,"params":{"revision":"c".repeat(40),"dockerfile":"./Dockerfile","context":"./"},"workspaces":[{"name":"shared-data","volume_claim_template":{"storage":"1Gi","access_modes":["ReadWriteOnce"]}}]}}});
    let Json(created) = crate::app::pipeline::intents::create_work_item_pipeline_intent(
        State(f.state.clone()),
        None,
        Path(f.work_item_id.clone()),
        Json(serde_json::from_value(request).unwrap()),
    )
    .await
    .unwrap();
    let Json(_) = crate::app::pipeline::intents::transition_pipeline_intent(State(f.state.clone()),Path(created.pipeline_intent.id.clone()),Json(serde_json::from_value(json!({"target_status":"approved","actor":"unit-test","reason":"Fixture authorization; no external build"})).unwrap())).await.unwrap();
    f.state
        .store
        .get_pipeline_intent(&created.pipeline_intent.id)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn hosted_pipeline_authority_is_finite_non_deploying_and_cannot_be_manually_extended() {
    let fake = KubectlFixture::new(false);
    let f =
        finish_source(fixture_with_policy("build_authorized", &fake, Some(finance_policy())).await)
            .await;
    let intent = build_intent(&f).await;
    assert!(hosted::validate_intent(&f.state, &intent).await.unwrap());
    let item = f
        .state
        .store
        .get_work_item(&f.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        item.production_impacting,
        "the overall hosted workflow includes a future production decision"
    );
    let request = json!({"subject":f.state.policy.subject,"reason":"Unit fixture for the controller-derived non-deploying build grant","created_by":"unit-test","expires_at":(crate::app::clock::current_millis()+60_000).to_string(),"scope":{"environment":item.target_environment,"capability_kinds":["tekton_start_run"],"actions":["tekton_trigger_pipeline"],"max_risk":"high","namespaces":["tekton-pipelines"],"work_plan_ids":[intent.work_plan_id],"change_set_ids":[intent.change_set_id],"pipeline_intent_ids":[intent.id],"production_impacting":false},"policy":{"policy_mode":"supervised_autonomy"}});
    crate::app::approvals::create_permission_grant_record(
        &f.state.store,
        serde_json::from_value(request).unwrap(),
    )
    .await
    .unwrap();
    let preflight =
        crate::app::pipeline::execution::pipeline_intent_execution_preflight(&f.state, &intent.id)
            .await
            .unwrap();
    assert!(preflight.ready, "{:?}", preflight.checks);
    assert!(preflight
        .checks
        .iter()
        .any(|c| c["code"] == "hosted_build_authority"));
    assert!(!preflight
        .checks
        .iter()
        .any(|c| c["code"] == "approval_gate_production_impact"));
    let manual = crate::app::pipeline::execution::execute_pipeline_intent(
        State(f.state.clone()),
        None,
        Path(intent.id.clone()),
        Json(serde_json::from_value(json!({"dry_run":false})).unwrap()),
    )
    .await;
    assert!(manual
        .unwrap_err()
        .message
        .contains("Hosted builds advance"));
    let grant = crate::app::pipeline::intents::create_pipeline_intent_trusted_envelope(
        State(f.state.clone()),
        Path(intent.id.clone()),
        Json(serde_json::from_value(json!({"reason":"Try to extend build authority"})).unwrap()),
    )
    .await;
    assert!(grant.unwrap_err().message.contains("manual envelope"));
    for (pointer, value) in [
        ("/execution/production_impacting", json!(true)),
        ("/execution/namespace", json!("apps-prod")),
        (
            "/execution/pipeline_ref",
            json!("pharness-finance-frontend-build"),
        ),
        ("/execution/params/revision", json!("e".repeat(40))),
        ("/source_provenance/merge_commit_sha", json!("e".repeat(40))),
        ("/pipeline_contract/version", json!("unreviewed")),
    ] {
        let mut changed = intent.clone();
        *changed.intent_json.pointer_mut(pointer).unwrap() = value;
        assert!(
            hosted::validate_intent(&f.state, &changed).await.is_err(),
            "{pointer}"
        );
    }
    let control = f
        .state
        .store
        .get_workflow_reconciliation(&f.work_item_id)
        .await
        .unwrap()
        .unwrap();
    let paused = f
        .state
        .store
        .set_workflow_control(
            &f.work_item_id,
            control.control_version,
            "paused",
            "unit-test",
            "Pause before build",
            crate::app::clock::current_millis() as i64,
        )
        .await
        .unwrap();
    assert!(hosted::validate_intent(&f.state, &intent).await.is_err());
    f.state
        .store
        .set_workflow_control(
            &f.work_item_id,
            paused.control_version,
            "active",
            "unit-test",
            "Resume unchanged authority",
            crate::app::clock::current_millis() as i64,
        )
        .await
        .unwrap();
    assert!(hosted::validate_intent(&f.state, &intent).await.unwrap());
    f.state
        .store
        .update_pipeline_contract_status(
            "pipeline_test",
            "retired",
            Some("unit-test".into()),
            Some("Retire the bound build contract".into()),
        )
        .await
        .unwrap();
    assert!(hosted::validate_intent(&f.state, &intent).await.is_err());
    assert_eq!(
        fake.creates(),
        1,
        "source fixture is the only external-operation fixture"
    );
}

#[tokio::test]
async fn hosted_build_outputs_require_declared_results_without_conflicting_evidence() {
    let fake = KubectlFixture::new(false);
    let f =
        finish_source(fixture_with_policy("build_results", &fake, Some(finance_policy())).await)
            .await;
    let intent = build_intent(&f).await;
    let commit = "c".repeat(40);
    let image = format!("registry.lucas.engineering/yfinance_wrapper:git-{commit}");
    let digest = format!("sha256:{}", "d".repeat(64));
    let analysis = json!({"kind":"PipelineRunAnalysis","pipeline_run":{"uid":"real-kubernetes-uid"},"summary":{"status":"succeeded"},"outputs":{"commit":commit,"source_commit":commit,"image_url":image,"image_digest":digest,"declared_results":{"SOURCE_COMMIT":commit,"IMAGE_URL":image,"IMAGE_DIGEST":digest},"result_conflicts":[]}});
    assert!(
        crate::app::pipeline::execution::pipeline_build_output_from_analysis(&intent, &analysis)
            .is_some()
    );
    for (pointer, value) in [
        ("/outputs/declared_results/SOURCE_COMMIT", Value::Null),
        (
            "/outputs/declared_results/SOURCE_COMMIT",
            json!("e".repeat(40)),
        ),
        ("/outputs/result_conflicts", json!(["source_commit"])),
        (
            "/outputs/declared_results/IMAGE_URL",
            json!("registry.lucas.engineering/finance-frontend:git-other"),
        ),
        (
            "/outputs/image_digest",
            json!(format!("sha256:{}", "e".repeat(64))),
        ),
        ("/pipeline_run/uid", Value::Null),
        ("/summary/status", json!("failed")),
    ] {
        let mut changed = analysis.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(
            crate::app::pipeline::execution::pipeline_build_output_from_analysis(&intent, &changed)
                .is_none(),
            "{pointer}"
        );
    }
}

#[tokio::test]
async fn hosted_pipeline_consumes_sealed_merge_without_legacy_run_artifacts() {
    let fake = KubectlFixture::new(false);
    let f = merged_source("build_sealed", &fake).await;
    let change = change(&f).await;
    assert!(change.run_id.is_none());
    let provenance = work_item_pipeline_source_provenance(&f.state.store, &change)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(provenance["merge_commit_sha"], "c".repeat(40));
    assert_eq!(
        provenance["hosted"]["source_delivery_intent_id"],
        f.intent_id
    );
    assert_eq!(
        provenance["hosted"]["change_set_material_hash"],
        change.material_hash
    );
    assert_eq!(
        fake.creates(),
        1,
        "reading source provenance cannot dispatch a build"
    );
    assert!(!f
        .state
        .store
        .list_effective_stage_outcomes(&f.work_item_id)
        .await
        .unwrap()
        .iter()
        .any(|o| matches!(o.stage_key.as_str(), "release" | "observe")));
}

#[tokio::test]
async fn hosted_pipeline_rejects_unsealed_or_changed_source_instead_of_falling_back() {
    let fake = KubectlFixture::new(false);
    let f = fixture("build_unmerged", &fake).await;
    let c = change(&f).await;
    assert!(hosted::source_provenance(&f.state.store, &c).await.is_err());
    let fake = KubectlFixture::new(false);
    let f = merged_source("build_changed", &fake).await;
    let mut c = change(&f).await;
    c.material_hash = format!("sha256:{}", "e".repeat(64));
    assert!(hosted::source_provenance(&f.state.store, &c).await.is_err());
    let c = change(&f).await;
    let intent = f
        .state
        .store
        .get_source_delivery_intent(&f.intent_id)
        .await
        .unwrap()
        .unwrap();
    let mut proof = intent.merge_provenance.clone().unwrap();
    proof["merge_commit_sha"] = Value::String("e".repeat(40));
    f.state
        .store
        .update_source_delivery_intent(
            &intent.id,
            intent.state_version,
            "merged",
            None,
            None,
            None,
            Some(&proof),
            None,
            "test-controller",
            "Conflicting provider evidence must not reuse the old sealed outcome",
        )
        .await
        .unwrap();
    assert!(hosted::source_provenance(&f.state.store, &c).await.is_err());
    assert_eq!(fake.creates(), 1);
}
