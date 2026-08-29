use super::{
    approval_gates_from_work_item, authorize_change_set_git_delivery, create_deployment_contract,
    create_work_item_pipeline_intent, execute_work_item_action, fs,
    gitops_base_revision_reconcile_state, internal_gitops_delivery_outcome, json,
    pipeline_intent_execution_preflight, preflight_change_set_git_delivery,
    preflight_deployment_intent, reconcile_work_item, unique_suffix, work_item_flow, AppState,
    ApprovalGateListFilter, ArtifactResponse, CreateArtifact, CreateChangeSet,
    CreateDeploymentContractRequest, CreateDeploymentIntent, CreateGitDeliveryAuthorizationRequest,
    CreateGitOpsChangeSet, CreatePipelineContract, CreatePipelineIntent, CreateRun, CreateSession,
    CreateWorkItem, CreateWorkItemPipelineIntentRequest, CreateWorkPlan,
    DeploymentIntentDeliveryFlowResponse, DeploymentIntentExecutionPreflight,
    DeploymentIntentPreflightRequest, ExecuteWorkItemActionRequest, GitDeliveryFlowResponse,
    GitDeliveryPreflightRequest, GitOpsBaseRevisionReconcileState, GitOpsDeliveryFlowResponse,
    GitOpsDeliveryOutcomeRequest, Json, Path, PermissionsExt, ReconcileWorkItemRequest,
    ReleaseResponse, RunId, SessionId, State, StatusCode, StoredGitOpsChangeSet,
    StoredPipelineIntent, StoredRelease, Value,
};

use super::characterization::test_state_with_git_observer;
use super::support::reconcile_deployment_intent;

async fn seed_git_delivery_fixture(
    state: &AppState,
    completed_delivery_result: bool,
) -> (String, String, RunId) {
    let work_item_id = "witem_git_observation".to_string();
    let work_plan_id = "wplan_git_observation".to_string();
    let change_set_id = "cset_git_observation".to_string();
    let session_id = SessionId::new("ses_git_observation");
    let run_id = RunId::new("run_git_observation");
    let repository = "https://github.com/example/finance-app.git";
    let commit = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";

    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: "Git observation fixture".to_string(),
            cwd: "/workspace".to_string(),
        })
        .await
        .unwrap();
    state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: "Observe a completed source pull request".to_string(),
            cwd: "/workspace".to_string(),
            max_turns: 1,
            initial_status: "completed".to_string(),
            execution_target_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_work_item(CreateWorkItem {
            id: work_item_id.clone(),
            status: "awaiting_approval".to_string(),
            title: "Observe finance pull request".to_string(),
            intent: "Record immutable pull-request state before CI.".to_string(),
            acceptance_criteria: Vec::new(),
            source_repo: repository.to_string(),
            source_ref: "main".to_string(),
            source_commit: None,
            pipeline_contract_id: None,
            deployment_contract_id: None,
            gitops_repo: None,
            gitops_ref: None,
            gitops_kustomization_path: None,
            gitops_image_name: None,
            target_environment: "dev".to_string(),
            target_namespace: Some("apps-dev".to_string()),
            argo_application: None,
            workload_kind: None,
            workload_name: None,
            rollback_owner: None,
            production_impacting: false,
            max_attempts: 1,
            max_elapsed_seconds: 600,
            environment_profile_id: None,
            run_budget: Default::default(),
            repository_contract_json: None,
            repository_contract_hash: None,
            environment_preparation_status: "not_required".to_string(),
            created_by: Some("lucas".to_string()),
        })
        .await
        .unwrap();
    state
        .store
        .create_work_plan(CreateWorkPlan {
            id: work_plan_id.clone(),
            work_item_id: Some(work_item_id.clone()),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Observe source delivery".to_string(),
            summary: "Use the read-only Git observer.".to_string(),
            risk_level: "low".to_string(),
            requires_approval: false,
            resource_namespace: None,
            resource_kind: None,
            resource_name: None,
            work_plan_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_change_set(CreateChangeSet {
            id: change_set_id.clone(),
            work_item_id: Some(work_item_id.clone()),
            work_plan_id,
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Finance source ChangeSet".to_string(),
            summary: "A reviewed source diff.".to_string(),
            risk_level: "medium".to_string(),
            material_hash: "material_git_observation".to_string(),
            resource_namespace: None,
            resource_kind: None,
            resource_name: None,
            change_set_json: json!({ "source": { "kind": "workspace_git" } }),
        })
        .await
        .unwrap();
    let plan_id = "art_git_observation_plan";
    state
        .store
        .create_artifact(CreateArtifact {
            id: plan_id.to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "git_delivery_plan".to_string(),
            label: "Immutable source Git delivery plan".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "change_set": {
                    "id": change_set_id,
                    "revision": 1,
                    "material_hash": "material_git_observation"
                },
                "operation": "branch_and_pull_request",
                "source": {
                    "repository": repository,
                    "base_ref": "main",
                    "base_commit": commit,
                    "head_branch": "pharness/witem-git-observation/attempt-1",
                    "workspace_id": "ws_git_observation"
                }
            })),
        })
        .await
        .unwrap();
    if completed_delivery_result {
        state
            .store
            .create_artifact(CreateArtifact {
                id: "art_git_observation_result".to_string(),
                session_id,
                run_id: Some(run_id.clone()),
                kind: "git_delivery_result".to_string(),
                label: "Completed source Git delivery".to_string(),
                mime_type: Some("application/json".to_string()),
                path: None,
                content_text: None,
                content_json: Some(json!({
                    "execution_id": "gexec_completed",
                    "status": "completed",
                    "change_set_id": change_set_id,
                    "git_delivery_plan_artifact_id": plan_id,
                    "details": {
                        "branch": "pharness/witem-git-observation/attempt-1",
                        "commit_sha": commit,
                        "pull_request_url": "https://github.com/example/finance-app/pull/7",
                        "pull_request_number": 7
                    }
                })),
            })
            .await
            .unwrap();
    }

    (work_item_id, change_set_id, run_id)
}

#[tokio::test]
async fn reconcile_explicitly_dispatches_one_read_only_git_observer_then_waits() {
    let kubectl_stub =
        std::env::temp_dir().join(format!("pharness-git-observer-kubectl-{}", unique_suffix()));
    fs::write(&kubectl_stub, "#!/bin/sh\ncat >/dev/null\nexit 0\n").unwrap();
    fs::set_permissions(&kubectl_stub, fs::Permissions::from_mode(0o755)).unwrap();
    let state = test_state_with_git_observer(
        kubectl_stub.to_string_lossy().to_string(),
        "https://github.com/example/finance-app.git".to_string(),
    )
    .await;
    let (work_item_id, _change_set_id, run_id) = seed_git_delivery_fixture(&state, true).await;

    let Json(preview) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.clone()),
        Json(ReconcileWorkItemRequest {
            apply: false,
            actor: Some("lucas".to_string()),
            reason: None,
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(preview.action, "awaiting_pull_request_observation");
    assert!(!preview.applied);
    assert!(preview.controller_wait.is_none());

    let Json(applied) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.clone()),
        Json(ReconcileWorkItemRequest {
            apply: true,
            actor: Some("lucas".to_string()),
            reason: Some("dispatch bounded source PR observation".to_string()),
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    let wait_id = applied
        .controller_wait
        .as_ref()
        .map(|wait| wait.id.clone())
        .expect("successful dispatch schedules one bounded wait");
    assert!(applied.applied);
    assert!(applied
        .message
        .contains("dispatched the configured read-only Git observer"));

    let Json(repeated) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.clone()),
        Json(ReconcileWorkItemRequest {
            apply: true,
            actor: Some("lucas".to_string()),
            reason: Some("confirm idempotent source PR observation".to_string()),
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        repeated
            .controller_wait
            .as_ref()
            .map(|wait| wait.id.as_str()),
        Some(wait_id.as_str())
    );
    assert!(repeated
        .message
        .contains("reused the read-only Git observer dispatch"));
    let artifacts = state.store.list_artifacts(&run_id).await.unwrap();
    assert_eq!(
        artifacts
            .iter()
            .filter(|artifact| artifact.kind == "git_delivery_observation_execution")
            .count(),
        1
    );
    let audit = state
        .store
        .list_audit_events(Some("work_item"), Some(&work_item_id), None, 20)
        .await
        .unwrap();
    assert!(audit
        .iter()
        .any(|event| event.kind == "work_item.git_delivery_observation_dispatched"));
    fs::remove_file(kubectl_stub).unwrap();
}

#[tokio::test]
async fn failed_git_observer_dispatch_is_durable_and_retriable() {
    let kubectl_stub = std::env::temp_dir().join(format!(
        "pharness-git-observer-failure-kubectl-{}",
        unique_suffix()
    ));
    fs::write(&kubectl_stub, "#!/bin/sh\ncat >/dev/null\nexit 1\n").unwrap();
    fs::set_permissions(&kubectl_stub, fs::Permissions::from_mode(0o755)).unwrap();
    let state = test_state_with_git_observer(
        kubectl_stub.to_string_lossy().to_string(),
        "https://github.com/example/finance-app.git".to_string(),
    )
    .await;
    let (work_item_id, _change_set_id, run_id) = seed_git_delivery_fixture(&state, true).await;

    let Json(failed) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.clone()),
        Json(ReconcileWorkItemRequest {
            apply: true,
            actor: Some("lucas".to_string()),
            reason: Some("attempt bounded source PR observation".to_string()),
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert!(failed.applied);
    assert!(failed.controller_wait.is_none());
    assert!(failed.message.contains("dispatch failure"));
    let artifacts = state.store.list_artifacts(&run_id).await.unwrap();
    assert_eq!(
        artifacts
            .iter()
            .filter(|artifact| artifact.kind == "git_delivery_observation_dispatch_failure")
            .count(),
        1
    );

    fs::write(&kubectl_stub, "#!/bin/sh\ncat >/dev/null\nexit 0\n").unwrap();
    let Json(retried) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.clone()),
        Json(ReconcileWorkItemRequest {
            apply: true,
            actor: Some("lucas".to_string()),
            reason: Some("retry bounded source PR observation after executor repair".to_string()),
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert!(retried.controller_wait.is_some());
    let artifacts = state.store.list_artifacts(&run_id).await.unwrap();
    assert_eq!(
        artifacts
            .iter()
            .filter(|artifact| artifact.kind == "git_delivery_observation_execution")
            .count(),
        2
    );
    let audit = state
        .store
        .list_audit_events(Some("work_item"), Some(&work_item_id), None, 20)
        .await
        .unwrap();
    assert_eq!(
        audit
            .iter()
            .filter(|event| event.kind == "work_item.git_delivery_observation_dispatched")
            .count(),
        2
    );
    fs::remove_file(kubectl_stub).unwrap();
}

#[tokio::test]
async fn reconcile_dispatches_one_approved_git_writer_then_waits() {
    let kubectl_stub =
        std::env::temp_dir().join(format!("pharness-git-writer-kubectl-{}", unique_suffix()));
    fs::write(&kubectl_stub, "#!/bin/sh\ncat >/dev/null\nexit 0\n").unwrap();
    fs::set_permissions(&kubectl_stub, fs::Permissions::from_mode(0o755)).unwrap();
    let state = test_state_with_git_observer(
        kubectl_stub.to_string_lossy().to_string(),
        "https://github.com/example/finance-app.git".to_string(),
    )
    .await;
    let (work_item_id, change_set_id, run_id) = seed_git_delivery_fixture(&state, false).await;
    let work_item = state
        .store
        .get_work_item(&work_item_id)
        .await
        .unwrap()
        .unwrap();
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&work_item_id)
        .await
        .unwrap()
        .unwrap();
    for gate in approval_gates_from_work_item(&work_item, &work_plan) {
        state.store.create_approval_gate(gate).await.unwrap();
    }
    let git_gate = state
        .store
        .list_approval_gates(ApprovalGateListFilter {
            work_item_id: Some(work_item_id.clone()),
            gate_kind: Some("git_mutation".to_string()),
            limit: 1,
            ..ApprovalGateListFilter::default()
        })
        .await
        .unwrap()
        .pop()
        .unwrap();
    state
        .store
        .decide_approval_gate(
            &git_gate.id,
            "satisfied",
            Some("lucas".to_string()),
            Some("approve exact source delivery".to_string()),
        )
        .await
        .unwrap();
    let Json(authorization) = authorize_change_set_git_delivery(
        State(state.clone()),
        None,
        Path(change_set_id.clone()),
        Json(CreateGitDeliveryAuthorizationRequest {
            subject: None,
            created_by: Some("lucas".to_string()),
            reason: "authorize one immutable source delivery".to_string(),
            expires_at: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(authorization.grant.subject, "agent:git-writer");
    let Json(preflight) = preflight_change_set_git_delivery(
        State(state.clone()),
        None,
        Path(change_set_id.clone()),
        Json(GitDeliveryPreflightRequest {
            subject: None,
            actor: Some("lucas".to_string()),
            reason: Some("record execution-ready source delivery".to_string()),
        }),
    )
    .await
    .unwrap();
    assert!(preflight.dispatch_ready);

    let Json(preview) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.clone()),
        Json(ReconcileWorkItemRequest {
            apply: false,
            actor: Some("lucas".to_string()),
            reason: None,
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(preview.action, "awaiting_git_delivery_execution");
    assert!(!preview.applied);

    let Json(applied) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.clone()),
        Json(ReconcileWorkItemRequest {
            apply: true,
            actor: Some("lucas".to_string()),
            reason: Some("dispatch approved source writer".to_string()),
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert!(applied.applied);
    assert_eq!(
        applied
            .controller_wait
            .as_ref()
            .map(|wait| wait.wait_kind.as_str()),
        Some("git_delivery_execution")
    );
    assert!(applied
        .message
        .contains("dispatched the approved isolated Git writer"));

    let Json(repeated) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.clone()),
        Json(ReconcileWorkItemRequest {
            apply: true,
            actor: Some("lucas".to_string()),
            reason: Some("retain source writer wait".to_string()),
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(repeated.action, "wait_for_git_delivery");
    let artifacts = state.store.list_artifacts(&run_id).await.unwrap();
    assert_eq!(
        artifacts
            .iter()
            .filter(|artifact| artifact.kind == "git_delivery_execution")
            .count(),
        1
    );
    let audit = state
        .store
        .list_audit_events(Some("work_item"), Some(&work_item_id), None, 20)
        .await
        .unwrap();
    assert!(audit
        .iter()
        .any(|event| event.kind == "work_item.git_delivery_dispatched"));
    fs::remove_file(kubectl_stub).unwrap();
}

#[tokio::test]
async fn reconcile_dispatches_one_preflighted_tekton_executor_then_waits() {
    let kubectl_stub = std::env::temp_dir().join(format!(
        "pharness-tekton-executor-kubectl-{}",
        unique_suffix()
    ));
    fs::write(&kubectl_stub, "#!/bin/sh\ncat >/dev/null\nexit 0\n").unwrap();
    fs::set_permissions(&kubectl_stub, fs::Permissions::from_mode(0o755)).unwrap();
    let state = test_state_with_git_observer(
        kubectl_stub.to_string_lossy().to_string(),
        "https://github.com/example/finance-app.git".to_string(),
    )
    .await;
    let (work_item_id, change_set_id, _run_id) = seed_git_delivery_fixture(&state, false).await;
    let work_item = state
        .store
        .get_work_item(&work_item_id)
        .await
        .unwrap()
        .unwrap();
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&work_item_id)
        .await
        .unwrap()
        .unwrap();
    let change_set = state
        .store
        .get_change_set(&change_set_id)
        .await
        .unwrap()
        .unwrap();
    let merge_sha = "b1b2c3d4e5f60718293a4b5c6d7e8f9012345678";

    for gate in approval_gates_from_work_item(&work_item, &work_plan) {
        state.store.create_approval_gate(gate).await.unwrap();
    }
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_tekton_controller_git_merge".to_string(),
            session_id: change_set.session_id.clone(),
            run_id: change_set.run_id.clone(),
            kind: "git_delivery_merge".to_string(),
            label: "Immutable merged finance source".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "change_set_id": change_set_id,
                "git_delivery_plan_artifact_id": "art_git_observation_plan",
                "pull_request_url": "https://github.com/example/finance-app/pull/7",
                "pull_request_number": 7,
                "head_commit_sha": "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678",
                "merge_commit_sha": merge_sha,
            })),
        })
        .await
        .unwrap();
    state
        .store
        .create_pipeline_contract(CreatePipelineContract {
            id: "pcontract_tekton_controller".to_string(),
            status: "active".to_string(),
            namespace: "ci".to_string(),
            pipeline_ref: "finance-ci".to_string(),
            version: "v1".to_string(),
            contract_json: json!({
                "source_revision_param": "source-revision",
                "params": [{ "name": "source-revision", "type": "scalar", "required": true }]
            }),
            actor: Some("lucas".to_string()),
            reason: Some("disposable finance CI contract".to_string()),
        })
        .await
        .unwrap();
    let Json(pipeline) = create_work_item_pipeline_intent(
        State(state.clone()),
        None,
        Path(work_item_id.clone()),
        Json(CreateWorkItemPipelineIntentRequest {
            pipeline_contract_id: "pcontract_tekton_controller".to_string(),
            title: Some("Finance CI from reviewed source".to_string()),
            summary: None,
            risk_level: None,
            intent_kind: None,
            intent_json: Some(json!({
                "execution": {
                    "enabled": true,
                    "namespace": "ci",
                    "pipeline_ref": "finance-ci",
                    "params": { "source-revision": merge_sha }
                },
                "pipeline": { "provider": "tekton", "name": "finance-ci" }
            })),
            actor: Some("lucas".to_string()),
            reason: Some("define immutable disposable finance build".to_string()),
        }),
    )
    .await
    .unwrap();
    state
        .store
        .update_pipeline_intent_status(
            &pipeline.pipeline_intent.id,
            "approved",
            Some("lucas".to_string()),
            Some("approve exact disposable Tekton definition".to_string()),
        )
        .await
        .unwrap();
    for gate_kind in ["pipeline_mutation", "cluster_mutation"] {
        let gate = state
            .store
            .list_approval_gates(ApprovalGateListFilter {
                work_item_id: Some(work_item_id.clone()),
                gate_kind: Some(gate_kind.to_string()),
                limit: 1,
                ..ApprovalGateListFilter::default()
            })
            .await
            .unwrap()
            .pop()
            .unwrap();
        state
            .store
            .decide_approval_gate(
                &gate.id,
                "satisfied",
                Some("lucas".to_string()),
                Some(format!("approve exact {gate_kind} boundary")),
            )
            .await
            .unwrap();
    }
    let Json(authorization_flow) = work_item_flow(State(state.clone()), Path(work_item_id.clone()))
        .await
        .unwrap();
    let authorization_action = authorization_flow
        .action_rail
        .iter()
        .find(|action| action.id == "authorize_pipeline_execution")
        .expect("approved PipelineIntent must expose exact execution authorization");
    assert!(authorization_flow
        .reconcile_preview
        .authorization_checks
        .iter()
        .any(|check| check.kind == "approval_gate" && check.status == "ready"));
    assert!(authorization_flow
        .reconcile_preview
        .authorization_checks
        .iter()
        .any(|check| check.kind == "permission_grant" && check.status == "missing"));
    assert_eq!(authorization_action.status, "ready");
    assert!(authorization_action.approval_required);
    assert!(authorization_action
        .external_effect_summary
        .contains("does not start Tekton"));

    let stale = execute_work_item_action(
        State(state.clone()),
        None,
        Path((work_item_id.clone(), authorization_action.id.clone())),
        Json(ExecuteWorkItemActionRequest {
            actor: Some("lucas".to_string()),
            reason: "reject stale execution authorization preview".to_string(),
            state_hash: "stale-pipeline-authorization-state".to_string(),
            inference_policies: None,
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(stale.status, StatusCode::CONFLICT);

    let Json(envelope_value) = execute_work_item_action(
        State(state.clone()),
        None,
        Path((work_item_id.clone(), authorization_action.id.clone())),
        Json(ExecuteWorkItemActionRequest {
            actor: Some("lucas".to_string()),
            reason: "authorize one exact disposable Tekton execution".to_string(),
            state_hash: authorization_action.state_hash.clone(),
            inference_policies: None,
        }),
    )
    .await
    .unwrap();
    let envelope_grant_id = envelope_value
        .pointer("/grant/id")
        .and_then(Value::as_str)
        .expect("authorization action must return the scoped PermissionGrant")
        .to_string();
    let preflight = pipeline_intent_execution_preflight(&state, &pipeline.pipeline_intent.id)
        .await
        .unwrap();
    assert!(preflight.ready, "preflight checks: {:?}", preflight.checks);
    assert_eq!(
        preflight.grant_id.as_deref(),
        Some(envelope_grant_id.as_str())
    );

    let Json(preview) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.clone()),
        Json(ReconcileWorkItemRequest {
            apply: false,
            actor: Some("lucas".to_string()),
            reason: None,
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(preview.action, "awaiting_pipeline_execution");
    assert!(!preview.applied);

    let Json(applied) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.clone()),
        Json(ReconcileWorkItemRequest {
            apply: true,
            actor: Some("lucas".to_string()),
            reason: Some("dispatch approved disposable Tekton executor".to_string()),
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert!(applied.applied);
    assert_eq!(
        applied
            .controller_wait
            .as_ref()
            .map(|wait| wait.wait_kind.as_str()),
        Some("pipeline_execution")
    );
    assert!(applied
        .message
        .contains("dispatched the approved isolated Tekton executor"));

    let Json(repeated) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.clone()),
        Json(ReconcileWorkItemRequest {
            apply: true,
            actor: Some("lucas".to_string()),
            reason: Some("retain bounded Tekton wait".to_string()),
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(repeated.action, "wait_for_pipeline_execution");
    let pipeline = state
        .store
        .get_pipeline_intent(&pipeline.pipeline_intent.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pipeline.status, "executing");
    assert_eq!(
        pipeline
            .intent_json
            .pointer("/execution_state/state")
            .and_then(Value::as_str),
        Some("executor_job_created")
    );
    let audit = state
        .store
        .list_audit_events(Some("work_item"), Some(&work_item_id), None, 20)
        .await
        .unwrap();
    assert_eq!(
        audit
            .iter()
            .filter(|event| event.kind == "work_item.pipeline_execution_dispatched")
            .count(),
        1
    );
    fs::remove_file(kubectl_stub).unwrap();
}

#[tokio::test]
async fn reconcile_dispatches_one_preflighted_gitops_writer_then_waits() {
    let kubectl_stub = std::env::temp_dir().join(format!(
        "pharness-gitops-writer-kubectl-{}",
        unique_suffix()
    ));
    fs::write(&kubectl_stub, "#!/bin/sh\ncat >/dev/null\nexit 0\n").unwrap();
    fs::set_permissions(&kubectl_stub, fs::Permissions::from_mode(0o755)).unwrap();
    let gitops_repo = "https://github.com/example/finance-gitops.git";
    let state = test_state_with_git_observer(
        kubectl_stub.to_string_lossy().to_string(),
        gitops_repo.to_string(),
    )
    .await;
    let session_id = SessionId::new("ses_gitops_controller");
    let run_id = RunId::new("run_gitops_controller");
    let work_item_id = "witem_gitops_controller";
    let work_plan_id = "wplan_gitops_controller";
    let change_set_id = "cset_gitops_controller";
    let pipeline_intent_id = "pint_gitops_controller";
    let deployment_intent_id = "dint_gitops_controller";
    let gitops_change_set_id = "gset_gitops_controller";
    let source_commit = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
    let gitops_base_commit = "b1b2c3d4e5f60718293a4b5c6d7e8f9012345678";

    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: "GitOps controller fixture".to_string(),
            cwd: "/workspace".to_string(),
        })
        .await
        .unwrap();
    state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: "Deliver a verified finance image through GitOps".to_string(),
            cwd: "/workspace".to_string(),
            max_turns: 1,
            initial_status: "completed".to_string(),
            execution_target_json: json!({}),
        })
        .await
        .unwrap();
    let work_item = state
        .store
        .create_work_item(CreateWorkItem {
            id: work_item_id.to_string(),
            status: "awaiting_approval".to_string(),
            title: "Update disposable finance GitOps image".to_string(),
            intent: "Promote one verified disposable finance image in dev.".to_string(),
            acceptance_criteria: vec!["GitOps update is reviewed".to_string()],
            source_repo: "https://github.com/example/finance-api.git".to_string(),
            source_ref: "main".to_string(),
            source_commit: None,
            pipeline_contract_id: None,
            deployment_contract_id: None,
            gitops_repo: Some(gitops_repo.to_string()),
            gitops_ref: Some("main".to_string()),
            gitops_kustomization_path: None,
            gitops_image_name: None,
            target_environment: "dev".to_string(),
            target_namespace: Some("apps-dev".to_string()),
            argo_application: Some("finance-api".to_string()),
            workload_kind: None,
            workload_name: None,
            rollback_owner: None,
            production_impacting: false,
            max_attempts: 1,
            max_elapsed_seconds: 600,
            environment_profile_id: None,
            run_budget: Default::default(),
            repository_contract_json: None,
            repository_contract_hash: None,
            environment_preparation_status: "not_required".to_string(),
            created_by: Some("lucas".to_string()),
        })
        .await
        .unwrap();
    let work_plan = state
        .store
        .create_work_plan(CreateWorkPlan {
            id: work_plan_id.to_string(),
            work_item_id: Some(work_item_id.to_string()),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Disposable finance delivery plan".to_string(),
            summary: "Build and update one dev GitOps image reference.".to_string(),
            risk_level: "high".to_string(),
            requires_approval: true,
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Application".to_string()),
            resource_name: Some("finance-api".to_string()),
            work_plan_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_change_set(CreateChangeSet {
            id: change_set_id.to_string(),
            work_item_id: Some(work_item_id.to_string()),
            work_plan_id: work_plan_id.to_string(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Disposable finance source ChangeSet".to_string(),
            summary: "Reviewed source change.".to_string(),
            risk_level: "medium".to_string(),
            material_hash: "material_gitops_controller".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: None,
            resource_name: None,
            change_set_json: json!({}),
        })
        .await
        .unwrap();
    state
            .store
            .create_artifact(CreateArtifact {
                id: "art_gitops_controller_source_plan".to_string(),
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                kind: "git_delivery_plan".to_string(),
                label: "Immutable source delivery plan".to_string(),
                mime_type: Some("application/json".to_string()),
                path: None,
                content_text: None,
                content_json: Some(json!({
                    "change_set": { "id": change_set_id, "revision": 1, "material_hash": "material_gitops_controller" },
                    "source": {
                        "repository": "https://github.com/example/finance-api.git",
                        "base_ref": "main",
                        "base_commit": source_commit,
                        "head_branch": "pharness/witem-gitops-controller/attempt-1"
                    }
                })),
            })
            .await
            .unwrap();
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_gitops_controller_source_merge".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "git_delivery_merge".to_string(),
            label: "Immutable source merge".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "change_set_id": change_set_id,
                "git_delivery_plan_artifact_id": "art_gitops_controller_source_plan",
                "head_commit_sha": source_commit,
                "merge_commit_sha": source_commit,
            })),
        })
        .await
        .unwrap();
    state
        .store
        .create_pipeline_intent(CreatePipelineIntent {
            id: pipeline_intent_id.to_string(),
            change_set_id: change_set_id.to_string(),
            work_plan_id: work_plan_id.to_string(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Verified disposable finance build".to_string(),
            summary: "Build output is ready for dev GitOps.".to_string(),
            risk_level: "medium".to_string(),
            intent_kind: "tekton_build_test_package".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("Pipeline".to_string()),
            resource_name: Some("finance-ci".to_string()),
            intent_json: json!({
                "execution_state": { "state": "pipeline_run_succeeded" },
                "evidence": { "status": "satisfied" },
                "build_output": { "status": "verified" }
            }),
        })
        .await
        .unwrap();
    state
        .store
        .create_deployment_intent(CreateDeploymentIntent {
            id: deployment_intent_id.to_string(),
            pipeline_intent_id: pipeline_intent_id.to_string(),
            change_set_id: change_set_id.to_string(),
            work_plan_id: work_plan_id.to_string(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "proposed".to_string(),
            title: "Disposable finance dev deployment".to_string(),
            summary: "Declare the exact dev GitOps target.".to_string(),
            risk_level: "high".to_string(),
            intent_kind: "argo_sync_deploy".to_string(),
            target_environment: Some("dev".to_string()),
            target_namespace: Some("apps-dev".to_string()),
            argo_application: Some("finance-api".to_string()),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Application".to_string()),
            resource_name: Some("finance-api".to_string()),
            intent_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_gitops_controller_update".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "gitops_update_plan".to_string(),
            label: "Verified disposable GitOps update".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({ "source": "verified_pipeline_build_output" })),
        })
        .await
        .unwrap();
    let gitops_change_set = state
        .store
        .create_gitops_change_set(CreateGitOpsChangeSet {
            id: gitops_change_set_id.to_string(),
            work_item_id: work_item_id.to_string(),
            work_plan_id: work_plan_id.to_string(),
            source_change_set_id: change_set_id.to_string(),
            pipeline_intent_id: pipeline_intent_id.to_string(),
            deployment_intent_id: deployment_intent_id.to_string(),
            gitops_update_plan_artifact_id: "art_gitops_controller_update".to_string(),
            session_id: session_id.clone(),
            run_id: run_id.clone(),
            status: "proposed".to_string(),
            title: "Disposable finance GitOps ChangeSet".to_string(),
            summary: "Digest-pin the approved dev image.".to_string(),
            risk_level: "high".to_string(),
            material_hash: "material_gitops_controller".to_string(),
            gitops_repo: gitops_repo.to_string(),
            gitops_ref: "main".to_string(),
            head_branch: "pharness/witem-gitops-controller/gitops".to_string(),
            kustomization_path: "apps/finance-api/kustomization.yaml".to_string(),
            image_name: "registry.example.test/finance-api".to_string(),
            image_ref: "registry.example.test/finance-api@sha256:1234567890abcdef".to_string(),
            gitops_change_set_json: json!({}),
        })
        .await
        .unwrap();
    let Json(review_flow) = work_item_flow(State(state.clone()), Path(work_item_id.to_string()))
        .await
        .unwrap();
    let approve_gitops_change_set = review_flow
        .action_rail
        .iter()
        .find(|action| action.id == "approve_gitops_change_set")
        .expect("proposed GitOps ChangeSet must be reviewable from the action rail");
    assert_eq!(approve_gitops_change_set.resource, gitops_change_set.id);
    assert_eq!(approve_gitops_change_set.status, "ready");
    let Json(reviewed) = execute_work_item_action(
        State(state.clone()),
        None,
        Path((
            work_item_id.to_string(),
            approve_gitops_change_set.id.clone(),
        )),
        Json(ExecuteWorkItemActionRequest {
            actor: Some("lucas".to_string()),
            reason: "approve the exact digest-pinned GitOps update".to_string(),
            state_hash: approve_gitops_change_set.state_hash.clone(),
            inference_policies: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(reviewed["gitops_change_set"]["status"], json!("approved"));
    let Json(base_revision_flow) =
        work_item_flow(State(state.clone()), Path(work_item_id.to_string()))
            .await
            .unwrap();
    let base_revision_action = base_revision_flow
        .action_rail
        .iter()
        .find(|action| action.id == "awaiting_gitops_base_revision")
        .expect("approved GitOps ChangeSet must expose base-revision observation");
    assert_eq!(base_revision_action.status, "ready");
    let Json(base_revision_dispatch) = execute_work_item_action(
        State(state.clone()),
        None,
        Path((work_item_id.to_string(), base_revision_action.id.clone())),
        Json(ExecuteWorkItemActionRequest {
            actor: Some("lucas".to_string()),
            reason: "observe the exact disposable GitOps base revision".to_string(),
            state_hash: base_revision_action.state_hash.clone(),
            inference_policies: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(base_revision_dispatch["applied"], json!(true));
    assert_eq!(
        base_revision_dispatch["action"],
        json!("awaiting_gitops_base_revision")
    );
    assert_eq!(
        base_revision_dispatch["controller_wait"]["wait_kind"],
        json!("gitops_base_revision")
    );
    let artifacts = state.store.list_artifacts(&run_id).await.unwrap();
    assert_eq!(
        artifacts
            .iter()
            .filter(|artifact| artifact.kind == "gitops_base_revision_execution")
            .count(),
        1
    );
    let audit = state
        .store
        .list_audit_events(Some("work_item"), Some(work_item_id), None, 20)
        .await
        .unwrap();
    assert_eq!(
        audit
            .iter()
            .filter(|event| event.kind == "work_item.gitops_base_revision_dispatched")
            .count(),
        1
    );
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_gitops_controller_base".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "gitops_base_revision".to_string(),
            label: "Immutable GitOps base revision".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": "grev_gitops_controller",
                "status": "resolved",
                "gitops_change_set_id": gitops_change_set_id,
                "material_hash": "material_gitops_controller",
                "repository": gitops_repo,
                "base_ref": "main",
                "base_commit": gitops_base_commit,
                "identity": "agent:git-observer",
            })),
        })
        .await
        .unwrap();
    let wait = state
        .store
        .get_active_controller_wait_for_work_item(work_item_id)
        .await
        .unwrap()
        .expect("base-revision dispatch must create a controller wait");
    state
        .store
        .resolve_controller_wait(
            &wait.id,
            "resolved",
            "durable base-revision fixture is present".to_string(),
        )
        .await
        .unwrap();
    let Json(delivery_plan_flow) =
        work_item_flow(State(state.clone()), Path(work_item_id.to_string()))
            .await
            .unwrap();
    let delivery_plan_action = delivery_plan_flow
        .action_rail
        .iter()
        .find(|action| action.id == "awaiting_gitops_delivery_plan")
        .expect("resolved base revision must expose GitOps delivery planning");
    assert_eq!(delivery_plan_action.status, "ready");
    assert_eq!(delivery_plan_action.effect_class, "internal");
    let Json(prepared) = execute_work_item_action(
        State(state.clone()),
        None,
        Path((work_item_id.to_string(), delivery_plan_action.id.clone())),
        Json(ExecuteWorkItemActionRequest {
            actor: Some("lucas".to_string()),
            reason: "prepare exact disposable GitOps writer input".to_string(),
            state_hash: delivery_plan_action.state_hash.clone(),
            inference_policies: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(prepared["applied"], json!(true));
    assert_eq!(prepared["action"], json!("awaiting_gitops_delivery_plan"));
    let plan_artifact_id = state
        .store
        .list_artifacts(&run_id)
        .await
        .unwrap()
        .into_iter()
        .filter(|artifact| artifact.kind == "gitops_delivery_plan")
        .max_by_key(|artifact| (artifact.created_at.clone(), artifact.id.clone()))
        .expect("controller action must persist one GitOps delivery plan")
        .id;
    for gate in approval_gates_from_work_item(&work_item, &work_plan) {
        state.store.create_approval_gate(gate).await.unwrap();
    }
    let gitops_gate = state
        .store
        .list_approval_gates(ApprovalGateListFilter {
            work_item_id: Some(work_item_id.to_string()),
            gate_kind: Some("gitops_mutation".to_string()),
            limit: 1,
            ..ApprovalGateListFilter::default()
        })
        .await
        .unwrap()
        .pop()
        .unwrap();
    state
        .store
        .decide_approval_gate(
            &gitops_gate.id,
            "satisfied",
            Some("lucas".to_string()),
            Some("approve exact disposable GitOps update".to_string()),
        )
        .await
        .unwrap();
    let Json(authorization_flow) =
        work_item_flow(State(state.clone()), Path(work_item_id.to_string()))
            .await
            .unwrap();
    let authorization_action = authorization_flow
        .action_rail
        .iter()
        .find(|action| action.id == "authorize_gitops_delivery")
        .expect("satisfied GitOps gate must expose writer authorization");
    assert_eq!(authorization_action.status, "ready");
    assert_eq!(authorization_action.effect_class, "approval_boundary");
    let Json(authorization) = execute_work_item_action(
        State(state.clone()),
        None,
        Path((work_item_id.to_string(), authorization_action.id.clone())),
        Json(ExecuteWorkItemActionRequest {
            actor: Some("lucas".to_string()),
            reason: "authorize one exact disposable GitOps update".to_string(),
            state_hash: authorization_action.state_hash.clone(),
            inference_policies: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        authorization.pointer("/preflight/status"),
        Some(&json!("ready_for_writer"))
    );
    assert_eq!(
        authorization.pointer("/preflight/dispatch_ready"),
        Some(&json!(true))
    );
    let grant_id = authorization
        .pointer("/authorization/grant/id")
        .and_then(Value::as_str)
        .expect("authorization action must return its exact PermissionGrant");
    assert_eq!(
        authorization
            .pointer("/preflight/permission_grant/id")
            .and_then(Value::as_str),
        Some(grant_id)
    );
    assert_eq!(
        authorization
            .pointer("/preflight/plan/id")
            .and_then(Value::as_str),
        Some(plan_artifact_id.as_str())
    );

    let Json(preview) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.to_string()),
        Json(ReconcileWorkItemRequest {
            apply: false,
            actor: Some("lucas".to_string()),
            reason: None,
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(preview.action, "awaiting_gitops_delivery_execution");
    assert!(!preview.applied);

    let Json(applied) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.to_string()),
        Json(ReconcileWorkItemRequest {
            apply: true,
            actor: Some("lucas".to_string()),
            reason: Some("dispatch approved disposable GitOps writer".to_string()),
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert!(applied.applied);
    assert_eq!(
        applied
            .controller_wait
            .as_ref()
            .map(|wait| wait.wait_kind.as_str()),
        Some("gitops_delivery_execution")
    );
    assert!(applied
        .message
        .contains("dispatched the approved isolated GitOps writer"));

    let Json(repeated) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.to_string()),
        Json(ReconcileWorkItemRequest {
            apply: true,
            actor: Some("lucas".to_string()),
            reason: Some("retain bounded GitOps writer wait".to_string()),
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(repeated.action, "wait_for_gitops_delivery");
    let artifacts = state.store.list_artifacts(&run_id).await.unwrap();
    assert_eq!(
        artifacts
            .iter()
            .filter(|artifact| artifact.kind == "gitops_delivery_execution")
            .count(),
        1
    );
    let audit = state
        .store
        .list_audit_events(Some("work_item"), Some(work_item_id), None, 20)
        .await
        .unwrap();
    assert_eq!(
        audit
            .iter()
            .filter(|event| event.kind == "work_item.gitops_delivery_dispatched")
            .count(),
        1
    );

    let execution_id = artifacts
        .iter()
        .find(|artifact| artifact.kind == "gitops_delivery_execution")
        .and_then(|artifact| artifact.content_json.as_ref())
        .and_then(|content| content.get("execution_id"))
        .and_then(Value::as_str)
        .expect("dispatched GitOps delivery must have an execution id")
        .to_string();
    let Json(_) = internal_gitops_delivery_outcome(
        State(state.clone()),
        Path(gitops_change_set_id.to_string()),
        Json(GitOpsDeliveryOutcomeRequest {
            execution_id,
            status: "failed".to_string(),
            error_code: Some("git_push_permission_denied".to_string()),
            branch: None,
            commit_sha: None,
            pull_request_url: None,
            pull_request_number: None,
        }),
    )
    .await
    .unwrap();
    let Json(failed_flow) = work_item_flow(State(state.clone()), Path(work_item_id.to_string()))
        .await
        .unwrap();
    let retry_review = failed_flow
        .action_rail
        .iter()
        .find(|action| action.id == "repropose_gitops_change_set")
        .expect("failed GitOps delivery must expose an explicit retry review action");
    assert_eq!(retry_review.status, "ready");
    assert_eq!(retry_review.effect_class, "approval_boundary");
    let Json(reproposed) = execute_work_item_action(
        State(state.clone()),
        None,
        Path((work_item_id.to_string(), retry_review.id.clone())),
        Json(ExecuteWorkItemActionRequest {
            actor: Some("lucas".to_string()),
            reason: "review a new immutable GitOps delivery attempt".to_string(),
            state_hash: retry_review.state_hash.clone(),
            inference_policies: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        reproposed.pointer("/gitops_change_set/status"),
        Some(&json!("proposed"))
    );
    assert_eq!(
        reproposed.pointer("/gitops_change_set/revision"),
        Some(&json!(2))
    );
    assert_eq!(
        state
            .store
            .get_permission_grant(grant_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "revoked"
    );
    let reproposed_change_set = state
        .store
        .get_gitops_change_set(gitops_change_set_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        reproposed_change_set.head_branch,
        "pharness/witem-gitops-controller/gitops-revision-2"
    );
    assert!(!reproposed_change_set
        .head_branch
        .starts_with("pharness/witem-gitops-controller/gitops/"));
    assert_eq!(
        gitops_base_revision_reconcile_state(&state.store, &reproposed_change_set)
            .await
            .unwrap(),
        GitOpsBaseRevisionReconcileState::Missing
    );
    fs::remove_file(kubectl_stub).unwrap();
}

#[tokio::test]
async fn reconcile_dispatches_one_preflighted_argo_runner_after_gitops_merge_then_waits() {
    let kubectl_stub = std::env::temp_dir().join(format!(
        "pharness-argo-controller-kubectl-{}",
        unique_suffix()
    ));
    fs::write(&kubectl_stub, "#!/bin/sh\ncat >/dev/null\nexit 0\n").unwrap();
    fs::set_permissions(&kubectl_stub, fs::Permissions::from_mode(0o755)).unwrap();
    let state = test_state_with_git_observer(
        kubectl_stub.to_string_lossy().to_string(),
        "https://github.com/example/finance-gitops.git".to_string(),
    )
    .await;
    let session_id = SessionId::new("ses_argo_controller");
    let run_id = RunId::new("run_argo_controller");
    let work_item_id = "witem_argo_controller";
    let work_plan_id = "wplan_argo_controller";
    let change_set_id = "cset_argo_controller";
    let pipeline_intent_id = "pint_argo_controller";
    let deployment_intent_id = "dint_argo_controller";
    let gitops_change_set_id = "gset_argo_controller";
    let source_commit = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
    let gitops_commit = "b1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
    let gitops_repo = "https://github.com/example/finance-gitops.git";

    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: "Argo controller fixture".to_string(),
            cwd: "/workspace".to_string(),
        })
        .await
        .unwrap();
    state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: "Deploy reviewed disposable finance GitOps revision".to_string(),
            cwd: "/workspace".to_string(),
            max_turns: 1,
            initial_status: "completed".to_string(),
            execution_target_json: json!({}),
        })
        .await
        .unwrap();
    let work_item = state
        .store
        .create_work_item(CreateWorkItem {
            id: work_item_id.to_string(),
            status: "awaiting_approval".to_string(),
            title: "Deploy disposable finance API to dev".to_string(),
            intent: "Sync the reviewed disposable GitOps revision to dev.".to_string(),
            acceptance_criteria: vec!["Argo target is exact and reviewed".to_string()],
            source_repo: "https://github.com/example/finance-api.git".to_string(),
            source_ref: "main".to_string(),
            source_commit: None,
            pipeline_contract_id: None,
            deployment_contract_id: None,
            gitops_repo: Some(gitops_repo.to_string()),
            gitops_ref: Some("main".to_string()),
            gitops_kustomization_path: None,
            gitops_image_name: None,
            target_environment: "dev".to_string(),
            target_namespace: Some("apps-dev".to_string()),
            argo_application: Some("finance-api".to_string()),
            workload_kind: None,
            workload_name: None,
            rollback_owner: None,
            production_impacting: false,
            max_attempts: 1,
            max_elapsed_seconds: 600,
            environment_profile_id: None,
            run_budget: Default::default(),
            repository_contract_json: None,
            repository_contract_hash: None,
            environment_preparation_status: "not_required".to_string(),
            created_by: Some("lucas".to_string()),
        })
        .await
        .unwrap();
    let work_plan = state
        .store
        .create_work_plan(CreateWorkPlan {
            id: work_plan_id.to_string(),
            work_item_id: Some(work_item_id.to_string()),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Disposable finance deployment plan".to_string(),
            summary: "Deliver one reviewed dev revision.".to_string(),
            risk_level: "high".to_string(),
            requires_approval: true,
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Application".to_string()),
            resource_name: Some("finance-api".to_string()),
            work_plan_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_change_set(CreateChangeSet {
            id: change_set_id.to_string(),
            work_item_id: Some(work_item_id.to_string()),
            work_plan_id: work_plan_id.to_string(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Disposable finance source ChangeSet".to_string(),
            summary: "Reviewed source change.".to_string(),
            risk_level: "medium".to_string(),
            material_hash: "material_argo_controller".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: None,
            resource_name: None,
            change_set_json: json!({}),
        })
        .await
        .unwrap();
    state
            .store
            .create_artifact(CreateArtifact {
                id: "art_argo_controller_source_plan".to_string(),
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                kind: "git_delivery_plan".to_string(),
                label: "Immutable source delivery plan".to_string(),
                mime_type: Some("application/json".to_string()),
                path: None,
                content_text: None,
                content_json: Some(json!({
                    "change_set": { "id": change_set_id, "revision": 1, "material_hash": "material_argo_controller" }
                })),
            })
            .await
            .unwrap();
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_argo_controller_source_merge".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "git_delivery_merge".to_string(),
            label: "Immutable source merge".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "change_set_id": change_set_id,
                "git_delivery_plan_artifact_id": "art_argo_controller_source_plan",
                "head_commit_sha": source_commit,
                "merge_commit_sha": source_commit,
            })),
        })
        .await
        .unwrap();
    state
        .store
        .create_pipeline_intent(CreatePipelineIntent {
            id: pipeline_intent_id.to_string(),
            change_set_id: change_set_id.to_string(),
            work_plan_id: work_plan_id.to_string(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Verified disposable finance build".to_string(),
            summary: "Build evidence is ready for deployment.".to_string(),
            risk_level: "medium".to_string(),
            intent_kind: "tekton_build_test_package".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("Pipeline".to_string()),
            resource_name: Some("finance-ci".to_string()),
            intent_json: json!({
                "execution_state": { "state": "pipeline_run_succeeded" },
                "evidence": { "status": "satisfied" },
                "build_output": { "status": "verified" }
            }),
        })
        .await
        .unwrap();
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_argo_controller_gitops_update".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "gitops_update_plan".to_string(),
            label: "Verified GitOps update".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({})),
        })
        .await
        .unwrap();
    state
        .store
        .create_deployment_intent(CreateDeploymentIntent {
            id: deployment_intent_id.to_string(),
            pipeline_intent_id: pipeline_intent_id.to_string(),
            change_set_id: change_set_id.to_string(),
            work_plan_id: work_plan_id.to_string(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Disposable finance dev deployment".to_string(),
            summary: "Sync exact reviewed Application target.".to_string(),
            risk_level: "high".to_string(),
            intent_kind: "argo_sync_deploy".to_string(),
            target_environment: Some("dev".to_string()),
            target_namespace: Some("apps-dev".to_string()),
            argo_application: Some("finance-api".to_string()),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Application".to_string()),
            resource_name: Some("finance-api".to_string()),
            intent_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_gitops_change_set(CreateGitOpsChangeSet {
            id: gitops_change_set_id.to_string(),
            work_item_id: work_item_id.to_string(),
            work_plan_id: work_plan_id.to_string(),
            source_change_set_id: change_set_id.to_string(),
            pipeline_intent_id: pipeline_intent_id.to_string(),
            deployment_intent_id: deployment_intent_id.to_string(),
            gitops_update_plan_artifact_id: "art_argo_controller_gitops_update".to_string(),
            session_id: session_id.clone(),
            run_id: run_id.clone(),
            status: "approved".to_string(),
            title: "Disposable finance GitOps ChangeSet".to_string(),
            summary: "Reviewed digest-pinned dev update.".to_string(),
            risk_level: "high".to_string(),
            material_hash: "material_argo_controller".to_string(),
            gitops_repo: gitops_repo.to_string(),
            gitops_ref: "main".to_string(),
            head_branch: "pharness/witem-argo-controller/gitops".to_string(),
            kustomization_path: "apps/finance-api/kustomization.yaml".to_string(),
            image_name: "registry.example.test/finance-api".to_string(),
            image_ref: "registry.example.test/finance-api@sha256:1234567890abcdef".to_string(),
            gitops_change_set_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_argo_controller_gitops_base".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "gitops_base_revision".to_string(),
            label: "Immutable GitOps base revision".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "status": "resolved",
                "gitops_change_set_id": gitops_change_set_id,
                "material_hash": "material_argo_controller",
                "repository": gitops_repo,
                "base_ref": "main",
                "base_commit": gitops_commit,
            })),
        })
        .await
        .unwrap();
    state
            .store
            .create_artifact(CreateArtifact {
                id: "art_argo_controller_gitops_plan".to_string(),
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                kind: "gitops_delivery_plan".to_string(),
                label: "Immutable GitOps delivery plan".to_string(),
                mime_type: Some("application/json".to_string()),
                path: None,
                content_text: None,
                content_json: Some(json!({
                    "gitops_change_set": { "id": gitops_change_set_id, "revision": 1, "material_hash": "material_argo_controller" },
                    "source": { "base_revision_artifact_id": "art_argo_controller_gitops_base" }
                })),
            })
            .await
            .unwrap();
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_argo_controller_gitops_merge".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "gitops_delivery_merge".to_string(),
            label: "Immutable GitOps merge".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "gitops_change_set_id": gitops_change_set_id,
                "gitops_delivery_plan_artifact_id": "art_argo_controller_gitops_plan",
                "merge_commit_sha": gitops_commit,
            })),
        })
        .await
        .unwrap();
    let Json(contract) = create_deployment_contract(
        State(state.clone()),
        None,
        Json(CreateDeploymentContractRequest {
            target_environment: "dev".to_string(),
            target_namespace: "apps-dev".to_string(),
            argo_application: "finance-api".to_string(),
            version: Some("v1".to_string()),
            contract_json: json!({ "operation": "sync", "prune": false, "force": false }),
            actor: Some("lucas".to_string()),
            reason: Some("exact disposable Argo target".to_string()),
        }),
    )
    .await
    .unwrap();
    for gate in approval_gates_from_work_item(&work_item, &work_plan) {
        state.store.create_approval_gate(gate).await.unwrap();
    }
    let cluster_gate = state
        .store
        .list_approval_gates(ApprovalGateListFilter {
            work_item_id: Some(work_item_id.to_string()),
            gate_kind: Some("cluster_mutation".to_string()),
            limit: 1,
            ..ApprovalGateListFilter::default()
        })
        .await
        .unwrap()
        .pop()
        .unwrap();
    state
        .store
        .decide_approval_gate(
            &cluster_gate.id,
            "satisfied",
            Some("lucas".to_string()),
            Some("approve exact disposable Argo sync".to_string()),
        )
        .await
        .unwrap();
    let Json(authorization_flow) =
        work_item_flow(State(state.clone()), Path(work_item_id.to_string()))
            .await
            .unwrap();
    let authorization_action = authorization_flow
        .action_rail
        .iter()
        .find(|action| action.id == "authorize_deployment_execution")
        .expect("satisfied deployment gates must expose Argo authorization");
    assert_eq!(authorization_action.status, "ready");
    assert_eq!(authorization_action.effect_class, "approval_boundary");
    let Json(envelope) = execute_work_item_action(
        State(state.clone()),
        None,
        Path((work_item_id.to_string(), authorization_action.id.clone())),
        Json(ExecuteWorkItemActionRequest {
            actor: Some("lucas".to_string()),
            reason: "authorize one exact disposable Argo sync".to_string(),
            state_hash: authorization_action.state_hash.clone(),
            inference_policies: None,
        }),
    )
    .await
    .unwrap();
    let Json(preflight) = preflight_deployment_intent(
        State(state.clone()),
        None,
        Path(deployment_intent_id.to_string()),
        Json(DeploymentIntentPreflightRequest {
            actor: Some("lucas".to_string()),
            reason: Some("record controller Argo readiness".to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(preflight.status, "ready_for_argo_runner");
    assert!(preflight.dispatch_ready);
    assert_eq!(
        preflight
            .permission_grant
            .as_ref()
            .map(|grant| grant.id.as_str()),
        envelope.pointer("/grant/id").and_then(Value::as_str)
    );
    assert_eq!(
        preflight
            .deployment_contract
            .as_ref()
            .map(|item| item.id.as_str()),
        Some(contract.id.as_str())
    );

    let Json(preview) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.to_string()),
        Json(ReconcileWorkItemRequest {
            apply: false,
            actor: Some("lucas".to_string()),
            reason: None,
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(preview.action, "awaiting_deployment_execution");
    assert!(!preview.applied);

    let Json(applied) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.to_string()),
        Json(ReconcileWorkItemRequest {
            apply: true,
            actor: Some("lucas".to_string()),
            reason: Some("dispatch approved disposable Argo runner".to_string()),
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert!(applied.applied);
    assert_eq!(
        applied
            .controller_wait
            .as_ref()
            .map(|wait| wait.wait_kind.as_str()),
        Some("deployment_execution")
    );
    assert!(applied
        .message
        .contains("dispatched the approved isolated Argo runner"));

    let Json(repeated) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.to_string()),
        Json(ReconcileWorkItemRequest {
            apply: true,
            actor: Some("lucas".to_string()),
            reason: Some("retain bounded Argo runner wait".to_string()),
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(repeated.action, "wait_for_deployment_execution");
    let artifacts = state.store.list_artifacts(&run_id).await.unwrap();
    assert_eq!(
        artifacts
            .iter()
            .filter(|artifact| artifact.kind == "argo_sync_execution")
            .count(),
        1
    );
    let audit = state
        .store
        .list_audit_events(Some("work_item"), Some(work_item_id), None, 20)
        .await
        .unwrap();
    assert_eq!(
        audit
            .iter()
            .filter(|event| event.kind == "work_item.deployment_execution_dispatched")
            .count(),
        1
    );
    let execution = artifacts
        .iter()
        .find(|artifact| artifact.kind == "argo_sync_execution")
        .expect("controller dispatch must persist one Argo execution");
    let execution_id = execution
        .content_json
        .as_ref()
        .and_then(|content| content.get("execution_id"))
        .and_then(Value::as_str)
        .expect("Argo execution must have an immutable execution ID");
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_argo_controller_sync_result".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "argo_sync_result".to_string(),
            label: "Completed disposable Argo sync".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": execution_id,
                "status": "completed",
                "deployment_intent_id": deployment_intent_id,
                "details": {
                    "sync_status": "Synced",
                    "operation_phase": "Succeeded",
                    "revision": gitops_commit,
                }
            })),
        })
        .await
        .unwrap();
    let deployment_wait = state
        .store
        .get_active_controller_wait_for_work_item(work_item_id)
        .await
        .unwrap()
        .expect("Argo dispatch must create a controller wait");
    state
        .store
        .resolve_controller_wait(
            &deployment_wait.id,
            "resolved",
            "durable Argo completion fixture is present".to_string(),
        )
        .await
        .unwrap();

    let Json(release_flow) = work_item_flow(State(state.clone()), Path(work_item_id.to_string()))
        .await
        .unwrap();
    let release_action = release_flow
        .action_rail
        .iter()
        .find(|action| action.id == "awaiting_release_definition")
        .expect("completed Argo sync must expose Release definition");
    assert_eq!(release_action.status, "ready");
    assert_eq!(release_action.effect_class, "internal");
    let Json(release_result) = execute_work_item_action(
        State(state.clone()),
        None,
        Path((work_item_id.to_string(), release_action.id.clone())),
        Json(ExecuteWorkItemActionRequest {
            actor: Some("lucas".to_string()),
            reason: "propose exact disposable release".to_string(),
            state_hash: release_action.state_hash.clone(),
            inference_policies: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(release_result["applied"], json!(true));
    assert_eq!(
        release_result["action"],
        json!("awaiting_release_definition")
    );
    let release = state
        .store
        .get_release_by_deployment_intent(deployment_intent_id)
        .await
        .unwrap()
        .expect("Release definition action must persist a proposed Release");
    assert_eq!(release.status, "proposed");
    assert_eq!(release.commit_sha.as_deref(), Some(gitops_commit));
    fs::remove_file(kubectl_stub).unwrap();
}

pub(super) fn reconcile_artifact(kind: &str, content_json: serde_json::Value) -> ArtifactResponse {
    ArtifactResponse {
        id: format!("art_{kind}"),
        run_id: None,
        kind: kind.to_string(),
        label: kind.to_string(),
        mime_type: Some("application/json".to_string()),
        path: None,
        content_text: None,
        content_json: Some(content_json),
        created_at: "1".to_string(),
    }
}

pub(super) fn reconcile_git_delivery_flow() -> GitDeliveryFlowResponse {
    GitDeliveryFlowResponse {
        plan: reconcile_artifact("git_delivery_plan", json!({})),
        latest_preflight: None,
        latest_execution: None,
        latest_result: None,
        latest_observation: None,
        latest_merge: None,
    }
}

pub(super) fn reconcile_gitops_change_set(status: &str) -> StoredGitOpsChangeSet {
    StoredGitOpsChangeSet {
        id: "gset_reconcile".to_string(),
        work_item_id: "witem_reconcile".to_string(),
        work_plan_id: "wplan_reconcile".to_string(),
        source_change_set_id: "cset_reconcile".to_string(),
        pipeline_intent_id: "pint_reconcile".to_string(),
        deployment_intent_id: "dint_reconcile".to_string(),
        gitops_update_plan_artifact_id: "art_gitops_update_plan".to_string(),
        session_id: SessionId::new("ses_reconcile"),
        run_id: RunId::new("run_reconcile"),
        status: status.to_string(),
        title: "Reconcile GitOps ChangeSet".to_string(),
        summary: "Reconcile GitOps delivery state".to_string(),
        risk_level: "high".to_string(),
        material_hash: "material_reconcile".to_string(),
        revision: 1,
        gitops_repo: "https://github.com/example/finance-gitops.git".to_string(),
        gitops_ref: "main".to_string(),
        head_branch: "pharness/gset-reconcile".to_string(),
        kustomization_path: "apps/dev/finance-api".to_string(),
        image_name: "registry.example/finance-api".to_string(),
        image_ref: "sha256:reconcile".to_string(),
        gitops_change_set_json: json!({}),
        created_at: "1".to_string(),
        updated_at: None,
        status_changed_at: None,
        status_changed_by: None,
        status_reason: None,
    }
}

pub(super) fn reconcile_gitops_delivery_flow() -> GitOpsDeliveryFlowResponse {
    GitOpsDeliveryFlowResponse {
        plan: reconcile_artifact("gitops_delivery_plan", json!({})),
        base_revision: reconcile_artifact("gitops_base_revision", json!({})),
        latest_preflight: None,
        latest_execution: None,
        latest_result: None,
        latest_observation: None,
        latest_merge: None,
    }
}

pub(super) fn reconcile_pipeline_intent(
    status: &str,
    execution_state: Option<&str>,
    build_output_status: Option<&str>,
    evidence_status: Option<&str>,
) -> StoredPipelineIntent {
    let mut intent_json = serde_json::Map::new();
    if let Some(execution_state) = execution_state {
        intent_json.insert(
            "execution_state".to_string(),
            json!({ "state": execution_state }),
        );
    }
    if let Some(build_output_status) = build_output_status {
        intent_json.insert(
            "build_output".to_string(),
            json!({ "status": build_output_status }),
        );
    }
    if let Some(evidence_status) = evidence_status {
        intent_json.insert("evidence".to_string(), json!({ "status": evidence_status }));
    }
    StoredPipelineIntent {
        id: "pint_reconcile".to_string(),
        change_set_id: "cset_reconcile".to_string(),
        work_plan_id: "wplan_reconcile".to_string(),
        remediation_plan_id: None,
        incident_id: None,
        session_id: SessionId::new("ses_reconcile"),
        run_id: None,
        status: status.to_string(),
        title: "Reconcile pipeline intent".to_string(),
        summary: "Reconcile pipeline intent state".to_string(),
        risk_level: "high".to_string(),
        intent_kind: "tekton_build_test_package".to_string(),
        resource_namespace: Some("ci".to_string()),
        resource_kind: Some("Pipeline".to_string()),
        resource_name: Some("finance-ci".to_string()),
        intent_json: Value::Object(intent_json),
        created_at: "1".to_string(),
        updated_at: None,
        status_changed_at: None,
        status_changed_by: None,
        status_reason: None,
    }
}

pub(super) fn reconcile_deployment_preflight(ready: bool) -> DeploymentIntentExecutionPreflight {
    DeploymentIntentExecutionPreflight {
        ready,
        intent: reconcile_deployment_intent(),
        contract: None,
        grant: None,
        gitops_merge: None,
        checks: Vec::new(),
    }
}

pub(super) fn reconcile_deployment_delivery() -> DeploymentIntentDeliveryFlowResponse {
    DeploymentIntentDeliveryFlowResponse {
        latest_execution: None,
        latest_result: None,
        release: None,
    }
}

pub(super) fn reconcile_release(status: &str) -> ReleaseResponse {
    StoredRelease {
        id: "rel_reconcile".to_string(),
        deployment_intent_id: "dint_reconcile".to_string(),
        pipeline_intent_id: "pint_reconcile".to_string(),
        change_set_id: "cset_reconcile".to_string(),
        work_plan_id: "wplan_reconcile".to_string(),
        remediation_plan_id: None,
        incident_id: None,
        session_id: SessionId::new("ses_reconcile"),
        run_id: Some(RunId::new("run_reconcile")),
        status: status.to_string(),
        title: "Reconcile release".to_string(),
        summary: "Reconcile release state".to_string(),
        risk_level: "high".to_string(),
        release_kind: "gitops_release".to_string(),
        target_environment: Some("dev".to_string()),
        target_namespace: Some("apps-dev".to_string()),
        argo_application: Some("finance-api".to_string()),
        version: None,
        commit_sha: None,
        image_digest: None,
        rollback_ref: None,
        release_json: json!({}),
        created_at: "1".to_string(),
        updated_at: None,
        status_changed_at: None,
        status_changed_by: None,
        status_reason: None,
    }
    .into()
}
