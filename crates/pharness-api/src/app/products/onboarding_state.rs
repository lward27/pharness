use super::onboarding_policy::validate_onboarding_contract_compatibility;
use super::types::{RepositoryOnboardingActionResponse, RepositoryOnboardingResponse};
use crate::app::hashing::canonical_material_hash;
use crate::app::{ApiError, AppState};
use pharness_store::StoredRepositoryOnboarding;
use serde_json::{json, Value};

pub(super) async fn find_onboarding(
    state: &AppState,
    id: &str,
) -> Result<StoredRepositoryOnboarding, ApiError> {
    state
        .store
        .get_repository_onboarding(id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository_onboarding", id))
}

pub(super) fn onboarding_response(
    onboarding: StoredRepositoryOnboarding,
) -> Result<RepositoryOnboardingResponse, ApiError> {
    let state_hash = canonical_material_hash(&json!({
        "onboarding_id": onboarding.id,
        "state_version": onboarding.state_version,
        "status": onboarding.status,
        "registered_commit": onboarding.registered_commit,
        "resolved_commit": onboarding.resolved_commit,
        "current_discovery_id": onboarding.current_discovery_id,
        "current_proposal_revision": onboarding.current_proposal_revision,
        "approved_proposal_hash": onboarding.approved_proposal_hash,
        "source_delivery_intent_id": onboarding.source_delivery_intent_id,
        "contract_version_id": onboarding.contract_version_id,
        "readiness_assessment_id": onboarding.readiness_assessment_id,
        "proposer_run_id": onboarding.proposer_run_id,
        "proposer_profile_hash": onboarding.proposer_profile_hash,
        "proposer_stop_reason": onboarding.proposer_stop_reason,
        "patch_execution_id": onboarding.patch_execution_id,
        "patch_artifact_id": onboarding.patch_artifact_id,
        "patch_hash": onboarding.patch_hash,
        "validation_execution_id": onboarding.validation_execution_id,
        "validation_stop_reason": onboarding.validation_stop_reason,
    }))?;
    let action = match onboarding.status.as_str() {
        "registered" => Some(("start_discovery", Vec::new())),
        "discovery_failed" => Some(("retry_discovery", Vec::new())),
        "discovered" => Some(("start_proposer", Vec::new())),
        "proposal_failed" => Some(("retry_proposer", Vec::new())),
        "proposal_ready" => Some(("approve_proposal", Vec::new())),
        "proposal_approved" => Some(("prepare_onboarding_patch", Vec::new())),
        "patch_failed" => Some(("retry_onboarding_patch", Vec::new())),
        "delivery_ready" => Some(("authorize_onboarding_source_delivery", Vec::new())),
        "waiting_external" | "waiting_checks" | "waiting_merge" => {
            Some(("observe_onboarding_source_delivery", Vec::new()))
        }
        "merge_observed" => Some(("validate_merged_contract", Vec::new())),
        "validation_failed" => Some(("retry_merged_contract_validation", Vec::new())),
        _ => None,
    };
    let actions = action
        .into_iter()
        .map(|(id, blockers)| RepositoryOnboardingActionResponse {
            id: id.into(),
            lifecycle_stage: onboarding_stage_for_action(id).into(),
            resource: json!({
                "kind": "repository_onboarding",
                "id": onboarding.id,
                "product_id": onboarding.product_id,
                "repository_id": onboarding.repository_id,
                "source_commit": onboarding.registered_commit,
                "proposal_revision": onboarding.current_proposal_revision,
            }),
            status: if blockers.is_empty() {
                "available".into()
            } else {
                "blocked".into()
            },
            effect_class: if id == "approve_proposal" {
                "human_review".into()
            } else if matches!(id, "start_proposer" | "retry_proposer") {
                "model_execution".into()
            } else if matches!(id, "prepare_onboarding_patch" | "retry_onboarding_patch") {
                "isolated_source_materialization".into()
            } else if id == "authorize_onboarding_source_delivery" {
                "external_source_mutation".into()
            } else if id == "observe_onboarding_source_delivery" {
                "external_observation".into()
            } else {
                "isolated_read".into()
            },
            external_effect_summary: onboarding_action_effect(id, &onboarding),
            approval_requirements: if matches!(
                id,
                "approve_proposal" | "authorize_onboarding_source_delivery"
            ) {
                vec!["actor, reason, and current state hash".into()]
            } else {
                Vec::new()
            },
            expected_result: onboarding_action_result(id).into(),
            requires_confirmation: matches!(
                id,
                "approve_proposal"
                    | "start_proposer"
                    | "retry_proposer"
                    | "prepare_onboarding_patch"
                    | "retry_onboarding_patch"
                    | "authorize_onboarding_source_delivery"
                    | "observe_onboarding_source_delivery"
                    | "validate_merged_contract"
                    | "retry_merged_contract_validation"
            ),
            blockers,
            state_hash: state_hash.clone(),
        })
        .collect();
    Ok(RepositoryOnboardingResponse {
        id: onboarding.id,
        product_id: onboarding.product_id,
        repository_id: onboarding.repository_id,
        binding_id: onboarding.binding_id,
        onboarding_kind: onboarding.onboarding_kind,
        status: onboarding.status,
        registered_commit: onboarding.registered_commit,
        resolved_commit: onboarding.resolved_commit,
        current_discovery_id: onboarding.current_discovery_id,
        current_proposal_revision: onboarding.current_proposal_revision,
        approved_proposal_hash: onboarding.approved_proposal_hash,
        source_delivery_intent_id: onboarding.source_delivery_intent_id,
        contract_version_id: onboarding.contract_version_id,
        readiness_assessment_id: onboarding.readiness_assessment_id,
        proposer_run_id: onboarding.proposer_run_id,
        proposer_profile_hash: onboarding.proposer_profile_hash,
        proposer_stop_reason: onboarding.proposer_stop_reason,
        patch_execution_id: onboarding.patch_execution_id,
        patch_artifact_id: onboarding.patch_artifact_id,
        patch_hash: onboarding.patch_hash,
        validation_execution_id: onboarding.validation_execution_id,
        validation_stop_reason: onboarding.validation_stop_reason,
        state_version: onboarding.state_version,
        state_hash,
        blockers: onboarding.blockers,
        actions,
        created_at: onboarding.created_at,
        updated_at: onboarding.updated_at,
    })
}

pub(super) async fn onboarding_operator_response(
    state: &AppState,
    onboarding: StoredRepositoryOnboarding,
) -> Result<RepositoryOnboardingResponse, ApiError> {
    let mut response = onboarding_response(onboarding.clone())?;
    if onboarding.status != "proposal_ready" {
        return Ok(response);
    }
    if let Some(blocker) = onboarding_compatibility_blocker(state, &onboarding).await? {
        response.actions = vec![RepositoryOnboardingActionResponse {
            id: "refresh_onboarding".into(),
            lifecycle_stage: "proposal".into(),
            resource: json!({
                "kind":"repository_onboarding",
                "id":onboarding.id,
                "product_id":onboarding.product_id,
                "repository_id":onboarding.repository_id,
                "source_commit":onboarding.registered_commit,
            }),
            status: "blocked".into(),
            effect_class: "corrective_action".into(),
            external_effect_summary: "Register the prerequisite merge SHA and start a fresh onboarding; this immutable proposal remains historical evidence".into(),
            approval_requirements: Vec::new(),
            expected_result: "A fresh discovery and proposer Run use compatible EnvironmentProfile descriptors".into(),
            requires_confirmation: false,
            blockers: vec![blocker],
            state_hash: response.state_hash.clone(),
        }];
    }
    Ok(response)
}

async fn onboarding_compatibility_blocker(
    state: &AppState,
    onboarding: &StoredRepositoryOnboarding,
) -> Result<Option<String>, ApiError> {
    let Some(proposal) = state
        .store
        .get_current_repository_onboarding_proposal(&onboarding.id)
        .await?
    else {
        return Ok(Some("onboarding proposal is unavailable".into()));
    };
    let typed: pharness_core::RepositoryOnboardingProposal =
        match serde_json::from_value(proposal.proposal) {
            Ok(typed) => typed,
            Err(error) => {
                return Ok(Some(format!(
                    "stored onboarding proposal is invalid: {error}"
                )))
            }
        };
    let contract: pharness_core::RepositoryContract =
        match serde_json::from_value(typed.candidate_contract) {
            Ok(contract) => contract,
            Err(error) => {
                return Ok(Some(format!(
                    "stored candidate contract is invalid: {error}"
                )))
            }
        };
    let Some(discovery) = state
        .store
        .get_repository_discovery(&proposal.discovery_id)
        .await?
        .filter(|discovery| {
            discovery.status == "succeeded"
                && discovery.content_hash.as_deref() == Some(proposal.discovery_hash.as_str())
        })
    else {
        return Ok(Some("proposal discovery is unavailable".into()));
    };
    let Some(inventory) = discovery.inventory_json.as_ref() else {
        return Ok(Some("proposal discovery inventory is unavailable".into()));
    };
    let Some(repository) = state
        .store
        .get_repository(&onboarding.repository_id)
        .await?
    else {
        return Ok(Some("onboarding repository is unavailable".into()));
    };
    Ok(validate_onboarding_contract_compatibility(
        &state.environment_profiles,
        &repository.canonical_url,
        inventory,
        &contract,
    )
    .err()
    .map(|error| error.message))
}

pub(in crate::app) async fn onboarding_operator_projection(
    state: &AppState,
    onboarding: StoredRepositoryOnboarding,
) -> Result<Value, ApiError> {
    serde_json::to_value(onboarding_operator_response(state, onboarding).await?)
        .map_err(|error| ApiError::internal(error.to_string()))
}

fn onboarding_stage_for_action(action_id: &str) -> &'static str {
    match action_id {
        "start_discovery" | "retry_discovery" => "discovery",
        "start_proposer" | "retry_proposer" | "approve_proposal" => "proposal",
        "prepare_onboarding_patch" | "retry_onboarding_patch" => "contract_change",
        "authorize_onboarding_source_delivery" | "observe_onboarding_source_delivery" => {
            "source_delivery"
        }
        "validate_merged_contract" | "retry_merged_contract_validation" => "readiness",
        _ => "onboarding",
    }
}

fn onboarding_action_effect(action_id: &str, onboarding: &StoredRepositoryOnboarding) -> String {
    match action_id {
        "approve_proposal" => format!(
            "Approve onboarding proposal revision {} for Repository {}",
            onboarding.current_proposal_revision, onboarding.repository_id
        ),
        "prepare_onboarding_patch" | "retry_onboarding_patch" => format!(
            "Materialize the reviewed .pharness configuration change for Repository {} at {}",
            onboarding.repository_id, onboarding.registered_commit
        ),
        "authorize_onboarding_source_delivery" => format!(
            "Create one reviewed onboarding pull request for Repository {} at {}",
            onboarding.repository_id, onboarding.registered_commit
        ),
        "observe_onboarding_source_delivery" => format!(
            "Read pull-request and provider-check state for Repository {}",
            onboarding.repository_id
        ),
        "start_proposer" | "retry_proposer" => {
            "Run the bounded repository-onboarding proposer".into()
        }
        _ => "Advance the isolated onboarding lifecycle without changing provider state".into(),
    }
}

fn onboarding_action_result(action_id: &str) -> &'static str {
    match action_id {
        "start_discovery" | "retry_discovery" => "Immutable discovery evidence is recorded",
        "start_proposer" | "retry_proposer" => {
            "A versioned onboarding proposal is ready for review"
        }
        "approve_proposal" => {
            "The exact executable proposal is approved; Product topology suggestions remain a separate Product-model review"
        }
        "prepare_onboarding_patch" | "retry_onboarding_patch" => {
            "A bounded onboarding patch is materialized"
        }
        "authorize_onboarding_source_delivery" => {
            "The onboarding source-delivery intent is dispatched"
        }
        "observe_onboarding_source_delivery" => {
            "Provider checks and manual-merge state are refreshed"
        }
        "validate_merged_contract" | "retry_merged_contract_validation" => {
            "The merged canonical contract is validated"
        }
        _ => "The onboarding state advances",
    }
}
