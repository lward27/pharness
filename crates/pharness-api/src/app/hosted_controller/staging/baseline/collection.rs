use super::{now, preparation, records, ApiError, AppState};
use pharness_core::tools::{
    FinanceRuntimeEvidence, FinanceVerificationPhase, ToolError, ToolResult,
};
use serde_json::{json, Value};

fn content(result: Result<ToolResult, ToolError>) -> Value {
    result
        .map(|result| result.content)
        .unwrap_or_else(|_| json!({"collection_error":"native_read_unavailable"}))
}

pub(super) async fn before(
    state: &AppState,
    saved: &preparation::Saved,
    window: &records::WindowRecord,
) -> Result<(), ApiError> {
    if !records::begin(state, saved, "before").await? {
        return records::fail(state, saved, "baseline_identity_collection_interrupted").await;
    }
    let value = content(
        state
            .cluster_tools
            .observe_finance_deployment(&window.expected)
            .await,
    );
    records::write(state, saved, "before", json!({"observation":value})).await?;
    if value["identity_state"] != "verified"
        || value["completed_at_unix_ms"].as_u64().unwrap_or(u64::MAX)
            > window.window.start_unix_seconds * 1000
    {
        records::fail(
            state,
            saved,
            "baseline_initial_deployment_identity_unverified",
        )
        .await?;
    }
    Ok(())
}

pub(super) async fn probes(
    state: &AppState,
    saved: &preparation::Saved,
    window: &records::WindowRecord,
) -> Result<(), ApiError> {
    if !records::begin(state, saved, "probes").await? {
        return records::fail(state, saved, "baseline_functional_collection_interrupted").await;
    }
    let value = content(
        state
            .cluster_tools
            .observe_finance_probes(&window.expected)
            .await,
    );
    records::write(state, saved, "probes", json!({"observation":value})).await
}

pub(super) async fn complete(
    state: &AppState,
    saved: &preparation::Saved,
    window: &records::WindowRecord,
) -> Result<(), ApiError> {
    if !records::begin(state, saved, "complete").await? {
        return records::fail(state, saved, "baseline_final_collection_interrupted").await;
    }
    let before = records::read(state, saved, "before")
        .await?
        .ok_or_else(|| ApiError::conflict("baseline initial observation is missing"))?
        ["observation"]
        .clone();
    let probes = records::read(state, saved, "probes")
        .await?
        .ok_or_else(|| ApiError::conflict("baseline functional observation is missing"))?
        ["observation"]
        .clone();
    let after = content(
        state
            .cluster_tools
            .observe_finance_deployment(&window.expected)
            .await,
    );
    records::write(state, saved, "after", json!({"observation":after})).await?;
    let (signals, trace) = tokio::join!(
        state.cluster_tools.observe_finance_signals(
            &window.expected,
            &window.window,
            &before,
            &after
        ),
        state.cluster_tools.observe_finance_health_trace(
            &window.expected,
            &window.window,
            &before,
            &after,
            &probes
        ),
    );
    let evidence = FinanceRuntimeEvidence {
        phase: FinanceVerificationPhase::Baseline,
        expected: window.expected.clone(),
        window: window.window.clone(),
        identity_before: before,
        identity_after: after,
        functional_probes: probes,
        signals: content(signals),
        health_trace: content(trace),
    };
    let checked = now();
    let assessment = evidence.assess(checked as u64);
    records::write(
        state,
        saved,
        "result",
        json!({"completed_at_ms":checked,"runtime_evidence":evidence,"assessment":assessment}),
    )
    .await
}
