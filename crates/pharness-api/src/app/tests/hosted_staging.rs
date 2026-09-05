use super::hosted_build::verified_build;
use super::hosted_source_merge::tick;
use super::repo_mode_v1::RepoDeliveryFixture;
use crate::app::hosted_controller::staging;
use crate::dispatch::KubectlFixture;
use axum::extract::{Path, Query, State};
use axum::Json;
use pharness_core::hosted_sdlc::staging::{HostedStagingAuthority, StagingGitOpsPlan, PLAN_SCHEMA};
use pharness_store::{DeliveryStage, StoredWorkflowOperation};
use serde_json::{json, Value};

fn request<T: serde::de::DeserializeOwned>(value: Value) -> Json<T> {
    Json(serde_json::from_value(value).unwrap())
}
fn now() -> i64 {
    crate::app::clock::current_millis() as i64
}
fn creates(fake: &KubectlFixture) -> usize {
    std::fs::read_to_string(fake.dir.join("build-creates"))
        .unwrap_or_default()
        .lines()
        .count()
}
async fn operation(f: &RepoDeliveryFixture) -> StoredWorkflowOperation {
    f.state
        .store
        .active_workflow_operation(&f.work_item_id)
        .await
        .unwrap()
        .unwrap()
}
async fn start(
    suffix: &str,
    fake: &KubectlFixture,
) -> (
    RepoDeliveryFixture,
    StoredWorkflowOperation,
    HostedStagingAuthority,
) {
    let f = verified_build(suffix, fake).await;
    tick(&f).await;
    let op = operation(&f).await;
    let control = f
        .state
        .store
        .get_workflow_reconciliation(&f.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(control.condition, "waiting", "{}", control.condition_reason);
    assert_eq!(op.action, "stage_verified_build");
    assert_eq!(
        creates(fake),
        2,
        "one build and one original staging writer"
    );
    let a =
        serde_json::from_value(op.resource_refs["staging_dispatch"]["authority"].clone()).unwrap();
    (f, op, a)
}
fn plan(a: &HostedStagingAuthority) -> StagingGitOpsPlan {
    let original =
        include_str!("../../../../pharness-core/src/hosted_sdlc/staging/fixtures/yfinance.yaml");
    StagingGitOpsPlan {
        schema_version: PLAN_SCHEMA.into(),
        authority_hash: a.material_hash().unwrap(),
        base_commit_sha: "a".repeat(40),
        original_blob_sha: "b".repeat(40),
        original_content: original.into(),
        updated_content: pharness_core::hosted_sdlc::gitops_patch::update_kustomization_image(
            original,
            a.coordinates().unwrap().1,
            &a.image_ref().unwrap(),
        )
        .unwrap(),
    }
}
async fn save(
    f: &RepoDeliveryFixture,
    a: &HostedStagingAuthority,
    p: &StagingGitOpsPlan,
) -> Result<Json<Value>, crate::app::ApiError> {
    staging::internal_staging_plan(State(f.state.clone()), Path(a.deployment_intent_id.clone()), request(json!({"execution_id":a.execution_id,"authority_hash":a.material_hash().unwrap(),"plan":p}))).await
}
async fn admit(
    f: &RepoDeliveryFixture,
    a: &HostedStagingAuthority,
    p: &StagingGitOpsPlan,
) -> Result<Json<Value>, crate::app::ApiError> {
    staging::internal_staging_attempt(State(f.state.clone()), Path(a.deployment_intent_id.clone()), request(json!({"execution_id":a.execution_id,"authority_hash":a.material_hash().unwrap(),"plan_hash":p.material_hash().unwrap()}))).await
}
async fn context(
    f: &RepoDeliveryFixture,
    a: &HostedStagingAuthority,
    observer: bool,
) -> Result<Json<Value>, crate::app::ApiError> {
    staging::internal_staging_context(
        State(f.state.clone()),
        Path(a.deployment_intent_id.clone()),
        Query(
            serde_json::from_value(json!({"execution_id":a.execution_id,"observe_only":observer}))
                .unwrap(),
        ),
    )
    .await
}
async fn control(f: &RepoDeliveryFixture, value: &str) {
    let old = f
        .state
        .store
        .get_workflow_reconciliation(&f.work_item_id)
        .await
        .unwrap()
        .unwrap();
    f.state
        .store
        .set_workflow_control(
            &f.work_item_id,
            old.control_version,
            value,
            "unit-test",
            "Exercise original staging authority",
            now(),
        )
        .await
        .unwrap();
}
fn observed(a: &HostedStagingAuthority, p: &StagingGitOpsPlan) -> Value {
    json!({"execution_id":a.execution_id,"authority_hash":a.material_hash().unwrap(),"status":"committed","checked_at_ms":now(),"observe_only":false,"observation":{"gitops_commit_sha":"e".repeat(40),"base_commit_sha":p.base_commit_sha,"current_gitops_revision":"e".repeat(40),"file_blob_sha":"f".repeat(40),"path":a.coordinates().unwrap().0,"image_ref":a.image_ref().unwrap(),"authority_hash":a.material_hash().unwrap(),"plan_hash":p.material_hash().unwrap(),"parent_count":1,"changed_file_count":1,"current_file_matches":true,"observation_attempts":1}})
}
async fn outcome(
    f: &RepoDeliveryFixture,
    a: &HostedStagingAuthority,
    body: Value,
) -> Result<Json<Value>, crate::app::ApiError> {
    staging::internal_staging_outcome(
        State(f.state.clone()),
        Path(a.deployment_intent_id.clone()),
        request(body),
    )
    .await
}
fn remove_job(fake: &KubectlFixture, manifest: &Value) {
    std::fs::remove_file(fake.dir.join(format!(
        "{}.json",
        manifest["metadata"]["name"].as_str().unwrap()
    )))
    .unwrap();
}

#[tokio::test]
async fn staging_reclaims_expired_owner_without_replacing_authority_or_repeating_admission() {
    let fake = KubectlFixture::new(false);
    let (f, op, a) = start("staging_restart", &fake).await;
    let p = plan(&a);
    let _ = save(&f, &a, &p).await.unwrap();
    let past = now() - 120_000;
    f.state
        .store
        .wake_workflow(&f.work_item_id, past)
        .await
        .unwrap();
    let old = f
        .state
        .store
        .claim_due_workflow("interrupted-api", past, 60_000)
        .await
        .unwrap()
        .unwrap();
    crate::app::hosted_controller::reconcile_once(&f.state, "replacement-api")
        .await
        .unwrap();
    assert_eq!(creates(&fake), 2);
    let current = operation(&f).await;
    assert_eq!(current.id, op.id);
    assert_eq!(current.resource_refs, op.resource_refs);
    assert!(f
        .state
        .store
        .record_workflow_operation(
            &old,
            &op.id,
            "running",
            &op.resource_refs,
            "Stale owner must be fenced",
            now()
        )
        .await
        .is_err());
    remove_job(
        &fake,
        &op.resource_refs["staging_dispatch"]["executor_job_manifest"],
    );
    tick(&f).await;
    assert_eq!(
        creates(&fake),
        3,
        "only the original not-yet-admitted writer identity is recoverable"
    );
    assert_eq!(
        operation(&f).await.resource_refs["staging_dispatch"],
        op.resource_refs["staging_dispatch"]
    );
    let _ = admit(&f, &a, &p).await.unwrap();
    assert!(admit(&f, &a, &p).await.is_err());
}

#[tokio::test]
async fn expired_staging_preparation_cannot_renew_authority_and_cancelled_work_cannot_admit() {
    use pharness_store::{BeginWorkflowOperation, FinishWorkflowReconciliation};
    let fake = KubectlFixture::new(false);
    let f = verified_build("staging_expired", &fake).await;
    let past = now() - 3_700_000;
    let store = &f.state.store;
    store.wake_workflow(&f.work_item_id, past).await.unwrap();
    let old = store
        .claim_due_workflow("interrupted-api", past, 60_000)
        .await
        .unwrap()
        .unwrap();
    let op = store
        .begin_workflow_operation(
            &old,
            BeginWorkflowOperation {
                id: "operation_expired_staging",
                action: "stage_verified_build",
                input_hash: "recorded-expired-input",
                effect: "development",
                resource_keys: &["gitops:lucas_engineering:main"],
            },
            past,
        )
        .await
        .unwrap();
    store
        .finish_workflow_reconciliation(
            &old,
            FinishWorkflowReconciliation {
                next_due_at: now(),
                condition: "waiting",
                reason: "Interrupted before staging preparation",
                observed_state_hash: None,
            },
            past + 1,
        )
        .await
        .unwrap();
    tick(&f).await;
    tick(&f).await;
    assert_eq!(creates(&fake), 1);
    assert_eq!(
        store
            .get_workflow_operation(&op.id)
            .await
            .unwrap()
            .unwrap()
            .resource_refs,
        json!({})
    );
    assert_eq!(
        store
            .get_workflow_reconciliation(&f.work_item_id)
            .await
            .unwrap()
            .unwrap()
            .condition,
        "wait_expired"
    );
    let second = KubectlFixture::new(false);
    let (f, _, a) = start("staging_cancelled", &second).await;
    let p = plan(&a);
    let _ = save(&f, &a, &p).await.unwrap();
    control(&f, "cancelled").await;
    assert_eq!(
        context(&f, &a, false).await.unwrap().0["may_advance"],
        false
    );
    assert!(admit(&f, &a, &p).await.is_err());
    tick(&f).await;
    assert_eq!(creates(&second), 2);
}

#[tokio::test]
async fn unconfirmed_staging_evidence_is_repeat_safe_and_never_counts_as_a_commit() {
    let fake = KubectlFixture::new(false);
    let (f, _, a) = start("staging_unconfirmed", &fake).await;
    let p = plan(&a);
    let _ = save(&f, &a, &p).await.unwrap();
    let _ = admit(&f, &a, &p).await.unwrap();
    let body = json!({"execution_id":a.execution_id,"authority_hash":a.material_hash().unwrap(),"status":"unconfirmed","error_code":"staging_commit_not_established","checked_at_ms":now(),"observe_only":false});
    let first = outcome(&f, &a, body.clone()).await.unwrap();
    let second = outcome(&f, &a, body.clone()).await.unwrap();
    assert_eq!(first.0, second.0);
    assert_eq!(first.0["status"], "unconfirmed");
    assert!(f
        .state
        .store
        .get_artifact(&format!("staging_commit_{}", a.execution_id))
        .await
        .unwrap()
        .is_none());
    let mut invalid = body;
    invalid["observation"] = observed(&a, &p)["observation"].clone();
    assert!(outcome(&f, &a, invalid).await.is_err());
    tick(&f).await;
    assert_eq!(creates(&fake), 2);
    assert!(operation(&f)
        .await
        .resource_refs
        .get("staging_result")
        .is_none());
}

#[tokio::test]
async fn staging_progresses_from_verified_build_and_preserves_distinct_completion_boundaries() {
    let fake = KubectlFixture::new(false);
    let (f, op, a) = start("staging_normal", &fake).await;
    let original = op.resource_refs.clone();
    let p = plan(&a);
    assert_eq!(context(&f, &a, false).await.unwrap().0["may_advance"], true);
    assert_eq!(context(&f, &a, false).await.unwrap().0["plan"], Value::Null);
    assert!(context(&f, &a, true).await.is_err());
    assert!(f
        .state
        .store
        .get_gitops_change_set_by_deployment_intent(&a.deployment_intent_id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        operation(&f).await.resource_refs,
        original,
        "GET does not prepare or advance work"
    );
    assert!(admit(&f, &a, &p).await.is_err());
    let _ = save(&f, &a, &p).await.unwrap();
    let _ = save(&f, &a, &p).await.unwrap();
    let mut replaced = p.clone();
    replaced.base_commit_sha = "c".repeat(40);
    assert!(save(&f, &a, &replaced).await.is_err());
    assert!(
        outcome(&f, &a, observed(&a, &p)).await.is_err(),
        "a plausible commit cannot replace admission"
    );
    assert_eq!(admit(&f, &a, &p).await.unwrap().0["admitted"], true);
    assert!(admit(&f, &a, &p).await.is_err());
    assert_eq!(
        context(&f, &a, false).await.unwrap().0["admission_recorded"],
        true
    );
    let body = observed(&a, &p);
    assert_eq!(
        outcome(&f, &a, body.clone()).await.unwrap().0["status"],
        "gitops_committed"
    );
    let _ = outcome(&f, &a, body.clone()).await.unwrap();
    let mut later = body.clone();
    later["observation"]["current_gitops_revision"] = json!("9".repeat(40));
    let _ = outcome(&f, &a, later).await.unwrap();
    let mut conflict = body;
    conflict["observation"]["gitops_commit_sha"] = json!("8".repeat(40));
    assert!(outcome(&f, &a, conflict).await.is_err());
    tick(&f).await;
    tick(&f).await;
    assert_eq!(creates(&fake), 2);
    let current = operation(&f).await;
    assert_eq!(current.id, op.id);
    assert_eq!(current.status, "running");
    assert_eq!(
        current.resource_refs["staging_result"]["status"],
        "gitops_committed"
    );
    assert_eq!(
        current.resource_refs["staging_dispatch"],
        original["staging_dispatch"]
    );
    assert!(current
        .resource_keys
        .contains(&"gitops:lucas_engineering:main".into()));
    assert!(f
        .state
        .store
        .get_deployment_intent_for_stage(&a.pipeline_intent_id, DeliveryStage::Production)
        .await
        .unwrap()
        .is_none());
    assert!(f
        .state
        .store
        .get_repo_work_item_metadata(&f.work_item_id)
        .await
        .unwrap()
        .unwrap()
        .closed_at
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

#[tokio::test]
async fn staging_pause_blocks_writes_but_allows_one_reader_to_recover_unknown_commit() {
    let fake = KubectlFixture::new(false);
    let (f, op, a) = start("staging_recovery", &fake).await;
    let p = plan(&a);
    let _ = save(&f, &a, &p).await.unwrap();
    control(&f, "paused").await;
    assert_eq!(
        context(&f, &a, false).await.unwrap().0["may_advance"],
        false
    );
    assert!(admit(&f, &a, &p).await.is_err());
    tick(&f).await;
    assert_eq!(creates(&fake), 2);
    control(&f, "active").await;
    let _ = admit(&f, &a, &p).await.unwrap();
    control(&f, "paused").await;
    let dispatch = &op.resource_refs["staging_dispatch"];
    remove_job(&fake, &dispatch["executor_job_manifest"]);
    tick(&f).await;
    tick(&f).await;
    assert_eq!(creates(&fake), 3);
    let observer = &dispatch["observer_job_manifest"]["spec"]["template"]["spec"];
    assert_eq!(observer["serviceAccountName"], "pharness-gitops-observer");
    let env = observer["containers"][0]["env"].as_array().unwrap();
    assert!(env
        .iter()
        .any(|v| v["name"] == "PHARNESS_GIT_OBSERVER_TOKEN"
            && v["valueFrom"]["secretKeyRef"]["name"] == "pharness-gitops-observer-token"));
    assert!(!env.iter().any(|v| v["name"] == "PHARNESS_GIT_WRITER_TOKEN"));
    assert!(context(&f, &a, true).await.unwrap().0["may_advance"] == false);
    remove_job(&fake, &dispatch["observer_job_manifest"]);
    tick(&f).await;
    tick(&f).await;
    assert_eq!(
        creates(&fake),
        3,
        "an expired or deleted observer never receives a new budget"
    );
    assert_eq!(
        f.state
            .store
            .get_workflow_reconciliation(&f.work_item_id)
            .await
            .unwrap()
            .unwrap()
            .condition,
        "blocked"
    );
    let mut body = observed(&a, &p);
    body["observe_only"] = json!(true);
    let _ = outcome(&f, &a, body).await.unwrap();
    tick(&f).await;
    assert_eq!(
        operation(&f).await.resource_refs["staging_result"]["status"],
        "gitops_committed"
    );
    assert!(admit(&f, &a, &p).await.is_err());
}

#[tokio::test]
async fn staging_partial_plan_is_read_only_until_serialized_admission_repairs_linkage() {
    let fake = KubectlFixture::new(false);
    let (f, _, a) = start("staging_partial", &fake).await;
    let p = plan(&a);
    let pipeline = f
        .state
        .store
        .get_pipeline_intent(&a.pipeline_intent_id)
        .await
        .unwrap()
        .unwrap();
    f.state
        .store
        .create_artifact(pharness_store::CreateArtifact {
            id: format!("staging_plan_{}", a.execution_id),
            session_id: pipeline.session_id,
            run_id: pipeline.run_id,
            kind: "hosted_staging_plan".into(),
            label: "Interrupted plan fixture".into(),
            mime_type: Some("application/json".into()),
            path: None,
            content_text: None,
            content_json: Some(json!(p)),
        })
        .await
        .unwrap();
    assert!(context(&f, &a, false).await.unwrap().0["plan"].is_object());
    assert!(f
        .state
        .store
        .get_gitops_change_set_by_deployment_intent(&a.deployment_intent_id)
        .await
        .unwrap()
        .is_none());
    let _ = admit(&f, &a, &p).await.unwrap();
    let change = f
        .state
        .store
        .get_gitops_change_set_by_deployment_intent(&a.deployment_intent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(change.id, a.gitops_change_set_id);
    assert_eq!(
        change.gitops_change_set_json["delivery_mode"],
        "atomic_commit_expected_head"
    );
    assert!(change.run_id.is_none());
    assert!(admit(&f, &a, &p).await.is_err());
}

#[tokio::test]
async fn staging_rejects_changed_build_and_contradictory_or_misbound_observations() {
    let fake = KubectlFixture::new(false);
    let (f, _, a) = start("staging_integrity", &fake).await;
    let p = plan(&a);
    let _ = save(&f, &a, &p).await.unwrap();
    let _ = admit(&f, &a, &p).await.unwrap();
    let body = observed(&a, &p);
    for (pointer, bad) in [
        ("/observation/base_commit_sha", json!("c".repeat(40))),
        ("/observation/plan_hash", json!("sha256:wrong")),
        (
            "/observation/path",
            json!("charts/yfinance-wrapper/kustomization.yaml"),
        ),
        ("/observation/parent_count", json!(2)),
        ("/observation/changed_file_count", json!(2)),
        ("/observation/current_file_matches", json!(false)),
        ("/observation/file_blob_sha", json!(p.original_blob_sha)),
        (
            "/observation/image_ref",
            json!("registry.lucas.engineering/yfinance_wrapper:latest"),
        ),
        ("/checked_at_ms", json!(a.created_at_ms - 1)),
    ] {
        let mut invalid = body.clone();
        *invalid.pointer_mut(pointer).unwrap() = bad;
        assert!(outcome(&f, &a, invalid).await.is_err(), "{pointer}");
    }
    let mut contradictory = body.clone();
    contradictory["error_code"] = json!("staging_unavailable");
    assert!(outcome(&f, &a, contradictory).await.is_err());
    let mut pipeline = f
        .state
        .store
        .get_pipeline_intent(&a.pipeline_intent_id)
        .await
        .unwrap()
        .unwrap();
    pipeline.intent_json["build_output"]["image_digest"] =
        json!(format!("sha256:{}", "8".repeat(64)));
    f.state
        .store
        .update_pipeline_intent_execution(
            &pipeline.id,
            pharness_store::UpdatePipelineIntentExecution {
                status: pipeline.status,
                intent_json: pipeline.intent_json,
                actor: None,
                reason: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        context(&f, &a, false).await.unwrap().0["may_advance"],
        false
    );
    // Authentic already-admitted effects remain auditable even when current
    // authority changes. They do not allow further delivery progression.
    let _ = outcome(&f, &a, body).await.unwrap();
    tick(&f).await;
    assert_eq!(
        f.state
            .store
            .get_workflow_reconciliation(&f.work_item_id)
            .await
            .unwrap()
            .unwrap()
            .condition,
        "blocked"
    );
    assert!(operation(&f)
        .await
        .resource_refs
        .get("staging_result")
        .is_none());
    assert_eq!(creates(&fake), 2);
}
