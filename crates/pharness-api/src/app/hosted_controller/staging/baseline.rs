//! One controller-owned baseline window before a staging GitOps write.
use super::super::state::{condition, Condition};
use super::{artifact_id, evidence, now, preparation, ApiError, AppState};
use pharness_store::StoredWorkflowReconciliation;
use serde_json::json;

mod admission;
mod collection;
mod records;
#[cfg(test)]
pub(super) mod test_support;
pub(super) use admission::{fresh, historical, revalidate};

pub(super) async fn reconcile(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    saved: &preparation::Saved,
    expired: bool,
) -> Result<Option<Condition>, ApiError> {
    if saved.plan.is_none() {
        return Ok(None);
    }
    let window = match records::window(state, saved).await? {
        Some(record) => record,
        None => {
            if expired || claim.control != "active" {
                return Ok(Some(condition(if expired { "wait_expired" } else { &claim.control }, "A new staging baseline is stopped; no observation window or GitOps write was started.")));
            }
            preparation::validate_current(state, saved, true).await?;
            let record = records::WindowRecord::new(saved, now())?;
            record.validate(saved)?;
            records::write(state, saved, "window", json!(record)).await?;
            record
        }
    };
    // Recover an interruption between the immutable window and its operation
    // reference. This can only attach the same window; the store rejects rebinds.
    let mut refs = saved.operation.resource_refs.clone();
    refs["staging_baseline"] = json!({"window_artifact_id":records::id(saved,"window"),"result_artifact_id":records::id(saved,"result"),"window":window.window,"admission_expires_at_ms":window.expires_at()});
    if refs != saved.operation.resource_refs {
        state.store.record_workflow_operation(claim, &saved.operation.id, "running", &refs, "Original five-minute baseline window linked before native reads; no new execution allowance", now()).await?;
    }
    if records::read(state, saved, "result").await?.is_some() {
        if fresh(state, saved).await? {
            return Ok(None);
        }
        return Ok(Some(condition("blocked", "The original staging baseline failed, is inconclusive or expired. Its evidence is retained; no replacement window or GitOps write is authorized.")));
    }
    if expired || now() >= window.expires_at() {
        records::fail(state, saved, "baseline_original_window_expired").await?;
    } else if records::read(state, saved, "before").await?.is_none() {
        if now() >= window.window.start_unix_seconds as i64 * 1000 {
            records::fail(state, saved, "baseline_initial_identity_window_missed").await?;
        } else {
            collection::before(state, saved, &window).await?;
        }
    } else if now() < window.window.start_unix_seconds as i64 * 1000 {
        // The next persisted controller due time resumes this same window.
    } else if records::read(state, saved, "probes").await?.is_none() {
        if now() >= window.window.end_unix_seconds as i64 * 1000 {
            records::fail(state, saved, "baseline_functional_window_missed").await?;
        } else {
            collection::probes(state, saved, &window).await?;
        }
    } else if now() >= (window.window.end_unix_seconds as i64 + 10) * 1000 {
        // A 10-second ingestion allowance stays inside the existing 60-second
        // freshness ceiling. Reads take at most two consecutive 15-second rounds.
        collection::complete(state, saved, &window).await?;
    }
    if records::read(state, saved, "result").await?.is_some() {
        if fresh(state, saved).await? {
            return Ok(Some(condition("waiting", "The original staging baseline passed. One write still requires fresh deployment identity and current source, build, policy and control authorization.")));
        }
        return Ok(Some(condition("blocked", "The original staging baseline did not pass. Inspect its retained native observations; no replacement window or GitOps write was authorized.")));
    }
    Ok(Some(condition("waiting", "Collecting the original five-minute staging baseline. Pausing stops writes; read-only observation continues without renewing this window.")))
}
