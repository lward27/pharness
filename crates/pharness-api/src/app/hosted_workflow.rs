mod delivery;
pub(super) mod projection;
pub(super) mod stages;

use crate::app::deployment::contracts::{
    deployment_contract_spec, validate_deployment_contract_spec, VerificationRequirement,
};
use crate::app::hashing::canonical_material_hash;
use crate::app::pipeline::contracts::{pipeline_contract_spec, validate_pipeline_contract_spec};
use crate::app::{ApiError, AppState};
use pharness_core::hosted_sdlc::{
    HostedAutomaticAction, HostedRollbackPermission, HostedWorkflowPolicySnapshot,
    ProductionApprovalBoundary, HOSTED_WORKFLOW_SCHEMA,
};
use pharness_core::{InferencePolicyRef, InferenceStage, RunBudget};
use pharness_store::StoredRepository;
use serde_json::json;

/// Resolve only server-owned Product/Repository configuration. The caller's
/// preflight hash includes the entire result, so a policy edit requires a new
/// authorization instead of silently rebinding existing work.
pub(super) async fn resolve_policy(
    state: &AppState,
    product_id: &str,
    repository: &StoredRepository,
    builder_budget: &RunBudget,
    max_attempts: u32,
    planner_policy: Option<&InferencePolicyRef>,
) -> Result<Option<HostedWorkflowPolicySnapshot>, ApiError> {
    if !state.hosted_workflow.enabled {
        return Ok(None);
    }
    state
        .hosted_workflow
        .validate()
        .map_err(ApiError::conflict)?;
    if !state.inference.enabled || !state.repo_mode.coding_reliability_v2_enabled {
        return Err(ApiError::conflict(
            "hosted work requires the qualified gateway and Coding Reliability V2",
        ));
    }
    let binding = state
        .hosted_workflow
        .bindings
        .iter()
        .find(|binding| binding.product_id == product_id && binding.repository_id == repository.id)
        .ok_or_else(|| {
            ApiError::conflict("this Product/Repository has no hosted delivery binding")
        })?
        .clone();
    if binding.source_repo != repository.canonical_url
        || binding.source_ref != repository.default_branch
    {
        return Err(ApiError::conflict(
            "the hosted delivery binding no longer matches the registered repository",
        ));
    }
    if !state
        .worker
        .gitops_writer_settings()
        .is_some_and(|settings| settings.allowed_repos.contains(&binding.gitops_repo))
        || !state
            .worker
            .gitops_observer_settings()
            .is_some_and(|settings| settings.allowed_repos.contains(&binding.gitops_repo))
    {
        return Err(ApiError::conflict("the separately authorized GitOps writer and observer do not allow this delivery repository"));
    }
    let pipeline = state
        .store
        .get_pipeline_contract(&binding.pipeline_contract_id)
        .await?
        .ok_or_else(|| ApiError::conflict("the hosted PipelineContract is unavailable"))?;
    let staging = state
        .store
        .get_deployment_contract(&binding.staging.deployment_contract_id)
        .await?
        .ok_or_else(|| ApiError::conflict("the staging DeploymentContract is unavailable"))?;
    let production = state
        .store
        .get_deployment_contract(&binding.production.deployment_contract_id)
        .await?
        .ok_or_else(|| ApiError::conflict("the production DeploymentContract is unavailable"))?;
    let pipeline_spec = pipeline_contract_spec(&pipeline.contract_json)?;
    validate_pipeline_contract_spec(&pipeline_spec)?;
    if pipeline_spec.source_revision_param.as_deref() != Some("revision") {
        return Err(ApiError::conflict(
            "the hosted pipeline must require its exact source revision",
        ));
    }
    for contract in [&staging, &production] {
        let spec = deployment_contract_spec(&contract.contract_json)?;
        validate_deployment_contract_spec(&spec)?;
        if spec.workload_kind.as_deref() != Some("Deployment")
            || spec.workload_name.as_deref().map_or(true, str::is_empty)
            || spec.service_name.as_deref().map_or(true, str::is_empty)
            || spec.service_port.is_none()
            || spec.post_sync_verification.service_healthz != VerificationRequirement::Required
            || spec
                .health_path
                .as_deref()
                .map_or(true, |path| !path.starts_with('/'))
        {
            return Err(ApiError::conflict("hosted DeploymentContracts require an exact Deployment, Service, port, and required health probe"));
        }
    }
    let mut profiles = Vec::new();
    let mut selections = json!({"test":{"mode":"deterministic"}});
    for (stage_key, profile_id, stage) in [
        ("plan", "repo-planner", InferenceStage::Plan),
        ("implement", "repo-builder", InferenceStage::Implement),
        ("repair", "repo-repair", InferenceStage::Implement),
        (
            "test_diagnosis",
            "repo-test-diagnoser",
            InferenceStage::Test,
        ),
        ("verify", "repo-verifier", InferenceStage::Verify),
    ] {
        let (profile, selection) = qualified_stage(
            state,
            stage_key,
            profile_id,
            stage,
            if stage_key == "plan" {
                planner_policy
            } else {
                None
            },
        )
        .await?;
        profiles.push(profile);
        selections[stage_key] = selection;
    }
    let policy = HostedWorkflowPolicySnapshot {
        schema_version: HOSTED_WORKFLOW_SCHEMA.into(),
        delivery_binding_hash: canonical_material_hash(&json!(binding))?,
        rollback: if binding.rollback_permitted {
            HostedRollbackPermission::OnePreviousVerifiedDeployment
        } else {
            HostedRollbackPermission::Disabled
        },
        delivery_binding: binding,
        pipeline_contract: json!(pipeline),
        staging_contract: json!(staging),
        production_contract: json!(production),
        builder_budget: builder_budget.clone(),
        max_attempts,
        agent_profiles: profiles,
        inference_registry_hash: state.inference.registry.config_hash.clone(),
        stage_inference: selections,
        automatic_actions: HostedAutomaticAction::authorized_sequence(),
        production_approval: ProductionApprovalBoundary::BeforeGitopsMerge,
    };
    policy.validate().map_err(ApiError::conflict)?;
    delivery::validate_finance_coordinates(&policy)?;
    Ok(Some(policy))
}

/// Match a live stage with evidence from its exact frozen qualification suite.
/// Evaluation-specific tool bindings remain distinct from WorkItem bindings.
pub(super) async fn qualified_stage(
    state: &AppState,
    stage_key: &str,
    profile_id: &str,
    stage: InferenceStage,
    requested: Option<&InferencePolicyRef>,
) -> Result<(pharness_core::AgentProfile, serde_json::Value), ApiError> {
    let policy_ref = crate::app::inference::policy_reference(state, stage, profile_id, requested)?;
    let (suite_id, profile, qualification_binding) =
        crate::app::inference::qualification_binding_for_policy(state, &policy_ref)?;
    if profile.id != profile_id || !policy_ref.policy_id.ends_with("-v2") {
        return Err(ApiError::conflict(
            "hosted work requires the matching Coding Reliability V2 profile",
        ));
    }
    let mut selection =
        crate::app::inference::preview_selection(state, stage, &json!(profile), Some(&policy_ref))
            .await?;
    let suite_hash = pharness_core::inference_qualification_suite_hash(suite_id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    // Frozen fixtures and live WorkItems have different constrained tool
    // inputs. Compare qualification with its own exact suite binding, while
    // retaining the separately resolved WorkItem binding below.
    let qualification = state.store
        .list_inference_policy_qualifications(&policy_ref.policy_id, &policy_ref.revision).await?
        .into_iter().find(|qualification| qualification.agent_profile_id == profile_id)
        .filter(|qualification| qualification.verdict == "passed"
            && qualification.runtime_revision == state.build.api_revision
            && qualification.suite_id == suite_id
            && qualification.suite_hash == suite_hash
            && (!matches!(profile_id, "repo-builder" | "repo-repair") || qualification.attempts == 2)
            && selection["policy_hash"] == qualification.policy_hash
            && selection["target_hash"] == qualification.target_hash
            && qualification_binding.agent_profile_hash == qualification.agent_profile_hash)
        .ok_or_else(|| ApiError::conflict(format!("{stage_key} requires a passing gateway qualification for this runtime, frozen suite, and policy")))?;
    selection["qualification_profile_hash"] = json!(qualification.agent_profile_hash);
    selection["qualification_suite_hash"] = json!(qualification.suite_hash);
    selection["qualification_id"] = json!(qualification.id);
    selection["qualified_runtime_revision"] = json!(qualification.runtime_revision);
    Ok((profile, selection))
}
