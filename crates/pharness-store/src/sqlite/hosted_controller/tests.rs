use super::*;
use serde_json::json;

async fn seed(store: &SqliteStore, id: &str, hosted: bool) {
    sqlx::query(
        "INSERT INTO work_items(id,status,title,intent,acceptance_criteria_json,source_repo,
         source_ref,target_environment,production_impacting,max_attempts,max_elapsed_seconds,
         created_at,updated_at,status_changed_at,mode,state_version,workflow_policy_json,workflow_policy_hash)
         VALUES(?1,'submitted','Fixture','Bounded request','[]','https://github.com/example/app.git',
         'main','repository',0,2,3600,'100','100','100','repo',1,?2,?3)",
    )
    .bind(id)
    .bind(hosted.then_some("{\"schema_version\":\"pharness.dev/hosted-workflow/v1alpha1\"}"))
    .bind(hosted.then_some("sha256:fixture"))
    .execute(&store.pool).await.unwrap();
}

fn operation<'a>(id: &'a str, keys: &'a [&'a str]) -> BeginWorkflowOperation<'a> {
    BeginWorkflowOperation {
        id,
        action: "start_planner",
        input_hash: "sha256:input",
        effect: "development",
        resource_keys: keys,
    }
}

#[tokio::test]
async fn due_claims_are_exclusive_fenced_and_reads_do_not_enroll_legacy_work() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, "legacy", false).await;
    seed(&store, "hosted", true).await;
    assert!(store
        .get_workflow_reconciliation("legacy")
        .await
        .unwrap()
        .is_none());
    assert!(store
        .claim_due_workflow("a", 99, 10)
        .await
        .unwrap()
        .is_none());
    let a = store
        .claim_due_workflow("a", 100, 10)
        .await
        .unwrap()
        .unwrap();
    assert!(store
        .claim_due_workflow("b", 101, 10)
        .await
        .unwrap()
        .is_none());
    let b = store
        .claim_due_workflow("b", 110, 10)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(b.claim_fence, a.claim_fence + 1);
    let finish = || FinishWorkflowReconciliation {
        next_due_at: 125,
        condition: "waiting",
        reason: "waiting for recorded work",
        observed_state_hash: Some("sha256:state"),
    };
    assert!(store
        .finish_workflow_reconciliation(&a, finish(), 111)
        .await
        .is_err());
    store
        .finish_workflow_reconciliation(&b, finish(), 111)
        .await
        .unwrap();
    assert!(store
        .claim_due_workflow("c", 124, 10)
        .await
        .unwrap()
        .is_none());
    let c = store
        .claim_due_workflow("c", 125, 10)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(c.observed_state_hash.as_deref(), Some("sha256:state"));
    assert_eq!(c.unchanged_checks, 0);
    store
        .finish_workflow_reconciliation(
            &c,
            FinishWorkflowReconciliation {
                next_due_at: 150,
                condition: "waiting",
                reason: "unchanged",
                observed_state_hash: Some("sha256:state"),
            },
            126,
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .get_workflow_reconciliation("hosted")
            .await
            .unwrap()
            .unwrap()
            .unchanged_checks,
        1
    );
    assert!(store.claim_due_workflow("", 150, 10).await.is_err());
    assert!(store.claim_due_workflow("a", 150, 60_001).await.is_err());
}

#[tokio::test]
async fn expired_claim_does_not_release_global_or_repository_locks() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, "a", true).await;
    seed(&store, "b", true).await;
    let a = store
        .claim_due_workflow("first", 100, 10)
        .await
        .unwrap()
        .unwrap();
    let b = store
        .claim_due_workflow("second", 100, 10)
        .await
        .unwrap()
        .unwrap();
    let keys = ["coding", "repository:example/app:staging"];
    let first = store
        .begin_workflow_operation(&a, operation("op_a", &keys), 101)
        .await
        .unwrap();
    assert!(store
        .begin_workflow_operation(&b, operation("op_b", &keys), 101)
        .await
        .is_err());
    assert!(store
        .active_workflow_operation("b")
        .await
        .unwrap()
        .is_none());
    let resumed = store
        .claim_due_workflow("restart", 110, 20)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resumed.work_item_id, "a");
    assert_eq!(
        store
            .begin_workflow_operation(&resumed, operation("op_a", &keys), 111)
            .await
            .unwrap(),
        first
    );
    let second = store
        .claim_due_workflow("restart", 110, 20)
        .await
        .unwrap()
        .unwrap();
    assert!(store
        .begin_workflow_operation(&second, operation("op_b", &keys), 111)
        .await
        .is_err());
    store
        .record_workflow_operation(
            &resumed,
            "op_a",
            "blocked",
            &json!({"run_id":"run_a"}),
            "outcome unknown",
            112,
        )
        .await
        .unwrap();
    assert!(store
        .begin_workflow_operation(&second, operation("op_b", &keys), 113)
        .await
        .is_err());
    store
        .record_workflow_operation(
            &resumed,
            "op_a",
            "succeeded",
            &json!({"run_id":"run_a"}),
            "recorded run reconciled terminal",
            114,
        )
        .await
        .unwrap();
    store
        .begin_workflow_operation(&second, operation("op_b", &keys), 115)
        .await
        .unwrap();
}

#[tokio::test]
async fn pause_and_cancellation_fence_dispatch_but_preserve_observation() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, "hosted", true).await;
    let active = store
        .claim_due_workflow("api", 100, 30)
        .await
        .unwrap()
        .unwrap();
    let paused = store
        .set_workflow_control("hosted", 1, "paused", "operator", "pause development", 101)
        .await
        .unwrap();
    assert_eq!(paused.control_version, 2);
    assert!(store
        .begin_workflow_operation(&active, operation("op", &[]), 102)
        .await
        .is_err());
    let claim = store
        .claim_due_workflow("api", 102, 30)
        .await
        .unwrap()
        .unwrap();
    let mut forged = claim.clone();
    forged.control = "active".into();
    assert!(store
        .begin_workflow_operation(&forged, operation("op", &[]), 103)
        .await
        .is_err());
    assert!(store
        .begin_workflow_operation(&claim, operation("op", &[]), 103)
        .await
        .is_err());
    store
        .begin_workflow_operation(
            &claim,
            BeginWorkflowOperation {
                id: "observe",
                action: "observe_existing_release",
                input_hash: "sha256:release",
                effect: "observation",
                resource_keys: &[],
            },
            103,
        )
        .await
        .unwrap();
    store
        .record_workflow_operation(
            &claim,
            "observe",
            "succeeded",
            &json!({"release_id":"release"}),
            "observation preserved",
            104,
        )
        .await
        .unwrap();
    assert!(store
        .set_workflow_control("hosted", 1, "active", "operator", "stale resume", 105)
        .await
        .is_err());
    let resumed = store
        .set_workflow_control("hosted", 2, "active", "operator", "resume", 105)
        .await
        .unwrap();
    assert_eq!(resumed.control_version, 3);
    store
        .set_workflow_control("hosted", 3, "cancelled", "operator", "stop new work", 106)
        .await
        .unwrap();
    assert!(store
        .set_workflow_control(
            "hosted",
            4,
            "active",
            "operator",
            "cannot undo cancellation",
            107
        )
        .await
        .is_err());
    let claim = store
        .claim_due_workflow("api", 108, 30)
        .await
        .unwrap()
        .unwrap();
    assert!(store
        .begin_workflow_operation(&claim, operation("op", &[]), 109)
        .await
        .is_err());
    store
        .begin_workflow_operation(
            &claim,
            BeginWorkflowOperation {
                id: "recover",
                action: "recover_approved_release",
                input_hash: "sha256:recovery",
                effect: "recovery",
                resource_keys: &[],
            },
            109,
        )
        .await
        .unwrap();
    let audit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE kind='hosted.workflow_control'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap();
    assert_eq!(audit_count, 3);
}

#[tokio::test]
async fn operation_identity_and_completed_outcome_are_repeat_safe() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, "hosted", true).await;
    let claim = store
        .claim_due_workflow("api", 100, 30)
        .await
        .unwrap()
        .unwrap();
    let original = store
        .begin_workflow_operation(&claim, operation("op", &[]), 101)
        .await
        .unwrap();
    assert_eq!(
        store
            .begin_workflow_operation(&claim, operation("op", &[]), 102)
            .await
            .unwrap(),
        original
    );
    assert!(store
        .begin_workflow_operation(&claim, operation("different_id", &[]), 102)
        .await
        .is_err());
    store
        .record_workflow_operation(
            &claim,
            "op",
            "running",
            &json!({"run_id":"original"}),
            "dispatched",
            103,
        )
        .await
        .unwrap();
    assert!(store
        .record_workflow_operation(
            &claim,
            "op",
            "running",
            &json!({"run_id":"replacement"}),
            "cannot replace",
            104
        )
        .await
        .is_err());
    let done = store
        .record_workflow_operation(
            &claim,
            "op",
            "succeeded",
            &json!({"run_id":"original","stage_id":"stage"}),
            "terminal evidence observed",
            105,
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .record_workflow_operation(
                &claim,
                "op",
                "succeeded",
                &done.resource_refs,
                "duplicate callback",
                106
            )
            .await
            .unwrap(),
        done
    );
    assert!(store
        .record_workflow_operation(
            &claim,
            "op",
            "running",
            &done.resource_refs,
            "cannot reopen",
            107
        )
        .await
        .is_err());
    assert!(store
        .record_workflow_operation(
            &claim,
            "op",
            "succeeded",
            &json!({"run_id":"original","stage_id":"stage","other":"new"}),
            "cannot rewrite",
            107
        )
        .await
        .is_err());
}

#[tokio::test]
async fn reopening_sqlite_recovers_dispatch_identity_without_freeing_external_locks() {
    let path = std::env::temp_dir().join(format!(
        "pharness-hosted-restart-{}-{}.db",
        std::process::id(),
        crate::sqlite::now_string()
    ));
    let store = SqliteStore::connect(&path).await.unwrap();
    seed(&store, "hosted", true).await;
    let claim = store
        .claim_due_workflow("before_restart", 100, 10)
        .await
        .unwrap()
        .unwrap();
    store
        .begin_workflow_operation(&claim, operation("stable_dispatch", &["coding"]), 101)
        .await
        .unwrap();
    store
        .record_workflow_operation(
            &claim,
            "stable_dispatch",
            "running",
            &json!({"run_id":"same_run"}),
            "dispatch acknowledged",
            102,
        )
        .await
        .unwrap();
    store.pool.close().await;
    drop(store);
    let restarted = SqliteStore::connect(&path).await.unwrap();
    let claim = restarted
        .claim_due_workflow("after_restart", 110, 10)
        .await
        .unwrap()
        .unwrap();
    let recovered = restarted
        .begin_workflow_operation(&claim, operation("stable_dispatch", &["coding"]), 111)
        .await
        .unwrap();
    assert_eq!(recovered.status, "running");
    assert_eq!(recovered.resource_refs, json!({"run_id":"same_run"}));
    let lock: String = sqlx::query_scalar(
        "SELECT operation_id FROM hosted_operation_locks WHERE resource_key='coding'",
    )
    .fetch_one(&restarted.pool)
    .await
    .unwrap();
    assert_eq!(lock, "stable_dispatch");
    restarted.pool.close().await;
    std::fs::remove_file(path).unwrap();
}
