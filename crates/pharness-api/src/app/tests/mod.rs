mod support;

use super::{
    advance_work_item, approval_gate_summary, approval_gates_from_work_item, approval_summary,
    attach_deployment_intent_evidence, attach_pipeline_intent_evidence, attach_release_evidence,
    authorize_change_set_git_delivery, authorize_gitops_change_set_delivery,
    block_work_item_from_delivery_failure, build_pipeline_run_manifest, cancel_run,
    cancel_work_item, capability_preflight_is_statically_unavailable,
    capability_verification_summary, change_set_flow, change_set_readiness,
    coding_run_scope_matches_source, complete_work_item_from_verified_release, config_effective,
    create_change_set, create_change_set_trusted_envelope, create_declared_deployment_handoff,
    create_deployment_contract, create_deployment_intent_from_pipeline_intent,
    create_deployment_intent_trusted_envelope, create_incident, create_observation,
    create_operator_run, create_pipeline_intent_from_change_set,
    create_registry_evidence_from_registry_inspection, create_registry_evidence_from_release,
    create_release_from_deployment_intent, create_remediation_plan, create_run, create_work_item,
    create_work_item_pipeline_intent, create_work_plan_from_remediation_plan,
    create_work_plan_from_work_item, create_work_plan_trusted_envelope,
    current_pipeline_build_output, decide_run_approval, deny_approval,
    deployment_intent_reconcile_action, ensure_pipeline_evidence_ready_for_deployment,
    environment_profile_readiness_blocker, execute_capability, execute_deployment_intent,
    execute_work_item_action, execution_matches_pipeline_contract, get_approval, get_approval_gate,
    get_artifact, get_deployment_contract, get_deployment_intent, get_incident, get_observation,
    get_permission_grant, get_pipeline_intent, get_registry_evidence, get_release,
    get_remediation_plan, get_run, get_run_diff, get_run_events, get_run_operator_summary,
    get_work_plan, git_delivery_reconcile_action, gitops_artifact_change_set_revision,
    gitops_change_set_reconcile_action, gitops_delivery_flow, gitops_observation_closed_unmerged,
    internal_argo_sync_outcome, internal_gitops_delivery_observation_outcome,
    internal_gitops_delivery_outcome, internal_workspace_provisioned, last_event_seq,
    list_approval_gates, list_approvals, list_audit_events, list_change_sets,
    list_deployment_contracts, list_deployment_intents, list_incidents, list_observations,
    list_permission_grants, list_pipeline_intents, list_registry_evidence, list_releases,
    list_remediation_plans, list_run_artifacts, list_run_observations, list_runs,
    list_work_item_controller_waits, list_work_item_events, list_work_items, list_work_plans,
    list_workspaces, merge_pipeline_execution_state, observe_due_controller_wait,
    observed_gitops_merge_for_deployment, parse_last_event_id, persist_pipeline_build_output,
    persist_pipeline_execution_evidence, persist_pipeline_run_analysis,
    pipeline_build_output_from_analysis, pipeline_intent_execution_preflight,
    pipeline_intent_is_gitops_update_eligible, pipeline_intent_reconcile_action, policy_json,
    preflight_change_set_git_delivery, preflight_deployment_intent,
    preflight_gitops_change_set_delivery, prepare_change_set_git_delivery,
    prepare_gitops_change_set_delivery, reconcile_due_controller_waits, reconcile_work_item,
    release_reconcile_action, release_workload_verification_action, replan_work_item,
    required_baseline_capability_result, revise_change_set, revise_work_plan,
    revoke_permission_grant, router, run_policy, run_summary, satisfy_approval_gate,
    schedule_controller_wait, set_pipeline_intent_evidence, stream_start_seq,
    supersede_active_controller_wait_if_present, tekton_execution_spec, transition_change_set,
    transition_deployment_contract, transition_deployment_intent, transition_pipeline_intent,
    transition_registry_evidence, transition_release, transition_remediation_plan,
    transition_work_item, transition_work_plan, unique_suffix, validate_permission_grant_request,
    validate_pipeline_deployment_handoff, validate_terminal_pipeline_run_analysis, verify_release,
    work_item_flow, work_item_pipeline_intent_context, work_plan_flow, work_plan_readiness,
    AppState, ApprovalGateSummaryQuery, ApprovalSummaryQuery, DeploymentIntentExecutionPreflight,
    GitDeliveryFlowResponse, GitOpsBaseRevisionReconcileState, GitOpsDeliveryFlowResponse,
    InternalWorkspaceProvisionedRequest, ListApprovalGatesQuery, ListApprovalsQuery,
    ListAuditEventsQuery, ListChangeSetsQuery, ListControllerWaitsQuery,
    ListDeploymentContractsQuery, ListDeploymentIntentsQuery, ListIncidentsQuery,
    ListObservationsQuery, ListPermissionGrantsQuery, ListPipelineIntentsQuery,
    ListRegistryEvidenceQuery, ListReleasesQuery, ListRemediationPlansQuery, ListRunsQuery,
    ListWorkItemsQuery, ListWorkPlansQuery, ListWorkspacesQuery, OperatorIdentity,
    PipelineDeploymentHandoffSpec, StreamRunEventsQuery, WorkItemPipelineContextQuery,
    WorkItemReconcileAction, CONTROLLER_WAIT_MAX_CHECKS, GIT_DELIVERY_ACTIONS,
};
use crate::dispatch::{KubernetesJobDispatcher, RunDispatcher};
use crate::dto::{
    AdvanceWorkItemRequest, ApprovalDecision, ArgoSyncOutcomeRequest, ArtifactResponse,
    AttachDeploymentIntentEvidenceRequest, AttachPipelineIntentEvidenceRequest,
    AttachReleaseEvidenceRequest, CreateChangeSetRequest, CreateDeploymentContractRequest,
    CreateDeploymentIntentFromPipelineIntentRequest, CreateDeploymentIntentTrustedEnvelopeRequest,
    CreateGitDeliveryAuthorizationRequest, CreateGitOpsDeliveryAuthorizationRequest,
    CreateIncidentRequest, CreateObservationRequest, CreatePermissionGrantRequest,
    CreatePipelineIntentFromChangeSetRequest, CreateRegistryEvidenceFromInspectionRequest,
    CreateRegistryEvidenceFromReleaseRequest, CreateReleaseFromDeploymentIntentRequest,
    CreateRemediationPlanRequest, CreateRunRequest, CreateTrustedEnvelopeRequest,
    CreateWorkItemPipelineIntentRequest, CreateWorkItemRequest,
    CreateWorkPlanFromRemediationPlanRequest, DecideApprovalGateRequest, DecideApprovalRequest,
    DeploymentIntentDeliveryFlowResponse, DeploymentIntentPreflightRequest,
    ExecuteCapabilityRequest, ExecuteCapabilityResponse, ExecuteDeploymentIntentRequest,
    ExecuteWorkItemActionRequest, GitDeliveryPreflightRequest,
    GitOpsDeliveryObservationOutcomeRequest, GitOpsDeliveryOutcomeRequest,
    GitOpsDeliveryPreflightRequest, PipelineIntentExecutionOutcomeRequest,
    PrepareGitDeliveryRequest, PrepareGitOpsDeliveryRequest, ReconcileDueControllerWaitsRequest,
    ReconcileWorkItemRequest, ReleaseResponse, ReplanWorkItemRequest, ReviewApprovalRequest,
    ReviseChangeSetRequest, ReviseWorkPlanRequest, RevokePermissionGrantRequest,
    TransitionChangeSetRequest, TransitionDeploymentContractRequest,
    TransitionDeploymentIntentRequest, TransitionPipelineIntentRequest,
    TransitionRegistryEvidenceRequest, TransitionReleaseRequest, TransitionRemediationPlanRequest,
    TransitionWorkItemRequest, TransitionWorkPlanRequest, VerifyReleaseRequest,
};
use crate::workspace::WorkspaceProvisioner;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::{Extension, Json};
use pharness_config::WorkerKubernetesConfig;
use pharness_core::{
    AgentAction, AgentEvent, EventId, EventKind, PolicyDecision, PolicyMode, ReadOnlyClusterTools,
    RiskLevel, RunId, RunScope, SafetyPolicy, SessionId,
};
use pharness_store::{
    ApprovalGateListFilter, ApprovalGateSummaryFilter, CreateApproval, CreateApprovalGate,
    CreateArtifact, CreateChangeSet, CreateControllerWait, CreateDeploymentIntent,
    CreateFileChange, CreateGitOpsChangeSet, CreateIncident, CreateObservation,
    CreatePipelineContract, CreatePipelineIntent, CreateRelease, CreateRemediationPlan, CreateRun,
    CreateSession, CreateWorkItem, CreateWorkPlan, CreateWorkspace, ObservationListFilter,
    SqliteStore, StoredDeploymentContract, StoredDeploymentIntent, StoredGitOpsChangeSet,
    StoredPipelineContract, StoredPipelineIntent, StoredRelease,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;

#[test]
fn v3_characterization_fixture_matches_frozen_constants() {
    use self::support as baseline;

    let fixture = baseline::v3_characterization_fixture();
    assert_eq!(fixture["release_commit"], baseline::V3_RELEASE_COMMIT);
    assert_eq!(fixture["source_revision"], baseline::V3_SOURCE_REVISION);
    assert_eq!(fixture["runtime_digest"], baseline::V3_RUNTIME_DIGEST);
    assert_eq!(fixture["ui_digest"], baseline::V3_UI_DIGEST);
    assert_eq!(fixture["runner_digest"], baseline::V3_RUNNER_DIGEST);
    assert_eq!(fixture["work_item_id"], baseline::V3_WORK_ITEM_ID);
    assert_eq!(fixture["run_id"], baseline::V3_RUN_ID);
    assert_eq!(fixture["release_id"], baseline::V3_RELEASE_ID);
    assert_eq!(
        fixture["rollback_intent_id"],
        baseline::V3_ROLLBACK_INTENT_ID
    );
    assert_eq!(
        fixture["running_yfinance_digest"],
        baseline::V3_RUNNING_YFINANCE_DIGEST
    );
    assert_eq!(
        fixture["rollback_baseline_digest"],
        baseline::V3_ROLLBACK_BASELINE_DIGEST
    );
}

fn materialize_inventory_path(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            if segment.starts_with(':') {
                "fixture"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[tokio::test]
async fn route_inventory_matches_mounted_routes_and_auth_classes() {
    use self::support::{route_inventory, routes_mounted_in_source, RouteAuthClass};
    use tower::ServiceExt;

    let mut inventory = route_inventory();
    inventory.sort();
    assert_eq!(
        inventory.len(),
        166,
        "update the checked-in inventory only after reviewing an intentional route change"
    );
    assert_eq!(
        routes_mounted_in_source(),
        inventory,
        "checked-in route inventory differs from the source registrations"
    );
    assert!(inventory
        .iter()
        .any(|entry| entry.path == "/health" && !entry.path.contains(':')));
    assert!(inventory.iter().any(|entry| entry.path.contains(':')));
    assert!(inventory
        .iter()
        .any(|entry| entry.auth_class == RouteAuthClass::Operator));
    assert!(inventory
        .iter()
        .any(|entry| entry.auth_class == RouteAuthClass::Worker));

    let app = router(
        Arc::new(SqliteStore::connect_in_memory().await.unwrap()),
        RunDispatcher::Disabled,
        ReadOnlyClusterTools::default(),
        SafetyPolicy::default(),
        Some("worker-secret".to_string()),
        vec![("lucas".to_string(), "operator-secret".to_string())],
        WorkspaceProvisioner::new(std::env::temp_dir(), Vec::new()),
    );

    for entry in inventory {
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method(axum::http::Method::from_bytes(entry.method.as_bytes()).unwrap())
                    .uri(materialize_inventory_path(&entry.path))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let expected = match entry.auth_class {
            RouteAuthClass::Open => StatusCode::OK,
            RouteAuthClass::Operator | RouteAuthClass::Worker => StatusCode::UNAUTHORIZED,
        };
        assert_eq!(
            response.status(),
            expected,
            "{} {} did not mount with {:?} authentication",
            entry.method,
            entry.path,
            entry.auth_class
        );
    }

    let not_found = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/decomposition-route-that-does-not-exist")
                .header("authorization", "Bearer operator-secret")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(not_found.status(), StatusCode::NOT_FOUND);
}

#[test]
fn gitops_artifact_revision_keeps_legacy_revision_one_but_not_retry_evidence() {
    assert_eq!(gitops_artifact_change_set_revision(&json!({})), 1);
    assert_eq!(
        gitops_artifact_change_set_revision(&json!({
            "gitops_change_set_revision": 2
        })),
        2
    );
    assert_ne!(gitops_artifact_change_set_revision(&json!({})), 2);
}

#[test]
fn recognizes_only_a_closed_unmerged_gitops_observation_as_retryable() {
    assert!(gitops_observation_closed_unmerged(Some(&json!({
        "status": "observed",
        "pull_request_state": "closed",
        "merged": false,
    }))));
    assert!(!gitops_observation_closed_unmerged(Some(&json!({
        "status": "observed",
        "pull_request_state": "open",
        "merged": false,
    }))));
    assert!(!gitops_observation_closed_unmerged(Some(&json!({
        "status": "observed",
        "pull_request_state": "closed",
        "merged": true,
    }))));
}

#[test]
fn refreshes_only_retryable_gitops_observations() {
    assert!(super::gitops_observation_refreshable(Some(&json!({
        "status": "observed",
        "pull_request_state": "open",
        "merged": false,
    }))));
    assert!(super::gitops_observation_refreshable(Some(&json!({
        "status": "failed",
    }))));
    assert!(!super::gitops_observation_refreshable(Some(&json!({
        "status": "observed",
        "pull_request_state": "closed",
        "merged": false,
    }))));
    assert!(!super::gitops_observation_refreshable(Some(&json!({
        "status": "observed",
        "pull_request_state": "closed",
        "merged": true,
    }))));
    assert!(!super::gitops_observation_refreshable(None));
}

#[test]
fn release_verification_uses_the_pinned_contract_workload() {
    let intent = reconcile_deployment_intent();
    assert_eq!(intent.resource_kind.as_deref(), Some("Application"));
    let contract = StoredDeploymentContract {
        id: "dcontract_release_workload".to_string(),
        status: "active".to_string(),
        target_environment: "production".to_string(),
        target_namespace: "apps-prod".to_string(),
        argo_application: "yfinance-wrapper".to_string(),
        version: "yfinance-v1".to_string(),
        contract_json: json!({
            "operation": "sync",
            "prune": false,
            "force": false,
            "workload_kind": "Deployment",
            "workload_name": "yfinance-wrapper"
        }),
        created_at: "1".to_string(),
        updated_at: "1".to_string(),
        status_changed_at: "1".to_string(),
        status_changed_by: Some("lucas".to_string()),
        status_reason: Some("bind exact workload".to_string()),
    };

    let action =
        release_workload_verification_action(&intent, Some(&contract), "rel_contract_workload")
            .unwrap();

    match action {
        AgentAction::KubernetesGet {
            resource,
            namespace,
            name,
            ..
        } => {
            assert_eq!(resource, "deployments");
            assert_eq!(namespace.as_deref(), Some("apps-prod"));
            assert_eq!(name.as_deref(), Some("yfinance-wrapper"));
        }
        other => panic!("expected KubernetesGet, got {other:?}"),
    }
}

#[test]
fn release_inventory_gate_requires_collection_not_unrelated_global_health() {
    let inventory = json!({
        "inventory": {
            "targets": {
                "status": "success",
                "active_count": 35,
                "unhealthy_count": 3
            },
            "rules": {
                "status": "success",
                "problem_rule_count": 0
            },
            "alerts": {
                "status": "success",
                "alert_count": 0
            }
        }
    });

    assert!(super::release_prometheus_inventory_collected(&inventory));
    assert_eq!(
        super::prometheus_inventory_observability_status(&inventory),
        "attention_required"
    );
    assert!(
        super::release_prometheus_inventory_summary(&inventory).contains("3 unhealthy target(s)")
    );

    let missing_rules = json!({
        "inventory": {
            "targets": { "status": "success" },
            "alerts": { "status": "success" }
        }
    });
    assert!(!super::release_prometheus_inventory_collected(
        &missing_rules
    ));
}

#[test]
fn failed_capability_verification_can_retry_but_static_unavailability_cannot() {
    let static_unavailable = super::CapabilityStatusResponse {
        capability: "gitops_writer".to_string(),
        status: "unavailable".to_string(),
        summary: "GitOps writer is not configured".to_string(),
        verified_at: None,
        expires_at: None,
    };
    assert!(capability_preflight_is_statically_unavailable(
        &static_unavailable
    ));

    let failed_verification = super::CapabilityStatusResponse {
        verified_at: Some("1787134555765".to_string()),
        expires_at: Some("1787135455765".to_string()),
        summary: "Isolated identity did not verify repository_push".to_string(),
        ..static_unavailable
    };
    assert!(!capability_preflight_is_statically_unavailable(
        &failed_verification
    ));
}

async fn test_state() -> AppState {
    AppState {
        store: Arc::new(SqliteStore::connect_in_memory().await.unwrap()),
        worker: RunDispatcher::Disabled,
        cluster_tools: ReadOnlyClusterTools::default(),
        policy: SafetyPolicy::default(),
        worker_token: None,
        operator_tokens: Arc::new(Vec::new()),
        workspace: WorkspaceProvisioner::new(std::env::temp_dir(), Vec::new()),
        build: super::BuildMetadata::from_env(),
        protected_target: super::ProtectedTargetConfiguration::from_env(),
        environment_profiles: Arc::new(Vec::new()),
    }
}

#[test]
fn coding_run_scope_requires_matching_production_classification() {
    let scope = RunScope {
        run_id: Some("run_1".to_string()),
        namespace: Some("apps-prod".to_string()),
        repo: Some("https://github.com/lward27/yfinance_wrapper.git".to_string()),
        branch: Some("pharness/witem_1/attempt-1".to_string()),
        work_item_id: Some("witem_1".to_string()),
        workspace_id: Some("ws_1".to_string()),
        work_plan_id: Some("wplan_1".to_string()),
        change_set_id: None,
        production_impacting: true,
    };

    assert!(coding_run_scope_matches_source(
        &scope,
        "witem_1",
        "ws_1",
        "https://github.com/lward27/yfinance_wrapper.git",
        "pharness/witem_1/attempt-1",
        true,
    ));
    assert!(!coding_run_scope_matches_source(
        &scope,
        "witem_1",
        "ws_1",
        "https://github.com/lward27/yfinance_wrapper.git",
        "pharness/witem_1/attempt-1",
        false,
    ));
}

#[test]
fn readiness_explains_unverified_runner_profiles_without_empty_blockers() {
    let profile = crate::dto::EnvironmentProfileResponse {
        id: "python-3.11".to_string(),
        status: "configured_unverified".to_string(),
        image: format!("example.test/python@sha256:{}", "a".repeat(64)),
        revision: "b".repeat(40),
        platform: "linux/amd64".to_string(),
        required_executables: vec!["python".to_string()],
        preparation_strategy: "python_hashed_requirements".to_string(),
        service_account: "pharness-python-runner".to_string(),
        repository_allowlist: vec!["https://github.com/example/repo.git".to_string()],
        blockers: Vec::new(),
    };

    assert_eq!(
            environment_profile_readiness_blocker(&profile).as_deref(),
            Some(
                "environment_profile python-3.11: runner profile requires a fresh passing isolated verification"
            )
        );
}

#[test]
fn failed_capability_summary_names_only_the_known_check_scope() {
    let outcome = crate::dispatch::CapabilityVerificationOutcome {
        available: false,
        principal: Some("system:serviceaccount:pharness:runner".to_string()),
        repository: Some("https://github.com/example/repo.git".to_string()),
        permission: Some("repository_read".to_string()),
    };

    assert_eq!(
        capability_verification_summary(&outcome),
        "Isolated identity did not verify repository_read for https://github.com/example/repo.git"
    );
}

async fn test_state_with_cluster_tools(cluster_tools: ReadOnlyClusterTools) -> AppState {
    AppState {
        store: Arc::new(SqliteStore::connect_in_memory().await.unwrap()),
        worker: RunDispatcher::Disabled,
        cluster_tools,
        policy: SafetyPolicy::default(),
        worker_token: None,
        operator_tokens: Arc::new(Vec::new()),
        workspace: WorkspaceProvisioner::new(std::env::temp_dir(), Vec::new()),
        build: super::BuildMetadata::from_env(),
        protected_target: super::ProtectedTargetConfiguration::from_env(),
        environment_profiles: Arc::new(Vec::new()),
    }
}

async fn test_state_with_git_observer(kubectl_bin: String, allowed_repo: String) -> AppState {
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let worker = RunDispatcher::Kubernetes(KubernetesJobDispatcher::new(
        store.clone(),
        kubectl_bin,
        WorkerKubernetesConfig {
            namespace: "pharness-test".to_string(),
            image: "example.test/pharness:latest".to_string(),
            service_account: "pharness-worker".to_string(),
            tekton_executor_service_account: "pharness-tekton-runner".to_string(),
            tekton_allowed_namespaces: vec!["ci".to_string()],
            tekton_executor_poll_seconds: 5,
            argo_executor_enabled: true,
            argo_executor_service_account: "pharness-argo-runner".to_string(),
            argo_executor_namespace: "argocd".to_string(),
            argo_executor_allowed_applications: vec![
                "finance-api".to_string(),
                "finance-app".to_string(),
            ],
            argo_executor_poll_seconds: 5,
            argo_executor_active_deadline_seconds: 600,
            argo_executor_ttl_seconds_after_finished: 3600,
            git_writer_enabled: true,
            git_writer_service_account: "pharness-git-writer".to_string(),
            git_writer_token_secret_name: Some("pharness-git-writer-token".to_string()),
            git_writer_allowed_repos: vec![allowed_repo.clone()],
            git_writer_github_api_url: "https://api.github.com".to_string(),
            git_writer_author_name: "Pharness".to_string(),
            git_writer_author_email: "pharness@example.test".to_string(),
            git_writer_active_deadline_seconds: 900,
            git_writer_ttl_seconds_after_finished: 3600,
            gitops_writer_enabled: true,
            gitops_writer_service_account: "pharness-gitops-writer".to_string(),
            gitops_writer_token_secret_name: Some("pharness-gitops-writer-token".to_string()),
            gitops_writer_allowed_repos: vec![allowed_repo.clone()],
            gitops_writer_github_api_url: "https://api.github.com".to_string(),
            gitops_writer_author_name: "Pharness".to_string(),
            gitops_writer_author_email: "pharness@example.test".to_string(),
            gitops_writer_active_deadline_seconds: 900,
            gitops_writer_ttl_seconds_after_finished: 3600,
            git_observer_enabled: true,
            git_observer_service_account: "pharness-git-observer".to_string(),
            git_observer_token_secret_name: Some("pharness-git-observer-token".to_string()),
            git_observer_allowed_repos: vec![allowed_repo.clone()],
            git_observer_github_api_url: "https://api.github.com".to_string(),
            git_observer_active_deadline_seconds: 300,
            git_observer_ttl_seconds_after_finished: 3600,
            gitops_observer_enabled: true,
            gitops_observer_service_account: "pharness-gitops-observer".to_string(),
            gitops_observer_token_secret_name: Some("pharness-gitops-observer-token".to_string()),
            gitops_observer_allowed_repos: vec![allowed_repo],
            gitops_observer_github_api_url: "https://api.github.com".to_string(),
            gitops_observer_active_deadline_seconds: 300,
            gitops_observer_ttl_seconds_after_finished: 3600,
            api_url: "http://pharness-api:4777".to_string(),
            workspace_dir: "/workspace".to_string(),
            workspace_size_limit: "4Gi".to_string(),
            workspace_storage_class: Some("local-path".to_string()),
            workspace_ephemeral_storage_request: "2Gi".to_string(),
            workspace_ephemeral_storage_limit: "4Gi".to_string(),
            workspace_node_hostname: None,
            max_concurrent_run_jobs: 1,
            fireworks_secret_name: "pharness-fireworks".to_string(),
            worker_token_secret_name: "pharness-worker-token".to_string(),
            active_deadline_seconds: 3600,
            ttl_seconds_after_finished: 3600,
        },
        "accounts/fireworks/models/test".to_string(),
        "https://example.test/v1".to_string(),
        Vec::new(),
    ));
    AppState {
        store,
        worker,
        cluster_tools: ReadOnlyClusterTools::default(),
        policy: SafetyPolicy::default(),
        worker_token: None,
        operator_tokens: Arc::new(Vec::new()),
        workspace: WorkspaceProvisioner::new(std::env::temp_dir(), Vec::new()),
        build: super::BuildMetadata::from_env(),
        protected_target: super::ProtectedTargetConfiguration::from_env(),
        environment_profiles: Arc::new(Vec::new()),
    }
}

async fn seed_approved_release(state: &AppState) -> String {
    let session_id = SessionId::new("ses_registry_inspection");
    let run_id = RunId::new("run_registry_inspection");
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: "registry inspection".to_string(),
            cwd: ".".to_string(),
        })
        .await
        .unwrap();
    state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: "registry inspection".to_string(),
            cwd: ".".to_string(),
            max_turns: 1,
            initial_status: "completed".to_string(),
            execution_target_json: serde_json::json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_observation(CreateObservation {
            id: "obs_registry_inspection".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            source: "test".to_string(),
            kind: "smoke".to_string(),
            subject: "checkout-api".to_string(),
            summary: "seed observation".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Deployment".to_string()),
            resource_name: Some("checkout-api".to_string()),
            resource_ref_json: None,
            artifact_id: None,
            data_json: serde_json::json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_incident(CreateIncident {
            id: "inc_registry_inspection".to_string(),
            observation_id: "obs_registry_inspection".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "resolved".to_string(),
            severity: "medium".to_string(),
            title: "Seed incident".to_string(),
            summary: "seed incident".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Deployment".to_string()),
            resource_name: Some("checkout-api".to_string()),
            data_json: serde_json::json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_remediation_plan(CreateRemediationPlan {
            id: "rplan_registry_inspection".to_string(),
            incident_id: "inc_registry_inspection".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Seed remediation".to_string(),
            summary: "seed remediation".to_string(),
            risk_level: "medium".to_string(),
            requires_approval: true,
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Deployment".to_string()),
            resource_name: Some("checkout-api".to_string()),
            plan_json: serde_json::json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_work_plan(CreateWorkPlan {
            id: "wplan_registry_inspection".to_string(),
            work_item_id: None,
            remediation_plan_id: Some("rplan_registry_inspection".to_string()),
            incident_id: Some("inc_registry_inspection".to_string()),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Seed work".to_string(),
            summary: "seed work".to_string(),
            risk_level: "medium".to_string(),
            requires_approval: true,
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Deployment".to_string()),
            resource_name: Some("checkout-api".to_string()),
            work_plan_json: serde_json::json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_change_set(CreateChangeSet {
            id: "cset_registry_inspection".to_string(),
            work_item_id: None,
            work_plan_id: "wplan_registry_inspection".to_string(),
            remediation_plan_id: Some("rplan_registry_inspection".to_string()),
            incident_id: Some("inc_registry_inspection".to_string()),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Seed changes".to_string(),
            summary: "seed changes".to_string(),
            risk_level: "medium".to_string(),
            material_hash: "hash_registry_inspection".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Application".to_string()),
            resource_name: Some("checkout-api".to_string()),
            change_set_json: serde_json::json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_pipeline_intent(CreatePipelineIntent {
            id: "pint_registry_inspection".to_string(),
            change_set_id: "cset_registry_inspection".to_string(),
            work_plan_id: "wplan_registry_inspection".to_string(),
            remediation_plan_id: Some("rplan_registry_inspection".to_string()),
            incident_id: Some("inc_registry_inspection".to_string()),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Seed pipeline".to_string(),
            summary: "seed pipeline".to_string(),
            risk_level: "medium".to_string(),
            intent_kind: "tekton_build_test_package".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("checkout-api".to_string()),
            intent_json: serde_json::json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_deployment_intent(CreateDeploymentIntent {
            id: "dint_registry_inspection".to_string(),
            pipeline_intent_id: "pint_registry_inspection".to_string(),
            change_set_id: "cset_registry_inspection".to_string(),
            work_plan_id: "wplan_registry_inspection".to_string(),
            remediation_plan_id: Some("rplan_registry_inspection".to_string()),
            incident_id: Some("inc_registry_inspection".to_string()),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Seed deploy".to_string(),
            summary: "seed deploy".to_string(),
            risk_level: "medium".to_string(),
            intent_kind: "argo_sync_deploy".to_string(),
            target_environment: Some("dev".to_string()),
            target_namespace: Some("apps-dev".to_string()),
            argo_application: Some("checkout-api".to_string()),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Deployment".to_string()),
            resource_name: Some("checkout-api".to_string()),
            intent_json: serde_json::json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_release(CreateRelease {
            id: "rel_registry_inspection".to_string(),
            deployment_intent_id: "dint_registry_inspection".to_string(),
            pipeline_intent_id: "pint_registry_inspection".to_string(),
            change_set_id: "cset_registry_inspection".to_string(),
            work_plan_id: "wplan_registry_inspection".to_string(),
            remediation_plan_id: Some("rplan_registry_inspection".to_string()),
            incident_id: Some("inc_registry_inspection".to_string()),
            session_id,
            run_id: Some(run_id),
            status: "approved".to_string(),
            title: "Seed release".to_string(),
            summary: "seed release".to_string(),
            risk_level: "medium".to_string(),
            release_kind: "gitops_release".to_string(),
            target_environment: Some("dev".to_string()),
            target_namespace: Some("apps-dev".to_string()),
            argo_application: Some("checkout-api".to_string()),
            version: Some("v0.1.0-smoke".to_string()),
            commit_sha: Some("abc1234".to_string()),
            image_digest: None,
            rollback_ref: None,
            release_json: serde_json::json!({}),
        })
        .await
        .unwrap();

    "rel_registry_inspection".to_string()
}

async fn seed_approved_work_item_release(state: &AppState) -> String {
    seed_approved_release(state).await;
    let inherited = state
        .store
        .get_release("rel_registry_inspection")
        .await
        .unwrap()
        .unwrap();
    let session_id = inherited.session_id.clone();
    let run_id = inherited.run_id.clone().unwrap();

    state
        .store
        .create_work_item(CreateWorkItem {
            id: "witem_post_sync".to_string(),
            status: "executing".to_string(),
            title: "Post-sync verification fixture".to_string(),
            intent: "verify a disposable development release".to_string(),
            acceptance_criteria: vec!["rollout is healthy".to_string()],
            source_repo: "https://github.example.test/team/checkout-api.git".to_string(),
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
            argo_application: Some("checkout-api".to_string()),
            workload_kind: None,
            workload_name: None,
            rollback_owner: None,
            production_impacting: false,
            max_attempts: 1,
            max_elapsed_seconds: 300,
            environment_profile_id: None,
            run_budget: Default::default(),
            repository_contract_json: None,
            repository_contract_hash: None,
            environment_preparation_status: "not_required".to_string(),
            created_by: Some("tester".to_string()),
        })
        .await
        .unwrap();
    state
        .store
        .create_work_plan(CreateWorkPlan {
            id: "wplan_post_sync".to_string(),
            work_item_id: Some("witem_post_sync".to_string()),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Post-sync verification plan".to_string(),
            summary: "verify a disposable development release".to_string(),
            risk_level: "medium".to_string(),
            requires_approval: true,
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Deployment".to_string()),
            resource_name: Some("checkout-api".to_string()),
            work_plan_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_change_set(CreateChangeSet {
            id: "cset_post_sync".to_string(),
            work_item_id: Some("witem_post_sync".to_string()),
            work_plan_id: "wplan_post_sync".to_string(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Post-sync verification changes".to_string(),
            summary: "fixture source change".to_string(),
            risk_level: "medium".to_string(),
            material_hash: "hash_post_sync".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Deployment".to_string()),
            resource_name: Some("checkout-api".to_string()),
            change_set_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_pipeline_intent(CreatePipelineIntent {
            id: "pint_post_sync".to_string(),
            change_set_id: "cset_post_sync".to_string(),
            work_plan_id: "wplan_post_sync".to_string(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Post-sync verification pipeline".to_string(),
            summary: "fixture build".to_string(),
            risk_level: "medium".to_string(),
            intent_kind: "tekton_build_test_package".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Deployment".to_string()),
            resource_name: Some("checkout-api".to_string()),
            intent_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_deployment_intent(CreateDeploymentIntent {
            id: "dint_post_sync".to_string(),
            pipeline_intent_id: "pint_post_sync".to_string(),
            change_set_id: "cset_post_sync".to_string(),
            work_plan_id: "wplan_post_sync".to_string(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Post-sync verification deployment".to_string(),
            summary: "fixture deployment".to_string(),
            risk_level: "medium".to_string(),
            intent_kind: "argo_sync_deploy".to_string(),
            target_environment: Some("dev".to_string()),
            target_namespace: Some("apps-dev".to_string()),
            argo_application: Some("checkout-api".to_string()),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Deployment".to_string()),
            resource_name: Some("checkout-api".to_string()),
            intent_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_release(CreateRelease {
            id: "rel_post_sync".to_string(),
            deployment_intent_id: "dint_post_sync".to_string(),
            pipeline_intent_id: "pint_post_sync".to_string(),
            change_set_id: "cset_post_sync".to_string(),
            work_plan_id: "wplan_post_sync".to_string(),
            remediation_plan_id: None,
            incident_id: None,
            session_id,
            run_id: Some(run_id),
            status: "approved".to_string(),
            title: "Post-sync verification release".to_string(),
            summary: "fixture release".to_string(),
            risk_level: "medium".to_string(),
            release_kind: "gitops_release".to_string(),
            target_environment: Some("dev".to_string()),
            target_namespace: Some("apps-dev".to_string()),
            argo_application: Some("checkout-api".to_string()),
            version: Some("v0.1.0-smoke".to_string()),
            commit_sha: Some("abc1234".to_string()),
            image_digest: None,
            rollback_ref: None,
            release_json: json!({}),
        })
        .await
        .unwrap();

    "rel_post_sync".to_string()
}

#[tokio::test]
async fn operator_auth_gates_api_routes_and_resolves_identity() {
    use tower::ServiceExt;

    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let app = router(
        store,
        RunDispatcher::Disabled,
        ReadOnlyClusterTools::default(),
        SafetyPolicy::default(),
        None,
        vec![("lucas".to_string(), "op-secret".to_string())],
        WorkspaceProvisioner::new(std::env::temp_dir(), Vec::new()),
    );

    let health = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/health")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);

    let unauthenticated = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/runs")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let wrong = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/runs")
                .header("authorization", "Bearer nope")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);

    let authed = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/config/effective")
                .header("authorization", "Bearer op-secret")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(authed.status(), StatusCode::OK);
    let body = axum::body::to_bytes(authed.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["operator"]["auth_required"], true);
    assert_eq!(payload["operator"]["name"], "lucas");
}

#[tokio::test]
async fn internal_routes_are_disabled_without_worker_token() {
    use tower::ServiceExt;

    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let app = router(
        store,
        RunDispatcher::Disabled,
        ReadOnlyClusterTools::default(),
        SafetyPolicy::default(),
        None,
        Vec::new(),
        WorkspaceProvisioner::new(std::env::temp_dir(), Vec::new()),
    );

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/internal/runs/run_x/control")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn internal_routes_reject_missing_or_wrong_worker_token() {
    use tower::ServiceExt;

    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let app = router(
        store,
        RunDispatcher::Disabled,
        ReadOnlyClusterTools::default(),
        SafetyPolicy::default(),
        Some("worker-secret".to_string()),
        Vec::new(),
        WorkspaceProvisioner::new(std::env::temp_dir(), Vec::new()),
    );

    let missing = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/internal/runs/run_x/control")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

    let wrong = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/internal/runs/run_x/control")
                .header("authorization", "Bearer not-the-token")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn internal_worker_contract_marks_running_and_finishes_run() {
    use tower::ServiceExt;

    let state = test_state().await;
    let session_id = SessionId::new("ses_internal_contract");
    let run_id = RunId::new("run_internal_contract");
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: "internal contract".to_string(),
            cwd: ".".to_string(),
        })
        .await
        .unwrap();
    state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: "internal contract".to_string(),
            cwd: ".".to_string(),
            max_turns: 5,
            initial_status: "queued".to_string(),
            execution_target_json: json!({ "kind": "kubernetes_job" }),
        })
        .await
        .unwrap();

    let app = router(
        state.store.clone(),
        RunDispatcher::Disabled,
        ReadOnlyClusterTools::default(),
        SafetyPolicy::default(),
        Some("worker-secret".to_string()),
        Vec::new(),
        WorkspaceProvisioner::new(std::env::temp_dir(), Vec::new()),
    );

    let context = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/internal/runs/run_internal_contract/attempt-context")
                .header("authorization", "Bearer worker-secret")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(context.status(), StatusCode::OK);

    let marked = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/internal/runs/run_internal_contract/mark-running")
                .header("authorization", "Bearer worker-secret")
                .header("content-type", "application/json")
                .body(axum::body::Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(marked.status(), StatusCode::OK);

    let event = AgentEvent {
        event_id: EventId::new("evt_run_internal_contract_2"),
        session_id: session_id.clone(),
        run_id: run_id.clone(),
        seq: 2,
        kind: EventKind::RunStarted,
        payload: json!({ "source": "worker" }),
    };
    let ingested = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/internal/runs/run_internal_contract/events")
                .header("authorization", "Bearer worker-secret")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&json!({ "events": [event] })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ingested.status(), StatusCode::OK);

    let outcome = json!({
        "status": "completed",
        "turns": 1,
        "summary": "done",
        "error": null,
        "approval": null,
    });
    let finished = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/internal/runs/run_internal_contract/outcome")
                .header("authorization", "Bearer worker-secret")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&outcome).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(finished.status(), StatusCode::OK);

    let run = state.store.get_run(&run_id).await.unwrap().unwrap();
    assert_eq!(run.status, "completed");
    let events = state.store.list_events(&run_id).await.unwrap();
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn router_mounts_static_and_dynamic_run_routes() {
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());

    let _app = router(
        store,
        RunDispatcher::Disabled,
        ReadOnlyClusterTools::default(),
        SafetyPolicy::default(),
        None,
        Vec::new(),
        WorkspaceProvisioner::new(std::env::temp_dir(), Vec::new()),
    );
}

fn fake_kubectl_script() -> PathBuf {
    let path = std::env::temp_dir().join(format!("pharness-fake-kubectl-{}", unique_suffix()));
    fs::write(
        &path,
        r#"#!/bin/sh
printf '%s\n' '{"apiVersion":"v1","kind":"List","items":[]}'
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    path
}

fn fake_healthy_rollout_kubectl_script() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "pharness-fake-healthy-rollout-kubectl-{}",
        unique_suffix()
    ));
    fs::write(
            &path,
            r#"#!/bin/sh
case "$*" in
  *applications.argoproj.io*)
    printf '%s\n' '{"apiVersion":"argoproj.io/v1alpha1","kind":"Application","metadata":{"name":"checkout-api","namespace":"argocd"},"status":{"sync":{"status":"Synced","revision":"abc1234"},"health":{"status":"Healthy"}}}'
    ;;
  *deployments*)
    printf '%s\n' '{"apiVersion":"apps/v1","kind":"Deployment","metadata":{"name":"checkout-api","namespace":"apps-dev","generation":1},"spec":{"replicas":1},"status":{"observedGeneration":1,"updatedReplicas":1,"availableReplicas":1,"readyReplicas":1}}'
    ;;
  *)
    exit 1
    ;;
esac
"#,
        )
        .unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    path
}

fn fake_succeeded_tekton_kubectl_script() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "pharness-fake-succeeded-tekton-kubectl-{}",
        unique_suffix()
    ));
    fs::write(
            &path,
            r#"#!/bin/sh
case "$*" in
  *pipelineruns.tekton.dev*)
    printf '%s\n' '{"apiVersion":"tekton.dev/v1","kind":"PipelineRun","metadata":{"name":"finance-build","namespace":"ci","labels":{"tekton.dev/pipeline":"finance-ci"}},"status":{"conditions":[{"type":"Succeeded","status":"True","reason":"Succeeded"}]}}'
    ;;
  *taskruns.tekton.dev*)
    printf '%s\n' '{"apiVersion":"tekton.dev/v1","kind":"TaskRunList","items":[]}'
    ;;
  *)
    exit 1
    ;;
esac
"#,
        )
        .unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    path
}

fn fake_completed_argo_wait_kubectl_script() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "pharness-fake-completed-argo-wait-kubectl-{}",
        unique_suffix()
    ));
    fs::write(
            &path,
            r#"#!/bin/sh
case "$*" in
  *applications.argoproj.io*)
    printf '%s\n' '{"apiVersion":"argoproj.io/v1alpha1","kind":"Application","metadata":{"name":"checkout-api","namespace":"argocd"},"status":{"sync":{"status":"Synced","revision":"abc1234"},"health":{"status":"Healthy"},"operationState":{"phase":"Succeeded","syncResult":{"revision":"abc1234"}}}}'
    ;;
  *)
    exit 1
    ;;
esac
"#,
        )
        .unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    path
}

async fn seed_completed_argo_sync(state: &AppState, release_id: &str) {
    seed_completed_argo_sync_with_contract(state, release_id, None).await;
}

async fn seed_completed_argo_sync_with_contract(
    state: &AppState,
    release_id: &str,
    deployment_contract_id: Option<&str>,
) {
    let release = state.store.get_release(release_id).await.unwrap().unwrap();
    let run_id = release.run_id.clone().unwrap();
    let mut execution_content = json!({
        "execution_id": "aexec_seed",
        "deployment_intent_id": release.deployment_intent_id,
    });
    if let Some(deployment_contract_id) = deployment_contract_id {
        execution_content["deployment_contract_id"] = json!(deployment_contract_id);
    }
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_argo_sync_execution".to_string(),
            session_id: release.session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "argo_sync_execution".to_string(),
            label: "seed argo sync execution".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(execution_content),
        })
        .await
        .unwrap();
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_argo_sync_result".to_string(),
            session_id: release.session_id,
            run_id: Some(run_id),
            kind: "argo_sync_result".to_string(),
            label: "seed argo sync completion".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": "aexec_seed",
                "status": "completed",
                "deployment_intent_id": release.deployment_intent_id,
                "details": {
                    "sync_status": "Synced",
                    "operation_phase": "Succeeded"
                }
            })),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn post_sync_release_verification_reads_declared_targets_and_completes_only_when_healthy() {
    let fake_kubectl = fake_healthy_rollout_kubectl_script();
    let state = test_state_with_cluster_tools(
        ReadOnlyClusterTools::default().with_kubectl_bin(fake_kubectl.display().to_string()),
    )
    .await;
    let release_id = seed_approved_work_item_release(&state).await;
    seed_completed_argo_sync(&state, &release_id).await;

    let Json(response) = verify_release(
        State(state.clone()),
        None,
        Path(release_id.clone()),
        Json(VerifyReleaseRequest {
            complete: true,
            actor: Some("lucas".to_string()),
            reason: Some("healthy disposable dev rollout".to_string()),
            timeout_ms: None,
        }),
    )
    .await
    .unwrap();

    assert_eq!(response.status, "verified");
    assert!(response.verified);
    assert!(response.completed);
    assert_eq!(response.release.status, "completed");
    assert_eq!(
        response.release.release_json["post_sync_verification"]["status"],
        "verified"
    );
    assert_eq!(
        response.argo_observation.data_json["analysis"]["sync_status"],
        "Synced"
    );
    assert_eq!(
        response.workload_observation.data_json["analysis"]["status"],
        "healthy"
    );

    let audits = state
        .store
        .list_audit_events(Some("release"), Some(&release_id), None, 20)
        .await
        .unwrap();
    assert!(audits
        .iter()
        .any(|event| event.kind == "release.post_sync_verified"));
    assert!(audits.iter().any(|event| event.kind == "release.completed"));
}

#[tokio::test]
async fn post_sync_release_verification_blocks_completion_when_required_prometheus_is_unavailable()
{
    let fake_kubectl = fake_healthy_rollout_kubectl_script();
    let state = test_state_with_cluster_tools(
        ReadOnlyClusterTools::default().with_kubectl_bin(fake_kubectl.display().to_string()),
    )
    .await;
    let release_id = seed_approved_work_item_release(&state).await;
    let Json(contract) = create_deployment_contract(
        State(state.clone()),
        None,
        Json(CreateDeploymentContractRequest {
            target_environment: "dev".to_string(),
            target_namespace: "apps-dev".to_string(),
            argo_application: "checkout-api".to_string(),
            version: Some("v2-prometheus-required".to_string()),
            contract_json: json!({
                "operation": "sync",
                "prune": false,
                "force": false,
                "post_sync_verification": {
                    "prometheus_inventory": "required"
                }
            }),
            actor: Some("lucas".to_string()),
            reason: Some("require bounded runtime evidence".to_string()),
        }),
    )
    .await
    .unwrap();
    seed_completed_argo_sync_with_contract(&state, &release_id, Some(&contract.id)).await;

    let Json(response) = verify_release(
        State(state.clone()),
        None,
        Path(release_id.clone()),
        Json(VerifyReleaseRequest {
            complete: true,
            actor: Some("lucas".to_string()),
            reason: Some("do not complete without required runtime evidence".to_string()),
            timeout_ms: None,
        }),
    )
    .await
    .unwrap();

    assert_eq!(response.status, "attention_required");
    assert!(!response.verified);
    assert!(!response.completed);
    assert_eq!(response.release.status, "approved");
    assert!(response.observability_observation.is_none());
    assert_eq!(
        response.release.release_json["post_sync_verification"]["deployment_contract_id"],
        json!(contract.id)
    );
    assert_eq!(
        response.release.release_json["post_sync_verification"]["observability"]
            ["prometheus_inventory"]["required"],
        json!(true)
    );
    assert_eq!(
        response.release.release_json["post_sync_verification"]["observability"]
            ["prometheus_inventory"]["status"],
        json!("attention_required")
    );
    assert!(response.checks.iter().any(|check| {
        check["code"] == "prometheus_inventory" && check["passed"] == json!(false)
    }));
    let audits = state
        .store
        .list_audit_events(Some("release"), Some(&release_id), None, 20)
        .await
        .unwrap();
    assert!(audits
        .iter()
        .any(|event| event.kind == "release.post_sync_attention_required"));
}

#[tokio::test]
async fn post_sync_release_verification_requires_a_completed_current_sync() {
    let state = test_state().await;
    let release_id = seed_approved_work_item_release(&state).await;

    let error = verify_release(
        State(state),
        None,
        Path(release_id),
        Json(VerifyReleaseRequest {
            complete: false,
            actor: Some("lucas".to_string()),
            reason: Some("verification fixture".to_string()),
            timeout_ms: None,
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(error.status, StatusCode::CONFLICT);
    assert!(error
        .message
        .contains("current Argo sync execution to have a completed result"));
}

fn slow_fake_kubectl_script() -> PathBuf {
    let path = std::env::temp_dir().join(format!("pharness-slow-fake-kubectl-{}", unique_suffix()));
    fs::write(
        &path,
        r#"#!/bin/sh
sleep 2
printf '%s\n' '{"apiVersion":"v1","kind":"List","items":[]}'
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    path
}

#[tokio::test]
async fn creates_gets_lists_events_and_cancels_run() {
    let state = test_state().await;

    let Json(created) = create_run(
        State(state.clone()),
        Json(CreateRunRequest {
            task: "inspect app".to_string(),
            cwd: Some(".".to_string()),
            max_turns: Some(12),
            policy_mode: None,
            scope: None,
        }),
    )
    .await
    .unwrap();

    assert_eq!(created.status, "queued");
    assert_eq!(created.max_turns, 12);
    assert_eq!(created.origin, "operator");

    let Json(fetched) = get_run(State(state.clone()), Path(created.id.to_string()))
        .await
        .unwrap();
    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.origin, "operator");

    let Json(events) = get_run_events(State(state.clone()), Path(created.id.to_string()))
        .await
        .unwrap();
    assert_eq!(events.events.len(), 1);

    let Json(listed) = list_runs(
        State(state.clone()),
        Query(ListRunsQuery {
            search: None,
            status: Some("queued".to_string()),
            origin: Some("operator".to_string()),
            actor: None,
            namespace: None,
            repo: None,
            branch: None,
            production_impacting: None,
            started_after_ms: None,
            started_before_ms: None,
            limit: Some(50),
            offset: Some(0),
        }),
    )
    .await
    .unwrap();
    assert_eq!(listed.groups.len(), 1);
    assert_eq!(listed.groups[0].count, 1);
    assert_eq!(listed.groups[0].members[0].id, created.id.to_string());

    let Json(cancelled) = cancel_run(State(state.clone()), Path(created.id.to_string()))
        .await
        .unwrap();
    assert_eq!(cancelled.status, "cancelled");
}

#[tokio::test]
async fn authenticated_run_creation_persists_and_filters_creator() {
    let state = test_state().await;
    let Json(created) = create_operator_run(
        State(state.clone()),
        Some(Extension(OperatorIdentity("lucas".to_string()))),
        Json(CreateRunRequest {
            task: "inspect finance app".to_string(),
            cwd: Some(".".to_string()),
            max_turns: Some(4),
            policy_mode: None,
            scope: None,
        }),
    )
    .await
    .unwrap();

    assert_eq!(created.created_by.as_deref(), Some("lucas"));
    let Json(listed) = list_runs(
        State(state),
        Query(ListRunsQuery {
            search: None,
            status: Some("queued".to_string()),
            origin: Some("operator".to_string()),
            actor: Some("lucas".to_string()),
            namespace: None,
            repo: None,
            branch: None,
            production_impacting: None,
            started_after_ms: None,
            started_before_ms: None,
            limit: Some(10),
            offset: Some(0),
        }),
    )
    .await
    .unwrap();

    assert_eq!(listed.count, 1);
    assert_eq!(listed.runs[0].id, created.id);
    assert_eq!(listed.runs[0].created_by.as_deref(), Some("lucas"));
}

#[tokio::test]
async fn operator_run_groups_cover_all_matching_pages() {
    let state = test_state().await;
    for _ in 0..3 {
        let _ = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "repeatable operator group".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(1),
                policy_mode: None,
                scope: None,
            }),
        )
        .await
        .unwrap();
    }

    let Json(listed) = list_runs(
        State(state),
        Query(ListRunsQuery {
            search: Some("repeatable operator".to_string()),
            status: Some("queued".to_string()),
            origin: Some("operator".to_string()),
            actor: None,
            namespace: None,
            repo: None,
            branch: None,
            production_impacting: None,
            started_after_ms: None,
            started_before_ms: None,
            limit: Some(1),
            offset: Some(2),
        }),
    )
    .await
    .unwrap();

    assert_eq!(listed.runs.len(), 1);
    assert_eq!(listed.count, 3);
    assert_eq!(listed.groups.len(), 1);
    assert_eq!(listed.groups[0].count, 3);
    assert_eq!(listed.groups[0].members.len(), 3);
}

#[tokio::test]
async fn create_run_persists_requested_policy_mode() {
    let state = test_state().await;

    let Json(created) = create_run(
        State(state.clone()),
        Json(CreateRunRequest {
            task: "write file".to_string(),
            cwd: Some(".".to_string()),
            max_turns: Some(12),
            policy_mode: Some(PolicyMode::TrustedWrites),
            scope: None,
        }),
    )
    .await
    .unwrap();
    let stored = state.store.get_run(&created.id).await.unwrap().unwrap();
    let Json(events) = get_run_events(State(state.clone()), Path(created.id.to_string()))
        .await
        .unwrap();

    assert_eq!(
        stored.execution_target_json["policy"]["mode"],
        "trusted_writes"
    );
    assert_eq!(
        stored.execution_target_json["policy"]["environment"],
        "local"
    );
    assert_eq!(
        events.events[0].payload["policy_mode"],
        serde_json::json!("trusted_writes")
    );
    assert_eq!(
        events.events[0].payload["policy_environment"],
        serde_json::json!("local")
    );
}

#[tokio::test]
async fn create_run_normalizes_empty_run_scope() {
    let state = test_state().await;

    let Json(created) = create_run(
        State(state.clone()),
        Json(CreateRunRequest {
            task: "inspect app".to_string(),
            cwd: Some(".".to_string()),
            max_turns: Some(12),
            policy_mode: None,
            scope: None,
        }),
    )
    .await
    .unwrap();
    let stored = state.store.get_run(&created.id).await.unwrap().unwrap();
    let Json(fetched) = get_run(State(state.clone()), Path(created.id.to_string()))
        .await
        .unwrap();
    let Json(events) = get_run_events(State(state.clone()), Path(created.id.to_string()))
        .await
        .unwrap();

    assert!(stored.execution_target_json["run_scope"].is_null());
    assert!(fetched.scope.is_none());
    assert!(events.events[0].payload["run_scope"].is_null());
}

#[tokio::test]
async fn create_run_persists_run_scope_metadata() {
    let state = test_state().await;
    let scope = RunScope {
        run_id: None,
        namespace: Some("apps-dev".to_string()),
        repo: Some("git@example.test/team/app.git".to_string()),
        branch: Some("feature/pharness".to_string()),
        work_item_id: None,
        workspace_id: None,
        work_plan_id: Some("wplan_scope".to_string()),
        change_set_id: Some("cset_scope".to_string()),
        production_impacting: false,
    };

    let Json(created) = create_run(
        State(state.clone()),
        Json(CreateRunRequest {
            task: "inspect app".to_string(),
            cwd: Some(".".to_string()),
            max_turns: Some(12),
            policy_mode: None,
            scope: Some(scope.clone()),
        }),
    )
    .await
    .unwrap();
    let stored = state.store.get_run(&created.id).await.unwrap().unwrap();
    let Json(fetched) = get_run(State(state.clone()), Path(created.id.to_string()))
        .await
        .unwrap();
    let Json(events) = get_run_events(State(state.clone()), Path(created.id.to_string()))
        .await
        .unwrap();

    assert_eq!(
        stored.execution_target_json["run_scope"]["namespace"],
        "apps-dev"
    );
    assert_eq!(fetched.scope.as_ref(), Some(&scope));
    assert_eq!(
        events.events[0].payload["run_scope"]["branch"],
        "feature/pharness"
    );

    let Json(listed) = list_runs(
        State(state.clone()),
        Query(ListRunsQuery {
            search: None,
            status: Some("queued".to_string()),
            origin: Some("operator".to_string()),
            actor: None,
            namespace: Some("apps-dev".to_string()),
            repo: Some("git@example.test/team/app.git".to_string()),
            branch: Some("feature/pharness".to_string()),
            production_impacting: Some(false),
            started_after_ms: Some(0),
            started_before_ms: None,
            limit: Some(10),
            offset: Some(0),
        }),
    )
    .await
    .unwrap();

    assert_eq!(listed.count, 1);
    assert_eq!(listed.runs[0].id, created.id);
    assert_eq!(listed.runs[0].started_at, fetched.started_at);

    let Json(summary) = run_summary(
        State(state),
        Query(ListRunsQuery {
            search: None,
            status: Some("queued".to_string()),
            origin: None,
            actor: None,
            namespace: Some("apps-dev".to_string()),
            repo: Some("git@example.test/team/app.git".to_string()),
            branch: Some("feature/pharness".to_string()),
            production_impacting: Some(false),
            started_after_ms: Some(0),
            started_before_ms: None,
            limit: None,
            offset: None,
        }),
    )
    .await
    .unwrap();

    assert_eq!(summary.summary.total, 1);
    assert_eq!(
        summary.summary.by_status[0].value.as_deref(),
        Some("queued")
    );
}

#[tokio::test]
async fn create_run_snapshots_active_permission_grants() {
    let state = test_state().await;

    let Json(grant) = super::create_permission_grant(
        State(state.clone()),
        Json(CreatePermissionGrantRequest {
            subject: "agent:local-worker".to_string(),
            created_by: None,
            reason: "trusted local write smoke".to_string(),
            scope: serde_json::json!({
                "environment": "local",
                "capability_kinds": ["filesystem"],
                "actions": ["write_file"],
                "max_risk": "medium"
            }),
            policy: serde_json::json!({
                "policy_mode": "trusted_writes"
            }),
            expires_at: None,
        }),
    )
    .await
    .unwrap();
    let Json(created) = create_run(
        State(state.clone()),
        Json(CreateRunRequest {
            task: "write file".to_string(),
            cwd: Some(".".to_string()),
            max_turns: Some(12),
            policy_mode: None,
            scope: None,
        }),
    )
    .await
    .unwrap();
    let stored = state.store.get_run(&created.id).await.unwrap().unwrap();

    assert_eq!(
        stored.execution_target_json["policy"]["permission_grants"][0]["id"],
        grant.id
    );
}

#[tokio::test]
async fn reports_disabled_worker_config() {
    let state = test_state().await;

    let Json(config) = config_effective(State(state), None).await;

    assert_eq!(config["worker"]["enabled"], false);
    assert!(config["worker"]["model"].is_null());
    assert_eq!(config["cluster"]["argocd_namespace"], "argocd");
    assert_eq!(config["cluster"]["loki_configured"], false);
    assert_eq!(config["policy"]["mode"], "default");
    assert_eq!(config["policy"]["environment"], "local");
}

#[test]
fn run_policy_applies_mode_override_without_mutating_defaults() {
    let default = SafetyPolicy::default();
    let policy = run_policy(&default, Some(PolicyMode::TrustedWrites));

    assert_eq!(policy.mode, PolicyMode::TrustedWrites);
    assert_eq!(default.mode, PolicyMode::Default);
}

#[test]
fn policy_json_exposes_decision_flags_without_secrets() {
    let policy = SafetyPolicy {
        mode: PolicyMode::Plan,
        ..SafetyPolicy::default()
    };
    let json = policy_json(&policy);

    assert_eq!(json["mode"], "plan");
    assert_eq!(json["subject"], "agent:local-worker");
    assert_eq!(json["environment"], "local");
    assert_eq!(json["permission_grant_count"], 0);
    assert_eq!(json["deny_secret_access"], true);
}

#[tokio::test]
async fn direct_capability_execution_denies_secret_reads() {
    let state = test_state().await;
    let Json(response) = execute_capability(
        State(state.clone()),
        Json(ExecuteCapabilityRequest {
            action: AgentAction::KubernetesGet {
                id: "act_secret".into(),
                reason: "read secret".to_string(),
                resource: "secrets".to_string(),
                namespace: Some("argocd".to_string()),
                name: None,
                all_namespaces: false,
                label_selector: None,
            },
            timeout_ms: None,
        }),
    )
    .await
    .unwrap();

    assert_eq!(response.status, "denied");
    assert_eq!(response.action, "kubernetes_get");
    assert!(!response.executed);
    assert!(response.result.is_none());
    let Json(audit_events) = list_audit_events(
        State(state),
        Query(ListAuditEventsQuery {
            resource_kind: Some("capability".to_string()),
            resource_id: Some("kubernetes_get".to_string()),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    assert!(audit_events.events.iter().any(|event| {
        event.kind == "direct_capability.denied"
            && event.payload["action"] == "kubernetes_get"
            && event.payload["executed"] == false
    }));
}

#[tokio::test]
async fn direct_capability_execution_audits_success_summary() {
    let fake_kubectl = fake_kubectl_script();
    let state = test_state_with_cluster_tools(
        ReadOnlyClusterTools::default().with_kubectl_bin(fake_kubectl.display().to_string()),
    )
    .await;
    let Json(response) = execute_capability(
        State(state.clone()),
        Json(ExecuteCapabilityRequest {
            action: AgentAction::KubernetesGet {
                id: "act_pods".into(),
                reason: "read pods".to_string(),
                resource: "pods".to_string(),
                namespace: Some("argocd".to_string()),
                name: None,
                all_namespaces: false,
                label_selector: None,
            },
            timeout_ms: None,
        }),
    )
    .await
    .unwrap();

    assert_eq!(response.status, "ok");
    assert_eq!(response.action, "kubernetes_get");
    assert!(response.executed);
    let artifact_id = response.artifact_id.clone().unwrap();
    let observation_id = response.observation_id.clone().unwrap();
    let Json(artifact) = get_artifact(State(state.clone()), Path(artifact_id.clone()))
        .await
        .unwrap();
    assert_eq!(artifact.id, artifact_id);
    assert_eq!(artifact.kind, "kubernetes_tool_result");
    assert!(artifact.run_id.is_none());
    assert_eq!(
        artifact.content_json.as_ref().unwrap()["output"]["item_count"],
        0
    );
    let Json(observations) = list_observations(
        State(state.clone()),
        Query(ListObservationsQuery {
            run_id: None,
            source: Some("kubernetes".to_string()),
            kind: Some("pods".to_string()),
            subject: None,
            resource_namespace: Some("argocd".to_string()),
            resource_kind: Some("pods".to_string()),
            resource_name: None,
            observed_after_ms: None,
            observed_before_ms: None,
            limit: Some(50),
            offset: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(observations.count, 1);
    assert_eq!(observations.observations[0].id, observation_id);
    assert_eq!(
        observations.observations[0].artifact_id.as_deref(),
        Some(artifact_id.as_str())
    );
    let Json(audit_events) = list_audit_events(
        State(state.clone()),
        Query(ListAuditEventsQuery {
            resource_kind: Some("capability".to_string()),
            resource_id: Some("kubernetes_get".to_string()),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    let event = audit_events
        .events
        .iter()
        .find(|event| event.kind == "direct_capability.executed")
        .unwrap();

    assert_eq!(event.payload["executed"], true);
    assert_eq!(event.payload["result"]["source"], "kubernetes");
    assert_eq!(event.payload["result"]["output"]["item_count"], 0);
    assert!(!event.payload.to_string().contains("PodList"));
    let Json(observation_audit_events) = list_audit_events(
        State(state),
        Query(ListAuditEventsQuery {
            resource_kind: Some("observation".to_string()),
            resource_id: Some(observation_id),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    assert!(observation_audit_events
        .events
        .iter()
        .any(|event| event.kind == "observation.created"));
    let _ = fs::remove_file(fake_kubectl);
}

#[tokio::test]
async fn direct_capability_execution_can_be_cancelled_by_timeout() {
    let fake_kubectl = slow_fake_kubectl_script();
    let state = test_state_with_cluster_tools(
        ReadOnlyClusterTools::default()
            .with_kubectl_bin(fake_kubectl.display().to_string())
            .with_timeout_ms(5_000),
    )
    .await;
    let Json(response) = execute_capability(
        State(state.clone()),
        Json(ExecuteCapabilityRequest {
            action: AgentAction::KubernetesGet {
                id: "act_cancel".into(),
                reason: "read pods".to_string(),
                resource: "pods".to_string(),
                namespace: Some("argocd".to_string()),
                name: None,
                all_namespaces: false,
                label_selector: None,
            },
            timeout_ms: Some(10),
        }),
    )
    .await
    .unwrap();

    assert_eq!(response.status, "cancelled");
    assert_eq!(response.action, "kubernetes_get");
    assert!(response.executed);
    assert!(response.cancelled);
    assert_eq!(response.timeout_ms, 10);
    assert!(response.result.is_none());
    let Json(audit_events) = list_audit_events(
        State(state),
        Query(ListAuditEventsQuery {
            resource_kind: Some("capability".to_string()),
            resource_id: Some("kubernetes_get".to_string()),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    assert!(audit_events.events.iter().any(|event| {
        event.kind == "direct_capability.cancelled"
            && event.payload["executed"] == true
            && event.payload["cancelled"] == true
            && event.payload["timeout_ms"] == 10
    }));
    let _ = fs::remove_file(fake_kubectl);
}

#[tokio::test]
async fn direct_capability_execution_denies_secret_shaped_tekton_reads() {
    let Json(response) = execute_capability(
        State(test_state().await),
        Json(ExecuteCapabilityRequest {
            action: AgentAction::TektonGetPipelineRuns {
                id: "act_tekton_secret".into(),
                reason: "read pipeline runs".to_string(),
                namespace: Some("token-store".to_string()),
                name: None,
                all_namespaces: false,
                label_selector: None,
            },
            timeout_ms: None,
        }),
    )
    .await
    .unwrap();

    assert_eq!(response.status, "denied");
    assert_eq!(response.action, "tekton_get_pipeline_runs");
    assert!(!response.executed);
    assert!(response.result.is_none());
}

#[tokio::test]
async fn direct_capability_execution_denies_secret_shaped_tekton_task_reads() {
    let Json(response) = execute_capability(
        State(test_state().await),
        Json(ExecuteCapabilityRequest {
            action: AgentAction::TektonGetTaskRuns {
                id: "act_tekton_task_secret".into(),
                reason: "read task runs".to_string(),
                namespace: Some("token-store".to_string()),
                name: None,
                all_namespaces: false,
                label_selector: None,
            },
            timeout_ms: None,
        }),
    )
    .await
    .unwrap();

    assert_eq!(response.status, "denied");
    assert_eq!(response.action, "tekton_get_task_runs");
    assert!(!response.executed);
    assert!(response.result.is_none());
}

#[tokio::test]
async fn direct_capability_execution_denies_secret_shaped_tekton_analysis() {
    let Json(response) = execute_capability(
        State(test_state().await),
        Json(ExecuteCapabilityRequest {
            action: AgentAction::TektonAnalyzePipelineRun {
                id: "act_tekton_analysis_secret".into(),
                reason: "analyze pipeline run".to_string(),
                namespace: "ci".to_string(),
                name: "token-build".to_string(),
            },
            timeout_ms: None,
        }),
    )
    .await
    .unwrap();

    assert_eq!(response.status, "denied");
    assert_eq!(response.action, "tekton_analyze_pipeline_run");
    assert!(!response.executed);
    assert!(response.result.is_none());
}

#[tokio::test]
async fn direct_capability_execution_returns_tool_errors_as_json() {
    let state = test_state().await;
    let Json(response) = execute_capability(
        State(state.clone()),
        Json(ExecuteCapabilityRequest {
            action: AgentAction::PrometheusQuery {
                id: "act_prom".into(),
                reason: "query".to_string(),
                query: "up".to_string(),
            },
            timeout_ms: None,
        }),
    )
    .await
    .unwrap();

    assert_eq!(response.status, "tool_error");
    assert_eq!(response.action, "prometheus_query");
    assert!(response.executed);
    assert!(response
        .error
        .as_deref()
        .unwrap()
        .contains("not configured"));
    let Json(audit_events) = list_audit_events(
        State(state),
        Query(ListAuditEventsQuery {
            resource_kind: Some("capability".to_string()),
            resource_id: Some("prometheus_query".to_string()),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    assert!(audit_events.events.iter().any(|event| {
        event.kind == "direct_capability.failed"
            && event.payload["executed"] == true
            && event.payload["error"]
                .as_str()
                .unwrap()
                .contains("not configured")
    }));
}

#[test]
fn production_baseline_surfaces_the_exact_read_only_capability_failure() {
    let error = required_baseline_capability_result(
        ExecuteCapabilityResponse {
            status: "tool_error".to_string(),
            action: "kubernetes_get".to_string(),
            decision: PolicyDecision::Allow {
                risk: RiskLevel::Low,
                summary: "typed read-only observation".to_string(),
                grant_id: None,
            },
            executed: true,
            cancelled: false,
            timeout_ms: 60_000,
            artifact_id: None,
            observation_id: None,
            result: None,
            error: Some(
                "deployments.apps yfinance-wrapper is forbidden for pharness-api".to_string(),
            ),
        },
        "Deployment",
    )
    .unwrap_err();

    assert_eq!(error.status, StatusCode::CONFLICT);
    assert!(error.message.contains("production baseline Deployment"));
    assert!(error.message.contains("yfinance-wrapper is forbidden"));
}

#[tokio::test]
async fn direct_capability_execution_accepts_prometheus_inventory() {
    let Json(response) = execute_capability(
        State(test_state().await),
        Json(ExecuteCapabilityRequest {
            action: AgentAction::PrometheusInventory {
                id: "act_prom_inventory".into(),
                reason: "inventory".to_string(),
            },
            timeout_ms: None,
        }),
    )
    .await
    .unwrap();

    assert_eq!(response.status, "tool_error");
    assert_eq!(response.action, "prometheus_inventory");
    assert!(response.executed);
    assert!(response
        .error
        .as_deref()
        .unwrap()
        .contains("not configured"));
}

#[tokio::test]
async fn direct_capability_execution_accepts_loki_log_summary() {
    let Json(response) = execute_capability(
        State(test_state().await),
        Json(ExecuteCapabilityRequest {
            action: AgentAction::LokiLogSummary {
                id: "act_loki".into(),
                reason: "logs".to_string(),
                query: r#"{namespace="apps-dev"}"#.to_string(),
                since_seconds: Some(900),
                limit: Some(25),
            },
            timeout_ms: None,
        }),
    )
    .await
    .unwrap();

    assert_eq!(response.status, "tool_error");
    assert_eq!(response.action, "loki_log_summary");
    assert!(response.executed);
    assert!(response
        .error
        .as_deref()
        .unwrap()
        .contains("not configured"));
}

#[tokio::test]
async fn direct_capability_execution_accepts_registry_inspection() {
    let state = test_state().await;
    let Json(response) = execute_capability(
        State(state.clone()),
        Json(ExecuteCapabilityRequest {
            action: AgentAction::RegistryInspectImage {
                id: "act_registry".into(),
                reason: "inspect image evidence".to_string(),
                image_ref: "team/checkout-api:v1".to_string(),
                registry_base_url: None,
            },
            timeout_ms: None,
        }),
    )
    .await
    .unwrap();

    assert_eq!(response.status, "ok");
    assert_eq!(response.action, "registry_inspect_image");
    assert!(response.executed);
    let result = response.result.unwrap();
    assert_eq!(result.content["source"], "registry");
    assert_eq!(result.content["image"]["repository"], "team/checkout-api");
    assert_eq!(result.content["verification_status"], "unknown");

    let Json(audit_events) = list_audit_events(
        State(state),
        Query(ListAuditEventsQuery {
            resource_kind: Some("capability".to_string()),
            resource_id: Some("registry_inspect_image".to_string()),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    assert!(audit_events.events.iter().any(|event| {
        event.kind == "direct_capability.executed"
            && event.payload["executed"] == true
            && event.payload["result"]["image"]["repository"] == "team/checkout-api"
            && event.payload["result"]["image"]["verification_status"] == "unknown"
    }));
}

#[tokio::test]
async fn registry_inspection_records_registry_evidence() {
    let state = test_state().await;
    let release_id = seed_approved_release(&state).await;
    let Json(response) = create_registry_evidence_from_registry_inspection(
        State(state.clone()),
        Json(CreateRegistryEvidenceFromInspectionRequest {
            release_id: release_id.clone(),
            image_ref: "team/checkout-api:v0.1.0-smoke".to_string(),
            registry_base_url: None,
            title: None,
            summary: None,
            risk_level: None,
            actor: Some("lucas".to_string()),
            reason: Some("registry inspection smoke".to_string()),
            timeout_ms: Some(5_000),
        }),
    )
    .await
    .unwrap();

    assert!(response.created);
    assert_eq!(response.inspection.status, "ok");
    assert!(response.inspection.executed);
    let evidence = response.registry_evidence.unwrap();
    assert_eq!(evidence.release_id, release_id);
    assert_eq!(evidence.status, "proposed");
    assert_eq!(evidence.source, "registry_inspect_image");
    assert_eq!(evidence.verification_status, "unknown");
    assert_eq!(evidence.repository.as_deref(), Some("team/checkout-api"));
    assert_eq!(
        evidence.image_ref.as_deref(),
        Some("team/checkout-api:v0.1.0-smoke")
    );
    assert_eq!(
        evidence.evidence_json["execution"]["capability"],
        "registry_inspect_image"
    );
    assert_eq!(
        evidence.evidence_json["execution"]["manifest_body_persisted"],
        false
    );

    let Json(registry_audit_events) = list_audit_events(
        State(state.clone()),
        Query(ListAuditEventsQuery {
            resource_kind: Some("registry_evidence".to_string()),
            resource_id: Some(evidence.id.clone()),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    assert!(registry_audit_events.events.iter().any(|event| {
        event.kind == "registry_evidence.proposed"
            && event.payload["extra"]["source"] == "registry_inspection"
            && event.payload["extra"]["execution_enabled"] == true
    }));

    let Json(capability_audit_events) = list_audit_events(
        State(state),
        Query(ListAuditEventsQuery {
            resource_kind: Some("capability".to_string()),
            resource_id: Some("registry_inspect_image".to_string()),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    assert!(capability_audit_events.events.iter().any(|event| {
        event.kind == "direct_capability.executed"
            && event.payload["executed"] == true
            && event.payload["result"]["image"]["repository"] == "team/checkout-api"
    }));
}

#[tokio::test]
async fn readiness_distinguishes_identity_evidence_from_supply_chain_evidence() {
    let state = test_state().await;
    let release_id = seed_approved_release(&state).await;
    let Json(identity_evidence) = create_registry_evidence_from_release(
        State(state.clone()),
        Json(CreateRegistryEvidenceFromReleaseRequest {
            release_id,
            title: None,
            summary: None,
            risk_level: None,
            registry: Some("registry.example.test".to_string()),
            repository: Some("checkout-api".to_string()),
            image_ref: Some("registry.example.test/checkout-api:v0.1.0-smoke".to_string()),
            image_digest: Some("sha256:deadbeef".to_string()),
            tag: Some("v0.1.0-smoke".to_string()),
            source: Some("registry_inspect_image".to_string()),
            verification_status: Some("verified".to_string()),
            evidence_json: None,
            actor: Some("lucas".to_string()),
            reason: Some("identity evidence smoke".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(verified_identity_evidence) = transition_registry_evidence(
        State(state.clone()),
        Path(identity_evidence.registry_evidence.id.clone()),
        Json(TransitionRegistryEvidenceRequest {
            target_status: "verified".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("operator accepted identity evidence".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(identity_readiness) = change_set_readiness(
        State(state.clone()),
        Path(
            verified_identity_evidence
                .registry_evidence
                .change_set_id
                .clone(),
        ),
    )
    .await
    .unwrap();

    assert!(identity_readiness
        .warnings
        .iter()
        .any(|finding| finding.code == "registry_evidence_supply_chain_not_verified"));

    let state = test_state().await;
    let release_id = seed_approved_release(&state).await;
    let Json(supply_chain_evidence) = create_registry_evidence_from_release(
        State(state.clone()),
        Json(CreateRegistryEvidenceFromReleaseRequest {
            release_id,
            title: None,
            summary: None,
            risk_level: None,
            registry: Some("registry.example.test".to_string()),
            repository: Some("checkout-api".to_string()),
            image_ref: Some("registry.example.test/checkout-api:v0.1.0-smoke".to_string()),
            image_digest: Some("sha256:deadbeef".to_string()),
            tag: Some("v0.1.0-smoke".to_string()),
            source: Some("registry_inspect_image".to_string()),
            verification_status: Some("verified".to_string()),
            evidence_json: Some(serde_json::json!({
                "verification": {
                    "checks": [
                        {"name": "cosign_signature", "status": "verified"}
                    ]
                }
            })),
            actor: Some("lucas".to_string()),
            reason: Some("signature evidence smoke".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(verified_supply_chain_evidence) = transition_registry_evidence(
        State(state.clone()),
        Path(supply_chain_evidence.registry_evidence.id.clone()),
        Json(TransitionRegistryEvidenceRequest {
            target_status: "verified".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("operator accepted signature evidence".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(supply_chain_readiness) = change_set_readiness(
        State(state),
        Path(
            verified_supply_chain_evidence
                .registry_evidence
                .change_set_id
                .clone(),
        ),
    )
    .await
    .unwrap();

    assert!(!supply_chain_readiness
        .warnings
        .iter()
        .any(|finding| finding.code == "registry_evidence_supply_chain_not_verified"));
}

#[tokio::test]
async fn direct_capability_execution_rejects_non_cluster_actions() {
    let error = execute_capability(
        State(test_state().await),
        Json(ExecuteCapabilityRequest {
            action: AgentAction::ListDir {
                id: "act_list".into(),
                reason: "list".to_string(),
                path: ".".into(),
                depth: 1,
                max_entries: None,
            },
            timeout_ms: None,
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(error.status, StatusCode::BAD_REQUEST);
}

#[test]
fn parses_sse_last_event_ids() {
    assert_eq!(parse_last_event_id("7"), Some(7));
    assert_eq!(
        parse_last_event_id("evt_run_1778887440941720000_12"),
        Some(12)
    );
    assert_eq!(parse_last_event_id("nonsense"), None);
}

#[test]
fn reads_last_event_id_header() {
    let mut headers = HeaderMap::new();
    headers.insert("last-event-id", HeaderValue::from_static("evt_run_test_4"));

    assert_eq!(last_event_seq(&headers), 4);
}

#[test]
fn stream_start_seq_prefers_query_cursor() {
    let mut headers = HeaderMap::new();
    headers.insert("last-event-id", HeaderValue::from_static("evt_run_test_4"));

    assert_eq!(
        stream_start_seq(&headers, &StreamRunEventsQuery { after_seq: Some(9) }),
        9
    );
    assert_eq!(
        stream_start_seq(&headers, &StreamRunEventsQuery { after_seq: None }),
        4
    );
}

#[tokio::test]
async fn lists_pending_approvals() {
    let state = test_state().await;

    let Json(created) = create_run(
        State(state.clone()),
        Json(CreateRunRequest {
            task: "write file".to_string(),
            cwd: Some(".".to_string()),
            max_turns: Some(12),
            policy_mode: None,
            scope: None,
        }),
    )
    .await
    .unwrap();
    state
        .store
        .set_run_created_by(&created.id, Some("lucas".to_string()))
        .await
        .unwrap();
    state
        .store
        .create_approval(CreateApproval {
            id: "appr_list".to_string(),
            session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
            run_id: created.id.clone(),
            status: "pending".to_string(),
            kind: "file_write".to_string(),
            summary: "write README.md".to_string(),
            risk_level: "medium".to_string(),
            run_scope_json: None,
            action_json: None,
            preview_json: None,
            resume_messages_json: None,
            turns_completed: 1,
        })
        .await
        .unwrap();

    let Json(response) = list_approvals(
        State(state.clone()),
        Query(ListApprovalsQuery {
            search: None,
            status: Some("pending".to_string()),
            origin: Some("operator".to_string()),
            actor: Some("lucas".to_string()),
            namespace: None,
            repo: None,
            branch: None,
            production_impacting: None,
            requested_after_ms: None,
            requested_before_ms: None,
            limit: Some(50),
            offset: None,
        }),
    )
    .await
    .unwrap();

    assert_eq!(response.approvals.len(), 1);
    assert_eq!(response.count, 1);
    assert_eq!(response.limit, 50);
    assert_eq!(response.offset, 0);
    assert_eq!(response.approvals[0].id, "appr_list");
    assert_eq!(response.approvals[0].created_by.as_deref(), Some("lucas"));

    state
        .store
        .create_approval(CreateApproval {
            id: "appr_scoped".to_string(),
            session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
            run_id: created.id,
            status: "pending".to_string(),
            kind: "file_write".to_string(),
            summary: "write scoped file".to_string(),
            risk_level: "medium".to_string(),
            run_scope_json: Some(serde_json::json!({
                "namespace": "apps-dev",
                "repo": "git@example.test/team/pharness.git",
                "branch": "feature/approval-filter",
                "production_impacting": false
            })),
            action_json: None,
            preview_json: None,
            resume_messages_json: None,
            turns_completed: 1,
        })
        .await
        .unwrap();
    let Json(scoped) = list_approvals(
        State(state.clone()),
        Query(ListApprovalsQuery {
            search: None,
            status: Some("pending".to_string()),
            origin: None,
            actor: None,
            namespace: Some("apps-dev".to_string()),
            repo: Some("git@example.test/team/pharness.git".to_string()),
            branch: Some("feature/approval-filter".to_string()),
            production_impacting: Some(false),
            requested_after_ms: Some(0),
            requested_before_ms: None,
            limit: Some(10),
            offset: Some(0),
        }),
    )
    .await
    .unwrap();

    assert_eq!(scoped.approvals.len(), 1);
    assert_eq!(scoped.approvals[0].id, "appr_scoped");

    let Json(summary) = approval_summary(
        State(state),
        Query(ApprovalSummaryQuery {
            status: Some("pending".to_string()),
            namespace: Some("apps-dev".to_string()),
            repo: Some("git@example.test/team/pharness.git".to_string()),
            branch: Some("feature/approval-filter".to_string()),
            production_impacting: Some(false),
            requested_after_ms: Some(0),
            requested_before_ms: None,
        }),
    )
    .await
    .unwrap();

    assert_eq!(summary.summary.total, 1);
    assert_eq!(
        summary.summary.by_status[0].value.as_deref(),
        Some("pending")
    );
    assert_eq!(
        summary.summary.by_namespace[0].value.as_deref(),
        Some("apps-dev")
    );
    assert_eq!(
        summary.summary.by_age_bucket[0].value.as_deref(),
        Some("lt_5m")
    );
}

#[tokio::test]
async fn gets_and_denies_approval_by_id() {
    let state = test_state().await;

    let Json(created) = create_run(
        State(state.clone()),
        Json(CreateRunRequest {
            task: "write file".to_string(),
            cwd: Some(".".to_string()),
            max_turns: Some(12),
            policy_mode: None,
            scope: None,
        }),
    )
    .await
    .unwrap();
    state
        .store
        .create_approval(CreateApproval {
            id: "appr_by_id".to_string(),
            session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
            run_id: created.id.clone(),
            status: "pending".to_string(),
            kind: "file_write".to_string(),
            summary: "write README.md".to_string(),
            risk_level: "medium".to_string(),
            run_scope_json: Some(serde_json::json!({
                "namespace": "apps-dev",
                "repo": "git@example.test/team/app.git",
                "branch": "feature/pharness",
                "production_impacting": false
            })),
            action_json: Some(
                serde_json::to_value(AgentAction::WriteFile {
                    id: "act_write".into(),
                    reason: "test".to_string(),
                    path: "README.md".into(),
                    content: "hello".to_string(),
                })
                .unwrap(),
            ),
            preview_json: Some(serde_json::json!({
                "kind": "file_write",
                "action": "write_file",
                "status": "ok",
                "path": "README.md"
            })),
            resume_messages_json: Some(serde_json::json!([])),
            turns_completed: 1,
        })
        .await
        .unwrap();
    state
        .store
        .mark_run_approval_required(
            &created.id,
            serde_json::json!({
                "status": "approval_required",
                "approval_id": "appr_by_id"
            }),
        )
        .await
        .unwrap();

    let Json(fetched) = get_approval(State(state.clone()), Path("appr_by_id".to_string()))
        .await
        .unwrap();
    let Json(decided) = deny_approval(
        State(state.clone()),
        Path("appr_by_id".to_string()),
        Json(ReviewApprovalRequest {
            decided_by: Some("operator".to_string()),
            reason: Some("not aligned".to_string()),
        }),
    )
    .await
    .unwrap();

    assert_eq!(fetched.status, "pending");
    assert_eq!(fetched.preview.as_ref().unwrap()["path"], "README.md");
    assert_eq!(decided.approval.status, "denied");
    assert_eq!(decided.run.status, "failed");
    let Json(audit_events) = list_audit_events(
        State(state),
        Query(ListAuditEventsQuery {
            resource_kind: Some("approval".to_string()),
            resource_id: Some("appr_by_id".to_string()),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    assert!(audit_events.events.iter().any(|event| {
        event.kind == "approval.denied"
            && event.actor.as_deref() == Some("operator")
            && event.payload["approval_id"] == "appr_by_id"
    }));
}

#[tokio::test]
async fn approval_by_id_refuses_non_current_pending_approval() {
    let state = test_state().await;

    let Json(created) = create_run(
        State(state.clone()),
        Json(CreateRunRequest {
            task: "write file".to_string(),
            cwd: Some(".".to_string()),
            max_turns: Some(12),
            policy_mode: None,
            scope: None,
        }),
    )
    .await
    .unwrap();
    for approval_id in ["appr_old", "appr_current"] {
        state
            .store
            .create_approval(CreateApproval {
                id: approval_id.to_string(),
                session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
                run_id: created.id.clone(),
                status: "pending".to_string(),
                kind: "file_write".to_string(),
                summary: format!("write from {approval_id}"),
                risk_level: "medium".to_string(),
                run_scope_json: None,
                action_json: Some(
                    serde_json::to_value(AgentAction::WriteFile {
                        id: format!("act_{approval_id}").into(),
                        reason: "test".to_string(),
                        path: "README.md".into(),
                        content: "hello".to_string(),
                    })
                    .unwrap(),
                ),
                preview_json: None,
                resume_messages_json: Some(serde_json::json!([])),
                turns_completed: 1,
            })
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }

    let error = deny_approval(
        State(state),
        Path("appr_old".to_string()),
        Json(ReviewApprovalRequest {
            decided_by: Some("operator".to_string()),
            reason: Some("stale".to_string()),
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(error.status, StatusCode::CONFLICT);
    assert!(error.message.contains("current pending approval"));
}

#[tokio::test]
async fn creates_sdlc_root_chain_and_audits_each_record() {
    let state = test_state().await;
    let Json(run) = create_run(
        State(state.clone()),
        Json(CreateRunRequest {
            task: "seed SDLC roots".to_string(),
            cwd: Some(".".to_string()),
            max_turns: Some(1),
            policy_mode: None,
            scope: None,
        }),
    )
    .await
    .unwrap();

    let Json(observation) = create_observation(
        State(state.clone()),
        Json(CreateObservationRequest {
            id: Some("obs_public_create".to_string()),
            session_id: None,
            run_id: Some(run.id.clone()),
            source: "smoke".to_string(),
            kind: "pipeline_run_analysis".to_string(),
            subject: "checkout-api".to_string(),
            summary: "pipeline pending approval".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("pr-smoke".to_string()),
            resource_ref: Some(serde_json::json!({
                "apiVersion": "tekton.dev/v1",
                "kind": "PipelineRun",
                "namespace": "apps-dev",
                "name": "pr-smoke"
            })),
            artifact_id: None,
            data_json: Some(serde_json::json!({ "status": "running" })),
            actor: Some("test".to_string()),
            reason: Some("root smoke".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(incident) = create_incident(
        State(state.clone()),
        Json(CreateIncidentRequest {
            id: Some("inc_public_create".to_string()),
            observation_id: observation.id.clone(),
            status: Some("candidate".to_string()),
            severity: "medium".to_string(),
            title: "Pipeline needs review".to_string(),
            summary: "Pipeline is still running".to_string(),
            resource_namespace: None,
            resource_kind: None,
            resource_name: None,
            data_json: Some(serde_json::json!({ "reason": "running" })),
            actor: Some("test".to_string()),
            reason: Some("root smoke".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(plan) = create_remediation_plan(
        State(state.clone()),
        Json(CreateRemediationPlanRequest {
            id: Some("rplan_public_create".to_string()),
            incident_id: incident.id.clone(),
            status: Some("draft".to_string()),
            title: "Review pipeline".to_string(),
            summary: "Collect read-only evidence before any mutation".to_string(),
            risk_level: "medium".to_string(),
            requires_approval: Some(true),
            resource_namespace: None,
            resource_kind: None,
            resource_name: None,
            plan_json: Some(serde_json::json!({ "steps": ["inspect pipeline"] })),
            actor: Some("test".to_string()),
            reason: Some("root smoke".to_string()),
        }),
    )
    .await
    .unwrap();

    let Json(observations) = list_observations(
        State(state.clone()),
        Query(ListObservationsQuery {
            subject: Some("checkout-api".to_string()),
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    let Json(incidents) = list_incidents(
        State(state.clone()),
        Query(ListIncidentsQuery {
            status: Some("candidate".to_string()),
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    let Json(plans) = list_remediation_plans(
        State(state.clone()),
        Query(ListRemediationPlansQuery {
            incident_id: Some(incident.id.clone()),
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();

    assert_eq!(observation.run_id, Some(run.id));
    assert_eq!(incident.resource_namespace.as_deref(), Some("apps-dev"));
    assert_eq!(plan.incident_id, incident.id);
    assert_eq!(observations.count, 1);
    assert_eq!(incidents.count, 1);
    assert_eq!(plans.count, 1);

    for (resource_kind, resource_id, event_kind) in [
        (
            "observation",
            observation.id.as_str(),
            "observation.created",
        ),
        ("incident", incident.id.as_str(), "incident.created"),
        (
            "remediation_plan",
            plan.id.as_str(),
            "remediation_plan.created",
        ),
    ] {
        let Json(audit_events) = list_audit_events(
            State(state.clone()),
            Query(ListAuditEventsQuery {
                resource_kind: Some(resource_kind.to_string()),
                resource_id: Some(resource_id.to_string()),
                run_id: None,
                limit: Some(50),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        assert!(audit_events
            .events
            .iter()
            .any(|event| event.kind == event_kind && event.actor.as_deref() == Some("test")));
    }
}

#[tokio::test]
async fn creates_lists_gets_and_revokes_permission_grants() {
    let state = test_state().await;

    let Json(created) = super::create_permission_grant(
        State(state.clone()),
        Json(CreatePermissionGrantRequest {
            subject: "agent:local-worker".to_string(),
            created_by: Some("lucas".to_string()),
            reason: "trusted local write smoke".to_string(),
            scope: serde_json::json!({
                "environment": "local",
                "capability_kinds": ["filesystem"]
            }),
            policy: serde_json::json!({
                "policy_mode": "trusted_writes"
            }),
            expires_at: Some("9999999999999".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(listed) = list_permission_grants(
        State(state.clone()),
        Query(ListPermissionGrantsQuery {
            status: Some("active".to_string()),
            limit: Some(50),
        }),
    )
    .await
    .unwrap();
    let Json(fetched) = get_permission_grant(State(state.clone()), Path(created.id.clone()))
        .await
        .unwrap();
    let Json(revoked) = revoke_permission_grant(
        State(state.clone()),
        Path(created.id.clone()),
        Json(RevokePermissionGrantRequest {
            revoked_by: Some("tester".to_string()),
            reason: Some("done".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(audit_events) = list_audit_events(
        State(state),
        Query(ListAuditEventsQuery {
            resource_kind: Some("permission_grant".to_string()),
            resource_id: Some(created.id.clone()),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();

    assert_eq!(created.status, "active");
    assert_eq!(listed.grants.len(), 1);
    assert_eq!(fetched.id, created.id);
    assert_eq!(revoked.status, "revoked");
    assert_eq!(revoked.revoked_by.as_deref(), Some("tester"));
    assert_eq!(audit_events.events.len(), 2);
    assert!(audit_events
        .events
        .iter()
        .any(|event| event.kind == "permission_grant.created"));
    assert!(audit_events
        .events
        .iter()
        .any(|event| event.kind == "permission_grant.created"
            && event.actor.as_deref() == Some("lucas")));
    assert!(audit_events
        .events
        .iter()
        .any(|event| event.kind == "permission_grant.revoked"
            && event.actor.as_deref() == Some("tester")));
}

#[test]
fn rejects_invalid_permission_grant_shape() {
    let error = validate_permission_grant_request(&CreatePermissionGrantRequest {
        subject: "".to_string(),
        created_by: None,
        reason: "test".to_string(),
        scope: serde_json::json!({}),
        policy: serde_json::json!({}),
        expires_at: None,
    })
    .unwrap_err();

    assert_eq!(error.status, StatusCode::BAD_REQUEST);
}

#[test]
fn rejects_permission_grant_without_environment_scope() {
    let error = validate_permission_grant_request(&CreatePermissionGrantRequest {
        subject: "agent:local-worker".to_string(),
        created_by: None,
        reason: "test".to_string(),
        scope: serde_json::json!({
            "capability_kinds": ["filesystem"],
        }),
        policy: serde_json::json!({
            "policy_mode": "trusted_writes"
        }),
        expires_at: None,
    })
    .unwrap_err();

    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert!(error.message.contains("scope.environment"));
}

#[tokio::test]
async fn returns_run_diff() {
    let state = test_state().await;

    let Json(created) = create_run(
        State(state.clone()),
        Json(CreateRunRequest {
            task: "write file".to_string(),
            cwd: Some(".".to_string()),
            max_turns: Some(12),
            policy_mode: None,
            scope: None,
        }),
    )
    .await
    .unwrap();
    state
        .store
        .create_file_change(CreateFileChange {
            id: "chg_test".to_string(),
            session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
            run_id: created.id.clone(),
            path: "README.md".to_string(),
            before_hash: None,
            after_hash: None,
            diff: "--- before\n+++ after".to_string(),
        })
        .await
        .unwrap();

    let Json(response) = get_run_diff(State(state), Path(created.id.to_string()))
        .await
        .unwrap();

    assert_eq!(response.changes.len(), 1);
    assert!(response.diff.contains("+++ after"));
}

#[tokio::test]
async fn returns_run_artifacts_and_single_artifact() {
    let state = test_state().await;

    let Json(created) = create_run(
        State(state.clone()),
        Json(CreateRunRequest {
            task: "observe".to_string(),
            cwd: Some(".".to_string()),
            max_turns: Some(12),
            policy_mode: None,
            scope: None,
        }),
    )
    .await
    .unwrap();
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_test".to_string(),
            session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
            run_id: Some(created.id.clone()),
            kind: "tool_result".to_string(),
            label: "Prometheus query".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(serde_json::json!({"result_count": 33})),
        })
        .await
        .unwrap();

    let Json(listed) = list_run_artifacts(State(state.clone()), Path(created.id.to_string()))
        .await
        .unwrap();
    let Json(fetched) = get_artifact(State(state), Path("art_test".to_string()))
        .await
        .unwrap();

    assert_eq!(listed.artifacts.len(), 1);
    assert_eq!(listed.artifacts[0].id, "art_test");
    assert_eq!(fetched.content_json.unwrap()["result_count"], 33);
}

#[tokio::test]
async fn returns_run_observations_and_single_observation() {
    let state = test_state().await;

    let Json(created) = create_run(
        State(state.clone()),
        Json(CreateRunRequest {
            task: "observe".to_string(),
            cwd: Some(".".to_string()),
            max_turns: Some(12),
            policy_mode: None,
            scope: None,
        }),
    )
    .await
    .unwrap();
    state
        .store
        .create_observation(CreateObservation {
            id: "obs_test".to_string(),
            session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
            run_id: Some(created.id.clone()),
            source: "prometheus".to_string(),
            kind: "query".to_string(),
            subject: "up".to_string(),
            summary: "read Prometheus instant query".to_string(),
            resource_namespace: None,
            resource_kind: Some("query".to_string()),
            resource_name: Some("up".to_string()),
            resource_ref_json: Some(serde_json::json!({
                "provider": "prometheus",
                "kind": "query",
                "name": "up"
            })),
            artifact_id: None,
            data_json: serde_json::json!({"result_count": 33}),
        })
        .await
        .unwrap();

    let Json(listed) = list_run_observations(State(state.clone()), Path(created.id.to_string()))
        .await
        .unwrap();
    let Json(filtered) = list_observations(
        State(state.clone()),
        Query(ListObservationsQuery {
            run_id: Some(created.id.to_string()),
            source: Some("prometheus".to_string()),
            kind: Some("query".to_string()),
            subject: Some("up".to_string()),
            resource_namespace: None,
            resource_kind: Some("query".to_string()),
            resource_name: Some("up".to_string()),
            observed_after_ms: Some(0),
            observed_before_ms: None,
            limit: Some(10),
            offset: Some(0),
        }),
    )
    .await
    .unwrap();
    let Json(fetched) = get_observation(State(state), Path("obs_test".to_string()))
        .await
        .unwrap();

    assert_eq!(listed.observations.len(), 1);
    assert_eq!(listed.count, 1);
    assert_eq!(listed.observations[0].id, "obs_test");
    assert_eq!(
        listed.observations[0].resource_kind.as_deref(),
        Some("query")
    );
    assert_eq!(listed.observations[0].resource_name.as_deref(), Some("up"));
    assert_eq!(filtered.observations.len(), 1);
    assert_eq!(filtered.count, 1);
    assert_eq!(filtered.limit, Some(10));
    assert_eq!(filtered.offset, Some(0));
    assert_eq!(filtered.observations[0].id, "obs_test");
    assert_eq!(fetched.data_json["result_count"], 33);
}

#[tokio::test]
async fn returns_filtered_incidents_and_single_incident() {
    let state = test_state().await;

    let Json(created) = create_run(
        State(state.clone()),
        Json(CreateRunRequest {
            task: "observe incident".to_string(),
            cwd: Some(".".to_string()),
            max_turns: Some(12),
            policy_mode: None,
            scope: None,
        }),
    )
    .await
    .unwrap();
    state
        .store
        .create_observation(CreateObservation {
            id: "obs_incident".to_string(),
            session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
            run_id: Some(created.id.clone()),
            source: "tekton".to_string(),
            kind: "pipeline_run_analysis".to_string(),
            subject: "build-app".to_string(),
            summary: "analyzed Tekton PipelineRun ci/build-app".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-app".to_string()),
            resource_ref_json: None,
            artifact_id: None,
            data_json: serde_json::json!({"status":"failed"}),
        })
        .await
        .unwrap();
    state
        .store
        .create_incident(CreateIncident {
            id: "inc_test".to_string(),
            observation_id: "obs_incident".to_string(),
            session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
            run_id: Some(created.id.clone()),
            status: "candidate".to_string(),
            severity: "high".to_string(),
            title: "Tekton PipelineRun issue: ci/build-app".to_string(),
            summary: "PipelineRun status is failed".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-app".to_string()),
            data_json: serde_json::json!({"reasons":["PipelineRun status is failed"]}),
        })
        .await
        .unwrap();
    state
        .store
        .create_remediation_plan(CreateRemediationPlan {
            id: "rplan_test".to_string(),
            incident_id: "inc_test".to_string(),
            session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
            run_id: Some(created.id.clone()),
            status: "draft".to_string(),
            title: "Draft remediation for ci/build-app".to_string(),
            summary: "Review Tekton evidence before proposing a mutation".to_string(),
            risk_level: "high".to_string(),
            requires_approval: true,
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-app".to_string()),
            plan_json: serde_json::json!({
                "mode": "read_only_draft",
                "approval_gates": ["pipeline_mutation", "cluster_mutation"],
            }),
        })
        .await
        .unwrap();
    state
        .store
        .create_approval_gate(CreateApprovalGate {
            id: "agate_test".to_string(),
            work_item_id: None,
            remediation_plan_id: Some("rplan_test".to_string()),
            incident_id: Some("inc_test".to_string()),
            session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
            run_id: Some(created.id.clone()),
            status: "pending".to_string(),
            gate_kind: "pipeline_mutation".to_string(),
            gate_order: 1,
            title: "Approve pipeline mutation".to_string(),
            summary: "Require approval before rerunning Tekton resources".to_string(),
            risk_level: "high".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-app".to_string()),
            gate_json: serde_json::json!({
                "required_before": "rerunning PipelineRun",
            }),
        })
        .await
        .unwrap();

    let Json(listed) = list_incidents(
        State(state.clone()),
        Query(ListIncidentsQuery {
            run_id: Some(created.id.to_string()),
            status: Some("candidate".to_string()),
            severity: Some("high".to_string()),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-app".to_string()),
            created_after_ms: Some(0),
            created_before_ms: None,
            limit: Some(10),
            offset: Some(0),
        }),
    )
    .await
    .unwrap();
    let Json(fetched) = get_incident(State(state.clone()), Path("inc_test".to_string()))
        .await
        .unwrap();
    let Json(listed_plans) = list_remediation_plans(
        State(state.clone()),
        Query(ListRemediationPlansQuery {
            incident_id: Some("inc_test".to_string()),
            run_id: Some(created.id.to_string()),
            status: Some("draft".to_string()),
            risk_level: Some("high".to_string()),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-app".to_string()),
            created_after_ms: Some(0),
            created_before_ms: None,
            limit: Some(10),
            offset: Some(0),
        }),
    )
    .await
    .unwrap();
    let Json(fetched_plan) =
        get_remediation_plan(State(state.clone()), Path("rplan_test".to_string()))
            .await
            .unwrap();
    let derivation_error = create_work_plan_from_remediation_plan(
        State(state.clone()),
        Json(CreateWorkPlanFromRemediationPlanRequest {
            remediation_plan_id: "rplan_test".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("attempted before plan review".to_string()),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(derivation_error.status, StatusCode::CONFLICT);
    let Json(proposed_remediation) = transition_remediation_plan(
        State(state.clone()),
        Path("rplan_test".to_string()),
        Json(TransitionRemediationPlanRequest {
            target_status: "proposed".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("draft recovery evidence reviewed".to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(proposed_remediation.remediation_plan.status, "proposed");
    let approval_error = transition_remediation_plan(
        State(state.clone()),
        Path("rplan_test".to_string()),
        Json(TransitionRemediationPlanRequest {
            target_status: "approved".to_string(),
            actor: None,
            reason: None,
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(approval_error.status, StatusCode::BAD_REQUEST);
    let Json(approved_remediation) = transition_remediation_plan(
        State(state.clone()),
        Path("rplan_test".to_string()),
        Json(TransitionRemediationPlanRequest {
            target_status: "approved".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("bounded recovery plan approved".to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(approved_remediation.remediation_plan.status, "approved");
    let Json(created_work_plan) = create_work_plan_from_remediation_plan(
        State(state.clone()),
        Json(CreateWorkPlanFromRemediationPlanRequest {
            remediation_plan_id: "rplan_test".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("create an execution-disabled recovery work plan".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(existing_work_plan) = create_work_plan_from_remediation_plan(
        State(state.clone()),
        Json(CreateWorkPlanFromRemediationPlanRequest {
            remediation_plan_id: "rplan_test".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("verify idempotent work plan lookup".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(listed_work_plans) = list_work_plans(
        State(state.clone()),
        Query(ListWorkPlansQuery {
            work_item_id: None,
            remediation_plan_id: Some("rplan_test".to_string()),
            incident_id: Some("inc_test".to_string()),
            run_id: Some(created.id.to_string()),
            status: Some("proposed".to_string()),
            origin: None,
            actor: None,
            risk_level: Some("high".to_string()),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-app".to_string()),
            created_after_ms: Some(0),
            created_before_ms: None,
            limit: Some(10),
            offset: Some(0),
        }),
    )
    .await
    .unwrap();
    let Json(fetched_work_plan) = get_work_plan(
        State(state.clone()),
        Path(created_work_plan.work_plan.id.clone()),
    )
    .await
    .unwrap();
    let Json(listed_gates) = list_approval_gates(
        State(state.clone()),
        Query(ListApprovalGatesQuery {
            search: None,
            work_item_id: None,
            remediation_plan_id: Some("rplan_test".to_string()),
            incident_id: Some("inc_test".to_string()),
            run_id: Some(created.id.to_string()),
            status: Some("pending".to_string()),
            origin: None,
            actor: None,
            gate_kind: Some("pipeline_mutation".to_string()),
            risk_level: Some("high".to_string()),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-app".to_string()),
            created_after_ms: Some(0),
            created_before_ms: None,
            limit: Some(10),
            offset: Some(0),
        }),
    )
    .await
    .unwrap();
    let Json(fetched_gate) =
        get_approval_gate(State(state.clone()), Path("agate_test".to_string()))
            .await
            .unwrap();
    let Json(gate_summary) = approval_gate_summary(
        State(state.clone()),
        Query(ApprovalGateSummaryQuery {
            work_item_id: None,
            remediation_plan_id: Some("rplan_test".to_string()),
            incident_id: Some("inc_test".to_string()),
            run_id: Some(created.id.to_string()),
            status: Some("pending".to_string()),
            gate_kind: Some("pipeline_mutation".to_string()),
            risk_level: Some("high".to_string()),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-app".to_string()),
            created_after_ms: Some(0),
            created_before_ms: None,
        }),
    )
    .await
    .unwrap();
    let Json(decided_gate) = satisfy_approval_gate(
        State(state.clone()),
        Path("agate_test".to_string()),
        Json(DecideApprovalGateRequest {
            decided_by: Some("lucas".to_string()),
            reason: Some("reviewed remediation smoke".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(gate_audit_events) = list_audit_events(
        State(state.clone()),
        Query(ListAuditEventsQuery {
            resource_kind: Some("approval_gate".to_string()),
            resource_id: Some("agate_test".to_string()),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();

    assert_eq!(listed.count, 1);
    assert_eq!(listed.limit, 10);
    assert_eq!(listed.offset, 0);
    assert_eq!(listed.incidents[0].id, "inc_test");
    assert_eq!(fetched.observation_id, "obs_incident");
    assert_eq!(fetched.severity, "high");
    assert_eq!(listed_plans.count, 1);
    assert_eq!(listed_plans.limit, 10);
    assert_eq!(listed_plans.offset, 0);
    assert_eq!(listed_plans.remediation_plans[0].id, "rplan_test");
    assert_eq!(fetched_plan.incident_id, "inc_test");
    assert!(fetched_plan.requires_approval);
    assert_eq!(fetched_plan.plan_json["mode"], "read_only_draft");
    assert!(created_work_plan.created);
    assert!(!existing_work_plan.created);
    assert_eq!(
        created_work_plan.work_plan.remediation_plan_id.as_deref(),
        Some("rplan_test")
    );
    assert_eq!(
        existing_work_plan.work_plan.id,
        created_work_plan.work_plan.id
    );
    assert_eq!(listed_work_plans.count, 1);
    assert_eq!(
        listed_work_plans.work_plans[0].id,
        created_work_plan.work_plan.id
    );
    assert_eq!(fetched_work_plan.incident_id.as_deref(), Some("inc_test"));
    assert!(!fetched_work_plan.work_plan_json["execution"]["enabled"]
        .as_bool()
        .unwrap());
    assert_eq!(listed_gates.count, 1);
    assert_eq!(listed_gates.limit, 10);
    assert_eq!(listed_gates.offset, 0);
    assert_eq!(listed_gates.approval_gates[0].id, "agate_test");
    assert_eq!(
        fetched_gate.remediation_plan_id.as_deref(),
        Some("rplan_test")
    );
    assert_eq!(fetched_gate.gate_kind, "pipeline_mutation");
    assert_eq!(gate_summary.summary.total, 1);
    assert_eq!(
        gate_summary.summary.by_status[0].value.as_deref(),
        Some("pending")
    );
    assert_eq!(
        gate_summary.summary.by_gate_kind[0].value.as_deref(),
        Some("pipeline_mutation")
    );
    assert_eq!(
        gate_summary.summary.by_resource_namespace[0]
            .value
            .as_deref(),
        Some("ci")
    );
    assert_eq!(decided_gate.approval_gate.status, "satisfied");
    assert_eq!(
        decided_gate.approval_gate.decided_by.as_deref(),
        Some("lucas")
    );
    assert!(gate_audit_events
        .events
        .iter()
        .any(|event| event.kind == "approval_gate.satisfied"));
}

#[tokio::test]
async fn transitions_and_revisions_stale_work_plan_gates() {
    let state = test_state().await;

    let Json(created) = create_run(
        State(state.clone()),
        Json(CreateRunRequest {
            task: "plan lifecycle".to_string(),
            cwd: Some(".".to_string()),
            max_turns: Some(12),
            policy_mode: None,
            scope: None,
        }),
    )
    .await
    .unwrap();
    let session_id = pharness_core::SessionId::new(format!("ses_{}", created.id.as_str()));
    state
        .store
        .create_observation(CreateObservation {
            id: "obs_plan_lifecycle".to_string(),
            session_id: session_id.clone(),
            run_id: Some(created.id.clone()),
            source: "tekton".to_string(),
            kind: "pipeline_run_analysis".to_string(),
            subject: "build-api".to_string(),
            summary: "PipelineRun needs operator review".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-api".to_string()),
            resource_ref_json: None,
            artifact_id: None,
            data_json: serde_json::json!({"status":"failed"}),
        })
        .await
        .unwrap();
    state
        .store
        .create_incident(CreateIncident {
            id: "inc_plan_lifecycle".to_string(),
            observation_id: "obs_plan_lifecycle".to_string(),
            session_id: session_id.clone(),
            run_id: Some(created.id.clone()),
            status: "candidate".to_string(),
            severity: "high".to_string(),
            title: "Tekton PipelineRun issue: ci/build-api".to_string(),
            summary: "PipelineRun status is failed".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-api".to_string()),
            data_json: serde_json::json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_remediation_plan(CreateRemediationPlan {
            id: "rplan_lifecycle".to_string(),
            incident_id: "inc_plan_lifecycle".to_string(),
            session_id: session_id.clone(),
            run_id: Some(created.id.clone()),
            status: "approved".to_string(),
            title: "Draft remediation for ci/build-api".to_string(),
            summary: "Review evidence before proposing mutation".to_string(),
            risk_level: "high".to_string(),
            requires_approval: true,
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-api".to_string()),
            plan_json: serde_json::json!({
                "steps": [{"id": "inspect"}],
                "approval_gates": ["pipeline_mutation"],
            }),
        })
        .await
        .unwrap();
    state
        .store
        .create_approval_gate(CreateApprovalGate {
            id: "agate_lifecycle".to_string(),
            work_item_id: None,
            remediation_plan_id: Some("rplan_lifecycle".to_string()),
            incident_id: Some("inc_plan_lifecycle".to_string()),
            session_id,
            run_id: Some(created.id.clone()),
            status: "pending".to_string(),
            gate_kind: "pipeline_mutation".to_string(),
            gate_order: 1,
            title: "Approve pipeline mutation".to_string(),
            summary: "Require approval before changing pipeline state".to_string(),
            risk_level: "high".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-api".to_string()),
            gate_json: serde_json::json!({}),
        })
        .await
        .unwrap();

    let Json(created_work_plan) = create_work_plan_from_remediation_plan(
        State(state.clone()),
        Json(CreateWorkPlanFromRemediationPlanRequest {
            remediation_plan_id: "rplan_lifecycle".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("derive reviewed remediation work plan".to_string()),
        }),
    )
    .await
    .unwrap();
    let work_plan_id = created_work_plan.work_plan.id.clone();
    let draft_envelope_error = create_work_plan_trusted_envelope(
        State(state.clone()),
        Path(work_plan_id.clone()),
        Json(CreateTrustedEnvelopeRequest {
            subject: None,
            created_by: Some("lucas".to_string()),
            reason: "premature WorkPlan envelope".to_string(),
            environment: Some("local".to_string()),
            namespace: Some("apps-dev".to_string()),
            repo: Some("git@example.test/team/app.git".to_string()),
            branch: Some("feature/pharness".to_string()),
            production_impacting: Some(false),
            expires_at: None,
        }),
    )
    .await
    .unwrap_err();
    let proposed = created_work_plan.clone();
    let Json(approved) = transition_work_plan(
        State(state.clone()),
        Path(work_plan_id.clone()),
        Json(TransitionWorkPlanRequest {
            target_status: "approved".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("bounded plan approved".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(work_plan_envelope) = create_work_plan_trusted_envelope(
        State(state.clone()),
        Path(work_plan_id.clone()),
        Json(CreateTrustedEnvelopeRequest {
            subject: None,
            created_by: Some("lucas".to_string()),
            reason: "bounded WorkPlan approved".to_string(),
            environment: Some("local".to_string()),
            namespace: Some("apps-dev".to_string()),
            repo: Some("git@example.test/team/app.git".to_string()),
            branch: Some("feature/pharness".to_string()),
            production_impacting: Some(false),
            expires_at: None,
        }),
    )
    .await
    .unwrap();
    let Json(satisfied_gate) = satisfy_approval_gate(
        State(state.clone()),
        Path("agate_lifecycle".to_string()),
        Json(DecideApprovalGateRequest {
            decided_by: Some("lucas".to_string()),
            reason: Some("pipeline mutation reviewed".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(ready_before_revision) =
        work_plan_readiness(State(state.clone()), Path(work_plan_id.clone()))
            .await
            .unwrap();
    let Json(revised) = revise_work_plan(
        State(state.clone()),
        Path(work_plan_id.clone()),
        Json(ReviseWorkPlanRequest {
            title: None,
            summary: Some("Revised after new evidence".to_string()),
            risk_level: None,
            requires_approval: None,
            work_plan_json: serde_json::json!({
                "steps": [{"id": "inspect"}, {"id": "prepare_changeset"}],
            }),
            actor: Some("lucas".to_string()),
            reason: Some("new evidence changed execution plan".to_string()),
            material_change: true,
        }),
    )
    .await
    .unwrap();
    let staled_grant = state
        .store
        .get_permission_grant(&work_plan_envelope.grant.id)
        .await
        .unwrap()
        .unwrap();
    let Json(future_run) = create_run(
        State(state.clone()),
        Json(CreateRunRequest {
            task: "future scoped write".to_string(),
            cwd: Some(".".to_string()),
            max_turns: Some(12),
            policy_mode: None,
            scope: Some(RunScope {
                run_id: None,
                namespace: Some("apps-dev".to_string()),
                repo: Some("git@example.test/team/app.git".to_string()),
                branch: Some("feature/pharness".to_string()),
                work_item_id: None,
                workspace_id: None,
                work_plan_id: Some(approved.work_plan.id.clone()),
                change_set_id: None,
                production_impacting: false,
            }),
        }),
    )
    .await
    .unwrap();
    let future_run = state.store.get_run(&future_run.id).await.unwrap().unwrap();
    let Json(blocked_after_revision) =
        work_plan_readiness(State(state.clone()), Path(approved.work_plan.id.clone()))
            .await
            .unwrap();
    let Json(work_plan_audit_events) = list_audit_events(
        State(state.clone()),
        Query(ListAuditEventsQuery {
            resource_kind: Some("work_plan".to_string()),
            resource_id: Some(work_plan_id),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    let Json(grant_audit_events) = list_audit_events(
        State(state.clone()),
        Query(ListAuditEventsQuery {
            resource_kind: Some("permission_grant".to_string()),
            resource_id: Some(work_plan_envelope.grant.id.clone()),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    let Json(gate_audit_events) = list_audit_events(
        State(state.clone()),
        Query(ListAuditEventsQuery {
            resource_kind: Some("approval_gate".to_string()),
            resource_id: Some("agate_lifecycle".to_string()),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();

    assert_eq!(draft_envelope_error.status, StatusCode::CONFLICT);
    assert_eq!(proposed.work_plan.status, "proposed");
    assert_eq!(approved.work_plan.status, "approved");
    assert_eq!(
        work_plan_envelope.grant.scope["work_plan_ids"][0],
        serde_json::json!(approved.work_plan.id.clone())
    );
    assert!(work_plan_envelope.grant.scope["change_set_ids"].is_null());
    assert_eq!(satisfied_gate.approval_gate.status, "satisfied");
    assert!(ready_before_revision.ready);
    assert!(ready_before_revision.blockers.is_empty());
    assert_eq!(ready_before_revision.trusted_envelopes.active.len(), 1);
    assert!(ready_before_revision
        .warnings
        .iter()
        .any(|finding| finding.code == "missing_change_set"));
    assert_eq!(revised.work_plan.status, "draft");
    assert_eq!(revised.work_plan.revision, 2);
    assert_eq!(staled_grant.status, "stale");
    assert_eq!(staled_grant.revoked_by.as_deref(), Some("lucas"));
    assert_eq!(
        staled_grant.revoke_reason.as_deref(),
        Some("new evidence changed execution plan")
    );
    assert!(
        future_run.execution_target_json["policy"]["permission_grants"]
            .as_array()
            .is_none_or(Vec::is_empty)
    );
    assert!(!blocked_after_revision.ready);
    assert!(blocked_after_revision
        .blockers
        .iter()
        .any(|finding| finding.code == "work_plan_not_approved"));
    assert!(blocked_after_revision
        .blockers
        .iter()
        .any(|finding| finding.code == "missing_active_trusted_envelope"));
    assert!(blocked_after_revision
        .warnings
        .iter()
        .any(|finding| finding.code == "stale_trusted_envelope"));
    assert_eq!(revised.invalidated_gates.len(), 1);
    assert_eq!(revised.invalidated_gates[0].status, "stale");
    assert_eq!(
        revised.invalidated_gates[0].stale_reason.as_deref(),
        Some("new evidence changed execution plan")
    );
    assert!(work_plan_audit_events
        .events
        .iter()
        .any(|event| event.kind == "work_plan.revised"));
    assert!(work_plan_audit_events
        .events
        .iter()
        .any(|event| event.kind == "work_plan.trusted_envelope_created"));
    assert!(grant_audit_events
        .events
        .iter()
        .any(|event| event.kind == "permission_grant.stale"));
    assert!(gate_audit_events
        .events
        .iter()
        .any(|event| event.kind == "approval_gate.stale"));
}

#[tokio::test]
async fn creates_transitions_and_revisions_stale_change_set_gates() {
    let state = test_state().await;

    let Json(created) = create_run(
        State(state.clone()),
        Json(CreateRunRequest {
            task: "change set lifecycle".to_string(),
            cwd: Some(".".to_string()),
            max_turns: Some(12),
            policy_mode: None,
            scope: None,
        }),
    )
    .await
    .unwrap();
    let session_id = pharness_core::SessionId::new(format!("ses_{}", created.id.as_str()));
    state
        .store
        .create_observation(CreateObservation {
            id: "obs_changeset_lifecycle".to_string(),
            session_id: session_id.clone(),
            run_id: Some(created.id.clone()),
            source: "tekton".to_string(),
            kind: "pipeline_run_analysis".to_string(),
            subject: "build-api".to_string(),
            summary: "PipelineRun needs code change review".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-api".to_string()),
            resource_ref_json: None,
            artifact_id: None,
            data_json: serde_json::json!({"status":"failed"}),
        })
        .await
        .unwrap();
    state
        .store
        .create_incident(CreateIncident {
            id: "inc_changeset_lifecycle".to_string(),
            observation_id: "obs_changeset_lifecycle".to_string(),
            session_id: session_id.clone(),
            run_id: Some(created.id.clone()),
            status: "candidate".to_string(),
            severity: "high".to_string(),
            title: "Tekton PipelineRun issue: ci/build-api".to_string(),
            summary: "PipelineRun status is failed".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-api".to_string()),
            data_json: serde_json::json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_remediation_plan(CreateRemediationPlan {
            id: "rplan_changeset".to_string(),
            incident_id: "inc_changeset_lifecycle".to_string(),
            session_id: session_id.clone(),
            run_id: Some(created.id.clone()),
            status: "approved".to_string(),
            title: "Draft remediation for ci/build-api".to_string(),
            summary: "Prepare a bounded source change".to_string(),
            risk_level: "high".to_string(),
            requires_approval: true,
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-api".to_string()),
            plan_json: serde_json::json!({
                "steps": [{"id": "prepare_changeset"}],
                "approval_gates": ["source_change"],
            }),
        })
        .await
        .unwrap();
    state
        .store
        .create_approval_gate(CreateApprovalGate {
            id: "agate_changeset".to_string(),
            work_item_id: None,
            remediation_plan_id: Some("rplan_changeset".to_string()),
            incident_id: Some("inc_changeset_lifecycle".to_string()),
            session_id,
            run_id: Some(created.id.clone()),
            status: "pending".to_string(),
            gate_kind: "source_change".to_string(),
            gate_order: 1,
            title: "Approve source change".to_string(),
            summary: "Require approval before applying proposed source changes".to_string(),
            risk_level: "high".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-api".to_string()),
            gate_json: serde_json::json!({}),
        })
        .await
        .unwrap();

    let Json(created_work_plan) = create_work_plan_from_remediation_plan(
        State(state.clone()),
        Json(CreateWorkPlanFromRemediationPlanRequest {
            remediation_plan_id: "rplan_changeset".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("derive reviewed remediation work plan".to_string()),
        }),
    )
    .await
    .unwrap();
    let proposed_work_plan = created_work_plan.clone();
    let Json(approved_work_plan) = transition_work_plan(
        State(state.clone()),
        Path(created_work_plan.work_plan.id.clone()),
        Json(TransitionWorkPlanRequest {
            target_status: "approved".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("source plan approved".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(work_plan_flow_before_change_set) = work_plan_flow(
        State(state.clone()),
        Path(created_work_plan.work_plan.id.clone()),
    )
    .await
    .unwrap();
    let Json(created_change_set) = create_change_set(
        State(state.clone()),
        Json(CreateChangeSetRequest {
            work_plan_id: created_work_plan.work_plan.id.clone(),
            title: Some("ChangeSet: fix build config".to_string()),
            summary: Some("Update build config for checkout-api".to_string()),
            risk_level: Some("medium".to_string()),
            change_set_json: serde_json::json!({
                "changes": [{
                    "path": "build/checkout-api.yaml",
                    "diff": "--- before\n+++ after\n-retries: 1\n+retries: 2",
                }],
                "rollback": "restore previous build config",
            }),
            actor: Some("lucas".to_string()),
            reason: Some("prepare bounded source change".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(existing_change_set) = create_change_set(
        State(state.clone()),
        Json(CreateChangeSetRequest {
            work_plan_id: created_work_plan.work_plan.id.clone(),
            title: None,
            summary: None,
            risk_level: None,
            change_set_json: serde_json::json!({"changes":[]}),
            actor: None,
            reason: None,
        }),
    )
    .await
    .unwrap();
    let change_set_id = created_change_set.change_set.id.clone();
    let original_hash = created_change_set.change_set.material_hash.clone();
    assert_eq!(work_plan_flow_before_change_set.resource_kind, "work_plan");
    assert_eq!(
        work_plan_flow_before_change_set.resource_id,
        created_work_plan.work_plan.id
    );
    assert_eq!(
        work_plan_flow_before_change_set.work_plan.id,
        approved_work_plan.work_plan.id
    );
    assert!(work_plan_flow_before_change_set.change_set.is_none());
    assert!(work_plan_flow_before_change_set.pipeline_intent.is_none());
    assert!(work_plan_flow_before_change_set
        .readiness
        .warnings
        .iter()
        .any(|finding| finding.code == "missing_change_set"));
    assert!(work_plan_flow_before_change_set
        .incidents
        .iter()
        .any(|incident| incident.id == "inc_changeset_lifecycle"));
    assert!(work_plan_flow_before_change_set
        .remediation_plans
        .iter()
        .any(|plan| plan.id == "rplan_changeset"));
    let draft_envelope_error = create_change_set_trusted_envelope(
        State(state.clone()),
        Path(change_set_id.clone()),
        Json(CreateTrustedEnvelopeRequest {
            subject: None,
            created_by: Some("lucas".to_string()),
            reason: "premature ChangeSet envelope".to_string(),
            environment: Some("local".to_string()),
            namespace: Some("apps-dev".to_string()),
            repo: Some("git@example.test/team/app.git".to_string()),
            branch: Some("feature/pharness".to_string()),
            production_impacting: Some(false),
            expires_at: None,
        }),
    )
    .await
    .unwrap_err();
    let Json(listed_change_sets) = list_change_sets(
        State(state.clone()),
        Query(ListChangeSetsQuery {
            work_item_id: None,
            work_plan_id: Some(created_work_plan.work_plan.id.clone()),
            remediation_plan_id: Some("rplan_changeset".to_string()),
            incident_id: Some("inc_changeset_lifecycle".to_string()),
            run_id: Some(created.id.to_string()),
            status: Some("draft".to_string()),
            risk_level: Some("medium".to_string()),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-api".to_string()),
            created_after_ms: Some(0),
            created_before_ms: None,
            limit: Some(10),
            offset: Some(0),
        }),
    )
    .await
    .unwrap();
    let Json(proposed) = transition_change_set(
        State(state.clone()),
        Path(change_set_id.clone()),
        Json(TransitionChangeSetRequest {
            target_status: "proposed".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("ready for source review".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(approved) = transition_change_set(
        State(state.clone()),
        Path(change_set_id.clone()),
        Json(TransitionChangeSetRequest {
            target_status: "approved".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("source change approved".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(change_set_envelope) = create_change_set_trusted_envelope(
        State(state.clone()),
        Path(change_set_id.clone()),
        Json(CreateTrustedEnvelopeRequest {
            subject: None,
            created_by: Some("lucas".to_string()),
            reason: "bounded ChangeSet approved".to_string(),
            environment: Some("local".to_string()),
            namespace: Some("apps-dev".to_string()),
            repo: Some("git@example.test/team/app.git".to_string()),
            branch: Some("feature/pharness".to_string()),
            production_impacting: Some(false),
            expires_at: None,
        }),
    )
    .await
    .unwrap();
    let Json(satisfied_gate) = satisfy_approval_gate(
        State(state.clone()),
        Path("agate_changeset".to_string()),
        Json(DecideApprovalGateRequest {
            decided_by: Some("lucas".to_string()),
            reason: Some("source change reviewed".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(proposed_pipeline_intent) = create_pipeline_intent_from_change_set(
        State(state.clone()),
        Json(CreatePipelineIntentFromChangeSetRequest {
            change_set_id: change_set_id.clone(),
            title: None,
            summary: None,
            risk_level: None,
            intent_kind: None,
            intent_json: None,
            actor: Some("lucas".to_string()),
            reason: Some("pipeline intent smoke".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(existing_pipeline_intent) = create_pipeline_intent_from_change_set(
        State(state.clone()),
        Json(CreatePipelineIntentFromChangeSetRequest {
            change_set_id: change_set_id.clone(),
            title: Some("ignored duplicate".to_string()),
            summary: None,
            risk_level: None,
            intent_kind: None,
            intent_json: None,
            actor: None,
            reason: None,
        }),
    )
    .await
    .unwrap();
    let Json(listed_pipeline_intents) = list_pipeline_intents(
        State(state.clone()),
        Query(ListPipelineIntentsQuery {
            change_set_id: Some(change_set_id.clone()),
            work_plan_id: Some(created_work_plan.work_plan.id.clone()),
            remediation_plan_id: Some("rplan_changeset".to_string()),
            incident_id: Some("inc_changeset_lifecycle".to_string()),
            run_id: Some(created.id.to_string()),
            status: Some("proposed".to_string()),
            intent_kind: Some("tekton_build_test_package".to_string()),
            risk_level: Some("medium".to_string()),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-api".to_string()),
            created_after_ms: Some(0),
            created_before_ms: None,
            limit: Some(10),
            offset: Some(0),
        }),
    )
    .await
    .unwrap();
    let Json(fetched_pipeline_intent) = get_pipeline_intent(
        State(state.clone()),
        Path(proposed_pipeline_intent.pipeline_intent.id.clone()),
    )
    .await
    .unwrap();
    let Json(waiting_on_pipeline_intent) =
        change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
            .await
            .unwrap();
    let Json(approved_pipeline_intent) = transition_pipeline_intent(
        State(state.clone()),
        Path(proposed_pipeline_intent.pipeline_intent.id.clone()),
        Json(TransitionPipelineIntentRequest {
            target_status: "approved".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("pipeline intent approved".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(pipeline_observation) = create_observation(
        State(state.clone()),
        Json(CreateObservationRequest {
            id: Some("obs_pipeline_intent_evidence".to_string()),
            session_id: None,
            run_id: None,
            source: "tekton".to_string(),
            kind: "pipeline_run_analysis".to_string(),
            subject: "ci/build-api".to_string(),
            summary: "PipelineRun build-api succeeded".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-api".to_string()),
            resource_ref: Some(json!({
                "source": "tekton",
                "kind": "PipelineRun",
                "namespace": "ci",
                "name": "build-api",
            })),
            artifact_id: None,
            data_json: Some(json!({
                "analysis": {
                    "kind": "PipelineRunAnalysis",
                    "summary": {
                        "status": "succeeded",
                        "reason": "Succeeded",
                        "task_run_count": 3,
                        "failed_task_run_count": 0,
                        "running_task_run_count": 0,
                        "succeeded_task_run_count": 3,
                        "argo_sync_status": "Synced",
                        "argo_health_status": "Healthy",
                        "image_alignment": {
                            "status": "exact_match"
                        }
                    }
                }
            })),
            actor: Some("lucas".to_string()),
            reason: Some("pipeline evidence fixture".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(pipeline_intent_with_evidence) = attach_pipeline_intent_evidence(
        State(state.clone()),
        Path(proposed_pipeline_intent.pipeline_intent.id.clone()),
        Json(AttachPipelineIntentEvidenceRequest {
            observation_id: pipeline_observation.id.clone(),
            actor: Some("lucas".to_string()),
            reason: Some("pipeline evidence reviewed".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(waiting_on_deployment_intent) =
        change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
            .await
            .unwrap();
    let Json(proposed_deployment_intent) = create_deployment_intent_from_pipeline_intent(
        State(state.clone()),
        Json(CreateDeploymentIntentFromPipelineIntentRequest {
            pipeline_intent_id: proposed_pipeline_intent.pipeline_intent.id.clone(),
            title: None,
            summary: None,
            risk_level: None,
            intent_kind: None,
            target_environment: Some("dev".to_string()),
            target_namespace: Some("apps-dev".to_string()),
            argo_application: Some("checkout-api".to_string()),
            intent_json: None,
            actor: Some("lucas".to_string()),
            reason: Some("deployment intent smoke".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(existing_deployment_intent) = create_deployment_intent_from_pipeline_intent(
        State(state.clone()),
        Json(CreateDeploymentIntentFromPipelineIntentRequest {
            pipeline_intent_id: proposed_pipeline_intent.pipeline_intent.id.clone(),
            title: Some("ignored duplicate".to_string()),
            summary: None,
            risk_level: None,
            intent_kind: None,
            target_environment: None,
            target_namespace: None,
            argo_application: None,
            intent_json: None,
            actor: None,
            reason: None,
        }),
    )
    .await
    .unwrap();
    let Json(listed_deployment_intents) = list_deployment_intents(
        State(state.clone()),
        Query(ListDeploymentIntentsQuery {
            pipeline_intent_id: Some(proposed_pipeline_intent.pipeline_intent.id.clone()),
            change_set_id: Some(change_set_id.clone()),
            work_plan_id: Some(created_work_plan.work_plan.id.clone()),
            remediation_plan_id: Some("rplan_changeset".to_string()),
            incident_id: Some("inc_changeset_lifecycle".to_string()),
            run_id: Some(created.id.to_string()),
            status: Some("proposed".to_string()),
            intent_kind: Some("argo_sync_deploy".to_string()),
            risk_level: Some("medium".to_string()),
            target_environment: Some("dev".to_string()),
            target_namespace: Some("apps-dev".to_string()),
            argo_application: Some("checkout-api".to_string()),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-api".to_string()),
            created_after_ms: Some(0),
            created_before_ms: None,
            limit: Some(10),
            offset: Some(0),
        }),
    )
    .await
    .unwrap();
    let Json(fetched_deployment_intent) = get_deployment_intent(
        State(state.clone()),
        Path(proposed_deployment_intent.deployment_intent.id.clone()),
    )
    .await
    .unwrap();
    let Json(waiting_on_deployment_approval) =
        change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
            .await
            .unwrap();
    let Json(approved_deployment_intent) = transition_deployment_intent(
        State(state.clone()),
        Path(proposed_deployment_intent.deployment_intent.id.clone()),
        Json(TransitionDeploymentIntentRequest {
            target_status: "approved".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("deployment intent approved".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(deployment_observation) = create_observation(
        State(state.clone()),
        Json(CreateObservationRequest {
            id: Some("obs_deployment_intent_evidence".to_string()),
            session_id: None,
            run_id: None,
            source: "argocd".to_string(),
            kind: "applications.argoproj.io".to_string(),
            subject: "checkout-api".to_string(),
            summary: "Argo Application checkout-api is synced and healthy".to_string(),
            resource_namespace: Some("argocd".to_string()),
            resource_kind: Some("Application".to_string()),
            resource_name: Some("checkout-api".to_string()),
            resource_ref: Some(json!({
                "source": "argocd",
                "kind": "Application",
                "namespace": "argocd",
                "name": "checkout-api",
            })),
            artifact_id: None,
            data_json: Some(json!({
                "source": "argocd",
                "resource": "applications.argoproj.io",
                "namespace": "argocd",
                "name": "checkout-api",
                "output": {
                    "apiVersion": "argoproj.io/v1alpha1",
                    "kind": "Application",
                    "metadata": {
                        "namespace": "argocd",
                        "name": "checkout-api"
                    },
                    "status": {
                        "sync": {
                            "status": "Synced",
                            "revision": "abc1234"
                        },
                        "health": {
                            "status": "Healthy"
                        }
                    }
                }
            })),
            actor: Some("lucas".to_string()),
            reason: Some("deployment evidence fixture".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(deployment_intent_with_evidence) = attach_deployment_intent_evidence(
        State(state.clone()),
        Path(proposed_deployment_intent.deployment_intent.id.clone()),
        Json(AttachDeploymentIntentEvidenceRequest {
            observation_id: deployment_observation.id.clone(),
            actor: Some("lucas".to_string()),
            reason: Some("deployment evidence reviewed".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(waiting_on_release) =
        change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
            .await
            .unwrap();
    let Json(proposed_release) = create_release_from_deployment_intent(
        State(state.clone()),
        Json(CreateReleaseFromDeploymentIntentRequest {
            deployment_intent_id: proposed_deployment_intent.deployment_intent.id.clone(),
            title: None,
            summary: None,
            risk_level: None,
            release_kind: None,
            version: Some("v0.1.0-smoke".to_string()),
            commit_sha: Some("abc1234".to_string()),
            image_digest: Some("sha256:deadbeef".to_string()),
            rollback_ref: Some("previous-release".to_string()),
            release_json: None,
            actor: Some("lucas".to_string()),
            reason: Some("release smoke".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(existing_release) = create_release_from_deployment_intent(
        State(state.clone()),
        Json(CreateReleaseFromDeploymentIntentRequest {
            deployment_intent_id: proposed_deployment_intent.deployment_intent.id.clone(),
            title: Some("ignored duplicate".to_string()),
            summary: None,
            risk_level: None,
            release_kind: None,
            version: None,
            commit_sha: None,
            image_digest: None,
            rollback_ref: None,
            release_json: None,
            actor: None,
            reason: None,
        }),
    )
    .await
    .unwrap();
    let Json(listed_releases) = list_releases(
        State(state.clone()),
        Query(ListReleasesQuery {
            deployment_intent_id: Some(proposed_deployment_intent.deployment_intent.id.clone()),
            pipeline_intent_id: Some(proposed_pipeline_intent.pipeline_intent.id.clone()),
            change_set_id: Some(change_set_id.clone()),
            work_plan_id: Some(created_work_plan.work_plan.id.clone()),
            remediation_plan_id: Some("rplan_changeset".to_string()),
            incident_id: Some("inc_changeset_lifecycle".to_string()),
            run_id: Some(created.id.to_string()),
            status: Some("proposed".to_string()),
            release_kind: Some("gitops_release".to_string()),
            risk_level: Some("medium".to_string()),
            target_environment: Some("dev".to_string()),
            target_namespace: Some("apps-dev".to_string()),
            argo_application: Some("checkout-api".to_string()),
            version: Some("v0.1.0-smoke".to_string()),
            commit_sha: Some("abc1234".to_string()),
            image_digest: Some("sha256:deadbeef".to_string()),
            created_after_ms: Some(0),
            created_before_ms: None,
            limit: Some(10),
            offset: Some(0),
        }),
    )
    .await
    .unwrap();
    let Json(fetched_release) = get_release(
        State(state.clone()),
        Path(proposed_release.release.id.clone()),
    )
    .await
    .unwrap();
    let Json(waiting_on_release_approval) =
        change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
            .await
            .unwrap();
    let Json(approved_release) = transition_release(
        State(state.clone()),
        Path(proposed_release.release.id.clone()),
        Json(TransitionReleaseRequest {
            target_status: "approved".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("release approved".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(release_observation) = create_observation(
        State(state.clone()),
        Json(CreateObservationRequest {
            id: Some("obs_release_observability".to_string()),
            session_id: None,
            run_id: None,
            source: "prometheus".to_string(),
            kind: "inventory".to_string(),
            subject: "prometheus/inventory".to_string(),
            summary: "Prometheus inventory has no active alerts".to_string(),
            resource_namespace: None,
            resource_kind: Some("PrometheusInventory".to_string()),
            resource_name: Some("default".to_string()),
            resource_ref: Some(json!({
                "source": "prometheus",
                "kind": "inventory",
            })),
            artifact_id: None,
            data_json: Some(json!({
                "source": "prometheus",
                "resource": "inventory",
                "inventory": {
                    "targets": {
                        "active_count": 3,
                        "unhealthy_count": 0
                    },
                    "rules": {
                        "rule_count": 2,
                        "problem_rule_count": 0
                    },
                    "alerts": {
                        "alert_count": 0
                    }
                }
            })),
            actor: Some("lucas".to_string()),
            reason: Some("release observability fixture".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(release_with_observability) = attach_release_evidence(
        State(state.clone()),
        Path(proposed_release.release.id.clone()),
        Json(AttachReleaseEvidenceRequest {
            observation_id: release_observation.id.clone(),
            actor: Some("lucas".to_string()),
            reason: Some("release observability reviewed".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(release_alert_observation) = create_observation(
        State(state.clone()),
        Json(CreateObservationRequest {
            id: Some("obs_release_observability_alert".to_string()),
            session_id: None,
            run_id: None,
            source: "prometheus".to_string(),
            kind: "inventory".to_string(),
            subject: "prometheus/inventory".to_string(),
            summary: "Prometheus inventory has active alerts".to_string(),
            resource_namespace: None,
            resource_kind: Some("PrometheusInventory".to_string()),
            resource_name: Some("default".to_string()),
            resource_ref: Some(json!({
                "source": "prometheus",
                "kind": "inventory",
            })),
            artifact_id: None,
            data_json: Some(json!({
                "source": "prometheus",
                "resource": "inventory",
                "inventory": {
                    "targets": {
                        "active_count": 3,
                        "unhealthy_count": 1
                    },
                    "rules": {
                        "rule_count": 2,
                        "problem_rule_count": 1
                    },
                    "alerts": {
                        "alert_count": 1
                    }
                }
            })),
            actor: Some("lucas".to_string()),
            reason: Some("release observability alert fixture".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(release_with_observability_incident) = attach_release_evidence(
        State(state.clone()),
        Path(proposed_release.release.id.clone()),
        Json(AttachReleaseEvidenceRequest {
            observation_id: release_alert_observation.id.clone(),
            actor: Some("lucas".to_string()),
            reason: Some("release alert evidence reviewed".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(waiting_on_registry_evidence) =
        change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
            .await
            .unwrap();
    let Json(proposed_registry_evidence) = create_registry_evidence_from_release(
        State(state.clone()),
        Json(CreateRegistryEvidenceFromReleaseRequest {
            release_id: proposed_release.release.id.clone(),
            title: None,
            summary: None,
            risk_level: None,
            registry: Some("registry.example.test".to_string()),
            repository: Some("checkout-api".to_string()),
            image_ref: Some("registry.example.test/checkout-api:v0.1.0-smoke".to_string()),
            image_digest: None,
            tag: Some("v0.1.0-smoke".to_string()),
            source: Some("manual".to_string()),
            verification_status: Some("verified".to_string()),
            evidence_json: None,
            actor: Some("lucas".to_string()),
            reason: Some("registry evidence smoke".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(existing_registry_evidence) = create_registry_evidence_from_release(
        State(state.clone()),
        Json(CreateRegistryEvidenceFromReleaseRequest {
            release_id: proposed_release.release.id.clone(),
            title: Some("ignored duplicate".to_string()),
            summary: None,
            risk_level: None,
            registry: None,
            repository: None,
            image_ref: None,
            image_digest: None,
            tag: None,
            source: None,
            verification_status: None,
            evidence_json: None,
            actor: None,
            reason: None,
        }),
    )
    .await
    .unwrap();
    let Json(listed_registry_evidence) = list_registry_evidence(
        State(state.clone()),
        Query(ListRegistryEvidenceQuery {
            release_id: Some(proposed_release.release.id.clone()),
            deployment_intent_id: Some(proposed_deployment_intent.deployment_intent.id.clone()),
            pipeline_intent_id: Some(proposed_pipeline_intent.pipeline_intent.id.clone()),
            change_set_id: Some(change_set_id.clone()),
            work_plan_id: Some(created_work_plan.work_plan.id.clone()),
            remediation_plan_id: Some("rplan_changeset".to_string()),
            incident_id: Some("inc_changeset_lifecycle".to_string()),
            run_id: Some(created.id.to_string()),
            status: Some("proposed".to_string()),
            risk_level: Some("medium".to_string()),
            registry: Some("registry.example.test".to_string()),
            repository: Some("checkout-api".to_string()),
            image_ref: None,
            image_digest: Some("sha256:deadbeef".to_string()),
            tag: Some("v0.1.0-smoke".to_string()),
            source: Some("manual".to_string()),
            verification_status: Some("verified".to_string()),
            created_after_ms: Some(0),
            created_before_ms: None,
            limit: Some(10),
            offset: Some(0),
        }),
    )
    .await
    .unwrap();
    let Json(fetched_registry_evidence) = get_registry_evidence(
        State(state.clone()),
        Path(proposed_registry_evidence.registry_evidence.id.clone()),
    )
    .await
    .unwrap();
    let Json(waiting_on_registry_evidence_verification) =
        change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
            .await
            .unwrap();
    let Json(verified_registry_evidence) = transition_registry_evidence(
        State(state.clone()),
        Path(proposed_registry_evidence.registry_evidence.id.clone()),
        Json(TransitionRegistryEvidenceRequest {
            target_status: "verified".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("registry evidence verified".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(ready_before_revision) =
        change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
            .await
            .unwrap();
    let Json(flow_before_revision) =
        change_set_flow(State(state.clone()), Path(change_set_id.clone()))
            .await
            .unwrap();
    let Json(revised) = revise_change_set(
            State(state.clone()),
            Path(change_set_id.clone()),
            Json(ReviseChangeSetRequest {
                title: None,
                summary: Some("Update build config and timeout".to_string()),
                risk_level: None,
                change_set_json: serde_json::json!({
                    "changes": [{
                        "path": "build/checkout-api.yaml",
                        "diff": "--- before\n+++ after\n-retries: 1\n+retries: 2\n-timeout: 60\n+timeout: 90",
                    }],
                    "rollback": "restore previous build config",
                }),
                actor: Some("lucas".to_string()),
                reason: Some("source change payload changed".to_string()),
                material_change: true,
            }),
        )
        .await
        .unwrap();
    let staled_grant = state
        .store
        .get_permission_grant(&change_set_envelope.grant.id)
        .await
        .unwrap()
        .unwrap();
    let Json(future_run) = create_run(
        State(state.clone()),
        Json(CreateRunRequest {
            task: "future scoped changeset write".to_string(),
            cwd: Some(".".to_string()),
            max_turns: Some(12),
            policy_mode: None,
            scope: Some(RunScope {
                run_id: None,
                namespace: Some("apps-dev".to_string()),
                repo: Some("git@example.test/team/app.git".to_string()),
                branch: Some("feature/pharness".to_string()),
                work_item_id: None,
                workspace_id: None,
                work_plan_id: Some(created_work_plan.work_plan.id.clone()),
                change_set_id: Some(change_set_id.clone()),
                production_impacting: false,
            }),
        }),
    )
    .await
    .unwrap();
    let future_run = state.store.get_run(&future_run.id).await.unwrap().unwrap();
    let Json(blocked_after_revision) =
        change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
            .await
            .unwrap();
    let Json(_reproposed_change_set) = transition_change_set(
        State(state.clone()),
        Path(change_set_id.clone()),
        Json(TransitionChangeSetRequest {
            target_status: "proposed".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("source change ready again".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(_approved_revised_change_set) = transition_change_set(
        State(state.clone()),
        Path(change_set_id.clone()),
        Json(TransitionChangeSetRequest {
            target_status: "approved".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("revised source change approved".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(reproposed_pipeline_intent) = create_pipeline_intent_from_change_set(
        State(state.clone()),
        Json(CreatePipelineIntentFromChangeSetRequest {
            change_set_id: change_set_id.clone(),
            title: None,
            summary: None,
            risk_level: None,
            intent_kind: None,
            intent_json: None,
            actor: Some("lucas".to_string()),
            reason: Some("pipeline intent after source revision".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(waiting_on_reproposed_pipeline_intent) =
        change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
            .await
            .unwrap();
    let Json(approved_reproposed_pipeline_intent) = transition_pipeline_intent(
        State(state.clone()),
        Path(proposed_pipeline_intent.pipeline_intent.id.clone()),
        Json(TransitionPipelineIntentRequest {
            target_status: "approved".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("reproposed pipeline intent approved".to_string()),
        }),
    )
    .await
    .unwrap();
    state
        .store
        .create_observation(CreateObservation {
            id: "obs_reproposed_pipeline_evidence".to_string(),
            session_id: SessionId::new(format!("ses_{}", created.id.as_str())),
            run_id: Some(created.id.clone()),
            source: "tekton".to_string(),
            kind: "pipeline_run_analysis".to_string(),
            subject: "build-api".to_string(),
            summary: "Reproposed PipelineRun completed successfully".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-api".to_string()),
            resource_ref_json: None,
            artifact_id: None,
            data_json: json!({
                "analysis": {
                    "summary": {
                        "status": "succeeded",
                        "failed_task_run_count": 0,
                        "running_task_run_count": 0,
                        "succeeded_task_run_count": 1,
                        "image_alignment": { "status": "exact_match" }
                    }
                }
            }),
        })
        .await
        .unwrap();
    let Json(_reproposed_pipeline_evidence) = attach_pipeline_intent_evidence(
        State(state.clone()),
        Path(proposed_pipeline_intent.pipeline_intent.id.clone()),
        Json(AttachPipelineIntentEvidenceRequest {
            observation_id: "obs_reproposed_pipeline_evidence".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("reproposed PipelineRun evidence reviewed".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(waiting_on_reproposed_deployment_intent) =
        change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
            .await
            .unwrap();
    let Json(reproposed_deployment_intent) = create_deployment_intent_from_pipeline_intent(
        State(state.clone()),
        Json(CreateDeploymentIntentFromPipelineIntentRequest {
            pipeline_intent_id: proposed_pipeline_intent.pipeline_intent.id.clone(),
            title: None,
            summary: None,
            risk_level: None,
            intent_kind: None,
            target_environment: Some("dev".to_string()),
            target_namespace: Some("apps-dev".to_string()),
            argo_application: Some("checkout-api".to_string()),
            intent_json: None,
            actor: Some("lucas".to_string()),
            reason: Some("deployment intent after pipeline reproposal".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(waiting_on_reproposed_deployment_approval) =
        change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
            .await
            .unwrap();
    let Json(approved_reproposed_deployment_intent) = transition_deployment_intent(
        State(state.clone()),
        Path(proposed_deployment_intent.deployment_intent.id.clone()),
        Json(TransitionDeploymentIntentRequest {
            target_status: "approved".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("reproposed deployment intent approved".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(waiting_on_reproposed_release) =
        change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
            .await
            .unwrap();
    let Json(reproposed_release) = create_release_from_deployment_intent(
        State(state.clone()),
        Json(CreateReleaseFromDeploymentIntentRequest {
            deployment_intent_id: proposed_deployment_intent.deployment_intent.id.clone(),
            title: None,
            summary: None,
            risk_level: None,
            release_kind: None,
            version: Some("v0.1.1-smoke".to_string()),
            commit_sha: Some("def5678".to_string()),
            image_digest: Some("sha256:feedface".to_string()),
            rollback_ref: Some(proposed_release.release.id.clone()),
            release_json: None,
            actor: Some("lucas".to_string()),
            reason: Some("release after deployment reproposal".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(waiting_on_reproposed_release_approval) =
        change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
            .await
            .unwrap();
    let Json(approved_reproposed_release) = transition_release(
        State(state.clone()),
        Path(proposed_release.release.id.clone()),
        Json(TransitionReleaseRequest {
            target_status: "approved".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("reproposed release approved".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(waiting_on_reproposed_registry_evidence) =
        change_set_readiness(State(state.clone()), Path(change_set_id.clone()))
            .await
            .unwrap();
    let Json(reproposed_registry_evidence) = create_registry_evidence_from_release(
        State(state.clone()),
        Json(CreateRegistryEvidenceFromReleaseRequest {
            release_id: proposed_release.release.id.clone(),
            title: None,
            summary: None,
            risk_level: None,
            registry: Some("registry.example.test".to_string()),
            repository: Some("checkout-api".to_string()),
            image_ref: Some("registry.example.test/checkout-api:v0.1.1-smoke".to_string()),
            image_digest: None,
            tag: Some("v0.1.1-smoke".to_string()),
            source: Some("manual".to_string()),
            verification_status: Some("verified".to_string()),
            evidence_json: None,
            actor: Some("lucas".to_string()),
            reason: Some("registry evidence after release reproposal".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(verified_reproposed_registry_evidence) = transition_registry_evidence(
        State(state.clone()),
        Path(proposed_registry_evidence.registry_evidence.id.clone()),
        Json(TransitionRegistryEvidenceRequest {
            target_status: "verified".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("reproposed registry evidence verified".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(revised_work_plan) = revise_work_plan(
        State(state.clone()),
        Path(created_work_plan.work_plan.id.clone()),
        Json(ReviseWorkPlanRequest {
            title: None,
            summary: Some("Plan changed after source review".to_string()),
            risk_level: None,
            requires_approval: None,
            work_plan_json: serde_json::json!({
                "steps": [{"id": "prepare_changeset"}, {"id": "rerun_tests"}],
            }),
            actor: Some("lucas".to_string()),
            reason: Some("plan changed after source review".to_string()),
            material_change: true,
        }),
    )
    .await
    .unwrap();
    let Json(change_set_audit_events) = list_audit_events(
        State(state.clone()),
        Query(ListAuditEventsQuery {
            resource_kind: Some("change_set".to_string()),
            resource_id: Some(change_set_id.clone()),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    let Json(grant_audit_events) = list_audit_events(
        State(state.clone()),
        Query(ListAuditEventsQuery {
            resource_kind: Some("permission_grant".to_string()),
            resource_id: Some(change_set_envelope.grant.id.clone()),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    let Json(pipeline_intent_audit_events) = list_audit_events(
        State(state.clone()),
        Query(ListAuditEventsQuery {
            resource_kind: Some("pipeline_intent".to_string()),
            resource_id: Some(proposed_pipeline_intent.pipeline_intent.id.clone()),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    let Json(deployment_intent_audit_events) = list_audit_events(
        State(state.clone()),
        Query(ListAuditEventsQuery {
            resource_kind: Some("deployment_intent".to_string()),
            resource_id: Some(proposed_deployment_intent.deployment_intent.id.clone()),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    let Json(release_audit_events) = list_audit_events(
        State(state.clone()),
        Query(ListAuditEventsQuery {
            resource_kind: Some("release".to_string()),
            resource_id: Some(proposed_release.release.id.clone()),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    let Json(registry_evidence_audit_events) = list_audit_events(
        State(state.clone()),
        Query(ListAuditEventsQuery {
            resource_kind: Some("registry_evidence".to_string()),
            resource_id: Some(proposed_registry_evidence.registry_evidence.id.clone()),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    let Json(gate_audit_events) = list_audit_events(
        State(state.clone()),
        Query(ListAuditEventsQuery {
            resource_kind: Some("approval_gate".to_string()),
            resource_id: Some("agate_changeset".to_string()),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();

    assert!(created_change_set.created);
    assert!(!existing_change_set.created);
    assert_eq!(listed_change_sets.count, 1);
    assert_eq!(listed_change_sets.change_sets[0].revision, 1);
    assert_eq!(proposed_work_plan.work_plan.status, "proposed");
    assert_eq!(approved_work_plan.work_plan.status, "approved");
    assert_eq!(draft_envelope_error.status, StatusCode::CONFLICT);
    assert_eq!(proposed.change_set.status, "proposed");
    assert_eq!(approved.change_set.status, "approved");
    assert_eq!(
        change_set_envelope.grant.scope["work_plan_ids"][0],
        serde_json::json!(created_work_plan.work_plan.id.clone())
    );
    assert_eq!(
        change_set_envelope.grant.scope["change_set_ids"][0],
        serde_json::json!(change_set_id.clone())
    );
    assert_eq!(satisfied_gate.approval_gate.status, "satisfied");
    assert!(proposed_pipeline_intent.created);
    assert!(!existing_pipeline_intent.created);
    assert_eq!(
        existing_pipeline_intent.pipeline_intent.id,
        proposed_pipeline_intent.pipeline_intent.id
    );
    assert_eq!(listed_pipeline_intents.count, 1);
    assert_eq!(
        fetched_pipeline_intent.id,
        proposed_pipeline_intent.pipeline_intent.id
    );
    assert_eq!(proposed_pipeline_intent.pipeline_intent.status, "proposed");
    assert_eq!(
        proposed_pipeline_intent.pipeline_intent.intent_kind,
        "tekton_build_test_package"
    );
    assert!(
        !proposed_pipeline_intent.pipeline_intent.intent_json["execution"]["enabled"]
            .as_bool()
            .unwrap()
    );
    assert!(waiting_on_pipeline_intent.ready);
    assert!(waiting_on_pipeline_intent
        .warnings
        .iter()
        .any(|finding| finding.code == "pipeline_intent_not_approved"));
    assert_eq!(approved_pipeline_intent.pipeline_intent.status, "approved");
    assert_eq!(
        pipeline_intent_with_evidence
            .pipeline_intent
            .intent_json
            .pointer("/evidence/status"),
        Some(&json!("satisfied"))
    );
    assert_eq!(
        pipeline_intent_with_evidence
            .pipeline_intent
            .intent_json
            .pointer("/evidence/observation_id"),
        Some(&json!("obs_pipeline_intent_evidence"))
    );
    assert_eq!(
        pipeline_intent_with_evidence.observation.id,
        pipeline_observation.id
    );
    assert!(waiting_on_deployment_intent.ready);
    assert!(waiting_on_deployment_intent
        .warnings
        .iter()
        .any(|finding| finding.code == "missing_deployment_intent"));
    assert!(proposed_deployment_intent.created);
    assert!(!existing_deployment_intent.created);
    assert_eq!(
        existing_deployment_intent.deployment_intent.id,
        proposed_deployment_intent.deployment_intent.id
    );
    assert_eq!(listed_deployment_intents.count, 1);
    assert_eq!(
        fetched_deployment_intent.id,
        proposed_deployment_intent.deployment_intent.id
    );
    assert_eq!(
        proposed_deployment_intent.deployment_intent.status,
        "proposed"
    );
    assert_eq!(
        proposed_deployment_intent.deployment_intent.intent_kind,
        "argo_sync_deploy"
    );
    assert_eq!(
        proposed_deployment_intent
            .deployment_intent
            .target_environment
            .as_deref(),
        Some("dev")
    );
    assert_eq!(
        proposed_deployment_intent
            .deployment_intent
            .target_namespace
            .as_deref(),
        Some("apps-dev")
    );
    assert_eq!(
        proposed_deployment_intent
            .deployment_intent
            .argo_application
            .as_deref(),
        Some("checkout-api")
    );
    assert!(
        !proposed_deployment_intent.deployment_intent.intent_json["execution"]["enabled"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(
        proposed_deployment_intent
            .deployment_intent
            .intent_json
            .pointer("/pipeline_evidence/status"),
        Some(&json!("satisfied"))
    );
    assert_eq!(
        proposed_deployment_intent
            .deployment_intent
            .intent_json
            .pointer("/pipeline_evidence/deploy_ready"),
        Some(&json!(true))
    );
    assert_eq!(
        proposed_deployment_intent
            .deployment_intent
            .intent_json
            .pointer("/pipeline_evidence/observation_id"),
        Some(&json!("obs_pipeline_intent_evidence"))
    );
    assert!(waiting_on_deployment_approval
        .warnings
        .iter()
        .any(|finding| finding.code == "deployment_intent_not_approved"));
    assert_eq!(
        approved_deployment_intent.deployment_intent.status,
        "approved"
    );
    assert_eq!(
        deployment_intent_with_evidence
            .deployment_intent
            .intent_json
            .pointer("/deployment_evidence/status"),
        Some(&json!("satisfied"))
    );
    assert_eq!(
        deployment_intent_with_evidence
            .deployment_intent
            .intent_json
            .pointer("/deployment_evidence/deploy_ready"),
        Some(&json!(true))
    );
    assert_eq!(
        deployment_intent_with_evidence
            .deployment_intent
            .intent_json
            .pointer("/deployment_evidence/observation_id"),
        Some(&json!("obs_deployment_intent_evidence"))
    );
    assert_eq!(
        deployment_intent_with_evidence.observation.id,
        deployment_observation.id
    );
    assert!(waiting_on_release.ready);
    assert!(waiting_on_release
        .warnings
        .iter()
        .any(|finding| finding.code == "missing_release"));
    assert!(proposed_release.created);
    assert!(!existing_release.created);
    assert_eq!(existing_release.release.id, proposed_release.release.id);
    assert_eq!(listed_releases.count, 1);
    assert_eq!(fetched_release.id, proposed_release.release.id);
    assert_eq!(proposed_release.release.status, "proposed");
    assert_eq!(proposed_release.release.release_kind, "gitops_release");
    assert_eq!(
        proposed_release.release.target_environment.as_deref(),
        Some("dev")
    );
    assert_eq!(
        proposed_release.release.target_namespace.as_deref(),
        Some("apps-dev")
    );
    assert_eq!(
        proposed_release.release.argo_application.as_deref(),
        Some("checkout-api")
    );
    assert_eq!(
        proposed_release.release.version.as_deref(),
        Some("v0.1.0-smoke")
    );
    assert!(
        !proposed_release.release.release_json["execution"]["enabled"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(
        proposed_release
            .release
            .release_json
            .pointer("/deployment_evidence/status"),
        Some(&json!("satisfied"))
    );
    assert_eq!(
        proposed_release
            .release
            .release_json
            .pointer("/deployment_evidence/release_ready"),
        Some(&json!(true))
    );
    assert_eq!(
        proposed_release
            .release
            .release_json
            .pointer("/deployment_evidence/observation_id"),
        Some(&json!("obs_deployment_intent_evidence"))
    );
    assert!(waiting_on_release_approval
        .warnings
        .iter()
        .any(|finding| finding.code == "release_not_approved"));
    assert_eq!(approved_release.release.status, "approved");
    assert_eq!(release_with_observability.release.status, "approved");
    assert_eq!(
        release_with_observability
            .release
            .release_json
            .pointer("/observability_evidence/0/observation_id"),
        Some(&json!("obs_release_observability"))
    );
    assert_eq!(
        release_with_observability
            .release
            .release_json
            .pointer("/observability_evidence/0/status"),
        Some(&json!("observed"))
    );
    assert_eq!(
        release_with_observability.observation.id,
        release_observation.id
    );
    assert!(release_with_observability.incident.is_none());
    assert!(release_with_observability.remediation_plan.is_none());
    let release_incident = release_with_observability_incident
        .incident
        .as_ref()
        .expect("attention-required release observability should create an incident");
    let release_remediation_plan = release_with_observability_incident
        .remediation_plan
        .as_ref()
        .expect("attention-required release observability should create a remediation plan");
    let release_remediation_gates = state
        .store
        .list_approval_gates(ApprovalGateListFilter {
            remediation_plan_id: Some(release_remediation_plan.id.clone()),
            incident_id: Some(release_incident.id.clone()),
            limit: 20,
            ..ApprovalGateListFilter::default()
        })
        .await
        .unwrap();
    assert_eq!(release_incident.status, "candidate");
    assert_eq!(release_incident.severity, "high");
    assert_eq!(
        release_incident.observation_id,
        "obs_release_observability_alert"
    );
    assert_eq!(release_remediation_plan.status, "draft");
    assert_eq!(release_remediation_plan.incident_id, release_incident.id);
    assert!(release_remediation_plan.requires_approval);
    assert_eq!(
        release_remediation_plan.plan_json.pointer("/source"),
        Some(&json!("release_observability_evidence"))
    );
    assert_eq!(release_remediation_gates.len(), 4);
    assert!(release_remediation_gates
        .iter()
        .any(|gate| gate.gate_kind == "cluster_mutation"));
    assert!(release_remediation_gates
        .iter()
        .all(|gate| gate.status == "pending"));
    assert_eq!(
        release_with_observability_incident
            .release
            .release_json
            .pointer("/observability_evidence/1/status"),
        Some(&json!("attention_required"))
    );
    assert!(waiting_on_registry_evidence
        .warnings
        .iter()
        .any(|finding| finding.code == "missing_registry_evidence"));
    assert!(!waiting_on_registry_evidence
        .warnings
        .iter()
        .any(|finding| finding.code == "missing_release_observability_evidence"));
    assert!(waiting_on_registry_evidence
        .warnings
        .iter()
        .any(|finding| finding.code == "release_observability_attention_required"));
    assert!(proposed_registry_evidence.created);
    assert!(!existing_registry_evidence.created);
    assert_eq!(
        existing_registry_evidence.registry_evidence.id,
        proposed_registry_evidence.registry_evidence.id
    );
    assert_eq!(listed_registry_evidence.count, 1);
    assert_eq!(
        fetched_registry_evidence.id,
        proposed_registry_evidence.registry_evidence.id
    );
    assert_eq!(
        proposed_registry_evidence.registry_evidence.status,
        "proposed"
    );
    assert_eq!(
        proposed_registry_evidence
            .registry_evidence
            .verification_status,
        "verified"
    );
    assert_eq!(
        proposed_registry_evidence
            .registry_evidence
            .image_digest
            .as_deref(),
        Some("sha256:deadbeef")
    );
    assert!(waiting_on_registry_evidence_verification
        .warnings
        .iter()
        .any(|finding| finding.code == "registry_evidence_not_verified"));
    assert_eq!(
        verified_registry_evidence.registry_evidence.status,
        "verified"
    );
    assert!(ready_before_revision.ready);
    assert!(ready_before_revision.blockers.is_empty());
    assert!(!ready_before_revision
        .warnings
        .iter()
        .any(|finding| finding.code == "pipeline_intent_not_approved"));
    assert!(!ready_before_revision
        .warnings
        .iter()
        .any(|finding| finding.code == "missing_deployment_intent"));
    assert!(!ready_before_revision
        .warnings
        .iter()
        .any(|finding| finding.code == "deployment_intent_not_approved"));
    assert!(!ready_before_revision
        .warnings
        .iter()
        .any(|finding| finding.code == "missing_release"));
    assert!(!ready_before_revision
        .warnings
        .iter()
        .any(|finding| finding.code == "release_not_approved"));
    assert!(!ready_before_revision
        .warnings
        .iter()
        .any(|finding| finding.code == "missing_registry_evidence"));
    assert!(!ready_before_revision
        .warnings
        .iter()
        .any(|finding| finding.code == "registry_evidence_not_verified"));
    assert_eq!(
        ready_before_revision
            .deployment_intent
            .as_ref()
            .map(|intent| intent.id.as_str()),
        Some(approved_deployment_intent.deployment_intent.id.as_str())
    );
    assert_eq!(
        ready_before_revision
            .release
            .as_ref()
            .map(|release| release.id.as_str()),
        Some(approved_release.release.id.as_str())
    );
    assert_eq!(
        ready_before_revision
            .registry_evidence
            .as_ref()
            .map(|evidence| evidence.id.as_str()),
        Some(verified_registry_evidence.registry_evidence.id.as_str())
    );
    assert_eq!(ready_before_revision.trusted_envelopes.active.len(), 1);
    assert_eq!(flow_before_revision.resource_kind, "change_set");
    assert_eq!(flow_before_revision.resource_id, change_set_id);
    assert!(flow_before_revision.readiness.ready);
    assert_eq!(
        flow_before_revision
            .change_set
            .as_ref()
            .map(|change_set| change_set.id.as_str()),
        Some(approved.change_set.id.as_str())
    );
    assert_eq!(
        flow_before_revision
            .pipeline_intent
            .as_ref()
            .map(|intent| intent.id.as_str()),
        Some(approved_pipeline_intent.pipeline_intent.id.as_str())
    );
    assert_eq!(
        flow_before_revision
            .release
            .as_ref()
            .map(|release| release.id.as_str()),
        Some(approved_release.release.id.as_str())
    );
    assert!(flow_before_revision
        .incidents
        .iter()
        .any(|incident| incident.id == release_incident.id));
    assert!(flow_before_revision
        .remediation_plans
        .iter()
        .any(|plan| plan.id == release_remediation_plan.id));
    assert!(flow_before_revision.approval_gates.iter().any(|gate| gate
        .remediation_plan_id
        .as_deref()
        == Some(&release_remediation_plan.id)
        && gate.gate_kind == "cluster_mutation"));
    assert!(flow_before_revision
        .audit_events
        .iter()
        .any(|event| event.kind == "remediation_plan.created"
            && event.resource_id == release_remediation_plan.id));
    assert_eq!(revised.change_set.status, "draft");
    assert_eq!(revised.change_set.revision, 2);
    assert!(revised.material_hash_changed);
    assert_ne!(revised.change_set.material_hash, original_hash);
    assert_eq!(
        revised
            .invalidated_pipeline_intent
            .as_ref()
            .map(|intent| intent.status.as_str()),
        Some("stale")
    );
    assert_eq!(
        revised
            .invalidated_deployment_intent
            .as_ref()
            .map(|intent| intent.status.as_str()),
        Some("stale")
    );
    assert_eq!(
        revised
            .invalidated_release
            .as_ref()
            .map(|release| release.status.as_str()),
        Some("stale")
    );
    assert_eq!(
        revised
            .invalidated_registry_evidence
            .as_ref()
            .map(|evidence| evidence.status.as_str()),
        Some("stale")
    );
    assert_eq!(staled_grant.status, "stale");
    assert_eq!(staled_grant.revoked_by.as_deref(), Some("lucas"));
    assert_eq!(
        staled_grant.revoke_reason.as_deref(),
        Some("source change payload changed")
    );
    assert!(
        future_run.execution_target_json["policy"]["permission_grants"]
            .as_array()
            .is_none_or(Vec::is_empty)
    );
    assert!(!blocked_after_revision.ready);
    assert!(blocked_after_revision
        .blockers
        .iter()
        .any(|finding| finding.code == "change_set_not_approved"));
    assert!(blocked_after_revision
        .blockers
        .iter()
        .any(|finding| finding.code == "missing_active_trusted_envelope"));
    assert!(blocked_after_revision
        .warnings
        .iter()
        .any(|finding| finding.code == "stale_trusted_envelope"));
    assert!(blocked_after_revision
        .warnings
        .iter()
        .any(|finding| finding.code == "stale_pipeline_intent"));
    assert!(!reproposed_pipeline_intent.created);
    assert_eq!(
        reproposed_pipeline_intent.pipeline_intent.id,
        proposed_pipeline_intent.pipeline_intent.id
    );
    assert_eq!(
        reproposed_pipeline_intent.pipeline_intent.status,
        "proposed"
    );
    assert_eq!(
        reproposed_pipeline_intent.pipeline_intent.intent_json["source"]["material_hash"],
        serde_json::json!(revised.change_set.material_hash)
    );
    assert!(waiting_on_reproposed_pipeline_intent
        .warnings
        .iter()
        .any(|finding| finding.code == "pipeline_intent_not_approved"));
    assert_eq!(
        approved_reproposed_pipeline_intent.pipeline_intent.status,
        "approved"
    );
    assert!(waiting_on_reproposed_deployment_intent
        .warnings
        .iter()
        .any(|finding| finding.code == "stale_deployment_intent"));
    assert!(!reproposed_deployment_intent.created);
    assert_eq!(
        reproposed_deployment_intent.deployment_intent.id,
        proposed_deployment_intent.deployment_intent.id
    );
    assert_eq!(
        reproposed_deployment_intent.deployment_intent.status,
        "proposed"
    );
    assert!(waiting_on_reproposed_deployment_approval
        .warnings
        .iter()
        .any(|finding| finding.code == "deployment_intent_not_approved"));
    assert_eq!(
        approved_reproposed_deployment_intent
            .deployment_intent
            .status,
        "approved"
    );
    assert!(waiting_on_reproposed_release
        .warnings
        .iter()
        .any(|finding| finding.code == "stale_release"));
    assert!(!reproposed_release.created);
    assert_eq!(reproposed_release.release.id, proposed_release.release.id);
    assert_eq!(reproposed_release.release.status, "proposed");
    assert_eq!(
        reproposed_release.release.version.as_deref(),
        Some("v0.1.1-smoke")
    );
    assert!(waiting_on_reproposed_release_approval
        .warnings
        .iter()
        .any(|finding| finding.code == "release_not_approved"));
    assert_eq!(approved_reproposed_release.release.status, "approved");
    assert!(waiting_on_reproposed_registry_evidence
        .warnings
        .iter()
        .any(|finding| finding.code == "stale_registry_evidence"));
    assert!(!reproposed_registry_evidence.created);
    assert_eq!(
        reproposed_registry_evidence.registry_evidence.id,
        proposed_registry_evidence.registry_evidence.id
    );
    assert_eq!(
        reproposed_registry_evidence
            .registry_evidence
            .image_digest
            .as_deref(),
        Some("sha256:feedface")
    );
    assert_eq!(
        verified_reproposed_registry_evidence
            .registry_evidence
            .status,
        "verified"
    );
    assert_eq!(revised.invalidated_gates.len(), 1);
    assert_eq!(revised.invalidated_gates[0].status, "stale");
    assert_eq!(
        revised.invalidated_gates[0].stale_reason.as_deref(),
        Some("source change payload changed")
    );
    let invalidated_change_set = revised_work_plan.invalidated_change_set.unwrap();
    assert_eq!(invalidated_change_set.id, change_set_id);
    assert_eq!(invalidated_change_set.status, "stale");
    assert!(change_set_audit_events
        .events
        .iter()
        .any(|event| event.kind == "change_set.revised"));
    assert!(change_set_audit_events
        .events
        .iter()
        .any(|event| event.kind == "change_set.trusted_envelope_created"));
    assert!(grant_audit_events
        .events
        .iter()
        .any(|event| event.kind == "permission_grant.stale"));
    assert!(pipeline_intent_audit_events
        .events
        .iter()
        .any(|event| event.kind == "pipeline_intent.proposed"));
    assert!(pipeline_intent_audit_events
        .events
        .iter()
        .any(|event| event.kind == "pipeline_intent.approved"));
    assert!(pipeline_intent_audit_events.events.iter().any(|event| {
        event.kind == "pipeline_intent.evidence_attached"
            && event.payload["extra"]["observation_id"] == "obs_pipeline_intent_evidence"
            && event.payload["extra"]["evidence_status"] == "satisfied"
    }));
    assert!(pipeline_intent_audit_events
        .events
        .iter()
        .any(|event| event.kind == "pipeline_intent.stale"));
    assert!(pipeline_intent_audit_events
        .events
        .iter()
        .any(|event| event.kind == "pipeline_intent.reproposed"));
    assert!(deployment_intent_audit_events
        .events
        .iter()
        .any(|event| event.kind == "deployment_intent.proposed"));
    assert!(deployment_intent_audit_events
        .events
        .iter()
        .any(|event| event.kind == "deployment_intent.approved"));
    assert!(deployment_intent_audit_events
        .events
        .iter()
        .any(|event| event.kind == "deployment_intent.stale"));
    assert!(deployment_intent_audit_events
        .events
        .iter()
        .any(|event| event.kind == "deployment_intent.reproposed"));
    assert!(release_audit_events
        .events
        .iter()
        .any(|event| event.kind == "release.proposed"));
    assert!(release_audit_events
        .events
        .iter()
        .any(|event| event.kind == "release.approved"));
    assert!(release_audit_events
        .events
        .iter()
        .any(|event| event.kind == "release.evidence_attached"));
    assert!(release_audit_events
        .events
        .iter()
        .any(|event| event.kind == "release.stale"));
    assert!(release_audit_events
        .events
        .iter()
        .any(|event| event.kind == "release.reproposed"));
    assert!(registry_evidence_audit_events
        .events
        .iter()
        .any(|event| event.kind == "registry_evidence.proposed"));
    assert!(registry_evidence_audit_events
        .events
        .iter()
        .any(|event| event.kind == "registry_evidence.verified"));
    assert!(registry_evidence_audit_events
        .events
        .iter()
        .any(|event| event.kind == "registry_evidence.stale"));
    assert!(registry_evidence_audit_events
        .events
        .iter()
        .any(|event| event.kind == "registry_evidence.reproposed"));
    assert!(gate_audit_events
        .events
        .iter()
        .any(|event| event.kind == "approval_gate.stale"));
}

#[tokio::test]
async fn denial_decides_pending_approval_and_blocks_run() {
    let state = test_state().await;

    let Json(created) = create_run(
        State(state.clone()),
        Json(CreateRunRequest {
            task: "write file".to_string(),
            cwd: Some(".".to_string()),
            max_turns: Some(12),
            policy_mode: None,
            scope: None,
        }),
    )
    .await
    .unwrap();

    state
        .store
        .create_approval(CreateApproval {
            id: "appr_test".to_string(),
            session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
            run_id: created.id.clone(),
            status: "pending".to_string(),
            kind: "file_write".to_string(),
            summary: "write README.md".to_string(),
            risk_level: "medium".to_string(),
            run_scope_json: Some(serde_json::json!({
                "namespace": "apps-dev",
                "repo": "git@example.test/team/app.git",
                "branch": "feature/pharness",
                "production_impacting": false
            })),
            action_json: Some(
                serde_json::to_value(AgentAction::WriteFile {
                    id: "act_write".into(),
                    reason: "test".to_string(),
                    path: "README.md".into(),
                    content: "hello".to_string(),
                })
                .unwrap(),
            ),
            preview_json: None,
            resume_messages_json: Some(serde_json::json!([])),
            turns_completed: 1,
        })
        .await
        .unwrap();
    state
        .store
        .mark_run_approval_required(
            &created.id,
            serde_json::json!({
                "status": "approval_required",
                "approval_id": "appr_test"
            }),
        )
        .await
        .unwrap();

    let Json(response) = decide_run_approval(
        State(state.clone()),
        Path(created.id.to_string()),
        Json(DecideApprovalRequest {
            decision: ApprovalDecision::Deny,
            decided_by: Some("test".to_string()),
            reason: Some("not now".to_string()),
        }),
    )
    .await
    .unwrap();

    assert_eq!(response.approval.status, "denied");
    assert_eq!(
        response
            .approval
            .scope
            .as_ref()
            .unwrap()
            .namespace
            .as_deref(),
        Some("apps-dev")
    );
    assert_eq!(response.run.status, "failed");
    let events = state.store.list_events(&created.id).await.unwrap();
    assert!(events.iter().any(|event| {
        event.kind == pharness_core::EventKind::ApprovalDecided
            && event.payload["run_scope"]["namespace"] == "apps-dev"
    }));
    let Json(audit_events) = list_audit_events(
        State(state),
        Query(ListAuditEventsQuery {
            resource_kind: Some("approval".to_string()),
            resource_id: Some("appr_test".to_string()),
            run_id: None,
            limit: Some(50),
            ..Default::default()
        }),
    )
    .await
    .unwrap();
    assert!(audit_events.events.iter().any(|event| {
        event.kind == "approval.denied"
            && event.actor.as_deref() == Some("test")
            && event.payload["run_scope"]["namespace"] == "apps-dev"
            && event.payload["action"] == "write_file"
    }));
}

#[test]
fn builds_a_constrained_tekton_pipeline_run_manifest() {
    let intent_json = json!({
        "source_provenance": {
            "merge_commit_sha": "0123456789abcdef0123456789abcdef01234567"
        },
        "execution": {
            "enabled": true,
            "namespace": "tekton-pipelines",
            "pipeline_ref": "clone-build-push",
            "params": { "repo-url": "https://example.test/team/app.git" },
            "workspaces": [{
                "name": "shared-data",
                "volume_claim_template": { "storage": "1Gi" }
            }]
        }
    });
    let execution = tekton_execution_spec(&intent_json).unwrap();
    let mut intent = StoredPipelineIntent {
        id: "pint_123".to_string(),
        change_set_id: "cset_456".to_string(),
        work_plan_id: "wplan_789".to_string(),
        remediation_plan_id: Some("rplan_1".to_string()),
        incident_id: Some("inc_1".to_string()),
        session_id: SessionId::new("ses_1"),
        run_id: None,
        status: "approved".to_string(),
        title: "build".to_string(),
        summary: "build".to_string(),
        risk_level: "high".to_string(),
        intent_kind: "tekton_build_test_package".to_string(),
        resource_namespace: None,
        resource_kind: None,
        resource_name: None,
        intent_json,
        created_at: "1".to_string(),
        updated_at: None,
        status_changed_at: None,
        status_changed_by: None,
        status_reason: None,
    };
    let manifest = build_pipeline_run_manifest(&intent, &execution).unwrap();

    assert_eq!(manifest["apiVersion"], "tekton.dev/v1");
    assert_eq!(manifest["kind"], "PipelineRun");
    assert_eq!(manifest["metadata"]["namespace"], "tekton-pipelines");
    assert_eq!(
        manifest["metadata"]["annotations"]["pharness.lucas.engineering/source-commit"],
        "0123456789abcdef0123456789abcdef01234567"
    );
    assert_eq!(manifest["spec"]["pipelineRef"]["name"], "clone-build-push");
    assert_eq!(
        manifest["spec"]["workspaces"][0]["volumeClaimTemplate"]["spec"]["accessModes"][0],
        "ReadWriteOnce"
    );
    assert!(manifest
        .pointer("/spec/taskRunTemplate/serviceAccountName")
        .is_none());

    intent.intent_json["execution_attempt"] = json!(2);
    let retry_manifest = build_pipeline_run_manifest(&intent, &execution).unwrap();
    assert_eq!(retry_manifest["metadata"]["name"], "pharness-pint-123-2");
}

#[tokio::test]
async fn failed_pipeline_intent_requires_review_and_preserves_evidence_for_one_retry() {
    let state = test_state().await;
    let source_merge_sha = "0123456789abcdef0123456789abcdef01234567";
    let Json(work_item) = create_work_item(
        State(state.clone()),
        None,
        Json(CreateWorkItemRequest {
            title: "Retry a reviewed pipeline failure".to_string(),
            intent: "Preserve the failed execution before a supervised retry.".to_string(),
            acceptance_criteria: vec!["Pipeline retry remains explicit".to_string()],
            source_repo: "team/finance-api".to_string(),
            source_ref: "main".to_string(),
            source_commit: Some(source_merge_sha.to_string()),
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
    let Json(planned) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item.id.clone()),
        Json(ReconcileWorkItemRequest {
            apply: true,
            actor: Some("operator".to_string()),
            reason: Some("declare the retry fixture WorkPlan".to_string()),
            max_turns: None,
        }),
    )
    .await
    .unwrap();
    let work_plan_id = planned.work_plan.unwrap().id;
    let _ = transition_work_plan(
        State(state.clone()),
        Path(work_plan_id.clone()),
        Json(TransitionWorkPlanRequest {
            target_status: "approved".to_string(),
            actor: Some("operator".to_string()),
            reason: Some("approve the retry fixture WorkPlan".to_string()),
        }),
    )
    .await
    .unwrap();
    let work_plan = state
        .store
        .get_work_plan(&work_plan_id)
        .await
        .unwrap()
        .unwrap();
    let change_set = state
        .store
        .create_change_set(CreateChangeSet {
            id: "cset_pipeline_retry".to_string(),
            work_item_id: Some(work_item.id.clone()),
            work_plan_id: work_plan.id.clone(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: work_plan.session_id.clone(),
            run_id: work_plan.run_id.clone(),
            status: "approved".to_string(),
            title: "Reviewed source change".to_string(),
            summary: "Source delivery already completed.".to_string(),
            risk_level: "high".to_string(),
            material_hash: "material_pipeline_retry".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("application".to_string()),
            resource_name: Some("finance-api".to_string()),
            change_set_json: json!({}),
        })
        .await
        .unwrap();
    let pipeline_intent = state
        .store
        .create_pipeline_intent(CreatePipelineIntent {
            id: "pint_pipeline_retry".to_string(),
            change_set_id: change_set.id,
            work_plan_id: work_plan.id,
            remediation_plan_id: None,
            incident_id: None,
            session_id: work_plan.session_id,
            run_id: work_plan.run_id,
            status: "failed".to_string(),
            title: "Build reviewed source".to_string(),
            summary: "The first supervised execution failed.".to_string(),
            risk_level: "high".to_string(),
            intent_kind: "tekton_build_test_package".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("application".to_string()),
            resource_name: Some("finance-api".to_string()),
            intent_json: json!({
                "source_provenance": {
                    "kind": "github_merged_pull_request",
                    "immutable": true,
                    "merge_commit_sha": source_merge_sha,
                },
                "execution": {
                    "enabled": true,
                    "namespace": "tekton-pipelines",
                    "pipeline_ref": "finance-build",
                    "params": { "revision": source_merge_sha },
                    "workspaces": [],
                    "production_impacting": false,
                },
                "execution_state": {
                    "execution_id": "pexec_failed_1",
                    "state": "pipeline_run_failed",
                    "pipeline_run_namespace": "tekton-pipelines",
                    "pipeline_run_name": "pharness-pint-pipeline-retry",
                    "permission_grant_id": "pgrant_failed_1",
                },
                "execution_evidence": {
                    "status": "failed",
                    "artifact_id": "art_pipeline_failed_1",
                    "observation_id": "obs_pipeline_failed_1",
                    "pipeline_run": {
                        "namespace": "tekton-pipelines",
                        "name": "pharness-pint-pipeline-retry",
                    },
                },
                "evidence": {
                    "status": "failed",
                    "artifact_id": "art_pipeline_analysis_failed_1",
                },
            }),
        })
        .await
        .unwrap();

    let Json(flow) = work_item_flow(State(state.clone()), Path(work_item.id.clone()))
        .await
        .unwrap();
    let retry = flow
        .action_rail
        .iter()
        .find(|action| action.id == "retry_pipeline_intent")
        .expect("failed PipelineIntent must expose one supervised retry review");
    assert_eq!(retry.status, "ready");
    assert!(retry.approval_required);
    assert!(retry
        .external_effect_summary
        .contains("does not start Tekton"));

    let Json(reproposed) = execute_work_item_action(
        State(state.clone()),
        None,
        Path((work_item.id.clone(), retry.id.clone())),
        Json(ExecuteWorkItemActionRequest {
            actor: Some("operator".to_string()),
            reason: "reviewed the exact failed PipelineRun evidence".to_string(),
            state_hash: retry.state_hash.clone(),
        }),
    )
    .await
    .unwrap();
    assert_eq!(reproposed["status"], "proposed");
    let stored = state
        .store
        .get_pipeline_intent(&pipeline_intent.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.intent_json["execution_attempt"], json!(2));
    assert_eq!(
        stored.intent_json["execution_history"][0]["execution_evidence"]["artifact_id"],
        "art_pipeline_failed_1"
    );
    assert!(stored.intent_json.get("execution_state").is_none());
    assert!(stored.intent_json.get("execution_evidence").is_none());
    let Json(after) = work_item_flow(State(state.clone()), Path(work_item.id.clone()))
        .await
        .unwrap();
    assert!(after
        .action_rail
        .iter()
        .any(|action| action.id == "approve_pipeline_intent" && action.status == "ready"));
    assert!(!after
        .action_rail
        .iter()
        .any(|action| action.id == "retry_pipeline_intent"));
    let audit_events = state
        .store
        .list_audit_events(Some("pipeline_intent"), Some(&pipeline_intent.id), None, 20)
        .await
        .unwrap();
    assert!(audit_events
        .iter()
        .any(|event| event.kind == "pipeline_intent.retry_proposed"));
}

#[test]
fn pipeline_contract_rejects_unknown_or_wrongly_shaped_inputs() {
    let execution = tekton_execution_spec(&json!({
        "execution": {
            "enabled": true,
            "namespace": "tekton-pipelines",
            "pipeline_ref": "clone-build-push",
            "params": { "branches": "main", "unknown": "value" },
            "workspaces": []
        }
    }))
    .unwrap();
    let contract = StoredPipelineContract {
        id: "pcontract_1".to_string(),
        status: "active".to_string(),
        namespace: "tekton-pipelines".to_string(),
        pipeline_ref: "clone-build-push".to_string(),
        version: "v1".to_string(),
        contract_json: json!({
            "params": [{ "name": "branches", "type": "array", "required": true }],
            "workspaces": []
        }),
        created_at: "1".to_string(),
        updated_at: "1".to_string(),
        status_changed_at: "1".to_string(),
        status_changed_by: None,
        status_reason: None,
    };

    let error = execution_matches_pipeline_contract(&execution, &contract, None).unwrap_err();
    assert!(error.message.contains("branches"));
}

#[test]
fn work_item_pipeline_contract_requires_the_observed_merge_commit() {
    let merge_commit = "0123456789abcdef0123456789abcdef01234567";
    let execution = tekton_execution_spec(&json!({
        "execution": {
            "enabled": true,
            "namespace": "tekton-pipelines",
            "pipeline_ref": "clone-build-push",
            "params": { "source-revision": merge_commit },
            "workspaces": []
        }
    }))
    .unwrap();
    let contract = StoredPipelineContract {
        id: "pcontract_source".to_string(),
        status: "active".to_string(),
        namespace: "tekton-pipelines".to_string(),
        pipeline_ref: "clone-build-push".to_string(),
        version: "v1".to_string(),
        contract_json: json!({
            "params": [{ "name": "source-revision", "type": "scalar", "required": true }],
            "source_revision_param": "source-revision"
        }),
        created_at: "1".to_string(),
        updated_at: "1".to_string(),
        status_changed_at: "1".to_string(),
        status_changed_by: None,
        status_reason: None,
    };

    execution_matches_pipeline_contract(&execution, &contract, Some(merge_commit)).unwrap();

    let error = execution_matches_pipeline_contract(
        &execution,
        &contract,
        Some("abcdef0123456789abcdef0123456789abcdef01"),
    )
    .unwrap_err();
    assert!(error
        .message
        .contains("must equal the observed merged commit"));

    let missing_binding = StoredPipelineContract {
        contract_json: json!({
            "params": [{ "name": "source-revision", "type": "scalar", "required": true }]
        }),
        ..contract
    };
    let error =
        execution_matches_pipeline_contract(&execution, &missing_binding, Some(merge_commit))
            .unwrap_err();
    assert!(error.message.contains("source_revision_param"));
}

#[tokio::test]
async fn deployment_contract_is_exact_audited_and_retirable() {
    let state = test_state().await;
    let Json(created) = create_deployment_contract(
        State(state.clone()),
        None,
        Json(CreateDeploymentContractRequest {
            target_environment: "homelab".to_string(),
            target_namespace: "pharness".to_string(),
            argo_application: "pharness".to_string(),
            version: Some("v1".to_string()),
            contract_json: json!({ "operation": "sync", "prune": false, "force": false }),
            actor: Some("lucas".to_string()),
            reason: Some("reviewed bounded Argo target".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(listed) = list_deployment_contracts(
        State(state.clone()),
        Query(ListDeploymentContractsQuery {
            target_environment: Some("homelab".to_string()),
            target_namespace: Some("pharness".to_string()),
            argo_application: Some("pharness".to_string()),
            status: Some("active".to_string()),
            limit: Some(10),
            offset: Some(0),
        }),
    )
    .await
    .unwrap();
    let Json(fetched) = get_deployment_contract(State(state.clone()), Path(created.id.clone()))
        .await
        .unwrap();
    let Json(retired) = transition_deployment_contract(
        State(state.clone()),
        None,
        Path(created.id.clone()),
        Json(TransitionDeploymentContractRequest {
            target_status: "retired".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("target withdrawn".to_string()),
        }),
    )
    .await
    .unwrap();

    assert_eq!(created.status, "active");
    assert_eq!(listed.count, 1);
    assert_eq!(fetched.id, created.id);
    assert_eq!(retired.status, "retired");
    let audits = state
        .store
        .list_audit_events(Some("deployment_contract"), Some(&created.id), None, 10)
        .await
        .unwrap();
    assert!(audits
        .iter()
        .any(|event| event.kind == "deployment_contract.created"));
    assert!(audits
        .iter()
        .any(|event| event.kind == "deployment_contract.retired"));
}

#[test]
fn execution_outcome_keeps_dispatch_identity_for_reconciliation() {
    let mut intent = json!({
        "execution_state": {
            "execution_id": "exec_1",
            "executor_job_name": "pharness-tekton-exec-1",
            "permission_grant_id": "pgrant_1",
            "state": "dispatched"
        }
    });

    merge_pipeline_execution_state(
        &mut intent,
        json!({
            "execution_id": "exec_1",
            "state": "pipeline_run_created",
            "pipeline_run_namespace": "tekton-pipelines",
            "pipeline_run_name": "build-1",
            "error": null
        }),
    );

    assert_eq!(
        intent.pointer("/execution_state/executor_job_name"),
        Some(&json!("pharness-tekton-exec-1"))
    );
    assert_eq!(
        intent.pointer("/execution_state/permission_grant_id"),
        Some(&json!("pgrant_1"))
    );
    assert_eq!(
        intent.pointer("/execution_state/state"),
        Some(&json!("pipeline_run_created"))
    );
}

#[tokio::test]
async fn terminal_execution_evidence_is_compact_and_idempotent() {
    let state = test_state().await;
    let session_id = SessionId::new("ses_execution_evidence");
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: "execution evidence".to_string(),
            cwd: ".".to_string(),
        })
        .await
        .unwrap();
    let run_id = RunId::new("run_execution_evidence");
    state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: "execution evidence".to_string(),
            cwd: ".".to_string(),
            max_turns: 1,
            initial_status: "completed".to_string(),
            execution_target_json: json!({}),
        })
        .await
        .unwrap();
    let intent = StoredPipelineIntent {
        id: "pint_execution_evidence".to_string(),
        change_set_id: "cset_execution_evidence".to_string(),
        work_plan_id: "wplan_execution_evidence".to_string(),
        remediation_plan_id: Some("rplan_execution_evidence".to_string()),
        incident_id: Some("inc_execution_evidence".to_string()),
        session_id,
        run_id: Some(run_id),
        status: "executing".to_string(),
        title: "execution evidence".to_string(),
        summary: "execution evidence".to_string(),
        risk_level: "high".to_string(),
        intent_kind: "tekton_build_test_package".to_string(),
        resource_namespace: None,
        resource_kind: None,
        resource_name: None,
        intent_json: json!({}),
        created_at: "1".to_string(),
        updated_at: None,
        status_changed_at: None,
        status_changed_by: None,
        status_reason: None,
    };
    let outcome = PipelineIntentExecutionOutcomeRequest {
        execution_id: "pexec_execution_evidence".to_string(),
        status: "completed".to_string(),
        pipeline_run_namespace: Some("tekton-pipelines".to_string()),
        pipeline_run_name: Some("pharness-smoke".to_string()),
        error: None,
        pipeline_run_analysis: Some(json!({
            "kind": "PipelineRunAnalysis",
            "pipeline_run": {
                "namespace": "tekton-pipelines",
                "name": "pharness-smoke"
            },
            "summary": {
                "status": "succeeded",
                "failed_task_run_count": 0,
                "running_task_run_count": 0
            }
        })),
        analysis_error: None,
    };

    let first = persist_pipeline_execution_evidence(
        &state.store,
        &intent,
        &outcome,
        "pipeline_run_succeeded",
    )
    .await
    .unwrap();
    let second = persist_pipeline_execution_evidence(
        &state.store,
        &intent,
        &outcome,
        "pipeline_run_succeeded",
    )
    .await
    .unwrap();

    assert_eq!(first, second);
    assert_eq!(first["status"], "succeeded");
    assert_eq!(first["pipeline_run"]["namespace"], "tekton-pipelines");
    let artifact_id = first["artifact_id"].as_str().unwrap();
    let observation_id = first["observation_id"].as_str().unwrap();
    assert_eq!(
        state
            .store
            .get_artifact(artifact_id)
            .await
            .unwrap()
            .unwrap()
            .kind,
        "tekton_pipeline_run_execution"
    );
    assert_eq!(
        state
            .store
            .get_observation(observation_id)
            .await
            .unwrap()
            .unwrap()
            .kind,
        "pipeline_run_execution"
    );

    let analysis = persist_pipeline_run_analysis(
        &state.store,
        &intent,
        &outcome,
        outcome.pipeline_run_analysis.as_ref().unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(analysis.kind, "pipeline_run_analysis");
    assert_eq!(analysis.resource_name.as_deref(), Some("pharness-smoke"));
    let mut intent_json = intent.intent_json.clone();
    set_pipeline_intent_evidence(&mut intent_json, &analysis);
    assert_eq!(
        intent_json.pointer("/evidence/status"),
        Some(&json!("satisfied"))
    );

    let digest = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let build_analysis = json!({
        "outputs": {
            "image_url": "registry.example.test/team/finance-api:build-42",
            "image_digest": digest,
            "commit": "0123456789abcdef0123456789abcdef01234567",
        }
    });
    let build_output = pipeline_build_output_from_analysis(&intent, &build_analysis)
        .expect("valid terminal output should produce digest-pinned build provenance");
    assert_eq!(build_output.status, "verified");
    assert_eq!(
        build_output.image_reference,
        format!("registry.example.test/team/finance-api:build-42@{digest}")
    );
    let persisted = persist_pipeline_build_output(&state.store, &intent, &outcome, &build_analysis)
        .await
        .unwrap()
        .expect("real coding run provenance should persist build output");
    assert_eq!(persisted.kind, "pipeline_build_output");
    assert_eq!(
        persisted
            .content_json
            .as_ref()
            .and_then(|content| content.pointer("/image/reference")),
        Some(&json!(format!(
            "registry.example.test/team/finance-api:build-42@{digest}"
        )))
    );
    let artifacts = state
        .store
        .list_artifacts(intent.run_id.as_ref().unwrap())
        .await
        .unwrap();
    let current = current_pipeline_build_output(&artifacts, &intent)
        .unwrap()
        .expect("verified build output should be available for GitOps planning");
    assert_eq!(current.artifact_id, persisted.id);
    assert_eq!(current.image_reference, build_output.image_reference);
    let mut intent_without_run = intent.clone();
    intent_without_run.id = "pint_execution_evidence_without_run".to_string();
    intent_without_run.run_id = None;
    let mut no_run_outcome = outcome.clone();
    no_run_outcome.execution_id = "pexec_execution_evidence_without_run".to_string();
    let persisted_without_run = persist_pipeline_build_output(
        &state.store,
        &intent_without_run,
        &no_run_outcome,
        &build_analysis,
    )
    .await
    .unwrap()
    .expect("PipelineIntent-owned build output should not require a coding Run");
    assert!(persisted_without_run.run_id.is_none());
    assert_eq!(
        persisted_without_run
            .content_json
            .as_ref()
            .and_then(|content| content.get("pipeline_intent_id")),
        Some(&json!("pint_execution_evidence_without_run"))
    );
    let mut linked_intent = intent.clone();
    linked_intent.intent_json = json!({
        "source_provenance": {
            "merge_commit_sha": "abcdef0123456789abcdef0123456789abcdef01"
        }
    });
    let untrusted = pipeline_build_output_from_analysis(&linked_intent, &build_analysis)
        .expect("the output itself is still safe to record");
    assert_eq!(untrusted.status, "untrusted");
    assert_eq!(untrusted.reason, Some("source_commit_mismatch"));
}

#[tokio::test]
async fn release_and_registry_evidence_inherit_verified_pipeline_build_output() {
    let state = test_state().await;
    seed_approved_release(&state).await;
    let pipeline_intent = state
        .store
        .get_pipeline_intent("pint_registry_inspection")
        .await
        .unwrap()
        .unwrap();
    let run_id = pipeline_intent.run_id.clone().unwrap();
    let digest = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let image_reference = format!("registry.example.test/team/finance-api:build-42@{digest}");
    state
        .store
        .create_artifact(CreateArtifact {
            id: "art_release_build_output".to_string(),
            session_id: pipeline_intent.session_id.clone(),
            run_id: Some(run_id),
            kind: "pipeline_build_output".to_string(),
            label: "verified terminal build output".to_string(),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "status": "verified",
                "pipeline_intent_id": pipeline_intent.id,
                "image": {
                    "url": "registry.example.test/team/finance-api:build-42",
                    "digest": digest,
                    "reference": image_reference,
                },
                "source": { "commit": "0123456789abcdef0123456789abcdef01234567" },
            })),
        })
        .await
        .unwrap();
    state
        .store
        .update_release_status(
            "rel_registry_inspection",
            "stale",
            Some("lucas".to_string()),
            Some("refresh with terminal build provenance".to_string()),
        )
        .await
        .unwrap();

    let Json(created) = create_release_from_deployment_intent(
        State(state.clone()),
        Json(CreateReleaseFromDeploymentIntentRequest {
            deployment_intent_id: "dint_registry_inspection".to_string(),
            title: None,
            summary: None,
            risk_level: None,
            release_kind: None,
            version: Some("v0.1.0-build-output".to_string()),
            commit_sha: None,
            image_digest: None,
            rollback_ref: None,
            release_json: None,
            actor: Some("lucas".to_string()),
            reason: Some("derive immutable build identity".to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(created.release.image_digest.as_deref(), Some(digest));
    assert_eq!(
        created
            .release
            .release_json
            .pointer("/build_output/artifact_id"),
        Some(&json!("art_release_build_output"))
    );
    assert_eq!(
        created
            .release
            .release_json
            .pointer("/build_output/image_reference"),
        Some(&json!(image_reference))
    );

    let error = create_release_from_deployment_intent(
        State(state.clone()),
        Json(CreateReleaseFromDeploymentIntentRequest {
            deployment_intent_id: "dint_registry_inspection".to_string(),
            title: None,
            summary: None,
            risk_level: None,
            release_kind: None,
            version: None,
            commit_sha: None,
            image_digest: Some("sha256:deadbeef".to_string()),
            rollback_ref: None,
            release_json: None,
            actor: None,
            reason: None,
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(error.status, StatusCode::CONFLICT);

    let Json(approved) = transition_release(
        State(state.clone()),
        Path(created.release.id.clone()),
        Json(TransitionReleaseRequest {
            target_status: "approved".to_string(),
            actor: Some("lucas".to_string()),
            reason: Some("reviewed build provenance".to_string()),
        }),
    )
    .await
    .unwrap();
    let Json(evidence) = create_registry_evidence_from_release(
        State(state),
        Json(CreateRegistryEvidenceFromReleaseRequest {
            release_id: approved.release.id,
            title: None,
            summary: None,
            risk_level: None,
            registry: None,
            repository: None,
            image_ref: None,
            image_digest: None,
            tag: None,
            source: None,
            verification_status: None,
            evidence_json: None,
            actor: Some("lucas".to_string()),
            reason: Some("carry build identity into registry review".to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        evidence.registry_evidence.image_ref.as_deref(),
        Some(image_reference.as_str())
    );
    assert_eq!(
        evidence.registry_evidence.image_digest.as_deref(),
        Some(digest)
    );
    assert_eq!(evidence.registry_evidence.source, "tekton_build_output");
    assert_eq!(
        evidence
            .registry_evidence
            .evidence_json
            .pointer("/build_output/artifact_id"),
        Some(&json!("art_release_build_output"))
    );
}

#[tokio::test]
async fn deployment_preflight_is_durable_and_never_dispatches_an_argo_sync() {
    let state = test_state().await;
    seed_approved_release(&state).await;

    let Json(preflight) = preflight_deployment_intent(
        State(state.clone()),
        None,
        Path("dint_registry_inspection".to_string()),
        Json(DeploymentIntentPreflightRequest {
            actor: Some("lucas".to_string()),
            reason: Some("prove review-only deployment boundary".to_string()),
        }),
    )
    .await
    .unwrap();

    assert_eq!(preflight.status, "blocked");
    assert!(!preflight.ready_for_argo_runner);
    assert!(!preflight.dispatch_ready);
    assert!(preflight.permission_grant.is_none());
    assert!(preflight.checks.iter().any(|check| {
        check["code"] == "supported_work_item_target" && check["passed"] == false
    }));
    let audit = state
        .store
        .list_audit_events(
            Some("deployment_intent"),
            Some("dint_registry_inspection"),
            None,
            10,
        )
        .await
        .unwrap();
    assert!(audit.iter().any(|event| {
        event.kind == "deployment_intent.preflighted"
            && event.payload_json["extra"]["dispatch_ready"] == false
    }));
}

#[tokio::test]
async fn deployment_preflight_requires_the_exact_dev_gate_contract_and_envelope() {
    let kubectl_stub = std::env::temp_dir().join(format!(
        "pharness-argo-executor-kubectl-{}",
        unique_suffix()
    ));
    fs::write(&kubectl_stub, "#!/bin/sh\ncat >/dev/null\nexit 0\n").unwrap();
    fs::set_permissions(&kubectl_stub, fs::Permissions::from_mode(0o755)).unwrap();
    let state = test_state_with_git_observer(
        kubectl_stub.to_string_lossy().to_string(),
        "https://github.com/example/finance-app.git".to_string(),
    )
    .await;
    let session_id = SessionId::new("ses_deployment_preflight");
    let run_id = RunId::new("run_deployment_preflight");
    let work_item_id = "witem_deployment_preflight";
    let work_plan_id = "wplan_deployment_preflight";
    let change_set_id = "cset_deployment_preflight";
    let pipeline_intent_id = "pint_deployment_preflight";
    let deployment_intent_id = "dint_deployment_preflight";
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: "Deployment preflight".to_string(),
            cwd: "/workspace".to_string(),
        })
        .await
        .unwrap();
    state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: "Deployment preflight".to_string(),
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
            title: "Deploy finance-app to dev".to_string(),
            intent: "Exercise the bounded dev deployment preflight".to_string(),
            acceptance_criteria: vec!["dry preflight is ready".to_string()],
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
            argo_application: Some("finance-app".to_string()),
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
            title: "Deploy finance-app".to_string(),
            summary: "Bounded dev delivery".to_string(),
            risk_level: "high".to_string(),
            requires_approval: true,
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Application".to_string()),
            resource_name: Some("finance-app".to_string()),
            work_plan_json: json!({}),
        })
        .await
        .unwrap();
    for gate in approval_gates_from_work_item(&work_item, &work_plan) {
        state.store.create_approval_gate(gate).await.unwrap();
    }
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
            title: "Finance ChangeSet".to_string(),
            summary: "Reviewable dev change".to_string(),
            risk_level: "high".to_string(),
            material_hash: "deployment_preflight_hash".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Application".to_string()),
            resource_name: Some("finance-app".to_string()),
            change_set_json: json!({}),
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
            title: "Finance build".to_string(),
            summary: "Verified build evidence".to_string(),
            risk_level: "high".to_string(),
            intent_kind: "tekton_build_test_package".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("finance-build".to_string()),
            intent_json: json!({ "evidence": { "status": "satisfied" } }),
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
            title: "Finance deploy".to_string(),
            summary: "Bounded Argo sync".to_string(),
            risk_level: "high".to_string(),
            intent_kind: "argo_sync_deploy".to_string(),
            target_environment: Some("dev".to_string()),
            target_namespace: Some("apps-dev".to_string()),
            argo_application: Some("finance-app".to_string()),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Application".to_string()),
            resource_name: Some("finance-app".to_string()),
            intent_json: json!({}),
        })
        .await
        .unwrap();
    let Json(contract) = create_deployment_contract(
        State(state.clone()),
        None,
        Json(CreateDeploymentContractRequest {
            target_environment: "dev".to_string(),
            target_namespace: "apps-dev".to_string(),
            argo_application: "finance-app".to_string(),
            version: Some("v1".to_string()),
            contract_json: json!({ "operation": "sync", "prune": false, "force": false }),
            actor: Some("lucas".to_string()),
            reason: Some("bounded dev target".to_string()),
        }),
    )
    .await
    .unwrap();

    let Json(blocked) = preflight_deployment_intent(
        State(state.clone()),
        None,
        Path(deployment_intent_id.to_string()),
        Json(DeploymentIntentPreflightRequest {
            actor: Some("lucas".to_string()),
            reason: Some("prove gate and envelope are required".to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(blocked.status, "blocked");
    assert!(blocked.deployment_contract.is_some());
    assert!(blocked.permission_grant.is_none());

    let Json(envelope) = create_deployment_intent_trusted_envelope(
        State(state.clone()),
        Path(deployment_intent_id.to_string()),
        Json(CreateDeploymentIntentTrustedEnvelopeRequest {
            subject: None,
            created_by: Some("lucas".to_string()),
            reason: "authorize the exact dev Argo target".to_string(),
            expires_at: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(envelope.grant.subject, "agent:argo-runner");
    assert_eq!(
        envelope.grant.scope["argo_applications"],
        json!(["finance-app"])
    );

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
            Some("reviewed bounded dev sync".to_string()),
        )
        .await
        .unwrap();

    let Json(ready) = preflight_deployment_intent(
        State(state.clone()),
        None,
        Path(deployment_intent_id.to_string()),
        Json(DeploymentIntentPreflightRequest {
            actor: Some("lucas".to_string()),
            reason: Some("prove Argo runner readiness".to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(ready.status, "ready_for_argo_runner");
    assert!(ready.ready_for_argo_runner);
    assert!(ready.dispatch_ready);
    assert_eq!(
        ready
            .deployment_contract
            .as_ref()
            .map(|item| item.id.as_str()),
        Some(contract.id.as_str())
    );
    assert_eq!(
        ready.permission_grant.as_ref().map(|item| item.id.as_str()),
        Some(envelope.grant.id.as_str())
    );

    let Json(execution) = execute_deployment_intent(
        State(state.clone()),
        None,
        Path(deployment_intent_id.to_string()),
        Json(ExecuteDeploymentIntentRequest {
            dry_run: false,
            actor: Some("lucas".to_string()),
            reason: Some("dispatch the preflighted disposable Argo sync".to_string()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(execution.status, "dispatched");
    assert!(execution.created);
    assert!(execution.executor_job_name.is_some());
    let execution_id = execution
        .execution_id
        .expect("Argo execution id is recorded");
    let request = ArgoSyncOutcomeRequest {
        execution_id,
        status: "completed".to_string(),
        sync_status: Some("Synced".to_string()),
        health_status: Some("Progressing".to_string()),
        operation_phase: Some("Succeeded".to_string()),
        revision: Some("deadbeef".to_string()),
        error_code: None,
    };
    let Json(first) = internal_argo_sync_outcome(
        State(state.clone()),
        Path(deployment_intent_id.to_string()),
        Json(request.clone()),
    )
    .await
    .unwrap();
    let Json(second) = internal_argo_sync_outcome(
        State(state.clone()),
        Path(deployment_intent_id.to_string()),
        Json(request),
    )
    .await
    .unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(
        first
            .content_json
            .as_ref()
            .map(|content| &content["status"]),
        Some(&json!("completed"))
    );
    fs::remove_file(kubectl_stub).unwrap();
}

#[tokio::test]
async fn terminal_pipeline_handoff_creates_one_proposed_deployment_intent() {
    let state = test_state().await;
    seed_approved_release(&state).await;
    let session_id = SessionId::new("ses_registry_inspection");
    let run_id = RunId::new("run_registry_inspection");
    state
        .store
        .create_remediation_plan(CreateRemediationPlan {
            id: "rplan_deployment_handoff".to_string(),
            incident_id: "inc_registry_inspection".to_string(),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Deployment handoff remediation".to_string(),
            summary: "handoff fixture".to_string(),
            risk_level: "medium".to_string(),
            requires_approval: true,
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Deployment".to_string()),
            resource_name: Some("checkout-api".to_string()),
            plan_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_work_plan(CreateWorkPlan {
            id: "wplan_deployment_handoff".to_string(),
            work_item_id: None,
            remediation_plan_id: Some("rplan_deployment_handoff".to_string()),
            incident_id: Some("inc_registry_inspection".to_string()),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Deployment handoff work".to_string(),
            summary: "handoff fixture".to_string(),
            risk_level: "medium".to_string(),
            requires_approval: true,
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Deployment".to_string()),
            resource_name: Some("checkout-api".to_string()),
            work_plan_json: json!({}),
        })
        .await
        .unwrap();
    state
        .store
        .create_change_set(CreateChangeSet {
            id: "cset_deployment_handoff".to_string(),
            work_item_id: None,
            work_plan_id: "wplan_deployment_handoff".to_string(),
            remediation_plan_id: Some("rplan_deployment_handoff".to_string()),
            incident_id: Some("inc_registry_inspection".to_string()),
            session_id: session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "approved".to_string(),
            title: "Declared deployment handoff".to_string(),
            summary: "handoff fixture".to_string(),
            risk_level: "medium".to_string(),
            material_hash: "hash_deployment_handoff".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Deployment".to_string()),
            resource_name: Some("checkout-api".to_string()),
            change_set_json: json!({}),
        })
        .await
        .unwrap();
    let pipeline_intent = state
        .store
        .create_pipeline_intent(CreatePipelineIntent {
            id: "pint_deployment_handoff".to_string(),
            change_set_id: "cset_deployment_handoff".to_string(),
            work_plan_id: "wplan_deployment_handoff".to_string(),
            remediation_plan_id: Some("rplan_deployment_handoff".to_string()),
            incident_id: Some("inc_registry_inspection".to_string()),
            session_id,
            run_id: Some(run_id),
            status: "approved".to_string(),
            title: "Build checkout-api".to_string(),
            summary: "terminal build evidence attached".to_string(),
            risk_level: "medium".to_string(),
            intent_kind: "tekton_build_test_package".to_string(),
            resource_namespace: Some("apps-dev".to_string()),
            resource_kind: Some("Deployment".to_string()),
            resource_name: Some("checkout-api".to_string()),
            intent_json: json!({
                "evidence": { "status": "satisfied" },
                "deployment_handoff": {
                    "target_environment": "dev",
                    "target_namespace": "apps-dev",
                    "argo_application": "checkout-api"
                }
            }),
        })
        .await
        .unwrap();

    let created = create_declared_deployment_handoff(&state, &pipeline_intent)
        .await
        .unwrap()
        .expect("declared handoff should create a deployment intent");
    let duplicate = create_declared_deployment_handoff(&state, &pipeline_intent)
        .await
        .unwrap();

    assert_eq!(created.status, "proposed");
    assert_eq!(created.target_environment.as_deref(), Some("dev"));
    assert_eq!(created.target_namespace.as_deref(), Some("apps-dev"));
    assert_eq!(created.argo_application.as_deref(), Some("checkout-api"));
    assert!(duplicate.is_none());
    let audit_events = state
        .store
        .list_audit_events(Some("deployment_intent"), Some(&created.id), None, 10)
        .await
        .unwrap();
    assert!(audit_events
        .iter()
        .any(|event| event.kind == "deployment_intent.auto_proposed"));
}

#[test]
fn pipeline_deployment_handoff_requires_exact_target_identifiers() {
    let valid = PipelineDeploymentHandoffSpec {
        target_environment: "dev".to_string(),
        target_namespace: "apps-dev".to_string(),
        argo_application: "checkout-api".to_string(),
        title: None,
        summary: None,
        risk_level: None,
    };
    assert!(validate_pipeline_deployment_handoff(&valid).is_ok());

    let invalid = PipelineDeploymentHandoffSpec {
        target_environment: "dev".to_string(),
        target_namespace: "apps-dev".to_string(),
        argo_application: "checkout api".to_string(),
        title: None,
        summary: None,
        risk_level: None,
    };
    assert!(validate_pipeline_deployment_handoff(&invalid).is_err());
}

#[test]
fn terminal_analysis_must_match_the_executor_pipeline_run() {
    let outcome = PipelineIntentExecutionOutcomeRequest {
        execution_id: "pexec_analysis_mismatch".to_string(),
        status: "completed".to_string(),
        pipeline_run_namespace: Some("tekton-pipelines".to_string()),
        pipeline_run_name: Some("expected-run".to_string()),
        error: None,
        pipeline_run_analysis: None,
        analysis_error: None,
    };
    let error = validate_terminal_pipeline_run_analysis(
        &outcome,
        &json!({
            "kind": "PipelineRunAnalysis",
            "pipeline_run": {
                "namespace": "tekton-pipelines",
                "name": "other-run"
            },
            "summary": { "status": "succeeded" }
        }),
    )
    .unwrap_err();

    assert!(error.message.contains("PipelineRun name"));
}

#[test]
fn cancelled_pipeline_analysis_is_a_terminal_failed_execution() {
    let outcome = PipelineIntentExecutionOutcomeRequest {
        execution_id: "pexec_cancelled".to_string(),
        status: "failed".to_string(),
        pipeline_run_namespace: Some("ci".to_string()),
        pipeline_run_name: Some("finance-build".to_string()),
        error: Some("PipelineRun reached terminal cancelled status".to_string()),
        pipeline_run_analysis: None,
        analysis_error: None,
    };
    assert!(validate_terminal_pipeline_run_analysis(
        &outcome,
        &json!({
            "kind": "PipelineRunAnalysis",
            "pipeline_run": { "namespace": "ci", "name": "finance-build" },
            "summary": { "status": "cancelled" }
        }),
    )
    .is_ok());
}

#[test]
fn deployment_approval_requires_matching_satisfied_pipeline_evidence() {
    let mut intent = StoredPipelineIntent {
        id: "pint_deployment_evidence".to_string(),
        change_set_id: "cset_deployment_evidence".to_string(),
        work_plan_id: "wplan_deployment_evidence".to_string(),
        remediation_plan_id: Some("rplan_deployment_evidence".to_string()),
        incident_id: Some("inc_deployment_evidence".to_string()),
        session_id: SessionId::new("ses_deployment_evidence"),
        run_id: None,
        status: "approved".to_string(),
        title: "deployment evidence".to_string(),
        summary: "deployment evidence".to_string(),
        risk_level: "high".to_string(),
        intent_kind: "tekton_build_test_package".to_string(),
        resource_namespace: None,
        resource_kind: None,
        resource_name: None,
        intent_json: json!({
            "execution_evidence": {
                "status": "succeeded",
                "pipeline_run": { "namespace": "tekton-pipelines", "name": "build-1" }
            }
        }),
        created_at: "1".to_string(),
        updated_at: None,
        status_changed_at: None,
        status_changed_by: None,
        status_reason: None,
    };

    assert!(ensure_pipeline_evidence_ready_for_deployment(&intent).is_err());
    intent.intent_json["evidence"] = json!({
        "status": "satisfied",
        "resource": { "namespace": "tekton-pipelines", "name": "other-run" }
    });
    assert!(ensure_pipeline_evidence_ready_for_deployment(&intent).is_err());
    intent.intent_json["evidence"]["resource"]["name"] = json!("build-1");
    assert!(ensure_pipeline_evidence_ready_for_deployment(&intent).is_ok());
}

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
async fn worker_can_only_pin_the_exact_issued_remote_workspace() {
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
        build: super::BuildMetadata::from_env(),
        protected_target: super::ProtectedTargetConfiguration::from_env(),
        environment_profiles: Arc::new(Vec::new()),
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
            status: "provisioning".to_string(),
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
            expires_at: Some((super::current_millis() + 60_000).to_string()),
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
        super::gitops_base_revision_reconcile_state(&state.store, &reproposed_change_set)
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

fn reconcile_artifact(kind: &str, content_json: serde_json::Value) -> ArtifactResponse {
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

fn reconcile_git_delivery_flow() -> GitDeliveryFlowResponse {
    GitDeliveryFlowResponse {
        plan: reconcile_artifact("git_delivery_plan", json!({})),
        latest_preflight: None,
        latest_execution: None,
        latest_result: None,
        latest_observation: None,
        latest_merge: None,
    }
}

fn reconcile_gitops_change_set(status: &str) -> StoredGitOpsChangeSet {
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

fn reconcile_gitops_delivery_flow() -> GitOpsDeliveryFlowResponse {
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

fn reconcile_pipeline_intent(
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

fn reconcile_deployment_intent() -> StoredDeploymentIntent {
    StoredDeploymentIntent {
        id: "dint_reconcile".to_string(),
        pipeline_intent_id: "pint_reconcile".to_string(),
        change_set_id: "cset_reconcile".to_string(),
        work_plan_id: "wplan_reconcile".to_string(),
        remediation_plan_id: None,
        incident_id: None,
        session_id: SessionId::new("ses_reconcile"),
        run_id: None,
        status: "proposed".to_string(),
        title: "Reconcile deployment intent".to_string(),
        summary: "Declare exact deployment target".to_string(),
        risk_level: "high".to_string(),
        intent_kind: "argo_sync_deploy".to_string(),
        target_environment: Some("dev".to_string()),
        target_namespace: Some("apps-dev".to_string()),
        argo_application: Some("finance-api".to_string()),
        resource_namespace: Some("apps-dev".to_string()),
        resource_kind: Some("Application".to_string()),
        resource_name: Some("finance-api".to_string()),
        intent_json: json!({}),
        created_at: "1".to_string(),
        updated_at: None,
        status_changed_at: None,
        status_changed_by: None,
        status_reason: None,
    }
}

fn reconcile_deployment_preflight(ready: bool) -> DeploymentIntentExecutionPreflight {
    DeploymentIntentExecutionPreflight {
        ready,
        intent: reconcile_deployment_intent(),
        contract: None,
        grant: None,
        gitops_merge: None,
        checks: Vec::new(),
    }
}

fn reconcile_deployment_delivery() -> DeploymentIntentDeliveryFlowResponse {
    DeploymentIntentDeliveryFlowResponse {
        latest_execution: None,
        latest_result: None,
        release: None,
    }
}

fn reconcile_release(status: &str) -> ReleaseResponse {
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
    assert!(!super::deployment_intent_requires_execution_preflight(
        Some("https://github.com/team/gitops.git"),
        Some("main"),
        "proposed",
        false,
    )
    .unwrap());
    assert!(!super::deployment_intent_requires_execution_preflight(
        Some("https://github.com/team/gitops.git"),
        Some("main"),
        "approved",
        false,
    )
    .unwrap());
    assert!(super::deployment_intent_requires_execution_preflight(
        Some("https://github.com/team/gitops.git"),
        Some("main"),
        "approved",
        true,
    )
    .unwrap());
    assert!(
        super::deployment_intent_requires_execution_preflight(None, None, "approved", false)
            .unwrap()
    );
    assert!(super::deployment_intent_requires_execution_preflight(
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
        super::action_effect(WorkItemReconcileAction::AwaitingGitOpsPullRequestMerge),
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
        source_repo: super::PROTECTED_SOURCE_REPO.to_string(),
        source_ref: "main".to_string(),
        source_commit: Some("a".repeat(40)),
        pipeline_contract_id: Some("pcontract_yfinance".to_string()),
        deployment_contract_id: Some("dcontract_yfinance".to_string()),
        gitops_repo: Some(super::PROTECTED_GITOPS_REPO.to_string()),
        gitops_ref: Some("main".to_string()),
        gitops_kustomization_path: Some(super::PROTECTED_KUSTOMIZATION_PATH.to_string()),
        gitops_image_name: Some(super::PROTECTED_IMAGE_NAME.to_string()),
        target_environment: super::PROTECTED_ENVIRONMENT.to_string(),
        target_namespace: Some(super::PROTECTED_NAMESPACE.to_string()),
        argo_application: Some(super::PROTECTED_ARGO_APPLICATION.to_string()),
        workload_kind: Some(super::PROTECTED_WORKLOAD_KIND.to_string()),
        workload_name: Some(super::PROTECTED_WORKLOAD_NAME.to_string()),
        rollback_owner: Some(super::PROTECTED_ROLLBACK_OWNER.to_string()),
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
    assert!(super::request_matches_protected_target(&request));

    let mut wrong_namespace = request.clone();
    wrong_namespace.target_namespace = Some("apps-staging".to_string());
    assert!(!super::request_matches_protected_target(&wrong_namespace));

    let mut wrong_image = request.clone();
    wrong_image.gitops_image_name = Some("registry.example.test/other".to_string());
    assert!(!super::request_matches_protected_target(&wrong_image));

    let mut wrong_repository = request;
    wrong_repository.gitops_repo = Some("https://github.com/lward27/other.git".to_string());
    assert!(!super::request_matches_protected_target(&wrong_repository));
}

#[test]
fn immutable_production_identifiers_reject_mutable_or_malformed_values() {
    assert!(super::immutable_git_object_id(&"a".repeat(40)));
    assert!(super::immutable_git_object_id(&"b".repeat(64)));
    assert!(!super::immutable_git_object_id("main"));
    assert!(!super::immutable_git_object_id(&"A".repeat(40)));
    assert!(super::immutable_image_digest(&format!(
        "sha256:{}",
        "c".repeat(64)
    )));
    assert!(!super::immutable_image_digest("latest"));
    assert!(!super::immutable_image_digest("sha256:abc"));
    assert!(!super::immutable_image_digest(&format!(
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
    let now = super::current_millis();
    assert!(super::bounded_production_grant_expiry(&item, Some((now - 1).to_string())).is_err());
    assert!(super::bounded_production_grant_expiry(
        &item,
        Some((now + 30 * 60 * 1_000 + 1).to_string())
    )
    .is_err());
    assert!(super::bounded_production_grant_expiry(
        &item,
        Some((now + 5 * 60 * 1_000).to_string())
    )
    .is_ok());
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
            source_repo: super::PROTECTED_SOURCE_REPO.to_string(),
            source_ref: "main".to_string(),
            source_commit: Some("a".repeat(40)),
            pipeline_contract_id: Some("pcontract_yfinance".to_string()),
            deployment_contract_id: Some("dcontract_yfinance".to_string()),
            gitops_repo: Some(super::PROTECTED_GITOPS_REPO.to_string()),
            gitops_ref: Some("main".to_string()),
            gitops_kustomization_path: Some(super::PROTECTED_KUSTOMIZATION_PATH.to_string()),
            gitops_image_name: Some(super::PROTECTED_IMAGE_NAME.to_string()),
            target_environment: super::PROTECTED_ENVIRONMENT.to_string(),
            target_namespace: Some(super::PROTECTED_NAMESPACE.to_string()),
            argo_application: Some(super::PROTECTED_ARGO_APPLICATION.to_string()),
            workload_kind: Some(super::PROTECTED_WORKLOAD_KIND.to_string()),
            workload_name: Some(super::PROTECTED_WORKLOAD_NAME.to_string()),
            rollback_owner: Some(super::PROTECTED_ROLLBACK_OWNER.to_string()),
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

    let Json(listed) = super::list_approval_gates(
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
    let batch = super::batch_decide_approval_gates(
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
    assert!(super::approval_gate_uses_dedicated_lifecycle_action(
        "production_rollback"
    ));
    assert!(!super::approval_gate_uses_dedicated_lifecycle_action(
        "cluster_mutation"
    ));
}

#[tokio::test]
async fn rollback_writer_and_observer_stay_bound_to_the_captured_digest_and_manual_merge() {
    let state = test_state_with_git_observer(
        "/bin/true".to_string(),
        super::PROTECTED_GITOPS_REPO.to_string(),
    )
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
            resource_namespace: Some(super::PROTECTED_NAMESPACE.to_string()),
            resource_kind: Some("Application".to_string()),
            resource_name: Some(super::PROTECTED_ARGO_APPLICATION.to_string()),
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
            resource_namespace: Some(super::PROTECTED_NAMESPACE.to_string()),
            resource_kind: Some("Application".to_string()),
            resource_name: Some(super::PROTECTED_ARGO_APPLICATION.to_string()),
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
            target_environment: Some(super::PROTECTED_ENVIRONMENT.to_string()),
            target_namespace: Some(super::PROTECTED_NAMESPACE.to_string()),
            argo_application: Some(super::PROTECTED_ARGO_APPLICATION.to_string()),
            resource_namespace: Some(super::PROTECTED_NAMESPACE.to_string()),
            resource_kind: Some(super::PROTECTED_WORKLOAD_KIND.to_string()),
            resource_name: Some(super::PROTECTED_WORKLOAD_NAME.to_string()),
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
            gitops_repo: super::PROTECTED_GITOPS_REPO.to_string(),
            gitops_ref: "main".to_string(),
            head_branch: "pharness/yfinance/gitops".to_string(),
            kustomization_path: super::PROTECTED_KUSTOMIZATION_PATH.to_string(),
            image_name: super::PROTECTED_IMAGE_NAME.to_string(),
            image_ref: format!("{}@sha256:{}", super::PROTECTED_IMAGE_NAME, "f".repeat(64)),
            gitops_change_set_json: json!({}),
        })
        .await
        .unwrap();
    state.store.create_artifact(CreateArtifact { id: "art_rollback_gitops_base".to_string(), session_id: session_id.clone(), run_id: Some(run_id.clone()), kind: "gitops_base_revision".to_string(), label: "Protected GitOps base".to_string(), mime_type: Some("application/json".to_string()), path: None, content_text: None, content_json: Some(json!({ "status": "resolved", "gitops_change_set_id": gitops_change_set.id, "material_hash": gitops_change_set.material_hash, "repository": super::PROTECTED_GITOPS_REPO, "base_ref": "main", "base_commit": "d1b2c3d4e5f60718293a4b5c6d7e8f9012345678" })) }).await.unwrap();
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
            resource_namespace: Some(super::PROTECTED_NAMESPACE.to_string()),
            resource_kind: Some(super::PROTECTED_WORKLOAD_KIND.to_string()),
            resource_name: Some(super::PROTECTED_WORKLOAD_NAME.to_string()),
            gate_json: json!({
                "rollback_intent_id": rollback_id,
                "baseline_digest": baseline_digest,
                "argo_application": super::PROTECTED_ARGO_APPLICATION,
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
        super::latest_rollback_intent(&state, &completed_attempt_item, Some(rollback_id),)
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
        super::approval_gate_lifecycle_readiness(&state, &rollback_gate)
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
    let prepared =
        super::latest_rollback_intent(&state, &completed_attempt_item, Some(rollback_id))
            .await
            .unwrap()
            .unwrap();
    let Json(writer_approved) = super::execute_work_item_action(
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
    let wrong_action = super::execute_work_item_action(
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
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(wrong_action.status, StatusCode::CONFLICT);
    let writer_execution = "rbexec_contract";
    let head_branch = "pharness/rollback-contract";
    state.store.create_artifact(CreateArtifact { id: "art_rollback_delivery_execution_contract".to_string(), session_id: session_id.clone(), run_id: Some(run_id.clone()), kind: "rollback_delivery_execution".to_string(), label: "Rollback delivery".to_string(), mime_type: Some("application/json".to_string()), path: None, content_text: None, content_json: Some(json!({ "rollback_intent_id": rollback_id, "execution_id": writer_execution, "status": "dispatched", "context": { "execution_id": writer_execution, "repository": super::PROTECTED_GITOPS_REPO, "base_ref": "main", "base_commit": deployment_gitops_merge, "head_branch": head_branch, "kustomization_path": super::PROTECTED_KUSTOMIZATION_PATH, "image_name": super::PROTECTED_IMAGE_NAME, "image_ref": format!("{}@{}", super::PROTECTED_IMAGE_NAME, baseline_digest), "commit_subject": "rollback yfinance", "commit_body": "restore captured digest", "pull_request_title": "rollback yfinance", "pull_request_body": "manual merge required", "github_api_url": "https://api.github.com", "author_name": "Pharness", "author_email": "pharness@example.test" } })) }).await.unwrap();
    let Json(context) =
        super::internal_rollback_delivery_context(&state, rollback_id, writer_execution)
            .await
            .unwrap();
    assert_eq!(
        context.image_ref,
        format!("{}@{}", super::PROTECTED_IMAGE_NAME, baseline_digest)
    );
    assert_eq!(context.base_commit, deployment_gitops_merge);
    let Json(result) = super::internal_rollback_delivery_outcome(
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
    state.store.create_artifact(CreateArtifact { id: "art_rollback_observation_execution_contract".to_string(), session_id: session_id.clone(), run_id: Some(run_id.clone()), kind: "rollback_delivery_observation_execution".to_string(), label: "Rollback observation".to_string(), mime_type: Some("application/json".to_string()), path: None, content_text: None, content_json: Some(json!({ "rollback_intent_id": rollback_id, "execution_id": observer_execution, "status": "dispatched", "source": { "repository": super::PROTECTED_GITOPS_REPO, "head_branch": head_branch, "source_commit_sha": base_commit, "pull_request_url": "https://github.com/lward27/lucas_engineering/pull/42", "pull_request_number": 42 } })) }).await.unwrap();
    let merge_commit = "b1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
    let _ = super::internal_rollback_delivery_observation_outcome(
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
    let latest = super::latest_rollback_intent(&state, &item, Some(rollback_id))
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

    let Json(approved) = super::approve_rollback_intent(
        State(state.clone()),
        None,
        Path(rollback_id.to_string()),
        Json(super::RollbackIntentRequest {
            actor: Some("lucas".to_string()),
            reason: "approve exact rollback Argo sync".to_string(),
            expires_at: Some((super::current_millis() + 60_000).to_string()),
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
        super::preflight_rollback_intent(State(state.clone()), Path(rollback_id.to_string()))
            .await
            .unwrap();
    assert_eq!(preflight["status"], "ready_for_argo");
    assert_eq!(preflight["exact_binding"], true);
    assert_eq!(preflight["argo_grant_fresh"], true);

    let argo_execution_id = "rbaexec_contract";
    state.store.create_artifact(CreateArtifact { id: "art_rollback_argo_execution_contract".to_string(), session_id: session_id.clone(), run_id: Some(run_id.clone()), kind: "rollback_argo_sync_execution".to_string(), label: "Rollback Argo execution".to_string(), mime_type: Some("application/json".to_string()), path: None, content_text: None, content_json: Some(json!({ "rollback_intent_id": rollback_id, "execution_id": argo_execution_id, "status": "dispatched", "permission_grant_id": approved.pointer("/content/argo_permission_grant_id"), "deployment_contract_id": "dcontract_yfinance", "gitops_merge_sha": merge_commit, "baseline_digest": baseline_digest, "target": super::protected_target_json() })) }).await.unwrap();
    let wrong_revision = super::internal_rollback_argo_sync_outcome(
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
    let Json(first) = super::internal_rollback_argo_sync_outcome(&state, rollback_id, outcome)
        .await
        .unwrap();
    let Json(second) = super::internal_rollback_argo_sync_outcome(
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
