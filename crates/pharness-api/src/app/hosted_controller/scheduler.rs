use super::state::{now, Snapshot};
use super::{progression, DISPATCH_BOUNDARY};
use crate::app::{
    ApiError, AppState, OperationalMode, CONTROLLER_WAIT_INTERVAL_MS, CONTROLLER_WAIT_MAX_CHECKS,
};
use pharness_store::FinishWorkflowReconciliation;
use std::time::Duration;

pub(in crate::app) fn spawn(state: AppState) {
    if OperationalMode::from_env() != OperationalMode::Normal {
        return;
    }
    // Creation may be disabled while already-authorized work still needs
    // observation or recovery. Only persisted hosted rows are eligible.
    let owner = format!("hosted-api-{}", uuid::Uuid::now_v7().simple());
    tokio::spawn(async move {
        loop {
            if let Err(error) = reconcile_once(&state, &owner).await {
                tracing::warn!(?error, "hosted reconciliation did not complete; persisted claim and operation retained");
            }
            tokio::time::sleep(Duration::from_millis(CONTROLLER_WAIT_INTERVAL_MS as u64)).await;
        }
    });
}

pub(in crate::app) async fn reconcile_once(
    state: &AppState,
    owner: &str,
) -> Result<bool, ApiError> {
    // Controls and dispatch share this boundary within the single writer.
    // Durable claims/operation identities, rather than this mutex, survive exit.
    let _boundary = DISPATCH_BOUNDARY.lock().await;
    let Some(claim) = state.store.claim_due_workflow(owner, now(), 60_000).await? else {
        return Ok(false);
    };
    let pass = async {
        let loaded = Snapshot::load(state, &claim.work_item_id).await;
        let hash = loaded.as_ref().ok().map(|snapshot| snapshot.hash.clone());
        let result = match loaded {
            Ok(snapshot) => {
                let expired = claim.unchanged_checks >= i64::from(CONTROLLER_WAIT_MAX_CHECKS)
                    && claim.observed_state_hash.as_deref() == Some(snapshot.hash.as_str());
                progression::advance(state, &claim, &snapshot, expired).await
            }
            Err(error) => Err(error),
        };
        let (condition, reason) = match result {
            Ok(result) => result,
            Err(error) => ("blocked".into(), error.message),
        };
        let time = now();
        state
            .store
            .finish_workflow_reconciliation(
                &claim,
                FinishWorkflowReconciliation {
                    next_due_at: time.saturating_add(CONTROLLER_WAIT_INTERVAL_MS as i64),
                    condition: &condition,
                    reason: &reason,
                    observed_state_hash: hash.as_deref(),
                },
                time,
            )
            .await?;
        Ok::<_, ApiError>(())
    };
    // A timeout does not release a possibly dispatched operation or mark it
    // failed. The next owner must reconcile the exact persisted identity.
    tokio::time::timeout(Duration::from_secs(45), pass)
        .await
        .map_err(|_| {
            ApiError::unavailable("hosted reconciliation timed out; recovery remains due")
        })??;
    Ok(true)
}
