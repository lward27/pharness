//! Synthetic persisted history for candidate reconciliation tests only.
use super::{
    artifact_id, now, preparation,
    records::{Context, Window},
    AppState,
};
use pharness_core::tools::FinanceRuntimeEvidence;
use pharness_store::FinishWorkflowReconciliation;
use serde_json::{json, Value};

async fn staging_context(state: &AppState, deployment: &str, execution: &str) -> Context {
    let saved = preparation::saved(state, deployment, execution)
        .await
        .unwrap();
    let receipt = state
        .store
        .get_artifact(&artifact_id("commit", execution))
        .await
        .unwrap()
        .unwrap();
    Context::new(state, &saved, &receipt).await.unwrap()
}

pub(in crate::app) async fn seed_runtime_step(
    state: &AppState,
    deployment: &str,
    execution: &str,
    step: &str,
    body: Value,
) {
    staging_context(state, deployment, execution)
        .await
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
    let context = staging_context(state, deployment, execution).await;
    state
        .store
        .wake_workflow(&context.work_item_id, now())
        .await
        .unwrap();
    let claim = state
        .store
        .claim_due_workflow("interrupted-runtime-observer", now(), 60_000)
        .await
        .unwrap()
        .unwrap();
    let mut refs = context.operation.resource_refs.clone();
    for index in 1..=count {
        let step = format!("identity_{index}");
        refs[context.ref_key(&step)] =
            json!({"started_at_ms":now()-1,"artifact_id":context.id(&step)});
    }
    state
        .store
        .record_workflow_operation(
            &claim,
            &context.operation.id,
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
    let context = staging_context(state, deployment, execution).await;
    seed_window(state, &context, evidence, completed, change).await;
}

/// Persist one original candidate window for any bound runtime context
/// (staging, production or post-release observation) as if the controller
/// had collected it: the identity marker on the operation, the window record,
/// the native receipts and, when `completed`, the assessed result.
pub(in crate::app::hosted_controller) async fn seed_window(
    state: &AppState,
    context: &Context,
    evidence: FinanceRuntimeEvidence,
    completed: bool,
    change: Option<(&str, Value)>,
) {
    assert_eq!(evidence.expected, context.expected);
    assert_eq!(evidence.phase, context.phase);
    // Identity attempts the controller already recorded stay bound; the
    // synthetic window starts at the next free step.
    let step = (1..=crate::app::CONTROLLER_WAIT_MAX_CHECKS)
        .map(|n| format!("identity_{n}"))
        .find(|step| {
            context
                .operation
                .resource_refs
                .get(context.ref_key(step))
                .is_none()
        })
        .unwrap();
    let window = Window::new(
        context,
        evidence.window.start_unix_seconds as i64 * 1000 - 30_001,
        step.clone(),
    )
    .unwrap();
    assert_eq!(window.window, evidence.window);
    state
        .store
        .wake_workflow(&context.work_item_id, now())
        .await
        .unwrap();
    let claim = state
        .store
        .claim_due_workflow("runtime-test-seed", now(), 60_000)
        .await
        .unwrap()
        .unwrap();
    let mut refs = context.operation.resource_refs.clone();
    refs[context.ref_key(&step)] =
        json!({"started_at_ms":window.created_at_ms,"artifact_id":context.id(&step)});
    state
        .store
        .record_workflow_operation(
            &claim,
            &context.operation.id,
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
            &step,
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
