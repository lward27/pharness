//! Production promotion after verified staging: one reviewable proposal, one
//! identified human decision, one admitted GitOps write, two bounded runtime
//! windows and the sealed WorkItem. Native evidence is synthetic (`native.rs`);
//! the controller clock is advanced instead of waiting for real windows.
use super::runtime::{candidate, committed};
use super::{
    control, creates, native, now, operation, request, staging, tick, KubectlFixture,
    RepoDeliveryFixture,
};
use crate::app::approvals::{
    batch_decide_approval_gates, decide_approval_gate, satisfy_approval_gate,
};
use crate::app::hosted_controller::production;
use crate::app::ApiError;
use crate::dto::{BatchDecideApprovalGatesRequest, DecideApprovalGateRequest};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use pharness_core::hosted_sdlc::production::{
    HostedProductionAuthority, ProductionGitOpsPlan, ProductionPreview, ProductionPreviewAuthority,
    PREVIEW_SCHEMA,
};
use pharness_core::hosted_sdlc::staging::HostedStagingAuthority;
use pharness_core::tools::{
    FinanceApplication, FinanceDeploymentExpectation, FinanceEnvironment, FinanceVerificationPhase,
};
use pharness_store::{DeliveryStage, StoredApprovalGate, StoredWorkflowReconciliation};
use serde_json::{json, Value};

/// Digest pinned by the checked-in production Kustomization fixture.
const PREVIOUS: &str = "sha256:a1a8e08c3f3c7596f153c166e282f4a4c4e737a860c42f35cce21bf25ef415b4";
const PROPOSE: &str = "propose_production_release";
const PROMOTE: &str = "promote_approved_release";
const OBSERVE: &str = "observe_production_release";
const KUSTOMIZATION: &str = include_str!(
    "../../../../../pharness-core/src/hosted_sdlc/production/fixtures/yfinance-wrapper.yaml"
);

fn base_sha() -> String {
    "1".repeat(40)
}

fn expectation(gitops_commit_sha: &str, image_digest: &str) -> FinanceDeploymentExpectation {
    FinanceDeploymentExpectation {
        application: FinanceApplication::Yfinance,
        environment: FinanceEnvironment::Production,
        gitops_commit_sha: gitops_commit_sha.into(),
        image_digest: image_digest.into(),
    }
}

async fn reconciliation(f: &RepoDeliveryFixture) -> StoredWorkflowReconciliation {
    f.state
        .store
        .get_workflow_reconciliation(&f.work_item_id)
        .await
        .unwrap()
        .unwrap()
}

async fn verified(
    suffix: &str,
    fake: &KubectlFixture,
) -> (RepoDeliveryFixture, HostedStagingAuthority) {
    let (f, a, _) = committed(suffix, fake).await;
    let evidence = candidate(&f, fake, &a, 90_000).await;
    staging::seed_runtime(
        &f.state,
        &a.deployment_intent_id,
        &a.execution_id,
        evidence,
        true,
        None,
    )
    .await;
    tick(&f).await;
    assert!(f
        .state
        .store
        .active_workflow_operation(&f.work_item_id)
        .await
        .unwrap()
        .is_none());
    (f, a)
}

async fn preview_authority(f: &RepoDeliveryFixture) -> ProductionPreviewAuthority {
    let op = operation(f).await;
    assert_eq!(op.action, PROPOSE);
    serde_json::from_value(op.resource_refs["production_preview_dispatch"]["authority"].clone())
        .unwrap()
}

fn preview_file(pa: &ProductionPreviewAuthority) -> ProductionPreview {
    ProductionPreview {
        schema_version: PREVIEW_SCHEMA.into(),
        authority_hash: pa.material_hash().unwrap(),
        base_commit_sha: base_sha(),
        original_blob_sha: "2".repeat(40),
        original_content: KUSTOMIZATION.into(),
    }
}

fn observed_preview(pa: &ProductionPreviewAuthority, file: &ProductionPreview) -> Value {
    json!({"status":"observed","execution_id":pa.execution_id,"authority_hash":pa.material_hash().unwrap(),"preview":file,"checked_at_ms":now()})
}

async fn preview(
    f: &RepoDeliveryFixture,
    pa: &ProductionPreviewAuthority,
    body: Value,
) -> Result<Json<Value>, ApiError> {
    production::internal_production_preview(
        State(f.state.clone()),
        Path(pa.deployment_intent_id.clone()),
        request(body),
    )
    .await
}

async fn proposed(
    suffix: &str,
    fake: &KubectlFixture,
) -> (
    RepoDeliveryFixture,
    ProductionPreviewAuthority,
    StoredApprovalGate,
) {
    let (f, _) = verified(suffix, fake).await;
    tick(&f).await;
    assert_eq!(creates(fake), 3);
    let pa = preview_authority(&f).await;
    let _ = preview(&f, &pa, observed_preview(&pa, &preview_file(&pa)))
        .await
        .unwrap();
    let _ = native::evidence_for(
        &f,
        fake,
        expectation(&base_sha(), PREVIOUS),
        FinanceVerificationPhase::Baseline,
        0,
    )
    .await;
    tick(&f).await;
    let op = f
        .state
        .store
        .get_workflow_operation(&pa.operation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(op.status, "succeeded", "{}", op.resource_refs);
    let gate = f
        .state
        .store
        .get_approval_gate(
            op.resource_refs["production_proposal"]["gate_id"]
                .as_str()
                .unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    (f, pa, gate)
}

async fn decide(
    f: &RepoDeliveryFixture,
    gate_id: &str,
    status: &str,
    decided_by: Option<&str>,
    reason: Option<&str>,
) -> Result<StoredApprovalGate, ApiError> {
    let request = DecideApprovalGateRequest {
        decided_by: decided_by.map(Into::into),
        reason: reason.map(Into::into),
    };
    if status == "satisfied" {
        let _ = satisfy_approval_gate(
            State(f.state.clone()),
            None,
            Path(gate_id.into()),
            Json(request),
        )
        .await?;
    } else {
        let _ =
            decide_approval_gate(f.state.clone(), None, gate_id.into(), status, request).await?;
    }
    Ok(f.state
        .store
        .get_approval_gate(gate_id)
        .await
        .unwrap()
        .unwrap())
}

async fn promoted(
    suffix: &str,
    fake: &KubectlFixture,
) -> (
    RepoDeliveryFixture,
    HostedProductionAuthority,
    ProductionGitOpsPlan,
) {
    let (f, _, gate) = proposed(suffix, fake).await;
    let _ = decide(
        &f,
        &gate.id,
        "satisfied",
        Some("lucas"),
        Some("Reviewed the diff"),
    )
    .await
    .unwrap();
    tick(&f).await;
    assert_eq!(creates(fake), 4, "one original production writer");
    let op = operation(&f).await;
    assert_eq!(op.action, PROMOTE);
    let a: HostedProductionAuthority =
        serde_json::from_value(op.resource_refs["production_dispatch"]["authority"].clone())
            .unwrap();
    let plan: ProductionGitOpsPlan = serde_json::from_value(
        f.state
            .store
            .get_artifact(&format!("production_plan_{}", a.execution_id))
            .await
            .unwrap()
            .unwrap()
            .content_json
            .unwrap(),
    )
    .unwrap();
    (f, a, plan)
}

fn observation(a: &HostedProductionAuthority, plan: &ProductionGitOpsPlan) -> Value {
    json!({"execution_id":a.execution_id,"authority_hash":a.material_hash().unwrap(),"status":"committed","checked_at_ms":now(),"observe_only":false,"observation":{"gitops_commit_sha":"e".repeat(40),"base_commit_sha":plan.base_commit_sha,"current_gitops_revision":"e".repeat(40),"file_blob_sha":"f".repeat(40),"path":a.coordinates().unwrap().0,"image_ref":a.image_ref().unwrap(),"authority_hash":plan.proposal_hash,"plan_hash":plan.material_hash().unwrap(),"parent_count":1,"changed_file_count":1,"current_file_matches":true,"observation_attempts":1}})
}

async fn context(
    f: &RepoDeliveryFixture,
    a: &HostedProductionAuthority,
    observe_only: bool,
) -> Result<Json<Value>, ApiError> {
    production::internal_production_context(
        State(f.state.clone()),
        Path(a.proposal.deployment_intent_id.clone()),
        Query(
            serde_json::from_value(
                json!({"execution_id":a.execution_id,"observe_only":observe_only}),
            )
            .unwrap(),
        ),
    )
    .await
}

async fn attempt(
    f: &RepoDeliveryFixture,
    a: &HostedProductionAuthority,
    plan: &ProductionGitOpsPlan,
) -> Result<Json<Value>, ApiError> {
    production::internal_production_attempt(State(f.state.clone()), Path(a.proposal.deployment_intent_id.clone()), request(json!({"execution_id":a.execution_id,"authority_hash":a.material_hash().unwrap(),"plan_hash":plan.material_hash().unwrap()}))).await
}

async fn outcome(
    f: &RepoDeliveryFixture,
    a: &HostedProductionAuthority,
    body: Value,
) -> Result<Json<Value>, ApiError> {
    production::internal_production_outcome(
        State(f.state.clone()),
        Path(a.proposal.deployment_intent_id.clone()),
        request(body),
    )
    .await
}

async fn stage_outcome(
    f: &RepoDeliveryFixture,
    key: &str,
) -> Option<pharness_store::StoredStageOutcome> {
    f.state
        .store
        .list_effective_stage_outcomes(&f.work_item_id)
        .await
        .unwrap()
        .into_iter()
        .find(|o| o.stage_key == key)
}

fn env(manifest: &Value) -> &Vec<Value> {
    manifest["spec"]["template"]["spec"]["containers"][0]["env"]
        .as_array()
        .unwrap()
}

#[tokio::test]
async fn verified_staging_proposes_one_reviewable_production_diff_and_waits_for_a_human() {
    let fake = KubectlFixture::new(false);
    let (f, staged) = verified("production_proposal", &fake).await;
    tick(&f).await;
    assert_eq!(creates(&fake), 3, "one read-only production preview reader");
    let op = operation(&f).await;
    let pa = preview_authority(&f).await;
    assert_eq!(pa.image_digest, staged.image_digest);
    assert_eq!(pa.source_commit_sha, staged.source_commit_sha);
    assert_eq!(
        pa.deployment_intent_id,
        format!(
            "dintent_production_{}",
            op.id.trim_start_matches("workflowop_")
        )
    );
    let reader = &op.resource_refs["production_preview_dispatch"]["job_manifest"];
    assert_eq!(
        reader["spec"]["template"]["spec"]["serviceAccountName"],
        "pharness-gitops-observer"
    );
    assert!(env(reader)
        .iter()
        .any(|v| v["name"] == "PHARNESS_PRODUCTION_PHASE" && v["value"] == "preview"));
    assert!(env(reader).iter().any(
        |v| v["name"] == "PHARNESS_EXECUTION_KIND" && v["value"] == "hosted_production_gitops"
    ));
    assert!(!env(reader)
        .iter()
        .any(|v| v["name"] == "PHARNESS_GIT_WRITER_TOKEN"));
    let repository = f
        .state
        .store
        .get_repo_work_item_metadata(&f.work_item_id)
        .await
        .unwrap()
        .unwrap()
        .repository_id;
    assert!(op
        .resource_keys
        .contains(&format!("delivery:{repository}:production")));
    assert!(
        !op.resource_keys
            .iter()
            .any(|k| k == "gitops:lucas_engineering:main"),
        "proposing never holds the GitOps write lock"
    );
    let production = f
        .state
        .store
        .get_deployment_intent_for_stage(&staged.pipeline_intent_id, DeliveryStage::Production)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(production.id, pa.deployment_intent_id);
    assert_eq!(production.status, "proposed");
    assert_eq!(production.target_namespace.as_deref(), Some("apps-prod"));
    let ctx = production::internal_production_preview_context(
        State(f.state.clone()),
        Path(pa.deployment_intent_id.clone()),
        Query(serde_json::from_value(json!({"execution_id":pa.execution_id})).unwrap()),
    )
    .await
    .unwrap();
    assert_eq!(ctx.0["authority"], json!(pa));
    assert_eq!(ctx.0["preview_recorded"], false);

    let file = preview_file(&pa);
    let mut wrong_hash = observed_preview(&pa, &file);
    wrong_hash["authority_hash"] = json!("sha256:wrong");
    assert!(preview(&f, &pa, wrong_hash).await.is_err());
    let mut no_code = observed_preview(&pa, &file);
    no_code["status"] = json!("unavailable");
    no_code["preview"] = Value::Null;
    assert!(preview(&f, &pa, no_code).await.is_err());
    let mut staging_file = file.clone();
    staging_file.original_content = KUSTOMIZATION.replace(
        "kind: Kustomization\n",
        "kind: Kustomization\nnamespace: apps-staging\n",
    );
    assert!(
        preview(&f, &pa, observed_preview(&pa, &staging_file))
            .await
            .is_err(),
        "a staging namespace is not a production file"
    );
    let mut running_candidate = file.clone();
    running_candidate.original_content = KUSTOMIZATION.replace(PREVIOUS, &pa.image_digest);
    let _ = preview(&f, &pa, observed_preview(&pa, &running_candidate))
        .await
        .unwrap();
    assert!(
        preview(&f, &pa, observed_preview(&pa, &file))
            .await
            .is_err(),
        "the recorded preview cannot be replaced"
    );
    let _ = preview(&f, &pa, observed_preview(&pa, &running_candidate))
        .await
        .unwrap();
    tick(&f).await;
    let state = reconciliation(&f).await;
    assert_eq!(state.condition, "blocked", "{}", state.condition_reason);
    assert_eq!(
        f.state
            .store
            .get_workflow_operation(&op.id)
            .await
            .unwrap()
            .unwrap()
            .resource_refs["production_preview_result"]["reason"],
        "production_already_runs_candidate_digest"
    );
    assert_eq!(creates(&fake), 3);
    assert!(f
        .state
        .store
        .get_artifact(&format!("production_proposal_{}", pa.execution_id))
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        f.state
            .store
            .get_deployment_intent(&pa.deployment_intent_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "proposed"
    );

    let second = KubectlFixture::new(false);
    let (f, _) = verified("production_proposal_baseline", &second).await;
    tick(&f).await;
    let pa = preview_authority(&f).await;
    let file = preview_file(&pa);
    let _ = preview(&f, &pa, observed_preview(&pa, &file))
        .await
        .unwrap();
    let _ = preview(&f, &pa, observed_preview(&pa, &file))
        .await
        .unwrap();
    // The native fixture still shows staging: production does not yet prove the
    // previewed revision and digest, so the baseline read is repeated later.
    let reads = native::reads(&second);
    tick(&f).await;
    let state = reconciliation(&f).await;
    assert_eq!(state.condition, "waiting", "{}", state.condition_reason);
    assert!(state.condition_reason.contains("baseline"));
    assert!(native::reads(&second) > reads);
    let op = operation(&f).await;
    assert!(op.resource_refs.get("production_baseline_1").is_some());
    assert!(op.resource_refs.get("production_proposal").is_none());
    let _ = native::evidence_for(
        &f,
        &second,
        expectation(&base_sha(), PREVIOUS),
        FinanceVerificationPhase::Baseline,
        0,
    )
    .await;
    let reads = native::reads(&second);
    tick(&f).await;
    assert!(native::reads(&second) > reads);
    let op = f
        .state
        .store
        .get_workflow_operation(&pa.operation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(op.status, "succeeded", "{}", op.resource_refs);
    assert!(op.resource_refs.get("production_baseline_2").is_some());
    let proposal = &op.resource_refs["production_proposal"];
    let gate = f
        .state
        .store
        .get_approval_gate(proposal["gate_id"].as_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(gate.gate_kind, production::GATE_KIND);
    assert_eq!(gate.status, "pending");
    assert_eq!(gate.work_item_id.as_deref(), Some(f.work_item_id.as_str()));
    assert_eq!(gate.gate_json["proposal_hash"], proposal["proposal_hash"]);
    assert_eq!(gate.gate_json["review"]["previous_image_digest"], PREVIOUS);
    assert_eq!(
        gate.gate_json["review"]["candidate_image_digest"],
        pa.image_digest
    );
    assert_eq!(gate.gate_json["review"]["expected_head_oid"], base_sha());
    assert_eq!(
        gate.gate_json["review"]["path"],
        "charts/yfinance-wrapper/kustomization.yaml"
    );
    assert_eq!(gate.gate_json["plan"]["original_content"], KUSTOMIZATION);
    assert_eq!(
        gate.gate_json["plan"]["updated_content"],
        KUSTOMIZATION.replace(PREVIOUS, &pa.image_digest)
    );
    assert_eq!(
        gate.gate_json["proposal"]["baseline_evidence_artifact_id"],
        format!("production_baseline_2_{}", pa.execution_id)
    );
    let state = reconciliation(&f).await;
    assert_eq!(
        state.condition, "awaiting_human",
        "{}",
        state.condition_reason
    );
    let reads = native::reads(&second);
    tick(&f).await;
    tick(&f).await;
    assert_eq!(reconciliation(&f).await.condition, "awaiting_human");
    assert_eq!(
        native::reads(&second),
        reads,
        "waiting for a human repeats no read"
    );
    assert_eq!(creates(&second), 3);
    assert!(f
        .state
        .store
        .active_workflow_operation(&f.work_item_id)
        .await
        .unwrap()
        .is_none());
    assert!(
        f.state
            .store
            .get_gitops_change_set_by_deployment_intent(&pa.deployment_intent_id)
            .await
            .unwrap()
            .is_none(),
        "no GitOps ChangeSet exists before the decision"
    );
}

#[tokio::test]
async fn the_production_gate_is_decided_once_by_an_identified_operator_over_current_evidence() {
    let fake = KubectlFixture::new(false);
    let (f, pa, gate) = proposed("production_gate", &fake).await;
    let approval_id = format!("production_approval_{}", pa.execution_id);
    let reviewed = "Reviewed the exact diff and the healthy baseline";
    let reason = Some(reviewed);
    let waived = decide(&f, &gate.id, "waived", Some("lucas"), reason)
        .await
        .unwrap_err();
    assert_eq!(waived.status, StatusCode::CONFLICT);
    let unreasoned = decide(&f, &gate.id, "satisfied", Some("lucas"), None)
        .await
        .unwrap_err();
    assert_eq!(unreasoned.status, StatusCode::BAD_REQUEST);
    let anonymous = decide(&f, &gate.id, "satisfied", None, reason)
        .await
        .unwrap_err();
    assert_eq!(anonymous.status, StatusCode::BAD_REQUEST);
    let batch = batch_decide_approval_gates(
        State(f.state.clone()),
        None,
        Json(BatchDecideApprovalGatesRequest {
            gate_ids: vec![gate.id.clone()],
            decision: "satisfied".into(),
            decided_by: "lucas".into(),
            reason: "batch".into(),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(batch.status, StatusCode::CONFLICT);
    control(&f, "paused").await;
    let paused = decide(&f, &gate.id, "satisfied", Some("lucas"), reason)
        .await
        .unwrap_err();
    assert_eq!(paused.status, StatusCode::CONFLICT);
    control(&f, "active").await;
    assert_eq!(
        f.state
            .store
            .get_approval_gate(&gate.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "pending"
    );
    assert!(f
        .state
        .store
        .get_artifact(&approval_id)
        .await
        .unwrap()
        .is_none());

    let decided = decide(&f, &gate.id, "satisfied", Some("lucas"), reason)
        .await
        .unwrap();
    assert_eq!(decided.status, "satisfied");
    assert_eq!(decided.decided_by.as_deref(), Some("lucas"));
    let approval = f
        .state
        .store
        .get_artifact(&approval_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(approval.kind, "hosted_production_approval");
    let body = approval.content_json.unwrap();
    assert_eq!(body["approval"]["operator_identity"], "lucas");
    assert_eq!(body["approval"]["reason"], reviewed);
    assert_eq!(
        body["approval"]["proposal_hash"],
        gate.gate_json["proposal_hash"]
    );
    assert_eq!(
        body["approval"]["gitops_plan_hash"],
        gate.gate_json["plan_hash"]
    );
    assert_eq!(body["gate_id"], gate.id);
    assert_eq!(
        body["approval"]["expires_at_ms"].as_i64().unwrap()
            - body["approval"]["approved_at_ms"].as_i64().unwrap(),
        30 * 60 * 1000
    );
    assert_eq!(
        f.state
            .store
            .get_deployment_intent(&pa.deployment_intent_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "approved"
    );
    assert_eq!(
        decide(&f, &gate.id, "satisfied", Some("lucas"), reason)
            .await
            .unwrap_err()
            .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        decide(&f, &gate.id, "rejected", Some("lucas"), reason)
            .await
            .unwrap_err()
            .status,
        StatusCode::CONFLICT
    );

    let second = KubectlFixture::new(false);
    let (f, pa, gate) = proposed("production_gate_rejected", &second).await;
    let rejected = decide(
        &f,
        &gate.id,
        "rejected",
        Some("lucas"),
        Some("Not this week"),
    )
    .await
    .unwrap();
    assert_eq!(rejected.status, "rejected");
    assert!(f
        .state
        .store
        .get_artifact(&format!("production_rejection_{}", pa.execution_id))
        .await
        .unwrap()
        .is_some());
    assert!(f
        .state
        .store
        .get_artifact(&format!("production_approval_{}", pa.execution_id))
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        f.state
            .store
            .get_deployment_intent(&pa.deployment_intent_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "rejected"
    );
    tick(&f).await;
    let state = reconciliation(&f).await;
    assert_eq!(state.condition, "blocked", "{}", state.condition_reason);
    assert!(state.condition_reason.contains("rejected by an operator"));
    assert_eq!(creates(&second), 3, "a rejection starts no writer");
    assert!(f
        .state
        .store
        .active_workflow_operation(&f.work_item_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn an_approved_release_is_written_once_then_verified_over_two_windows_and_sealed() {
    let fake = KubectlFixture::new(false);
    let (f, a, plan) = promoted("production_release", &fake).await;
    let op = operation(&f).await;
    assert_eq!(a.approval.operator_identity, "lucas");
    assert_eq!(a.proposal.previous_image_digest, PREVIOUS);
    assert!(op
        .resource_keys
        .iter()
        .any(|k| k == "gitops:lucas_engineering:main"));
    let dispatch = &op.resource_refs["production_dispatch"];
    let writer = &dispatch["executor_job_manifest"]["spec"]["template"]["spec"];
    assert_eq!(writer["serviceAccountName"], "pharness-gitops-writer");
    assert!(env(&dispatch["executor_job_manifest"])
        .iter()
        .any(|v| v["name"] == "PHARNESS_PRODUCTION_PHASE" && v["value"] == "deliver"));
    assert!(env(&dispatch["observer_job_manifest"])
        .iter()
        .any(|v| v["name"] == "PHARNESS_PRODUCTION_PHASE" && v["value"] == "observe"));
    assert!(!env(&dispatch["observer_job_manifest"])
        .iter()
        .any(|v| v["name"] == "PHARNESS_GIT_WRITER_TOKEN"));
    let change = f
        .state
        .store
        .get_gitops_change_set_by_deployment_intent(&a.proposal.deployment_intent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(change.id, a.gitops_change_set_id);
    assert_eq!(change.status, "approved");
    assert_eq!(change.gitops_change_set_json["scope"], "production");
    assert_eq!(
        change.gitops_change_set_json["expected_head_oid"],
        plan.base_commit_sha
    );
    assert_eq!(
        change.kustomization_path,
        "charts/yfinance-wrapper/kustomization.yaml"
    );
    assert_eq!(change.image_ref, a.image_ref().unwrap());

    let ctx = context(&f, &a, false).await.unwrap();
    assert_eq!(ctx.0["may_advance"], true);
    assert_eq!(ctx.0["plan"], json!(plan));
    assert_eq!(ctx.0["admission_recorded"], false);
    assert!(context(&f, &a, true).await.is_err());
    assert!(
        outcome(&f, &a, observation(&a, &plan)).await.is_err(),
        "a commit cannot be reported before admission"
    );
    let admitted = attempt(&f, &a, &plan).await.unwrap();
    assert_eq!(admitted.0["admitted"], true);
    assert_eq!(admitted.0["approval_artifact_id"], a.approval_artifact_id);
    assert!(attempt(&f, &a, &plan).await.is_err());
    assert_eq!(
        context(&f, &a, false).await.unwrap().0["may_advance"],
        false
    );
    let body = observation(&a, &plan);
    assert_eq!(
        outcome(&f, &a, body.clone()).await.unwrap().0["status"],
        "gitops_committed"
    );
    let _ = outcome(&f, &a, body.clone()).await.unwrap();
    let mut other = body.clone();
    other["observation"]["gitops_commit_sha"] = json!("8".repeat(40));
    assert!(outcome(&f, &a, other).await.is_err());
    for (pointer, bad) in [
        (
            "/observation/path",
            json!("charts/finance-staging/yfinance/kustomization.yaml"),
        ),
        (
            "/observation/authority_hash",
            json!(a.material_hash().unwrap()),
        ),
        ("/observation/changed_file_count", json!(2)),
    ] {
        let mut invalid = body.clone();
        *invalid.pointer_mut(pointer).unwrap() = bad;
        assert!(outcome(&f, &a, invalid).await.is_err(), "{pointer}");
    }
    tick(&f).await;
    let op = operation(&f).await;
    assert_eq!(op.status, "running");
    assert_eq!(
        op.resource_refs["production_result"]["status"],
        "gitops_committed"
    );
    assert!(
        f.state
            .store
            .get_artifact(&format!("production_runtime_window_{}", a.execution_id))
            .await
            .unwrap()
            .is_none(),
        "the old production image cannot start the window"
    );
    assert_eq!(creates(&fake), 4);

    crate::app::clock::advance_for_test(15 * 60 * 1000);
    let released = expectation(&"e".repeat(40), &a.proposal.image_digest);
    let evidence = native::evidence_for(
        &f,
        &fake,
        released.clone(),
        FinanceVerificationPhase::Production,
        90_000,
    )
    .await;
    assert_eq!(
        evidence.window.end_unix_seconds - evidence.window.start_unix_seconds,
        600
    );
    production::seed_production_window(
        &f.state,
        &a.proposal.deployment_intent_id,
        &a.execution_id,
        evidence,
        true,
    )
    .await;
    let reads = native::reads(&fake);
    tick(&f).await;
    let promote = f
        .state
        .store
        .get_workflow_operation(&op.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(promote.status, "succeeded", "{}", promote.resource_refs);
    let result = &promote.resource_refs["production_runtime_result"];
    assert_eq!(result["status"], "verified");
    assert_eq!(result["production_authorized"], true);
    assert_eq!(result["expected"], json!(released));
    assert_eq!(
        native::reads(&fake),
        reads,
        "a recorded result is not re-collected"
    );
    let deployment = f
        .state
        .store
        .get_deployment_intent(&a.proposal.deployment_intent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        deployment.intent_json["hosted_production"]["runtime_observation"]["status"],
        "verified"
    );
    assert_eq!(
        deployment.intent_json["hosted_production"]["gitops_observation"]["status"],
        "committed"
    );
    assert!(stage_outcome(&f, "release").await.is_none());

    tick(&f).await;
    let release = stage_outcome(&f, "release").await.unwrap();
    assert_eq!(release.status, "succeeded");
    assert_eq!(release.outcome["origin"], "controller");
    assert_eq!(
        release.outcome["pinned_inputs"]["workflow_operation_id"],
        promote.id
    );
    assert_eq!(
        release.outcome["pinned_inputs"]["result_artifact_id"],
        result["artifact_id"]
    );
    assert_eq!(
        f.state
            .store
            .get_stage_execution(&release.stage_execution_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "succeeded"
    );
    let observe = operation(&f).await;
    assert_eq!(observe.action, OBSERVE);
    assert_eq!(observe.effect, "observation");
    assert!(!observe
        .resource_keys
        .iter()
        .any(|k| k == "gitops:lucas_engineering:main"));
    assert_eq!(
        observe.resource_refs["observe_dispatch"]["release_outcome_id"],
        release.id
    );
    assert!(stage_outcome(&f, "observe").await.is_none());
    assert!(f
        .state
        .store
        .get_repo_work_item_metadata(&f.work_item_id)
        .await
        .unwrap()
        .unwrap()
        .closed_at
        .is_none());
    assert_eq!(creates(&fake), 4, "post-release observation starts no Job");

    crate::app::clock::advance_for_test(15 * 60 * 1000);
    let evidence = native::evidence_for(
        &f,
        &fake,
        released.clone(),
        FinanceVerificationPhase::Production,
        90_000,
    )
    .await;
    production::seed_observe_window(&f.state, &f.work_item_id, evidence, true).await;
    tick(&f).await;
    let observed = f
        .state
        .store
        .get_workflow_operation(&observe.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(observed.status, "succeeded", "{}", observed.resource_refs);
    assert_eq!(
        observed.resource_refs["observe_runtime_result"]["status"],
        "verified"
    );
    tick(&f).await;
    let sealed = stage_outcome(&f, "observe").await.unwrap();
    assert_eq!(sealed.status, "succeeded");
    assert_eq!(
        sealed.stage_execution_id,
        format!(
            "stage_observe_{}",
            observe.id.trim_start_matches("workflowop_")
        )
    );
    assert_eq!(
        sealed.outcome["pinned_inputs"]["workflow_operation_id"],
        observe.id
    );
    let metadata = f
        .state
        .store
        .get_repo_work_item_metadata(&f.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert!(metadata.closed_at.is_some());
    assert_eq!(
        f.state
            .store
            .get_work_item(&f.work_item_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "completed"
    );
    let state = reconciliation(&f).await;
    assert_eq!(state.condition, "closed", "{}", state.condition_reason);
    let reads = native::reads(&fake);
    f.state
        .store
        .wake_workflow(&f.work_item_id, now())
        .await
        .unwrap();
    assert!(
        !crate::app::hosted_controller::reconcile_once(&f.state, "replacement-api")
            .await
            .unwrap(),
        "a closed WorkItem is never claimed again"
    );
    assert_eq!(reconciliation(&f).await.condition, "closed");
    assert_eq!(native::reads(&fake), reads);
    assert_eq!(creates(&fake), 4);
    assert!(f
        .state
        .store
        .active_workflow_operation(&f.work_item_id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        deployment.intent_json["hosted_production"]["delivery"]["approval_hash"],
        a.approval.material_hash().unwrap()
    );
}

#[tokio::test]
async fn a_closed_approval_window_or_changed_evidence_admits_no_production_write() {
    let fake = KubectlFixture::new(false);
    let (f, a, plan) = promoted("production_expired", &fake).await;
    let promote = operation(&f).await.id;
    crate::app::clock::advance_for_test(31 * 60 * 1000);
    assert!(
        attempt(&f, &a, &plan).await.is_err(),
        "an expired grant admits nothing"
    );
    assert_eq!(
        context(&f, &a, false).await.unwrap().0["may_advance"],
        false
    );
    tick(&f).await;
    let op = f
        .state
        .store
        .get_workflow_operation(&promote)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(op.status, "blocked", "{}", op.resource_refs);
    assert_eq!(
        op.resource_refs["production_result"]["reason"],
        "production_approval_window_closed_before_admission"
    );
    let state = reconciliation(&f).await;
    assert_eq!(state.condition, "blocked", "{}", state.condition_reason);
    tick(&f).await;
    assert_eq!(reconciliation(&f).await.condition, "blocked");
    assert_eq!(creates(&fake), 4, "no replacement writer");
    assert!(f
        .state
        .store
        .get_artifact(&format!("production_attempt_{}", a.execution_id))
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

    let second = KubectlFixture::new(false);
    let (f, pa, gate) = proposed("production_changed", &second).await;
    let _ = decide(&f, &gate.id, "satisfied", Some("lucas"), Some("Reviewed"))
        .await
        .unwrap();
    let mut pipeline = f
        .state
        .store
        .get_pipeline_intent(&pa.pipeline_intent_id)
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
    let state = reconciliation(&f).await;
    assert_eq!(state.condition, "blocked", "{}", state.condition_reason);
    assert_eq!(
        creates(&second),
        3,
        "changed source after approval starts no writer"
    );
    assert!(f
        .state
        .store
        .get_gitops_change_set_by_deployment_intent(&pa.deployment_intent_id)
        .await
        .unwrap()
        .is_none());
}
