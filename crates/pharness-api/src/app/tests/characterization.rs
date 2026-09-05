use super::{
    capability_preflight_is_statically_unavailable, capability_verification_summary,
    coding_run_scope_matches_source, create_deployment_contract, create_observation,
    environment_profile_readiness_blocker, fs, gitops_artifact_change_set_revision,
    gitops_observation_closed_unmerged, gitops_observation_refreshable, json, list_observations,
    prometheus_inventory_observability_status, release_prometheus_inventory_collected,
    release_prometheus_inventory_summary, release_workload_verification_action, router,
    source_capability_statuses_for_repository, system_readiness, unique_suffix, verify_release,
    AgentAction, AgentEvent, AppState, Arc, BuildMetadata, CapabilityStatusResponse,
    CreateArtifact, CreateCapabilityVerification, CreateChangeSet, CreateDeploymentContractRequest,
    CreateDeploymentIntent, CreateIncident, CreateObservation, CreateObservationRequest,
    CreatePipelineIntent, CreateRelease, CreateRemediationPlan, CreateRun, CreateSession,
    CreateWorkItem, CreateWorkPlan, EventId, EventKind, Json, KubernetesJobDispatcher,
    ListObservationsQuery, Path, PathBuf, PermissionsExt, ProtectedTargetConfiguration, Query,
    ReadOnlyClusterTools, RunDispatcher, RunId, RunScope, SafetyPolicy, SessionId, SqliteStore,
    State, StatusCode, StoredDeploymentContract, Value, VerifyReleaseRequest,
    WorkerKubernetesConfig, WorkspaceProvisioner,
};
use crate::app::RepoModeConfiguration;

use super::support::reconcile_deployment_intent;

#[test]
fn v3_characterization_fixture_matches_frozen_constants() {
    use super::support as baseline;

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
    use super::support::{route_inventory, routes_mounted_in_source, RouteAuthClass};
    use tower::ServiceExt;

    let mut inventory = route_inventory();
    inventory.sort();
    assert_eq!(
        inventory.len(),
        230,
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

#[tokio::test]
async fn organization_overview_is_a_read_only_projection() {
    use tower::ServiceExt;

    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let organization_id = RepoModeConfiguration::from_env().organization.id;
    assert!(store
        .get_organization(&organization_id)
        .await
        .unwrap()
        .is_none());
    let app = router(
        store.clone(),
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
                .uri("/api/organization/overview")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(store
        .get_organization(&organization_id)
        .await
        .unwrap()
        .is_none());
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
    assert!(gitops_observation_refreshable(Some(&json!({
        "status": "observed",
        "pull_request_state": "open",
        "merged": false,
    }))));
    assert!(gitops_observation_refreshable(Some(&json!({
        "status": "failed",
    }))));
    assert!(!gitops_observation_refreshable(Some(&json!({
        "status": "observed",
        "pull_request_state": "closed",
        "merged": false,
    }))));
    assert!(!gitops_observation_refreshable(Some(&json!({
        "status": "observed",
        "pull_request_state": "closed",
        "merged": true,
    }))));
    assert!(!gitops_observation_refreshable(None));
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

    assert!(release_prometheus_inventory_collected(&inventory));
    assert_eq!(
        prometheus_inventory_observability_status(&inventory),
        "attention_required"
    );
    assert!(release_prometheus_inventory_summary(&inventory).contains("3 unhealthy target(s)"));

    let missing_rules = json!({
        "inventory": {
            "targets": { "status": "success" },
            "alerts": { "status": "success" }
        }
    });
    assert!(!release_prometheus_inventory_collected(&missing_rules));
}

#[test]
fn failed_capability_verification_can_retry_but_static_unavailability_cannot() {
    let static_unavailable = CapabilityStatusResponse {
        capability: "gitops_writer".to_string(),
        status: "unavailable".to_string(),
        summary: "GitOps writer is not configured".to_string(),
        verified_at: None,
        expires_at: None,
    };
    assert!(capability_preflight_is_statically_unavailable(
        &static_unavailable
    ));

    let failed_verification = CapabilityStatusResponse {
        verified_at: Some("1787134555765".to_string()),
        expires_at: Some("1787135455765".to_string()),
        summary: "Isolated identity did not verify repository_push".to_string(),
        ..static_unavailable
    };
    assert!(!capability_preflight_is_statically_unavailable(
        &failed_verification
    ));
}

pub(super) async fn test_state() -> AppState {
    AppState {
        store: Arc::new(SqliteStore::connect_in_memory().await.unwrap()),
        worker: RunDispatcher::Disabled,
        cluster_tools: ReadOnlyClusterTools::default(),
        policy: SafetyPolicy::default(),
        worker_token: None,
        operator_tokens: Arc::new(Vec::new()),
        workspace: WorkspaceProvisioner::new(std::env::temp_dir(), Vec::new()),
        build: BuildMetadata::from_env(),
        protected_target: ProtectedTargetConfiguration::from_env(),
        environment_profiles: Arc::new(Vec::new()),
        repo_mode: RepoModeConfiguration::test_enabled(),
        inference: Arc::new(pharness_config::InferenceGatewayConfig::legacy_default()),
        agent_execution: Arc::new(pharness_config::AgentExecutionBackendConfig::disabled_default()),
        hosted_workflow: Arc::new(pharness_core::hosted_sdlc::HostedWorkflowConfig::default()),
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
        runtime_kind: "python".to_string(),
        accepted_dependency_lock_kinds: vec!["pip_requirements".to_string()],
        lifecycle_scripts: "denied".to_string(),
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

#[tokio::test]
async fn readiness_preserves_matching_and_mismatched_revision_semantics() {
    let digest = format!("sha256:{}", "a".repeat(64));
    let mut matching = test_state().await;
    matching.build = BuildMetadata {
        api_revision: "b".repeat(40),
        ui_revision: "b".repeat(40),
        runtime_image_digest: digest.clone(),
        ui_image_digest: digest.clone(),
    };

    let Json(matching_response) = system_readiness(State(matching)).await.unwrap();
    assert!(matching_response.platform_versions_match);

    let mut mismatched = test_state().await;
    mismatched.build = BuildMetadata {
        api_revision: "b".repeat(40),
        ui_revision: "c".repeat(40),
        runtime_image_digest: digest.clone(),
        ui_image_digest: digest,
    };

    let Json(mismatched_response) = system_readiness(State(mismatched)).await.unwrap();
    assert!(!mismatched_response.platform_versions_match);
}

#[tokio::test]
async fn operator_evidence_json_snapshots_preserve_empty_filtered_and_paginated_shapes() {
    fn normalize_observed_at(mut value: Value) -> Value {
        if let Some(observations) = value.get_mut("observations").and_then(Value::as_array_mut) {
            for observation in observations {
                observation["observed_at"] = json!("<timestamp>");
            }
        }
        value
    }

    let state = test_state().await;
    let Json(empty) = list_observations(
        State(state.clone()),
        Query(ListObservationsQuery {
            limit: Some(3),
            offset: Some(0),
            ..ListObservationsQuery::default()
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_value(empty).unwrap(),
        json!({"observations": [], "count": 0, "limit": 3, "offset": 0})
    );

    for ordinal in 1..=3 {
        let source = if ordinal == 2 { "loki" } else { "prometheus" };
        let _ = create_observation(
            State(state.clone()),
            Json(CreateObservationRequest {
                id: Some(format!("obs_snapshot_{ordinal}")),
                session_id: None,
                run_id: None,
                source: source.to_string(),
                kind: "inventory".to_string(),
                subject: "apps-prod/yfinance-wrapper".to_string(),
                summary: format!("snapshot {ordinal}"),
                resource_namespace: Some("apps-prod".to_string()),
                resource_kind: Some("Deployment".to_string()),
                resource_name: Some("yfinance-wrapper".to_string()),
                resource_ref: Some(json!({
                    "kind": "Deployment",
                    "name": "yfinance-wrapper"
                })),
                artifact_id: None,
                data_json: Some(json!({"ordinal": ordinal})),
                actor: Some("snapshot-fixture".to_string()),
                reason: Some("D3 response characterization".to_string()),
            }),
        )
        .await
        .unwrap();
    }

    let Json(filtered) = list_observations(
        State(state.clone()),
        Query(ListObservationsQuery {
            source: Some("prometheus".to_string()),
            limit: Some(2),
            offset: Some(0),
            ..ListObservationsQuery::default()
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        normalize_observed_at(serde_json::to_value(filtered).unwrap()),
        json!({
            "observations": [
                {
                    "id": "obs_snapshot_3",
                    "run_id": null,
                    "source": "prometheus",
                    "kind": "inventory",
                    "subject": "apps-prod/yfinance-wrapper",
                    "summary": "snapshot 3",
                    "resource_namespace": "apps-prod",
                    "resource_kind": "Deployment",
                    "resource_name": "yfinance-wrapper",
                    "resource_ref": {"kind": "Deployment", "name": "yfinance-wrapper"},
                    "artifact_id": null,
                    "data_json": {"ordinal": 3},
                    "observed_at": "<timestamp>"
                },
                {
                    "id": "obs_snapshot_1",
                    "run_id": null,
                    "source": "prometheus",
                    "kind": "inventory",
                    "subject": "apps-prod/yfinance-wrapper",
                    "summary": "snapshot 1",
                    "resource_namespace": "apps-prod",
                    "resource_kind": "Deployment",
                    "resource_name": "yfinance-wrapper",
                    "resource_ref": {"kind": "Deployment", "name": "yfinance-wrapper"},
                    "artifact_id": null,
                    "data_json": {"ordinal": 1},
                    "observed_at": "<timestamp>"
                }
            ],
            "count": 2,
            "limit": 2,
            "offset": 0
        })
    );

    let Json(paginated) = list_observations(
        State(state),
        Query(ListObservationsQuery {
            limit: Some(1),
            offset: Some(1),
            ..ListObservationsQuery::default()
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        normalize_observed_at(serde_json::to_value(paginated).unwrap()),
        json!({
            "observations": [{
                "id": "obs_snapshot_2",
                "run_id": null,
                "source": "loki",
                "kind": "inventory",
                "subject": "apps-prod/yfinance-wrapper",
                "summary": "snapshot 2",
                "resource_namespace": "apps-prod",
                "resource_kind": "Deployment",
                "resource_name": "yfinance-wrapper",
                "resource_ref": {"kind": "Deployment", "name": "yfinance-wrapper"},
                "artifact_id": null,
                "data_json": {"ordinal": 2},
                "observed_at": "<timestamp>"
            }],
            "count": 1,
            "limit": 1,
            "offset": 1
        })
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

#[tokio::test]
async fn source_capability_posture_is_scoped_to_the_exact_repository() {
    let allowed_repository = "https://github.com/example/allowed.git".to_string();
    let state = test_state_with_git_observer("kubectl".into(), allowed_repository.clone()).await;

    let configured = source_capability_statuses_for_repository(&state, &allowed_repository)
        .await
        .unwrap();
    assert!(configured.iter().all(|status| {
        status.status == "configured_unverified" && status.verified_at.is_none()
    }));

    let now = crate::app::clock::current_millis();
    state
        .store
        .create_capability_verification(CreateCapabilityVerification {
            id: "capverify_source_reader_exact".into(),
            capability: "source_reader".into(),
            status: "available".into(),
            summary: format!("verified {allowed_repository}"),
            principal: Some("system:serviceaccount:pharness:source-reader".into()),
            repository: Some(allowed_repository.clone()),
            permission: Some("repository_read".into()),
            verified_at: now.to_string(),
            expires_at: (now + 900_000).to_string(),
        })
        .await
        .unwrap();

    let exact = source_capability_statuses_for_repository(&state, &allowed_repository)
        .await
        .unwrap();
    assert_eq!(
        exact
            .iter()
            .find(|status| status.capability == "source_reader")
            .unwrap()
            .status,
        "available"
    );

    let other = source_capability_statuses_for_repository(
        &state,
        "https://github.com/example/not-allowlisted.git",
    )
    .await
    .unwrap();
    assert!(other.iter().all(|status| {
        status.status == "unavailable"
            && status.verified_at.is_none()
            && status.expires_at.is_none()
    }));
}

pub(super) async fn test_state_with_cluster_tools(cluster_tools: ReadOnlyClusterTools) -> AppState {
    AppState {
        store: Arc::new(SqliteStore::connect_in_memory().await.unwrap()),
        worker: RunDispatcher::Disabled,
        cluster_tools,
        policy: SafetyPolicy::default(),
        worker_token: None,
        operator_tokens: Arc::new(Vec::new()),
        workspace: WorkspaceProvisioner::new(std::env::temp_dir(), Vec::new()),
        build: BuildMetadata::from_env(),
        protected_target: ProtectedTargetConfiguration::from_env(),
        environment_profiles: Arc::new(Vec::new()),
        repo_mode: RepoModeConfiguration::test_enabled(),
        inference: Arc::new(pharness_config::InferenceGatewayConfig::legacy_default()),
        agent_execution: Arc::new(pharness_config::AgentExecutionBackendConfig::disabled_default()),
        hosted_workflow: Arc::new(pharness_core::hosted_sdlc::HostedWorkflowConfig::default()),
    }
}

pub(super) async fn test_state_with_git_observer(
    kubectl_bin: String,
    allowed_repo: String,
) -> AppState {
    test_state_with_git_and_gitops(kubectl_bin, allowed_repo.clone(), allowed_repo).await
}

pub(super) async fn test_state_with_git_and_gitops(
    kubectl_bin: String,
    allowed_repo: String,
    gitops_repo: String,
) -> AppState {
    test_state_with_delivery_namespaces(kubectl_bin, allowed_repo, gitops_repo, vec!["ci".into()])
        .await
}

pub(super) async fn test_state_with_hosted_build(
    kubectl_bin: String,
    allowed_repo: String,
) -> AppState {
    test_state_with_delivery_namespaces(
        kubectl_bin,
        allowed_repo.clone(),
        allowed_repo,
        vec!["tekton-pipelines".into()],
    )
    .await
}

async fn test_state_with_delivery_namespaces(
    kubectl_bin: String,
    allowed_repo: String,
    gitops_repo: String,
    tekton_namespaces: Vec<String>,
) -> AppState {
    let store = Arc::new(SqliteStore::connect_in_memory().await.unwrap());
    let worker = RunDispatcher::Kubernetes(KubernetesJobDispatcher::new(
        store.clone(),
        kubectl_bin,
        WorkerKubernetesConfig {
            namespace: "pharness-test".to_string(),
            image: "example.test/pharness:latest".to_string(),
            inference_evaluation_image: "example.test/pharness-eval:latest".to_string(),
            inference_evaluation_node_hostname: None,
            service_account: "pharness-worker".to_string(),
            tekton_executor_service_account: "pharness-tekton-runner".to_string(),
            tekton_allowed_namespaces: tekton_namespaces,
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
            gitops_writer_allowed_repos: vec![gitops_repo.clone()],
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
            gitops_observer_allowed_repos: vec![gitops_repo],
            gitops_observer_github_api_url: "https://api.github.com".to_string(),
            gitops_observer_active_deadline_seconds: 300,
            gitops_observer_ttl_seconds_after_finished: 3600,
            source_reader_enabled: true,
            source_reader_service_account: "pharness-source-reader".to_string(),
            source_reader_token_secret_name: None,
            source_reader_allowed_repos: vec![allowed_repo],
            source_reader_active_deadline_seconds: 600,
            source_reader_ttl_seconds_after_finished: 3600,
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
        build: BuildMetadata::from_env(),
        protected_target: ProtectedTargetConfiguration::from_env(),
        environment_profiles: Arc::new(Vec::new()),
        repo_mode: RepoModeConfiguration::test_enabled(),
        inference: Arc::new(pharness_config::InferenceGatewayConfig::legacy_default()),
        agent_execution: Arc::new(pharness_config::AgentExecutionBackendConfig::disabled_default()),
        hosted_workflow: Arc::new(pharness_core::hosted_sdlc::HostedWorkflowConfig::default()),
    }
}

pub(super) async fn seed_approved_release(state: &AppState) -> String {
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

pub(super) async fn seed_approved_work_item_release(state: &AppState) -> String {
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

pub(super) fn fake_kubectl_script() -> PathBuf {
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

pub(super) fn fake_healthy_rollout_kubectl_script() -> PathBuf {
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

pub(super) fn fake_succeeded_tekton_kubectl_script() -> PathBuf {
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

pub(super) fn fake_completed_argo_wait_kubectl_script() -> PathBuf {
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

pub(super) async fn seed_completed_argo_sync(state: &AppState, release_id: &str) {
    seed_completed_argo_sync_with_contract(state, release_id, None).await;
}

pub(super) async fn seed_completed_argo_sync_with_contract(
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

pub(super) fn slow_fake_kubectl_script() -> PathBuf {
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
