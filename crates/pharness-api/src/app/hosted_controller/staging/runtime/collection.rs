use super::{
    now,
    records::{Context, Window},
    ApiError, AppState,
};
use pharness_core::tools::{
    FinanceRuntimeEvidence, FinanceVerificationPhase, ToolError, ToolResult,
};
use serde_json::{json, Value};

pub(super) fn content(result: Result<ToolResult, ToolError>) -> Value {
    result
        .map(|r| r.content)
        .unwrap_or_else(|_| json!({"collection_error":"native_read_unavailable"}))
}

pub(super) async fn collect(
    state: &AppState,
    context: &Context<'_>,
    window: &Window,
) -> Result<(), ApiError> {
    if now() >= window.expires_at() {
        return context.fail(state, "staging_original_window_expired").await;
    }
    if now() < window.window.start_unix_seconds as i64 * 1000 {
        return Ok(());
    }
    if context.read(state, "probes").await?.is_none() {
        if now() >= window.window.end_unix_seconds as i64 * 1000 {
            return context
                .fail(state, "staging_functional_window_missed")
                .await;
        }
        if !context.begin(state, "probes").await? {
            return context
                .fail(state, "staging_functional_collection_interrupted")
                .await;
        }
        let probes = content(
            state
                .cluster_tools
                .observe_finance_probes(&context.expected)
                .await,
        );
        return context
            .write(state, "probes", json!({"observation":probes}))
            .await;
    }
    if now() < (window.window.end_unix_seconds as i64 + 10) * 1000 {
        return Ok(());
    }
    if !context.begin(state, "complete").await? {
        return context
            .fail(state, "staging_final_collection_interrupted")
            .await;
    }
    let before = context
        .read(state, &window.initial_step)
        .await?
        .ok_or_else(|| ApiError::conflict("staging initial identity is missing"))?["observation"]
        .clone();
    let probes = context.read(state, "probes").await?.unwrap()["observation"].clone();
    let after = content(
        state
            .cluster_tools
            .observe_finance_deployment(&context.expected)
            .await,
    );
    context
        .write(state, "after", json!({"observation":after}))
        .await?;
    let (signals, trace) = tokio::join!(
        state.cluster_tools.observe_finance_signals(
            &context.expected,
            &window.window,
            &before,
            &after
        ),
        state.cluster_tools.observe_finance_health_trace(
            &context.expected,
            &window.window,
            &before,
            &after,
            &probes
        ),
    );
    let evidence = FinanceRuntimeEvidence {
        phase: FinanceVerificationPhase::Staging,
        expected: context.expected.clone(),
        window: window.window.clone(),
        identity_before: before,
        identity_after: after,
        functional_probes: probes,
        signals: content(signals),
        health_trace: content(trace),
    };
    let checked = now();
    let assessment = evidence.assess(checked as u64);
    context
        .write(
            state,
            "result",
            json!({"completed_at_ms":checked,"runtime_evidence":evidence,"assessment":assessment}),
        )
        .await
}
