use super::stages::start_repo_builder;
use super::state::{repo_metadata, repo_work_item_state_hash};
use crate::app::clock::current_millis;
use crate::app::hosted_workflow::stages as hosted;
use crate::app::identifiers::new_prefixed_id;
use crate::app::{ApiError, AppState};
use pharness_store::{CreateStageChainAuthorization, CreateWorkspace};
use serde_json::{json, Value};

pub(in crate::app) async fn authorize_repo_stage_chain(
    state: &AppState,
    work_item_id: &str,
    actor: &str,
    reason: &str,
    reuse_workspace: Option<pharness_store::StoredWorkspace>,
    inference_policies: Option<&crate::dto::StageChainInferencePolicyRequest>,
    execution_policies: Option<&crate::dto::StageChainExecutionPolicyRequest>,
) -> Result<Value, ApiError> {
    let metadata = repo_metadata(state, work_item_id).await?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    if work_item.attempt_count >= work_item.max_attempts {
        return Err(ApiError::conflict(
            "Repo Mode WorkItem attempt limit is exhausted",
        ));
    }
    let plan = state
        .store
        .get_work_plan_by_work_item(work_item_id)
        .await?
        .filter(|plan| plan.status == "approved")
        .ok_or_else(|| ApiError::conflict("an approved WorkPlan is required"))?;
    let contract = work_item
        .repository_contract_json
        .clone()
        .ok_or_else(|| ApiError::conflict("RepositoryContract is unavailable"))?;
    let contract: pharness_core::RepositoryContract =
        serde_json::from_value(contract).map_err(|error| {
            ApiError::internal(format!("stored RepositoryContract is invalid: {error}"))
        })?;
    hosted::validate_runtime(state, &metadata)?;
    if metadata.workflow_policy.is_some() {
        if execution_policies.is_some() {
            return Err(ApiError::conflict("hosted stage chains use the recorded gateway; native execution overrides are unavailable"));
        }
        for (id, requested) in [
            (
                "repo-builder",
                inference_policies.and_then(|p| p.implement.as_ref()),
            ),
            (
                "repo-repair",
                inference_policies.and_then(|p| p.repair.as_ref()),
            ),
            (
                "repo-test-diagnoser",
                inference_policies
                    .and_then(|p| p.test_diagnosis.as_ref())
                    .or_else(|| inference_policies.and_then(|p| p.test.as_ref())),
            ),
            (
                "repo-verifier",
                inference_policies.and_then(|p| p.verify.as_ref()),
            ),
        ] {
            hosted::validate_preview(state, &metadata, id, requested).await?;
        }
    }
    let reusing_prepared_workspace = reuse_workspace.is_some();
    let workspace = if let Some(workspace) = reuse_workspace {
        if workspace.work_item_id != work_item_id
            || workspace.source_repo != work_item.source_repo
            || workspace.source_ref != work_item.source_ref
            || workspace.resolved_commit != work_item.source_commit
            || workspace.branch.is_none()
        {
            return Err(ApiError::conflict(
                "correction workspace no longer matches the pinned WorkItem source",
            ));
        }
        workspace
    } else {
        state
            .store
            .create_workspace(CreateWorkspace {
                id: new_prefixed_id("ws"),
                work_item_id: work_item_id.into(),
                run_id: None,
                status: "declared".into(),
                source_repo: work_item.source_repo.clone(),
                source_ref: work_item.source_ref.clone(),
                resolved_commit: work_item.source_commit.clone(),
                branch: Some(format!(
                    "pharness/{work_item_id}/attempt-{}",
                    work_item.attempt_count + 1
                )),
                retention_status: "retained".into(),
                actor: Some(actor.into()),
                reason: Some(reason.into()),
            })
            .await?
    };
    let model = state
        .worker
        .config_json()
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unconfigured")
        .to_string();
    let reliability_v2 = state.repo_mode.coding_reliability_v2_enabled;
    let profiles = metadata
        .workflow_policy
        .as_ref()
        .map(|policy| policy.agent_profiles.clone())
        .unwrap_or_else(|| state.compiled_agent_profiles(&model))
        .into_iter()
        .filter(|profile| {
            if reliability_v2 {
                matches!(
                    profile.id.as_str(),
                    "repo-builder" | "repo-repair" | "repo-test-diagnoser" | "repo-verifier"
                )
            } else {
                matches!(
                    profile.id.as_str(),
                    "repo-builder" | "repo-tester" | "repo-verifier"
                )
            }
        })
        .collect::<Vec<_>>();
    let expected_profiles = if reliability_v2 { 4 } else { 3 };
    if profiles.len() != expected_profiles {
        return Err(ApiError::internal(
            "compiled Repo Mode stage chain is incomplete",
        ));
    }
    let chain_state_hash = repo_work_item_state_hash(&metadata)?;
    let mut planned_execution = Vec::new();
    let mut execution_profiles = std::collections::BTreeSet::new();
    let mut requested_execution = vec![(
        "repo-builder",
        pharness_core::InferenceStage::Implement,
        execution_policies.and_then(|value| value.implement.as_ref()),
    )];
    if reliability_v2 {
        requested_execution.push((
            "repo-repair",
            pharness_core::InferenceStage::Repair,
            execution_policies.and_then(|value| value.repair.as_ref()),
        ));
    }
    requested_execution.push((
        "repo-verifier",
        pharness_core::InferenceStage::Verify,
        execution_policies.and_then(|value| value.verify.as_ref()),
    ));
    if metadata.workflow_policy.is_some() {
        requested_execution.clear();
    }
    for (profile_id, stage, requested) in requested_execution {
        if let Some(selection) = crate::app::agent_hosts::create_planned_execution_selection(
            state,
            crate::app::agent_hosts::PlannedExecutionSelectionRequest {
                subject_kind: "work_item",
                subject_id: work_item_id,
                stage_key: profile_id,
                stage,
                environment_profile_id: work_item
                    .environment_profile_id
                    .as_deref()
                    .ok_or_else(|| ApiError::conflict("EnvironmentProfile is unavailable"))?,
                requested,
                actor,
                reason,
                state_hash: &chain_state_hash,
            },
        )
        .await?
        {
            execution_profiles.insert(profile_id.to_string());
            planned_execution.push(selection);
        }
    }
    let mut planned_inference = Vec::new();
    if state.inference.enabled {
        let mut requested_stages = vec![(
            "repo-builder",
            pharness_core::InferenceStage::Implement,
            inference_policies.and_then(|value| value.implement.as_ref()),
        )];
        if reliability_v2 {
            requested_stages.push((
                "repo-repair",
                pharness_core::InferenceStage::Implement,
                inference_policies.and_then(|value| value.repair.as_ref()),
            ));
            let diagnosis = inference_policies
                .and_then(|value| value.test_diagnosis.as_ref())
                .or_else(|| inference_policies.and_then(|value| value.test.as_ref()));
            if diagnosis.is_some() || metadata.workflow_policy.is_some() {
                requested_stages.push((
                    "repo-test-diagnoser",
                    pharness_core::InferenceStage::Test,
                    diagnosis,
                ));
            }
        } else {
            requested_stages.insert(
                1,
                (
                    "repo-tester",
                    pharness_core::InferenceStage::Test,
                    inference_policies.and_then(|value| value.test.as_ref()),
                ),
            );
        }
        requested_stages.push((
            "repo-verifier",
            pharness_core::InferenceStage::Verify,
            inference_policies.and_then(|value| value.verify.as_ref()),
        ));
        for (profile_id, stage, requested) in requested_stages {
            if execution_profiles.contains(profile_id) {
                continue;
            }
            let profile = profiles
                .iter()
                .find(|profile| profile.id == profile_id)
                .ok_or_else(|| ApiError::internal("stage-chain AgentProfile is unavailable"))?;
            let pinned_reference = hosted::pinned_policy_ref(&metadata, profile_id)?;
            planned_inference.push(
                crate::app::inference::create_planned_selection(
                    state,
                    crate::app::inference::PlannedSelectionRequest {
                        subject_kind: "work_item",
                        subject_id: work_item_id,
                        stage,
                        profile: &serde_json::to_value(profile)
                            .map_err(|error| ApiError::internal(error.to_string()))?,
                        requested: pinned_reference.as_ref().or(requested),
                        actor,
                        reason,
                        state_hash: &chain_state_hash,
                    },
                )
                .await?,
            );
        }
    }
    let authorization = state
        .store
        .create_stage_chain_authorization(CreateStageChainAuthorization {
            id: new_prefixed_id("chain"),
            work_item_id: work_item_id.into(),
            work_plan_id: plan.id.clone(),
            work_plan_revision: plan.revision,
            product_model_snapshot_id: metadata.product_model_snapshot_id.clone(),
            product_model_snapshot_hash: metadata.product_model_snapshot_hash.clone(),
            repository_id: metadata.repository_id.clone(),
            source_commit: work_item
                .source_commit
                .clone()
                .ok_or_else(|| ApiError::conflict("source_commit is unavailable"))?,
            workspace_id: workspace.id.clone(),
            writable_paths: serde_json::to_value(&contract.writable_paths)
                .map_err(|error| ApiError::internal(error.to_string()))?,
            profile_chain: serde_json::to_value(&profiles)
                .map_err(|error| ApiError::internal(error.to_string()))?,
            budget_chain: json!({
                "coding_reliability_v2":reliability_v2,
                "deterministic_test":reliability_v2,
                "max_internal_corrections":if reliability_v2 {1} else {0},
                "internal_corrections_used":0,
                "repo-builder":work_item.run_budget,
                "repo-tester":profiles.iter().find(|profile| profile.id == "repo-tester").map(|profile| &profile.budget),
                "repo-repair":profiles.iter().find(|profile| profile.id == "repo-repair").map(|profile| &profile.budget),
                "repo-test-diagnoser":profiles.iter().find(|profile| profile.id == "repo-test-diagnoser").map(|profile| &profile.budget),
                "repo-verifier":profiles.iter().find(|profile| profile.id == "repo-verifier").map(|profile| &profile.budget),
                "requested_repair_policy":hosted::pinned_policy_ref(&metadata, "repo-repair")?.as_ref().or_else(|| inference_policies.and_then(|value| value.repair.as_ref())),
                "requested_test_diagnosis_policy":hosted::pinned_policy_ref(&metadata, "repo-test-diagnoser")?.as_ref().or_else(|| inference_policies.and_then(|value| value.test_diagnosis.as_ref()).or_else(|| inference_policies.and_then(|value| value.test.as_ref()))),
                "agent_execution_selections":planned_execution.iter().map(|selection| json!({
                    "selection_id":selection.id,
                    "stage_key":selection.stage_key,
                    "policy_id":selection.policy_id,
                    "policy_revision":selection.policy_revision,
                    "policy_hash":selection.policy_hash,
                    "binding_hash":selection.binding_hash,
                })).collect::<Vec<_>>(),
            }),
            state_hash: chain_state_hash,
            created_by: actor.into(),
            creation_reason: reason.into(),
            expires_at: (current_millis() + 4 * 60 * 60 * 1_000).to_string(),
        })
        .await?;
    match start_repo_builder(
        state,
        &metadata,
        &work_item,
        &plan,
        &workspace,
        &authorization,
        &contract,
        actor,
        reason,
        reusing_prepared_workspace,
        "repo-builder",
        None,
    )
    .await
    {
        Ok(started) => Ok(json!({
            "stage_chain_authorization":authorization,
            "workspace":workspace,
            "builder":started,
            "inference_selections":planned_inference,
            "agent_execution_selections":planned_execution,
        })),
        Err(error) => {
            state
                .store
                .revoke_stage_chain_authorization(
                    &authorization.id,
                    "Builder dispatch failed before the authorized chain started",
                )
                .await?;
            Err(error)
        }
    }
}
