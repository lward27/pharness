use super::{now, preparation, records, ApiError, AppState};
use pharness_core::tools::{FinanceRuntimeEvidence, FinanceVerificationPhase};
use serde_json::{json, Value};

async fn accepted(
    state: &AppState,
    saved: &preparation::Saved,
    at_ms: u64,
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
        || completed > at_ms
        || at_ms > now() as u64
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
    if evidence.assess(at_ms)["runtime_verification"] != "passed" {
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
        && accepted(state, saved, now() as u64).await?.is_some())
}

pub(in crate::app::hosted_controller::staging) async fn revalidate(
    state: &AppState,
    saved: &preparation::Saved,
) -> Result<Value, ApiError> {
    let (window, evidence, record) =
        accepted(state, saved, now() as u64).await?.ok_or_else(|| {
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

/// Validate the original write's evidence at admission time. Historical proof
/// remains inspectable after its window expires; it grants no new write authority.
pub(in crate::app::hosted_controller::staging) async fn historical(
    state: &AppState,
    saved: &preparation::Saved,
) -> Result<Value, ApiError> {
    let id = super::artifact_id("attempt", &saved.authority.execution_id);
    let attempt = state
        .store
        .get_artifact(&id)
        .await?
        .ok_or_else(|| ApiError::conflict("staging runtime has no original admission"))?;
    let body = attempt
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("staging admission has no content"))?;
    let time = body["admitted_at_ms"]
        .as_i64()
        .ok_or_else(|| ApiError::conflict("staging admission time is missing"))?;
    if attempt.kind != "hosted_staging_admission"
        || attempt.session_id != saved.pipeline.session_id
        || attempt.run_id != saved.pipeline.run_id
        || body["execution_id"] != saved.authority.execution_id
        || body["operation_id"] != saved.operation.id
        || body["authority_hash"]
            != saved
                .authority
                .material_hash()
                .map_err(ApiError::conflict)?
        || body["plan_hash"]
            != saved
                .plan
                .as_ref()
                .ok_or_else(|| ApiError::conflict("staging admission plan is missing"))?
                .material_hash()
                .map_err(ApiError::conflict)?
        || time < saved.authority.created_at_ms
        || time >= saved.authority.expires_at_ms
        || time >= saved.operation.created_at.saturating_add(30 * 60 * 1000)
    {
        return Err(ApiError::conflict(
            "staging admission has different native authority or time bounds",
        ));
    }
    let (window, evidence, baseline) =
        accepted(state, saved, time as u64).await?.ok_or_else(|| {
            ApiError::conflict(
                "staging write has no passed baseline at its original admission time",
            )
        })?;
    let identity = records::read(state, saved, "admission_identity")
        .await?
        .ok_or_else(|| ApiError::conflict("staging admission identity is missing"))?;
    evidence
        .validate_current_deployment(&identity["observation"], time as u64)
        .map_err(ApiError::conflict)?;
    let expected = json!({"baseline_artifact_id":records::id(saved,"result"),"baseline_sha256":super::super::hash(&baseline)?,"identity_artifact_id":records::id(saved,"admission_identity"),"identity_sha256":super::super::hash(&identity["observation"])?,"admission_expires_at_ms":window.expires_at()});
    if body["baseline"] != expected || time >= window.expires_at() {
        return Err(ApiError::conflict(
            "staging admission differs from its original baseline and identity receipts",
        ));
    }
    Ok(
        json!({"admission_artifact_id":id,"admission_artifact_sha256":super::super::hash(body)?,"baseline":expected}),
    )
}
