use super::repo_mode_v1::repo_fixture_with_workflow;
use crate::app::repo_mode::start_repo_builder;
use pharness_store::{CreateStageChainAuthorization, CreateWorkspace};
use serde_json::json;

// Exercise the real dispatch entry with a disabled worker. Invalid authority
// must be rejected before consulting the worker or creating execution records.
#[tokio::test]
async fn repair_rechecks_original_plan_and_expiry_before_dispatch() {
    let fixture = repo_fixture_with_workflow("repair_authority", false, false).await;
    let state = &fixture.state;
    let store = &state.store;
    let item = store
        .get_work_item(&fixture.work_item_id)
        .await
        .unwrap()
        .unwrap();
    let metadata = store
        .get_repo_work_item_metadata(&item.id)
        .await
        .unwrap()
        .unwrap();
    let plan = store
        .get_work_plan_by_work_item(&item.id)
        .await
        .unwrap()
        .unwrap();
    let workspace = store
        .create_workspace(CreateWorkspace {
            id: "workspace_repair_authority".into(),
            work_item_id: item.id.clone(),
            run_id: None,
            status: "ready".into(),
            source_repo: item.source_repo.clone(),
            source_ref: item.source_ref.clone(),
            resolved_commit: item.source_commit.clone(),
            branch: Some("pharness/repair-authority".into()),
            retention_status: "retained".into(),
            actor: Some("unit-test".into()),
            reason: Some("authority fixture".into()),
        })
        .await
        .unwrap();
    let chain = store
        .create_stage_chain_authorization(CreateStageChainAuthorization {
            id: "chain_repair_authority".into(),
            work_item_id: item.id.clone(),
            work_plan_id: plan.id.clone(),
            work_plan_revision: plan.revision,
            product_model_snapshot_id: metadata.product_model_snapshot_id.clone(),
            product_model_snapshot_hash: metadata.product_model_snapshot_hash.clone(),
            repository_id: metadata.repository_id.clone(),
            source_commit: item.source_commit.clone().unwrap(),
            workspace_id: workspace.id.clone(),
            writable_paths: json!(["src/**"]),
            profile_chain: json!([]),
            budget_chain: json!([]),
            state_hash: "sha256:unit-test-authority".into(),
            created_by: "unit-test".into(),
            creation_reason: "original bounded chain".into(),
            expires_at: "9999999999999".into(),
        })
        .await
        .unwrap();
    let contract = serde_json::from_value(item.repository_contract_json.clone().unwrap()).unwrap();
    let before = store.list_stage_executions(&item.id).await.unwrap();
    for (field, value) in [
        ("work_plan_revision", json!(plan.revision + 1)),
        ("expires_at", json!("0")),
        ("expires_at", json!("invalid")),
        ("source_commit", json!("b".repeat(40))),
        ("product_model_snapshot_hash", json!("changed")),
        ("status", json!("revoked")),
        ("work_item_id", json!("another_work_item")),
    ] {
        let mut altered = serde_json::to_value(&chain).unwrap();
        altered[field] = value;
        let altered = serde_json::from_value(altered).unwrap();
        for profile in ["repo-builder", "repo-repair"] {
            let error = start_repo_builder(
                state,
                &metadata,
                &item,
                &plan,
                &workspace,
                &altered,
                &contract,
                "controller:repo-mode",
                "bounded dispatch",
                true,
                profile,
                None,
            )
            .await
            .unwrap_err();
            assert!(
                error.message.contains("stage-chain authorization"),
                "{profile} / {field}: {}",
                error.message
            );
        }
    }
    // Original source-only approvals remain readable; this reaches the actual
    // unavailable-worker boundary rather than requiring new Planner metadata.
    let error = start_repo_builder(
        state,
        &metadata,
        &item,
        &plan,
        &workspace,
        &chain,
        &contract,
        "controller:repo-mode",
        "bounded dispatch",
        true,
        "repo-repair",
        None,
    )
    .await
    .unwrap_err();
    assert!(
        error
            .message
            .contains("model execution worker is unavailable"),
        "{}",
        error.message
    );
    assert_eq!(store.list_stage_executions(&item.id).await.unwrap(), before);
}
