use super::projection::{
    bounded_context_discovery_projection, derive_repo_actions,
    rejected_change_set_precedes_work_plan, repo_action_run_id,
    validate_change_set_outcome_binding, ChangeSetOutcomeBinding, RepoActionInputs,
};
use super::source_delivery::derive_provider_check_status;
use super::stages::{
    agent_profile_from_chain, correction_environment_snapshot_for_reuse,
    reusable_correction_environment_snapshot,
};
use pharness_core::{RunBudgetConsumption, RunId, SessionId};
use pharness_store::{
    StoredBudgetExtension, StoredChangeSet, StoredOperatorAnnotation, StoredRepoWorkItemMetadata,
    StoredRun, StoredSourceDeliveryIntent, StoredStageOutcome,
};
use serde_json::{json, Value};

fn correction_environment_profile(
    image: &str,
    revision: &str,
) -> pharness_core::EnvironmentProfile {
    serde_json::from_value(json!({
        "id":"python-3.11",
        "active":true,
        "image":image,
        "revision":revision,
        "platform":"linux/amd64",
        "required_executables":["pharness-worker","git","python","pip"],
        "preparation_strategy":"python_hashed_requirements",
        "service_account":"pharness-python-runner",
        "repository_allowlist":["https://github.com/example/repo.git"],
        "limits":{"cpu":"1","memory":"1Gi","ephemeral_storage":"2Gi"},
    }))
    .unwrap()
}

fn correction_environment_snapshot(image: &str, revision: &str) -> Value {
    json!({
        "source_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "manifest_sha256":format!("sha256:{}", "b".repeat(64)),
        "dependency_lock_sha256":format!("sha256:{}", "c".repeat(64)),
        "runner_image_digest":image,
        "runner_revision":revision,
        "os":"linux",
        "architecture":"x86_64",
        "effective_user":"65532",
        "python_version":"Python 3.11.16",
        "python_path":"/workspace/.pharness-runtime/venv/bin/python",
        "writable_paths":["src/**"],
        "unavailable_tools":["docker"],
        "agent_network":"denied",
        "package_installation":"preparation_only",
        "acceptance_commands":[{"name":"unit","command":"python -m unittest"}],
        "preparation_evidence":{},
    })
}

fn metadata() -> StoredRepoWorkItemMetadata {
    StoredRepoWorkItemMetadata {
        work_item_id: "witem_repo".into(),
        mode: "repo".into(),
        product_id: "prod_repo".into(),
        repository_id: "repo_repo".into(),
        product_model_snapshot_id: "pmodel_repo".into(),
        product_model_snapshot_hash: "sha256:model".into(),
        repository_contract_version_id: "rcontract_repo".into(),
        contract_version: "pharness.dev/v1alpha1".into(),
        acceptance_command_names: vec!["unit".into()],
        context_repositories: json!([]),
        current_stage_execution_id: Some("stageexec_verify".into()),
        state_version: 8,
        closed_at: None,
        closure_reason: None,
        workflow_policy: None,
        workflow_policy_hash: None,
    }
}

fn proposed_change_set() -> StoredChangeSet {
    StoredChangeSet {
        id: "cset_repo".into(),
        work_item_id: Some("witem_repo".into()),
        work_plan_id: "wplan_repo".into(),
        remediation_plan_id: None,
        incident_id: None,
        session_id: SessionId::new("ses_repo"),
        run_id: Some(RunId::new("run_repo")),
        status: "proposed".into(),
        title: "Source change".into(),
        summary: "Verified change".into(),
        risk_level: "medium".into(),
        material_hash: format!("sha256:{}", "a".repeat(64)),
        revision: 1,
        resource_namespace: None,
        resource_kind: Some("Repository".into()),
        resource_name: Some("https://github.com/example/repo.git".into()),
        change_set_json: json!({}),
        created_at: "1".into(),
        updated_at: Some("1".into()),
        status_changed_at: Some("1".into()),
        status_changed_by: None,
        status_reason: None,
    }
}

fn proposed_work_plan(revision: i64) -> pharness_store::StoredWorkPlan {
    pharness_store::StoredWorkPlan {
        id: "wplan_repo".into(),
        work_item_id: Some("witem_repo".into()),
        remediation_plan_id: None,
        incident_id: None,
        session_id: SessionId::new("ses_repo"),
        run_id: Some(RunId::new("run_plan")),
        status: "proposed".into(),
        title: "Plan".into(),
        summary: "Correct the rejected source change".into(),
        risk_level: "medium".into(),
        requires_approval: true,
        resource_namespace: None,
        resource_kind: Some("Repository".into()),
        resource_name: Some("https://github.com/example/repo.git".into()),
        work_plan_json: json!({}),
        created_at: "1".into(),
        updated_at: Some("3".into()),
        revision,
        status_changed_at: Some("3".into()),
        status_changed_by: Some("controller".into()),
        status_reason: Some("new Planner submission".into()),
        created_by: Some("operator".into()),
        origin: "operator".into(),
    }
}

fn stage_execution(
    id: &str,
    stage_key: &str,
    status: &str,
    created_at: &str,
) -> pharness_store::StoredStageExecution {
    pharness_store::StoredStageExecution {
        id: id.into(),
        work_item_id: "witem_repo".into(),
        stage_key: stage_key.into(),
        sequence: 1,
        status: status.into(),
        origin: "controller".into(),
        agent_profile_id: None,
        agent_profile_version: None,
        agent_profile_hash: None,
        context_pack_id: None,
        run_id: None,
        workspace_id: Some("workspace_repo".into()),
        input_snapshot: json!({}),
        input_hash: format!("sha256:{}", "b".repeat(64)),
        stop_reason: None,
        created_at: created_at.into(),
        started_at: None,
        finished_at: None,
    }
}

fn stage_outcome(id: &str, stage_key: &str) -> StoredStageOutcome {
    StoredStageOutcome {
        id: id.into(),
        stage_execution_id: format!("stageexec_{id}"),
        work_item_id: "witem_repo".into(),
        stage_key: stage_key.into(),
        status: "succeeded".into(),
        origin: "agent".into(),
        schema_version: pharness_core::STAGE_OUTCOME_SCHEMA.into(),
        outcome: json!({}),
        content_hash: format!("sha256:{id}"),
        state_version: 1,
        supersedes_outcome_id: None,
        sealed_by: "controller".into(),
        sealed_at: "1".into(),
    }
}

fn outcome_reference(outcome: &StoredStageOutcome) -> Value {
    json!({
        "id":outcome.id,
        "stage":outcome.stage_key,
        "status":outcome.status,
        "hash":outcome.content_hash,
    })
}

fn source_delivery_intent(status: &str) -> StoredSourceDeliveryIntent {
    StoredSourceDeliveryIntent {
        id: "srcintent_repo".into(),
        subject_kind: "work_item_change_set".into(),
        subject_id: "cset_repo".into(),
        repository_id: "repo_repo".into(),
        source_repo: "https://github.com/example/repo.git".into(),
        base_ref: "main".into(),
        base_commit: "a".repeat(40),
        head_branch: "pharness/witem_repo".into(),
        patch_artifact_id: Some("artifact_repo".into()),
        patch_hash: format!("sha256:{}", "c".repeat(64)),
        status: status.into(),
        state_version: 4,
        authorization: json!({}),
        writer_execution_id: Some("writer_repo".into()),
        observer_execution_id: None,
        pull_request: Some(json!({"number":7,"head_sha":"d".repeat(40)})),
        merge_provenance: None,
        provider_checks: None,
        created_by: "operator".into(),
        creation_reason: "test".into(),
        created_at: "1".into(),
        updated_at: "1".into(),
        status_changed_at: "1".into(),
        status_changed_by: None,
        status_reason: None,
    }
}

#[test]
fn stage_chain_profile_lookup_finds_every_compiled_profile() {
    let profiles = pharness_core::compiled_agent_profiles("test-model", "test-prompt")
        .into_iter()
        .filter(|profile| {
            matches!(
                profile.id.as_str(),
                "repo-builder" | "repo-tester" | "repo-verifier"
            )
        })
        .collect::<Vec<_>>();
    let profile_chain = serde_json::to_value(profiles).unwrap();

    for profile_id in ["repo-builder", "repo-tester", "repo-verifier"] {
        assert_eq!(
            agent_profile_from_chain(&profile_chain, profile_id).map(|profile| profile.id),
            Some(profile_id.to_string())
        );
    }
    assert!(agent_profile_from_chain(&profile_chain, "repo-unknown").is_none());
}

#[test]
fn context_repository_projection_is_bounded_and_contains_no_raw_source() {
    let discovery = pharness_store::StoredRepositoryDiscovery {
        id: "rdisc_context".into(),
        onboarding_id: "ronb_context".into(),
        source_commit: "a".repeat(40),
        resolved_commit: Some("a".repeat(40)),
        status: "succeeded".into(),
        schema_version: "pharness.dev/repository-discovery/v1alpha1".into(),
        inventory_json: Some(json!({
            "command_candidates":(0..150).map(|index| json!({"name":format!("command-{index}")})).collect::<Vec<_>>(),
            "raw_source":"must-not-be-exposed",
            "limits":{"entries":20_000},
        })),
        content_hash: Some("sha256:discovery".into()),
        error_code: None,
        error_summary: None,
        started_at: Some("1".into()),
        finished_at: Some("2".into()),
        created_at: "1".into(),
        updated_at: "2".into(),
    };
    let projection = bounded_context_discovery_projection(
        &json!({
            "repository_id":"repo_context",
            "canonical_url":"https://github.com/example/context.git",
            "source_commit":"a".repeat(40),
        }),
        &discovery,
    );
    assert_eq!(
        projection
            .pointer("/bounded_inventory/command_candidates")
            .and_then(Value::as_array)
            .unwrap()
            .len(),
        100
    );
    assert!(projection
        .pointer("/bounded_inventory/raw_source")
        .is_none());
    assert_eq!(
        projection.pointer("/limits/raw_repository_content_included"),
        Some(&json!(false))
    );
}

#[test]
fn proposed_change_set_replaces_stage_chain_reauthorization_actions() {
    let change_set = proposed_change_set();
    let actions = derive_repo_actions(
        &metadata(),
        RepoActionInputs {
            attempts: (0, 2),
            work_plan: None,
            change_set: Some(&change_set),
            source_delivery_intent: None,
            executions: &[],
            chain: None,
            pending_annotation_effects: &[],
            pending_budget_extension: None,
            current_run: None,
            retryable_budget_extension: None,
        },
    )
    .unwrap();
    assert_eq!(
        actions
            .iter()
            .map(|action| action.id.as_str())
            .collect::<Vec<_>>(),
        vec!["approve_change_set", "reject_change_set"]
    );
}

#[test]
fn approved_change_set_repairs_stored_run_provenance_before_source_authorization() {
    let mut change_set = proposed_change_set();
    change_set.status = "approved".into();
    change_set.run_id = Some(RunId::new("run_builder_stale"));
    change_set.change_set_json = json!({
        "source_provenance":{"run_id":"run_builder_current"},
        "verification_run_id":"run_verifier_current",
    });

    let actions = derive_repo_actions(
        &metadata(),
        RepoActionInputs {
            attempts: (1, 3),
            work_plan: None,
            change_set: Some(&change_set),
            source_delivery_intent: None,
            executions: &[],
            chain: None,
            pending_annotation_effects: &[],
            pending_budget_extension: None,
            current_run: None,
            retryable_budget_extension: None,
        },
    )
    .unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].id, "repair_change_set_provenance");
    assert_eq!(actions[0].effect_class, "controller_internal");
    assert!(actions[0].approval_required);
    assert!(!actions[0]
        .external_effect_summary
        .contains("create a branch"));

    change_set.run_id = Some(RunId::new("run_builder_current"));
    let actions = derive_repo_actions(
        &metadata(),
        RepoActionInputs {
            attempts: (1, 3),
            work_plan: None,
            change_set: Some(&change_set),
            source_delivery_intent: None,
            executions: &[],
            chain: None,
            pending_annotation_effects: &[],
            pending_budget_extension: None,
            current_run: None,
            retryable_budget_extension: None,
        },
    )
    .unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].id, "authorize_source_delivery");
}

#[test]
fn legacy_outcome_binding_allows_only_one_immutable_historical_verifier() {
    let implement = stage_outcome("implement_current", "implement");
    let test = stage_outcome("test_current", "test");
    let verify = stage_outcome("verify_current", "verify");
    let effective = vec![implement.clone(), test.clone(), verify.clone()];
    let current_material = effective.iter().map(outcome_reference).collect::<Vec<_>>();
    assert_eq!(
        validate_change_set_outcome_binding(&current_material, &effective).unwrap(),
        ChangeSetOutcomeBinding::Current
    );

    let historical_verify = stage_outcome("verify_historical", "verify");
    let historical_material = vec![
        outcome_reference(&implement),
        outcome_reference(&test),
        outcome_reference(&historical_verify),
    ];
    assert_eq!(
        validate_change_set_outcome_binding(&historical_material, &effective).unwrap(),
        ChangeSetOutcomeBinding::HistoricalVerifier {
            id: historical_verify.id.clone(),
            hash: historical_verify.content_hash.clone(),
        }
    );

    assert!(validate_change_set_outcome_binding(
        &[
            outcome_reference(&implement),
            outcome_reference(&historical_verify),
        ],
        &effective,
    )
    .is_err());
    assert!(validate_change_set_outcome_binding(
        &[
            outcome_reference(&implement),
            outcome_reference(&test),
            outcome_reference(&historical_verify),
            outcome_reference(&stage_outcome("verify_other", "verify")),
        ],
        &effective,
    )
    .is_err());
    let mut extra_stale_non_verify = historical_material;
    extra_stale_non_verify.push(outcome_reference(&stage_outcome(
        "implement_historical",
        "implement",
    )));
    assert!(validate_change_set_outcome_binding(&extra_stale_non_verify, &effective).is_err());
}

#[test]
fn newer_proposed_work_plan_preempts_a_rejected_change_set_revision() {
    let plan = proposed_work_plan(2);
    let mut change_set = proposed_change_set();
    change_set.status = "rejected".into();
    change_set.change_set_json = json!({"work_plan":{"id":"wplan_repo","revision":1}});
    let executions = vec![stage_execution(
        "stageexec_plan_2",
        "plan",
        "succeeded",
        "3",
    )];

    assert!(rejected_change_set_precedes_work_plan(
        &change_set,
        &plan,
        None
    ));
    let actions = derive_repo_actions(
        &metadata(),
        RepoActionInputs {
            attempts: (1, 3),
            work_plan: Some(&plan),
            change_set: Some(&change_set),
            source_delivery_intent: None,
            executions: &executions,
            chain: None,
            pending_annotation_effects: &[],
            pending_budget_extension: None,
            current_run: None,
            retryable_budget_extension: None,
        },
    )
    .unwrap();

    assert_eq!(
        actions
            .iter()
            .map(|action| action.id.as_str())
            .collect::<Vec<_>>(),
        vec!["approve_work_plan", "reject_work_plan"]
    );
}

#[test]
fn provider_check_status_is_controller_derived() {
    assert_eq!(derive_provider_check_status(&json!([])).unwrap(), "passing");
    assert_eq!(
        derive_provider_check_status(&json!([
            {"status":"passing"},
            {"status":"pending"}
        ]))
        .unwrap(),
        "pending"
    );
    assert_eq!(
        derive_provider_check_status(&json!([
            {"status":"passing"},
            {"status":"failed"}
        ]))
        .unwrap(),
        "failed"
    );
}

#[test]
fn terminal_stage_failure_offers_bounded_same_workspace_correction_and_replan() {
    let executions = vec![
        stage_execution("stageexec_plan", "plan", "succeeded", "1"),
        stage_execution("stageexec_implement", "implement", "failed", "2"),
    ];
    let actions = derive_repo_actions(
        &metadata(),
        RepoActionInputs {
            attempts: (1, 2),
            work_plan: None,
            change_set: None,
            source_delivery_intent: None,
            executions: &executions,
            chain: None,
            pending_annotation_effects: &[],
            pending_budget_extension: None,
            current_run: None,
            retryable_budget_extension: None,
        },
    )
    .unwrap();
    assert_eq!(
        actions
            .iter()
            .map(|action| (action.id.as_str(), action.status.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("correct_stage_chain", "ready"),
            ("replan_work_item", "ready")
        ]
    );

    let exhausted = derive_repo_actions(
        &metadata(),
        RepoActionInputs {
            attempts: (2, 2),
            work_plan: None,
            change_set: None,
            source_delivery_intent: None,
            executions: &executions,
            chain: None,
            pending_annotation_effects: &[],
            pending_budget_extension: None,
            current_run: None,
            retryable_budget_extension: None,
        },
    )
    .unwrap();
    assert!(exhausted.iter().all(|action| action.status == "blocked"));
}

#[test]
fn zero_turn_builder_startup_failure_preempts_attempt_exhaustion_for_recovery() {
    let run_id = RunId::new("run_startup_recovery");
    let mut execution =
        stage_execution("stageexec_startup_recovery", "implement", "preparing", "3");
    execution.run_id = Some(run_id.clone());
    let mut metadata = metadata();
    metadata.current_stage_execution_id = Some(execution.id.clone());
    let run = StoredRun {
        id: run_id,
        session_id: SessionId::new("ses_startup_recovery"),
        cwd: "/workspace".into(),
        status: "preparing".into(),
        user_task: "start the bounded Builder".into(),
        max_turns: 48,
        started_at: "3".into(),
        finished_at: None,
        cancel_requested_at: None,
        error: None,
        result_json: None,
        execution_target_json: json!({
            "kind":"kubernetes_workspace",
            "repo_mode":{
                "stage":"implement",
                "stage_execution_id":execution.id,
            },
        }),
        origin: "controller".into(),
        created_by: Some("operator".into()),
        run_budget: pharness_core::RunBudget::default(),
        budget_consumption: RunBudgetConsumption {
            allowed_turns: 48,
            allowed_tokens: 400_000,
            ..RunBudgetConsumption::default()
        },
        stop_reason: None,
        retention_state: "retained".into(),
        sealed_summary: None,
    };
    let actions = derive_repo_actions(
        &metadata,
        RepoActionInputs {
            attempts: (2, 2),
            work_plan: None,
            change_set: None,
            source_delivery_intent: None,
            executions: &[execution.clone()],
            chain: None,
            pending_annotation_effects: &[],
            pending_budget_extension: None,
            current_run: Some(&run),
            retryable_budget_extension: None,
        },
    )
    .unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].id, "recover_stage_startup");
    assert_eq!(actions[0].status, "ready");
    assert_eq!(actions[0].effect_class, "controller_internal");

    let mut consumed = run;
    consumed.budget_consumption.turns_used = 1;
    let actions = derive_repo_actions(
        &metadata,
        RepoActionInputs {
            attempts: (2, 2),
            work_plan: None,
            change_set: None,
            source_delivery_intent: None,
            executions: &[execution],
            chain: None,
            pending_annotation_effects: &[],
            pending_budget_extension: None,
            current_run: Some(&consumed),
            retryable_budget_extension: None,
        },
    )
    .unwrap();
    assert!(actions
        .iter()
        .all(|action| action.id != "recover_stage_startup"));
}

#[test]
fn zero_turn_followup_startup_failure_retries_without_an_attempt_budget() {
    let run_id = RunId::new("run_tester_startup_recovery");
    let mut execution = stage_execution("stageexec_tester_startup_recovery", "test", "failed", "4");
    execution.run_id = Some(run_id.clone());
    execution.input_snapshot = json!({"chain_authorization_id":"chain_failed"});
    let mut metadata = metadata();
    metadata.current_stage_execution_id = Some(execution.id.clone());
    let run = StoredRun {
        id: run_id,
        session_id: SessionId::new("ses_tester_startup_recovery"),
        cwd: "/workspace".into(),
        status: "failed".into(),
        user_task: "run the bounded Tester".into(),
        max_turns: 8,
        started_at: "4".into(),
        finished_at: Some("5".into()),
        cancel_requested_at: None,
        error: Some("worker job failed before reporting a durable outcome".into()),
        result_json: Some(json!({
            "status":"failed",
            "turns":0,
            "error":"worker job failed before reporting a durable outcome",
        })),
        execution_target_json: json!({
            "kind":"kubernetes_workspace",
            "repo_mode":{
                "stage":"test",
                "stage_execution_id":execution.id,
                "chain_authorization_id":"chain_failed",
            },
        }),
        origin: "controller".into(),
        created_by: Some("controller:repo-mode".into()),
        run_budget: pharness_core::RunBudget::default(),
        budget_consumption: RunBudgetConsumption {
            allowed_turns: 8,
            allowed_tokens: 80_000,
            ..RunBudgetConsumption::default()
        },
        stop_reason: Some("worker job failed before reporting a durable outcome".into()),
        retention_state: "retained".into(),
        sealed_summary: None,
    };
    let actions = derive_repo_actions(
        &metadata,
        RepoActionInputs {
            attempts: (3, 3),
            work_plan: None,
            change_set: None,
            source_delivery_intent: None,
            executions: &[execution.clone()],
            chain: None,
            pending_annotation_effects: &[],
            pending_budget_extension: None,
            current_run: Some(&run),
            retryable_budget_extension: None,
        },
    )
    .unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].id, "retry_stage_startup");
    assert_eq!(actions[0].status, "ready");
    assert_eq!(actions[0].effect_class, "model_execution");
    assert!(actions[0]
        .external_effect_summary
        .contains("does not consume another WorkItem attempt"));

    let mut consumed = run;
    consumed.budget_consumption.turns_used = 1;
    let actions = derive_repo_actions(
        &metadata,
        RepoActionInputs {
            attempts: (3, 3),
            work_plan: None,
            change_set: None,
            source_delivery_intent: None,
            executions: &[execution],
            chain: None,
            pending_annotation_effects: &[],
            pending_budget_extension: None,
            current_run: Some(&consumed),
            retryable_budget_extension: None,
        },
    )
    .unwrap();
    assert!(actions
        .iter()
        .all(|action| action.id != "retry_stage_startup"));
}

#[test]
fn correction_reuses_an_exact_environment_snapshot() {
    let revision = "d".repeat(40);
    let image = format!("registry.example/runner@sha256:{}", "e".repeat(64));
    let profile = correction_environment_profile(&image, &revision);
    let snapshot = correction_environment_snapshot(&image, &revision);

    assert_eq!(
        reusable_correction_environment_snapshot(
            snapshot.clone(),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            &format!("sha256:{}", "b".repeat(64)),
            &profile,
        )
        .unwrap(),
        Some(snapshot)
    );
}

#[test]
fn correction_refreshes_runner_provenance_on_the_preserved_workspace() {
    let old_revision = "d".repeat(40);
    let current_revision = "e".repeat(40);
    let old_image = format!("registry.example/runner@sha256:{}", "f".repeat(64));
    let current_image = format!("registry.example/runner@sha256:{}", "1".repeat(64));
    let profile = correction_environment_profile(&current_image, &current_revision);
    let snapshot = correction_environment_snapshot(&old_image, &old_revision);

    assert!(reusable_correction_environment_snapshot(
        snapshot,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        &format!("sha256:{}", "b".repeat(64)),
        &profile,
    )
    .unwrap()
    .is_none());
}

#[test]
fn correction_reprepares_when_prior_attempt_has_no_sealed_snapshot() {
    let revision = "d".repeat(40);
    let image = format!("registry.example/runner@sha256:{}", "e".repeat(64));
    let profile = correction_environment_profile(&image, &revision);

    assert!(correction_environment_snapshot_for_reuse(
        None,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        &format!("sha256:{}", "b".repeat(64)),
        &profile,
    )
    .unwrap()
    .is_none());
}

#[test]
fn correction_never_refreshes_around_source_or_contract_drift() {
    let revision = "d".repeat(40);
    let image = format!("registry.example/runner@sha256:{}", "e".repeat(64));
    let profile = correction_environment_profile(&image, &revision);
    let snapshot = correction_environment_snapshot(&image, &revision);

    assert!(reusable_correction_environment_snapshot(
        snapshot.clone(),
        "ffffffffffffffffffffffffffffffffffffffff",
        &format!("sha256:{}", "b".repeat(64)),
        &profile,
    )
    .is_err());
    assert!(reusable_correction_environment_snapshot(
        snapshot,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        &format!("sha256:{}", "9".repeat(64)),
        &profile,
    )
    .is_err());
}

#[test]
fn pending_budget_extension_preempts_stale_failed_attempt_actions() {
    let executions = vec![
        stage_execution("stageexec_plan", "plan", "succeeded", "1"),
        stage_execution("stageexec_prior", "implement", "failed", "2"),
        stage_execution("stageexec_current", "implement", "paused", "3"),
    ];
    let extension = StoredBudgetExtension {
        id: "budgetext_repo".into(),
        work_item_id: "witem_repo".into(),
        run_id: RunId::new("run_current"),
        status: "pending".into(),
        turn_increment: 20,
        token_increment: 200_000,
        state_hash: "sha256:budget-extension-state".into(),
        requested_at: "4".into(),
        approved_at: None,
        approved_by: None,
        approval_reason: None,
    };

    let actions = derive_repo_actions(
        &metadata(),
        RepoActionInputs {
            attempts: (2, 2),
            work_plan: None,
            change_set: None,
            source_delivery_intent: None,
            executions: &executions,
            chain: None,
            pending_annotation_effects: &[],
            pending_budget_extension: Some(&extension),
            current_run: None,
            retryable_budget_extension: None,
        },
    )
    .unwrap();

    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].id, "approve_budget_extension");
    assert_eq!(actions[0].resource, extension.id);
    assert_eq!(actions[0].status, "ready");
    assert_eq!(actions[0].state_hash, extension.state_hash);
    assert!(actions[0]
        .external_effect_summary
        .contains("200000 additional tokens"));
}

#[test]
fn pending_budget_extension_describes_only_remaining_hard_limit_headroom() {
    let executions = vec![stage_execution(
        "stageexec_current",
        "implement",
        "paused",
        "3",
    )];
    let run_id = RunId::new("run_current");
    let extension = StoredBudgetExtension {
        id: "budgetext_capped".into(),
        work_item_id: "witem_repo".into(),
        run_id: run_id.clone(),
        status: "pending".into(),
        turn_increment: 20,
        token_increment: 200_000,
        state_hash: "sha256:capped-extension-state".into(),
        requested_at: "4".into(),
        approved_at: None,
        approved_by: None,
        approval_reason: None,
    };
    let run = StoredRun {
        id: run_id,
        session_id: SessionId::new("session_current"),
        cwd: "/workspace".into(),
        status: "budget_extension_required".into(),
        user_task: "finish within the hard budget".into(),
        max_turns: 95,
        started_at: "1".into(),
        finished_at: None,
        cancel_requested_at: None,
        error: None,
        result_json: None,
        execution_target_json: json!({"kind":"kubernetes_workspace"}),
        origin: "controller".into(),
        created_by: Some("controller:repo-mode".into()),
        run_budget: pharness_core::RunBudget::default(),
        budget_consumption: RunBudgetConsumption {
            allowed_turns: 95,
            allowed_tokens: 900_000,
            ..RunBudgetConsumption::default()
        },
        stop_reason: Some("budget extension required".into()),
        retention_state: "retained".into(),
        sealed_summary: None,
    };

    let actions = derive_repo_actions(
        &metadata(),
        RepoActionInputs {
            attempts: (2, 2),
            work_plan: None,
            change_set: None,
            source_delivery_intent: None,
            executions: &executions,
            chain: None,
            pending_annotation_effects: &[],
            pending_budget_extension: Some(&extension),
            current_run: Some(&run),
            retryable_budget_extension: None,
        },
    )
    .unwrap();

    assert_eq!(actions.len(), 1);
    assert!(actions[0]
        .external_effect_summary
        .contains("exactly 5 additional turns and 100000 additional tokens"));
}

#[test]
fn repo_actions_follow_the_current_stage_run_after_automatic_dispatch() {
    let fallback_run_id = RunId::new("run_builder");
    let mut builder = stage_execution("stageexec_builder", "implement", "succeeded", "1");
    builder.run_id = Some(fallback_run_id.clone());
    let mut verifier = stage_execution("stageexec_verify", "verify", "queued", "2");
    verifier.run_id = Some(RunId::new("run_verifier"));
    let executions = vec![builder, verifier];

    let selected = repo_action_run_id(&metadata(), &executions, Some(&fallback_run_id));

    assert_eq!(selected.map(RunId::as_str), Some("run_verifier"));
}

#[test]
fn failed_approved_budget_extension_dispatch_offers_exact_same_run_retry() {
    let executions = vec![
        stage_execution("stageexec_prior", "implement", "failed", "1"),
        stage_execution("stageexec_current", "implement", "queued", "2"),
    ];
    let run = StoredRun {
        id: RunId::new("run_current"),
        session_id: SessionId::new("session_current"),
        cwd: "/workspace".into(),
        status: "failed".into(),
        user_task: "finish the approved builder stage".into(),
        max_turns: 68,
        started_at: "1".into(),
        finished_at: Some("3".into()),
        cancel_requested_at: None,
        error: Some(
            "failed to launch worker job: jobs.batch pharness-run-current-i already exists".into(),
        ),
        result_json: Some(json!({
            "status":"budget_extension_required",
            "budget_extension":{
                "resume_messages":[],
                "turns_completed":22,
            },
        })),
        execution_target_json: json!({"kind":"kubernetes_job"}),
        origin: "controller".into(),
        created_by: Some("operator".into()),
        run_budget: pharness_core::RunBudget::default(),
        budget_consumption: RunBudgetConsumption {
            allowed_turns: 68,
            allowed_tokens: 600_000,
            turns_used: 22,
            tokens_used: 420_894,
            active_execution_seconds_used: 159,
            extensions: 1,
        },
        stop_reason: None,
        retention_state: "retained".into(),
        sealed_summary: None,
    };
    let extension = StoredBudgetExtension {
        id: "budgetext_repo_approved".into(),
        work_item_id: "witem_repo".into(),
        run_id: run.id.clone(),
        status: "approved".into(),
        turn_increment: 20,
        token_increment: 200_000,
        state_hash: "sha256:approved-extension-state".into(),
        requested_at: "2".into(),
        approved_at: Some("3".into()),
        approved_by: Some("operator".into()),
        approval_reason: Some("finish evidence".into()),
    };

    let actions = derive_repo_actions(
        &metadata(),
        RepoActionInputs {
            attempts: (2, 2),
            work_plan: None,
            change_set: None,
            source_delivery_intent: None,
            executions: &executions,
            chain: None,
            pending_annotation_effects: &[],
            pending_budget_extension: None,
            current_run: Some(&run),
            retryable_budget_extension: Some(&extension),
        },
    )
    .unwrap();

    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].id, "retry_budget_extension_dispatch");
    assert_eq!(actions[0].resource, extension.id);
    assert_eq!(actions[0].status, "ready");
    assert!(actions[0]
        .external_effect_summary
        .contains("grants no additional budget"));
}

#[test]
fn annotation_effect_is_state_hashed_and_cannot_cross_source_delivery() {
    let annotation = StoredOperatorAnnotation {
        id: "annot_replan".into(),
        work_item_id: "witem_repo".into(),
        target_kind: "work_item".into(),
        target_id: "witem_repo".into(),
        statement: "The evidence requires a new plan".into(),
        evidence_refs: json!([]),
        requested_effect: "replan".into(),
        actor: "operator".into(),
        reason: "reviewed contradiction".into(),
        state_hash: "sha256:annotation-preview".into(),
        created_at: "1".into(),
    };
    let actions = derive_repo_actions(
        &metadata(),
        RepoActionInputs {
            attempts: (1, 2),
            work_plan: None,
            change_set: None,
            source_delivery_intent: None,
            executions: &[],
            chain: None,
            pending_annotation_effects: &[&annotation],
            pending_budget_extension: None,
            current_run: None,
            retryable_budget_extension: None,
        },
    )
    .unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].id, "apply_annotation_effect");
    assert_eq!(actions[0].status, "ready");

    let change_set = proposed_change_set();
    let blocked = derive_repo_actions(
        &metadata(),
        RepoActionInputs {
            attempts: (1, 2),
            work_plan: None,
            change_set: Some(&change_set),
            source_delivery_intent: None,
            executions: &[],
            chain: None,
            pending_annotation_effects: &[&annotation],
            pending_budget_extension: None,
            current_run: None,
            retryable_budget_extension: None,
        },
    )
    .unwrap();
    assert_eq!(blocked[0].id, "apply_annotation_effect");
    assert_eq!(blocked[0].status, "blocked");
}

#[test]
fn source_head_drift_remains_observable_until_closed_then_offers_replan() {
    let mut change_set = proposed_change_set();
    change_set.status = "approved".into();
    let drifting = source_delivery_intent("head_drift");
    let actions = derive_repo_actions(
        &metadata(),
        RepoActionInputs {
            attempts: (1, 2),
            work_plan: None,
            change_set: Some(&change_set),
            source_delivery_intent: Some(&drifting),
            executions: &[],
            chain: None,
            pending_annotation_effects: &[],
            pending_budget_extension: None,
            current_run: None,
            retryable_budget_extension: None,
        },
    )
    .unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].id, "observe_source_delivery");

    let closed = source_delivery_intent("pull_request_closed");
    let actions = derive_repo_actions(
        &metadata(),
        RepoActionInputs {
            attempts: (1, 2),
            work_plan: None,
            change_set: Some(&change_set),
            source_delivery_intent: Some(&closed),
            executions: &[],
            chain: None,
            pending_annotation_effects: &[],
            pending_budget_extension: None,
            current_run: None,
            retryable_budget_extension: None,
        },
    )
    .unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].id, "replan_work_item");
    assert_eq!(actions[0].status, "ready");
}

#[test]
fn failed_source_writer_permission_offers_only_an_exact_intent_retry() {
    let mut change_set = proposed_change_set();
    change_set.status = "approved".into();
    let mut failed = source_delivery_intent("failed");
    failed.pull_request = None;
    failed.status_reason = Some("git_push_permission_denied".into());

    let actions = derive_repo_actions(
        &metadata(),
        RepoActionInputs {
            attempts: (1, 2),
            work_plan: None,
            change_set: Some(&change_set),
            source_delivery_intent: Some(&failed),
            executions: &[],
            chain: None,
            pending_annotation_effects: &[],
            pending_budget_extension: None,
            current_run: None,
            retryable_budget_extension: None,
        },
    )
    .unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].id, "retry_source_delivery");
    assert_eq!(actions[0].effect_class, "external_source_mutation");
    assert!(actions[0]
        .external_effect_summary
        .contains("same immutable SourceDeliveryIntent"));

    failed.status_reason = Some("git_push_policy_rejected".into());
    let actions = derive_repo_actions(
        &metadata(),
        RepoActionInputs {
            attempts: (1, 2),
            work_plan: None,
            change_set: Some(&change_set),
            source_delivery_intent: Some(&failed),
            executions: &[],
            chain: None,
            pending_annotation_effects: &[],
            pending_budget_extension: None,
            current_run: None,
            retryable_budget_extension: None,
        },
    )
    .unwrap();
    assert!(actions.is_empty());
}
