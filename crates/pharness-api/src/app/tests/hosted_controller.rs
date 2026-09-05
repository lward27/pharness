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
