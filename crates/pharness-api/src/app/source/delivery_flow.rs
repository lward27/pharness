use super::super::ApiError;
use crate::dto::GitDeliveryFlowResponse;
use pharness_store::{SqliteStore, StoredArtifact, StoredChangeSet};
use serde_json::Value;

pub(in crate::app) async fn git_delivery_flow(
    store: &SqliteStore,
    change_set: Option<&StoredChangeSet>,
) -> Result<Option<GitDeliveryFlowResponse>, ApiError> {
    let Some(change_set) = change_set else {
        return Ok(None);
    };
    let Some(run_id) = &change_set.run_id else {
        return Ok(None);
    };
    let artifacts = store.list_artifacts(run_id).await?;
    let Some(plan) = artifacts
        .iter()
        .find(|artifact| git_delivery_plan_matches_change_set(artifact, change_set))
    else {
        return Ok(None);
    };
    let latest_preflight = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "git_delivery_preflight"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content
                        .get("git_delivery_plan_artifact_id")
                        .and_then(Value::as_str)
                        == Some(plan.id.as_str())
                })
        })
        .max_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        })
        .cloned()
        .map(Into::into);
    let latest_execution = artifacts
        .iter()
        .filter(|artifact| {
            git_delivery_artifact_matches_plan(artifact, "git_delivery_execution", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    let latest_result = artifacts
        .iter()
        .filter(|artifact| {
            git_delivery_artifact_matches_plan(artifact, "git_delivery_result", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    let latest_observation = artifacts
        .iter()
        .filter(|artifact| {
            git_delivery_artifact_matches_plan(artifact, "git_delivery_pr_observation", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    let latest_merge = artifacts
        .iter()
        .filter(|artifact| {
            git_delivery_artifact_matches_plan(artifact, "git_delivery_merge", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);

    Ok(Some(GitDeliveryFlowResponse {
        plan: plan.clone().into(),
        latest_preflight,
        latest_execution,
        latest_result,
        latest_observation,
        latest_merge,
    }))
}

pub(in crate::app) fn git_delivery_artifact_matches_plan(
    artifact: &StoredArtifact,
    kind: &str,
    plan_id: &str,
) -> bool {
    artifact.kind == kind
        && artifact.content_json.as_ref().is_some_and(|content| {
            content
                .get("git_delivery_plan_artifact_id")
                .and_then(Value::as_str)
                == Some(plan_id)
        })
}

pub(in crate::app) fn git_delivery_plan_matches_change_set(
    artifact: &StoredArtifact,
    change_set: &StoredChangeSet,
) -> bool {
    artifact.kind == "git_delivery_plan"
        && artifact.content_json.as_ref().is_some_and(|plan| {
            plan.get("change_set")
                .and_then(|value| value.get("id"))
                .and_then(Value::as_str)
                == Some(change_set.id.as_str())
                && plan
                    .get("change_set")
                    .and_then(|value| value.get("revision"))
                    .and_then(Value::as_i64)
                    == Some(change_set.revision)
                && plan
                    .get("change_set")
                    .and_then(|value| value.get("material_hash"))
                    .and_then(Value::as_str)
                    == Some(change_set.material_hash.as_str())
        })
}
