use super::state::{condition, now, Condition};
use crate::app::hashing::canonical_material_hash;
use crate::app::identifiers::new_prefixed_id;
use crate::app::{ApiError, AppState, CONTROLLER_WAIT_INTERVAL_MS, CONTROLLER_WAIT_MAX_CHECKS};
use crate::dispatch::SourceJobKind;
use axum::extract::{Path, Query, State};
use axum::Json;
use pharness_core::hosted_sdlc::{
    HostedAutomaticAction, HostedSourceMergeAuthority, HOSTED_SOURCE_MERGE_SCHEMA,
};
use pharness_store::{
    StoredChangeSet, StoredSourceDeliveryIntent, StoredWorkflowOperation,
    StoredWorkflowReconciliation,
};
use serde::Deserialize;
use serde_json::{json, Value};

mod receipts;
pub(in crate::app) use receipts::{internal_source_merge_attempt, internal_source_merge_outcome};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct MergeQuery {
    pub execution_id: String,
}

pub(super) fn deadline(operation: &StoredWorkflowOperation) -> i64 {
    operation.created_at.saturating_add(
        (CONTROLLER_WAIT_INTERVAL_MS as i64).saturating_mul(i64::from(CONTROLLER_WAIT_MAX_CHECKS)),
    )
}

pub(super) fn receipt_id(execution_id: &str) -> String {
    format!("source_merge_receipt_{execution_id}")
}

/// This is independent provider observation, not the merge worker's receipt.
/// Legacy source-only observations keep their original completion contract.
pub(in crate::app) async fn observed_proof(
    state: &AppState,
    intent: &StoredSourceDeliveryIntent,
    request: &crate::dto::GitDeliveryObservationOutcomeRequest,
) -> Result<Option<Value>, ApiError> {
    if intent.subject_kind != "work_item_change_set" {
        return Ok(None);
    }
    let change = state
        .store
        .get_change_set(&intent.subject_id)
        .await?
        .ok_or_else(|| ApiError::conflict("observed source ChangeSet is unavailable"))?;
    let item_id = change
        .work_item_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("observed source WorkItem is unavailable"))?;
    let metadata = state
        .store
        .get_repo_work_item_metadata(item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("observed workflow metadata is unavailable"))?;
    if metadata.workflow_policy.is_none() {
        return Ok(None);
    }
    let Some(operation) = state
        .store
        .workflow_operation_for_source_intent(&intent.id)
        .await?
    else {
        return Ok(Some(
            json!({"accepted":false,"reason":"no recorded autonomous source merge operation"}),
        ));
    };
    let execution_id = operation.resource_refs["source_merge_authority"]["execution_id"]
        .as_str()
        .unwrap_or_default();
    let (_, _, authority, _) = match saved(state, &intent.id, execution_id, false).await {
        Ok(saved) => saved,
        Err(_) => {
            return Ok(Some(
                json!({"accepted":false,"reason":"recorded source merge authority no longer matches its evidence"}),
            ))
        }
    };
    if validate_binding(state, intent, &operation, &authority, false)
        .await
        .is_err()
    {
        return Ok(Some(
            json!({"accepted":false,"reason":"source or workflow changed after merge admission"}),
        ));
    }
    let attempt = state.store.get_artifact(&attempt_id(execution_id)).await?;
    let receipt = state.store.get_artifact(&receipt_id(execution_id)).await?;
    let expected_parents = vec![
        authority.base_commit_sha.clone(),
        authority.head_commit_sha.clone(),
    ];
    let accepted = attempt.as_ref().is_some_and(|record| {
        record.kind == "source_merge_attempt"
            && record
                .content_json
                .as_ref()
                .is_some_and(|material| material["authority"] == json!(authority))
    }) && request.merge_parent_shas.as_ref() == Some(&expected_parents)
        && request
            .merge_tree_sha
            .as_deref()
            .is_some_and(crate::app::identifiers::is_git_sha);
    Ok(Some(json!({"accepted":accepted,"authority":authority,
        "authority_hash":operation.resource_refs["source_merge_authority_hash"],
        "attempt_id":attempt.as_ref().map(|a| &a.id),"attempt_hash":attempt.as_ref().map(|a| &a.content_hash),
        "worker_receipt_id":receipt.as_ref().map(|a| &a.id),"worker_receipt_hash":receipt.as_ref().map(|a| &a.content_hash),
        "merge_parent_shas":request.merge_parent_shas,"merge_tree_sha":request.merge_tree_sha,
        "acknowledgement":if receipt.as_ref().and_then(|a| a.content_json.as_ref()).is_some_and(|v| v["outcome"]["status"] == "merged" && v["outcome"]["origin"] == "api_acknowledged" && v["outcome"]["merge_commit_sha"] == json!(request.merge_commit_sha)) {"worker_receipt"} else {"recovered_by_provider_observation"},
        "reason":if accepted {"provider merge matches the admitted exact source and base"} else {"merge lacks an admitted attempt or has incompatible source ancestry"},
    })))
}

fn attempt_id(execution_id: &str) -> String {
    format!("source_merge_attempt_{execution_id}")
}

pub(super) async fn reconcile(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    operation: &StoredWorkflowOperation,
    intent: &StoredSourceDeliveryIntent,
) -> Result<Option<Condition>, ApiError> {
    let mut operation = operation.clone();
    if operation
        .resource_refs
        .get("source_merge_authority")
        .is_none()
    {
        if intent.status != "waiting_merge"
            || claim.control != "active"
            || now() >= deadline(&operation)
        {
            return Ok(None);
        }
        let change = state
            .store
            .get_change_set(&intent.subject_id)
            .await?
            .ok_or_else(|| ApiError::conflict("source merge has no approved ChangeSet"))?;
        let pull = intent
            .pull_request
            .as_ref()
            .ok_or_else(|| ApiError::conflict("source merge has no recorded pull request"))?;
        let authority = HostedSourceMergeAuthority {
            schema_version: HOSTED_SOURCE_MERGE_SCHEMA.into(),
            operation_id: operation.id.clone(),
            execution_id: new_prefixed_id("srcmerge"),
            work_item_id: claim.work_item_id.clone(),
            source_delivery_intent_id: intent.id.clone(),
            workflow_policy_hash: intent.authorization["workflow_policy_hash"]
                .as_str()
                .unwrap_or_default()
                .into(),
            change_set_material_hash: change.material_hash,
            repository: intent.source_repo.clone(),
            base_ref: intent.base_ref.clone(),
            base_commit_sha: intent.base_commit.clone(),
            head_branch: intent.head_branch.clone(),
            head_commit_sha: pull["head_sha"].as_str().unwrap_or_default().into(),
            pull_request_number: pull["number"].as_u64().unwrap_or_default(),
            pull_request_url: pull["url"].as_str().unwrap_or_default().into(),
            required_check_context: "Source integrity".into(),
            required_check_app_id: 15368,
            expires_at_ms: deadline(&operation),
        };
        authority.validate(now()).map_err(ApiError::conflict)?;
        validate_binding(state, intent, &operation, &authority, true).await?;
        let mut refs = operation.resource_refs.clone();
        refs["source_merge_authority_hash"] =
            json!(authority.material_hash().map_err(ApiError::conflict)?);
        refs["source_merge_authority"] = json!(authority);
        operation = state.store.record_workflow_operation(claim, &operation.id, "running", &refs,
            "Exact source merge authority recorded before dispatch; publication authority remains unchanged", now()).await?;
    }
    let authority: HostedSourceMergeAuthority =
        serde_json::from_value(operation.resource_refs["source_merge_authority"].clone())
            .map_err(|_| ApiError::conflict("recorded source merge authority is invalid"))?;
    // A receipt or an admitted attempt can only lead to read-only observation.
    // Never recreate the merge Job after the write boundary was admitted.
    if state
        .store
        .get_artifact(&receipt_id(&authority.execution_id))
        .await?
        .is_some()
    {
        return Ok(None);
    }
    let attempted = state
        .store
        .get_artifact(&attempt_id(&authority.execution_id))
        .await?
        .is_some();
    let recover = !attempted && claim.control == "active" && now() < authority.expires_at_ms;
    let observed = state
        .worker
        .reconcile_source_delivery_job(
            &intent.id,
            &authority.execution_id,
            SourceJobKind::Merge,
            recover,
        )
        .await
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    if attempted && matches!(observed.status, "missing" | "failed" | "succeeded") {
        return Ok(None);
    }
    if matches!(observed.status, "failed" | "succeeded") {
        return Ok(Some(condition("blocked", "The source merge worker terminated before an attempt or receipt was recorded. Its identity is retained; it will not be rerun.")));
    }
    Ok(Some(condition(
        if now() >= authority.expires_at_ms {
            "wait_expired"
        } else if claim.control != "active" {
            &claim.control
        } else {
            "waiting"
        },
        format!(
            "Source merge Job {} is {} under its original bounded authority.",
            observed.job_name, observed.status
        ),
    )))
}

pub(in crate::app) async fn internal_source_merge_context(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Query(query): Query<MergeQuery>,
) -> Result<Json<Value>, ApiError> {
    let (_, _, authority, _) = saved(&state, &intent_id, &query.execution_id, true).await?;
    let settings = state
        .worker
        .git_writer_settings()
        .ok_or_else(|| ApiError::conflict("source merge executor is unavailable"))?;
    if settings.github_api_url != "https://api.github.com"
        || !settings.allowed_repos.contains(&authority.repository)
    {
        return Err(ApiError::conflict(
            "source merge repository or provider is not writer-allowlisted",
        ));
    }
    Ok(Json(
        json!({"authority_hash":authority.material_hash().map_err(ApiError::conflict)?,"authority":authority,"github_api_url":settings.github_api_url}),
    ))
}

async fn saved(
    state: &AppState,
    intent_id: &str,
    execution_id: &str,
    for_write: bool,
) -> Result<
    (
        StoredSourceDeliveryIntent,
        StoredWorkflowOperation,
        HostedSourceMergeAuthority,
        StoredChangeSet,
    ),
    ApiError,
> {
    let intent = state
        .store
        .get_source_delivery_intent(intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("source_delivery_intent", intent_id))?;
    let operation = state
        .store
        .workflow_operation_for_source_intent(intent_id)
        .await?
        .ok_or_else(|| ApiError::conflict("source merge operation is not recorded"))?;
    let authority: HostedSourceMergeAuthority =
        serde_json::from_value(operation.resource_refs["source_merge_authority"].clone())
            .map_err(|_| ApiError::conflict("source merge authority is not recorded"))?;
    if authority.execution_id != execution_id
        || operation.resource_refs["source_merge_authority_hash"]
            != json!(authority.material_hash().map_err(ApiError::conflict)?)
    {
        return Err(ApiError::conflict(
            "source merge execution or authority hash changed",
        ));
    }
    // Structural validation remains possible after expiry; it grants no write.
    authority
        .validate(if for_write {
            now()
        } else {
            authority.expires_at_ms.saturating_sub(1)
        })
        .map_err(ApiError::conflict)?;
    let change = if for_write {
        validate_binding(state, &intent, &operation, &authority, true).await?
    } else {
        let change = state
            .store
            .get_change_set(&intent.subject_id)
            .await?
            .ok_or_else(|| ApiError::conflict("recorded source ChangeSet is unavailable"))?;
        if operation.id != authority.operation_id
            || operation.work_item_id != authority.work_item_id
            || operation.action != "authorize_source_delivery"
            || operation.resource_refs["action_resource"] != intent.subject_id
            || authority.source_delivery_intent_id != intent.id
            || intent.subject_kind != "work_item_change_set"
            || change.work_item_id.as_deref() != Some(&authority.work_item_id)
            || authority.expires_at_ms != deadline(&operation)
        {
            return Err(ApiError::conflict(
                "source merge receipt is outside its recorded operation",
            ));
        }
        change
    };
    Ok((intent, operation, authority, change))
}

async fn validate_binding(
    state: &AppState,
    intent: &StoredSourceDeliveryIntent,
    operation: &StoredWorkflowOperation,
    authority: &HostedSourceMergeAuthority,
    for_write: bool,
) -> Result<StoredChangeSet, ApiError> {
    let item = state
        .store
        .get_work_item(&authority.work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("source merge WorkItem is unavailable"))?;
    let metadata = state
        .store
        .get_repo_work_item_metadata(&item.id)
        .await?
        .ok_or_else(|| ApiError::conflict("source merge workflow metadata is unavailable"))?;
    let policy = metadata
        .workflow_policy
        .as_ref()
        .ok_or_else(|| ApiError::conflict("source-only work has no autonomous merge authority"))?;
    policy.validate().map_err(ApiError::conflict)?;
    let change = state
        .store
        .get_change_set(&intent.subject_id)
        .await?
        .ok_or_else(|| ApiError::conflict("source merge ChangeSet is unavailable"))?;
    let pull = intent
        .pull_request
        .as_ref()
        .ok_or_else(|| ApiError::conflict("source merge pull request is unavailable"))?;
    if operation.id != authority.operation_id
        || operation.work_item_id != item.id
        || operation.action != "authorize_source_delivery"
        || operation.resource_refs["action_resource"] != intent.subject_id
        || authority.source_delivery_intent_id != intent.id
        || intent.subject_kind != "work_item_change_set"
        || change.work_item_id.as_deref() != Some(&item.id)
        || intent.authorization["work_item_id"] != item.id
        || metadata.workflow_policy_hash.as_deref() != Some(&authority.workflow_policy_hash)
        || canonical_material_hash(&json!(policy))? != authority.workflow_policy_hash
        || intent.authorization["workflow_policy_hash"] != authority.workflow_policy_hash
        || metadata.repository_id != intent.repository_id
        || policy.delivery_binding.source_repo != authority.repository
        || intent.source_repo != authority.repository
        || item.source_repo != authority.repository
        || item.source_commit.as_deref() != Some(&authority.base_commit_sha)
        || intent.base_commit != authority.base_commit_sha
        || intent.base_ref != authority.base_ref
        || intent.head_branch != authority.head_branch
        || pull["head_branch"] != authority.head_branch
        || pull["head_sha"] != authority.head_commit_sha
        || pull["url"] != authority.pull_request_url
        || pull["number"] != authority.pull_request_number
        || change.material_hash != authority.change_set_material_hash
        || intent.patch_hash
            != change.change_set_json["patch"]["hash"]
                .as_str()
                .unwrap_or_default()
        || intent.patch_artifact_id.as_deref()
            != change.change_set_json["patch"]["artifact_id"].as_str()
        || authority.expires_at_ms != deadline(operation)
        || !policy
            .automatic_actions
            .contains(&HostedAutomaticAction::SourceDelivery)
    {
        return Err(ApiError::conflict(
            "source merge no longer matches its recorded source and workflow authority",
        ));
    }
    if for_write {
        if crate::app::OperationalMode::from_env() != crate::app::OperationalMode::Normal {
            return Err(ApiError::conflict(
                "source merge is withheld while PHarness is draining or read-only",
            ));
        }
        let control = state
            .store
            .get_workflow_reconciliation(&item.id)
            .await?
            .ok_or_else(|| ApiError::conflict("source merge controller state is unavailable"))?;
        if control.control != "active"
            || operation.status != "running"
            || metadata.closed_at.is_some()
            || intent.status != "waiting_merge"
            || change.status != "approved"
        {
            return Err(ApiError::conflict(
                "source merge is paused, stale, closed or not ready",
            ));
        }
        super::approval::validate(state, &item.id, "approve_change_set", &change.id).await?;
        let checks = state
            .store
            .latest_provider_check_set_observation(&intent.id, "pre_merge")
            .await?
            .ok_or_else(|| ApiError::conflict("source merge has no pre-merge check evidence"))?;
        if !checks.authoritative_rules_succeeded
            || checks.status != "passing"
            || !checks.required_checks.as_array().is_some_and(|items| {
                items.iter().any(|item| {
                    item["name"] == authority.required_check_context
                        && item["app_id"].as_u64() == Some(authority.required_check_app_id)
                })
            })
            || checks.head_sha != authority.head_commit_sha
            || checks.required_set_hash != canonical_material_hash(&checks.required_checks)?
            || !checks
                .expires_at
                .parse::<i64>()
                .is_ok_and(|expiry| expiry > now())
        {
            return Err(ApiError::conflict(
                "source merge requires fresh checks for the exact approved head",
            ));
        }
    }
    Ok(change)
}
