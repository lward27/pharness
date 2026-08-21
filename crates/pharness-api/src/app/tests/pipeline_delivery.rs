use super::{
    approval_gates_from_work_item, build_pipeline_run_manifest, create_declared_deployment_handoff,
    create_deployment_contract, create_deployment_intent_trusted_envelope,
    create_registry_evidence_from_release, create_release_from_deployment_intent, create_work_item,
    current_pipeline_build_output, ensure_pipeline_evidence_ready_for_deployment,
    execute_deployment_intent, execute_work_item_action, execution_matches_pipeline_contract, fs,
    get_deployment_contract, internal_argo_sync_outcome, json, list_deployment_contracts,
    merge_pipeline_execution_state, persist_pipeline_build_output,
    persist_pipeline_execution_evidence, persist_pipeline_run_analysis,
    pipeline_build_output_from_analysis, preflight_deployment_intent, reconcile_work_item,
    set_pipeline_intent_evidence, tekton_execution_spec, transition_deployment_contract,
    transition_release, transition_work_plan, unique_suffix, validate_pipeline_deployment_handoff,
    validate_terminal_pipeline_run_analysis, work_item_flow, ApprovalGateListFilter,
    ArgoSyncOutcomeRequest, CreateArtifact, CreateChangeSet, CreateDeploymentContractRequest,
    CreateDeploymentIntent, CreateDeploymentIntentTrustedEnvelopeRequest, CreatePipelineIntent,
    CreateRegistryEvidenceFromReleaseRequest, CreateReleaseFromDeploymentIntentRequest,
    CreateRemediationPlan, CreateRun, CreateSession, CreateWorkItem, CreateWorkItemRequest,
    CreateWorkPlan, DeploymentIntentPreflightRequest, ExecuteDeploymentIntentRequest,
    ExecuteWorkItemActionRequest, Json, ListDeploymentContractsQuery, Path, PermissionsExt,
    PipelineDeploymentHandoffSpec, PipelineIntentExecutionOutcomeRequest, Query,
    ReconcileWorkItemRequest, RunId, SessionId, State, StatusCode, StoredPipelineContract,
    StoredPipelineIntent, TransitionDeploymentContractRequest, TransitionReleaseRequest,
    TransitionWorkPlanRequest,
};

use super::characterization::{seed_approved_release, test_state, test_state_with_git_observer};

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
