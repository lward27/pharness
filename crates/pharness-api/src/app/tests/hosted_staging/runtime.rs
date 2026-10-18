use super::{
    control, creates, native, now, observed, operation, outcome, plan, save, staging, start, tick,
    HostedStagingAuthority, KubectlFixture, RepoDeliveryFixture, StagingGitOpsPlan,
};
use pharness_core::tools::{
    FinanceApplication, FinanceDeploymentExpectation, FinanceEnvironment, FinanceRuntimeEvidence,
    FinanceVerificationPhase,
};
use serde_json::{json, Value};

async fn committed(
    suffix: &str,
    fake: &KubectlFixture,
) -> (
    RepoDeliveryFixture,
    HostedStagingAuthority,
    StagingGitOpsPlan,
) {
    committed_with_clock(suffix, fake, None).await
}

async fn committed_with_clock(
    suffix: &str,
    fake: &KubectlFixture,
    checked_at: Option<i64>,
) -> (
    RepoDeliveryFixture,
    HostedStagingAuthority,
    StagingGitOpsPlan,
) {
    let (f, _, a) = start(suffix, fake).await;
    let p = plan(&a);
    let _ = save(&f, &a, &p).await.unwrap();
    let baseline = native::evidence(&f, fake, &a, &p, 600_000).await;
    let admitted = staging::seed_historical_admission(
        &f.state,
        &a.deployment_intent_id,
        &a.execution_id,
        baseline,
    )
    .await;
    let mut body = observed(&a, &p);
    body["checked_at_ms"] = json!(checked_at.unwrap_or(admitted + 1_000));
    let _ = outcome(&f, &a, body).await.unwrap();
    (f, a, p)
}

async fn candidate(
    f: &RepoDeliveryFixture,
    fake: &KubectlFixture,
    a: &HostedStagingAuthority,
    age_ms: i64,
) -> FinanceRuntimeEvidence {
    native::evidence_for(
        f,
        fake,
        FinanceDeploymentExpectation {
            application: FinanceApplication::Yfinance,
            environment: FinanceEnvironment::Staging,
            gitops_commit_sha: "e".repeat(40),
            image_digest: a.image_digest.clone(),
        },
        FinanceVerificationPhase::Staging,
        age_ms,
    )
    .await
}

async fn record(f: &RepoDeliveryFixture, a: &HostedStagingAuthority, step: &str) -> Option<Value> {
    f.state
        .store
        .get_artifact(&format!("staging_runtime_{step}_{}", a.execution_id))
        .await
        .unwrap()
        .map(|v| v.content_json.unwrap())
}

#[tokio::test]
async fn candidate_waits_for_exact_argo_image_then_fixes_one_window_even_while_paused() {
    let fake = KubectlFixture::new(false);
    let (f, a, _) = committed("runtime_argo_wait", &fake).await;
    let initial = native::reads(&fake);
    tick(&f).await;
    assert!(
        record(&f, &a, "window").await.is_none(),
        "old image cannot start the candidate window"
    );
    assert_eq!(
        record(&f, &a, "identity_1").await.unwrap()["observation"]["identity_state"],
        "inconclusive"
    );
    assert_eq!(native::reads(&fake), initial + 6);
    let _ = candidate(&f, &fake, &a, 0).await;
    control(&f, "paused").await;
    tick(&f).await;
    let window = record(&f, &a, "window").await.unwrap();
    assert_eq!(window["expected"]["image_digest"], a.image_digest);
    assert_eq!(window["window_record"]["initial_step"], "identity_2");
    assert!(record(&f, &a, "probes").await.is_none());
    assert!(record(&f, &a, "result").await.is_none());
    assert_eq!(operation(&f).await.status, "running");
    assert_eq!(
        creates(&fake),
        2,
        "observation while paused starts no new writer or build"
    );
}

#[tokio::test]
async fn candidate_missing_telemetry_keeps_original_evidence_and_blocks_promotion() {
    let fake = KubectlFixture::new(false);
    let (f, a, _) = committed("runtime_no_telemetry", &fake).await;
    let evidence = candidate(&f, &fake, &a, 0).await;
    staging::seed_runtime(
        &f.state,
        &a.deployment_intent_id,
        &a.execution_id,
        evidence,
        false,
        None,
    )
    .await;
    let reads = native::reads(&fake);
    tick(&f).await;
    let first = record(&f, &a, "result").await.unwrap();
    assert_eq!(first["assessment"]["runtime_verification"], "inconclusive");
    assert_eq!(
        first["runtime_evidence"]["identity_after"]["identity_state"],
        "verified"
    );
    assert_eq!(native::reads(&fake), reads + 6);
    let op = operation(&f).await;
    assert_eq!(
        op.resource_refs["staging_runtime_result"]["status"],
        "blocked"
    );
    assert!(op
        .resource_keys
        .contains(&"gitops:lucas_engineering:main".into()));
    tick(&f).await;
    assert_eq!(record(&f, &a, "result").await.unwrap(), first);
    assert_eq!(native::reads(&fake), reads + 6);
}

#[tokio::test]
async fn verified_candidate_closes_only_staging_and_does_not_repeat_work_after_restart() {
    let fake = KubectlFixture::new(false);
    let (f, a, p) = committed("runtime_verified", &fake).await;
    let evidence = candidate(&f, &fake, &a, 90_000).await;
    let end = evidence.window.end_unix_seconds * 1000;
    assert!(
        now() as u64 > end + 60_000,
        "historical result is validated at collection time, not falsely renewed"
    );
    staging::seed_runtime(
        &f.state,
        &a.deployment_intent_id,
        &a.execution_id,
        evidence,
        true,
        None,
    )
    .await;
    let reads = native::reads(&fake);
    tick(&f).await;
    assert!(f
        .state
        .store
        .active_workflow_operation(&f.work_item_id)
        .await
        .unwrap()
        .is_none());
    let op = f
        .state
        .store
        .get_workflow_operation(&a.operation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(op.status, "succeeded");
    assert_eq!(
        op.resource_refs["staging_runtime_window"]["artifact_id"],
        format!("staging_runtime_window_{}", a.execution_id),
        "a partially persisted original window link is repaired before completion"
    );
    assert_eq!(
        op.resource_refs["staging_runtime_result"]["status"],
        "verified"
    );
    assert_eq!(
        op.resource_refs["staging_runtime_result"]["production_authorized"],
        false
    );
    assert!(f
        .state
        .store
        .get_repo_work_item_metadata(&f.work_item_id)
        .await
        .unwrap()
        .unwrap()
        .closed_at
        .is_none());
    assert!(f
        .state
        .store
        .get_deployment_intent_for_stage(
            &a.pipeline_intent_id,
            pharness_store::DeliveryStage::Production
        )
        .await
        .unwrap()
        .is_none());
    tick(&f).await;
    tick(&f).await;
    assert_eq!(native::reads(&fake), reads);
    assert_eq!(creates(&fake), 2);
    let state = f
        .state
        .store
        .get_workflow_reconciliation(&f.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.condition, "waiting", "{}", state.condition_reason);
    assert!(state.condition_reason.contains("Production"));
    let before = f
        .state
        .store
        .get_deployment_intent(&a.deployment_intent_id)
        .await
        .unwrap()
        .unwrap();
    let _ = outcome(&f, &a, observed(&a, &p)).await.unwrap();
    let after = f
        .state
        .store
        .get_deployment_intent(&a.deployment_intent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        before.intent_json, after.intent_json,
        "duplicate commit callbacks preserve the separate runtime result"
    );
    let original_result = record(&f, &a, "result").await.unwrap();
    let mut pipeline = f
        .state
        .store
        .get_pipeline_intent(&a.pipeline_intent_id)
        .await
        .unwrap()
        .unwrap();
    pipeline.intent_json["source_provenance"]["merge_commit_sha"] = json!("7".repeat(40));
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
    tick(&f).await;
    let state = f
        .state
        .store
        .get_workflow_reconciliation(&f.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.condition, "blocked", "{}", state.condition_reason);
    assert_eq!(record(&f, &a, "result").await.unwrap(), original_result);
    assert_eq!(creates(&fake), 2);
    assert_eq!(native::reads(&fake), reads);
}

#[tokio::test]
async fn candidate_waits_for_a_bounded_future_worker_clock_before_reading() {
    let fake = KubectlFixture::new(false);
    let checked = now() + 25_000;
    let (f, a, _) = committed_with_clock("runtime_clock", &fake, Some(checked)).await;
    let _ = candidate(&f, &fake, &a, 0).await;
    let reads = native::reads(&fake);
    assert!(now() < checked);
    tick(&f).await;
    assert!(record(&f, &a, "identity_1").await.is_none());
    assert!(record(&f, &a, "window").await.is_none());
    assert_eq!(native::reads(&fake), reads);
    assert_eq!(operation(&f).await.status, "running");
}

#[tokio::test]
async fn a_candidate_window_cannot_precede_its_recorded_gitops_commit() {
    let fake = KubectlFixture::new(false);
    let (f, a, _) = committed("runtime_precommit_window", &fake).await;
    let evidence = candidate(&f, &fake, &a, 600_000).await;
    staging::seed_runtime_step(
        &f.state,
        &a.deployment_intent_id,
        &a.execution_id,
        "window",
        json!({"window_record":{
            "created_at_ms":evidence.window.start_unix_seconds as i64*1000-30_001,
            "window":evidence.window,"initial_step":"identity_1"
        }}),
    )
    .await;
    let reads = native::reads(&fake);
    tick(&f).await;
    let state = f
        .state
        .store
        .get_workflow_reconciliation(&f.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.condition, "blocked");
    assert!(
        state.condition_reason.contains("original operation"),
        "{}",
        state.condition_reason
    );
    assert!(record(&f, &a, "probes").await.is_none());
    assert!(record(&f, &a, "result").await.is_none());
    assert_eq!(native::reads(&fake), reads);
    assert_eq!(creates(&fake), 2);
}

#[tokio::test]
async fn interrupted_candidate_final_collection_is_inconclusive_without_repeating_queries() {
    let fake = KubectlFixture::new(false);
    let (f, a, _) = committed("runtime_interrupted", &fake).await;
    let evidence = candidate(&f, &fake, &a, 0).await;
    staging::seed_runtime(
        &f.state,
        &a.deployment_intent_id,
        &a.execution_id,
        evidence,
        false,
        None,
    )
    .await;
    staging::seed_runtime_step(
        &f.state,
        &a.deployment_intent_id,
        &a.execution_id,
        "complete_started",
        json!({"started_at_ms":now()-1}),
    )
    .await;
    let reads = native::reads(&fake);
    tick(&f).await;
    let first = record(&f, &a, "result").await.unwrap();
    assert_eq!(first["assessment"]["runtime_verification"], "inconclusive");
    assert_eq!(
        first["assessment"]["reasons"],
        json!(["staging_final_collection_interrupted"])
    );
    tick(&f).await;
    assert_eq!(record(&f, &a, "result").await.unwrap(), first);
    assert_eq!(native::reads(&fake), reads);
    assert_eq!(creates(&fake), 2);
    assert_eq!(operation(&f).await.status, "running");
}

#[tokio::test]
async fn candidate_identity_attempts_cannot_exceed_the_original_wait_allowance() {
    let fake = KubectlFixture::new(false);
    let (f, a, _) = committed("runtime_allowance", &fake).await;
    let count = crate::app::CONTROLLER_WAIT_MAX_CHECKS;
    staging::seed_runtime_identity_attempts(
        &f.state,
        &a.deployment_intent_id,
        &a.execution_id,
        count,
    )
    .await;
    let reads = native::reads(&fake);
    tick(&f).await;
    let first = record(&f, &a, "result").await.unwrap();
    assert_eq!(
        first["assessment"]["reasons"],
        json!(["staging_identity_observation_allowance_exhausted"])
    );
    assert!(record(&f, &a, &format!("identity_{}", count + 1))
        .await
        .is_none());
    tick(&f).await;
    assert_eq!(record(&f, &a, "result").await.unwrap(), first);
    assert_eq!(native::reads(&fake), reads);
    assert_eq!(creates(&fake), 2);
}

#[tokio::test]
async fn a_passed_candidate_summary_cannot_override_mixed_native_receipts() {
    let fake = KubectlFixture::new(false);
    let (f, a, _) = committed("runtime_mixed", &fake).await;
    let evidence = candidate(&f, &fake, &a, 0).await;
    staging::seed_runtime(
        &f.state,
        &a.deployment_intent_id,
        &a.execution_id,
        evidence,
        true,
        Some((
            "/runtime_evidence/signals/identity_after_sha256",
            json!("sha256:wrong"),
        )),
    )
    .await;
    tick(&f).await;
    assert_eq!(operation(&f).await.status, "running");
    let state = f
        .state
        .store
        .get_workflow_reconciliation(&f.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.condition, "blocked");
    assert!(state.condition_reason.contains("assessment"));
}
