use super::super::products::{
    internal_onboarding_contract_validation_context,
    internal_onboarding_contract_validation_outcome, internal_onboarding_patch_outcome,
    InternalOnboardingContractValidationQuery, OnboardingContractValidationOutcomeRequest,
    OnboardingPatchOutcomeRequest,
};
use super::characterization::{test_state, test_state_with_git_observer};
use super::{
    cancel_run, internal_source_delivery_observation_outcome, json,
    ApproveRepositoryOnboardingProposal, CreateChangeSet, CreateProductAggregate,
    CreateRepoWorkItem, CreateRepositoryContractVersion, CreateRepositoryOnboardingProposal,
    CreateRepositoryReadinessAssessment, CreateRun, CreateSession, CreateSourceDeliveryIntent,
    CreateStageExecution, CreateWorkPlan, CreateWorkspace, GitDeliveryObservationOutcomeRequest,
    Json, Path, Query, RegisterRepositoryAggregate, RunBudget, RunId, SessionId, State,
    StoredRepositoryDraft, Value,
};
use sha2::Digest;

const SOURCE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HEAD_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const MERGE_SHA: &str = "cccccccccccccccccccccccccccccccccccccccc";

struct RepoDeliveryFixture {
    state: super::AppState,
    work_item_id: String,
    intent_id: String,
}

#[tokio::test]
async fn existing_canonical_contract_records_no_change_provenance_without_a_source_pr() {
    let mut state = test_state_with_git_observer(
        "/bin/true".into(),
        "https://github.com/example/no-change.git".into(),
    )
    .await;
    state.environment_profiles = std::sync::Arc::new(vec![pharness_core::EnvironmentProfile {
        id: "python-3.11".into(),
        active: true,
        image: format!("example.test/python@sha256:{}", "a".repeat(64)),
        revision: "b".repeat(40),
        platform: "linux/amd64".into(),
        required_executables: vec![
            "pharness-worker".into(),
            "git".into(),
            "python".into(),
            "pip".into(),
        ],
        preparation_strategy: pharness_core::PreparationStrategy::PythonHashedRequirements,
        service_account: "pharness-python-runner".into(),
        repository_allowlist: vec!["https://github.com/example/no-change.git".into()],
        limits: pharness_core::EnvironmentProfileLimits {
            cpu: "1".into(),
            memory: "1Gi".into(),
            ephemeral_storage: "1Gi".into(),
        },
    }]);
    state
        .store
        .ensure_bootstrap_organization(&state.repo_mode.organization)
        .await
        .unwrap();
    state
        .store
        .create_product(CreateProductAggregate {
            id: "prod_no_change".into(),
            organization_id: state.repo_mode.organization.id.clone(),
            product_key: "no-change".into(),
            display_name: "No Change".into(),
            description: "Existing canonical contract fixture".into(),
            owner_principal: "operator".into(),
            snapshot_id: "pmodel_no_change_initial".into(),
            snapshot_json: json!({"schema_version":"pharness.dev/product-model/v1alpha1","repositories":[]}),
            snapshot_hash: "sha256:no-change-initial".into(),
            actor: "operator".into(),
            reason: "create fixture".into(),
        })
        .await
        .unwrap();
    let registered = state
        .store
        .register_repository(RegisterRepositoryAggregate {
            repository: StoredRepositoryDraft {
                id: "repo_no_change".into(),
                provider: "github".into(),
                external_id: "example/no-change".into(),
                canonical_url: "https://github.com/example/no-change.git".into(),
                default_branch: "main".into(),
                registered_commit: SOURCE_SHA.into(),
            },
            binding_id: "rbind_no_change".into(),
            binding_revision_id: "rbindrev_no_change".into(),
            onboarding_id: "ronb_no_change".into(),
            binding_content_hash: "sha256:binding-no-change".into(),
            evidence_json: json!({"source_commit":SOURCE_SHA}),
            product_id: "prod_no_change".into(),
            expected_product_state_version: 1,
            snapshot_id: "pmodel_no_change_registered".into(),
            snapshot_json: json!({"schema_version":"pharness.dev/product-model/v1alpha1","repositories":["repo_no_change"]}),
            snapshot_hash: "sha256:no-change-registered".into(),
            actor: "operator".into(),
            reason: "register exact revision".into(),
        })
        .await
        .unwrap();
    state
        .store
        .create_repository_discovery("rdisc_no_change", &registered.onboarding.id, SOURCE_SHA)
        .await
        .unwrap();
    state
        .store
        .finish_repository_discovery(
            "rdisc_no_change",
            SOURCE_SHA,
            &json!({"schema_version":"pharness.dev/repository-discovery/v1alpha1"}),
            "sha256:discovery-no-change",
        )
        .await
        .unwrap();
    let session_id = SessionId::new("ses_no_change");
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: "No-change onboarding".into(),
            cwd: "/workspace".into(),
        })
        .await
        .unwrap();
    state
        .store
        .create_run(CreateRun {
            id: RunId::new("run_no_change"),
            session_id,
            user_task: "Propose the existing contract".into(),
            cwd: "/workspace".into(),
            max_turns: 16,
            initial_status: "completed".into(),
            execution_target_json: json!({}),
        })
        .await
        .unwrap();
    let onboarding = state
        .store
        .get_repository_onboarding(&registered.onboarding.id)
        .await
        .unwrap()
        .unwrap();
    state
        .store
        .start_repository_onboarding_proposer(
            &onboarding.id,
            onboarding.state_version,
            "run_no_change",
            "sha256:profile-no-change",
            "operator",
            "review existing contract",
        )
        .await
        .unwrap();
    let onboarding = state
        .store
        .get_repository_onboarding(&registered.onboarding.id)
        .await
        .unwrap()
        .unwrap();
    let candidate_contract = json!({
        "api_version":"pharness.dev/v1alpha1",
        "environment_profile":"python-3.11",
        "dependency_lock":{"kind":"pip_requirements","path":"requirements.lock","sha256":"d".repeat(64)},
        "writable_paths":["src/**","tests/**","readme.md"],
        "acceptance_commands":[{"name":"unit-tests","command":"python -m unittest discover -s tests -v"}],
        "roots":{"source":["src"],"tests":["tests"],"documentation":["readme.md"]},
        "agent_network":"denied",
        "package_installation":"preparation_only"
    });
    let proposal = state
        .store
        .create_repository_onboarding_proposal(CreateRepositoryOnboardingProposal {
            id: "rprop_no_change".into(),
            onboarding_id: onboarding.id.clone(),
            expected_state_version: onboarding.state_version,
            proposal: json!({
                "schema_version":"pharness.dev/repository-onboarding-proposal/v1alpha1",
                "discovery_id":"rdisc_no_change",
                "discovery_hash":"sha256:discovery-no-change",
                "candidate_contract":candidate_contract.clone(),
                "instructions":"Existing reviewed instructions.\n",
                "service_proposals":[],
                "binding_proposals":[],
                "assumptions":[],
                "conflicts":[],
                "blockers":[],
                "readiness_forecast":{}
            }),
            content_hash: "sha256:proposal-no-change".into(),
            discovery_id: "rdisc_no_change".into(),
            discovery_hash: "sha256:discovery-no-change".into(),
            actor: "repository-onboarding-proposer".into(),
            origin: "agent".into(),
        })
        .await
        .unwrap();
    let onboarding = state
        .store
        .get_repository_onboarding(&registered.onboarding.id)
        .await
        .unwrap()
        .unwrap();
    let approved = state
        .store
        .approve_repository_onboarding_proposal(ApproveRepositoryOnboardingProposal {
            onboarding_id: onboarding.id.clone(),
            proposal_id: proposal.id,
            proposal_hash: proposal.content_hash,
            expected_state_version: onboarding.state_version,
            actor: "operator".into(),
            reason: "approve exact existing contract".into(),
            model_change: None,
        })
        .await
        .unwrap();
    state
        .store
        .start_repository_onboarding_patch(
            &approved.id,
            approved.state_version,
            "onbpatch_no_change",
            "operator",
            "prove the approved configuration is unchanged",
        )
        .await
        .unwrap();

    let empty_hash = format!("sha256:{:x}", sha2::Sha256::digest([]));
    let Json(outcome) = internal_onboarding_patch_outcome(
        State(state.clone()),
        Path(registered.onboarding.id.clone()),
        Json(OnboardingPatchOutcomeRequest {
            status: "unchanged".into(),
            patch: Some(String::new()),
            patch_hash: Some(empty_hash),
            changed_paths: Vec::new(),
            error_code: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(outcome["onboarding"]["status"], "merge_observed");
    assert!(outcome["onboarding"]["source_delivery_intent_id"].is_null());
    let artifact_id = outcome["artifact_id"].as_str().unwrap();
    let artifact = state
        .store
        .get_artifact(artifact_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(artifact.kind, "repository_onboarding_no_change");
    assert_eq!(artifact.content_json.unwrap()["changed_paths"], json!([]));

    let onboarding = state
        .store
        .get_repository_onboarding(&registered.onboarding.id)
        .await
        .unwrap()
        .unwrap();
    state
        .store
        .start_repository_onboarding_contract_validation(
            &onboarding.id,
            onboarding.state_version,
            "onbvalidate_no_change",
            "operator",
            "validate the existing canonical contract",
        )
        .await
        .unwrap();
    let Json(context) = internal_onboarding_contract_validation_context(
        State(state.clone()),
        Path(registered.onboarding.id.clone()),
        Query(InternalOnboardingContractValidationQuery {
            execution_id: "onbvalidate_no_change".into(),
        }),
    )
    .await
    .unwrap();
    let context = serde_json::to_value(context).unwrap();
    assert_eq!(context["source_commit"], SOURCE_SHA);
    assert_eq!(context["proposal_id"], "rprop_no_change");

    let Json(outcome) = internal_onboarding_contract_validation_outcome(
        State(state.clone()),
        Path(registered.onboarding.id),
        Json(OnboardingContractValidationOutcomeRequest {
            status: "succeeded".into(),
            contract: Some(candidate_contract),
            contract_content_hash: Some(format!("sha256:{}", "e".repeat(64))),
            contract_source: Some("canonical".into()),
            warnings: Vec::new(),
            error_code: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(outcome["onboarding"]["status"], "contract_ready");
    let contract_version_id = outcome["onboarding"]["contract_version_id"]
        .as_str()
        .unwrap();
    let contract_version = state
        .store
        .get_repository_contract_version(contract_version_id)
        .await
        .unwrap()
        .unwrap();
    assert!(contract_version.merge_provenance["source_delivery_intent_id"].is_null());
    assert_eq!(
        contract_version.merge_provenance["no_change_artifact_id"],
        artifact_id
    );
    assert_eq!(
        contract_version.merge_provenance["no_change"]["changed_paths"],
        json!([])
    );
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
            evidence_refs: json!([{"kind":"repository_discovery","id":format!("rdisc_{suffix}"),"hash":format!("sha256:discovery-{suffix}")}]),
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
    // Repo Mode records Release and Observe as inapplicable before source
    // delivery can close. Final merge sealing must reuse that immutable tail
    // rather than attempting duplicate StageExecution sequence 1 records.
    super::super::repo_mode::seal_repo_inapplicable_tail(
        &fixture.state.store,
        &fixture.work_item_id,
    )
    .await
    .unwrap();
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
    let organization = fixture
        .state
        .store
        .get_organization(&fixture.state.repo_mode.organization.id)
        .await
        .unwrap()
        .unwrap();
    let overview = super::super::operator_experience::organization_overview_value(
        &fixture.state,
        &organization,
    )
    .await
    .unwrap();
    assert_eq!(overview["work_items"]["waiting"], 1);
    assert!(overview["attention"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["resource_id"] == fixture.work_item_id
                && item["resource_kind"] == "work_item"
                && item["kind"] == "external_wait"
                && item["action"]["external_effect_summary"].is_string()
        })
    }));
    assert!(overview["attention"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["resource_id"] == "onboard_success"
                && item["resource_kind"] == "repository_onboarding"
                && item["action"]["id"] == "prepare_onboarding_patch"
        })
    }));

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
    let Json(flow) = super::work_item_flow(
        State(fixture.state.clone()),
        Path(fixture.work_item_id.clone()),
    )
    .await
    .unwrap();
    assert_eq!(flow.work_item.mode.as_deref(), Some("repo"));
    assert_eq!(
        flow.work_item.product_model_snapshot_id.as_deref(),
        Some(metadata.product_model_snapshot_id.as_str())
    );
    assert_eq!(
        flow.work_item.repository_contract_version_id.as_deref(),
        Some(metadata.repository_contract_version_id.as_str())
    );
    assert_eq!(flow.work_item.state_version, Some(metadata.state_version));
    assert_eq!(flow.work_item.closed_at, metadata.closed_at);
    assert_eq!(flow.work_item.closure_reason, metadata.closure_reason);
    let Json(single) = super::get_work_item(
        State(fixture.state.clone()),
        Path(fixture.work_item_id.clone()),
    )
    .await
    .unwrap();
    assert_eq!(single.closed_at, metadata.closed_at);
    assert_eq!(single.state_version, Some(metadata.state_version));
    let Json(list) = super::list_work_items(
        State(fixture.state.clone()),
        axum::extract::Query(super::ListWorkItemsQuery::default()),
    )
    .await
    .unwrap();
    let listed = list
        .work_items
        .iter()
        .find(|item| item.id == fixture.work_item_id)
        .unwrap();
    assert_eq!(listed.closed_at, metadata.closed_at);
    assert_eq!(listed.state_version, Some(metadata.state_version));
    let Json(history) = super::list_work_items(
        State(fixture.state.clone()),
        axum::extract::Query(super::ListWorkItemsQuery {
            mode: Some("repo".into()),
            product_id: Some(metadata.product_id.clone()),
            repository_id: Some(metadata.repository_id.clone()),
            lifecycle: Some("history".into()),
            search: Some("reviewed source".into()),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    assert_eq!(history.count, 1);
    assert_eq!(history.work_items[0].id, fixture.work_item_id);
    let Json(current) = super::list_work_items(
        State(fixture.state.clone()),
        axum::extract::Query(super::ListWorkItemsQuery {
            mode: Some("repo".into()),
            product_id: Some(metadata.product_id.clone()),
            repository_id: Some(metadata.repository_id.clone()),
            lifecycle: Some("current".into()),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    assert_eq!(current.count, 0);
    let Json(product_overview) = super::super::operator_experience::product_overview(
        State(fixture.state.clone()),
        Path(metadata.product_id.clone()),
    )
    .await
    .unwrap();
    assert_eq!(
        product_overview["repositories"][0]["id"],
        metadata.repository_id
    );
    assert_eq!(
        product_overview["repositories"][0]["contract_readiness"],
        "ready"
    );
    assert_eq!(
        product_overview["repositories"][0]["coding_readiness"],
        "ready"
    );
    assert_eq!(
        product_overview["repository_bindings"][0]["binding"]["repository_id"],
        metadata.repository_id
    );
    assert!(
        product_overview["repository_bindings"][0]["current_revision"]["revision"]
            .as_u64()
            .is_some()
    );
    assert!(product_overview["connected_release_data"]["releases"]
        .as_array()
        .is_some());
    assert!(product_overview["evidence_summary"]["validation_count"]
        .as_u64()
        .is_some());
    assert_eq!(
        product_overview["evidence_summary"]["work_item_denominator"],
        1
    );
    assert!(product_overview["audit_events"].as_array().is_some());
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
    let flow = crate::app::repo_mode::repo_work_item_flow(&fixture.state, &fixture.work_item_id)
        .await
        .unwrap();
    let history = flow.repo_mode.as_ref().unwrap().get("history").unwrap();
    let timeline = &flow.repo_mode.as_ref().unwrap()["lifecycle_timeline"];
    assert!(timeline["as_of"].as_str().is_some());
    assert!(timeline["intervals"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| { entry["kind"] == "delivery_wait" && entry["finished_at"].is_string() }));
    // Repeated flow reads must not reconcile, create history, or change an
    // action hash. Only the observation clock may differ.
    let again = crate::app::repo_mode::repo_work_item_flow(&fixture.state, &fixture.work_item_id)
        .await
        .unwrap();
    let repo = flow.repo_mode.as_ref().unwrap();
    let next = again.repo_mode.as_ref().unwrap();
    for key in [
        "state_hash",
        "history",
        "stage_executions",
        "effective_stage_outcomes",
        "source_delivery_intent",
    ] {
        assert_eq!(repo[key], next[key], "read mutated {key}");
    }
    assert_eq!(
        timeline["intervals"],
        next["lifecycle_timeline"]["intervals"]
    );
    assert_eq!(
        serde_json::to_value(&flow.action_rail).unwrap(),
        serde_json::to_value(&again.action_rail).unwrap()
    );
    assert!(history["stage_outcomes"].as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| item["stage_key"] == "source_delivery")
    }));
    assert!(history["work_plans"].as_array().is_some());
    assert!(history["change_sets"].as_array().is_some());
    assert!(history["runs"].as_array().is_some());
    assert_eq!(
        flow.repo_mode.as_ref().unwrap()["ownership"]["product"]["id"],
        metadata.product_id
    );
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
async fn cancelling_paused_repo_run_seals_stage_and_cancels_budget_extension() {
    let fixture = repo_delivery_fixture("cancel_paused_repo_run").await;
    let run_id = RunId::new("run_cancel_paused_repo_run");
    let stage_execution_id = "stageexec_cancel_paused_repo_run";
    let budget = RunBudget::default();
    let consumption = pharness_core::RunBudgetConsumption {
        allowed_turns: budget.initial_turns,
        allowed_tokens: budget.initial_tokens,
        turns_used: 12,
        tokens_used: budget.initial_tokens,
        active_execution_seconds_used: 60,
        extensions: 0,
    };
    fixture
        .state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: SessionId::new("ses_cancel_paused_repo_run"),
            user_task: "cancel a no-progress Builder".into(),
            cwd: "/workspace".into(),
            max_turns: budget.initial_turns,
            initial_status: "queued".into(),
            execution_target_json: json!({
                "run_scope": {"work_item_id": fixture.work_item_id},
                "repo_mode": {
                    "stage": "implement",
                    "stage_execution_id": stage_execution_id,
                },
            }),
        })
        .await
        .unwrap();
    fixture
        .state
        .store
        .set_run_budget(&run_id, &budget, &consumption)
        .await
        .unwrap();
    fixture
        .state
        .store
        .pause_run_for_budget(
            &run_id,
            json!({"status":"budget_extension_required"}),
            "soft_token_budget_exhausted",
        )
        .await
        .unwrap();
    let extension = fixture
        .state
        .store
        .create_budget_extension(pharness_store::CreateBudgetExtension {
            id: "budget_cancel_paused_repo_run".into(),
            work_item_id: fixture.work_item_id.clone(),
            run_id: run_id.clone(),
            state_hash: "cancel-state".into(),
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
            sequence: 1,
            status: "paused".into(),
            agent_profile_id: Some("repo-builder".into()),
            agent_profile_version: Some("v1".into()),
            agent_profile_hash: Some("sha256:builder".into()),
            context_pack_id: None,
            run_id: Some(run_id.clone()),
            workspace_id: None,
            input_snapshot: json!({"source_commit": SOURCE_SHA}),
            input_hash: "sha256:cancel-paused-input".into(),
        })
        .await
        .unwrap();

    let Json(cancelled) = cancel_run(State(fixture.state.clone()), Path(run_id.to_string()))
        .await
        .unwrap();

    assert_eq!(cancelled.status, "cancelled");
    assert!(fixture
        .state
        .store
        .pending_budget_extension_for_run(&run_id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        fixture
            .state
            .store
            .get_budget_extension(&extension.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "cancelled"
    );
    assert_eq!(
        fixture
            .state
            .store
            .get_stage_execution(stage_execution_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "cancelled"
    );
    assert_eq!(
        fixture
            .state
            .store
            .get_stage_outcome_for_execution(stage_execution_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "cancelled"
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
        crate::app::repo_mode::RepoWorkItemActionExecutionRequest {
            actor: "operator".into(),
            reason: "recover startup that failed before preparation".into(),
            state_hash: action.state_hash.clone(),
            inference_policies: None,
            execution_policies: None,
        },
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
