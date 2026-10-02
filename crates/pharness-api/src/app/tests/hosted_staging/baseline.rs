use super::{
    admit, context, control, creates, native, now, operation, plan, save, staging, start, tick,
    HostedStagingAuthority, KubectlFixture, RepoDeliveryFixture,
};
use serde_json::{json, Value};

#[tokio::test]
async fn missing_telemetry_is_persisted_as_inconclusive_without_a_second_collection() {
    let fake = KubectlFixture::new(false);
    let (f, _, a) = start("baseline_no_telemetry", &fake).await;
    let p = plan(&a);
    let _ = save(&f, &a, &p).await.unwrap();
    let evidence = native::evidence(&f, &fake, &a, &p, 0).await;
    staging::seed_inputs(
        &f.state,
        &a.deployment_intent_id,
        &a.execution_id,
        &evidence,
    )
    .await;
    let reads = native::reads(&fake);
    tick(&f).await;
    let result = artifact(&f, &a, "result")
        .await
        .unwrap()
        .content_json
        .unwrap();
    assert_eq!(result["assessment"]["runtime_verification"], "inconclusive");
    assert_eq!(
        result["runtime_evidence"]["identity_after"]["identity_state"],
        "verified"
    );
    assert_eq!(
        result["runtime_evidence"]["signals"]["signal_state"],
        "inconclusive"
    );
    assert_eq!(native::reads(&fake), reads + 6);
    tick(&f).await;
    assert_eq!(
        artifact(&f, &a, "result").await.unwrap().content_json,
        Some(result)
    );
    assert_eq!(native::reads(&fake), reads + 6);
    assert_eq!(
        context(&f, &a, false).await.unwrap().0["may_advance"],
        false
    );
    assert!(admit(&f, &a, &p).await.is_err());
}

#[tokio::test]
async fn an_agent_claim_with_matching_hashes_cannot_substitute_for_native_origin() {
    let fake = KubectlFixture::new(false);
    let (f, _, a) = start("baseline_claim_origin", &fake).await;
    let p = plan(&a);
    let _ = save(&f, &a, &p).await.unwrap();
    let evidence = native::evidence(&f, &fake, &a, &p, 0).await;
    staging::seed_inputs(
        &f.state,
        &a.deployment_intent_id,
        &a.execution_id,
        &evidence,
    )
    .await;
    let pipeline = f
        .state
        .store
        .get_pipeline_intent(&a.pipeline_intent_id)
        .await
        .unwrap()
        .unwrap();
    f.state.store.create_artifact(pharness_store::CreateArtifact {
        id:format!("staging_baseline_result_{}",a.execution_id),session_id:pipeline.session_id,run_id:pipeline.run_id,
        kind:"agent_submitted_claim".into(),label:"Synthetic origin-confusion fixture".into(),mime_type:Some("application/json".into()),path:None,content_text:None,
        content_json:Some(json!({"authority_hash":a.material_hash().unwrap(),"plan_hash":p.material_hash().unwrap(),"assessment":evidence.assess(now() as u64),"runtime_evidence":evidence,"completed_at_ms":now()})),
    }).await.unwrap();
    let reads = native::reads(&fake);
    assert!(context(&f, &a, false)
        .await
        .unwrap_err()
        .message
        .contains("different native execution"));
    assert!(admit(&f, &a, &p).await.is_err());
    assert_eq!(native::reads(&fake), reads);
}

async fn artifact(
    f: &RepoDeliveryFixture,
    a: &HostedStagingAuthority,
    step: &str,
) -> Option<pharness_store::StoredArtifact> {
    f.state
        .store
        .get_artifact(&format!("staging_baseline_{step}_{}", a.execution_id))
        .await
        .unwrap()
}

#[tokio::test]
async fn baseline_is_required_and_context_get_never_collects_or_writes() {
    let fake = KubectlFixture::new(false);
    let (f, op, a) = start("baseline_required", &fake).await;
    let p = plan(&a);
    let _ = save(&f, &a, &p).await.unwrap();
    for _ in 0..3 {
        assert_eq!(
            context(&f, &a, false).await.unwrap().0["may_advance"],
            false
        );
    }
    assert_eq!(native::reads(&fake), 0);
    assert!(artifact(&f, &a, "window").await.is_none());
    assert_eq!(operation(&f).await.resource_refs, op.resource_refs);
    assert!(admit(&f, &a, &p)
        .await
        .unwrap_err()
        .message
        .contains("baseline"));
    assert_eq!(native::reads(&fake), 0);
    native::seed(&f, &fake, &a, &p).await;
    let before = native::reads(&fake);
    assert_eq!(context(&f, &a, false).await.unwrap().0["may_advance"], true);
    assert_eq!(native::reads(&fake), before);
    let admission = admit(&f, &a, &p).await.unwrap().0;
    assert!(
        admission["baseline"]["admission_expires_at_ms"]
            .as_i64()
            .unwrap()
            > now()
    );
    assert_eq!(native::reads(&fake), before + 6);
    assert!(admit(&f, &a, &p).await.is_err());
    assert_eq!(
        native::reads(&fake),
        before + 6,
        "repeat admission cannot repeat native reads"
    );
}

#[tokio::test]
async fn stale_and_contradictory_baselines_never_admit_even_with_a_passed_summary() {
    for (suffix, age, change) in [
        ("stale", 90_000, None),
        (
            "mixed",
            0,
            Some((
                "/runtime_evidence/signals/identity_after_sha256",
                json!("sha256:wrong"),
            )),
        ),
        (
            "summary",
            0,
            Some(("/assessment/reasons", json!(["telemetry_missing"]))),
        ),
    ] {
        let fake = KubectlFixture::new(false);
        let (f, _, a) = start(&format!("baseline_{suffix}"), &fake).await;
        let p = plan(&a);
        let _ = save(&f, &a, &p).await.unwrap();
        let evidence = native::evidence(&f, &fake, &a, &p, age).await;
        staging::seed_baseline(
            &f.state,
            &a.deployment_intent_id,
            &a.execution_id,
            evidence,
            change,
        )
        .await;
        let reads = native::reads(&fake);
        let result = context(&f, &a, false).await;
        assert!(result.is_err() || result.unwrap().0["may_advance"] == false);
        assert!(admit(&f, &a, &p).await.is_err(), "{suffix}");
        assert_eq!(native::reads(&fake), reads);
    }
}

#[tokio::test]
async fn fresh_baseline_cannot_admit_a_restarted_pod_or_reuse_failed_identity_collection() {
    let fake = KubectlFixture::new(false);
    let (f, _, a) = start("baseline_restart_pod", &fake).await;
    let p = plan(&a);
    let _ = save(&f, &a, &p).await.unwrap();
    native::seed(&f, &fake, &a, &p).await;
    let path = fake.dir.join("native-resources.json");
    let original = std::fs::read_to_string(&path).unwrap();
    let mut resources: Value = serde_json::from_str(&original).unwrap();
    resources["pods"]["items"][0]["status"]["containerStatuses"][0]["restartCount"] = json!(1);
    std::fs::write(&path, resources.to_string()).unwrap();
    let reads = native::reads(&fake);
    assert!(admit(&f, &a, &p)
        .await
        .unwrap_err()
        .message
        .contains("restart_count_changed"));
    assert_eq!(native::reads(&fake), reads + 6);
    std::fs::write(&path, original).unwrap();
    assert!(admit(&f, &a, &p).await.is_err());
    assert_eq!(
        native::reads(&fake),
        reads + 6,
        "repairing a fixture cannot renew a consumed admission attempt"
    );
    assert!(artifact(&f, &a, "admission_identity").await.is_some());
}

#[tokio::test]
async fn paused_reconciliation_retains_the_original_window_without_replaying_reads() {
    let fake = KubectlFixture::new(false);
    let (f, op, a) = start("baseline_wait_restart", &fake).await;
    let p = plan(&a);
    let _ = save(&f, &a, &p).await.unwrap();
    // Load only the isolated resource fixture; the real native reader supplies
    // the new before observation when reconciliation creates its fixed window.
    let _ = native::evidence(&f, &fake, &a, &p, 0).await;
    tick(&f).await;
    let window = artifact(&f, &a, "window").await.unwrap();
    let before = artifact(&f, &a, "before").await.unwrap();
    let reads = native::reads(&fake);
    control(&f, "paused").await;
    tick(&f).await;
    tick(&f).await;
    assert_eq!(
        artifact(&f, &a, "window").await.unwrap().content_json,
        window.content_json
    );
    assert_eq!(
        artifact(&f, &a, "before").await.unwrap().content_json,
        before.content_json
    );
    assert_eq!(native::reads(&fake), reads);
    assert_eq!(
        context(&f, &a, false).await.unwrap().0["may_advance"],
        false
    );
    assert!(admit(&f, &a, &p).await.is_err());
    assert_eq!(operation(&f).await.id, op.id);
    assert_eq!(creates(&fake), 2);
}

#[tokio::test]
async fn interrupted_or_missed_baseline_is_recorded_once_without_a_replacement_window() {
    for (suffix, created_offset, interrupted) in [
        ("interrupted", 0, Some("before")),
        ("missed", 120_000, None),
    ] {
        let fake = KubectlFixture::new(false);
        let (f, op, a) = start(&format!("baseline_{suffix}"), &fake).await;
        let p = plan(&a);
        let _ = save(&f, &a, &p).await.unwrap();
        let window = staging::seed_window(
            &f.state,
            &a.deployment_intent_id,
            &a.execution_id,
            now() - created_offset,
            interrupted,
        )
        .await;
        tick(&f).await;
        assert_eq!(
            operation(&f).await.resource_refs["staging_baseline"]["window"],
            window["window"]
        );
        let result = artifact(&f, &a, "result")
            .await
            .unwrap()
            .content_json
            .unwrap();
        assert_eq!(result["assessment"]["runtime_verification"], "inconclusive");
        tick(&f).await;
        assert_eq!(
            artifact(&f, &a, "result").await.unwrap().content_json,
            Some(result)
        );
        assert_eq!(
            artifact(&f, &a, "window").await.unwrap().content_json,
            Some(window)
        );
        assert_eq!(native::reads(&fake), 0);
        assert_eq!(operation(&f).await.id, op.id);
        assert_eq!(creates(&fake), 2);
    }
}
