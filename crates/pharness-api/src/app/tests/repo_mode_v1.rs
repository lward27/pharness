use super::characterization::test_state;
use super::{
    internal_source_delivery_observation_outcome, json, ApproveRepositoryOnboardingProposal,
    CreateChangeSet, CreateProductAggregate, CreateRepoWorkItem, CreateRepositoryContractVersion,
    CreateRepositoryOnboardingProposal, CreateRepositoryReadinessAssessment, CreateSession,
    CreateSourceDeliveryIntent, CreateStageExecution, CreateWorkPlan, CreateWorkspace,
    GitDeliveryObservationOutcomeRequest, Json, Path, RegisterRepositoryAggregate, RunBudget,
    RunId, SessionId, State, StoredRepositoryDraft, Value,
};

const SOURCE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HEAD_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const MERGE_SHA: &str = "cccccccccccccccccccccccccccccccccccccccc";

struct RepoDeliveryFixture {
    state: super::AppState,
    work_item_id: String,
    intent_id: String,
}

async fn repo_delivery_fixture(suffix: &str) -> RepoDeliveryFixture {
    let state = test_state().await;
    state
        .store
        .ensure_bootstrap_organization(&state.repo_mode.organization)
        .await
        .unwrap();

    let product_id = format!("prod_{suffix}");
    let initial_snapshot_id = format!("pmodel_initial_{suffix}");
    state
        .store
        .create_product(CreateProductAggregate {
            id: product_id.clone(),
            organization_id: state.repo_mode.organization.id.clone(),
            product_key: format!("product-{suffix}"),
            display_name: format!("Product {suffix}"),
            description: "Repo Mode fake-provider fixture".into(),
            owner_principal: "operator".into(),
            snapshot_id: initial_snapshot_id,
            snapshot_json: json!({"schema_version":"pharness.dev/product-model/v1alpha1","repositories":[]}),
            snapshot_hash: format!("sha256:initial-{suffix}"),
            actor: "operator".into(),
            reason: "create deterministic Repo Mode fixture".into(),
        })
        .await
        .unwrap();

    let repository_id = format!("repo_{suffix}");
    let onboarding_id = format!("onboard_{suffix}");
    let registration_snapshot_id = format!("pmodel_registered_{suffix}");
    let registered = state
        .store
        .register_repository(RegisterRepositoryAggregate {
            repository: StoredRepositoryDraft {
                id: repository_id.clone(),
                provider: "github".into(),
                external_id: format!("example/repo-{suffix}"),
                canonical_url: format!("https://github.com/example/repo-{suffix}.git"),
                default_branch: "main".into(),
                registered_commit: SOURCE_SHA.into(),
            },
            binding_id: format!("rbind_{suffix}"),
            binding_revision_id: format!("rbindrev_{suffix}"),
            onboarding_id: onboarding_id.clone(),
            binding_content_hash: format!("sha256:binding-{suffix}"),
            evidence_json: json!({"source_commit":SOURCE_SHA}),
            product_id: product_id.clone(),
            expected_product_state_version: 1,
            snapshot_id: registration_snapshot_id.clone(),
            snapshot_json: json!({"schema_version":"pharness.dev/product-model/v1alpha1","repositories":[repository_id]}),
            snapshot_hash: format!("sha256:registered-{suffix}"),
            actor: "operator".into(),
            reason: "register immutable repository revision".into(),
        })
        .await
        .unwrap();

    let discovery_id = format!("rdisc_{suffix}");
    let discovery_hash = format!("sha256:discovery-{suffix}");
    state
        .store
        .create_repository_discovery(&discovery_id, &onboarding_id, SOURCE_SHA)
        .await
        .unwrap();
    state
        .store
        .finish_repository_discovery(
            &discovery_id,
            SOURCE_SHA,
            &json!({
                "schema_version":"pharness.dev/repository-discovery/v1alpha1",
                "repository":{"id":registered.repository.id,"source_commit":SOURCE_SHA},
                "contract_files":[".pharness/repository.yaml"],
                "dependency_locks":["requirements.lock"],
            }),
            &discovery_hash,
        )
        .await
        .unwrap();

    let onboarding = state
        .store
        .get_repository_onboarding(&onboarding_id)
        .await
        .unwrap()
        .unwrap();
    let proposal_id = format!("rproposal_{suffix}");
    let proposal_hash = format!("sha256:proposal-{suffix}");
    state
        .store
        .create_repository_onboarding_proposal(CreateRepositoryOnboardingProposal {
            id: proposal_id.clone(),
            onboarding_id: onboarding_id.clone(),
            expected_state_version: onboarding.state_version,
            proposal: json!({
                "schema_version":"pharness.dev/repository-onboarding-proposal/v1alpha1",
                "discovery":{"id":discovery_id,"hash":discovery_hash},
                "candidate_contract":{"api_version":"pharness.dev/v1alpha1"},
            }),
            content_hash: proposal_hash.clone(),
            discovery_id,
            discovery_hash,
            actor: "repository-onboarding-proposer".into(),
            origin: "agent".into(),
        })
        .await
        .unwrap();
    let onboarding = state
        .store
        .get_repository_onboarding(&onboarding_id)
        .await
        .unwrap()
        .unwrap();
    state
        .store
        .approve_repository_onboarding_proposal(ApproveRepositoryOnboardingProposal {
            onboarding_id: onboarding_id.clone(),
            proposal_id,
            proposal_hash,
            expected_state_version: onboarding.state_version,
            actor: "operator".into(),
            reason: "review exact onboarding proposal".into(),
            model_change: None,
        })
        .await
        .unwrap();

    let contract_version_id = format!("rcontract_{suffix}");
    let contract_hash = format!("sha256:contract-{suffix}");
    state
        .store
        .create_repository_contract_version(CreateRepositoryContractVersion {
            id: contract_version_id.clone(),
            repository_id: repository_id.clone(),
            onboarding_id,
            source_commit: SOURCE_SHA.into(),
            contract: json!({"api_version":"pharness.dev/v1alpha1"}),
            content_hash: contract_hash.clone(),
            merge_provenance: json!({"head_sha":SOURCE_SHA,"merge_commit_sha":SOURCE_SHA}),
        })
        .await
        .unwrap();
    state
        .store
        .create_repository_readiness_assessment(CreateRepositoryReadinessAssessment {
            id: format!("rready_{suffix}"),
            repository_id: repository_id.clone(),
            source_commit: SOURCE_SHA.into(),
            contract_version_id: Some(contract_version_id.clone()),
            contract_hash: Some(contract_hash.clone()),
            dependency_lock_hash: Some(format!("sha256:lock-{suffix}")),
            environment_profile_id: Some("python-3.11".into()),
            environment_profile_revision: Some("v1".into()),
            runner_image_digest: Some(format!("sha256:{}", "d".repeat(64))),
            validation_policy_version: "repo-mode-v1".into(),
            contract_status: "ready".into(),
            coding_status: "ready".into(),
            checks: json!([{"key":"exact_checkout","status":"passing"}]),
            blockers: json!([]),
            warnings: json!([]),
            evidence_refs: json!([{"kind":"repository_discovery","id":format!("rdisc_{suffix}")}]),
            input_hash: format!("sha256:readiness-input-{suffix}"),
            content_hash: format!("sha256:readiness-{suffix}"),
            expires_at: None,
        })
        .await
        .unwrap();

    let work_item_id = format!("witem_{suffix}");
    state
        .store
        .create_repo_work_item(CreateRepoWorkItem {
            id: work_item_id.clone(),
            product_id,
            repository_id: repository_id.clone(),
            product_model_snapshot_id: registration_snapshot_id,
            product_model_snapshot_hash: format!("sha256:registered-{suffix}"),
            repository_contract_version_id: contract_version_id,
            contract_version: "pharness.dev/v1alpha1".into(),
            title: "Complete one reviewed source delivery".into(),
            intent: "Exercise the fixed Repo Mode stage and provider lifecycle".into(),
            acceptance_command_names: vec!["unit".into()],
            acceptance_commands: vec!["python -m unittest discover -s tests -v".into()],
            context_repositories: json!([]),
            source_repo: registered.repository.canonical_url.clone(),
            source_ref: "main".into(),
            source_commit: SOURCE_SHA.into(),
            environment_profile_id: "python-3.11".into(),
            run_budget: RunBudget::default(),
            max_attempts: 2,
            repository_contract_json: json!({
                "api_version":"pharness.dev/v1alpha1",
                "environment_profile":"python-3.11",
                "dependency_lock":{
                    "kind":"python_requirements",
                    "path":"requirements.lock",
                    "sha256":format!("sha256:{}", "d".repeat(64)),
                },
                "writable_paths":["src/**"],
                "acceptance_commands":[{
                    "name":"unit",
                    "command":"python -m unittest discover -s tests -v",
                }],
                "roots":{"source":["src"],"tests":["tests"],"documentation":[]},
                "agent_network":"denied",
                "package_installation":"preparation_only",
            }),
            repository_contract_hash: contract_hash,
            actor: "operator".into(),
        })
        .await
        .unwrap();
    let session_id = SessionId::new(format!("ses_{suffix}"));
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: "Repo Mode stage chain".into(),
            cwd: "/workspace".into(),
        })
        .await
        .unwrap();
    let plan_id = format!("wplan_{suffix}");
    state
        .store
        .create_work_plan(CreateWorkPlan {
            id: plan_id.clone(),
            work_item_id: Some(work_item_id.clone()),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: None,
            status: "approved".into(),
            title: "Reviewed plan".into(),
            summary: "Bounded source change".into(),
            risk_level: "low".into(),
            requires_approval: true,
            resource_namespace: None,
            resource_kind: Some("Repository".into()),
            resource_name: Some(registered.repository.canonical_url.clone()),
            work_plan_json: json!({"schema_version":"pharness.dev/work-plan/v1alpha1"}),
        })
        .await
        .unwrap();
    let change_set_id = format!("cset_{suffix}");
    state
        .store
        .create_change_set(CreateChangeSet {
            id: change_set_id.clone(),
            work_item_id: Some(work_item_id.clone()),
            work_plan_id: plan_id,
            remediation_plan_id: None,
            incident_id: None,
            session_id,
            run_id: None,
            status: "approved".into(),
            title: "Reviewed source diff".into(),
            summary: "Controller-bound source patch".into(),
            risk_level: "low".into(),
            material_hash: format!("sha256:{}", "e".repeat(64)),
            resource_namespace: None,
            resource_kind: Some("Repository".into()),
            resource_name: Some(registered.repository.canonical_url.clone()),
            change_set_json: json!({"workspace_id":format!("ws_{suffix}")}),
        })
        .await
        .unwrap();
    let intent_id = format!("srcintent_{suffix}");
    let intent = state
        .store
        .create_source_delivery_intent(CreateSourceDeliveryIntent {
            id: intent_id.clone(),
            subject_kind: "work_item_change_set".into(),
            subject_id: change_set_id,
            repository_id,
            source_repo: registered.repository.canonical_url,
            base_ref: "main".into(),
            base_commit: SOURCE_SHA.into(),
            head_branch: format!("pharness/{work_item_id}/source"),
            patch_artifact_id: Some(format!("artifact_{suffix}")),
            patch_hash: format!("sha256:{}", "f".repeat(64)),
            authorization: json!({"actor":"operator","reason":"reviewed source mutation"}),
            created_by: "operator".into(),
            creation_reason: "deliver approved ChangeSet".into(),
        })
        .await
        .unwrap();
    state
        .store
        .update_source_delivery_intent(
            &intent.id,
            intent.state_version,
            "observer_dispatched",
            Some(&format!("writer_{suffix}")),
            Some(&format!("observer_premerge_{suffix}")),
            Some(&json!({
                "number":7,
                "url":format!("https://github.com/example/repo-{suffix}/pull/7"),
                "head_branch":format!("pharness/{work_item_id}/source"),
                "head_sha":HEAD_SHA,
            })),
            None,
            None,
            "controller:repo-mode",
            "dispatch deterministic provider observer",
        )
        .await
        .unwrap();

    RepoDeliveryFixture {
        state,
        work_item_id,
        intent_id,
    }
}

fn provider_observation(execution_id: &str, merged: bool) -> GitDeliveryObservationOutcomeRequest {
    GitDeliveryObservationOutcomeRequest {
        execution_id: execution_id.into(),
        status: "observed".into(),
        pull_request_state: Some(if merged { "closed" } else { "open" }.into()),
        merged: Some(merged),
        merge_commit_sha: merged.then(|| MERGE_SHA.into()),
        head_branch: Some("pharness/test/source".into()),
        head_commit_sha: Some(HEAD_SHA.into()),
        error_code: None,
        authoritative_rules_succeeded: true,
        required_checks: json!([]),
        check_runs: json!([]),
        commit_statuses: json!([]),
        provider_check_status: Some("passing".into()),
    }
}

#[tokio::test]
async fn repo_mode_fake_provider_closes_only_after_fresh_checks_and_exact_merge() {
    let fixture = repo_delivery_fixture("success").await;
    let Json(pre_merge) = internal_source_delivery_observation_outcome(
        State(fixture.state.clone()),
        Path(fixture.intent_id.clone()),
        Json(provider_observation("observer_premerge_success", false)),
    )
    .await
    .unwrap();
    assert_eq!(
        pre_merge["source_delivery_intent"]["status"],
        "waiting_merge"
    );

    let intent = fixture
        .state
        .store
        .get_source_delivery_intent(&fixture.intent_id)
        .await
        .unwrap()
        .unwrap();
    fixture
        .state
        .store
        .update_source_delivery_intent(
            &intent.id,
            intent.state_version,
            "observer_dispatched",
            None,
            Some("observer_merge_success"),
            None,
            None,
            None,
            "controller:repo-mode",
            "observe exact manual merge",
        )
        .await
        .unwrap();
    let Json(merged) = internal_source_delivery_observation_outcome(
        State(fixture.state.clone()),
        Path(fixture.intent_id.clone()),
        Json(provider_observation("observer_merge_success", true)),
    )
    .await
    .unwrap();
    assert_eq!(merged["delivery_status"], "succeeded");

    let work_item = fixture
        .state
        .store
        .get_work_item(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(work_item.status, "completed");
    let metadata = fixture
        .state
        .store
        .get_repo_work_item_metadata(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert!(metadata.closed_at.is_some());
    let outcomes = fixture
        .state
        .store
        .list_effective_stage_outcomes(&fixture.work_item_id)
        .await
        .unwrap();
    let statuses = outcomes
        .iter()
        .map(|outcome| (outcome.stage_key.as_str(), outcome.status.as_str()))
        .collect::<Vec<_>>();
    assert!(statuses.contains(&("source_delivery", "succeeded")));
    assert!(statuses.contains(&("release", "inapplicable")));
    assert!(statuses.contains(&("observe", "inapplicable")));
}

#[tokio::test]
async fn repo_mode_fake_provider_closes_failed_when_merge_lacks_premerge_evidence() {
    let fixture = repo_delivery_fixture("missing_checks").await;
    let Json(merged) = internal_source_delivery_observation_outcome(
        State(fixture.state.clone()),
        Path(fixture.intent_id.clone()),
        Json(provider_observation(
            "observer_premerge_missing_checks",
            true,
        )),
    )
    .await
    .unwrap();
    assert_eq!(merged["delivery_status"], "failed");
    let work_item = fixture
        .state
        .store
        .get_work_item(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(work_item.status, "failed");
    assert!(fixture
        .state
        .store
        .get_repo_work_item_metadata(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap()
        .closed_at
        .is_some());
}

#[tokio::test]
async fn worker_boundary_failure_seals_repo_stage_and_blocks_for_correction() {
    let fixture = repo_delivery_fixture("worker_boundary_failure").await;
    let run_id = RunId::new("run_worker_boundary_failure");
    let stage_execution_id = "stageexec_worker_boundary_failure";
    fixture
        .state
        .store
        .create_run(super::CreateRun {
            id: run_id.clone(),
            session_id: SessionId::new("ses_worker_boundary_failure"),
            user_task: "verify retained workspace".into(),
            cwd: "/workspace".into(),
            max_turns: 12,
            initial_status: "queued".into(),
            execution_target_json: json!({
                "repo_mode": {
                    "stage": "verify",
                    "stage_execution_id": stage_execution_id,
                },
            }),
        })
        .await
        .unwrap();
    fixture
        .state
        .store
        .create_stage_execution(CreateStageExecution {
            id: stage_execution_id.into(),
            work_item_id: fixture.work_item_id.clone(),
            stage_key: "verify".into(),
            sequence: 1,
            status: "queued".into(),
            agent_profile_id: Some("repo-verifier".into()),
            agent_profile_version: Some("v1".into()),
            agent_profile_hash: Some("sha256:verifier".into()),
            context_pack_id: None,
            run_id: Some(run_id.clone()),
            workspace_id: None,
            input_snapshot: json!({"source_commit": SOURCE_SHA}),
            input_hash: "sha256:worker-boundary-input".into(),
        })
        .await
        .unwrap();

    crate::worker::fail_run_from_dispatch(
        &fixture.state.store,
        &run_id,
        "worker job failed before reporting a durable outcome".into(),
    )
    .await
    .unwrap();

    let run = fixture.state.store.get_run(&run_id).await.unwrap().unwrap();
    assert_eq!(run.status, "failed");
    assert_eq!(run.budget_consumption.turns_used, 0);
    let execution = fixture
        .state
        .store
        .get_stage_execution(stage_execution_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(execution.status, "failed");
    assert_eq!(
        execution.stop_reason.as_deref(),
        Some("worker job failed before reporting a durable outcome")
    );
    let outcome = fixture
        .state
        .store
        .get_stage_outcome_for_execution(stage_execution_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(outcome.status, "failed");
    assert_eq!(
        outcome.outcome["stop_reason"],
        "worker job failed before reporting a durable outcome"
    );
    assert_eq!(
        fixture
            .state
            .store
            .get_work_item(&fixture.work_item_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "blocked"
    );
}

#[tokio::test]
async fn zero_turn_stage_startup_recovery_refunds_attempt_and_seals_evidence() {
    let fixture = repo_delivery_fixture("startup_recovery").await;
    let run_id = RunId::new("run_startup_recovery");
    let stage_execution_id = "stageexec_startup_recovery";
    let workspace_id = "ws_startup_recovery";
    fixture
        .state
        .store
        .create_workspace(CreateWorkspace {
            id: workspace_id.into(),
            work_item_id: fixture.work_item_id.clone(),
            run_id: None,
            status: "preparing".into(),
            source_repo: "https://github.com/example/repo-startup_recovery.git".into(),
            source_ref: "main".into(),
            resolved_commit: Some(SOURCE_SHA.into()),
            branch: Some("pharness/startup-recovery/attempt-2".into()),
            retention_status: "retained".into(),
            actor: Some("operator".into()),
            reason: Some("exercise exact startup recovery".into()),
        })
        .await
        .unwrap();
    fixture
        .state
        .store
        .create_session(CreateSession {
            id: SessionId::new("ses_startup_recovery"),
            title: "Repo Mode startup recovery".into(),
            cwd: "/workspace".into(),
        })
        .await
        .unwrap();
    fixture
        .state
        .store
        .create_run(super::CreateRun {
            id: run_id.clone(),
            session_id: SessionId::new("ses_startup_recovery"),
            user_task: "start the bounded Builder".into(),
            cwd: "/workspace".into(),
            max_turns: 48,
            initial_status: "preparing".into(),
            execution_target_json: json!({
                "kind":"kubernetes_workspace",
                "repo_mode":{
                    "stage":"implement",
                    "stage_execution_id":stage_execution_id,
                },
                "run_scope":{
                    "work_item_id":fixture.work_item_id,
                    "workspace_id":workspace_id,
                },
            }),
        })
        .await
        .unwrap();
    fixture
        .state
        .store
        .create_stage_execution(CreateStageExecution {
            id: stage_execution_id.into(),
            work_item_id: fixture.work_item_id.clone(),
            stage_key: "implement".into(),
            sequence: 2,
            status: "preparing".into(),
            agent_profile_id: Some("repo-builder".into()),
            agent_profile_version: Some("v1".into()),
            agent_profile_hash: Some("sha256:builder".into()),
            context_pack_id: None,
            run_id: Some(run_id.clone()),
            workspace_id: Some(workspace_id.into()),
            input_snapshot: json!({"source_commit":SOURCE_SHA}),
            input_hash: "sha256:startup-recovery-input".into(),
        })
        .await
        .unwrap();
    let started = fixture
        .state
        .store
        .start_work_item_attempt(
            &fixture.work_item_id,
            &run_id,
            Some("operator".into()),
            Some("start exact correction".into()),
        )
        .await
        .unwrap();
    assert_eq!(started.attempt_count, 1);

    let flow = crate::app::repo_mode::repo_work_item_flow(&fixture.state, &fixture.work_item_id)
        .await
        .unwrap();
    let action = flow
        .action_rail
        .iter()
        .find(|action| action.id == "recover_stage_startup")
        .unwrap();
    let result = crate::app::repo_mode::execute_repo_work_item_action(
        &fixture.state,
        &fixture.work_item_id,
        &action.id,
        "operator".into(),
        "recover startup that failed before preparation".into(),
        action.state_hash.clone(),
    )
    .await
    .unwrap();
    assert_eq!(result["attempt_budget_restored"], true);
    let work_item = fixture
        .state
        .store
        .get_work_item(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(work_item.attempt_count, 0);
    assert_eq!(work_item.status, "blocked");
    let run = fixture.state.store.get_run(&run_id).await.unwrap().unwrap();
    assert_eq!(run.status, "failed");
    assert_eq!(
        run.stop_reason.as_deref(),
        Some("controller_stage_startup_failed_before_model")
    );
    let execution = fixture
        .state
        .store
        .get_stage_execution(stage_execution_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(execution.status, "failed");
    assert!(fixture
        .state
        .store
        .get_stage_outcome_for_execution(stage_execution_id)
        .await
        .unwrap()
        .is_some());
}

#[test]
fn provider_observation_fixture_is_bounded_json() {
    let request = provider_observation("observer", false);
    let encoded: Value = serde_json::to_value(request.required_checks).unwrap();
    assert_eq!(encoded, json!([]));
}
