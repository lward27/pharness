//! Synthetic persisted production and post-release windows for controller tests.
use super::{artifact_id, delivery, observe, runtime, AppState, OBSERVE_ACTION};
use pharness_core::tools::FinanceRuntimeEvidence;

pub(in crate::app) async fn seed_production_window(
    state: &AppState,
    deployment: &str,
    execution: &str,
    evidence: FinanceRuntimeEvidence,
    completed: bool,
) {
    let saved = delivery::saved(state, deployment, execution).await.unwrap();
    let receipt = state
        .store
        .get_artifact(&artifact_id("commit", execution))
        .await
        .unwrap()
        .unwrap();
    let context = delivery::runtime_context(&saved, &receipt).unwrap();
    runtime::test_support::seed_window(state, &context, evidence, completed, None).await;
}

pub(in crate::app) async fn seed_observe_window(
    state: &AppState,
    work_item_id: &str,
    evidence: FinanceRuntimeEvidence,
    completed: bool,
) {
    let operation = state
        .store
        .active_workflow_operation(work_item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(operation.action, OBSERVE_ACTION);
    let context = observe::context(state, &operation).await.unwrap();
    runtime::test_support::seed_window(state, &context, evidence, completed, None).await;
}
