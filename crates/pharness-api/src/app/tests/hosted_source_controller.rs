use super::repo_mode_v1::{repo_fixture_with_policy, RepoDeliveryFixture};
use crate::app::{clock::current_millis, hosted_controller::reconcile_once};
use crate::dispatch::{KubectlFixture, SourceJobKind};
use pharness_core::{RunId, SessionId};
use pharness_store::{
    BeginWorkflowOperation, CreateArtifact, CreateChangeSet, CreateRun, CreateSourceDeliveryIntent,
    FinishWorkflowReconciliation, StoredChangeSet,
};
use serde_json::{json, Value};
use sha2::Digest;

async fn fixture(suffix: &str, fake: &KubectlFixture) -> (RepoDeliveryFixture, StoredChangeSet) {
    let source = format!("https://github.com/example/repo-{suffix}.git");
    let state =
        super::characterization::test_state_with_git_observer(fake.command.clone(), source.clone())
            .await;
    let policy = serde_json::from_str(include_str!(
        "../../../../pharness-core/tests/fixtures/hosted-workflow.json"
    ))
    .unwrap();
    let fixture = repo_fixture_with_policy(suffix, false, state, Some(policy)).await;
    let store = &fixture.state.store;
    let run = store
        .create_run(CreateRun {
            id: RunId::new(format!("run_{suffix}")),
            session_id: SessionId::new(format!("ses_{suffix}")),
            user_task: "Approved source fixture".into(),
            cwd: "/workspace".into(),
            max_turns: 2,
            initial_status: "completed".into(),
            execution_target_json: json!({}),
        })
        .await
        .unwrap();
    let diff = "diff --git a/src/app.py b/src/app.py\n--- a/src/app.py\n+++ b/src/app.py\n@@ -1 +1 @@\n-old\n+new\n";
    let patch = store
        .create_artifact(CreateArtifact {
            id: format!("patch_{suffix}"),
            session_id: run.session_id.clone(),
            run_id: Some(run.id.clone()),
            kind: "workspace_git_diff".into(),
            label: "Exact approved source fixture".into(),
            mime_type: Some("text/x-diff".into()),
            path: None,
            content_text: Some(diff.into()),
            content_json: None,
        })
        .await
        .unwrap();
    let plan = store
        .get_work_plan_by_work_item(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    let change = store.create_change_set(CreateChangeSet {
        id: format!("change_{suffix}"), work_item_id: Some(fixture.work_item_id.clone()), work_plan_id: plan.id,
        remediation_plan_id: None, incident_id: None, session_id: run.session_id.clone(), run_id: Some(run.id),
        status: "approved".into(), title: "Approved source fixture".into(), summary: "Exact patch".into(),
        risk_level: "low".into(), material_hash: format!("sha256:{}", "b".repeat(64)),
        resource_namespace: None, resource_kind: Some("Repository".into()), resource_name: Some(source),
        change_set_json: json!({"patch":{"artifact_id":patch.id,"hash":format!("sha256:{:x}",sha2::Sha256::digest(diff.as_bytes()))}}),
    }).await.unwrap();
    (fixture, change)
}

async fn tick(fixture: &RepoDeliveryFixture) {
    fixture
        .state
        .store
        .wake_workflow(&fixture.work_item_id, current_millis() as i64)
        .await
        .unwrap();
    assert!(reconcile_once(&fixture.state, "replacement-api")
        .await
        .unwrap());
}

async fn checkpoint(fixture: &RepoDeliveryFixture, change: &StoredChangeSet, age: i64) -> String {
    let store = &fixture.state.store;
    let metadata = store
        .get_repo_work_item_metadata(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    let item = store
        .get_work_item(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    let id = format!("intent_{}", fixture.work_item_id);
    store.create_source_delivery_intent(CreateSourceDeliveryIntent {
        id: id.clone(), subject_kind: "work_item_change_set".into(), subject_id: change.id.clone(),
        repository_id: metadata.repository_id.clone(), source_repo: item.source_repo,
        base_ref: "main".into(), base_commit: item.source_commit.unwrap(), head_branch: "pharness/checkpoint".into(),
        patch_artifact_id: change.change_set_json["patch"]["artifact_id"].as_str().map(str::to_owned),
        patch_hash: change.change_set_json["patch"]["hash"].as_str().unwrap().into(),
        authorization: json!({"workflow_policy_hash":metadata.workflow_policy_hash,"work_item_id":fixture.work_item_id,"writer_execution_id":"srcexec_planned"}),
        created_by: "controller:hosted-workflow".into(), creation_reason: "Recorded before interrupted dispatch".into(),
    }).await.unwrap();
    let time = current_millis() as i64 - age;
    store
        .wake_workflow(&fixture.work_item_id, time)
        .await
        .unwrap();
    let claim = store
        .claim_due_workflow("departed-api", time, 60_000)
        .await
        .unwrap()
        .unwrap();
    let key = format!("repository:{}", metadata.repository_id);
    let op = store
        .begin_workflow_operation(
            &claim,
            BeginWorkflowOperation {
                id: "source-checkpoint",
                action: "authorize_source_delivery",
                input_hash: "recorded-source-hash",
                effect: "development",
                resource_keys: &[&key],
            },
            time,
        )
        .await
        .unwrap();
    store
        .record_workflow_operation(
            &claim,
            &op.id,
            "running",
            &json!({"action_resource":change.id,"before_run_ids":[]}),
            "Interrupted after intent creation",
            time,
        )
        .await
        .unwrap();
    store
        .finish_workflow_reconciliation(
            &claim,
            FinishWorkflowReconciliation {
                next_due_at: time + 1,
                condition: "waiting",
                reason: "Interrupted fixture",
                observed_state_hash: None,
            },
            time,
        )
        .await
        .unwrap();
    id
}

#[tokio::test]
async fn hosted_source_controller_automatically_publishes_once_under_saved_authority() {
    let fake = KubectlFixture::new(false);
    let (fixture, change) = fixture("source_automatic", &fake).await;
    let original = fixture
        .state
        .store
        .get_work_item(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    tick(&fixture).await;
    let intent = fixture
        .state
        .store
        .get_source_delivery_intent_by_subject("work_item_change_set", &change.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(intent.status, "writer_dispatched");
    let operation = fixture
        .state
        .store
        .active_workflow_operation(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(operation.action, "authorize_source_delivery");
    assert_eq!(
        operation.resource_refs["source_delivery_intent_id"],
        intent.id
    );
    for _ in 0..3 {
        tick(&fixture).await;
    }
    assert_eq!(fake.creates(), 1);
    assert_eq!(
        fixture
            .state
            .store
            .get_source_delivery_intent(&intent.id)
            .await
            .unwrap()
            .unwrap(),
        intent
    );
    let after = fixture
        .state
        .store
        .get_work_item(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.run_budget, original.run_budget);
    assert_eq!(after.attempt_count, original.attempt_count);
    assert!(fixture
        .state
        .store
        .get_repo_work_item_metadata(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap()
        .closed_at
        .is_none());
    let mut job: Value =
        serde_json::from_slice(&std::fs::read(fake.dir.join("job.json")).unwrap()).unwrap();
    job["status"] = json!({"succeeded":1});
    std::fs::write(fake.dir.join("job.json"), job.to_string()).unwrap();
    tick(&fixture).await;
    let state = fixture
        .state
        .store
        .get_workflow_reconciliation(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.condition, "blocked");
    assert!(state
        .condition_reason
        .contains("without its outcome recorded"));
    assert_eq!(fake.creates(), 1);
}

#[tokio::test]
async fn hosted_source_writer_callback_preserves_operation_identity_during_observation() {
    let fake = KubectlFixture::new(false);
    let (fixture, change) = fixture("source_callback_progression", &fake).await;
    tick(&fixture).await;
    let store = &fixture.state.store;
    let intent = store
        .get_source_delivery_intent_by_subject("work_item_change_set", &change.id)
        .await
        .unwrap()
        .unwrap();
    let before = store
        .active_workflow_operation(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    let axum::Json(callback) = crate::app::repo_mode::internal_source_delivery_writer_outcome(
        axum::extract::State(fixture.state.clone()),
        axum::extract::Path(intent.id.clone()),
        axum::Json(crate::dto::GitDeliveryOutcomeRequest {
            execution_id: intent.writer_execution_id.clone().unwrap(),
            status: "completed".into(),
            branch: Some(intent.head_branch.clone()),
            commit_sha: Some("e".repeat(40)),
            pull_request_url: Some(format!(
                "{}/pull/7",
                intent.source_repo.trim_end_matches(".git")
            )),
            pull_request_number: Some(7),
            error_code: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        callback["source_delivery_intent"]["status"],
        "pull_request_open"
    );
    tick(&fixture).await;
    let state = store
        .get_workflow_reconciliation(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.condition, "waiting", "{}", state.condition_reason);
    let after = store
        .active_workflow_operation(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(before.id, after.id);
    assert_eq!(before.resource_refs, after.resource_refs);
    assert_eq!(after.status, "running");
    assert_eq!(fake.creates(), 1);
    assert_eq!(
        store
            .get_source_delivery_intent(&intent.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "pull_request_open"
    );
}

#[tokio::test]
async fn hosted_source_checkpoint_recovers_original_identity_but_pause_and_expiry_withhold_writes()
{
    for control in ["active", "paused", "cancelled", "expired"] {
        let fake = KubectlFixture::new(false);
        let (fixture, change) = fixture(&format!("source_checkpoint_{control}"), &fake).await;
        let age = if control == "expired" { 3_600_001 } else { 0 };
        let id = checkpoint(&fixture, &change, age).await;
        if matches!(control, "paused" | "cancelled") {
            fixture
                .state
                .store
                .set_workflow_control(
                    &fixture.work_item_id,
                    1,
                    control,
                    "operator",
                    "Pause before missing writer recovery",
                    current_millis() as i64,
                )
                .await
                .unwrap();
        }
        tick(&fixture).await;
        let intent = fixture
            .state
            .store
            .get_source_delivery_intent(&id)
            .await
            .unwrap()
            .unwrap();
        let state = fixture
            .state
            .store
            .get_workflow_reconciliation(&fixture.work_item_id)
            .await
            .unwrap()
            .unwrap();
        if control == "active" {
            assert_eq!(
                intent.writer_execution_id.as_deref(),
                Some("srcexec_planned")
            );
            assert_eq!(intent.status, "writer_dispatched");
            tick(&fixture).await;
            assert_eq!(fake.creates(), 1);
        } else {
            assert_eq!(intent.status, "authorized");
            assert!(intent.writer_execution_id.is_none());
            assert_eq!(fake.creates(), 0);
            assert_eq!(
                state.condition,
                if control == "expired" {
                    "wait_expired"
                } else {
                    control
                }
            );
        }
    }
}

#[tokio::test]
async fn hosted_source_job_observation_checks_identity_and_never_recreates_terminal_jobs() {
    for kind in [SourceJobKind::Writer, SourceJobKind::Observer] {
        let fake = KubectlFixture::new(false);
        let (fixture, change) = fixture(&format!("source_job_{kind:?}"), &fake).await;
        let id = checkpoint(&fixture, &change, 0).await;
        let intent = fixture
            .state
            .store
            .get_source_delivery_intent(&id)
            .await
            .unwrap()
            .unwrap();
        fixture
            .state
            .store
            .update_source_delivery_intent(
                &id,
                intent.state_version,
                if kind == SourceJobKind::Writer {
                    "writer_dispatched"
                } else {
                    "observer_dispatched"
                },
                Some("srcexec_planned"),
                Some("srcobserve_planned"),
                None,
                None,
                None,
                "fixture",
                "Persist original execution",
            )
            .await
            .unwrap();
        let execution = if kind == SourceJobKind::Writer {
            "srcexec_planned"
        } else {
            "srcobserve_planned"
        };
        let worker = &fixture.state.worker;
        assert_eq!(
            worker
                .reconcile_source_delivery_job(&id, execution, kind, false)
                .await
                .unwrap()
                .status,
            "missing"
        );
        assert_eq!(fake.creates(), 0);
        assert!(worker
            .reconcile_source_delivery_job(&id, "another-execution", kind, true)
            .await
            .is_err());
        assert_eq!(fake.creates(), 0);
        assert_eq!(
            worker
                .reconcile_source_delivery_job(&id, execution, kind, true)
                .await
                .unwrap()
                .status,
            "active"
        );
        for status in ["succeeded", "failed"] {
            let mut job: Value =
                serde_json::from_slice(&std::fs::read(fake.dir.join("job.json")).unwrap()).unwrap();
            job["status"] = json!({status:1});
            std::fs::write(fake.dir.join("job.json"), job.to_string()).unwrap();
            assert_eq!(
                worker
                    .reconcile_source_delivery_job(&id, execution, kind, true)
                    .await
                    .unwrap()
                    .status,
                status
            );
            assert_eq!(fake.creates(), 1);
        }
    }
}

#[tokio::test]
async fn hosted_source_callback_does_not_reset_expired_wait_or_close_the_work_item() {
    for status in [
        "pull_request_open",
        "waiting_checks",
        "waiting_merge",
        "head_drift",
        "failed",
        "pull_request_closed",
        "merged",
    ] {
        let fake = KubectlFixture::new(false);
        let (fixture, change) = fixture(&format!("source_callback_{status}"), &fake).await;
        let id = checkpoint(&fixture, &change, 3_600_001).await;
        let store = &fixture.state.store;
        let intent = store
            .get_source_delivery_intent(&id)
            .await
            .unwrap()
            .unwrap();
        // Simulate a callback arriving after the original operation's deadline.
        // A new intent update must not start another hour of observation.
        store.update_source_delivery_intent(
            &id, intent.state_version, status, Some("srcexec_planned"),
            Some("srcobserve_late"),
            Some(&json!({"number":7,"head_sha":"b".repeat(40),"head_branch":"pharness/checkpoint"})),
            None, None, "callback-fixture", "Late recorded source outcome",
        ).await.unwrap();
        tick(&fixture).await;
        let state = store
            .get_workflow_reconciliation(&fixture.work_item_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fake.creates(), 0);
        let terminal = matches!(status, "merged" | "failed" | "pull_request_closed");
        assert_eq!(
            store
                .active_workflow_operation(&fixture.work_item_id)
                .await
                .unwrap()
                .is_none(),
            terminal,
        );
        assert_eq!(
            state.condition,
            match status {
                "merged" | "failed" | "head_drift" | "pull_request_closed" => "blocked",
                _ => "wait_expired",
            }
        );
        assert!(store
            .get_repo_work_item_metadata(&fixture.work_item_id)
            .await
            .unwrap()
            .unwrap()
            .closed_at
            .is_none());
        assert_eq!(
            store
                .get_source_delivery_intent(&id)
                .await
                .unwrap()
                .unwrap()
                .status,
            status
        );
    }
}
