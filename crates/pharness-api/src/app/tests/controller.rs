use super::{
    advance_work_item, approval_gates_from_work_item, authorize_change_set_git_delivery,
    cancel_work_item, change_set_flow, create_change_set, create_work_item,
    create_work_item_pipeline_intent, create_work_plan_from_work_item, current_millis,
    execute_work_item_action, fs, internal_workspace_provisioned, json,
    list_work_item_controller_waits, list_work_item_events, list_work_items, list_workspaces,
    observe_due_controller_wait, pipeline_intent_execution_preflight,
    preflight_change_set_git_delivery, prepare_change_set_git_delivery,
    reconcile_due_controller_waits, reconcile_work_item, replan_work_item, revise_work_plan,
    schedule_controller_wait, supersede_active_controller_wait_if_present, transition_work_item,
    work_item_flow, work_item_pipeline_intent_context, AdvanceWorkItemRequest, AppState,
    ApprovalGateListFilter, ApprovalGateSummaryFilter, Arc, BuildMetadata, CreateArtifact,
    CreateChangeSet, CreateChangeSetRequest, CreateControllerWait,
    CreateGitDeliveryAuthorizationRequest, CreatePipelineContract, CreatePipelineIntent, CreateRun,
    CreateSession, CreateWorkItem, CreateWorkItemPipelineIntentRequest, CreateWorkItemRequest,
    CreateWorkPlan, CreateWorkspace, Digest, ExecuteWorkItemActionRequest,
    GitDeliveryPreflightRequest, InternalWorkspaceProvisionedRequest, Json,
    ListControllerWaitsQuery, ListWorkItemsQuery, ListWorkspacesQuery, ObservationListFilter, Path,
    PrepareGitDeliveryRequest, ProtectedTargetConfiguration, Query, ReadOnlyClusterTools,
    ReconcileDueControllerWaitsRequest, ReconcileWorkItemRequest, ReplanWorkItemRequest,
    ReviseWorkPlanRequest, RunDispatcher, RunId, SafetyPolicy, SessionId, Sha256, SqliteStore,
    State, StatusCode, TransitionWorkItemRequest, Value, WorkItemPipelineContextQuery,
    WorkItemReconcileAction, WorkspaceProvisioner, CONTROLLER_WAIT_MAX_CHECKS,
    GIT_DELIVERY_ACTIONS,
};
use crate::app::RepoModeConfiguration;

use super::characterization::{
    fake_completed_argo_wait_kubectl_script, fake_succeeded_tekton_kubectl_script,
    seed_approved_work_item_release, test_state, test_state_with_cluster_tools,
};

#[tokio::test]
async fn work_item_reconcile_previews_then_declares_the_review_boundary() {
    let state = test_state().await;
    let Json(work_item) = create_work_item(
        State(state.clone()),
        None,
        Json(CreateWorkItemRequest {
            title: "Add a finance smoke endpoint".to_string(),
            intent: "Expose a read-only health endpoint with a focused test.".to_string(),
            acceptance_criteria: vec!["Endpoint returns a stable response".to_string()],
            source_repo: "team/finance-api".to_string(),
            source_ref: "main".to_string(),
            source_commit: None,
            pipeline_contract_id: None,
            deployment_contract_id: None,
            gitops_repo: Some("team/finance-gitops".to_string()),
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
            max_attempts: Some(2),
            max_elapsed_seconds: Some(900),
            preflight_state_hash: None,
            environment_profile_id: None,
            initial_turn_budget: None,
            hard_turn_budget: None,
            initial_token_budget: None,
            hard_token_budget: None,
            active_execution_seconds: None,
            recoverable_tool_error_limit: None,
            identical_failure_limit: None,
            actor: Some("operator".to_string()),
        }),
    )
    .await
    .unwrap();

    let Json(listed) = list_work_items(
        State(state.clone()),
        Query(ListWorkItemsQuery {
            status: Some("submitted".to_string()),
            source_repo: Some("team/finance-api".to_string()),
            target_environment: Some("dev".to_string()),
            target_namespace: Some("apps-dev".to_string()),
            production_impacting: Some(false),
            actor: Some("operator".to_string()),
            origin: Some("operator".to_string()),
            mode: None,
            product_id: None,
            repository_id: None,
            lifecycle: None,
            search: None,
            include: Some("operator_state".to_string()),
            limit: Some(1),
            offset: Some(1),
        }),
    )
    .await
    .unwrap();
    assert_eq!(listed.count, 1);
    assert!(listed.work_items.is_empty());
    assert!(listed.operator_state.is_some_and(|state| state.is_empty()));

    let Json(preview) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item.id.clone()),
        Json(ReconcileWorkItemRequest {
            apply: false,
            actor: Some("operator".to_string()),
            reason: None,
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(preview.action, "declare_work_plan");
    assert!(!preview.applied);
    assert_eq!(preview.work_item.status, "submitted");
    assert!(preview.work_plan.is_none());

    let stale = execute_work_item_action(
        State(state.clone()),
        None,
        Path((work_item.id.clone(), preview.action.clone())),
        Json(ExecuteWorkItemActionRequest {
            actor: Some("operator".to_string()),
            reason: "reviewed current controller boundary".to_string(),
            state_hash: "stale-preview-hash".to_string(),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(stale.status, StatusCode::CONFLICT);
    assert!(stale.message.contains("preview is stale"));

    let Json(advanced) = advance_work_item(
        State(state.clone()),
        None,
        Path(work_item.id.clone()),
        Json(AdvanceWorkItemRequest {
            actor: Some("operator".to_string()),
            reason: "advance reviewed feature intent".to_string(),
            max_steps: Some(10),
        }),
    )
    .await
    .unwrap();
    assert_eq!(advanced.steps.len(), 1);
    assert_eq!(advanced.stopped_at.id, "awaiting_work_plan_approval");
    let applied = &advanced.steps[0];
    assert_eq!(applied.action, "declare_work_plan");
    assert!(applied.applied);
    assert_eq!(applied.work_item.status, "awaiting_approval");
    assert!(applied.work_plan.is_some());
    assert_eq!(
        applied
            .workspace
            .as_ref()
            .map(|workspace| workspace.status.as_str()),
        Some("declared")
    );

    let Json(waiting) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item.id.clone()),
        Json(ReconcileWorkItemRequest {
            apply: false,
            actor: Some("operator".to_string()),
            reason: None,
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(waiting.action, "awaiting_work_plan_approval");
    assert!(!waiting.applied);

    let Json(flow) = work_item_flow(State(state.clone()), Path(work_item.id.clone()))
        .await
        .unwrap();
    let approve = flow
        .action_rail
        .iter()
        .find(|action| action.id == "approve_work_plan")
        .expect("proposed WorkPlan must be reviewable from the action rail");
    let stale_review = execute_work_item_action(
        State(state.clone()),
        None,
        Path((work_item.id.clone(), approve.id.clone())),
        Json(ExecuteWorkItemActionRequest {
            actor: Some("operator".to_string()),
            reason: "stale WorkPlan review".to_string(),
            state_hash: "stale-review".to_string(),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(stale_review.status, StatusCode::CONFLICT);
    let Json(reviewed) = execute_work_item_action(
        State(state.clone()),
        None,
        Path((work_item.id.clone(), approve.id.clone())),
        Json(ExecuteWorkItemActionRequest {
            actor: Some("operator".to_string()),
            reason: "approve the bounded WorkPlan".to_string(),
            state_hash: approve.state_hash.clone(),
        }),
    )
    .await
    .unwrap();
    assert_eq!(reviewed["work_plan"]["status"], "approved");
    let Json(reviewed_flow) = work_item_flow(State(state.clone()), Path(work_item.id.clone()))
        .await
        .unwrap();
    assert!(reviewed_flow
        .action_rail
        .iter()
        .any(|action| action.id == "authorize_workspace_and_start" && action.status == "ready"));

    let events = state
        .store
        .list_audit_events(Some("work_item"), Some(&work_item.id), None, 20)
        .await
        .unwrap();
    assert!(events
        .iter()
        .any(|event| event.kind == "work_item.planning"));
    assert!(events
        .iter()
        .any(|event| event.kind == "work_item.work_plan_created"));
}

#[tokio::test]
async fn controller_waits_are_bounded_idempotent_and_audited() {
    let state = test_state().await;
    let Json(created) = create_work_item(
        State(state.clone()),
        None,
        Json(CreateWorkItemRequest {
            title: "Observe a bounded pipeline run".to_string(),
            intent: "Wait for a terminal development pipeline result.".to_string(),
            acceptance_criteria: vec!["Terminal status is recorded".to_string()],
            source_repo: "team/finance-api".to_string(),
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
            argo_application: Some("finance-api".to_string()),
            workload_kind: None,
            workload_name: None,
            rollback_owner: None,
            production_impacting: false,
            max_attempts: Some(2),
            max_elapsed_seconds: Some(600),
            preflight_state_hash: None,
            environment_profile_id: None,
            initial_turn_budget: None,
            hard_turn_budget: None,
            initial_token_budget: None,
            hard_token_budget: None,
            active_execution_seconds: None,
            recoverable_tool_error_limit: None,
            identical_failure_limit: None,
            actor: Some("operator".to_string()),
        }),
    )
    .await
    .unwrap();
    let work_item = state
        .store
        .get_work_item(&created.id)
        .await
        .unwrap()
        .unwrap();

    let (scheduled, first_created) = schedule_controller_wait(
        &state,
        &work_item,
        WorkItemReconcileAction::WaitForPipelineExecution,
        Some("operator".to_string()),
    )
    .await
    .unwrap();
    assert!(first_created);
    assert_eq!(scheduled.status, "active");
    assert_eq!(scheduled.wait_kind, "pipeline_execution");
    assert_eq!(scheduled.max_checks, CONTROLLER_WAIT_MAX_CHECKS);
    assert_eq!(scheduled.check_count, 0);
    assert_eq!(scheduled.data_json["automatic_execution"], json!(false));
    assert_eq!(scheduled.data_json["automatic_retry"], json!(false));
    assert_eq!(scheduled.data_json["automatic_rollback"], json!(false));
    assert!(
        scheduled.deadline_at.parse::<u128>().unwrap()
            > scheduled.next_check_at.parse::<u128>().unwrap()
    );

    let (retained, second_created) = schedule_controller_wait(
        &state,
        &work_item,
        WorkItemReconcileAction::WaitForPipelineExecution,
        Some("operator".to_string()),
    )
    .await
    .unwrap();
    assert!(!second_created);
    assert_eq!(retained.id, scheduled.id);

    let Json(listed) = list_work_item_controller_waits(
        State(state.clone()),
        Path(work_item.id.clone()),
        Query(ListControllerWaitsQuery::default()),
    )
    .await
    .unwrap();
    assert_eq!(listed.count, 1);
    assert_eq!(listed.controller_waits[0].id, scheduled.id);

    let superseded = supersede_active_controller_wait_if_present(
        &state,
        &work_item.id,
        "terminal pipeline evidence was recorded".to_string(),
        Some("controller".to_string()),
    )
    .await
    .unwrap()
    .expect("active wait should be superseded");
    assert_eq!(superseded.status, "superseded");
    assert_eq!(
        superseded.resolution_reason.as_deref(),
        Some("terminal pipeline evidence was recorded")
    );

    let events = state
        .store
        .list_audit_events(Some("controller_wait"), Some(&scheduled.id), None, 20)
        .await
        .unwrap();
    assert!(events
        .iter()
        .any(|event| event.kind == "controller_wait.scheduled"));
    assert!(events
        .iter()
        .any(|event| event.kind == "controller_wait.superseded"));
}

#[tokio::test]
async fn due_controller_waits_only_progress_from_evidence_or_block_on_expiry() {
    let state = test_state().await;
    let Json(progress_item) = create_work_item(
        State(state.clone()),
        None,
        Json(CreateWorkItemRequest {
            title: "Observe controller progress".to_string(),
            intent: "Exercise durable evidence-only progression.".to_string(),
            acceptance_criteria: vec!["No external action is dispatched".to_string()],
            source_repo: "team/finance-api".to_string(),
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
            argo_application: Some("finance-api".to_string()),
            workload_kind: None,
            workload_name: None,
            rollback_owner: None,
            production_impacting: false,
            max_attempts: Some(2),
            max_elapsed_seconds: Some(600),
            preflight_state_hash: None,
            environment_profile_id: None,
            initial_turn_budget: None,
            hard_turn_budget: None,
            initial_token_budget: None,
            hard_token_budget: None,
            active_execution_seconds: None,
            recoverable_tool_error_limit: None,
            identical_failure_limit: None,
            actor: Some("operator".to_string()),
        }),
    )
    .await
    .unwrap();
    let progress_item_stored = state
        .store
        .get_work_item(&progress_item.id)
        .await
        .unwrap()
        .unwrap();
    let (progress_wait, _) = schedule_controller_wait(
        &state,
        &progress_item_stored,
        WorkItemReconcileAction::WaitForPipelineExecution,
        Some("operator".to_string()),
    )
    .await
    .unwrap();
    state
        .store
        .record_controller_wait_check(&progress_wait.id, "0".to_string())
        .await
        .unwrap();

    let Json(progressed) = reconcile_due_controller_waits(
        State(state.clone()),
        None,
        Json(ReconcileDueControllerWaitsRequest {
            limit: Some(10),
            actor: Some("controller".to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(progressed.checked, 1);
    assert_eq!(progressed.progressed, 1);
    assert_eq!(progressed.blocked, 0);
    assert_eq!(progressed.results[0].outcome, "progressed");
    assert_eq!(
        progressed.results[0].next_action.as_deref(),
        Some("declare_work_plan")
    );
    let progressed_wait = state
        .store
        .get_controller_wait(&progress_wait.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(progressed_wait.status, "resolved");

    let Json(expiring_item) = create_work_item(
        State(state.clone()),
        None,
        Json(CreateWorkItemRequest {
            title: "Expire a controller wait".to_string(),
            intent: "Prove a bounded wait stops safely.".to_string(),
            acceptance_criteria: vec!["The WorkItem blocks after expiry".to_string()],
            source_repo: "team/finance-api".to_string(),
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
            argo_application: Some("finance-api".to_string()),
            workload_kind: None,
            workload_name: None,
            rollback_owner: None,
            production_impacting: false,
            max_attempts: Some(2),
            max_elapsed_seconds: Some(600),
            preflight_state_hash: None,
            environment_profile_id: None,
            initial_turn_budget: None,
            hard_turn_budget: None,
            initial_token_budget: None,
            hard_token_budget: None,
            active_execution_seconds: None,
            recoverable_tool_error_limit: None,
            identical_failure_limit: None,
            actor: Some("operator".to_string()),
        }),
    )
    .await
    .unwrap();
    let session_id = SessionId::new("ses_controller_wait_expiry");
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: "controller wait expiry".to_string(),
            cwd: ".".to_string(),
        })
        .await
        .unwrap();
    let expiring_wait = state
        .store
        .create_controller_wait(CreateControllerWait {
            id: "cwait_expiry".to_string(),
            work_item_id: expiring_item.id.clone(),
            session_id,
            run_id: None,
            status: "active".to_string(),
            wait_kind: "pipeline_execution".to_string(),
            subject_kind: "work_item".to_string(),
            subject_id: expiring_item.id.clone(),
            next_check_at: "0".to_string(),
            deadline_at: "0".to_string(),
            max_checks: 1,
            data_json: json!({ "controller_action": "wait_for_pipeline_execution" }),
        })
        .await
        .unwrap();

    let Json(expired) = reconcile_due_controller_waits(
        State(state.clone()),
        None,
        Json(ReconcileDueControllerWaitsRequest {
            limit: Some(10),
            actor: Some("controller".to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(expired.checked, 1);
    assert_eq!(expired.blocked, 1);
    assert_eq!(expired.results[0].outcome, "blocked");
    let stored_expiring_wait = state
        .store
        .get_controller_wait(&expiring_wait.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored_expiring_wait.status, "expired");
    let blocked_item = state
        .store
        .get_work_item(&expiring_item.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(blocked_item.status, "blocked");
    let audit_events = state
        .store
        .list_audit_events(Some("work_item"), Some(&expiring_item.id), None, 20)
        .await
        .unwrap();
    assert!(audit_events
        .iter()
        .any(|event| event.kind == "work_item.controller_wait_blocked"));
}

#[tokio::test]
async fn due_pipeline_wait_observes_only_the_declared_tekton_run_and_persists_terminal_evidence() {
    let fake_kubectl = fake_succeeded_tekton_kubectl_script();
    let state = test_state_with_cluster_tools(
        ReadOnlyClusterTools::default()
            .with_kubectl_bin(fake_kubectl.display().to_string())
            .without_related_resource_lookups(),
    )
    .await;
    let session_id = SessionId::new("ses_pipeline_wait_observer");
    let run_id = RunId::new("run_pipeline_wait_observer");
    let work_item_id = "witem_pipeline_wait_observer";
    let work_plan_id = "wplan_pipeline_wait_observer";
    let change_set_id = "cset_pipeline_wait_observer";
    let pipeline_intent_id = "pint_pipeline_wait_observer";
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: "Pipeline wait observer".to_string(),
            cwd: "/workspace".to_string(),
        })
        .await
        .unwrap();
    state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: "observe exact Tekton PipelineRun".to_string(),
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
            id: work_item_id.to_string(),
            status: "awaiting_approval".to_string(),
            title: "Observe a declared finance build".to_string(),
            intent: "Persist terminal Tekton evidence without redispatching work.".to_string(),
            acceptance_criteria: vec!["Exact PipelineRun is observed".to_string()],
            source_repo: "team/finance-api".to_string(),
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
            created_by: Some("operator".to_string()),
        })
        .await
        .unwrap();
    state
        .store
        .create_work_plan(CreateWorkPlan {
            id: work_plan_id.to_string(),
            work_item_id: Some(work_item_id.to_string()),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Observe finance build".to_string(),
            summary: "Bounded test lineage".to_string(),
            risk_level: "medium".to_string(),
            requires_approval: true,
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("finance-build".to_string()),
            work_plan_json: json!({}),
        })
        .await
        .unwrap();
    let change_set = state
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
            title: "Finance source change".to_string(),
            summary: "Reviewed source change".to_string(),
            risk_level: "medium".to_string(),
            material_hash: "pipeline_wait_observer_hash".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("finance-build".to_string()),
            change_set_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_pipeline_wait_git_plan".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "git_delivery_plan".to_string(),
            label: "merged finance source plan".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "change_set": {
                    "id": change_set.id,
                    "revision": change_set.revision,
                    "material_hash": change_set.material_hash,
                }
            })),
        })
        .await
        .unwrap();
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_pipeline_wait_git_merge".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "git_delivery_merge".to_string(),
            label: "merged finance source".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "git_delivery_plan_artifact_id": "art_pipeline_wait_git_plan",
                "merge_commit_sha": "b1b2c3d4e5f60718293a4b5c6d7e8f9012345678",
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
            status: "executing".to_string(),
            title: "Finance build".to_string(),
            summary: "Declared PipelineRun is active".to_string(),
            risk_level: "medium".to_string(),
            intent_kind: "tekton_build_test_package".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("finance-build".to_string()),
            intent_json: json!({
                "execution_state": {
                    "execution_id": "pexec_pipeline_wait_observer",
                    "state": "pipeline_run_created",
                    "pipeline_run_namespace": "ci",
                    "pipeline_run_name": "finance-build",
                }
            }),
        })
        .await
        .unwrap();
    let wait = state
        .store
        .create_controller_wait(CreateControllerWait {
            id: "cwait_pipeline_wait_observer".to_string(),
            work_item_id: work_item_id.to_string(),
            session_id,
            run_id: Some(run_id),
            status: "active".to_string(),
            wait_kind: "pipeline_execution".to_string(),
            subject_kind: "pipeline_intent".to_string(),
            subject_id: pipeline_intent_id.to_string(),
            next_check_at: "0".to_string(),
            deadline_at: "9999999999999999999999".to_string(),
            max_checks: 3,
            data_json: json!({ "controller_action": "wait_for_pipeline_execution" }),
        })
        .await
        .unwrap();

    let Json(result) = reconcile_due_controller_waits(
        State(state.clone()),
        None,
        Json(ReconcileDueControllerWaitsRequest {
            limit: Some(10),
            actor: Some("controller".to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(result.checked, 1);
    assert_eq!(result.progressed, 1);
    assert_eq!(result.results[0].outcome, "progressed");
    assert_eq!(
        result.results[0].next_action.as_deref(),
        Some("awaiting_pipeline_build_output_review")
    );
    assert_eq!(
        state
            .store
            .get_controller_wait(&wait.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "resolved"
    );
    let intent = state
        .store
        .get_pipeline_intent(pipeline_intent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(intent.status, "approved");
    assert_eq!(
        intent.intent_json.pointer("/execution_state/state"),
        Some(&json!("pipeline_run_succeeded"))
    );
    assert_eq!(
        intent.intent_json.pointer("/evidence/status"),
        Some(&json!("satisfied"))
    );
    let observations = state
        .store
        .list_observations(ObservationListFilter {
            source: Some("tekton".to_string()),
            resource_name: Some("finance-build".to_string()),
            limit: 10,
            ..ObservationListFilter::default()
        })
        .await
        .unwrap();
    assert!(observations
        .iter()
        .any(|observation| observation.kind == "pipeline_run_analysis"));
    let audits = state
        .store
        .list_audit_events(Some("pipeline_intent"), Some(pipeline_intent_id), None, 20)
        .await
        .unwrap();
    assert!(audits
        .iter()
        .any(|event| event.kind == "pipeline_intent.execution_observed"));
    let _ = fs::remove_file(fake_kubectl);
}

#[tokio::test]
async fn due_deployment_wait_observes_only_the_declared_argo_application() {
    let fake_kubectl = fake_completed_argo_wait_kubectl_script();
    let state = test_state_with_cluster_tools(
        ReadOnlyClusterTools::default().with_kubectl_bin(fake_kubectl.display().to_string()),
    )
    .await;
    let release_id = seed_approved_work_item_release(&state).await;
    let release = state.store.get_release(&release_id).await.unwrap().unwrap();
    let run_id = release.run_id.clone().unwrap();
    let deployment_intent_id = release.deployment_intent_id.clone();
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_argo_wait_execution".to_string(),
            session_id: release.session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "argo_sync_execution".to_string(),
            label: "declared Argo sync execution".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": "aexec_argo_wait",
                "deployment_intent_id": deployment_intent_id,
                "target": {
                    "environment": "dev",
                    "namespace": "apps-dev",
                    "argo_application": "checkout-api",
                }
            })),
        })
        .await
        .unwrap();
    let wait = state
        .store
        .create_controller_wait(CreateControllerWait {
            id: "cwait_argo_wait_observer".to_string(),
            work_item_id: "witem_post_sync".to_string(),
            session_id: release.session_id,
            run_id: Some(run_id.clone()),
            status: "active".to_string(),
            wait_kind: "deployment_execution".to_string(),
            subject_kind: "deployment_intent".to_string(),
            subject_id: deployment_intent_id.clone(),
            next_check_at: "0".to_string(),
            deadline_at: "9999999999999999999999".to_string(),
            max_checks: 3,
            data_json: json!({ "controller_action": "wait_for_deployment_execution" }),
        })
        .await
        .unwrap();

    observe_due_controller_wait(&state, &wait, Some("controller".to_string()))
        .await
        .unwrap();

    let artifacts = state.store.list_artifacts(&run_id).await.unwrap();
    let result = artifacts
        .iter()
        .find(|artifact| {
            artifact.kind == "argo_sync_result"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("execution_id").and_then(Value::as_str) == Some("aexec_argo_wait")
                        && content.get("status").and_then(Value::as_str) == Some("completed")
                })
        })
        .expect("exact Argo execution should receive a terminal result");
    assert_eq!(
        result.content_json.as_ref().unwrap()["details"]["operation_phase"],
        "Succeeded"
    );
    let audits = state
        .store
        .list_audit_events(
            Some("deployment_intent"),
            Some(&deployment_intent_id),
            None,
            20,
        )
        .await
        .unwrap();
    assert!(audits
        .iter()
        .any(|event| event.kind == "deployment_intent.execution_observed"));
    let _ = fs::remove_file(fake_kubectl);
}

#[tokio::test]
async fn work_item_replan_requires_remaining_budget_and_resumes_at_the_controller_boundary() {
    let state = test_state().await;
    let Json(work_item) = create_work_item(
        State(state.clone()),
        None,
        Json(CreateWorkItemRequest {
            title: "Retry a bounded finance fix".to_string(),
            intent: "Make one focused development-only source change.".to_string(),
            acceptance_criteria: vec!["Focused test passes".to_string()],
            source_repo: "team/finance-api".to_string(),
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
            argo_application: Some("finance-api".to_string()),
            workload_kind: None,
            workload_name: None,
            rollback_owner: None,
            production_impacting: false,
            max_attempts: Some(2),
            max_elapsed_seconds: Some(900),
            preflight_state_hash: None,
            environment_profile_id: None,
            initial_turn_budget: None,
            hard_turn_budget: None,
            initial_token_budget: None,
            hard_token_budget: None,
            active_execution_seconds: None,
            recoverable_tool_error_limit: None,
            identical_failure_limit: None,
            actor: Some("operator".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(declared) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item.id.clone()),
        Json(ReconcileWorkItemRequest {
            apply: true,
            actor: Some("operator".to_string()),
            reason: Some("declare bounded retry plan".to_string()),
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    let work_plan = declared.work_plan.unwrap();
    state
        .store
        .update_work_plan_status(
            &work_plan.id,
            "approved",
            Some("operator".to_string()),
            Some("approved before first coding attempt".to_string()),
        )
        .await
        .unwrap();

    let session_id = SessionId::new("ses_work_item_replan");
    let run_id = RunId::new("run_work_item_replan");
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: "replan test".to_string(),
            cwd: ".".to_string(),
        })
        .await
        .unwrap();
    state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id,
            user_task: "replan test".to_string(),
            cwd: ".".to_string(),
            max_turns: 1,
            initial_status: "failed".to_string(),
            execution_target_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .start_work_item_attempt(
            &work_item.id,
            &run_id,
            Some("operator".to_string()),
            Some("start first bounded attempt".to_string()),
        )
        .await
        .unwrap();
    let failed = state
        .store
        .finish_work_item_attempt(
            &work_item.id,
            "failed",
            Some("worker".to_string()),
            Some("focused test failed".to_string()),
        )
        .await
        .unwrap();
    assert_eq!(failed.attempt_count, 1);
    assert!(failed.current_run_id.is_none());

    let Json(replanned) = replan_work_item(
        State(state.clone()),
        None,
        Path(work_item.id.clone()),
        Json(ReplanWorkItemRequest {
            actor: Some("operator".to_string()),
            reason: "retry after inspecting the failed focused test".to_string(),
        }),
    )
    .await
    .unwrap();
    assert_eq!(replanned.work_item.status, "awaiting_approval");
    assert_eq!(replanned.work_item.attempt_count, 1);
    assert_eq!(replanned.attempts_remaining, 1);
    assert_eq!(replanned.work_plan.status, "approved");
    assert!(replanned.work_item.current_run_id.is_none());

    let Json(preview) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item.id.clone()),
        Json(ReconcileWorkItemRequest {
            apply: false,
            actor: Some("operator".to_string()),
            reason: None,
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(preview.action, "start_coding_attempt");

    let events = state
        .store
        .list_audit_events(Some("work_item"), Some(&work_item.id), None, 20)
        .await
        .unwrap();
    let replan = events
        .iter()
        .find(|event| event.kind == "work_item.replanned")
        .expect("replan is durable and auditable");
    assert_eq!(replan.payload_json["extra"]["previous_status"], "failed");
    assert_eq!(replan.payload_json["extra"]["attempts_remaining"], 1);

    state
        .store
        .start_work_item_attempt(
            &work_item.id,
            &run_id,
            Some("operator".to_string()),
            Some("start final bounded attempt".to_string()),
        )
        .await
        .unwrap();
    state
        .store
        .finish_work_item_attempt(
            &work_item.id,
            "blocked",
            Some("worker".to_string()),
            Some("second attempt blocked".to_string()),
        )
        .await
        .unwrap();
    let error = replan_work_item(
        State(state),
        None,
        Path(work_item.id),
        Json(ReplanWorkItemRequest {
            actor: Some("operator".to_string()),
            reason: "must not exceed the attempt budget".to_string(),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(error.status, StatusCode::CONFLICT);
    assert!(error.message.contains("attempt budget is exhausted"));
}

#[tokio::test]
async fn work_item_replan_refuses_to_bypass_a_captured_change_set() {
    let state = test_state().await;
    let Json(work_item) = create_work_item(
        State(state.clone()),
        None,
        Json(CreateWorkItemRequest {
            title: "Review captured finance change".to_string(),
            intent: "Keep source review mandatory before another attempt.".to_string(),
            acceptance_criteria: Vec::new(),
            source_repo: "team/finance-api".to_string(),
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
            argo_application: Some("finance-api".to_string()),
            workload_kind: None,
            workload_name: None,
            rollback_owner: None,
            production_impacting: false,
            max_attempts: Some(2),
            max_elapsed_seconds: Some(900),
            preflight_state_hash: None,
            environment_profile_id: None,
            initial_turn_budget: None,
            hard_turn_budget: None,
            initial_token_budget: None,
            hard_token_budget: None,
            active_execution_seconds: None,
            recoverable_tool_error_limit: None,
            identical_failure_limit: None,
            actor: Some("operator".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(declared) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item.id.clone()),
        Json(ReconcileWorkItemRequest {
            apply: true,
            actor: Some("operator".to_string()),
            reason: Some("declare source review boundary".to_string()),
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    let work_plan = declared.work_plan.unwrap();
    state
        .store
        .update_work_plan_status(
            &work_plan.id,
            "approved",
            Some("operator".to_string()),
            Some("approved source plan".to_string()),
        )
        .await
        .unwrap();
    let stored_work_plan = state
        .store
        .get_work_plan(&work_plan.id)
        .await
        .unwrap()
        .unwrap();
    state
        .store
        .create_change_set(CreateChangeSet {
            id: "cset_replan_guard".to_string(),
            work_item_id: Some(work_item.id.clone()),
            work_plan_id: work_plan.id.clone(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: stored_work_plan.session_id,
            run_id: None,
            status: "proposed".to_string(),
            title: "captured source change".to_string(),
            summary: "requires review".to_string(),
            risk_level: "medium".to_string(),
            material_hash: "test-material-hash".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("application".to_string()),
            resource_name: Some("finance-api".to_string()),
            change_set_json: json!({"source": {"kind": "workspace_git"}}),
        })
        .await
        .unwrap();
    state
        .store
        .update_work_item_status(
            &work_item.id,
            "blocked",
            Some("worker".to_string()),
            Some("needs source review".to_string()),
        )
        .await
        .unwrap();

    let error = replan_work_item(
        State(state),
        None,
        Path(work_item.id),
        Json(ReplanWorkItemRequest {
            actor: Some("operator".to_string()),
            reason: "must not bypass a captured source review".to_string(),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(error.status, StatusCode::CONFLICT);
    assert!(error.message.contains("already has a ChangeSet"));
}

#[tokio::test]
async fn work_item_planning_declares_a_work_plan_and_ephemeral_workspace() {
    let state = test_state().await;
    let Json(work_item) = create_work_item(
        State(state.clone()),
        None,
        Json(CreateWorkItemRequest {
            title: "Add a finance smoke endpoint".to_string(),
            intent: "Expose a read-only health endpoint with a focused test.".to_string(),
            acceptance_criteria: vec!["Endpoint returns a stable response".to_string()],
            source_repo: "team/finance-api".to_string(),
            source_ref: "main".to_string(),
            source_commit: None,
            pipeline_contract_id: None,
            deployment_contract_id: None,
            gitops_repo: Some("team/finance-gitops".to_string()),
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
            max_attempts: Some(2),
            max_elapsed_seconds: Some(900),
            preflight_state_hash: None,
            environment_profile_id: None,
            initial_turn_budget: None,
            hard_turn_budget: None,
            initial_token_budget: None,
            hard_token_budget: None,
            active_execution_seconds: None,
            recoverable_tool_error_limit: None,
            identical_failure_limit: None,
            actor: Some("operator".to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(work_item.status, "submitted");

    let Json(planning) = transition_work_item(
        State(state.clone()),
        None,
        Path(work_item.id.clone()),
        Json(TransitionWorkItemRequest {
            target_status: "planning".to_string(),
            actor: Some("operator".to_string()),
            reason: Some("reviewed delivery intent".to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(planning.status, "planning");

    let Json(created_plan) =
        create_work_plan_from_work_item(State(state.clone()), None, Path(work_item.id.clone()))
            .await
            .unwrap();
    assert!(created_plan.created);
    assert_eq!(
        created_plan.work_plan.work_item_id.as_deref(),
        Some(work_item.id.as_str())
    );
    assert_eq!(created_plan.work_plan.remediation_plan_id, None);
    assert_eq!(
        created_plan.work_plan.work_plan_json["execution"]["enabled"],
        false
    );
    let gates = state
        .store
        .list_approval_gates(ApprovalGateListFilter {
            work_item_id: Some(work_item.id.clone()),
            limit: 10,
            ..ApprovalGateListFilter::default()
        })
        .await
        .unwrap();
    assert_eq!(gates.len(), 5);
    assert!(gates.iter().all(|gate| gate.status == "pending"));
    let git_gate = gates
        .iter()
        .find(|gate| gate.gate_kind == "git_mutation")
        .expect("WorkItem gate set should include Git delivery");
    assert_eq!(
        git_gate.gate_json.pointer("/scope/source_repository"),
        Some(&json!(work_item.source_repo))
    );
    assert_eq!(
        git_gate.gate_json.pointer("/scope/actions"),
        Some(&json!(GIT_DELIVERY_ACTIONS))
    );
    let gitops_gate = gates
        .iter()
        .find(|gate| gate.gate_kind == "gitops_mutation")
        .expect("WorkItem gate set should include GitOps delivery");
    assert_eq!(
        gitops_gate.gate_json.pointer("/scope/gitops_repository"),
        Some(&json!(work_item.gitops_repo))
    );
    let summary = state
        .store
        .approval_gate_summary(ApprovalGateSummaryFilter {
            work_item_id: Some(work_item.id.clone()),
            status: Some("pending".to_string()),
            ..ApprovalGateSummaryFilter::default()
        })
        .await
        .unwrap();
    assert_eq!(summary.total, 5);
    assert_eq!(summary.by_work_item_id.len(), 1);
    assert_eq!(
        summary.by_work_item_id[0].value.as_deref(),
        Some(work_item.id.as_str())
    );
    state
        .store
        .decide_approval_gate(
            &gates[0].id,
            "satisfied",
            Some("operator".to_string()),
            Some("reviewed alpha delivery boundary".to_string()),
        )
        .await
        .unwrap();
    let mut revised_plan_json = created_plan.work_plan.work_plan_json.clone();
    revised_plan_json["review_note"] = json!("materially refined acceptance evidence");
    let Json(revised_plan) = revise_work_plan(
        State(state.clone()),
        Path(created_plan.work_plan.id.clone()),
        Json(ReviseWorkPlanRequest {
            title: None,
            summary: None,
            risk_level: None,
            requires_approval: None,
            work_plan_json: revised_plan_json,
            actor: Some("operator".to_string()),
            reason: Some("acceptance criteria refined".to_string()),
            material_change: true,
        }),
    )
    .await
    .unwrap();
    assert_eq!(revised_plan.invalidated_gates.len(), 1);
    assert_eq!(revised_plan.invalidated_gates[0].id, gates[0].id);
    assert_eq!(revised_plan.invalidated_gates[0].status, "stale");

    let Json(workspaces) = list_workspaces(
        State(state.clone()),
        Query(ListWorkspacesQuery {
            work_item_id: Some(work_item.id.clone()),
            limit: Some(10),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    assert_eq!(workspaces.count, 1);
    assert_eq!(workspaces.workspaces[0].status, "declared");
    assert_eq!(workspaces.workspaces[0].retention_status, "ephemeral");

    let error = create_change_set(
        State(state.clone()),
        Json(CreateChangeSetRequest {
            work_plan_id: created_plan.work_plan.id,
            title: None,
            summary: None,
            risk_level: None,
            change_set_json: json!({"files": []}),
            actor: Some("operator".to_string()),
            reason: Some("must not create synthetic change set".to_string()),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(error.status, StatusCode::CONFLICT);
    assert!(error.message.contains("workspace Git diff provenance"));

    let Json(cancelled) = cancel_work_item(
        State(state.clone()),
        None,
        Path(work_item.id.clone()),
        Json(TransitionWorkItemRequest {
            target_status: "cancelled".to_string(),
            actor: Some("operator".to_string()),
            reason: Some("alpha complete".to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(cancelled.status, "cancelled");

    let Json(events) = list_work_item_events(State(state), Path(work_item.id))
        .await
        .unwrap();
    assert!(events
        .events
        .iter()
        .any(|event| event.kind == "work_item.work_plan_created"));
    assert!(events
        .events
        .iter()
        .any(|event| event.kind == "work_item.cancelled"));
}

#[tokio::test]
async fn worker_can_pin_the_exact_issued_remote_workspace_while_preparing() {
    let state = AppState {
        store: Arc::new(SqliteStore::connect_in_memory().await.unwrap()),
        worker: RunDispatcher::Disabled,
        cluster_tools: ReadOnlyClusterTools::default(),
        policy: SafetyPolicy::default(),
        worker_token: None,
        operator_tokens: Arc::new(Vec::new()),
        workspace: WorkspaceProvisioner::with_remote_repos(
            std::env::temp_dir(),
            Vec::new(),
            vec!["https://github.com/example/finance-app.git".to_string()],
        ),
        build: BuildMetadata::from_env(),
        protected_target: ProtectedTargetConfiguration::from_env(),
        environment_profiles: Arc::new(Vec::new()),
        repo_mode: RepoModeConfiguration::test_enabled(),
    };
    let work_item = state
        .store
        .create_work_item(CreateWorkItem {
            id: "witem_remote_pin".to_string(),
            status: "executing".to_string(),
            title: "Remote source pin".to_string(),
            intent: "Change a disposable finance app.".to_string(),
            acceptance_criteria: vec!["A focused test passes.".to_string()],
            source_repo: "https://github.com/example/finance-app.git".to_string(),
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
            created_by: Some("operator".to_string()),
        })
        .await
        .unwrap();
    let session_id = SessionId::new("ses_remote_pin");
    let run_id = RunId::new("run_remote_pin");
    let workspace_id = "ws_remote_pin";
    let branch = "pharness/witem_remote_pin/attempt-1";
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: "remote pin".to_string(),
            cwd: "/workspace".to_string(),
        })
        .await
        .unwrap();
    state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id,
            user_task: "remote pin".to_string(),
            cwd: "/workspace".to_string(),
            max_turns: 2,
            initial_status: "queued".to_string(),
            execution_target_json: json!({
                "kind": "kubernetes_workspace",
                "run_scope": {
                    "work_item_id": work_item.id,
                    "workspace_id": workspace_id,
                    "production_impacting": false
                },
                "workspace_source": {
                    "workspace_id": workspace_id,
                    "source_repo": "https://github.com/example/finance-app.git",
                    "source_ref": "main",
                    "branch": branch
                }
            }),
        })
        .await
        .unwrap();
    state
        .store
        .create_workspace(CreateWorkspace {
            id: workspace_id.to_string(),
            work_item_id: work_item.id.clone(),
            run_id: Some(run_id.clone()),
            // Repo Mode holds a fresh immutable-runner workspace in
            // `preparing` while its dedicated preparation Job checks out the
            // issued source contract. The worker's provisioning callback must
            // accept that state as well as the legacy `provisioning` state.
            status: "preparing".to_string(),
            source_repo: "https://github.com/example/finance-app.git".to_string(),
            source_ref: "main".to_string(),
            resolved_commit: None,
            branch: Some(branch.to_string()),
            retention_status: "ephemeral".to_string(),
            actor: Some("operator".to_string()),
            reason: Some("test".to_string()),
        })
        .await
        .unwrap();

    let mismatch = internal_workspace_provisioned(
        State(state.clone()),
        Path(run_id.to_string()),
        Json(InternalWorkspaceProvisionedRequest {
            workspace_id: workspace_id.to_string(),
            resolved_commit: "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678".to_string(),
            branch: "pharness/witem_remote_pin/attempt-2".to_string(),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(mismatch.status, StatusCode::CONFLICT);

    let Json(pinned) = internal_workspace_provisioned(
        State(state.clone()),
        Path(run_id.to_string()),
        Json(InternalWorkspaceProvisionedRequest {
            workspace_id: workspace_id.to_string(),
            resolved_commit: "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678".to_string(),
            branch: branch.to_string(),
        }),
    )
    .await
    .unwrap();
    assert_eq!(pinned.status, "executing");
    assert_eq!(
        pinned.resolved_commit.as_deref(),
        Some("a1b2c3d4e5f60718293a4b5c6d7e8f9012345678")
    );

    let audit = state
        .store
        .list_audit_events(Some("workspace"), Some(workspace_id), None, 10)
        .await
        .unwrap();
    assert!(audit
        .iter()
        .any(|event| event.kind == "workspace.provisioned"));
}

#[tokio::test]
async fn approved_workspace_change_set_prepares_an_idempotent_git_delivery_plan() {
    let state = test_state().await;
    let work_item_id = "witem_git_delivery";
    let work_plan_id = "wplan_git_delivery";
    let change_set_id = "cset_git_delivery";
    let run_id = RunId::new("run_git_delivery");
    let session_id = SessionId::new("ses_git_delivery");
    let diff = "diff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-before\n+after\n";
    let diff_sha256 = format!("{:x}", Sha256::digest(diff.as_bytes()));

    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: "Git delivery".to_string(),
            cwd: "/workspace".to_string(),
        })
        .await
        .unwrap();
    state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: "Git delivery".to_string(),
            cwd: "/workspace".to_string(),
            max_turns: 4,
            initial_status: "completed".to_string(),
            execution_target_json: json!({
                "run_scope": {
                    "work_item_id": work_item_id,
                    "workspace_id": "ws_git_delivery",
                    "repo": "https://github.com/example/finance-app.git",
                    "branch": "pharness/witem_git_delivery/attempt-1",
                    "production_impacting": false
                }
            }),
        })
        .await
        .unwrap();
    let work_item = state
        .store
        .create_work_item(CreateWorkItem {
            id: work_item_id.to_string(),
            status: "awaiting_approval".to_string(),
            title: "Finance documentation change".to_string(),
            intent: "Add a small finance documentation note".to_string(),
            acceptance_criteria: vec!["README is updated".to_string()],
            source_repo: "https://github.com/example/finance-app.git".to_string(),
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
        .create_workspace(CreateWorkspace {
            id: "ws_git_delivery".to_string(),
            work_item_id: work_item_id.to_string(),
            run_id: Some(run_id.clone()),
            status: "captured".to_string(),
            source_repo: "https://github.com/example/finance-app.git".to_string(),
            source_ref: "main".to_string(),
            resolved_commit: Some("a1b2c3d4e5f60718293a4b5c6d7e8f9012345678".to_string()),
            branch: Some("pharness/witem_git_delivery/attempt-1".to_string()),
            retention_status: "ephemeral".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("captured source diff".to_string()),
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
            title: "Finance documentation plan".to_string(),
            summary: "Update the finance README".to_string(),
            risk_level: "medium".to_string(),
            requires_approval: true,
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: None,
            resource_name: None,
            work_plan_json: json!({}),
        })
        .await
        .unwrap();
    for gate in approval_gates_from_work_item(&work_item, &work_plan) {
        state.store.create_approval_gate(gate).await.unwrap();
    }
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_git_delivery_diff".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "workspace_git_diff".to_string(),
            label: "Workspace diff".to_string(),
            mime_type: Some("text/x-diff".to_string()),
            path: None,
            content_text: Some(diff.to_string()),
            content_json: None,
        })
        .await
        .unwrap();
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_git_delivery_status".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "workspace_git_status".to_string(),
            label: "Workspace status".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "status": " M README.md",
                "changed_paths": ["README.md"],
                "test_events": []
            })),
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
            session_id,
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "ChangeSet: finance docs".to_string(),
            summary: "Add a concise finance documentation note".to_string(),
            risk_level: "medium".to_string(),
            material_hash: "material_git_delivery".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: None,
            resource_name: None,
            change_set_json: json!({
                "source": {
                    "kind": "workspace_git",
                    "workspace_id": "ws_git_delivery",
                    "base_commit": "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678",
                    "branch": "pharness/witem_git_delivery/attempt-1"
                },
                "evidence": {
                    "git_diff_artifact_id": "art_git_delivery_diff",
                    "git_status_artifact_id": "art_git_delivery_status",
                    "diff_sha256": diff_sha256,
                    "changed_paths": ["README.md"]
                }
            }),
        })
        .await
        .unwrap();

    let Json(captured_flow) = work_item_flow(State(state.clone()), Path(work_item_id.to_string()))
        .await
        .unwrap();
    assert!(!captured_flow
        .action_rail
        .iter()
        .any(|action| action.id == "authorize_workspace_and_start"));
    assert!(captured_flow.action_rail.iter().any(|action| {
        action.id == WorkItemReconcileAction::PrepareGitDelivery.as_str()
            && action.status == "ready"
    }));

    let missing_plan = authorize_change_set_git_delivery(
        State(state.clone()),
        None,
        Path(change_set_id.to_string()),
        Json(CreateGitDeliveryAuthorizationRequest {
            subject: None,
            created_by: Some("lucas".to_string()),
            reason: "must not authorize an unprepared delivery".to_string(),
            expires_at: None,
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(missing_plan.status, StatusCode::CONFLICT);

    let Json(first) = prepare_change_set_git_delivery(
        State(state.clone()),
        Path(change_set_id.to_string()),
        Json(PrepareGitDeliveryRequest {
            actor: Some("lucas".to_string()),
            reason: Some("prepare source review delivery".to_string()),
        }),
    )
    .await
    .unwrap();
    assert!(first.created);
    assert_eq!(
        first.artifact.content_json.as_ref().unwrap()["source"]["head_branch"],
        "pharness/witem_git_delivery/attempt-1"
    );
    assert_eq!(
        first.artifact.content_json.as_ref().unwrap()["authorization"]["state"],
        "not_authorized"
    );

    let Json(second) = prepare_change_set_git_delivery(
        State(state.clone()),
        Path(change_set_id.to_string()),
        Json(PrepareGitDeliveryRequest {
            actor: Some("lucas".to_string()),
            reason: Some("repeat request".to_string()),
        }),
    )
    .await
    .unwrap();
    assert!(!second.created);
    assert_eq!(second.artifact.id, first.artifact.id);

    let Json(blocked_preflight) = preflight_change_set_git_delivery(
        State(state.clone()),
        None,
        Path(change_set_id.to_string()),
        Json(GitDeliveryPreflightRequest {
            subject: None,
            actor: Some("lucas".to_string()),
            reason: Some("record missing Git writer authorization".to_string()),
        }),
    )
    .await
    .unwrap();
    assert!(blocked_preflight.created);
    assert_eq!(blocked_preflight.status, "blocked");
    assert!(!blocked_preflight.authorization_ready);
    assert!(!blocked_preflight.dispatch_ready);
    assert!(blocked_preflight.permission_grant.is_none());
    assert!(blocked_preflight.checks.iter().any(|check| {
        check["code"] == "trusted_git_delivery_grant" && check["passed"] == false
    }));
    assert!(blocked_preflight.checks.iter().any(|check| {
        check["code"] == "work_item_git_mutation_gate" && check["passed"] == false
    }));

    let Json(authorization) = authorize_change_set_git_delivery(
        State(state.clone()),
        None,
        Path(change_set_id.to_string()),
        Json(CreateGitDeliveryAuthorizationRequest {
            subject: None,
            created_by: Some("lucas".to_string()),
            reason: "authorize one reviewed Git delivery".to_string(),
            expires_at: Some((current_millis() + 60_000).to_string()),
        }),
    )
    .await
    .unwrap();
    assert!(authorization.created);
    assert_eq!(authorization.grant.subject, "agent:git-writer");
    assert_eq!(
        authorization.grant.policy["policy_mode"],
        "supervised_autonomy"
    );
    assert_eq!(
        authorization.grant.scope["capability_kinds"],
        json!(["git"])
    );
    assert_eq!(
        authorization.grant.scope["git_delivery_plan_artifact_ids"],
        json!([first.artifact.id])
    );

    let Json(authorized_but_gated_preflight) = preflight_change_set_git_delivery(
        State(state.clone()),
        None,
        Path(change_set_id.to_string()),
        Json(GitDeliveryPreflightRequest {
            subject: None,
            actor: Some("lucas".to_string()),
            reason: Some("prove grant cannot bypass the WorkItem gate".to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(authorized_but_gated_preflight.status, "blocked");
    assert!(authorized_but_gated_preflight.authorization_ready);
    assert!(!authorized_but_gated_preflight.approval_gate_ready);

    let git_gate = state
        .store
        .list_approval_gates(ApprovalGateListFilter {
            work_item_id: Some(work_item_id.to_string()),
            gate_kind: Some("git_mutation".to_string()),
            limit: 1,
            ..ApprovalGateListFilter::default()
        })
        .await
        .unwrap()
        .pop()
        .expect("WorkItem Git gate should exist");
    state
        .store
        .decide_approval_gate(
            &git_gate.id,
            "satisfied",
            Some("lucas".to_string()),
            Some("reviewed immutable source delivery".to_string()),
        )
        .await
        .unwrap();

    let Json(repeated_authorization) = authorize_change_set_git_delivery(
        State(state.clone()),
        None,
        Path(change_set_id.to_string()),
        Json(CreateGitDeliveryAuthorizationRequest {
            subject: None,
            created_by: Some("lucas".to_string()),
            reason: "repeat authorization".to_string(),
            expires_at: None,
        }),
    )
    .await
    .unwrap();
    assert!(!repeated_authorization.created);
    assert_eq!(repeated_authorization.grant.id, authorization.grant.id);

    let Json(ready_preflight) = preflight_change_set_git_delivery(
        State(state.clone()),
        None,
        Path(change_set_id.to_string()),
        Json(GitDeliveryPreflightRequest {
            subject: None,
            actor: Some("lucas".to_string()),
            reason: Some("record authorized Git delivery readiness".to_string()),
        }),
    )
    .await
    .unwrap();
    assert!(ready_preflight.created);
    assert_eq!(ready_preflight.status, "ready_for_writer");
    assert!(ready_preflight.approval_gate_ready);
    assert!(ready_preflight.authorization_ready);
    assert!(!ready_preflight.dispatch_ready);
    assert_eq!(
        ready_preflight
            .permission_grant
            .as_ref()
            .map(|grant| grant.id.as_str()),
        Some(authorization.grant.id.as_str())
    );
    assert!(ready_preflight.checks.iter().any(|check| {
        check["code"] == "git_writer_executor_available" && check["passed"] == false
    }));

    let Json(repeated_preflight) = preflight_change_set_git_delivery(
        State(state.clone()),
        None,
        Path(change_set_id.to_string()),
        Json(GitDeliveryPreflightRequest {
            subject: None,
            actor: Some("lucas".to_string()),
            reason: Some("repeat authorized Git delivery readiness".to_string()),
        }),
    )
    .await
    .unwrap();
    assert!(!repeated_preflight.created);
    assert_eq!(repeated_preflight.artifact.id, ready_preflight.artifact.id);

    let Json(reconciled) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.to_string()),
        Json(ReconcileWorkItemRequest {
            apply: true,
            actor: Some("lucas".to_string()),
            reason: Some("advance approved source delivery".to_string()),
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert!(!reconciled.applied);
    assert_eq!(reconciled.action, "awaiting_git_writer_availability");
    assert!(reconciled
        .git_delivery_preflight
        .as_ref()
        .is_some_and(|preflight| preflight.authorization_ready));

    let Json(flow) = change_set_flow(State(state.clone()), Path(change_set_id.to_string()))
        .await
        .unwrap();
    let git_delivery = flow
        .git_delivery
        .expect("Git delivery flow should be present");
    assert_eq!(git_delivery.plan.id, first.artifact.id);
    assert_eq!(
        git_delivery
            .latest_preflight
            .as_ref()
            .map(|artifact| artifact.id.as_str()),
        Some(ready_preflight.artifact.id.as_str())
    );

    state
        .store
        .create_pipeline_contract(CreatePipelineContract {
            id: "pcontract_finance_ci".to_string(),
            status: "active".to_string(),
            namespace: "ci".to_string(),
            pipeline_ref: "finance-ci".to_string(),
            version: "v1".to_string(),
            contract_json: json!({
                "source_revision_param": "source-revision",
                "params": [{ "name": "source-revision", "type": "scalar", "required": true }]
            }),
            actor: Some("lucas".to_string()),
            reason: Some("finance source pipeline contract".to_string()),
        })
        .await
        .unwrap();

    let missing_merge = create_work_item_pipeline_intent(
        State(state.clone()),
        None,
        Path(work_item_id.to_string()),
        Json(CreateWorkItemPipelineIntentRequest {
            pipeline_contract_id: "pcontract_finance_ci".to_string(),
            title: None,
            summary: None,
            risk_level: None,
            intent_kind: None,
            intent_json: Some(json!({ "execution": { "enabled": false } })),
            actor: Some("lucas".to_string()),
            reason: Some("must not build from a mutable branch".to_string()),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(missing_merge.status, StatusCode::CONFLICT);
    assert!(missing_merge
        .message
        .contains("observed GitHub merge evidence"));

    let stored_change_set = state
        .store
        .get_change_set(change_set_id)
        .await
        .unwrap()
        .expect("ChangeSet should remain available for merge provenance");
    let merge_sha = "b1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_git_delivery_merge".to_string(),
            session_id: stored_change_set.session_id.clone(),
            run_id: stored_change_set.run_id.clone(),
            kind: "git_delivery_merge".to_string(),
            label: "Immutable merged finance source".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "change_set_id": change_set_id,
                "git_delivery_plan_artifact_id": first.artifact.id,
                "pull_request_url": "https://github.com/example/finance-app/pull/7",
                "pull_request_number": 7,
                "head_commit_sha": "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678",
                "merge_commit_sha": merge_sha,
            })),
        })
        .await
        .unwrap();

    let Json(pipeline_intent) = create_work_item_pipeline_intent(
        State(state.clone()),
        None,
        Path(work_item_id.to_string()),
        Json(CreateWorkItemPipelineIntentRequest {
            pipeline_contract_id: "pcontract_finance_ci".to_string(),
            title: Some("Finance build from reviewed source".to_string()),
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
            reason: Some("define the reviewed finance build".to_string()),
        }),
    )
    .await
    .unwrap();
    assert!(pipeline_intent.created);
    assert_eq!(
        pipeline_intent.pipeline_intent.intent_json["source_provenance"]["merge_commit_sha"],
        merge_sha
    );
    assert_eq!(
        pipeline_intent.pipeline_intent.intent_json["source_provenance"]
            ["git_delivery_merge_artifact_id"],
        "art_git_delivery_merge"
    );
    assert_eq!(
        pipeline_intent.pipeline_intent.intent_json["pipeline_contract"]["id"],
        "pcontract_finance_ci"
    );
    assert_eq!(
        pipeline_intent.pipeline_intent.intent_json["pipeline_contract"]["version"],
        "v1"
    );
    let pinned_contract_preflight =
        pipeline_intent_execution_preflight(&state, &pipeline_intent.pipeline_intent.id)
            .await
            .unwrap();
    assert!(pinned_contract_preflight
        .checks
        .iter()
        .any(|check| { check["code"] == "active_pipeline_contract" && check["passed"] == true }));
    assert!(pinned_contract_preflight
        .checks
        .iter()
        .any(|check| { check["code"] == "pipeline_contract_inputs" && check["passed"] == true }));

    let Json(proposed_pipeline_reconcile) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.to_string()),
        Json(ReconcileWorkItemRequest {
            apply: false,
            actor: Some("lucas".to_string()),
            reason: Some("show durable pipeline approval handoff".to_string()),
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        proposed_pipeline_reconcile.action,
        "awaiting_pipeline_intent_approval"
    );
    assert_eq!(
        proposed_pipeline_reconcile
            .pipeline_intent
            .as_ref()
            .map(|intent| intent.id.as_str()),
        Some(pipeline_intent.pipeline_intent.id.as_str())
    );
    assert!(proposed_pipeline_reconcile
        .pipeline_execution_preflight
        .is_none());

    state
        .store
        .update_pipeline_intent_status(
            &pipeline_intent.pipeline_intent.id,
            "approved",
            Some("lucas".to_string()),
            Some("approve exact Tekton definition".to_string()),
        )
        .await
        .unwrap();
    let Json(approved_pipeline_reconcile) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.to_string()),
        Json(ReconcileWorkItemRequest {
            apply: false,
            actor: Some("lucas".to_string()),
            reason: Some("show bounded Tekton authorization handoff".to_string()),
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        approved_pipeline_reconcile.action,
        "awaiting_pipeline_execution_authorization"
    );
    assert!(approved_pipeline_reconcile
        .pipeline_execution_preflight
        .as_ref()
        .is_some_and(|preflight| !preflight.ready));

    let Json(existing_pipeline_intent) = create_work_item_pipeline_intent(
        State(state.clone()),
        None,
        Path(work_item_id.to_string()),
        Json(CreateWorkItemPipelineIntentRequest {
            pipeline_contract_id: "pcontract_finance_ci".to_string(),
            title: Some("ignored duplicate".to_string()),
            summary: None,
            risk_level: None,
            intent_kind: None,
            intent_json: Some(json!({
                "execution": {
                    "enabled": true,
                    "namespace": "ci",
                    "pipeline_ref": "finance-ci",
                    "params": { "source-revision": merge_sha }
                }
            })),
            actor: Some("lucas".to_string()),
            reason: Some("repeat controller request".to_string()),
        }),
    )
    .await
    .unwrap();
    assert!(!existing_pipeline_intent.created);
    assert_eq!(
        existing_pipeline_intent.pipeline_intent.id,
        pipeline_intent.pipeline_intent.id
    );

    state
        .store
        .create_pipeline_contract(CreatePipelineContract {
            id: "pcontract_unrelated".to_string(),
            status: "active".to_string(),
            namespace: "other-ci".to_string(),
            pipeline_ref: "unrelated".to_string(),
            version: "v1".to_string(),
            contract_json: json!({}),
            actor: Some("lucas".to_string()),
            reason: Some("unrelated contract should be filtered".to_string()),
        })
        .await
        .unwrap();
    let Json(context) = work_item_pipeline_intent_context(
        State(state.clone()),
        Path(work_item_id.to_string()),
        Query(WorkItemPipelineContextQuery {
            namespace: Some("ci".to_string()),
            pipeline_ref: Some("finance-ci".to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(context.work_item.id, work_item_id);
    assert_eq!(context.change_set.id, change_set_id);
    assert_eq!(context.source_provenance["merge_commit_sha"], merge_sha);
    assert_eq!(
        context
            .pipeline_intent
            .as_ref()
            .map(|intent| intent.id.as_str()),
        Some(pipeline_intent.pipeline_intent.id.as_str())
    );
    assert_eq!(context.contract_namespace.as_deref(), Some("ci"));
    assert_eq!(context.contract_pipeline_ref.as_deref(), Some("finance-ci"));
    assert_eq!(context.active_pipeline_contracts.len(), 1);
    assert_eq!(
        context.active_pipeline_contracts[0].id,
        "pcontract_finance_ci"
    );

    state
        .store
        .update_pipeline_contract_status(
            "pcontract_finance_ci",
            "retired",
            Some("lucas".to_string()),
            Some("prove pinned contract retirement blocks execution".to_string()),
        )
        .await
        .unwrap();
    let retired_contract_preflight =
        pipeline_intent_execution_preflight(&state, &pipeline_intent.pipeline_intent.id)
            .await
            .unwrap();
    assert!(retired_contract_preflight.checks.iter().any(|check| {
        check["code"] == "active_pipeline_contract"
            && check["passed"] == false
            && check["summary"]
                .as_str()
                .is_some_and(|message| message.contains("Pinned PipelineContract"))
    }));

    let audit = state
        .store
        .list_audit_events(Some("change_set"), Some(change_set_id), None, 10)
        .await
        .unwrap();
    assert!(audit
        .iter()
        .any(|event| event.kind == "change_set.git_delivery_prepared"));
    assert!(audit
        .iter()
        .any(|event| event.kind == "change_set.git_delivery_authorized"));
    assert!(audit
        .iter()
        .any(|event| event.kind == "change_set.git_delivery_preflighted"));
    let work_item_audit = state
        .store
        .list_audit_events(Some("work_item"), Some(work_item_id), None, 20)
        .await
        .unwrap();
    assert!(work_item_audit
        .iter()
        .any(|event| event.kind == "work_item.pipeline_intent_proposed"));
}
