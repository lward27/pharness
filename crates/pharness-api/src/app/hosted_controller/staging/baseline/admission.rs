use super::{now, preparation, records, ApiError, AppState};
use pharness_core::tools::{FinanceRuntimeEvidence, FinanceVerificationPhase};
use serde_json::{json, Value};

async fn accepted(
    state: &AppState,
    saved: &preparation::Saved,
) -> Result<Option<(records::WindowRecord, FinanceRuntimeEvidence, Value)>, ApiError> {
    let Some(window) = records::window(state, saved).await? else {
        return Ok(None);
    };
    let Some(record) = records::read(state, saved, "result").await? else {
        return Ok(None);
    };
    if record["assessment"]["runtime_verification"] != "passed" {
        return Ok(None);
    }
    let evidence: FinanceRuntimeEvidence =
        serde_json::from_value(record["runtime_evidence"].clone())
            .map_err(|_| ApiError::conflict("accepted baseline has invalid runtime evidence"))?;
    let completed = record["completed_at_ms"]
        .as_u64()
        .ok_or_else(|| ApiError::conflict("baseline completion time is missing"))?;
    if evidence.phase != FinanceVerificationPhase::Baseline
        || evidence.expected != window.expected
        || evidence.window != window.window
        || completed > now() as u64
        || completed < window.created_at_ms as u64
        || evidence.identity_before["started_at_unix_ms"]
            .as_u64()
            .unwrap_or_default()
            < window.created_at_ms as u64
        || record["assessment"] != evidence.assess(completed)
    {
        return Err(ApiError::conflict(
            "accepted baseline differs from its persisted window or assessment",
        ));
    }
    for (step, value) in [
        ("before", &evidence.identity_before),
        ("probes", &evidence.functional_probes),
        ("after", &evidence.identity_after),
    ] {
        let original = records::read(state, saved, step)
            .await?
            .ok_or_else(|| ApiError::conflict("baseline native receipt is missing"))?;
        if original["observation"] != *value {
            return Err(ApiError::conflict(
                "baseline result differs from its original native receipt",
            ));
        }
    }
    if evidence.assess(now() as u64)["runtime_verification"] != "passed" {
        return Ok(None);
    }
    Ok(Some((window, evidence, record)))
}

/// Read-only gate used by context GET and reconciliation. It performs no probes.
pub(in crate::app::hosted_controller::staging) async fn fresh(
    state: &AppState,
    saved: &preparation::Saved,
) -> Result<bool, ApiError> {
    Ok(records::read(state, saved, "admission_identity_started")
        .await?
        .is_none()
        && accepted(state, saved).await?.is_some())
}

pub(in crate::app::hosted_controller::staging) async fn revalidate(
    state: &AppState,
    saved: &preparation::Saved,
) -> Result<Value, ApiError> {
    let (window, evidence, record) = accepted(state, saved).await?.ok_or_else(|| {
        ApiError::conflict("staging admission requires a passed, fresh controller baseline")
    })?;
    if !records::begin(state, saved, "admission_identity").await? {
        return Err(ApiError::conflict("staging identity admission was already attempted; no new read or write allowance is granted"));
    }
    let observation = state
        .cluster_tools
        .observe_finance_deployment(&window.expected)
        .await
        .map_err(|_| ApiError::conflict("staging admission identity read was unavailable"))?
        .content;
    records::write(
        state,
        saved,
        "admission_identity",
        json!({"observation":observation}),
    )
    .await?;
    evidence
        .validate_current_deployment(&observation, now() as u64)
        .map_err(ApiError::conflict)?;
    preparation::validate_current(state, saved, true).await?;
    Ok(
        json!({"baseline_artifact_id":records::id(saved,"result"),"baseline_sha256":super::super::hash(&record)?,"identity_artifact_id":records::id(saved,"admission_identity"),"identity_sha256":super::super::hash(&observation)?,"admission_expires_at_ms":window.expires_at()}),
    )
}
