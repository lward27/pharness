use super::{
    approval_gate_summary, approval_summary, cancel_run, change_set_readiness, config_effective,
    create_incident, create_observation, create_operator_run, create_permission_grant,
    create_registry_evidence_from_registry_inspection, create_registry_evidence_from_release,
    create_remediation_plan, create_run, create_work_plan_from_remediation_plan, deny_approval,
    execute_capability, fs, get_approval, get_approval_gate, get_artifact, get_incident,
    get_observation, get_permission_grant, get_remediation_plan, get_run, get_run_diff,
    get_run_events, get_work_plan, last_event_seq, list_approval_gates, list_approvals,
    list_audit_events, list_incidents, list_observations, list_permission_grants,
    list_remediation_plans, list_run_artifacts, list_run_observations, list_runs, list_work_plans,
    parse_last_event_id, policy_json, required_baseline_capability_result, revoke_permission_grant,
    run_policy, run_summary, satisfy_approval_gate, stream_start_seq, transition_registry_evidence,
    transition_remediation_plan, validate_permission_grant_request, AgentAction,
    ApprovalGateSummaryQuery, ApprovalSummaryQuery, CreateApproval, CreateApprovalGate,
    CreateArtifact, CreateFileChange, CreateIncident, CreateIncidentRequest, CreateObservation,
    CreateObservationRequest, CreatePermissionGrantRequest,
    CreateRegistryEvidenceFromInspectionRequest, CreateRegistryEvidenceFromReleaseRequest,
    CreateRemediationPlan, CreateRemediationPlanRequest, CreateRunRequest,
    CreateWorkPlanFromRemediationPlanRequest, DecideApprovalGateRequest, ExecuteCapabilityRequest,
    ExecuteCapabilityResponse, Extension, HeaderMap, HeaderValue, Json, ListApprovalGatesQuery,
    ListApprovalsQuery, ListAuditEventsQuery, ListIncidentsQuery, ListObservationsQuery,
    ListPermissionGrantsQuery, ListRemediationPlansQuery, ListRunsQuery, ListWorkPlansQuery,
    OperatorIdentity, Path, PolicyDecision, PolicyMode, Query, ReadOnlyClusterTools,
    ReviewApprovalRequest, RevokePermissionGrantRequest, RiskLevel, RunScope, SafetyPolicy, State,
    StatusCode, StreamRunEventsQuery, TransitionRegistryEvidenceRequest,
    TransitionRemediationPlanRequest,
};

use super::characterization::{
    fake_kubectl_script, seed_approved_release, slow_fake_kubectl_script, test_state,
    test_state_with_cluster_tools,
};

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
            inference_policy: None,
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
            product_id: None,
            work_item_id: None,
            repository_id: None,
            stage_execution_id: None,
            agent_profile_id: None,
            lifecycle: None,
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
            inference_policy: None,
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
            product_id: None,
            work_item_id: None,
            repository_id: None,
            stage_execution_id: None,
            agent_profile_id: None,
            lifecycle: None,
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
                inference_policy: None,
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
            product_id: None,
            work_item_id: None,
            repository_id: None,
            stage_execution_id: None,
            agent_profile_id: None,
            lifecycle: None,
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
            inference_policy: None,
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
            inference_policy: None,
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
            inference_policy: None,
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
            product_id: None,
            work_item_id: None,
            repository_id: None,
            stage_execution_id: None,
            agent_profile_id: None,
            lifecycle: None,
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
            product_id: None,
            work_item_id: None,
            repository_id: None,
            stage_execution_id: None,
            agent_profile_id: None,
            lifecycle: None,
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

    let Json(grant) = create_permission_grant(
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
            inference_policy: None,
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
            inference_policy: None,
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
            inference_policy: None,
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
            inference_policy: None,
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
            inference_policy: None,
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

    let Json(created) = create_permission_grant(
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
            inference_policy: None,
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
            inference_policy: None,
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
            inference_policy: None,
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
            inference_policy: None,
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
