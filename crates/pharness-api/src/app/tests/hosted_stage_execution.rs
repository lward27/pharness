use super::characterization::test_state;
use super::repo_mode_v1::repo_fixture_with_workflow;
use crate::app::hosted_workflow::{qualified_stage, stages as hosted};
use pharness_core::{InferencePolicyRef, InferenceStage, RunId, SessionId};
use pharness_store::{
    CreateInferencePolicyQualification, CreateRun, CreateSession, WorkspaceListFilter,
};
use serde_json::json;
use std::sync::Arc;

fn enable_gateway(state: &mut crate::app::AppState) {
    state.repo_mode.coding_reliability_v2_enabled = true;
    state.build.api_revision = "a".repeat(40);
    let config = Arc::make_mut(&mut state.inference);
    config.enabled = true;
    config.registry = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../deploy/helm/pharness/files/inference-registry.json"
    )))
    .unwrap();
    config.registry.finalize_hashes().unwrap();
}

async fn qualification_fixture(
    state: &crate::app::AppState,
    stage: InferenceStage,
    profile_id: &str,
    attempts: u32,
) -> (
    InferencePolicyRef,
    pharness_core::AgentProfile,
    pharness_core::ResolvedInferenceBinding,
) {
    let reference =
        crate::app::inference::policy_reference(state, stage, profile_id, None).unwrap();
    let (suite, profile, binding) =
        crate::app::inference::qualification_binding_for_policy(state, &reference).unwrap();
    state
        .store
        .create_inference_policy_qualification(CreateInferencePolicyQualification {
            id: format!("qual_fixture_{profile_id}"),
            policy_id: reference.policy_id.clone(),
            policy_revision: reference.revision.clone(),
            policy_hash: binding.policy.policy_hash.clone(),
            target_id: binding.target.target_id.clone(),
            target_revision: binding.target.revision.clone(),
            target_hash: binding.target.config_hash.clone(),
            agent_profile_id: profile.id.clone(),
            agent_profile_hash: binding.agent_profile_hash.clone(),
            suite_id: suite.into(),
            suite_hash: pharness_core::inference_qualification_suite_hash(suite).unwrap(),
            runtime_revision: state.build.api_revision.clone(),
            attempts,
            metrics: json!({"fixture_only":true}),
            verdict: "passed".into(),
            evidence_artifact_id: None,
            actor: "unit-test".into(),
            reason: "deterministic qualification fixture; no live provider claim".into(),
        })
        .await
        .unwrap();
    (reference, profile, binding)
}

#[tokio::test]
async fn hosted_qualification_matches_frozen_suite_without_conflating_live_tool_bindings() {
    let mut state = test_state().await;
    enable_gateway(&mut state);
    let (_, expected_profile, frozen) =
        qualification_fixture(&state, InferenceStage::Implement, "repo-builder", 2).await;
    let (profile, selection) = qualified_stage(
        &state,
        "implement",
        "repo-builder",
        InferenceStage::Implement,
        None,
    )
    .await
    .unwrap();
    assert_eq!(profile, expected_profile);
    assert_eq!(
        selection["qualification_profile_hash"],
        frozen.agent_profile_hash
    );
    assert_ne!(selection["agent_profile_hash"], frozen.agent_profile_hash,
        "the frozen unit-command tool schema must remain distinguishable from an unbound live stage");
    state.build.api_revision = "b".repeat(40);
    assert!(qualified_stage(
        &state,
        "implement",
        "repo-builder",
        InferenceStage::Implement,
        None
    )
    .await
    .is_err());
}

#[tokio::test]
async fn hosted_builder_and_repair_reject_a_single_qualifying_run() {
    let mut state = test_state().await;
    enable_gateway(&mut state);
    for (stage, profile) in [("implement", "repo-builder"), ("repair", "repo-repair")] {
        qualification_fixture(&state, InferenceStage::Implement, profile, 1).await;
        assert!(
            qualified_stage(&state, stage, profile, InferenceStage::Implement, None)
                .await
                .is_err(),
            "{profile} must require two independent runs"
        );
    }
}

#[tokio::test]
async fn hosted_stage_configuration_changes_fail_before_new_workspaces_or_runs() {
    let fixture = repo_fixture_with_workflow("hosted_gateway_disabled", false, true).await;
    let filter = WorkspaceListFilter {
        work_item_id: Some(fixture.work_item_id.clone()),
        limit: 100,
        ..Default::default()
    };
    let before = fixture
        .state
        .store
        .list_workspaces(filter.clone())
        .await
        .unwrap();
    let stages = fixture
        .state
        .store
        .list_stage_executions(&fixture.work_item_id)
        .await
        .unwrap();
    let planner = crate::app::repo_mode::start_repo_planner(
        &fixture.state,
        &fixture.work_item_id,
        "operator",
        "resume saved workflow",
    )
    .await
    .unwrap_err();
    assert!(planner.message.contains("cannot fall back"));
    let chain = crate::app::repo_mode::authorize_repo_stage_chain(
        &fixture.state,
        &fixture.work_item_id,
        "operator",
        "resume saved workflow",
        None,
        None,
        None,
    )
    .await
    .unwrap_err();
    assert!(chain.message.contains("cannot fall back"));
    assert_eq!(
        fixture.state.store.list_workspaces(filter).await.unwrap(),
        before
    );
    assert_eq!(
        fixture
            .state
            .store
            .list_stage_executions(&fixture.work_item_id)
            .await
            .unwrap(),
        stages
    );
}

#[tokio::test]
async fn resumed_hosted_run_cannot_omit_its_authorization_and_use_a_direct_provider() {
    let mut fixture = repo_fixture_with_workflow("hosted_missing_marker", false, true).await;
    enable_gateway(&mut fixture.state);
    let session_id = SessionId::new("ses_hosted_resume");
    fixture
        .state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: "Resume contract check".into(),
            cwd: "/workspace".into(),
        })
        .await
        .unwrap();
    let run = fixture.state.store.create_run(CreateRun {
        id: RunId::new("run_hosted_resume"), session_id, user_task: "Resume pinned work".into(),
        cwd: "/workspace".into(), max_turns: 1, initial_status: "queued".into(),
        execution_target_json: json!({"run_scope":{"work_item_id":fixture.work_item_id},"inference":{"mode":"direct_fireworks"}}),
    }).await.unwrap();
    let error = crate::app::inference::ensure_run_inference_selection(&fixture.state, &run)
        .await
        .unwrap_err();
    assert!(error.message.contains("does not carry"));
    assert!(fixture
        .state
        .store
        .get_stage_inference_selection_for_run(run.id.as_str())
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn saved_stage_policy_ignores_changed_defaults_and_rejects_override_or_profile_drift() {
    let fixture = repo_fixture_with_workflow("hosted_policy_defaults", false, true).await;
    let mut state = fixture.state;
    enable_gateway(&mut state);
    let mut metadata = state
        .store
        .get_repo_work_item_metadata(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    let mut policy = metadata.workflow_policy.take().unwrap();
    policy.agent_profiles.clear();
    for (key, id, stage) in [
        ("plan", "repo-planner", InferenceStage::Plan),
        ("implement", "repo-builder", InferenceStage::Implement),
        ("repair", "repo-repair", InferenceStage::Implement),
        (
            "test_diagnosis",
            "repo-test-diagnoser",
            InferenceStage::Test,
        ),
        ("verify", "repo-verifier", InferenceStage::Verify),
    ] {
        qualification_fixture(&state, stage, id, 2).await;
        let (profile, selection) = qualified_stage(&state, key, id, stage, None).await.unwrap();
        policy.agent_profiles.push(profile);
        policy.stage_inference[key] = selection;
    }
    policy.validate().unwrap();
    metadata.workflow_policy_hash =
        Some(pharness_core::canonical_json_sha256(&json!(policy)).unwrap());
    metadata.workflow_policy = Some(policy);
    let saved = hosted::pinned_policy_ref(&metadata, "repo-builder")
        .unwrap()
        .unwrap();
    Arc::make_mut(&mut state.inference)
        .registry
        .defaults
        .insert(
            InferenceStage::Implement,
            InferencePolicyRef {
                policy_id: "unavailable-new-default".into(),
                revision: "v99".into(),
            },
        );
    hosted::validate_preview(&state, &metadata, "repo-builder", None)
        .await
        .unwrap();
    let changed = InferencePolicyRef {
        policy_id: saved.policy_id,
        revision: "v99".into(),
    };
    assert!(
        hosted::validate_preview(&state, &metadata, "repo-builder", Some(&changed))
            .await
            .unwrap_err()
            .message
            .contains("cannot override")
    );
    let policy = metadata.workflow_policy.as_mut().unwrap();
    policy
        .agent_profiles
        .iter_mut()
        .find(|p| p.id == "repo-builder")
        .unwrap()
        .prompt_version = "changed-implementation".into();
    metadata.workflow_policy_hash =
        Some(pharness_core::canonical_json_sha256(&json!(policy)).unwrap());
    let error = hosted::validate_preview(&state, &metadata, "repo-builder", None)
        .await
        .unwrap_err();
    assert!(
        error.message.contains("profile implementation changed"),
        "{}",
        error.message
    );
}

#[tokio::test]
async fn hosted_repair_binds_qualified_implementation_and_resumes_without_reselection() {
    let mut state = test_state().await;
    enable_gateway(&mut state);
    let mut policy: pharness_core::hosted_sdlc::HostedWorkflowPolicySnapshot =
        serde_json::from_str(include_str!(
            "../../../../pharness-core/tests/fixtures/hosted-workflow.json"
        ))
        .unwrap();
    policy.agent_profiles.clear();
    for (key, id, stage) in [
        ("plan", "repo-planner", InferenceStage::Plan),
        ("implement", "repo-builder", InferenceStage::Implement),
        ("repair", "repo-repair", InferenceStage::Implement),
        (
            "test_diagnosis",
            "repo-test-diagnoser",
            InferenceStage::Test,
        ),
        ("verify", "repo-verifier", InferenceStage::Verify),
    ] {
        qualification_fixture(&state, stage, id, 2).await;
        let (profile, selection) = qualified_stage(&state, key, id, stage, None).await.unwrap();
        policy.agent_profiles.push(profile);
        policy.stage_inference[key] = selection;
    }
    let fixture = super::repo_mode_v1::repo_fixture_with_policy(
        "hosted_repair_resume",
        false,
        state,
        Some(policy),
    )
    .await;
    let state = fixture.state;
    let metadata = state
        .store
        .get_repo_work_item_metadata(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    let profile = hosted::pinned_profile(&metadata, "repo-repair")
        .unwrap()
        .unwrap();
    let reference = hosted::pinned_policy_ref(&metadata, "repo-repair")
        .unwrap()
        .unwrap();
    let planned = crate::app::inference::create_planned_selection(
        &state,
        crate::app::inference::PlannedSelectionRequest {
            subject_kind: "work_item",
            subject_id: &fixture.work_item_id,
            stage: InferenceStage::Implement,
            profile: &json!(profile),
            requested: Some(&reference),
            actor: "controller",
            reason: "record authorized repair",
            state_hash: metadata.workflow_policy_hash.as_deref().unwrap(),
        },
    )
    .await
    .unwrap();
    let session_id = SessionId::new("ses_hosted_repair_resume");
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: "Repair resume".into(),
            cwd: "/workspace".into(),
        })
        .await
        .unwrap();
    let execution = hosted::bind_run(&metadata, "repo-repair", json!({
        "run_scope":{"work_item_id":fixture.work_item_id},
        "repo_mode":{"stage":"implement"},
        "agent_profile":profile, "run_budget":profile.budget,
        "repository_contract":{"acceptance_commands":[{"name":"unit","command":"python -m unittest"}]},
        "selected_acceptance_commands":["python -m unittest"],
        "inference":crate::app::inference::execution_marker_for_selection(&state, &planned),
    })).unwrap();
    let run = state
        .store
        .create_run(CreateRun {
            id: RunId::new("run_hosted_repair_resume"),
            session_id,
            user_task: "One bounded repair".into(),
            cwd: "/workspace".into(),
            max_turns: profile.budget.initial_turns,
            initial_status: "queued".into(),
            execution_target_json: execution,
        })
        .await
        .unwrap();
    let run = state
        .store
        .set_run_budget(
            &run.id,
            &profile.budget,
            &pharness_core::RunBudgetConsumption {
                allowed_turns: profile.budget.initial_turns,
                allowed_tokens: profile.budget.initial_tokens,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let first = crate::app::inference::ensure_run_inference_selection(&state, &run)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.binding.policy.policy_id, "repair-kimi-k3-v2");
    assert_eq!(first.binding.base_agent_profile_hash, profile.profile_hash);
    assert_ne!(
        first.binding.binding_hash, planned.binding_hash,
        "actual acceptance commands specialize the run binding"
    );
    let resumed = crate::app::inference::ensure_run_inference_selection(&state, &run)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.selection_id, resumed.selection_id);
    let mut changed_run = run.clone();
    changed_run.run_budget.hard_tokens += 1;
    assert!(
        crate::app::inference::ensure_run_inference_selection(&state, &changed_run)
            .await
            .unwrap_err()
            .message
            .contains("limits differ")
    );
    changed_run = run;
    changed_run.execution_target_json["inference"]["mode"] = json!("direct_fireworks");
    assert!(
        crate::app::inference::ensure_run_inference_selection(&state, &changed_run)
            .await
            .unwrap_err()
            .message
            .contains("backend differs")
    );
}

#[tokio::test]
async fn hosted_frontend_contract_registration_preserves_legacy_execution_boundary() {
    use crate::app::deployment::contracts::create_deployment_contract;
    use crate::dto::CreateDeploymentContractRequest;
    use axum::{extract::State, Json};
    let fixture = repo_fixture_with_workflow("hosted_frontend_contract", false, true).await;
    let contract = json!({"operation":"sync","workload_kind":"Deployment","workload_name":"finance-frontend",
        "service_name":"finance-frontend","service_port":8080,"health_path":"/",
        "post_sync_verification":{"service_healthz":"required"}});
    for (namespace, application, port, valid) in [
        ("apps-prod", "finance-frontend", 8080, true),
        ("apps-staging", "finance-frontend", 8080, false),
        ("apps-prod", "unbounded-application", 8080, false),
        ("apps-prod", "finance-frontend", 8090, false),
    ] {
        let mut spec = contract.clone();
        spec["service_port"] = json!(port);
        let result = create_deployment_contract(
            State(fixture.state.clone()),
            None,
            Json(CreateDeploymentContractRequest {
                target_environment: "production".into(),
                target_namespace: namespace.into(),
                argo_application: application.into(),
                version: Some("v1".into()),
                contract_json: spec,
                actor: Some("operator".into()),
                reason: Some("finite contract declaration".into()),
            }),
        )
        .await;
        assert_eq!(result.is_ok(), valid, "{namespace}/{application}:{port}");
    }
    let mut work_item = fixture
        .state
        .store
        .get_work_item(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    work_item.target_environment = "production".into();
    work_item.target_namespace = Some("apps-prod".into());
    work_item.argo_application = Some("finance-frontend".into());
    work_item.production_impacting = true;
    assert!(
        crate::app::deployment::target::ensure_supported_deployment_target(
            &work_item,
            &crate::app::deployment::target::DeploymentTarget {
                environment: "production".into(),
                namespace: "apps-prod".into(),
                application: "finance-frontend".into(),
            }
        )
        .is_err(),
        "registering the hosted contract must not authorize legacy Argo mutation"
    );
}

#[tokio::test]
async fn hosted_readiness_and_creation_preserve_exact_planner_authorization() {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use pharness_store::{
        CreateDeploymentContract, CreatePipelineContract, CreateRepositoryContractVersion,
    };
    use tower::ServiceExt;
    let mut state = super::characterization::test_state_with_git_and_gitops(
        "/bin/false".into(),
        "https://github.com/lward27/yfinance_wrapper.git".into(),
        "https://github.com/lward27/lucas_engineering.git".into(),
    )
    .await;
    enable_gateway(&mut state);
    for (id, stage) in [
        ("repo-planner", InferenceStage::Plan),
        ("repo-builder", InferenceStage::Implement),
        ("repo-repair", InferenceStage::Implement),
        ("repo-test-diagnoser", InferenceStage::Test),
        ("repo-verifier", InferenceStage::Verify),
    ] {
        qualification_fixture(&state, stage, id, 2).await;
    }
    let mut fixture = super::repo_mode_v1::repo_fixture_for_source(
        "hosted_planner_preview",
        false,
        state,
        None,
        Some("https://github.com/lward27/yfinance_wrapper.git"),
    )
    .await;
    let metadata = fixture
        .state
        .store
        .get_repo_work_item_metadata(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    let item = fixture
        .state
        .store
        .get_work_item(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    let old = fixture
        .state
        .store
        .get_repository_contract_version(&metadata.repository_contract_version_id)
        .await
        .unwrap()
        .unwrap();
    let mut contract = item.repository_contract_json.unwrap();
    contract["dependency_lock"]["kind"] = json!("pip_requirements");
    contract["dependency_lock"]["sha256"] = json!("d".repeat(64));
    fixture
        .state
        .store
        .create_repository_contract_version(CreateRepositoryContractVersion {
            id: "rcontract_preview".into(),
            repository_id: metadata.repository_id.clone(),
            onboarding_id: old.onboarding_id,
            source_commit: item.source_commit.clone().unwrap(),
            content_hash: crate::app::hashing::canonical_material_hash(&contract).unwrap(),
            contract,
            merge_provenance: json!({"fixture_only":true}),
        })
        .await
        .unwrap();
    fixture.state.store.create_pipeline_contract(CreatePipelineContract {
        id: "pipeline_test".into(), status: "active".into(), namespace: "tekton-pipelines".into(),
        pipeline_ref: "pharness-yfinance-build".into(), version: "v1".into(), actor: None, reason: None,
        contract_json: json!({"params":[{"name":"revision","type":"scalar","required":true}],"source_revision_param":"revision"}),
    }).await.unwrap();
    for (id, environment, namespace, application) in [
        (
            "staging_test",
            "staging",
            "apps-staging",
            "yfinance-staging",
        ),
        (
            "production_test",
            "production",
            "apps-prod",
            "yfinance-wrapper",
        ),
    ] {
        fixture.state.store.create_deployment_contract(CreateDeploymentContract {
            id: id.into(), status: "active".into(), target_environment: environment.into(), target_namespace: namespace.into(),
            argo_application: application.into(), version: "v1".into(), actor: None, reason: None,
            contract_json: json!({"operation":"sync","workload_kind":"Deployment","workload_name":"yfinance-wrapper",
                "service_name":"yfinance-wrapper","service_port":8090,"health_path":"/healthz",
                "post_sync_verification":{"service_healthz":"required"}}),
        }).await.unwrap();
    }
    let policy: pharness_core::hosted_sdlc::HostedWorkflowPolicySnapshot = serde_json::from_str(
        include_str!("../../../../pharness-core/tests/fixtures/hosted-workflow.json"),
    )
    .unwrap();
    let mut binding = policy.delivery_binding;
    binding.product_id = metadata.product_id.clone();
    binding.repository_id = metadata.repository_id.clone();
    binding.source_repo = item.source_repo;
    binding.gitops_repo = "https://github.com/lward27/lucas_engineering.git".into();
    binding.image_name = "registry.lucas.engineering/yfinance_wrapper".into();
    binding.staging.kustomization_path =
        "charts/finance-staging/yfinance/kustomization.yaml".into();
    binding.production.kustomization_path = "charts/yfinance-wrapper/kustomization.yaml".into();
    fixture.state.hosted_workflow = Arc::new(pharness_core::hosted_sdlc::HostedWorkflowConfig {
        enabled: true,
        bindings: vec![binding],
    });
    let response = crate::app::repo_mode::router().with_state(fixture.state.clone()).oneshot(Request::builder()
        .method("POST").uri(format!("/api/products/{}/work-items/preflight",metadata.product_id))
        .header("content-type","application/json").body(Body::from(json!({
            "title":"Use the recorded Planner", "intent":"Inspect qualification and planner identity", "repository_id":metadata.repository_id,
        }).to_string())).unwrap()).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 100_000).await.unwrap()).unwrap();
    assert!(body["workflow_policy"].is_object(), "{}", body["blockers"]);
    assert_eq!(
        body["planner_inference"],
        body["workflow_policy"]["stage_inference"]["plan"]
    );
    let planner = body["workflow_policy"]["agent_profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "repo-planner")
        .unwrap();
    assert_ne!(
        planner["model"], "accounts/fireworks/models/test",
        "the legacy worker default must not leak into hosted Planner authority"
    );
    assert!(
        !body["blockers"].as_array().unwrap().is_empty(),
        "this fixture deliberately does not claim complete repository readiness"
    );
    make_repository_ready(&mut fixture.state, &metadata.repository_id).await;
    let submission = json!({
        "title":"Use the recorded Planner", "intent":"Inspect qualification and planner identity", "repository_id":metadata.repository_id,
    });
    let router = crate::app::repo_mode::router().with_state(fixture.state.clone());
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/products/{}/work-items/preflight",
                    metadata.product_id
                ))
                .header("content-type", "application/json")
                .body(Body::from(submission.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let ready: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 100_000).await.unwrap()).unwrap();
    assert_eq!(ready["blockers"], json!([]), "{ready}");
    let mut creation = submission;
    creation["preflight_hash"] = ready["preflight_hash"].clone();
    creation["actor"] = json!("unit-test");
    creation["reason"] = json!("test exact hosted creation; no live provider or deployment proof");
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/products/{}/work-items", metadata.product_id))
                .header("content-type", "application/json")
                .body(Body::from(creation.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let created: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 100_000).await.unwrap()).unwrap();
    assert_eq!(status, axum::http::StatusCode::OK, "{created}");
    assert_eq!(created["work_item"]["workflow_kind"], "hosted_sdlc");
    assert_eq!(
        created["repo_mode"]["workflow_policy"],
        ready["workflow_policy"]
    );
    assert_eq!(created["discover_outcome"]["origin"], "controller");
    let created_id = created["work_item"]["id"].as_str().unwrap();
    let flow = crate::app::repo_mode::repo_work_item_flow(&fixture.state, created_id)
        .await
        .unwrap();
    assert_eq!(flow.work_item.workflow_kind, "hosted_sdlc");
    assert_eq!(flow.delivery_configuration["release"]["required"], true);
    assert!(flow.work_item.closed_at.is_none());
    assert!(flow.workspaces.is_empty());
    assert_eq!(
        fixture
            .state
            .store
            .list_effective_stage_outcomes(created_id)
            .await
            .unwrap()
            .len(),
        1
    );
    fixture
        .state
        .store
        .update_deployment_contract_status("staging_test", "retired", None, None)
        .await
        .unwrap();
    fixture.state.store.create_deployment_contract(CreateDeploymentContract {
        id: "staging_probe_disabled".into(), status: "active".into(), target_environment: "staging".into(),
        target_namespace: "apps-staging".into(), argo_application: "yfinance-staging".into(),
        version: "v2".into(), actor: None, reason: None,
        contract_json: json!({"operation":"sync","workload_kind":"Deployment","workload_name":"yfinance-wrapper",
            "service_name":"yfinance-wrapper","service_port":8090,"health_path":"/healthz"}),
    }).await.unwrap();
    Arc::make_mut(&mut fixture.state.hosted_workflow).bindings[0]
        .staging
        .deployment_contract_id = "staging_probe_disabled".into();
    let repository = fixture
        .state
        .store
        .get_repository(&metadata.repository_id)
        .await
        .unwrap()
        .unwrap();
    let error = crate::app::hosted_workflow::resolve_policy(
        &fixture.state,
        &metadata.product_id,
        &repository,
        &Default::default(),
        2,
        None,
    )
    .await
    .unwrap_err();
    assert!(error.message.contains("required health probe"));
}

async fn make_repository_ready(state: &mut crate::app::AppState, repository_id: &str) {
    use pharness_store::{CreateCapabilityVerification, CreateRepositoryReadinessAssessment};
    let repository = state
        .store
        .get_repository(repository_id)
        .await
        .unwrap()
        .unwrap();
    let version = state
        .store
        .latest_repository_contract_version(repository_id, &repository.registered_commit)
        .await
        .unwrap()
        .unwrap();
    let contract: pharness_core::RepositoryContract =
        serde_json::from_value(version.contract.clone()).unwrap();
    let profile: pharness_core::EnvironmentProfile = serde_json::from_value(json!({
        "id":"python-3.11", "active":true, "image":format!("example.test/python@sha256:{}","d".repeat(64)),
        "revision":"a".repeat(40), "platform":"linux/amd64", "required_executables":["pharness-worker","git","python","pip"],
        "preparation_strategy":"python_hashed_requirements", "service_account":"pharness-python-runner",
        "repository_allowlist":[repository.canonical_url], "limits":{"cpu":"1","memory":"1Gi","ephemeral_storage":"1Gi"},
    })).unwrap();
    contract.validate_for_profile(&profile).unwrap();
    state.environment_profiles = Arc::new(vec![profile.clone()]);
    let now = crate::app::clock::current_millis();
    let mut verifications = Vec::new();
    for (id, capability) in [
        ("capverify_hosted_source", "source_reader"),
        (
            "capverify_hosted_profile",
            "environment_profile:python-3.11",
        ),
    ] {
        verifications.push(
            state
                .store
                .create_capability_verification(CreateCapabilityVerification {
                    id: id.into(),
                    capability: capability.into(),
                    status: "available".into(),
                    summary: "deterministic readiness fixture".into(),
                    principal: None,
                    repository: Some(repository.canonical_url.clone()),
                    permission: None,
                    verified_at: now.to_string(),
                    expires_at: (now + 900_000).to_string(),
                })
                .await
                .unwrap(),
        );
    }
    let input = json!({
        "schema_version":"pharness.dev/repository-readiness-input/v1alpha1",
        "repository_id":repository.id, "source_commit":repository.registered_commit,
        "contract_version_id":version.id, "contract_hash":version.content_hash, "dependency_lock_hash":contract.dependency_lock.sha256,
        "environment_profile_id":profile.id, "environment_profile_revision":profile.revision, "runner_image":profile.image,
        "validation_policy_version":"repo-mode-v1", "required_executables":profile.required_executables, "acceptance_commands":contract.acceptance_commands,
        "capability_evidence":{
            "source_reader":{"id":verifications[0].id,"verified_at":verifications[0].verified_at,"expires_at":verifications[0].expires_at},
            "environment_profile":{"id":verifications[1].id,"verified_at":verifications[1].verified_at,"expires_at":verifications[1].expires_at},
        },
    });
    state.store.create_repository_readiness_assessment(CreateRepositoryReadinessAssessment {
        id:"rready_hosted_current".into(), repository_id:repository.id, source_commit:repository.registered_commit,
        contract_version_id:Some(version.id), contract_hash:Some(version.content_hash), dependency_lock_hash:Some(contract.dependency_lock.sha256),
        environment_profile_id:Some(profile.id), environment_profile_revision:Some(profile.revision), runner_image_digest:Some(format!("sha256:{}","d".repeat(64))),
        validation_policy_version:"repo-mode-v1".into(), contract_status:"ready".into(), coding_status:"ready".into(), checks:json!([]), blockers:json!([]), warnings:json!([]),
        evidence_refs:json!(verifications.iter().map(|v|json!({"kind":"capability_verification","id":v.id,"capability":v.capability})).collect::<Vec<_>>()),
        input_hash:crate::app::hashing::canonical_material_hash(&input).unwrap(), content_hash:"sha256:readiness-test-fixture".into(), expires_at:Some((now + 900_000).to_string()),
    }).await.unwrap();
}
