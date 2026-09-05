use super::execute_work_item_action;
use super::repo_mode_v1::repo_fixture_with_workflow;
use crate::app::repo_mode::repo_work_item_flow;
use crate::dto::ExecuteWorkItemActionRequest;
use axum::extract::{Path, State};
use axum::Json;

fn request(hash: &str) -> ExecuteWorkItemActionRequest {
    ExecuteWorkItemActionRequest {
        actor: Some("operator".into()),
        reason: "bounded workflow control test".into(),
        state_hash: hash.into(),
        inference_policies: None,
        execution_policies: None,
    }
}

#[tokio::test]
async fn hosted_flow_reads_do_not_claim_or_advance_and_hide_routine_manual_actions() {
    let fixture = repo_fixture_with_workflow("hosted_read_only_control", false, true).await;
    let before = fixture
        .state
        .store
        .get_workflow_reconciliation(&fixture.work_item_id)
        .await
        .unwrap();
    let stages = fixture
        .state
        .store
        .list_stage_executions(&fixture.work_item_id)
        .await
        .unwrap();
    for _ in 0..2 {
        let flow = repo_work_item_flow(&fixture.state, &fixture.work_item_id)
            .await
            .unwrap();
        assert!(!flow.reconcile_preview.can_apply);
        assert_eq!(
            flow.action_rail
                .iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>(),
            vec!["pause_workflow", "cancel_workflow"]
        );
        assert_eq!(
            flow.repo_mode.unwrap()["workflow_control"]["control"],
            "active"
        );
    }
    assert_eq!(
        fixture
            .state
            .store
            .get_workflow_reconciliation(&fixture.work_item_id)
            .await
            .unwrap(),
        before
    );
    assert_eq!(
        fixture
            .state
            .store
            .list_stage_executions(&fixture.work_item_id)
            .await
            .unwrap(),
        stages
    );
}

#[tokio::test]
async fn hosted_controls_are_versioned_and_never_rebind_budget_or_authorization() {
    let fixture = repo_fixture_with_workflow("hosted_versioned_control", false, true).await;
    let metadata = fixture
        .state
        .store
        .get_repo_work_item_metadata(&fixture.work_item_id)
        .await
        .unwrap();
    let item = fixture
        .state
        .store
        .get_work_item(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    let flow = repo_work_item_flow(&fixture.state, &fixture.work_item_id)
        .await
        .unwrap();
    let hash = flow.action_rail[0].state_hash.clone();
    let Json(paused) = execute_work_item_action(
        State(fixture.state.clone()),
        None,
        Path((fixture.work_item_id.clone(), "pause_workflow".into())),
        Json(request(&hash)),
    )
    .await
    .unwrap();
    assert_eq!(paused["workflow_control"]["control"], "paused");
    assert_eq!(
        paused["workflow_control"]["observation_and_authorized_recovery_continue"],
        true
    );
    assert!(execute_work_item_action(
        State(fixture.state.clone()),
        None,
        Path((fixture.work_item_id.clone(), "pause_workflow".into())),
        Json(request(&hash))
    )
    .await
    .is_err());
    let flow = repo_work_item_flow(&fixture.state, &fixture.work_item_id)
        .await
        .unwrap();
    assert_eq!(flow.action_rail[0].id, "resume_workflow");
    let Json(resumed) = execute_work_item_action(
        State(fixture.state.clone()),
        None,
        Path((fixture.work_item_id.clone(), "resume_workflow".into())),
        Json(request(&flow.action_rail[0].state_hash)),
    )
    .await
    .unwrap();
    assert_eq!(resumed["workflow_control"]["control"], "active");
    let flow = repo_work_item_flow(&fixture.state, &fixture.work_item_id)
        .await
        .unwrap();
    let Json(cancelled) = execute_work_item_action(
        State(fixture.state.clone()),
        None,
        Path((fixture.work_item_id.clone(), "cancel_workflow".into())),
        Json(request(&flow.action_rail[1].state_hash)),
    )
    .await
    .unwrap();
    assert_eq!(cancelled["workflow_control"]["control"], "cancelled");
    let flow = repo_work_item_flow(&fixture.state, &fixture.work_item_id)
        .await
        .unwrap();
    assert!(flow.action_rail.is_empty());
    assert_eq!(
        flow.repo_mode.unwrap()["workflow_control"]["control"],
        "cancelled"
    );
    assert_eq!(
        fixture
            .state
            .store
            .get_repo_work_item_metadata(&fixture.work_item_id)
            .await
            .unwrap(),
        metadata
    );
    let after = fixture
        .state
        .store
        .get_work_item(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.run_budget, item.run_budget);
    assert_eq!(after.attempt_count, item.attempt_count);
}

#[tokio::test]
async fn legacy_actions_remain_available_but_hosted_routine_clicks_are_retired() {
    let legacy = repo_fixture_with_workflow("legacy_controls", false, false).await;
    let flow = repo_work_item_flow(&legacy.state, &legacy.work_item_id)
        .await
        .unwrap();
    assert!(flow.action_rail.iter().any(|a| a.id == "start_planner"));
    let hosted = repo_fixture_with_workflow("hosted_retired_click", false, true).await;
    let before = hosted
        .state
        .store
        .list_stage_executions(&hosted.work_item_id)
        .await
        .unwrap();
    let flow = repo_work_item_flow(&hosted.state, &hosted.work_item_id)
        .await
        .unwrap();
    let result = execute_work_item_action(
        State(hosted.state.clone()),
        None,
        Path((hosted.work_item_id.clone(), "start_planner".into())),
        Json(request(&flow.action_rail[0].state_hash)),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(
        hosted
            .state
            .store
            .list_stage_executions(&hosted.work_item_id)
            .await
            .unwrap(),
        before
    );
}

#[tokio::test]
async fn scheduler_leaves_legacy_idle_and_records_unqualified_hosted_blocker() {
    use crate::app::hosted_controller::reconcile_once;
    let legacy = repo_fixture_with_workflow("scheduler_legacy", false, false).await;
    assert!(!reconcile_once(&legacy.state, "api").await.unwrap());
    let fixture = repo_fixture_with_workflow("scheduler_unqualified", false, true).await;
    assert!(reconcile_once(&fixture.state, "api").await.unwrap());
    let control = fixture
        .state
        .store
        .get_workflow_reconciliation(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(control.condition, "blocked");
    assert!(control.claim_owner.is_none());
    assert!(
        control.condition_reason.contains("gateway")
            || control.condition_reason.contains("profile"),
        "{}",
        control.condition_reason
    );
    assert!(fixture
        .state
        .store
        .active_workflow_operation(&fixture.work_item_id)
        .await
        .unwrap()
        .is_none());
    assert!(!reconcile_once(&fixture.state, "api").await.unwrap());
}

async fn completed_planner(
    fixture: &super::repo_mode_v1::RepoDeliveryFixture,
    contradictions: &[&str],
) -> pharness_store::StoredRun {
    use pharness_core::{RunId, SessionId};
    use pharness_store::{CreateAgentContextPack, CreateRun, CreateSession, CreateStageExecution};
    use serde_json::json;
    let store = &fixture.state.store;
    let metadata = store
        .get_repo_work_item_metadata(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    let id = format!("run_recover_{}", fixture.work_item_id);
    let session = SessionId::new(format!("ses_recover_{}", fixture.work_item_id));
    let stage = format!("stage_recover_{}", fixture.work_item_id);
    let context = format!("context_recover_{}", fixture.work_item_id);
    store
        .create_session(CreateSession {
            id: session.clone(),
            title: "Recovery fixture".into(),
            cwd: "/workspace".into(),
        })
        .await
        .unwrap();
    let run = store.create_run(CreateRun {
        id:RunId::new(id), session_id:session, user_task:"Bounded planner fixture".into(), cwd:"/workspace".into(), max_turns:10, initial_status:"completed".into(),
        execution_target_json:json!({"hosted_workflow_policy_hash":metadata.workflow_policy_hash,
            "run_scope":{"work_item_id":fixture.work_item_id}, "repo_mode":{"stage_execution_id":stage,"stage":"plan"}}),
    }).await.unwrap();
    store
        .create_stage_execution(CreateStageExecution {
            id: stage.clone(),
            work_item_id: fixture.work_item_id.clone(),
            stage_key: "plan".into(),
            sequence: 1,
            status: "completed".into(),
            agent_profile_id: Some("repo-planner".into()),
            agent_profile_version: Some("fixture".into()),
            agent_profile_hash: Some("sha256:fixture".into()),
            context_pack_id: None,
            run_id: Some(run.id.clone()),
            workspace_id: None,
            input_snapshot: json!({}),
            input_hash: "sha256:fixture".into(),
        })
        .await
        .unwrap();
    store
        .create_agent_context_pack(CreateAgentContextPack {
            id: context,
            work_item_id: fixture.work_item_id.clone(),
            stage_execution_id: stage,
            context: json!({"schema_version":pharness_core::AGENT_CONTEXT_SCHEMA}),
            estimated_tokens: 10,
            content_hash: "sha256:fixture".into(),
        })
        .await
        .unwrap();
    let plan = store
        .get_work_plan_by_work_item(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    let document = json!({"title":"Bounded fixture", "summary":"Preserve existing behavior", "steps":[{"title":"Test", "description":"Run existing checks", "acceptance_names":["unit"]}], "risk_level":"low"});
    let plan = store
        .revise_work_plan(
            &plan.id,
            pharness_store::UpdateWorkPlanRevision {
                title: None,
                summary: None,
                risk_level: None,
                requires_approval: None,
                work_plan_json: document.clone(),
                session_id: Some(run.session_id.clone()),
                run_id: Some(run.id.clone()),
                actor: Some("controller".into()),
                reason: Some("Validated Planner fixture".into()),
            },
        )
        .await
        .unwrap();
    let outcome = json!({"schema_version":pharness_core::STAGE_OUTCOME_SCHEMA,"work_item_id":fixture.work_item_id,"stage_execution_id":format!("stage_recover_{}",fixture.work_item_id),"stage":"plan","status":"succeeded","contradictions":contradictions,"outputs":[{"kind":"work_plan","id":plan.id,"revision":plan.revision}],"agent_claims":[{"kind":"planner_submission","document":document}]});
    store
        .seal_stage_outcome(pharness_store::SealStageOutcome {
            id: format!("outcome_recover_{}", fixture.work_item_id),
            stage_execution_id: format!("stage_recover_{}", fixture.work_item_id),
            work_item_id: fixture.work_item_id.clone(),
            stage_key: "plan".into(),
            status: "succeeded".into(),
            content_hash: pharness_core::canonical_json_sha256(&outcome).unwrap(),
            outcome,
            state_version: metadata.state_version,
            supersedes_outcome_id: None,
            effective: true,
            actor: "controller".into(),
            reason: "Validated Planner fixture".into(),
        })
        .await
        .unwrap();
    run
}

#[tokio::test]
async fn scheduler_applies_saved_plan_authority_once_without_a_browser_action() {
    use crate::app::hosted_controller::reconcile_once;
    let fixture = repo_fixture_with_workflow("scheduler_plan", false, true).await;
    completed_planner(&fixture, &[]).await;
    let plan = fixture
        .state
        .store
        .get_work_plan_by_work_item(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    fixture
        .state
        .store
        .update_work_plan_status(&plan.id, "proposed", Some("fixture".into()), None)
        .await
        .unwrap();
    assert!(reconcile_once(&fixture.state, "api").await.unwrap());
    let approved = fixture
        .state
        .store
        .get_work_plan(&plan.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(approved.status, "approved");
    let recorded = fixture
        .state
        .store
        .get_workflow_reconciliation(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        recorded.condition, "progressing",
        "{}",
        recorded.condition_reason
    );
    assert!(fixture
        .state
        .store
        .active_workflow_operation(&fixture.work_item_id)
        .await
        .unwrap()
        .is_none());
    assert!(!reconcile_once(&fixture.state, "replacement-api")
        .await
        .unwrap());
    assert_eq!(
        fixture
            .state
            .store
            .get_work_plan(&plan.id)
            .await
            .unwrap()
            .unwrap(),
        approved
    );
}

#[tokio::test]
async fn scheduler_adopts_lost_acknowledgement_and_observes_terminal_run_while_paused() {
    use crate::app::{clock::current_millis, hosted_controller::reconcile_once};
    use pharness_store::{BeginWorkflowOperation, RunListFilter};
    use serde_json::json;
    let fixture = repo_fixture_with_workflow("scheduler_recovery", false, true).await;
    let store = &fixture.state.store;
    let time = current_millis() as i64;
    let claim = store
        .claim_due_workflow("departed-api", time, 60_000)
        .await
        .unwrap()
        .unwrap();
    store
        .begin_workflow_operation(
            &claim,
            BeginWorkflowOperation {
                id: "op_lost_ack",
                action: "start_planner",
                input_hash: "sha256:fixture",
                effect: "development",
                resource_keys: &["coding"],
            },
            time,
        )
        .await
        .unwrap();
    store
        .record_workflow_operation(
            &claim,
            "op_lost_ack",
            "running",
            &json!({"action_resource":fixture.work_item_id,"before_run_ids":[]}),
            "dispatch recorded",
            time,
        )
        .await
        .unwrap();
    let run = completed_planner(&fixture, &[]).await;
    store
        .set_workflow_control(
            &fixture.work_item_id,
            1,
            "paused",
            "operator",
            "pause while worker finishes",
            current_millis() as i64,
        )
        .await
        .unwrap();
    assert!(reconcile_once(&fixture.state, "replacement-api")
        .await
        .unwrap());
    let op = store
        .get_workflow_operation("op_lost_ack")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(op.status, "succeeded");
    assert_eq!(op.resource_refs["run_id"], run.id.as_str());
    assert_eq!(op.resource_refs["terminal_run_status"], "completed");
    assert_eq!(
        store
            .get_workflow_reconciliation(&fixture.work_item_id)
            .await
            .unwrap()
            .unwrap()
            .control,
        "paused"
    );
    for _ in 0..2 {
        store
            .wake_workflow(&fixture.work_item_id, current_millis() as i64)
            .await
            .unwrap();
        assert!(reconcile_once(&fixture.state, "replacement-api")
            .await
            .unwrap());
    }
    assert_eq!(
        store
            .list_runs(RunListFilter {
                work_item_id: Some(fixture.work_item_id.clone()),
                limit: 200,
                ..Default::default()
            })
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(store.get_run(&run.id).await.unwrap().unwrap(), run);
}

#[tokio::test]
async fn scheduler_rejects_changed_plan_revision_and_unresolved_contradictions() {
    use crate::app::hosted_controller::reconcile_once;
    for conflict in [false, true] {
        let fixture = repo_fixture_with_workflow(
            if conflict {
                "scheduler_contradiction"
            } else {
                "scheduler_stale_plan"
            },
            false,
            true,
        )
        .await;
        let contradictions = if conflict {
            vec!["Unresolved conflicting requirement"]
        } else {
            vec![]
        };
        completed_planner(&fixture, &contradictions).await;
        let plan = fixture
            .state
            .store
            .get_work_plan_by_work_item(&fixture.work_item_id)
            .await
            .unwrap()
            .unwrap();
        if !conflict {
            fixture
                .state
                .store
                .revise_work_plan(
                    &plan.id,
                    pharness_store::UpdateWorkPlanRevision {
                        title: None,
                        summary: None,
                        risk_level: None,
                        requires_approval: None,
                        work_plan_json: serde_json::json!({"summary":"Changed after validation"}),
                        session_id: None,
                        run_id: None,
                        actor: Some("operator".into()),
                        reason: Some("Changed fixture".into()),
                    },
                )
                .await
                .unwrap();
        }
        fixture
            .state
            .store
            .update_work_plan_status(&plan.id, "proposed", Some("fixture".into()), None)
            .await
            .unwrap();
        assert!(reconcile_once(&fixture.state, "api").await.unwrap());
        assert_eq!(
            fixture
                .state
                .store
                .get_work_plan(&plan.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            "proposed"
        );
        let control = fixture
            .state
            .store
            .get_workflow_reconciliation(&fixture.work_item_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(control.condition, "blocked");
        assert!(
            control.condition_reason.contains(if conflict {
                "contradictions"
            } else {
                "revision"
            }),
            "{}",
            control.condition_reason
        );
        assert!(fixture
            .state
            .store
            .active_workflow_operation(&fixture.work_item_id)
            .await
            .unwrap()
            .is_none());
    }
}

#[tokio::test]
async fn unchanged_wait_expiry_does_not_create_another_run_or_extend_its_budget() {
    use crate::app::{
        clock::current_millis, hosted_controller::reconcile_once, CONTROLLER_WAIT_MAX_CHECKS,
    };
    use pharness_store::FinishWorkflowReconciliation;
    let fixture = repo_fixture_with_workflow("scheduler_wait_limit", false, true).await;
    assert!(reconcile_once(&fixture.state, "api").await.unwrap());
    let store = &fixture.state.store;
    let original = store
        .get_work_item(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    let hash = store
        .get_workflow_reconciliation(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap()
        .observed_state_hash
        .unwrap();
    // Advance the persisted unchanged-observation count with a synthetic clock;
    // no wall-clock sleep or external worker is needed to exercise the limit.
    for tick in 0..=CONTROLLER_WAIT_MAX_CHECKS {
        let time = current_millis() as i64 + i64::from(tick) * 2;
        store
            .wake_workflow(&fixture.work_item_id, time)
            .await
            .unwrap();
        let claim = store
            .claim_due_workflow("fixture-observer", time, 60_000)
            .await
            .unwrap()
            .unwrap();
        store
            .finish_workflow_reconciliation(
                &claim,
                FinishWorkflowReconciliation {
                    next_due_at: time + 1,
                    condition: "waiting",
                    reason: "unchanged fixture",
                    observed_state_hash: Some(&hash),
                },
                time,
            )
            .await
            .unwrap();
    }
    store
        .wake_workflow(&fixture.work_item_id, current_millis() as i64)
        .await
        .unwrap();
    assert!(reconcile_once(&fixture.state, "replacement-api")
        .await
        .unwrap());
    let control = store
        .get_workflow_reconciliation(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        control.condition, "wait_expired",
        "{}",
        control.condition_reason
    );
    assert!(store
        .active_workflow_operation(&fixture.work_item_id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        store
            .get_work_item(&fixture.work_item_id)
            .await
            .unwrap()
            .unwrap(),
        original
    );
}
