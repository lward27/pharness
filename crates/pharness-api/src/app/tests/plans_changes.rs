use super::{
    attach_deployment_intent_evidence, attach_pipeline_intent_evidence, attach_release_evidence,
    change_set_flow, change_set_readiness, create_change_set, create_change_set_trusted_envelope,
    create_deployment_intent_from_pipeline_intent, create_observation,
    create_pipeline_intent_from_change_set, create_registry_evidence_from_release,
    create_release_from_deployment_intent, create_run, create_work_plan_from_remediation_plan,
    create_work_plan_trusted_envelope, decide_run_approval, get_deployment_intent,
    get_pipeline_intent, get_registry_evidence, get_release, json, list_audit_events,
    list_change_sets, list_deployment_intents, list_pipeline_intents, list_registry_evidence,
    list_releases, revise_change_set, revise_work_plan, satisfy_approval_gate,
    transition_change_set, transition_deployment_intent, transition_pipeline_intent,
    transition_registry_evidence, transition_release, transition_work_plan, work_plan_flow,
    work_plan_readiness, AgentAction, ApprovalDecision, ApprovalGateListFilter,
    AttachDeploymentIntentEvidenceRequest, AttachPipelineIntentEvidenceRequest,
    AttachReleaseEvidenceRequest, CreateApproval, CreateApprovalGate, CreateChangeSetRequest,
    CreateDeploymentIntentFromPipelineIntentRequest, CreateIncident, CreateObservation,
    CreateObservationRequest, CreatePipelineIntentFromChangeSetRequest,
    CreateRegistryEvidenceFromReleaseRequest, CreateReleaseFromDeploymentIntentRequest,
    CreateRemediationPlan, CreateRunRequest, CreateTrustedEnvelopeRequest,
    CreateWorkPlanFromRemediationPlanRequest, DecideApprovalGateRequest, DecideApprovalRequest,
    Json, ListAuditEventsQuery, ListChangeSetsQuery, ListDeploymentIntentsQuery,
    ListPipelineIntentsQuery, ListRegistryEvidenceQuery, ListReleasesQuery, Path, Query,
    ReviseChangeSetRequest, ReviseWorkPlanRequest, RunScope, SessionId, State, StatusCode,
    TransitionChangeSetRequest, TransitionDeploymentIntentRequest, TransitionPipelineIntentRequest,
    TransitionRegistryEvidenceRequest, TransitionReleaseRequest, TransitionWorkPlanRequest,
};

use super::characterization::test_state;

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
            inference_policy: None,
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
            inference_policy: None,
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
            inference_policy: None,
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
            inference_policy: None,
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
            inference_policy: None,
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
