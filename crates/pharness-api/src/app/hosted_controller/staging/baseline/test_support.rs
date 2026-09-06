//! Persisted-clock fixtures only; no production clock overrides or live access.
use super::{now, preparation, records, AppState};
use pharness_core::tools::FinanceRuntimeEvidence;
use pharness_store::{BeginWorkflowOperation, FinishWorkflowReconciliation};
use serde_json::{json, Value};

pub(in crate::app) async fn seed_pending(state: &AppState, item: &str, past: i64) {
    let snapshot = super::super::super::state::Snapshot::load(state, item)
        .await
        .unwrap();
    let (input, _) = super::super::candidate(state, &snapshot)
        .await
        .unwrap()
        .unwrap();
    let id = format!(
        "workflowop_{}",
        super::super::hash(&json!([item, super::super::ACTION, input]))
            .unwrap()
            .trim_start_matches("sha256:")
    );
    let keys = [
        format!("repository:{}", snapshot.metadata.repository_id),
        format!("delivery:{}:staging", snapshot.metadata.repository_id),
        "gitops:lucas_engineering:main".into(),
    ];
    state.store.wake_workflow(item, past).await.unwrap();
    let claim = state
        .store
        .claim_due_workflow("baseline-test-original", past, 60_000)
        .await
        .unwrap()
        .unwrap();
    state
        .store
        .begin_workflow_operation(
            &claim,
            BeginWorkflowOperation {
                id: &id,
                action: super::super::ACTION,
                input_hash: &input,
                effect: "development",
                resource_keys: &keys.iter().map(String::as_str).collect::<Vec<_>>(),
            },
            past,
        )
        .await
        .unwrap();
    state
        .store
        .finish_workflow_reconciliation(
            &claim,
            FinishWorkflowReconciliation {
                next_due_at: now(),
                condition: "waiting",
                reason: "Original staging preparation interrupted",
                observed_state_hash: None,
            },
            past + 1,
        )
        .await
        .unwrap();
}

pub(in crate::app) async fn seed_baseline(
    state: &AppState,
    deployment: &str,
    execution: &str,
    evidence: FinanceRuntimeEvidence,
    change: Option<(&str, Value)>,
) {
    seed_inputs(state, deployment, execution, &evidence).await;
    let saved = preparation::saved(state, deployment, execution)
        .await
        .unwrap();
    records::write(
        state,
        &saved,
        "after",
        json!({"observation":evidence.identity_after}),
    )
    .await
    .unwrap();
    let completed = (evidence.window.end_unix_seconds * 1000) + 12_000;
    // Test fixtures choose an end at least 15 seconds in the past.
    let assessment = evidence.assess(completed);
    assert_eq!(assessment["runtime_verification"], "passed", "{assessment}");
    let mut body =
        json!({"completed_at_ms":completed,"runtime_evidence":evidence,"assessment":assessment});
    if let Some((pointer, value)) = change {
        *body.pointer_mut(pointer).unwrap() = value;
    }
    records::write(state, &saved, "result", body).await.unwrap();
}

pub(in crate::app) async fn seed_inputs(
    state: &AppState,
    deployment: &str,
    execution: &str,
    evidence: &FinanceRuntimeEvidence,
) {
    let saved = preparation::saved(state, deployment, execution)
        .await
        .unwrap();
    let created = evidence.window.start_unix_seconds as i64 * 1000 - 30_001;
    let window = records::WindowRecord::new(&saved, created).unwrap();
    window.validate(&saved).unwrap();
    assert_eq!(window.window, evidence.window);
    assert_eq!(window.expected, evidence.expected);
    records::write(state, &saved, "window", json!(window))
        .await
        .unwrap();
    for (step, value) in [
        ("before", &evidence.identity_before),
        ("probes", &evidence.functional_probes),
    ] {
        records::write(state, &saved, step, json!({"observation":value}))
            .await
            .unwrap();
    }
}

pub(in crate::app) async fn seed_window(
    state: &AppState,
    deployment: &str,
    execution: &str,
    created: i64,
    interrupted: Option<&str>,
) -> Value {
    let saved = preparation::saved(state, deployment, execution)
        .await
        .unwrap();
    let window = records::WindowRecord::new(&saved, created).unwrap();
    records::write(state, &saved, "window", json!(window))
        .await
        .unwrap();
    if let Some(step) = interrupted {
        records::begin(state, &saved, step).await.unwrap();
    }
    json!(window)
}
