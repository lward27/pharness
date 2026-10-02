//! Synthetic persisted history for candidate reconciliation tests only.
use super::{
    artifact_id, now, preparation,
    records::{Context, Window},
    AppState,
};
use pharness_core::tools::FinanceRuntimeEvidence;
use pharness_store::FinishWorkflowReconciliation;
use serde_json::{json, Value};

pub(in crate::app) async fn seed_runtime_step(
    state: &AppState,
    deployment: &str,
    execution: &str,
    step: &str,
    body: Value,
) {
    let saved = preparation::saved(state, deployment, execution)
        .await
        .unwrap();
    let receipt = state
        .store
        .get_artifact(&artifact_id("commit", execution))
        .await
        .unwrap()
        .unwrap();
    Context::new(state, &saved, &receipt)
        .await
        .unwrap()
        .write(state, step, body)
        .await
        .unwrap();
}

pub(in crate::app) async fn seed_runtime_identity_attempts(
    state: &AppState,
    deployment: &str,
    execution: &str,
    count: u32,
) {
    let saved = preparation::saved(state, deployment, execution)
        .await
        .unwrap();
    let receipt = state
        .store
        .get_artifact(&artifact_id("commit", execution))
        .await
        .unwrap()
        .unwrap();
    let context = Context::new(state, &saved, &receipt).await.unwrap();
    state
        .store
        .wake_workflow(&saved.authority.work_item_id, now())
        .await
        .unwrap();
    let claim = state
        .store
        .claim_due_workflow("interrupted-runtime-observer", now(), 60_000)
        .await
        .unwrap()
        .unwrap();
    let mut refs = saved.operation.resource_refs.clone();
    for index in 1..=count {
        let step = format!("identity_{index}");
        refs[format!("staging_runtime_{step}")] =
            json!({"started_at_ms":now()-1,"artifact_id":context.id(&step)});
    }
    state
        .store
        .record_workflow_operation(
            &claim,
            &saved.operation.id,
            "running",
            &refs,
            "Synthetic interrupted observation attempts",
            now(),
        )
        .await
        .unwrap();
    state
        .store
        .finish_workflow_reconciliation(
            &claim,
            FinishWorkflowReconciliation {
                next_due_at: now() + 1,
                condition: "waiting",
                reason: "Interrupted candidate fixture ready",
                observed_state_hash: None,
            },
            now(),
        )
        .await
        .unwrap();
}

pub(in crate::app) async fn seed_runtime(
    state: &AppState,
    deployment: &str,
    execution: &str,
    evidence: FinanceRuntimeEvidence,
    completed: bool,
    change: Option<(&str, Value)>,
) {
    let saved = preparation::saved(state, deployment, execution)
        .await
        .unwrap();
    let receipt = state
        .store
        .get_artifact(&artifact_id("commit", execution))
        .await
        .unwrap()
        .unwrap();
    let context = Context::new(state, &saved, &receipt).await.unwrap();
    assert_eq!(evidence.expected, context.expected);
    let window = Window::new(
        &context,
        evidence.window.start_unix_seconds as i64 * 1000 - 30_001,
        "identity_1".into(),
    )
    .unwrap();
    assert_eq!(window.window, evidence.window);
    state
        .store
        .wake_workflow(&saved.authority.work_item_id, now())
        .await
        .unwrap();
    let claim = state
        .store
        .claim_due_workflow("runtime-test-seed", now(), 60_000)
        .await
        .unwrap()
        .unwrap();
    let mut refs = saved.operation.resource_refs.clone();
    refs["staging_runtime_identity_1"] =
        json!({"started_at_ms":window.created_at_ms,"artifact_id":context.id("identity_1")});
    state
        .store
        .record_workflow_operation(
            &claim,
            &saved.operation.id,
            "running",
            &refs,
            "Synthetic original candidate observation",
            now(),
        )
        .await
        .unwrap();
    context
        .write(
            state,
            "identity_1",
            json!({"observation":evidence.identity_before}),
        )
        .await
        .unwrap();
    context
        .write(state, "window", json!({"window_record":window}))
        .await
        .unwrap();
    context
        .write(
            state,
            "probes",
            json!({"observation":evidence.functional_probes}),
        )
        .await
        .unwrap();
    if completed {
        context
            .write(
                state,
                "after",
                json!({"observation":evidence.identity_after}),
            )
            .await
            .unwrap();
        let time = evidence.window.end_unix_seconds * 1000 + 12_000;
        let assessment = evidence.assess(time);
        assert_eq!(assessment["runtime_verification"], "passed", "{assessment}");
        let mut result =
            json!({"completed_at_ms":time,"runtime_evidence":evidence,"assessment":assessment});
        if let Some((pointer, value)) = change {
            *result.pointer_mut(pointer).unwrap() = value;
        }
        context.write(state, "result", result).await.unwrap();
    }
    state
        .store
        .finish_workflow_reconciliation(
            &claim,
            FinishWorkflowReconciliation {
                next_due_at: now() + 1,
                condition: "waiting",
                reason: "Candidate fixture ready",
                observed_state_hash: None,
            },
            now(),
        )
        .await
        .unwrap();
}
