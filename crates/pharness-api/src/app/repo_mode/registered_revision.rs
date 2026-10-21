use crate::app::identifiers::{is_git_sha, new_prefixed_id};
use crate::app::repository_readiness::ensure_repo_mode_enabled;
use crate::app::{ApiError, AppState};
use axum::extract::{Path, State};
use axum::Json;
use pharness_store::{AdvanceRepositoryRegisteredCommit, StoreError, StoredSourceDeliveryIntent};
use serde::Deserialize;
use serde_json::{json, Value};

const REGISTERED_REVISION_PROVENANCE_KIND: &str = "work_item_source_merge";

const CONTRACT_DIRECTORY: &str = ".pharness";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct AdvanceRegisteredRevisionRequest {
    pub source_delivery_intent_id: String,
    pub actor: String,
    pub reason: String,
}

/// Facts about one confirmed merge, as observed by the provider or recorded
/// in the intent's merge provenance.
pub(in crate::app) struct ConfirmedSourceMerge<'a> {
    pub merge_commit_sha: &'a str,
    pub merge_parent_shas: Option<&'a [String]>,
    pub actor: &'a str,
    pub reason: &'a str,
}

/// Advances the Repository's registered immutable revision to the merge
/// commit of a confirmed WorkItem source merge, carrying the validated
/// RepositoryContract forward.
///
/// The advance is withheld, never forced, unless the merge is proven to sit
/// exactly on the registered base and the merged change left `.pharness`
/// untouched; anything else still needs an onboarding refresh. The returned
/// document is durable provenance and always describes what happened.
pub(in crate::app) async fn advance_after_source_merge(
    state: &AppState,
    intent: &StoredSourceDeliveryIntent,
    merge: ConfirmedSourceMerge<'_>,
) -> Result<Value, ApiError> {
    if intent.subject_kind != "work_item_change_set" {
        return Ok(withheld(
            "only WorkItem source merges advance the registered revision",
        ));
    }
    if !is_git_sha(merge.merge_commit_sha) {
        return Ok(withheld("the merge commit is not a full Git object ID"));
    }
    let head_sha = intent
        .pull_request
        .as_ref()
        .and_then(|pull| pull.get("head_sha"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let exact_parents = merge.merge_parent_shas.is_some_and(|parents| {
        parents.len() == 2 && parents[0] == intent.base_commit && parents[1] == head_sha
    });
    if !exact_parents {
        return Ok(withheld(
            "the merge is not proven to join exactly the registered base and the approved head",
        ));
    }
    let change = state
        .store
        .get_change_set(&intent.subject_id)
        .await?
        .ok_or_else(|| ApiError::conflict("merged source ChangeSet is unavailable"))?;
    let Some(changed_paths) = change
        .change_set_json
        .pointer("/git_status/content/changed_paths")
        .and_then(Value::as_array)
        .map(|paths| {
            paths
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
    else {
        return Ok(withheld(
            "the merged ChangeSet records no changed paths, so the RepositoryContract cannot be carried forward",
        ));
    };
    if changed_paths.is_empty() {
        return Ok(withheld("the merged ChangeSet records an empty change"));
    }
    if changed_paths.iter().any(|path| touches_contract(path)) {
        return Ok(withheld(
            "the merged change touched .pharness; refresh onboarding to register the new revision",
        ));
    }
    let provenance = json!({
        "kind": REGISTERED_REVISION_PROVENANCE_KIND,
        "source_delivery_intent_id": intent.id,
        "work_item_id": change.work_item_id,
        "change_set_id": change.id,
        "change_set_material_hash": change.material_hash,
        "base_commit_sha": intent.base_commit,
        "head_commit_sha": head_sha,
        "merge_commit_sha": merge.merge_commit_sha,
        "changed_paths": changed_paths,
        "actor": merge.actor,
        "reason": merge.reason,
    });
    let advanced = state
        .store
        .advance_repository_registered_commit(AdvanceRepositoryRegisteredCommit {
            repository_id: intent.repository_id.clone(),
            base_commit: intent.base_commit.clone(),
            merge_commit: merge.merge_commit_sha.to_ascii_lowercase(),
            contract_version_id: new_prefixed_id("rcontract"),
            merge_provenance: provenance,
        })
        .await;
    match advanced {
        Ok(advanced) => {
            tracing::info!(
                repository_id = %advanced.repository.id,
                registered_commit = %advanced.repository.registered_commit,
                contract_version_id = %advanced.contract_version.id,
                source_delivery_intent_id = %intent.id,
                advanced = advanced.advanced,
                "registered revision follows the confirmed source merge"
            );
            Ok(json!({
                "status": if advanced.advanced { "advanced" } else { "already_current" },
                "registered_commit": advanced.repository.registered_commit,
                "repository_state_version": advanced.repository.state_version,
                "contract_version_id": advanced.contract_version.id,
                "contract_hash": advanced.contract_version.content_hash,
            }))
        }
        Err(StoreError::Conflict(reason)) => Ok(withheld(&reason)),
        Err(error) => Err(error.into()),
    }
}

/// Explicit operator recovery for a merge that was confirmed before the
/// controller advanced registered revisions automatically. The same proof is
/// required; the recorded provenance of the merged intent supplies it.
pub(in crate::app) async fn advance_repository_registered_revision(
    State(state): State<AppState>,
    Path(repository_id): Path<String>,
    Json(request): Json<AdvanceRegisteredRevisionRequest>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let actor = request.actor.trim();
    let reason = request.reason.trim();
    if actor.is_empty() || actor.len() > 200 || reason.is_empty() || reason.len() > 1_000 {
        return Err(ApiError::bad_request(
            "actor (1-200 characters) and reason (1-1000 characters) are required",
        ));
    }
    let repository = state
        .store
        .get_repository(&repository_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository", &repository_id))?;
    let intent = state
        .store
        .get_source_delivery_intent(&request.source_delivery_intent_id)
        .await?
        .filter(|intent| intent.repository_id == repository.id)
        .ok_or_else(|| {
            ApiError::not_found("source_delivery_intent", &request.source_delivery_intent_id)
        })?;
    if intent.status != "merged" {
        return Err(ApiError::conflict(
            "only a merged source delivery can advance the registered revision",
        ));
    }
    let provenance = intent.merge_provenance.clone().unwrap_or(Value::Null);
    if provenance["status"] != "succeeded" {
        return Err(ApiError::conflict(
            "the merged source delivery did not conclude with verified merge provenance",
        ));
    }
    let merge_commit_sha = provenance["merge_commit_sha"]
        .as_str()
        .ok_or_else(|| ApiError::conflict("merge provenance has no merge commit"))?;
    let parents = provenance["hosted_merge_proof"]["merge_parent_shas"]
        .as_array()
        .map(|parents| {
            parents
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        });
    let outcome = advance_after_source_merge(
        &state,
        &intent,
        ConfirmedSourceMerge {
            merge_commit_sha,
            merge_parent_shas: parents.as_deref(),
            actor,
            reason,
        },
    )
    .await?;
    if outcome["status"] == "withheld" {
        return Err(ApiError::conflict(format!(
            "registered revision was not advanced: {}",
            outcome["reason"].as_str().unwrap_or_default()
        )));
    }
    let repository = state
        .store
        .get_repository(&repository_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository", &repository_id))?;
    Ok(Json(json!({
        "repository": repository,
        "source_delivery_intent_id": intent.id,
        "registered_revision": outcome,
    })))
}

fn touches_contract(path: &str) -> bool {
    let path = path.trim_start_matches("./");
    path == CONTRACT_DIRECTORY
        || path
            .strip_prefix(CONTRACT_DIRECTORY)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn withheld(reason: &str) -> Value {
    json!({"status":"withheld","reason":reason})
}

#[cfg(test)]
mod tests {
    use super::touches_contract;

    #[test]
    fn contract_directory_detection_is_exact() {
        for path in [
            ".pharness",
            ".pharness/repository.yaml",
            ".pharness/instructions.md",
            "./.pharness/project.yaml",
        ] {
            assert!(touches_contract(path), "{path}");
        }
        for path in [
            "src/.pharness-like.py",
            "docs/.pharness/notes.md",
            ".pharnessx/file",
            "src/app.py",
        ] {
            assert!(!touches_contract(path), "{path}");
        }
    }
}
