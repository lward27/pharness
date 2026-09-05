//! Legacy delivery routes cannot grant or advance hosted promotion authority.
use crate::app::{pipeline::hosted::is_hosted, ApiError};
use pharness_store::{SqliteStore, StoredGitOpsChangeSet};

pub(super) fn run_id(change: &StoredGitOpsChangeSet) -> Result<&pharness_core::RunId, ApiError> {
    change.run_id.as_ref().ok_or_else(|| {
        ApiError::conflict(
        "Legacy GitOps delivery requires coding-run provenance; hosted work uses workflow evidence",
    )
    })
}

pub(super) async fn ensure_legacy_mutation(
    store: &SqliteStore,
    change: &StoredGitOpsChangeSet,
) -> Result<(), ApiError> {
    let pipeline = store
        .get_pipeline_intent(&change.pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::conflict("GitOps pipeline lineage is unavailable"))?;
    if is_hosted(store, &pipeline).await? {
        return Err(ApiError::conflict(
            "Hosted GitOps changes use the saved workflow and production approval; legacy delivery actions are unavailable",
        ));
    }
    Ok(())
}
