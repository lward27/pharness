use super::super::identifiers::is_git_sha;
use super::super::work_items::preflight::work_item_target_supported;
use super::super::ApiError;
use super::change_sets::safe_relative_gitops_path;
use super::delivery_flow::gitops_delivery_flow;
use crate::dto::ArtifactResponse;
use pharness_store::{SqliteStore, StoredGitOpsChangeSet, StoredPipelineIntent, StoredWorkItem};
use serde_json::Value;

/// Return immutable GitOps merge evidence when the WorkItem declares a GitOps
/// source of truth. A missing target intentionally stays compatible with the
/// existing non-GitOps dev delivery path; a partially declared or unmerged
/// target blocks Argo execution.
pub(in crate::app) async fn observed_gitops_merge_for_deployment(
    store: &SqliteStore,
    work_item: &StoredWorkItem,
    pipeline_intent: &StoredPipelineIntent,
) -> Result<Option<ArtifactResponse>, ApiError> {
    let (gitops_repo, gitops_ref) = match (&work_item.gitops_repo, &work_item.gitops_ref) {
        (None, None) => return Ok(None),
        (Some(repository), Some(reference)) => (repository, reference),
        _ => {
            return Err(ApiError::conflict(
                "WorkItem must declare both gitops_repo and gitops_ref before Argo execution",
            ))
        }
    };
    let change_set = store
        .get_gitops_change_set_by_pipeline_intent(&pipeline_intent.id)
        .await?
        .ok_or_else(|| {
            ApiError::conflict(
                "Deployment requires a GitOps ChangeSet for the completed PipelineIntent",
            )
        })?;
    if change_set.status != "approved"
        || change_set.gitops_repo != *gitops_repo
        || change_set.gitops_ref != *gitops_ref
    {
        return Err(ApiError::conflict(
            "GitOps ChangeSet is not the current approved target declared by the WorkItem",
        ));
    }
    ensure_gitops_delivery_target(work_item, &change_set)?;
    let flow = gitops_delivery_flow(store, Some(&change_set)).await?;
    let merge = flow.and_then(|flow| flow.latest_merge).ok_or_else(|| {
        ApiError::conflict("Deployment requires an observed immutable GitOps pull-request merge")
    })?;
    if !merge
        .content_json
        .as_ref()
        .and_then(|content| content.get("merge_commit_sha"))
        .and_then(Value::as_str)
        .is_some_and(is_git_sha)
    {
        return Err(ApiError::conflict(
            "GitOps merge evidence has no valid immutable merge commit SHA",
        ));
    }
    Ok(Some(merge))
}

pub(in crate::app) fn ensure_gitops_delivery_target(
    work_item: &StoredWorkItem,
    change_set: &StoredGitOpsChangeSet,
) -> Result<(), ApiError> {
    if !work_item_target_supported(work_item) {
        return Err(ApiError::conflict(
            "GitOps delivery is limited to dev or the exact protected production target",
        ));
    }
    if work_item.gitops_repo.as_deref() != Some(change_set.gitops_repo.as_str())
        || work_item.gitops_ref.as_deref() != Some(change_set.gitops_ref.as_str())
        || !safe_relative_gitops_path(&change_set.kustomization_path)
        || !change_set.image_ref.contains("@sha256:")
    {
        return Err(ApiError::conflict(
            "GitOps ChangeSet no longer matches its declared WorkItem target or safety constraints",
        ));
    }
    Ok(())
}
