use super::repo_mode_v1::{repo_fixture_for_source, RepoDeliveryFixture};
use crate::app::clock::current_millis;
use crate::app::hosted_controller::{reconcile_once, source_merge};
use crate::dispatch::{KubectlFixture, SourceJobKind};
use axum::extract::{Path, Query, State};
use axum::Json;
use pharness_core::canonical_json_sha256 as hash;
use pharness_core::hosted_sdlc::HostedSourceMergeAuthority;
use pharness_store::{
    BeginWorkflowOperation, CreateChangeSet, CreateProviderCheckSetObservation,
    CreateSourceDeliveryIntent, CreateStageExecution, FinishWorkflowReconciliation,
    SealStageOutcome,
};
use serde_json::{json, Value};

const REPO: &str = "https://github.com/lward27/yfinance_wrapper.git";

pub(super) async fn fixture(suffix: &str, fake: &KubectlFixture) -> RepoDeliveryFixture {
    fixture_with_policy(suffix, fake, None).await
}

pub(super) async fn fixture_with_policy(
    suffix: &str,
    fake: &KubectlFixture,
    policy: Option<Value>,
) -> RepoDeliveryFixture {
    let state = if policy.is_some() {
        super::characterization::test_state_with_hosted_build(fake.command.clone(), REPO.into())
            .await
    } else {
        super::characterization::test_state_with_git_observer(fake.command.clone(), REPO.into())
            .await
    };
    let policy = match policy {
        Some(mut policy) => {
            let p = &policy["pipeline_contract"];
            let contract = state
                .store
                .create_pipeline_contract(pharness_store::CreatePipelineContract {
                    id: p["id"].as_str().unwrap().into(),
                    status: "active".into(),
                    namespace: p["namespace"].as_str().unwrap().into(),
                    pipeline_ref: p["pipeline_ref"].as_str().unwrap().into(),
                    version: p["version"].as_str().unwrap().into(),
                    contract_json: p["contract_json"].clone(),
                    actor: Some("unit-test".into()),
                    reason: Some("Finite build contract fixture".into()),
                })
                .await
                .unwrap();
            policy["pipeline_contract"] = json!(contract);
            serde_json::from_value(policy).unwrap()
        }
        None => serde_json::from_str(include_str!(
            "../../../../pharness-core/tests/fixtures/hosted-workflow.json"
        ))
        .unwrap(),
    };
    let mut fixture = repo_fixture_for_source(suffix, false, state, Some(policy), Some(REPO)).await;
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
    let plan = store
        .get_work_plan_by_work_item(&item.id)
        .await
        .unwrap()
        .unwrap();
    let mut bound = Vec::new();
    for key in ["discover", "plan", "implement", "test", "verify"] {
        let id = format!("stage_{suffix}_{key}");
        let outcome = json!({"schema_version":pharness_core::STAGE_OUTCOME_SCHEMA,"stage":key,"status":"succeeded","contradictions":[],"work_item_id":item.id,"stage_execution_id":id});
        store
            .create_stage_execution(CreateStageExecution {
                id: id.clone(),
                work_item_id: item.id.clone(),
                stage_key: key.into(),
                sequence: 1,
                status: "succeeded".into(),
                agent_profile_id: None,
                agent_profile_version: None,
                agent_profile_hash: None,
                context_pack_id: None,
                run_id: None,
                workspace_id: None,
                input_snapshot: json!({}),
                input_hash: hash(&json!({})).unwrap(),
            })
            .await
            .unwrap();
        let sealed = store
            .seal_stage_outcome(SealStageOutcome {
                id: format!("outcome_{suffix}_{key}"),
                stage_execution_id: id,
                work_item_id: item.id.clone(),
                stage_key: key.into(),
                status: "succeeded".into(),
                content_hash: hash(&outcome).unwrap(),
                outcome,
                state_version: metadata.state_version,
                supersedes_outcome_id: None,
                effective: true,
                actor: "test-controller".into(),
                reason: "Bounded deterministic fixture".into(),
            })
            .await
            .unwrap();
        bound.push(
            json!({"id":sealed.id,"stage":key,"hash":sealed.content_hash,"status":"succeeded"}),
        );
    }
    let material = json!({"effective_outcomes":bound,"verification_stage_execution_id":format!("stage_{suffix}_verify"),"patch":{"artifact_id":format!("patch_{suffix}"),"hash":format!("sha256:{}","d".repeat(64))}});
    let material_hash = hash(&material).unwrap();
    let change = store
        .create_change_set(CreateChangeSet {
            id: format!("change_{suffix}"),
            work_item_id: Some(item.id.clone()),
            work_plan_id: plan.id,
            remediation_plan_id: None,
            incident_id: None,
            session_id: plan.session_id,
            run_id: None,
            status: "approved".into(),
            title: "Verified source fixture".into(),
            summary: "Exact source".into(),
            risk_level: "low".into(),
            material_hash: material_hash.clone(),
            resource_namespace: None,
            resource_kind: Some("Repository".into()),
            resource_name: Some(REPO.into()),
            change_set_json: material,
        })
        .await
        .unwrap();
    let branch = format!("pharness/{}/{}", item.id, &material_hash[7..19]);
    fixture.intent_id = format!("srcintent_{suffix}");
    let intent = store.create_source_delivery_intent(CreateSourceDeliveryIntent {id:fixture.intent_id.clone(),subject_kind:"work_item_change_set".into(),subject_id:change.id.clone(),repository_id:metadata.repository_id.clone(),source_repo:REPO.into(),base_ref:"main".into(),base_commit:item.source_commit.unwrap(),head_branch:branch.clone(),patch_artifact_id:Some(format!("patch_{suffix}")),patch_hash:format!("sha256:{}","d".repeat(64)),authorization:json!({"workflow_policy_hash":metadata.workflow_policy_hash,"work_item_id":item.id,"writer_execution_id":"srcexec_fixture","external_effect":"create one GitHub branch, commit, and pull request; merge is not authorized"}),created_by:"test-controller".into(),creation_reason:"Recorded source fixture".into()}).await.unwrap();
    store.update_source_delivery_intent(&intent.id,intent.state_version,"waiting_merge",Some("srcexec_fixture"),None,Some(&json!({"number":7,"url":"https://github.com/lward27/yfinance_wrapper/pull/7","head_branch":branch,"head_sha":"b".repeat(40)})),None,None,"test-controller","Fresh observed PR").await.unwrap();
    let checks = json!([{"name":"Source integrity","app_id":15368,"status":"passing"}]);
    store
        .create_provider_check_set_observation(CreateProviderCheckSetObservation {
            id: format!("checks_{suffix}"),
            source_delivery_intent_id: intent.id.clone(),
            phase: "pre_merge".into(),
            repository_id: metadata.repository_id.clone(),
            pull_request_number: 7,
            head_sha: "b".repeat(40),
            required_set_hash: hash(&checks).unwrap(),
            authoritative_rules_succeeded: true,
            status: "passing".into(),
            required_checks: checks,
            check_runs: json!([]),
            commit_statuses: json!([]),
            content_hash: hash(&json!({"fixture":"passing"})).unwrap(),
            expires_at: (current_millis() + 900_000).to_string(),
        })
        .await
        .unwrap();
    let now = current_millis() as i64;
    store.wake_workflow(&item.id, now).await.unwrap();
    let claim = store
        .claim_due_workflow("prior-api", now, 60_000)
        .await
        .unwrap()
        .unwrap();
    let key = format!("repository:{}", metadata.repository_id);
    let operation = store
        .begin_workflow_operation(
            &claim,
            BeginWorkflowOperation {
                id: &format!("operation_{suffix}"),
                action: "authorize_source_delivery",
                input_hash: "original-publication",
                effect: "development",
                resource_keys: &[&key],
            },
            now,
        )
        .await
        .unwrap();
    store
        .record_workflow_operation(
            &claim,
            &operation.id,
            "running",
            &json!({"action_resource":change.id,"source_delivery_intent_id":intent.id}),
            "Original source identity",
            now,
        )
        .await
        .unwrap();
    store
        .finish_workflow_reconciliation(
            &claim,
            FinishWorkflowReconciliation {
                next_due_at: now + 1,
                condition: "waiting",
                reason: "Source observation is ready",
                observed_state_hash: None,
            },
            now,
        )
        .await
        .unwrap();
    tick(&fixture).await;
    let control = store
        .get_workflow_reconciliation(&item.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(control.condition, "waiting", "{}", control.condition_reason);
    fixture
}

pub(super) async fn tick(fixture: &RepoDeliveryFixture) {
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

pub(super) async fn authority(fixture: &RepoDeliveryFixture) -> HostedSourceMergeAuthority {
    let op = fixture
        .state
        .store
        .workflow_operation_for_source_intent(&fixture.intent_id)
        .await
        .unwrap()
        .unwrap();
    serde_json::from_value(op.resource_refs["source_merge_authority"].clone()).unwrap()
}

pub(super) async fn admit(
    fixture: &RepoDeliveryFixture,
    a: &HostedSourceMergeAuthority,
) -> Result<Json<Value>, crate::app::ApiError> {
    source_merge::internal_source_merge_attempt(
        State(fixture.state.clone()),
        Path(fixture.intent_id.clone()),
        Json(
            serde_json::from_value(
                json!({"execution_id":a.execution_id,"authority_hash":a.material_hash().unwrap()}),
            )
            .unwrap(),
        ),
    )
    .await
}

async fn context(
    fixture: &RepoDeliveryFixture,
    a: &HostedSourceMergeAuthority,
) -> Result<Json<Value>, crate::app::ApiError> {
    source_merge::internal_source_merge_context(
        State(fixture.state.clone()),
        Path(fixture.intent_id.clone()),
        Query(source_merge::MergeQuery {
            execution_id: a.execution_id.clone(),
        }),
    )
    .await
}

async fn control(fixture: &RepoDeliveryFixture, control: &str) {
    let current = fixture
        .state
        .store
        .get_workflow_reconciliation(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    fixture
        .state
        .store
        .set_workflow_control(
            &fixture.work_item_id,
            current.control_version,
            control,
            "operator",
            "Fixture control",
            current_millis() as i64,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn hosted_source_merge_admission_is_once_and_gets_are_read_only() {
    let fake = KubectlFixture::new(false);
    let fixture = fixture("merge_once", &fake).await;
    let a = authority(&fixture).await;
    assert_eq!(fake.creates(), 1);
    let intent = fixture
        .state
        .store
        .get_source_delivery_intent(&fixture.intent_id)
        .await
        .unwrap()
        .unwrap();
    for _ in 0..3 {
        let _ = context(&fixture, &a).await.unwrap();
    }
    assert_eq!(
        intent,
        fixture
            .state
            .store
            .get_source_delivery_intent(&fixture.intent_id)
            .await
            .unwrap()
            .unwrap()
    );
    let (one, two) = tokio::join!(admit(&fixture, &a), admit(&fixture, &a));
    assert_eq!(usize::from(one.is_ok()) + usize::from(two.is_ok()), 1);
    assert!(admit(&fixture, &a).await.is_err());
    tick(&fixture).await;
    assert_eq!(fake.creates(), 1);
    assert_eq!(authority(&fixture).await, a);
    // Simulate a missing Job after admission. The admitted identity is spent
    // even without its callback; only a future observer may be dispatched.
    std::fs::remove_file(fake.dir.join("job.json")).unwrap();
    std::fs::remove_dir(fake.dir.join("created")).unwrap();
    tick(&fixture).await;
    assert_eq!(fake.creates(), 1);
    assert!(!fake.dir.join("job.json").exists());
    assert!(intent.authorization["external_effect"]
        .as_str()
        .unwrap()
        .contains("merge is not authorized"));
    assert!(fixture
        .state
        .worker
        .reconcile_source_delivery_job(&intent.id, "another_execution", SourceJobKind::Merge, true)
        .await
        .is_err());
}

#[tokio::test]
async fn hosted_source_merge_pause_withholds_new_admission_and_retains_late_receipts() {
    let fake = KubectlFixture::new(false);
    let fixture = fixture("merge_pause", &fake).await;
    let a = authority(&fixture).await;
    let _ = admit(&fixture, &a).await.unwrap();
    control(&fixture, "paused").await;
    assert!(context(&fixture, &a).await.is_err());
    assert!(admit(&fixture, &a).await.is_err());
    let receipt = json!({"execution_id":a.execution_id,"authority_hash":a.material_hash().unwrap(),"checked_at_ms":current_millis() as i64,"status":"merged","origin":"api_acknowledged","merge_http_status":200,"base_commit_sha":a.base_commit_sha,"head_commit_sha":a.head_commit_sha,"merge_commit_sha":"c".repeat(40),"merge_tree_sha":"d".repeat(40)});
    let submit = |value: Value| {
        source_merge::internal_source_merge_outcome(
            State(fixture.state.clone()),
            Path(fixture.intent_id.clone()),
            Json(serde_json::from_value(value).unwrap()),
        )
    };
    let Json(first) = submit(receipt.clone()).await.unwrap();
    let Json(again) = submit(receipt.clone()).await.unwrap();
    assert_eq!(first, again);
    let mut conflicting = receipt;
    conflicting["merge_commit_sha"] = json!("e".repeat(40));
    assert!(submit(conflicting).await.is_err());
    let intent = fixture
        .state
        .store
        .get_source_delivery_intent(&fixture.intent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(intent.status, "waiting_merge");
    assert!(intent.merge_provenance.is_none());
    assert!(fixture
        .state
        .store
        .get_repo_work_item_metadata(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap()
        .closed_at
        .is_none());
}

#[tokio::test]
async fn hosted_source_merge_observation_requires_admission_and_exact_parents() {
    let fake = KubectlFixture::new(false);
    let fixture = fixture("merge_observation", &fake).await;
    let a = authority(&fixture).await;
    let intent = fixture
        .state
        .store
        .get_source_delivery_intent(&fixture.intent_id)
        .await
        .unwrap()
        .unwrap();
    let mut observed:crate::dto::GitDeliveryObservationOutcomeRequest=serde_json::from_value(json!({"execution_id":"observer","status":"observed","merge_parent_shas":[a.base_commit_sha,a.head_commit_sha],"merge_tree_sha":"d".repeat(40)})).unwrap();
    let proof = source_merge::observed_proof(&fixture.state, &intent, &observed)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(proof["accepted"], false);
    let _ = admit(&fixture, &a).await.unwrap();
    let proof = source_merge::observed_proof(&fixture.state, &intent, &observed)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(proof["accepted"], true);
    assert_eq!(
        proof["acknowledgement"],
        "recovered_by_provider_observation"
    );
    assert!(proof["worker_receipt_id"].is_null());
    for parents in [
        vec!["f".repeat(40), a.head_commit_sha.clone()],
        vec![a.base_commit_sha.clone()],
        vec![a.head_commit_sha, a.base_commit_sha],
    ] {
        observed.merge_parent_shas = Some(parents);
        assert_eq!(
            source_merge::observed_proof(&fixture.state, &intent, &observed)
                .await
                .unwrap()
                .unwrap()["accepted"],
            false
        );
    }
}

#[tokio::test]
async fn hosted_source_merge_rejects_cancelled_work_and_changed_pull_request() {
    let fake = KubectlFixture::new(false);
    let fixture = fixture("merge_stale", &fake).await;
    let a = authority(&fixture).await;
    control(&fixture, "cancelled").await;
    assert!(admit(&fixture, &a).await.is_err());
    assert!(context(&fixture, &a).await.is_err());
    let intent = fixture
        .state
        .store
        .get_source_delivery_intent(&fixture.intent_id)
        .await
        .unwrap()
        .unwrap();
    let mut pull = intent.pull_request.unwrap();
    pull["head_sha"] = json!("f".repeat(40));
    fixture
        .state
        .store
        .update_source_delivery_intent(
            &intent.id,
            intent.state_version,
            "head_drift",
            None,
            None,
            Some(&pull),
            None,
            None,
            "observer",
            "Changed head",
        )
        .await
        .unwrap();
    let receipt = json!({"execution_id":a.execution_id,"authority_hash":a.material_hash().unwrap(),"checked_at_ms":current_millis() as i64,"status":"failed","error_code":"source_merge_pull_request_changed"});
    let Json(recorded) = source_merge::internal_source_merge_outcome(
        State(fixture.state.clone()),
        Path(intent.id),
        Json(serde_json::from_value(receipt).unwrap()),
    )
    .await
    .unwrap();
    assert_eq!(recorded["status"], "recorded");
    assert_eq!(fake.creates(), 1);
}

#[tokio::test]
async fn hosted_source_merge_provider_callback_records_exact_proof_and_keeps_release_open() {
    for valid_parents in [true, false] {
        let fake = KubectlFixture::new(false);
        let fixture = fixture(
            if valid_parents {
                "merge_good_parents"
            } else {
                "merge_bad_parents"
            },
            &fake,
        )
        .await;
        let a = authority(&fixture).await;
        let _ = admit(&fixture, &a).await.unwrap();
        let store = &fixture.state.store;
        let intent = store
            .get_source_delivery_intent(&fixture.intent_id)
            .await
            .unwrap()
            .unwrap();
        store
            .update_source_delivery_intent(
                &intent.id,
                intent.state_version,
                "observer_dispatched",
                None,
                Some("observer_final"),
                None,
                None,
                None,
                "test-controller",
                "Observe admitted merge without a writer callback",
            )
            .await
            .unwrap();
        let request = json!({"execution_id":"observer_final","status":"observed","pull_request_state":"closed","merged":true,"merge_commit_sha":"c".repeat(40),"head_commit_sha":a.head_commit_sha,"head_branch":a.head_branch,"merge_parent_shas":[if valid_parents {a.base_commit_sha.clone()} else {"f".repeat(40)},a.head_commit_sha],"merge_tree_sha":"d".repeat(40),"authoritative_rules_succeeded":true,"required_checks":[{"name":"Source integrity","app_id":15368,"status":"passing"}],"check_runs":[],"commit_statuses":[],"provider_check_status":"passing"});
        let Json(result) = crate::app::repo_mode::internal_source_delivery_observation_outcome(
            State(fixture.state.clone()),
            Path(intent.id.clone()),
            Json(serde_json::from_value(request.clone()).unwrap()),
        )
        .await
        .unwrap();
        assert_eq!(
            result["delivery_status"],
            if valid_parents { "succeeded" } else { "failed" }
        );
        let outcomes = store
            .list_effective_stage_outcomes(&fixture.work_item_id)
            .await
            .unwrap();
        let source = outcomes
            .iter()
            .find(|o| o.stage_key == "source_delivery")
            .unwrap();
        assert_eq!(
            source.status,
            if valid_parents { "succeeded" } else { "failed" }
        );
        assert_eq!(
            source.outcome["pinned_inputs"]["hosted_merge_proof"]["accepted"],
            valid_parents
        );
        assert!(!outcomes
            .iter()
            .any(|o| matches!(o.stage_key.as_str(), "release" | "observe")));
        tick(&fixture).await;
        assert!(store
            .active_workflow_operation(&fixture.work_item_id)
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .get_repo_work_item_metadata(&fixture.work_item_id)
                .await
                .unwrap()
                .unwrap()
                .closed_at
                .is_none(),
            valid_parents
        );
        assert_eq!(
            store
                .get_workflow_reconciliation(&fixture.work_item_id)
                .await
                .unwrap()
                .unwrap()
                .condition,
            if valid_parents {
                "progressing"
            } else {
                "blocked"
            }
        );
        let _ = crate::app::repo_mode::internal_source_delivery_observation_outcome(
            State(fixture.state.clone()),
            Path(intent.id),
            Json(serde_json::from_value(request).unwrap()),
        )
        .await;
        assert_eq!(
            store
                .list_effective_stage_outcomes(&fixture.work_item_id)
                .await
                .unwrap(),
            outcomes
        );
        assert_eq!(fake.creates(), 1);
    }
}

#[tokio::test]
async fn hosted_source_merge_fresh_context_rejects_new_failed_or_expired_checks() {
    for (suffix, status, expiry) in [
        ("merge_failed_checks", "failed", current_millis() + 900_000),
        ("merge_expired_checks", "passing", current_millis() - 1),
    ] {
        let fake = KubectlFixture::new(false);
        let fixture = fixture(suffix, &fake).await;
        let a = authority(&fixture).await;
        let intent = fixture
            .state
            .store
            .get_source_delivery_intent(&fixture.intent_id)
            .await
            .unwrap()
            .unwrap();
        let required = json!([{"name":"Source integrity","app_id":15368,"status":status}]);
        fixture
            .state
            .store
            .create_provider_check_set_observation(CreateProviderCheckSetObservation {
                id: format!("new_checks_{suffix}"),
                source_delivery_intent_id: intent.id,
                phase: "pre_merge".into(),
                repository_id: intent.repository_id,
                pull_request_number: 7,
                head_sha: a.head_commit_sha.clone(),
                required_set_hash: hash(&required).unwrap(),
                authoritative_rules_succeeded: true,
                status: status.into(),
                required_checks: required,
                check_runs: json!([]),
                commit_statuses: json!([]),
                content_hash: hash(&json!({"fixture":status})).unwrap(),
                expires_at: expiry.to_string(),
            })
            .await
            .unwrap();
        assert!(context(&fixture, &a).await.is_err());
        assert!(admit(&fixture, &a).await.is_err());
        assert_eq!(fake.creates(), 1);
    }
}
