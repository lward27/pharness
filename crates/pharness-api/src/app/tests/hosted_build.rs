use super::hosted_pipeline::merged_finance_source;
use super::hosted_source_merge::tick;
use super::repo_mode_v1::RepoDeliveryFixture;
use crate::app::hosted_controller::build;
use crate::dispatch::KubectlFixture;
use axum::extract::{Path, State};
use axum::Json;
use pharness_store::StoredWorkflowOperation;
use serde_json::{json, Value};

fn build_transport(fake: &KubectlFixture) {
    let dir = serde_json::to_string(&fake.dir.to_str().unwrap()).unwrap();
    // Owned UUID temporary directory; this executable cannot contact Kubernetes.
    let script = format!(
        r#"#!/usr/bin/env python3
import sys,json,os
from pathlib import Path
root=Path({dir})
if sys.argv[1]=='get':
    target=root/(sys.argv[3]+'.json')
    if target.exists():print(target.read_text())
    sys.exit(0)
if sys.argv[1]=='create':
    value=json.load(sys.stdin);name=value['metadata']['name'];target=root/(name+'.json')
    try:
        with target.open('x') as f:json.dump(value,f)
        with (root/'build-creates').open('a') as f:f.write(name+'\n')
    except FileExistsError:pass
    sys.exit(1) # Server-side create succeeded; acknowledgement was lost.
sys.exit(99)
"#
    );
    std::fs::write(&fake.command, script).unwrap();
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
async fn start(suffix: &str, fake: &KubectlFixture) -> RepoDeliveryFixture {
    let f = merged_finance_source(suffix, fake).await;
    build_transport(fake);
    tick(&f).await;
    let control = f
        .state
        .store
        .get_workflow_reconciliation(&f.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(control.condition, "waiting", "{}", control.condition_reason);
    let op = operation(&f).await;
    assert_eq!(op.action, "build_verified_source");
    assert_eq!(creates(fake), 1);
    f
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
            "Exercise saved build control",
            crate::app::clock::current_millis() as i64,
        )
        .await
        .unwrap();
}
fn attempt(op: &StoredWorkflowOperation) -> Value {
    json!({"execution_id":op.resource_refs["build_dispatch"]["execution_id"],"manifest_hash":op.resource_refs["build_dispatch"]["manifest_hash"]})
}
async fn admit(
    f: &RepoDeliveryFixture,
    op: &StoredWorkflowOperation,
) -> Result<Json<Value>, crate::app::ApiError> {
    build::internal_build_attempt(
        State(f.state.clone()),
        Path(
            op.resource_refs["pipeline_intent_id"]
                .as_str()
                .unwrap()
                .into(),
        ),
        Json(serde_json::from_value(attempt(op)).unwrap()),
    )
    .await
}
fn observed(op: &StoredWorkflowOperation, successful: bool) -> Value {
    let d = &op.resource_refs["build_dispatch"];
    assert_eq!(
        d["observer_job_manifest"]["spec"]["template"]["spec"]["serviceAccountName"],
        "pharness-worker"
    );
    assert_eq!(
        d["executor_job_manifest"]["spec"]["template"]["spec"]["serviceAccountName"],
        "pharness-tekton-runner"
    );
    let mut run = d["pipeline_run_manifest"].clone();
    run["metadata"]["uid"] = json!("original-build-uid");
    let declared = json!({"SOURCE_COMMIT":"c".repeat(40),"IMAGE_URL":format!("registry.lucas.engineering/yfinance_wrapper:git-{}","c".repeat(40)),"IMAGE_DIGEST":format!("sha256:{}","d".repeat(64))});
    run["status"] = json!({"conditions":[{"type":"Succeeded","status":if successful {"True"}else{"False"}}],"results":declared.as_object().unwrap().iter().map(|(name,value)|json!({"name":name,"value":value})).collect::<Vec<_>>()});
    let analysis = json!({"kind":"PipelineRunAnalysis","pipeline_run":{"uid":run["metadata"]["uid"],"name":run["metadata"]["name"],"namespace":"tekton-pipelines"},"summary":{"status":if successful {"succeeded"}else{"failed"},"failed_task_run_count":if successful {0}else{1},"succeeded_task_run_count":if successful {4}else{3},"running_task_run_count":0,"task_run_count":4},"outputs":{"declared_results":declared,"source_commit":declared["SOURCE_COMMIT"],"commit":declared["SOURCE_COMMIT"],"image_url":declared["IMAGE_URL"],"image_digest":declared["IMAGE_DIGEST"],"result_conflicts":[]}});
    json!({"execution_id":d["execution_id"],"manifest_hash":d["manifest_hash"],"pipeline_run":run,"analysis":analysis,"observe_only":true})
}
async fn outcome(
    f: &RepoDeliveryFixture,
    op: &StoredWorkflowOperation,
    body: Value,
) -> Result<Json<Value>, crate::app::ApiError> {
    build::internal_build_outcome(
        State(f.state.clone()),
        Path(
            op.resource_refs["pipeline_intent_id"]
                .as_str()
                .unwrap()
                .into(),
        ),
        Json(serde_json::from_value(body).unwrap()),
    )
    .await
}

#[tokio::test]
async fn hosted_build_recovers_original_dispatch_and_admits_only_once() {
    let fake = KubectlFixture::new(false);
    let f = start("build_recovery", &fake).await;
    let op = operation(&f).await;
    tick(&f).await;
    tick(&f).await;
    assert_eq!(creates(&fake), 1);
    let d = &op.resource_refs["build_dispatch"];
    std::fs::remove_file(fake.dir.join(format!(
            "{}.json",
            d["executor_job_manifest"]["metadata"]["name"]
                .as_str()
                .unwrap()
        )))
    .unwrap();
    tick(&f).await;
    assert_eq!(
        creates(&fake),
        2,
        "only an unadmitted missing executor may be recreated"
    );
    assert_eq!(operation(&f).await.resource_refs, op.resource_refs);
    control(&f, "paused").await;
    assert!(admit(&f, &op).await.is_err());
    control(&f, "active").await;
    let (a, b) = tokio::join!(admit(&f, &op), admit(&f, &op));
    assert_ne!(a.is_ok(), b.is_ok());
    std::fs::remove_file(fake.dir.join(format!(
            "{}.json",
            d["executor_job_manifest"]["metadata"]["name"]
                .as_str()
                .unwrap()
        )))
    .unwrap();
    control(&f, "paused").await;
    tick(&f).await;
    assert_eq!(
        creates(&fake),
        3,
        "only the recorded read-only observer is created after admission"
    );
    assert!(!fake
        .dir
        .join(format!(
            "{}.json",
            d["executor_job_manifest"]["metadata"]["name"]
                .as_str()
                .unwrap()
        ))
        .exists());
    tick(&f).await;
    assert_eq!(creates(&fake), 3);
    std::fs::remove_file(fake.dir.join(format!(
        "{}.json",
        d["observer_job_manifest"]["metadata"]["name"].as_str().unwrap()
    )))
    .unwrap();
    tick(&f).await;
    assert_eq!(
        creates(&fake),
        3,
        "a lost observer cannot receive a fresh execution budget"
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
    let body = observed(&op, true);
    let first = outcome(&f, &op, body.clone()).await.unwrap();
    assert_eq!(first.0["status"], "pipeline_run_succeeded");
    let _ = outcome(&f, &op, body.clone()).await.unwrap();
    let mut conflict = body.clone();
    conflict["pipeline_run"]["metadata"]["uid"] = json!("replacement-uid");
    assert!(outcome(&f, &op, conflict).await.is_err());
    tick(&f).await;
    let complete = f
        .state
        .store
        .get_workflow_operation(&op.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(complete.status, "succeeded");
    assert_eq!(complete.resource_refs["build_result"]["status"], "verified");
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
    control(&f, "active").await;
    tick(&f).await;
    assert_eq!(creates(&fake), 3);
}

#[tokio::test]
async fn failed_or_incomplete_hosted_builds_stop_delivery_and_cannot_use_manual_retry() {
    for (suffix, success, missing) in [
        ("build_failed", false, false),
        ("build_missing_output", true, true),
    ] {
        let fake = KubectlFixture::new(false);
        let f = start(suffix, &fake).await;
        let op = operation(&f).await;
        let _ = admit(&f, &op).await.unwrap();
        let mut body = observed(&op, success);
        if missing {
            body["pipeline_run"]["status"]["results"]
                .as_array_mut()
                .unwrap()
                .retain(|r| r["name"] != "SOURCE_COMMIT");
            body["analysis"]["outputs"]["declared_results"]
                .as_object_mut()
                .unwrap()
                .remove("SOURCE_COMMIT");
            body["analysis"]["outputs"]["source_commit"] = Value::Null;
        }
        let _ = outcome(&f, &op, body.clone()).await.unwrap();
        let _ = outcome(&f, &op, body).await.unwrap();
        tick(&f).await;
        assert_eq!(
            f.state
                .store
                .get_workflow_operation(&op.id)
                .await
                .unwrap()
                .unwrap()
                .resource_refs["build_result"]["status"],
            "blocked"
        );
        tick(&f).await;
        assert_eq!(creates(&fake), 1);
        let id = op.resource_refs["pipeline_intent_id"].as_str().unwrap();
        let retry = crate::app::pipeline::intents::retry_failed_pipeline_intent(
            &f.state,
            id,
            "unit-test".into(),
            "Try an unbound replacement".into(),
        )
        .await;
        assert!(retry.unwrap_err().message.contains("original execution"));
    }
}

#[tokio::test]
async fn hosted_build_admission_revalidates_contract_and_rejects_unbound_callbacks() {
    let fake = KubectlFixture::new(false);
    let f = start("build_admission_scope", &fake).await;
    let op = operation(&f).await;
    let id = op.resource_refs["pipeline_intent_id"].as_str().unwrap();
    let mut wrong = attempt(&op);
    wrong["manifest_hash"] = json!(format!("sha256:{}", "f".repeat(64)));
    assert!(build::internal_build_attempt(
        State(f.state.clone()),
        Path(id.into()),
        Json(serde_json::from_value(wrong).unwrap())
    )
    .await
    .is_err());
    assert!(
        outcome(&f, &op, observed(&op, true)).await.is_err(),
        "a provider observation cannot stand in for autonomous admission"
    );
    let legacy=crate::app::pipeline::execution::internal_pipeline_intent_execution_outcome(State(f.state.clone()),Path(id.into()),Json(serde_json::from_value(json!({"execution_id":op.resource_refs["build_dispatch"]["execution_id"],"status":"completed"})).unwrap())).await;
    assert!(legacy.unwrap_err().message.contains("admitted PipelineRun"));
    f.state
        .store
        .update_pipeline_contract_status(
            "pipeline_test",
            "retired",
            Some("unit-test".into()),
            Some("Retire after dispatch, before creation".into()),
        )
        .await
        .unwrap();
    assert!(admit(&f, &op).await.is_err());
    assert_eq!(creates(&fake), 1);
    assert!(f
        .state
        .store
        .get_artifact(&format!(
            "build_attempt_{}",
            op.resource_refs["build_dispatch"]["execution_id"]
                .as_str()
                .unwrap()
        ))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn expired_original_build_preparation_does_not_create_new_authority_or_jobs() {
    use pharness_store::{BeginWorkflowOperation, FinishWorkflowReconciliation};
    let fake = KubectlFixture::new(false);
    let f = merged_finance_source("build_expired", &fake).await;
    build_transport(&fake);
    let current = crate::app::clock::current_millis() as i64;
    let past = current - 3_700_000;
    let store = &f.state.store;
    store.wake_workflow(&f.work_item_id, past).await.unwrap();
    let claim = store
        .claim_due_workflow("interrupted-api", past, 60_000)
        .await
        .unwrap()
        .unwrap();
    let op = store
        .begin_workflow_operation(
            &claim,
            BeginWorkflowOperation {
                id: "operation_expired_build",
                action: "build_verified_source",
                input_hash: "recorded-expired-input",
                effect: "development",
                resource_keys: &["repository:expired-build"],
            },
            past,
        )
        .await
        .unwrap();
    store
        .finish_workflow_reconciliation(
            &claim,
            FinishWorkflowReconciliation {
                next_due_at: current,
                condition: "waiting",
                reason: "Prior owner stopped before preparation",
                observed_state_hash: None,
            },
            past + 1,
        )
        .await
        .unwrap();
    tick(&f).await;
    tick(&f).await;
    assert_eq!(creates(&fake), 0);
    let unchanged = store.get_workflow_operation(&op.id).await.unwrap().unwrap();
    assert_eq!(unchanged.created_at, past);
    assert_eq!(unchanged.resource_refs, json!({}));
    assert_eq!(
        store
            .get_workflow_reconciliation(&f.work_item_id)
            .await
            .unwrap()
            .unwrap()
            .condition,
        "wait_expired"
    );
    assert!(store
        .list_permission_grants(None, 200)
        .await
        .unwrap()
        .is_empty());
}
