use super::super::identifiers::is_git_sha;
use super::super::ApiError;
use crate::dto::GitOpsDeliveryFlowResponse;
use pharness_store::{SqliteStore, StoredArtifact, StoredGitOpsChangeSet};
use serde_json::Value;

pub(in crate::app) fn gitops_base_revision_matches_change_set(
    artifact: &StoredArtifact,
    change_set: &StoredGitOpsChangeSet,
) -> bool {
    artifact.kind == "gitops_base_revision"
        && artifact.content_json.as_ref().is_some_and(|content| {
            content.get("status").and_then(Value::as_str) == Some("resolved")
                && content.get("gitops_change_set_id").and_then(Value::as_str)
                    == Some(change_set.id.as_str())
                && content.get("material_hash").and_then(Value::as_str)
                    == Some(change_set.material_hash.as_str())
                && gitops_artifact_change_set_revision(content) == change_set.revision
                && content.get("repository").and_then(Value::as_str)
                    == Some(change_set.gitops_repo.as_str())
                && content.get("base_ref").and_then(Value::as_str)
                    == Some(change_set.gitops_ref.as_str())
                && content
                    .get("base_commit")
                    .and_then(Value::as_str)
                    .is_some_and(is_git_sha)
        })
}

pub(in crate::app) fn gitops_artifact_change_set_revision(content: &Value) -> i64 {
    content
        .get("gitops_change_set_revision")
        .and_then(Value::as_i64)
        .unwrap_or(1)
}

pub(in crate::app) fn gitops_delivery_plan_matches_change_set(
    artifact: &StoredArtifact,
    change_set: &StoredGitOpsChangeSet,
) -> bool {
    artifact.kind == "gitops_delivery_plan"
        && artifact.content_json.as_ref().is_some_and(|plan| {
            plan.get("gitops_change_set")
                .and_then(|value| value.get("id"))
                .and_then(Value::as_str)
                == Some(change_set.id.as_str())
                && plan
                    .get("gitops_change_set")
                    .and_then(|value| value.get("revision"))
                    .and_then(Value::as_i64)
                    == Some(change_set.revision)
                && plan
                    .get("gitops_change_set")
                    .and_then(|value| value.get("material_hash"))
                    .and_then(Value::as_str)
                    == Some(change_set.material_hash.as_str())
        })
}

pub(in crate::app) fn gitops_delivery_artifact_matches_plan(
    artifact: &StoredArtifact,
    kind: &str,
    plan_id: &str,
) -> bool {
    artifact.kind == kind
        && artifact.content_json.as_ref().is_some_and(|content| {
            content
                .get("gitops_delivery_plan_artifact_id")
                .and_then(Value::as_str)
                == Some(plan_id)
        })
}

pub(in crate::app) async fn gitops_delivery_flow(
    store: &SqliteStore,
    change_set: Option<&StoredGitOpsChangeSet>,
) -> Result<Option<GitOpsDeliveryFlowResponse>, ApiError> {
    let Some(change_set) = change_set else {
        return Ok(None);
    };
    let artifacts = store.list_artifacts(&change_set.run_id).await?;
    let Some(plan) = artifacts
        .iter()
        .find(|artifact| gitops_delivery_plan_matches_change_set(artifact, change_set))
    else {
        return Ok(None);
    };
    let base_revision_id = plan
        .content_json
        .as_ref()
        .and_then(|content| content.pointer("/source/base_revision_artifact_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ApiError::conflict("GitOps delivery plan has no base revision provenance")
        })?;
    let base_revision = artifacts
        .iter()
        .find(|artifact| {
            artifact.id == base_revision_id
                && gitops_base_revision_matches_change_set(artifact, change_set)
        })
        .cloned()
        .ok_or_else(|| {
            ApiError::conflict("GitOps delivery plan base revision is no longer current")
        })?;
    let latest_preflight = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "gitops_delivery_preflight"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content
                        .get("gitops_delivery_plan_artifact_id")
                        .and_then(Value::as_str)
                        == Some(plan.id.as_str())
                })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    let latest_execution = artifacts
        .iter()
        .filter(|artifact| {
            gitops_delivery_artifact_matches_plan(artifact, "gitops_delivery_execution", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    let latest_result = artifacts
        .iter()
        .filter(|artifact| {
            gitops_delivery_artifact_matches_plan(artifact, "gitops_delivery_result", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    let latest_observation = artifacts
        .iter()
        .filter(|artifact| {
            gitops_delivery_artifact_matches_plan(
                artifact,
                "gitops_delivery_pr_observation",
                &plan.id,
            )
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    let latest_merge = artifacts
        .iter()
        .filter(|artifact| {
            gitops_delivery_artifact_matches_plan(artifact, "gitops_delivery_merge", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    Ok(Some(GitOpsDeliveryFlowResponse {
        plan: plan.clone().into(),
        base_revision: base_revision.into(),
        latest_preflight,
        latest_execution,
        latest_result,
        latest_observation,
        latest_merge,
    }))
}
