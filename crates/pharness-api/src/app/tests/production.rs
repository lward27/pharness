use super::{
    action_effect, approval_gate_lifecycle_readiness,
    approval_gate_uses_dedicated_lifecycle_action, approval_gates_from_work_item,
    approve_rollback_intent, authorize_gitops_change_set_delivery, batch_decide_approval_gates,
    block_work_item_from_delivery_failure, bounded_production_grant_expiry,
    complete_work_item_from_verified_release, current_millis, deployment_intent_reconcile_action,
    deployment_intent_requires_execution_preflight, execute_work_item_action,
    get_run_operator_summary, git_delivery_reconcile_action, gitops_change_set_reconcile_action,
    gitops_delivery_flow, immutable_git_object_id, immutable_image_digest,
    internal_gitops_delivery_observation_outcome, internal_rollback_argo_sync_outcome,
    internal_rollback_delivery_context, internal_rollback_delivery_observation_outcome,
    internal_rollback_delivery_outcome, json, latest_rollback_intent, list_approval_gates,
    observed_gitops_merge_for_deployment, pipeline_intent_is_gitops_update_eligible,
    pipeline_intent_reconcile_action, preflight_gitops_change_set_delivery,
    preflight_rollback_intent, prepare_gitops_change_set_delivery, protected_target_json,
    release_reconcile_action, request_matches_protected_target, satisfy_approval_gate,
    work_item_flow, AgentEvent, AppState, ApprovalGateListFilter, ArgoSyncOutcomeRequest,
    CreateApprovalGate, CreateArtifact, CreateChangeSet, CreateDeploymentIntent, CreateFileChange,
    CreateGitOpsChangeSet, CreateGitOpsDeliveryAuthorizationRequest, CreatePipelineIntent,
    CreateRelease, CreateRun, CreateSession, CreateWorkItem, CreateWorkItemRequest, CreateWorkPlan,
    CreateWorkspace, DecideApprovalGateRequest, Digest, EventId, EventKind,
    ExecuteWorkItemActionRequest, GitOpsBaseRevisionReconcileState,
    GitOpsDeliveryObservationOutcomeRequest, GitOpsDeliveryOutcomeRequest,
    GitOpsDeliveryPreflightRequest, Json, ListApprovalGatesQuery, Path,
    PrepareGitOpsDeliveryRequest, Query, RollbackIntentRequest, RunId, SessionId, Sha256, State,
    StatusCode, Value, WorkItemReconcileAction, PROTECTED_ARGO_APPLICATION, PROTECTED_ENVIRONMENT,
    PROTECTED_GITOPS_REPO, PROTECTED_IMAGE_NAME, PROTECTED_KUSTOMIZATION_PATH, PROTECTED_NAMESPACE,
    PROTECTED_ROLLBACK_OWNER, PROTECTED_SOURCE_REPO, PROTECTED_WORKLOAD_KIND,
    PROTECTED_WORKLOAD_NAME,
};

use super::characterization::{test_state, test_state_with_git_observer};
use super::delivery_reconcile::{
    reconcile_artifact, reconcile_deployment_delivery, reconcile_deployment_preflight,
    reconcile_git_delivery_flow, reconcile_gitops_change_set, reconcile_gitops_delivery_flow,
    reconcile_pipeline_intent, reconcile_release,
};
use super::support::reconcile_deployment_intent;

#[test]
fn pipeline_intent_reconcile_action_follows_approval_execution_and_build_output() {
    assert_eq!(
        pipeline_intent_reconcile_action(None, None, None),
        WorkItemReconcileAction::AwaitingPipelineIntentDefinition
    );
    let proposed = reconcile_pipeline_intent("proposed", None, None, None);
    assert_eq!(
        pipeline_intent_reconcile_action(Some(&proposed), None, None),
        WorkItemReconcileAction::AwaitingPipelineIntentApproval
    );
    let approved = reconcile_pipeline_intent("approved", None, None, None);
    assert_eq!(
        pipeline_intent_reconcile_action(Some(&approved), Some(false), None),
        WorkItemReconcileAction::AwaitingPipelineExecutionAuthorization
    );
    assert_eq!(
        pipeline_intent_reconcile_action(Some(&approved), Some(true), None),
        WorkItemReconcileAction::AwaitingPipelineExecution
    );
    let executing =
        reconcile_pipeline_intent("executing", Some("pipeline_run_created"), None, None);
    assert_eq!(
        pipeline_intent_reconcile_action(Some(&executing), None, None),
        WorkItemReconcileAction::WaitForPipelineExecution
    );
    let failed = reconcile_pipeline_intent("failed", Some("pipeline_run_failed"), None, None);
    assert_eq!(
        pipeline_intent_reconcile_action(Some(&failed), None, None),
        WorkItemReconcileAction::PipelineExecutionFailed
    );
    let completed_without_evidence =
        reconcile_pipeline_intent("approved", Some("pipeline_run_succeeded"), None, None);
    assert_eq!(
        pipeline_intent_reconcile_action(Some(&completed_without_evidence), None, None),
        WorkItemReconcileAction::AwaitingPipelineEvidenceReview
    );
    let completed_without_output = reconcile_pipeline_intent(
        "approved",
        Some("pipeline_run_succeeded"),
        None,
        Some("satisfied"),
    );
    assert_eq!(
        pipeline_intent_reconcile_action(Some(&completed_without_output), None, None),
        WorkItemReconcileAction::AwaitingPipelineBuildOutputReview
    );
    let completed_with_output = reconcile_pipeline_intent(
        "approved",
        Some("pipeline_run_succeeded"),
        Some("verified"),
        Some("satisfied"),
    );
    assert!(pipeline_intent_is_gitops_update_eligible(
        &completed_with_output
    ));
    assert_eq!(
        pipeline_intent_reconcile_action(Some(&completed_with_output), None, None),
        WorkItemReconcileAction::AwaitingDeploymentIntentDefinition
    );
    let deployment_intent = reconcile_deployment_intent();
    assert_eq!(
        pipeline_intent_reconcile_action(
            Some(&completed_with_output),
            None,
            Some(&deployment_intent),
        ),
        WorkItemReconcileAction::AwaitingGitOpsUpdatePlan
    );
    let stale = reconcile_pipeline_intent("stale", None, None, None);
    assert!(!pipeline_intent_is_gitops_update_eligible(&stale));
    assert_eq!(
        pipeline_intent_reconcile_action(Some(&stale), None, None),
        WorkItemReconcileAction::PipelineIntentBlocked
    );
}

#[test]
fn deployment_preflight_waits_for_gitops_merge_provenance() {
    assert!(!deployment_intent_requires_execution_preflight(
        Some("https://github.com/team/gitops.git"),
        Some("main"),
        "proposed",
        false,
    )
    .unwrap());
    assert!(!deployment_intent_requires_execution_preflight(
        Some("https://github.com/team/gitops.git"),
        Some("main"),
        "approved",
        false,
    )
    .unwrap());
    assert!(deployment_intent_requires_execution_preflight(
        Some("https://github.com/team/gitops.git"),
        Some("main"),
        "approved",
        true,
    )
    .unwrap());
    assert!(deployment_intent_requires_execution_preflight(None, None, "approved", false).unwrap());
    assert!(deployment_intent_requires_execution_preflight(
        Some("https://github.com/team/gitops.git"),
        None,
        "approved",
        false,
    )
    .is_err());
}

#[test]
fn git_delivery_reconcile_action_follows_durable_handoff_artifacts() {
    assert_eq!(
        git_delivery_reconcile_action(None),
        WorkItemReconcileAction::PrepareGitDelivery
    );

    let mut flow = reconcile_git_delivery_flow();
    assert_eq!(
        git_delivery_reconcile_action(Some(&flow)),
        WorkItemReconcileAction::AwaitingGitDeliveryAuthorization
    );
    flow.latest_preflight = Some(reconcile_artifact(
        "git_delivery_preflight",
        json!({ "status": "blocked", "dispatch_ready": false }),
    ));
    assert_eq!(
        git_delivery_reconcile_action(Some(&flow)),
        WorkItemReconcileAction::AwaitingGitDeliveryAuthorization
    );

    flow.latest_preflight = Some(reconcile_artifact(
        "git_delivery_preflight",
        json!({ "status": "ready_for_writer", "dispatch_ready": false }),
    ));
    assert_eq!(
        git_delivery_reconcile_action(Some(&flow)),
        WorkItemReconcileAction::AwaitingGitWriterAvailability
    );

    flow.latest_preflight = Some(reconcile_artifact(
        "git_delivery_preflight",
        json!({ "status": "ready_for_writer", "dispatch_ready": true }),
    ));
    assert_eq!(
        git_delivery_reconcile_action(Some(&flow)),
        WorkItemReconcileAction::AwaitingGitDeliveryExecution
    );

    flow.latest_execution = Some(reconcile_artifact("git_delivery_execution", json!({})));
    assert_eq!(
        git_delivery_reconcile_action(Some(&flow)),
        WorkItemReconcileAction::WaitForGitDelivery
    );

    flow.latest_result = Some(reconcile_artifact(
        "git_delivery_result",
        json!({ "status": "completed" }),
    ));
    assert_eq!(
        git_delivery_reconcile_action(Some(&flow)),
        WorkItemReconcileAction::AwaitingPullRequestObservation
    );

    flow.latest_result = Some(reconcile_artifact(
        "git_delivery_result",
        json!({ "status": "failed" }),
    ));
    assert_eq!(
        git_delivery_reconcile_action(Some(&flow)),
        WorkItemReconcileAction::GitDeliveryFailed
    );

    flow.latest_result = Some(reconcile_artifact(
        "git_delivery_result",
        json!({ "status": "completed" }),
    ));
    flow.latest_observation = Some(reconcile_artifact(
        "git_delivery_pr_observation",
        json!({ "status": "failed" }),
    ));
    assert_eq!(
        git_delivery_reconcile_action(Some(&flow)),
        WorkItemReconcileAction::AwaitingPullRequestObservation
    );

    flow.latest_observation = Some(reconcile_artifact(
        "git_delivery_pr_observation",
        json!({ "status": "observed", "merged": false }),
    ));
    assert_eq!(
        git_delivery_reconcile_action(Some(&flow)),
        WorkItemReconcileAction::AwaitingPullRequestMerge
    );

    flow.latest_merge = Some(reconcile_artifact("git_delivery_merge", json!({})));
    assert_eq!(
        git_delivery_reconcile_action(Some(&flow)),
        WorkItemReconcileAction::AwaitingPipelineIntentDefinition
    );
}

#[test]
fn gitops_change_set_reconcile_action_follows_durable_handoff_artifacts() {
    assert_eq!(
        gitops_change_set_reconcile_action(None, None, None),
        WorkItemReconcileAction::AwaitingGitOpsUpdatePlan
    );

    let proposed = reconcile_gitops_change_set("proposed");
    assert_eq!(
        gitops_change_set_reconcile_action(Some(&proposed), None, None),
        WorkItemReconcileAction::AwaitingGitOpsChangeSetApproval
    );
    let rejected = reconcile_gitops_change_set("rejected");
    assert_eq!(
        gitops_change_set_reconcile_action(Some(&rejected), None, None),
        WorkItemReconcileAction::GitOpsChangeSetBlocked
    );

    let approved = reconcile_gitops_change_set("approved");
    assert_eq!(
        gitops_change_set_reconcile_action(
            Some(&approved),
            None,
            Some(GitOpsBaseRevisionReconcileState::Missing),
        ),
        WorkItemReconcileAction::AwaitingGitOpsBaseRevision
    );
    assert_eq!(
        gitops_change_set_reconcile_action(
            Some(&approved),
            None,
            Some(GitOpsBaseRevisionReconcileState::Resolving),
        ),
        WorkItemReconcileAction::WaitForGitOpsBaseRevision
    );
    assert_eq!(
        gitops_change_set_reconcile_action(
            Some(&approved),
            None,
            Some(GitOpsBaseRevisionReconcileState::Resolved),
        ),
        WorkItemReconcileAction::AwaitingGitOpsDeliveryPlan
    );

    let mut flow = reconcile_gitops_delivery_flow();
    assert_eq!(
        gitops_change_set_reconcile_action(Some(&approved), Some(&flow), None),
        WorkItemReconcileAction::AwaitingGitOpsDeliveryAuthorization
    );
    flow.latest_preflight = Some(reconcile_artifact(
        "gitops_delivery_preflight",
        json!({ "status": "ready_for_writer", "dispatch_ready": false }),
    ));
    assert_eq!(
        gitops_change_set_reconcile_action(Some(&approved), Some(&flow), None),
        WorkItemReconcileAction::AwaitingGitOpsWriterAvailability
    );
    flow.latest_preflight = Some(reconcile_artifact(
        "gitops_delivery_preflight",
        json!({ "status": "ready_for_writer", "dispatch_ready": true }),
    ));
    assert_eq!(
        gitops_change_set_reconcile_action(Some(&approved), Some(&flow), None),
        WorkItemReconcileAction::AwaitingGitOpsDeliveryExecution
    );

    flow.latest_execution = Some(reconcile_artifact("gitops_delivery_execution", json!({})));
    assert_eq!(
        gitops_change_set_reconcile_action(Some(&approved), Some(&flow), None),
        WorkItemReconcileAction::WaitForGitOpsDelivery
    );

    flow.latest_result = Some(reconcile_artifact(
        "gitops_delivery_result",
        json!({ "status": "completed" }),
    ));
    assert_eq!(
        gitops_change_set_reconcile_action(Some(&approved), Some(&flow), None),
        WorkItemReconcileAction::AwaitingGitOpsPullRequestObservation
    );
    flow.latest_result = Some(reconcile_artifact(
        "gitops_delivery_result",
        json!({ "status": "failed" }),
    ));
    assert_eq!(
        gitops_change_set_reconcile_action(Some(&approved), Some(&flow), None),
        WorkItemReconcileAction::GitOpsDeliveryFailed
    );

    flow.latest_result = Some(reconcile_artifact(
        "gitops_delivery_result",
        json!({ "status": "completed" }),
    ));
    flow.latest_observation = Some(reconcile_artifact(
        "gitops_delivery_pr_observation",
        json!({ "status": "observed", "merged": false }),
    ));
    assert_eq!(
        gitops_change_set_reconcile_action(Some(&approved), Some(&flow), None),
        WorkItemReconcileAction::AwaitingGitOpsPullRequestMerge
    );
    assert!(WorkItemReconcileAction::AwaitingGitOpsPullRequestMerge.is_applyable());
    assert_eq!(
        action_effect(WorkItemReconcileAction::AwaitingGitOpsPullRequestMerge),
        "refresh the read-only GitOps pull-request observation to capture manual merge provenance"
    );
    flow.latest_observation = Some(reconcile_artifact(
        "gitops_delivery_pr_observation",
        json!({
            "status": "observed",
            "pull_request_state": "closed",
            "merged": false
        }),
    ));
    assert_eq!(
        gitops_change_set_reconcile_action(Some(&approved), Some(&flow), None),
        WorkItemReconcileAction::GitOpsDeliveryFailed
    );
    flow.latest_observation = Some(reconcile_artifact(
        "gitops_delivery_pr_observation",
        json!({
            "status": "observed",
            "pull_request_state": "closed",
            "merged": true
        }),
    ));
    assert_eq!(
        gitops_change_set_reconcile_action(Some(&approved), Some(&flow), None),
        WorkItemReconcileAction::AwaitingDeploymentIntentReview
    );
    let applied = reconcile_gitops_change_set("applied");
    assert_eq!(
        gitops_change_set_reconcile_action(Some(&applied), None, None),
        WorkItemReconcileAction::AwaitingDeploymentIntentReview
    );
}

#[test]
fn deployment_reconcile_action_follows_argo_and_release_artifacts() {
    let proposed = reconcile_deployment_intent();
    assert_eq!(
        deployment_intent_reconcile_action(Some(&proposed), None, None, None),
        WorkItemReconcileAction::AwaitingDeploymentIntentReview
    );
    let mut approved = reconcile_deployment_intent();
    approved.status = "approved".to_string();
    assert_eq!(
        deployment_intent_reconcile_action(Some(&approved), None, None, None),
        WorkItemReconcileAction::AwaitingDeploymentAuthorization
    );
    let blocked_preflight = reconcile_deployment_preflight(false);
    let flow = reconcile_deployment_delivery();
    assert_eq!(
        deployment_intent_reconcile_action(
            Some(&approved),
            Some(&blocked_preflight),
            Some(false),
            Some(&flow),
        ),
        WorkItemReconcileAction::AwaitingDeploymentAuthorization
    );
    let ready_preflight = reconcile_deployment_preflight(true);
    assert_eq!(
        deployment_intent_reconcile_action(
            Some(&approved),
            Some(&ready_preflight),
            Some(false),
            Some(&flow),
        ),
        WorkItemReconcileAction::AwaitingArgoRunnerAvailability
    );
    assert_eq!(
        deployment_intent_reconcile_action(
            Some(&approved),
            Some(&ready_preflight),
            Some(true),
            Some(&flow),
        ),
        WorkItemReconcileAction::AwaitingDeploymentExecution
    );

    let mut executing = reconcile_deployment_delivery();
    executing.latest_execution = Some(reconcile_artifact("argo_sync_execution", json!({})));
    assert_eq!(
        deployment_intent_reconcile_action(
            Some(&approved),
            Some(&ready_preflight),
            Some(true),
            Some(&executing),
        ),
        WorkItemReconcileAction::WaitForDeploymentExecution
    );
    executing.latest_result = Some(reconcile_artifact(
        "argo_sync_result",
        json!({ "status": "failed" }),
    ));
    assert_eq!(
        deployment_intent_reconcile_action(
            Some(&approved),
            Some(&ready_preflight),
            Some(true),
            Some(&executing),
        ),
        WorkItemReconcileAction::DeploymentExecutionFailed
    );
    executing.latest_result = Some(reconcile_artifact(
        "argo_sync_result",
        json!({ "status": "cancelled" }),
    ));
    assert_eq!(
        deployment_intent_reconcile_action(
            Some(&approved),
            Some(&ready_preflight),
            Some(true),
            Some(&executing),
        ),
        WorkItemReconcileAction::DeploymentExecutionFailed
    );
    executing.latest_result = Some(reconcile_artifact(
        "argo_sync_result",
        json!({ "status": "completed" }),
    ));
    assert_eq!(
        deployment_intent_reconcile_action(
            Some(&approved),
            Some(&ready_preflight),
            Some(true),
            Some(&executing),
        ),
        WorkItemReconcileAction::AwaitingReleaseDefinition
    );
    executing.release = Some(reconcile_release("proposed"));
    assert_eq!(
        deployment_intent_reconcile_action(
            Some(&approved),
            Some(&ready_preflight),
            Some(true),
            Some(&executing),
        ),
        WorkItemReconcileAction::AwaitingReleaseApproval
    );
    executing.release = Some(reconcile_release("approved"));
    assert_eq!(
        deployment_intent_reconcile_action(
            Some(&approved),
            Some(&ready_preflight),
            Some(true),
            Some(&executing),
        ),
        WorkItemReconcileAction::AwaitingReleaseVerification
    );
    executing.release = Some(reconcile_release("completed"));
    assert_eq!(
        deployment_intent_reconcile_action(
            Some(&approved),
            Some(&ready_preflight),
            Some(true),
            Some(&executing),
        ),
        WorkItemReconcileAction::CompleteWorkItem
    );
    assert_eq!(
        release_reconcile_action(Some(&reconcile_release("rejected"))),
        WorkItemReconcileAction::ReleaseBlocked
    );
    approved.status = "rejected".to_string();
    assert_eq!(
        deployment_intent_reconcile_action(Some(&approved), None, None, None),
        WorkItemReconcileAction::DeploymentIntentBlocked
    );
}

async fn seed_verified_completion_chain(state: &AppState, verified: bool) -> String {
    let session_id = SessionId::new(format!("ses_completion_{verified}"));
    let run_id = RunId::new(format!("run_completion_{verified}"));
    let work_item_id = format!("witem_completion_{verified}");
    let work_plan_id = format!("wplan_completion_{verified}");
    let change_set_id = format!("cset_completion_{verified}");
    let pipeline_intent_id = format!("pint_completion_{verified}");
    let deployment_intent_id = format!("dint_completion_{verified}");
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: "Completion fixture".to_string(),
            cwd: "/workspace".to_string(),
        })
        .await
        .unwrap();
    state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: "Completion fixture".to_string(),
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
            title: "Complete finance change".to_string(),
            intent: "Prove verified delivery completion".to_string(),
            acceptance_criteria: vec!["post-sync release verified".to_string()],
            source_repo: "https://github.com/example/finance-api.git".to_string(),
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
            title: "Completion plan".to_string(),
            summary: "Reviewed delivery plan".to_string(),
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
            id: change_set_id.clone(),
            work_item_id: Some(work_item_id.clone()),
            work_plan_id: work_plan_id.clone(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Completion ChangeSet".to_string(),
            summary: "Reviewed source diff".to_string(),
            risk_level: "high".to_string(),
            material_hash: "completion_hash".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Application".to_string()),
            resource_name: Some("finance-api".to_string()),
            change_set_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_pipeline_intent(CreatePipelineIntent {
            id: pipeline_intent_id.clone(),
            change_set_id: change_set_id.clone(),
            work_plan_id: work_plan_id.clone(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Completion pipeline".to_string(),
            summary: "Verified build".to_string(),
            risk_level: "high".to_string(),
            intent_kind: "tekton_build_test_package".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Pipeline".to_string()),
            resource_name: Some("finance-build".to_string()),
            intent_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_deployment_intent(CreateDeploymentIntent {
            id: deployment_intent_id.clone(),
            pipeline_intent_id: pipeline_intent_id.clone(),
            change_set_id: change_set_id.clone(),
            work_plan_id: work_plan_id.clone(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Completion deployment".to_string(),
            summary: "Verified dev deployment".to_string(),
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
            .create_release(CreateRelease {
                id: format!("rel_completion_{verified}"),
                deployment_intent_id,
                pipeline_intent_id,
                change_set_id,
                work_plan_id,
                remediation_plan_id: None,
                incident_id: None,
                session_id,
                run_id: Some(run_id),
                status: "completed".to_string(),
                title: "Completion release".to_string(),
                summary: "Verified dev release".to_string(),
                risk_level: "high".to_string(),
                release_kind: "gitops_release".to_string(),
                target_environment: Some("dev".to_string()),
                target_namespace: Some("apps-dev".to_string()),
                argo_application: Some("finance-api".to_string()),
                version: Some("v1".to_string()),
                commit_sha: Some("0123456789012345678901234567890123456789".to_string()),
                image_digest: Some("sha256:completion".to_string()),
                rollback_ref: None,
                release_json: if verified {
                    json!({ "post_sync_verification": { "status": "verified", "runtime_ready": true } })
                } else {
                    json!({ "post_sync_verification": { "status": "attention_required", "runtime_ready": false } })
                },
            })
            .await
            .unwrap();
    work_item_id
}

#[tokio::test]
async fn verified_release_completion_is_durable_and_fail_closed() {
    let state = test_state().await;
    let work_item_id = seed_verified_completion_chain(&state, true).await;
    let completed = complete_work_item_from_verified_release(
        &state,
        &work_item_id,
        Some("lucas".to_string()),
        Some("verified finance delivery".to_string()),
    )
    .await
    .unwrap();
    assert_eq!(completed.work_item.status, "completed");
    assert_eq!(completed.release.status, "completed");
    let audit = state
        .store
        .list_audit_events(Some("work_item"), Some(&work_item_id), None, 10)
        .await
        .unwrap();
    assert!(audit
        .iter()
        .any(|event| event.kind == "work_item.completed_from_verified_release"));

    let unverified_work_item_id = seed_verified_completion_chain(&state, false).await;
    let error = complete_work_item_from_verified_release(
        &state,
        &unverified_work_item_id,
        Some("lucas".to_string()),
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(error.status, StatusCode::CONFLICT);
    assert_eq!(
        state
            .store
            .get_work_item(&unverified_work_item_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "awaiting_approval"
    );
}

#[tokio::test]
async fn delivery_failure_blocks_once_and_never_retries_or_rolls_back() {
    let state = test_state().await;
    let work_item_id = seed_verified_completion_chain(&state, true).await;
    let blocked = block_work_item_from_delivery_failure(
        &state,
        &work_item_id,
        WorkItemReconcileAction::DeploymentExecutionFailed,
        "deployment_execution_failed",
        "the bounded Argo sync execution reported a failed delivery",
        Some("lucas".to_string()),
        None,
    )
    .await
    .unwrap();
    assert_eq!(blocked.status, "blocked");
    let repeat = block_work_item_from_delivery_failure(
        &state,
        &work_item_id,
        WorkItemReconcileAction::DeploymentExecutionFailed,
        "deployment_execution_failed",
        "the bounded Argo sync execution reported a failed delivery",
        Some("lucas".to_string()),
        None,
    )
    .await
    .unwrap();
    assert_eq!(repeat.status, "blocked");
    let audit = state
        .store
        .list_audit_events(Some("work_item"), Some(&work_item_id), None, 10)
        .await
        .unwrap();
    let delivery_blocks = audit
        .iter()
        .filter(|event| event.kind == "work_item.delivery_blocked")
        .collect::<Vec<_>>();
    assert_eq!(delivery_blocks.len(), 1);
    assert_eq!(
        delivery_blocks[0].payload_json["extra"]["controller_action"],
        json!("deployment_execution_failed")
    );
    assert_eq!(
        delivery_blocks[0].payload_json["extra"]["automatic_retry"],
        json!(false)
    );
    assert_eq!(
        delivery_blocks[0].payload_json["extra"]["automatic_rollback"],
        json!(false)
    );
    assert_eq!(
        delivery_blocks[0].payload_json["extra"]["mutation_performed"],
        json!(false)
    );
    let observation_id = delivery_blocks[0].payload_json["extra"]["observation_id"]
        .as_str()
        .unwrap();
    let incident_id = delivery_blocks[0].payload_json["extra"]["incident_id"]
        .as_str()
        .unwrap();
    let remediation_plan_id = delivery_blocks[0].payload_json["extra"]["remediation_plan_id"]
        .as_str()
        .unwrap();
    let observation = state
        .store
        .get_observation(observation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(observation.source, "pharness_controller");
    assert_eq!(observation.kind, "delivery_failure");
    assert_eq!(observation.subject, format!("work_item/{work_item_id}"));
    assert_eq!(observation.data_json["automatic_rollback"], json!(false));
    let incident = state
        .store
        .get_incident(incident_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(incident.observation_id, observation.id);
    assert_eq!(incident.status, "candidate");
    assert_eq!(incident.severity, "high");
    assert_eq!(incident.resource_namespace.as_deref(), Some("apps-dev"));
    assert_eq!(incident.resource_name.as_deref(), Some("finance-api"));
    assert_eq!(incident.data_json["mutation_performed"], json!(false));
    let remediation_plan = state
        .store
        .get_remediation_plan(remediation_plan_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(remediation_plan.incident_id, incident.id);
    assert_eq!(remediation_plan.status, "draft");
    assert!(remediation_plan.requires_approval);
    assert_eq!(
        remediation_plan.plan_json["source"],
        json!("work_item_delivery_failure")
    );
    assert_eq!(
        remediation_plan.plan_json["non_goals"][0],
        json!("No automatic retry")
    );
    let remediation_gates = state
        .store
        .list_approval_gates(ApprovalGateListFilter {
            remediation_plan_id: Some(remediation_plan.id.clone()),
            incident_id: Some(incident.id.clone()),
            limit: 20,
            ..ApprovalGateListFilter::default()
        })
        .await
        .unwrap();
    assert_eq!(remediation_gates.len(), 5);
    assert!(remediation_gates
        .iter()
        .any(|gate| gate.gate_kind == "git_mutation"));
    assert!(remediation_gates
        .iter()
        .all(|gate| gate.status == "pending"));
    let run_id = RunId::new("run_completion_true");
    let run_audit = state
        .store
        .list_audit_events(None, None, Some(&run_id), 20)
        .await
        .unwrap();
    assert_eq!(
        run_audit
            .iter()
            .filter(|event| event.kind == "observation.delivery_failure_recorded")
            .count(),
        1
    );
    assert_eq!(
        run_audit
            .iter()
            .filter(|event| event.kind == "incident.delivery_failure_created")
            .count(),
        1
    );
    assert_eq!(
        run_audit
            .iter()
            .filter(|event| event.kind == "remediation_plan.created")
            .count(),
        1
    );
}

#[tokio::test]
async fn gitops_delivery_plan_requires_a_current_resolved_base_revision() {
    let state = test_state().await;
    let session_id = SessionId::new("ses_gitops_delivery");
    let run_id = RunId::new("run_gitops_delivery");
    let work_item_id = "witem_gitops_delivery";
    let work_plan_id = "wplan_gitops_delivery";
    let gitops_change_set_id = "gset_gitops_delivery";
    let base_commit = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";

    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: "GitOps delivery".to_string(),
            cwd: "/workspace".to_string(),
        })
        .await
        .unwrap();
    state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: "GitOps delivery".to_string(),
            cwd: "/workspace".to_string(),
            max_turns: 4,
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
            title: "Finance GitOps update".to_string(),
            intent: "Promote a verified finance image in dev".to_string(),
            acceptance_criteria: vec!["GitOps image is digest-pinned".to_string()],
            source_repo: "https://github.com/example/finance-api.git".to_string(),
            source_ref: "main".to_string(),
            source_commit: None,
            pipeline_contract_id: None,
            deployment_contract_id: None,
            gitops_repo: Some("https://github.com/example/finance-gitops.git".to_string()),
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
            title: "Finance GitOps plan".to_string(),
            summary: "Update the dev image reference".to_string(),
            risk_level: "high".to_string(),
            requires_approval: true,
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("application".to_string()),
            resource_name: Some("finance-api".to_string()),
            work_plan_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_change_set(CreateChangeSet {
            id: "cset_finance_source".to_string(),
            work_item_id: Some(work_item_id.to_string()),
            work_plan_id: work_plan_id.to_string(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "applied".to_string(),
            title: "Finance source ChangeSet".to_string(),
            summary: "Verified source change".to_string(),
            risk_level: "medium".to_string(),
            material_hash: "material_finance_source".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: None,
            resource_name: None,
            change_set_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_pipeline_intent(CreatePipelineIntent {
            id: "pint_finance".to_string(),
            change_set_id: "cset_finance_source".to_string(),
            work_plan_id: work_plan_id.to_string(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "completed".to_string(),
            title: "Finance pipeline".to_string(),
            summary: "Verified finance build".to_string(),
            risk_level: "medium".to_string(),
            intent_kind: "build_test_package".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: None,
            resource_name: None,
            intent_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_deployment_intent(CreateDeploymentIntent {
            id: "dint_finance".to_string(),
            pipeline_intent_id: "pint_finance".to_string(),
            change_set_id: "cset_finance_source".to_string(),
            work_plan_id: work_plan_id.to_string(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "proposed".to_string(),
            title: "Finance deployment".to_string(),
            summary: "Prepare finance dev deployment".to_string(),
            risk_level: "high".to_string(),
            intent_kind: "argo_sync".to_string(),
            target_environment: Some("dev".to_string()),
            target_namespace: Some("apps-dev".to_string()),
            argo_application: Some("finance-api".to_string()),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: None,
            resource_name: None,
            intent_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_gitops_update".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "gitops_update_plan".to_string(),
            label: "GitOps update plan".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({})),
        })
        .await
        .unwrap();
    state
        .store
        .create_gitops_change_set(CreateGitOpsChangeSet {
            id: gitops_change_set_id.to_string(),
            work_item_id: work_item_id.to_string(),
            work_plan_id: work_plan_id.to_string(),
            source_change_set_id: "cset_finance_source".to_string(),
            pipeline_intent_id: "pint_finance".to_string(),
            deployment_intent_id: "dint_finance".to_string(),
            gitops_update_plan_artifact_id: "art_gitops_update".to_string(),
            session_id: session_id.clone(),
            run_id: run_id.clone(),
            status: "approved".to_string(),
            title: "Finance GitOps ChangeSet".to_string(),
            summary: "Set the dev finance image to a verified digest".to_string(),
            risk_level: "high".to_string(),
            material_hash: "material_gitops_delivery".to_string(),
            gitops_repo: "https://github.com/example/finance-gitops.git".to_string(),
            gitops_ref: "main".to_string(),
            head_branch: "pharness/witem_gitops_delivery/gitops".to_string(),
            kustomization_path: "apps/finance-api/kustomization.yaml".to_string(),
            image_name: "registry.example.test/finance-api".to_string(),
            image_ref: "registry.example.test/finance-api@sha256:1234567890abcdef".to_string(),
            gitops_change_set_json: json!({}),
        })
        .await
        .unwrap();

    let missing_base = prepare_gitops_change_set_delivery(
        State(state.clone()),
        Path(gitops_change_set_id.to_string()),
        Json(PrepareGitOpsDeliveryRequest {
            actor: Some("lucas".to_string()),
            reason: Some("must bind an immutable base".to_string()),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(missing_base.status, StatusCode::CONFLICT);

    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_gitops_base_revision".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "gitops_base_revision".to_string(),
            label: "Resolved GitOps base revision".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": "grev_test",
                "status": "resolved",
                "gitops_change_set_id": gitops_change_set_id,
                "material_hash": "material_gitops_delivery",
                "repository": "https://github.com/example/finance-gitops.git",
                "base_ref": "main",
                "base_commit": base_commit,
                "identity": "agent:git-observer",
            })),
        })
        .await
        .unwrap();

    let Json(first) = prepare_gitops_change_set_delivery(
        State(state.clone()),
        Path(gitops_change_set_id.to_string()),
        Json(PrepareGitOpsDeliveryRequest {
            actor: Some("lucas".to_string()),
            reason: Some("prepare immutable writer input".to_string()),
        }),
    )
    .await
    .unwrap();
    assert!(first.created);
    assert_eq!(first.base_revision.id, "art_gitops_base_revision");
    assert_eq!(
        first.artifact.content_json.as_ref().unwrap()["source"]["base_commit"],
        base_commit
    );
    assert_eq!(
        first.artifact.content_json.as_ref().unwrap()["update"]["operation"],
        "kustomize_set_image"
    );
    assert_eq!(
        first.artifact.content_json.as_ref().unwrap()["execution"]["enabled"],
        true
    );
    assert_eq!(
        first.artifact.content_json.as_ref().unwrap()["execution"]["mode"],
        "gitops_writer_job"
    );

    let Json(second) = prepare_gitops_change_set_delivery(
        State(state.clone()),
        Path(gitops_change_set_id.to_string()),
        Json(PrepareGitOpsDeliveryRequest {
            actor: Some("lucas".to_string()),
            reason: Some("repeat immutable writer input".to_string()),
        }),
    )
    .await
    .unwrap();
    assert!(!second.created);
    assert_eq!(second.artifact.id, first.artifact.id);

    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await
        .unwrap()
        .expect("work item should exist");
    let work_plan = state
        .store
        .get_work_plan(work_plan_id)
        .await
        .unwrap()
        .expect("work plan should exist");
    for gate in approval_gates_from_work_item(&work_item, &work_plan) {
        state.store.create_approval_gate(gate).await.unwrap();
    }
    let Json(blocked_preflight) = preflight_gitops_change_set_delivery(
        State(state.clone()),
        None,
        Path(gitops_change_set_id.to_string()),
        Json(GitOpsDeliveryPreflightRequest {
            subject: None,
            actor: Some("lucas".to_string()),
            reason: Some("record missing GitOps writer authorization".to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(blocked_preflight.status, "blocked");
    assert!(!blocked_preflight.authorization_ready);
    assert!(!blocked_preflight.approval_gate_ready);
    assert!(!blocked_preflight.dispatch_ready);

    let Json(authorization) = authorize_gitops_change_set_delivery(
        State(state.clone()),
        None,
        Path(gitops_change_set_id.to_string()),
        Json(CreateGitOpsDeliveryAuthorizationRequest {
            subject: None,
            created_by: Some("lucas".to_string()),
            reason: "authorize one reviewed GitOps delivery".to_string(),
            expires_at: None,
        }),
    )
    .await
    .unwrap();
    assert!(authorization.created);
    assert_eq!(authorization.grant.subject, "agent:gitops-writer");
    assert_eq!(
        authorization.grant.scope["gitops_change_set_ids"],
        json!([gitops_change_set_id])
    );
    assert_eq!(
        authorization.grant.scope["gitops_delivery_plan_artifact_ids"],
        json!([first.artifact.id])
    );

    let Json(authorized_but_gated) = preflight_gitops_change_set_delivery(
        State(state.clone()),
        None,
        Path(gitops_change_set_id.to_string()),
        Json(GitOpsDeliveryPreflightRequest {
            subject: None,
            actor: Some("lucas".to_string()),
            reason: Some("prove a grant cannot bypass the GitOps gate".to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(authorized_but_gated.status, "blocked");
    assert!(authorized_but_gated.authorization_ready);
    assert!(!authorized_but_gated.approval_gate_ready);

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
        .expect("WorkItem GitOps gate should exist");
    state
        .store
        .decide_approval_gate(
            &gitops_gate.id,
            "satisfied",
            Some("lucas".to_string()),
            Some("reviewed immutable GitOps delivery".to_string()),
        )
        .await
        .unwrap();

    let Json(ready_preflight) = preflight_gitops_change_set_delivery(
        State(state.clone()),
        None,
        Path(gitops_change_set_id.to_string()),
        Json(GitOpsDeliveryPreflightRequest {
            subject: None,
            actor: Some("lucas".to_string()),
            reason: Some("record GitOps writer readiness".to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(ready_preflight.status, "ready_for_writer");
    assert!(ready_preflight.authorization_ready);
    assert!(ready_preflight.approval_gate_ready);
    assert!(!ready_preflight.dispatch_ready);
    assert_eq!(
        ready_preflight
            .permission_grant
            .as_ref()
            .map(|grant| grant.id.as_str()),
        Some(authorization.grant.id.as_str())
    );
    assert!(ready_preflight.checks.iter().any(|check| {
        check["code"] == "gitops_writer_executor_available" && check["passed"] == false
    }));

    let audit = state
        .store
        .list_audit_events(
            Some("gitops_change_set"),
            Some(gitops_change_set_id),
            None,
            10,
        )
        .await
        .unwrap();
    assert!(audit
        .iter()
        .any(|event| event.kind == "gitops_change_set.delivery_prepared"));

    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_gitops_delivery_result".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "gitops_delivery_result".to_string(),
            label: "Completed GitOps delivery".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": "gopsexec_test",
                "status": "completed",
                "gitops_change_set_id": gitops_change_set_id,
                "gitops_delivery_plan_artifact_id": first.artifact.id,
                "details": {
                    "branch": "pharness/witem_gitops_delivery/gitops",
                    "commit_sha": base_commit,
                    "pull_request_url": "https://github.com/example/finance-gitops/pull/42",
                    "pull_request_number": 42,
                },
            })),
        })
        .await
        .unwrap();
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_gitops_delivery_observation_execution".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "gitops_delivery_observation_execution".to_string(),
            label: "GitOps delivery observation".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": "gopsobs_test",
                "status": "dispatched",
                "gitops_change_set_id": gitops_change_set_id,
                "gitops_delivery_plan_artifact_id": first.artifact.id,
                "gitops_delivery_result_artifact_id": "art_gitops_delivery_result",
                "source": {
                    "repository": "https://github.com/example/finance-gitops.git",
                    "head_branch": "pharness/witem_gitops_delivery/gitops",
                    "source_commit_sha": base_commit,
                    "pull_request_url": "https://github.com/example/finance-gitops/pull/42",
                    "pull_request_number": 42,
                },
            })),
        })
        .await
        .unwrap();
    let pipeline_intent = state
        .store
        .get_pipeline_intent("pint_finance")
        .await
        .unwrap()
        .expect("PipelineIntent should exist");
    let missing_merge =
        observed_gitops_merge_for_deployment(&state.store, &work_item, &pipeline_intent)
            .await
            .unwrap_err();
    assert!(missing_merge
        .message
        .contains("observed immutable GitOps pull-request merge"));
    let merge_commit = "b1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
    let Json(observation) = internal_gitops_delivery_observation_outcome(
        State(state.clone()),
        Path(gitops_change_set_id.to_string()),
        Json(GitOpsDeliveryObservationOutcomeRequest {
            execution_id: "gopsobs_test".to_string(),
            status: "observed".to_string(),
            error_code: None,
            pull_request_state: Some("closed".to_string()),
            merged: Some(true),
            merge_commit_sha: Some(merge_commit.to_string()),
            head_branch: Some("pharness/witem_gitops_delivery/gitops".to_string()),
            head_commit_sha: Some(base_commit.to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(observation.kind, "gitops_delivery_pr_observation");
    let gitops_change_set = state
        .store
        .get_gitops_change_set(gitops_change_set_id)
        .await
        .unwrap()
        .expect("GitOps change set should exist");
    let flow = gitops_delivery_flow(&state.store, Some(&gitops_change_set))
        .await
        .unwrap()
        .expect("GitOps delivery flow should exist");
    assert_eq!(
        flow.latest_observation
            .as_ref()
            .and_then(|artifact| artifact.content_json.as_ref())
            .and_then(|content| content.get("merged")),
        Some(&json!(true))
    );
    assert_eq!(
        flow.latest_merge
            .as_ref()
            .and_then(|artifact| artifact.content_json.as_ref())
            .and_then(|content| content.get("merge_commit_sha")),
        Some(&json!(merge_commit))
    );
    let observed_merge =
        observed_gitops_merge_for_deployment(&state.store, &work_item, &pipeline_intent)
            .await
            .unwrap()
            .expect("declared GitOps target should require and return merge evidence");
    assert_eq!(observed_merge.id, flow.latest_merge.unwrap().id);
}

fn protected_production_request() -> CreateWorkItemRequest {
    CreateWorkItemRequest {
        title: "Validate yfinance input".to_string(),
        intent: "Add bounded validation and tests".to_string(),
        acceptance_criteria: vec!["python -m unittest discover -s tests -v".to_string()],
        source_repo: PROTECTED_SOURCE_REPO.to_string(),
        source_ref: "main".to_string(),
        source_commit: Some("a".repeat(40)),
        pipeline_contract_id: Some("pcontract_yfinance".to_string()),
        deployment_contract_id: Some("dcontract_yfinance".to_string()),
        gitops_repo: Some(PROTECTED_GITOPS_REPO.to_string()),
        gitops_ref: Some("main".to_string()),
        gitops_kustomization_path: Some(PROTECTED_KUSTOMIZATION_PATH.to_string()),
        gitops_image_name: Some(PROTECTED_IMAGE_NAME.to_string()),
        target_environment: PROTECTED_ENVIRONMENT.to_string(),
        target_namespace: Some(PROTECTED_NAMESPACE.to_string()),
        argo_application: Some(PROTECTED_ARGO_APPLICATION.to_string()),
        workload_kind: Some(PROTECTED_WORKLOAD_KIND.to_string()),
        workload_name: Some(PROTECTED_WORKLOAD_NAME.to_string()),
        rollback_owner: Some(PROTECTED_ROLLBACK_OWNER.to_string()),
        production_impacting: true,
        max_attempts: Some(1),
        max_elapsed_seconds: Some(3_600),
        environment_profile_id: None,
        initial_turn_budget: None,
        hard_turn_budget: None,
        initial_token_budget: None,
        hard_token_budget: None,
        active_execution_seconds: None,
        recoverable_tool_error_limit: None,
        identical_failure_limit: None,
        actor: Some("lucas".to_string()),
        preflight_state_hash: None,
    }
}

#[test]
fn protected_production_target_requires_every_exact_server_owned_coordinate() {
    let request = protected_production_request();
    assert!(request_matches_protected_target(&request));

    let mut wrong_namespace = request.clone();
    wrong_namespace.target_namespace = Some("apps-staging".to_string());
    assert!(!request_matches_protected_target(&wrong_namespace));

    let mut wrong_image = request.clone();
    wrong_image.gitops_image_name = Some("registry.example.test/other".to_string());
    assert!(!request_matches_protected_target(&wrong_image));

    let mut wrong_repository = request;
    wrong_repository.gitops_repo = Some("https://github.com/lward27/other.git".to_string());
    assert!(!request_matches_protected_target(&wrong_repository));
}

#[test]
fn immutable_production_identifiers_reject_mutable_or_malformed_values() {
    assert!(immutable_git_object_id(&"a".repeat(40)));
    assert!(immutable_git_object_id(&"b".repeat(64)));
    assert!(!immutable_git_object_id("main"));
    assert!(!immutable_git_object_id(&"A".repeat(40)));
    assert!(immutable_image_digest(&format!(
        "sha256:{}",
        "c".repeat(64)
    )));
    assert!(!immutable_image_digest("latest"));
    assert!(!immutable_image_digest("sha256:abc"));
    assert!(!immutable_image_digest(&format!(
        "sha256:{}",
        "C".repeat(64)
    )));
}

#[tokio::test]
async fn production_authorization_rejects_expired_or_overlong_windows() {
    let state = test_state().await;
    let request = protected_production_request();
    state
        .store
        .create_work_item(CreateWorkItem {
            id: "witem_production_window".to_string(),
            status: "proposed".to_string(),
            title: request.title,
            intent: request.intent,
            acceptance_criteria: request.acceptance_criteria,
            source_repo: request.source_repo,
            source_ref: request.source_ref,
            source_commit: request.source_commit,
            pipeline_contract_id: request.pipeline_contract_id,
            deployment_contract_id: request.deployment_contract_id,
            gitops_repo: request.gitops_repo,
            gitops_ref: request.gitops_ref,
            gitops_kustomization_path: request.gitops_kustomization_path,
            gitops_image_name: request.gitops_image_name,
            target_environment: request.target_environment,
            target_namespace: request.target_namespace,
            argo_application: request.argo_application,
            workload_kind: request.workload_kind,
            workload_name: request.workload_name,
            rollback_owner: request.rollback_owner,
            production_impacting: true,
            max_attempts: 1,
            max_elapsed_seconds: 3_600,
            environment_profile_id: None,
            run_budget: Default::default(),
            repository_contract_json: None,
            repository_contract_hash: None,
            environment_preparation_status: "not_required".to_string(),
            created_by: request.actor,
        })
        .await
        .unwrap();
    let item = state
        .store
        .get_work_item("witem_production_window")
        .await
        .unwrap()
        .unwrap();
    let now = current_millis();
    assert!(bounded_production_grant_expiry(&item, Some((now - 1).to_string())).is_err());
    assert!(
        bounded_production_grant_expiry(&item, Some((now + 30 * 60 * 1_000 + 1).to_string()))
            .is_err()
    );
    assert!(
        bounded_production_grant_expiry(&item, Some((now + 5 * 60 * 1_000).to_string())).is_ok()
    );
}

#[tokio::test]
async fn future_production_gates_are_visible_but_cannot_be_decided_early() {
    let state = test_state().await;
    let work_item_id = "witem_future_production_gates";
    let session_id = SessionId::new("ses_future_production_gates");
    let item = state
        .store
        .create_work_item(CreateWorkItem {
            id: work_item_id.to_string(),
            status: "awaiting_approval".to_string(),
            title: "Protected production gate ordering".to_string(),
            intent: "Keep future gates inert".to_string(),
            acceptance_criteria: vec!["python -m compileall -q src tests".to_string()],
            source_repo: PROTECTED_SOURCE_REPO.to_string(),
            source_ref: "main".to_string(),
            source_commit: Some("a".repeat(40)),
            pipeline_contract_id: Some("pcontract_yfinance".to_string()),
            deployment_contract_id: Some("dcontract_yfinance".to_string()),
            gitops_repo: Some(PROTECTED_GITOPS_REPO.to_string()),
            gitops_ref: Some("main".to_string()),
            gitops_kustomization_path: Some(PROTECTED_KUSTOMIZATION_PATH.to_string()),
            gitops_image_name: Some(PROTECTED_IMAGE_NAME.to_string()),
            target_environment: PROTECTED_ENVIRONMENT.to_string(),
            target_namespace: Some(PROTECTED_NAMESPACE.to_string()),
            argo_application: Some(PROTECTED_ARGO_APPLICATION.to_string()),
            workload_kind: Some(PROTECTED_WORKLOAD_KIND.to_string()),
            workload_name: Some(PROTECTED_WORKLOAD_NAME.to_string()),
            rollback_owner: Some(PROTECTED_ROLLBACK_OWNER.to_string()),
            production_impacting: true,
            max_attempts: 2,
            max_elapsed_seconds: 3_600,
            created_by: Some("tester".to_string()),
            environment_profile_id: Some("python-3.11".to_string()),
            run_budget: Default::default(),
            repository_contract_json: None,
            repository_contract_hash: None,
            environment_preparation_status: "pending".to_string(),
        })
        .await
        .unwrap();
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: item.title.clone(),
            cwd: format!("work-item/{work_item_id}"),
        })
        .await
        .unwrap();
    let plan = state
        .store
        .create_work_plan(CreateWorkPlan {
            id: "wplan_future_production_gates".to_string(),
            work_item_id: Some(work_item_id.to_string()),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: None,
            status: "approved".to_string(),
            title: "Approved WorkPlan".to_string(),
            summary: item.intent.clone(),
            risk_level: "high".to_string(),
            requires_approval: true,
            resource_namespace: item.target_namespace.clone(),
            resource_kind: Some("application".to_string()),
            resource_name: item.argo_application.clone(),
            work_plan_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_workspace(CreateWorkspace {
            id: "ws_future_production_gates".to_string(),
            work_item_id: work_item_id.to_string(),
            run_id: None,
            status: "declared".to_string(),
            source_repo: item.source_repo.clone(),
            source_ref: item.source_ref.clone(),
            resolved_commit: item.source_commit.clone(),
            branch: None,
            retention_status: "ephemeral".to_string(),
            actor: Some("tester".to_string()),
            reason: Some("future gate test".to_string()),
        })
        .await
        .unwrap();
    let mut gate_ids = Vec::new();
    for gate in approval_gates_from_work_item(&item, &plan) {
        let gate = state.store.create_approval_gate(gate).await.unwrap();
        gate_ids.push(gate.id);
    }
    let source_gate = gate_ids
        .iter()
        .find(|id| id.ends_with("source_mutation"))
        .unwrap()
        .clone();
    let pipeline_gate = gate_ids
        .iter()
        .find(|id| id.ends_with("pipeline_mutation"))
        .unwrap()
        .clone();

    let Json(listed) = list_approval_gates(
        State(state.clone()),
        Query(ListApprovalGatesQuery {
            work_item_id: Some(work_item_id.to_string()),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    let listed_pipeline_gate = listed
        .approval_gates
        .iter()
        .find(|gate| gate.id == pipeline_gate)
        .unwrap();
    assert!(!listed_pipeline_gate.actionable);
    assert_eq!(
        listed_pipeline_gate.lifecycle_blocker.as_deref(),
        Some("Source delivery has not produced an approved ChangeSet.")
    );

    let early = satisfy_approval_gate(
        State(state.clone()),
        Path(source_gate.clone()),
        Json(DecideApprovalGateRequest {
            decided_by: Some("tester".to_string()),
            reason: Some("too early".to_string()),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(early.status, StatusCode::CONFLICT);
    let batch = batch_decide_approval_gates(
        State(state.clone()),
        Json(crate::dto::BatchDecideApprovalGatesRequest {
            gate_ids: vec![source_gate.clone(), pipeline_gate.clone()],
            decision: "satisfied".to_string(),
            decided_by: "tester".to_string(),
            reason: "must remain atomic".to_string(),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(batch.status, StatusCode::CONFLICT);
    assert_eq!(
        state
            .store
            .get_approval_gate(&source_gate)
            .await
            .unwrap()
            .unwrap()
            .status,
        "pending"
    );
    let Json(flow) = work_item_flow(State(state), Path(work_item_id.to_string()))
        .await
        .unwrap();
    assert!(flow.action_rail.iter().any(|action| {
        action.id == format!("satisfy_approval_gate:{pipeline_gate}")
            && action.status == "blocked"
            && action
                .blockers
                .iter()
                .any(|blocker| blocker.code == "future_lifecycle_gate")
    }));
}

#[tokio::test]
async fn operator_summary_uses_exact_acceptance_evidence_and_deduplicates_failures() {
    let state = test_state().await;
    let session_id = SessionId::new("ses_operator_summary_regression");
    let run_id = RunId::new("run_operator_summary_regression");
    let work_item_id = "witem_operator_summary_regression";
    let unit = "python -m unittest discover -s tests -v";
    let compile = "python -m compileall -q src tests";
    state
        .store
        .create_work_item(CreateWorkItem {
            id: work_item_id.to_string(),
            status: "executing".to_string(),
            title: "Operator summary regression".to_string(),
            intent: "Count exact evidence".to_string(),
            acceptance_criteria: vec![unit.to_string(), compile.to_string()],
            source_repo: "https://github.com/lward27/yfinance_wrapper.git".to_string(),
            source_ref: "main".to_string(),
            source_commit: Some("a".repeat(40)),
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
            max_attempts: 2,
            max_elapsed_seconds: 3_600,
            created_by: Some("tester".to_string()),
            environment_profile_id: Some("python-3.11".to_string()),
            run_budget: Default::default(),
            repository_contract_json: None,
            repository_contract_hash: None,
            environment_preparation_status: "succeeded".to_string(),
        })
        .await
        .unwrap();
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: "Operator summary regression".to_string(),
            cwd: "/workspace".to_string(),
        })
        .await
        .unwrap();
    state
            .store
            .create_run(CreateRun {
                id: run_id.clone(),
                session_id: session_id.clone(),
                user_task: "regression".to_string(),
                cwd: "/workspace".to_string(),
                max_turns: 48,
                initial_status: "failed".to_string(),
                execution_target_json: json!({
                    "run_scope": {"work_item_id": work_item_id},
                    "repository_contract": {
                        "api_version": "pharness.dev/v1alpha1",
                        "environment_profile": "python-3.11",
                        "dependency_lock": {"kind":"pip_requirements","path":"requirements.lock","sha256": "b".repeat(64)},
                        "writable_paths": ["src/**", "tests/**", "readme.md"],
                        "acceptance_commands": [
                            {"name":"unit-tests","command":unit},
                            {"name":"compile-check","command":compile}
                        ],
                        "roots": {"source":["src"],"tests":["tests"],"documentation":["readme.md"]},
                        "agent_network":"denied",
                        "package_installation":"preparation_only"
                    }
                }),
            })
            .await
            .unwrap();
    let payloads = [
        (
            EventKind::ModelRequestStarted,
            json!({"turn":0,"estimated_input_tokens":100}),
        ),
        (
            EventKind::ActionProposed,
            json!({"action":"run_shell","cmd":"mkdir -p tests"}),
        ),
        (EventKind::ToolStarted, json!({"action":"run_shell"})),
        (EventKind::ToolFinished, json!({"status":"ok"})),
        (
            EventKind::ActionProposed,
            json!({"action":"run_shell","cmd":unit}),
        ),
        (EventKind::ToolStarted, json!({"action":"run_shell"})),
        (EventKind::ToolFinished, json!({"status":"ok"})),
        (EventKind::ToolFinished, json!({"status":"error"})),
        (EventKind::ToolFinished, json!({"success":false})),
        (EventKind::ToolFinished, json!({"content":{"error":"boom"}})),
        (
            EventKind::ActionProposed,
            json!({"action":"run_shell","cmd":"which python"}),
        ),
    ];
    for (index, (kind, payload)) in payloads.into_iter().enumerate() {
        state
            .store
            .append_event(&AgentEvent {
                event_id: EventId::new(format!("evt_operator_summary_{index}")),
                session_id: session_id.clone(),
                run_id: run_id.clone(),
                seq: index as u64 + 1,
                kind,
                payload,
            })
            .await
            .unwrap();
    }
    for (index, path) in [
        "src/validation.py",
        "tests/test_validation.py",
        "src/validation.py",
    ]
    .into_iter()
    .enumerate()
    {
        state
            .store
            .create_file_change(CreateFileChange {
                id: format!("fchange_operator_summary_{index}"),
                session_id: session_id.clone(),
                run_id: run_id.clone(),
                path: path.to_string(),
                before_hash: None,
                after_hash: Some("hash".to_string()),
                diff: "diff".to_string(),
            })
            .await
            .unwrap();
    }

    let Json(summary) = get_run_operator_summary(State(state), Path(run_id.to_string()))
        .await
        .unwrap();
    assert_eq!(summary.tools_failed, 3);
    assert_eq!(summary.test_commands, vec![unit.to_string()]);
    assert!(!summary
        .test_commands
        .iter()
        .any(|command| command.contains("mkdir")));
    assert_eq!(
        summary.changed_paths,
        vec![
            "src/validation.py".to_string(),
            "tests/test_validation.py".to_string()
        ]
    );
    assert_eq!(summary.environment_discovery_turns, 1);
}

#[test]
fn production_rollback_gate_uses_its_bound_lifecycle_action() {
    assert!(approval_gate_uses_dedicated_lifecycle_action(
        "production_rollback"
    ));
    assert!(!approval_gate_uses_dedicated_lifecycle_action(
        "cluster_mutation"
    ));
}

#[tokio::test]
async fn rollback_writer_and_observer_stay_bound_to_the_captured_digest_and_manual_merge() {
    let state =
        test_state_with_git_observer("/bin/true".to_string(), PROTECTED_GITOPS_REPO.to_string())
            .await;
    let session_id = SessionId::new("ses_rollback_contract");
    let run_id = RunId::new("run_rollback_contract");
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: "rollback contract".to_string(),
            cwd: ".".to_string(),
        })
        .await
        .unwrap();
    state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: "rollback contract".to_string(),
            cwd: ".".to_string(),
            max_turns: 1,
            initial_status: "completed".to_string(),
            execution_target_json: json!({}),
        })
        .await
        .unwrap();
    let request = protected_production_request();
    state
        .store
        .create_work_item(CreateWorkItem {
            id: "witem_rollback_contract".to_string(),
            status: "executing".to_string(),
            title: request.title,
            intent: request.intent,
            acceptance_criteria: request.acceptance_criteria,
            source_repo: request.source_repo,
            source_ref: request.source_ref,
            source_commit: request.source_commit,
            pipeline_contract_id: request.pipeline_contract_id,
            deployment_contract_id: request.deployment_contract_id,
            gitops_repo: request.gitops_repo,
            gitops_ref: request.gitops_ref,
            gitops_kustomization_path: request.gitops_kustomization_path,
            gitops_image_name: request.gitops_image_name,
            target_environment: request.target_environment,
            target_namespace: request.target_namespace,
            argo_application: request.argo_application,
            workload_kind: request.workload_kind,
            workload_name: request.workload_name,
            rollback_owner: request.rollback_owner,
            production_impacting: true,
            max_attempts: 1,
            max_elapsed_seconds: 3_600,
            environment_profile_id: None,
            run_budget: Default::default(),
            repository_contract_json: None,
            repository_contract_hash: None,
            environment_preparation_status: "not_required".to_string(),
            created_by: request.actor,
        })
        .await
        .unwrap();
    state
        .store
        .start_work_item_attempt(
            "witem_rollback_contract",
            &run_id,
            Some("tester".to_string()),
            Some("seed rollback contract".to_string()),
        )
        .await
        .unwrap();
    state
        .store
        .create_work_plan(CreateWorkPlan {
            id: "wplan_rollback_contract".to_string(),
            work_item_id: Some("witem_rollback_contract".to_string()),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Rollback contract plan".to_string(),
            summary: "Reviewed protected production delivery".to_string(),
            risk_level: "critical".to_string(),
            requires_approval: true,
            resource_namespace: Some(PROTECTED_NAMESPACE.to_string()),
            resource_kind: Some("Application".to_string()),
            resource_name: Some(PROTECTED_ARGO_APPLICATION.to_string()),
            work_plan_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_change_set(CreateChangeSet {
            id: "cset_rollback_contract".to_string(),
            work_item_id: Some("witem_rollback_contract".to_string()),
            work_plan_id: "wplan_rollback_contract".to_string(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "applied".to_string(),
            title: "Rollback source ChangeSet".to_string(),
            summary: "Merged protected source change".to_string(),
            risk_level: "critical".to_string(),
            material_hash: "rollback_contract_hash".to_string(),
            resource_namespace: Some(PROTECTED_NAMESPACE.to_string()),
            resource_kind: Some("Application".to_string()),
            resource_name: Some(PROTECTED_ARGO_APPLICATION.to_string()),
            change_set_json: json!({}),
        })
        .await
        .unwrap();
    state
            .store
            .create_pipeline_intent(CreatePipelineIntent {
                id: "pint_rollback_contract".to_string(),
                change_set_id: "cset_rollback_contract".to_string(),
                work_plan_id: "wplan_rollback_contract".to_string(),
                remediation_plan_id: None,
                incident_id: None,
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                status: "completed".to_string(),
                title: "Rollback source pipeline".to_string(),
                summary: "Verified protected source build".to_string(),
                risk_level: "critical".to_string(),
                intent_kind: "tekton_build_test_package".to_string(),
                resource_namespace: Some("tekton-pipelines".to_string()),
                resource_kind: Some("Pipeline".to_string()),
                resource_name: Some("pharness-yfinance-build".to_string()),
                intent_json: json!({
                    "source_provenance": { "merge_commit_sha": "c1b2c3d4e5f60718293a4b5c6d7e8f9012345678" }
                }),
            })
            .await
            .unwrap();
    state
        .store
        .create_deployment_intent(CreateDeploymentIntent {
            id: "dint_rollback_contract".to_string(),
            pipeline_intent_id: "pint_rollback_contract".to_string(),
            change_set_id: "cset_rollback_contract".to_string(),
            work_plan_id: "wplan_rollback_contract".to_string(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Protected deployment".to_string(),
            summary: "Deploy the verified yfinance image".to_string(),
            risk_level: "critical".to_string(),
            intent_kind: "argo_sync".to_string(),
            target_environment: Some(PROTECTED_ENVIRONMENT.to_string()),
            target_namespace: Some(PROTECTED_NAMESPACE.to_string()),
            argo_application: Some(PROTECTED_ARGO_APPLICATION.to_string()),
            resource_namespace: Some(PROTECTED_NAMESPACE.to_string()),
            resource_kind: Some(PROTECTED_WORKLOAD_KIND.to_string()),
            resource_name: Some(PROTECTED_WORKLOAD_NAME.to_string()),
            intent_json: json!({}),
        })
        .await
        .unwrap();
    let deployment_gitops_merge = "e1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
    let gitops_material_hash = "rollback_gitops_material";
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_rollback_gitops_update".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "gitops_update_plan".to_string(),
            label: "Protected GitOps update".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({})),
        })
        .await
        .unwrap();
    let gitops_change_set = state
        .store
        .create_gitops_change_set(CreateGitOpsChangeSet {
            id: "gcset_rollback_contract".to_string(),
            work_item_id: "witem_rollback_contract".to_string(),
            work_plan_id: "wplan_rollback_contract".to_string(),
            source_change_set_id: "cset_rollback_contract".to_string(),
            pipeline_intent_id: "pint_rollback_contract".to_string(),
            deployment_intent_id: "dint_rollback_contract".to_string(),
            gitops_update_plan_artifact_id: "art_rollback_gitops_update".to_string(),
            session_id: session_id.clone(),
            run_id: run_id.clone(),
            status: "approved".to_string(),
            title: "Protected GitOps ChangeSet".to_string(),
            summary: "Set the verified yfinance digest".to_string(),
            risk_level: "critical".to_string(),
            material_hash: gitops_material_hash.to_string(),
            gitops_repo: PROTECTED_GITOPS_REPO.to_string(),
            gitops_ref: "main".to_string(),
            head_branch: "pharness/yfinance/gitops".to_string(),
            kustomization_path: PROTECTED_KUSTOMIZATION_PATH.to_string(),
            image_name: PROTECTED_IMAGE_NAME.to_string(),
            image_ref: format!("{}@sha256:{}", PROTECTED_IMAGE_NAME, "f".repeat(64)),
            gitops_change_set_json: json!({}),
        })
        .await
        .unwrap();
    state.store.create_artifact(CreateArtifact { id: "art_rollback_gitops_base".to_string(), session_id: session_id.clone(), run_id: Some(run_id.clone()), kind: "gitops_base_revision".to_string(), label: "Protected GitOps base".to_string(), mime_type: Some("application/json".to_string()), path: None, content_text: None, content_json: Some(json!({ "status": "resolved", "gitops_change_set_id": gitops_change_set.id, "material_hash": gitops_change_set.material_hash, "repository": PROTECTED_GITOPS_REPO, "base_ref": "main", "base_commit": "d1b2c3d4e5f60718293a4b5c6d7e8f9012345678" })) }).await.unwrap();
    state.store.create_artifact(CreateArtifact { id: "art_rollback_gitops_plan".to_string(), session_id: session_id.clone(), run_id: Some(run_id.clone()), kind: "gitops_delivery_plan".to_string(), label: "Protected GitOps delivery plan".to_string(), mime_type: Some("application/json".to_string()), path: None, content_text: None, content_json: Some(json!({ "gitops_change_set": { "id": gitops_change_set.id, "revision": gitops_change_set.revision, "material_hash": gitops_change_set.material_hash }, "source": { "base_revision_artifact_id": "art_rollback_gitops_base" } })) }).await.unwrap();
    state.store.create_artifact(CreateArtifact { id: "art_rollback_gitops_merge".to_string(), session_id: session_id.clone(), run_id: Some(run_id.clone()), kind: "gitops_delivery_merge".to_string(), label: "Protected GitOps merge".to_string(), mime_type: Some("application/json".to_string()), path: None, content_text: None, content_json: Some(json!({ "gitops_change_set_id": gitops_change_set.id, "gitops_delivery_plan_artifact_id": "art_rollback_gitops_plan", "merge_commit_sha": deployment_gitops_merge })) }).await.unwrap();
    let rollback_id = "rollback_contract";
    let baseline_digest = format!("sha256:{}", "d".repeat(64));
    let base_commit = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
    state
        .store
        .create_approval_gate(CreateApprovalGate {
            id: "agate_rollback_contract".to_string(),
            work_item_id: Some("witem_rollback_contract".to_string()),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "pending".to_string(),
            gate_kind: "production_rollback".to_string(),
            gate_order: 90,
            title: "Approve rollback".to_string(),
            summary: "Restore captured digest".to_string(),
            risk_level: "critical".to_string(),
            resource_namespace: Some(PROTECTED_NAMESPACE.to_string()),
            resource_kind: Some(PROTECTED_WORKLOAD_KIND.to_string()),
            resource_name: Some(PROTECTED_WORKLOAD_NAME.to_string()),
            gate_json: json!({
                "rollback_intent_id": rollback_id,
                "baseline_digest": baseline_digest,
                "argo_application": PROTECTED_ARGO_APPLICATION,
            }),
        })
        .await
        .unwrap();
    state.store.create_artifact(CreateArtifact { id: "art_rollback_intent_contract".to_string(), session_id: session_id.clone(), run_id: Some(run_id.clone()), kind: "rollback_intent".to_string(), label: "RollbackIntent contract".to_string(), mime_type: Some("application/json".to_string()), path: None, content_text: None, content_json: Some(json!({ "rollback_intent_id": rollback_id, "work_item_id": "witem_rollback_contract", "status": "prepared", "approval_gate_id": "agate_rollback_contract", "baseline": { "image_digest": baseline_digest, "gitops_revision": base_commit } })) }).await.unwrap();
    state
        .store
        .finish_work_item_attempt(
            "witem_rollback_contract",
            "awaiting_approval",
            Some("tester".to_string()),
            Some("coding completed before delivery and rollback planning".to_string()),
        )
        .await
        .unwrap();
    let completed_attempt_item = state
        .store
        .get_work_item("witem_rollback_contract")
        .await
        .unwrap()
        .unwrap();
    assert!(completed_attempt_item.current_run_id.is_none());
    assert!(
        latest_rollback_intent(&state, &completed_attempt_item, Some(rollback_id),)
            .await
            .unwrap()
            .is_some()
    );
    let rollback_gate = state
        .store
        .get_approval_gate("agate_rollback_contract")
        .await
        .unwrap()
        .unwrap();
    let (generic_actionable, generic_blocker) =
        approval_gate_lifecycle_readiness(&state, &rollback_gate)
            .await
            .unwrap();
    assert!(!generic_actionable);
    assert!(generic_blocker.contains("RollbackIntent approval action"));
    state
        .store
        .decide_approval_gate(
            "agate_rollback_contract",
            "satisfied",
            Some("lucas".to_string()),
            Some("satisfied through the generic gate before approval".to_string()),
        )
        .await
        .unwrap();
    let prepared = latest_rollback_intent(&state, &completed_attempt_item, Some(rollback_id))
        .await
        .unwrap()
        .unwrap();
    let Json(writer_approved) = execute_work_item_action(
        State(state.clone()),
        None,
        Path((
            "witem_rollback_contract".to_string(),
            "approve_rollback".to_string(),
        )),
        Json(ExecuteWorkItemActionRequest {
            actor: Some("lucas".to_string()),
            reason: "approve exact rollback writer".to_string(),
            state_hash: format!("{:x}", Sha256::digest(prepared.to_string().as_bytes())),
            inference_policies: None,
            execution_policies: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        writer_approved.pointer("/content/status"),
        Some(&json!("approved"))
    );
    assert_eq!(
        writer_approved.pointer("/content/rollback_base_commit"),
        Some(&json!(deployment_gitops_merge))
    );
    assert!(writer_approved
        .pointer("/content/authorization_expires_at")
        .and_then(Value::as_str)
        .is_some());
    let wrong_action = execute_work_item_action(
        State(state.clone()),
        None,
        Path((
            "witem_rollback_contract".to_string(),
            "execute_rollback_argo_sync".to_string(),
        )),
        Json(ExecuteWorkItemActionRequest {
            actor: Some("lucas".to_string()),
            reason: "attempt a stale lifecycle action".to_string(),
            state_hash: format!(
                "{:x}",
                Sha256::digest(writer_approved.to_string().as_bytes())
            ),
            inference_policies: None,
            execution_policies: None,
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(wrong_action.status, StatusCode::CONFLICT);
    let writer_execution = "rbexec_contract";
    let head_branch = "pharness/rollback-contract";
    state.store.create_artifact(CreateArtifact { id: "art_rollback_delivery_execution_contract".to_string(), session_id: session_id.clone(), run_id: Some(run_id.clone()), kind: "rollback_delivery_execution".to_string(), label: "Rollback delivery".to_string(), mime_type: Some("application/json".to_string()), path: None, content_text: None, content_json: Some(json!({ "rollback_intent_id": rollback_id, "execution_id": writer_execution, "status": "dispatched", "context": { "execution_id": writer_execution, "repository": PROTECTED_GITOPS_REPO, "base_ref": "main", "base_commit": deployment_gitops_merge, "head_branch": head_branch, "kustomization_path": PROTECTED_KUSTOMIZATION_PATH, "image_name": PROTECTED_IMAGE_NAME, "image_ref": format!("{}@{}", PROTECTED_IMAGE_NAME, baseline_digest), "commit_subject": "rollback yfinance", "commit_body": "restore captured digest", "pull_request_title": "rollback yfinance", "pull_request_body": "manual merge required", "github_api_url": "https://api.github.com", "author_name": "Pharness", "author_email": "pharness@example.test" } })) }).await.unwrap();
    let Json(context) = internal_rollback_delivery_context(&state, rollback_id, writer_execution)
        .await
        .unwrap();
    assert_eq!(
        context.image_ref,
        format!("{}@{}", PROTECTED_IMAGE_NAME, baseline_digest)
    );
    assert_eq!(context.base_commit, deployment_gitops_merge);
    let Json(result) = internal_rollback_delivery_outcome(
        &state,
        rollback_id,
        GitOpsDeliveryOutcomeRequest {
            execution_id: writer_execution.to_string(),
            status: "completed".to_string(),
            error_code: None,
            branch: Some(head_branch.to_string()),
            commit_sha: Some(base_commit.to_string()),
            pull_request_url: Some(
                "https://github.com/lward27/lucas_engineering/pull/42".to_string(),
            ),
            pull_request_number: Some(42),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        result
            .content_json
            .as_ref()
            .and_then(|value| value.get("status")),
        Some(&json!("completed"))
    );

    let observer_execution = "rbobs_contract";
    state.store.create_artifact(CreateArtifact { id: "art_rollback_observation_execution_contract".to_string(), session_id: session_id.clone(), run_id: Some(run_id.clone()), kind: "rollback_delivery_observation_execution".to_string(), label: "Rollback observation".to_string(), mime_type: Some("application/json".to_string()), path: None, content_text: None, content_json: Some(json!({ "rollback_intent_id": rollback_id, "execution_id": observer_execution, "status": "dispatched", "source": { "repository": PROTECTED_GITOPS_REPO, "head_branch": head_branch, "source_commit_sha": base_commit, "pull_request_url": "https://github.com/lward27/lucas_engineering/pull/42", "pull_request_number": 42 } })) }).await.unwrap();
    let merge_commit = "b1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
    let _ = internal_rollback_delivery_observation_outcome(
        &state,
        rollback_id,
        GitOpsDeliveryObservationOutcomeRequest {
            execution_id: observer_execution.to_string(),
            status: "observed".to_string(),
            error_code: None,
            pull_request_state: Some("closed".to_string()),
            merged: Some(true),
            merge_commit_sha: Some(merge_commit.to_string()),
            head_branch: Some(head_branch.to_string()),
            head_commit_sha: Some(base_commit.to_string()),
        },
    )
    .await
    .unwrap();
    let item = state
        .store
        .get_work_item("witem_rollback_contract")
        .await
        .unwrap()
        .unwrap();
    let latest = latest_rollback_intent(&state, &item, Some(rollback_id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        latest.pointer("/content/status"),
        Some(&json!("ready_for_argo_sync"))
    );
    assert_eq!(
        latest.pointer("/content/gitops_merge_sha"),
        Some(&json!(merge_commit))
    );

    let Json(approved) = approve_rollback_intent(
        State(state.clone()),
        None,
        Path(rollback_id.to_string()),
        Json(RollbackIntentRequest {
            actor: Some("lucas".to_string()),
            reason: "approve exact rollback Argo sync".to_string(),
            expires_at: Some((current_millis() + 60_000).to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        approved.pointer("/content/status"),
        Some(&json!("argo_approved"))
    );
    assert_eq!(
        approved.pointer("/content/gitops_merge_sha"),
        Some(&json!(merge_commit))
    );
    assert_eq!(
        approved.pointer("/content/rollback_base_commit"),
        Some(&json!(deployment_gitops_merge))
    );
    let Json(preflight) =
        preflight_rollback_intent(State(state.clone()), Path(rollback_id.to_string()))
            .await
            .unwrap();
    assert_eq!(preflight["status"], "ready_for_argo");
    assert_eq!(preflight["exact_binding"], true);
    assert_eq!(preflight["argo_grant_fresh"], true);

    let argo_execution_id = "rbaexec_contract";
    state.store.create_artifact(CreateArtifact { id: "art_rollback_argo_execution_contract".to_string(), session_id: session_id.clone(), run_id: Some(run_id.clone()), kind: "rollback_argo_sync_execution".to_string(), label: "Rollback Argo execution".to_string(), mime_type: Some("application/json".to_string()), path: None, content_text: None, content_json: Some(json!({ "rollback_intent_id": rollback_id, "execution_id": argo_execution_id, "status": "dispatched", "permission_grant_id": approved.pointer("/content/argo_permission_grant_id"), "deployment_contract_id": "dcontract_yfinance", "gitops_merge_sha": merge_commit, "baseline_digest": baseline_digest, "target": protected_target_json() })) }).await.unwrap();
    let wrong_revision = internal_rollback_argo_sync_outcome(
        &state,
        rollback_id,
        ArgoSyncOutcomeRequest {
            execution_id: argo_execution_id.to_string(),
            status: "completed".to_string(),
            sync_status: Some("Synced".to_string()),
            health_status: Some("Healthy".to_string()),
            operation_phase: Some("Succeeded".to_string()),
            revision: Some(base_commit.to_string()),
            error_code: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(wrong_revision.status, StatusCode::CONFLICT);
    let outcome = ArgoSyncOutcomeRequest {
        execution_id: argo_execution_id.to_string(),
        status: "completed".to_string(),
        sync_status: Some("Synced".to_string()),
        health_status: Some("Healthy".to_string()),
        operation_phase: Some("Succeeded".to_string()),
        revision: Some(merge_commit.to_string()),
        error_code: None,
    };
    let Json(first) = internal_rollback_argo_sync_outcome(&state, rollback_id, outcome)
        .await
        .unwrap();
    let Json(second) = internal_rollback_argo_sync_outcome(
        &state,
        rollback_id,
        ArgoSyncOutcomeRequest {
            execution_id: argo_execution_id.to_string(),
            status: "completed".to_string(),
            sync_status: Some("Synced".to_string()),
            health_status: Some("Healthy".to_string()),
            operation_phase: Some("Succeeded".to_string()),
            revision: Some(merge_commit.to_string()),
            error_code: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(first.id, second.id);
}
