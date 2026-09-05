use super::state::{repo_metadata, repo_work_item_state_hash};
use crate::app::hashing::canonical_material_hash;
use crate::app::identifiers::{is_git_sha, new_prefixed_id};
use crate::app::repository_readiness::{current_readiness_mismatches, ensure_repo_mode_enabled};
use crate::app::validation::required_text;
use crate::app::{ApiError, AppState};
use axum::extract::{Path, State};
use axum::Json;
use pharness_store::{CreateRepoWorkItem, CreateStageExecution, SealStageOutcome};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct ContextRepositoryRequest {
    repository_id: String,
    source_commit: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RepoWorkItemPreflightRequest {
    title: String,
    intent: String,
    repository_id: String,
    #[serde(default)]
    source_commit: Option<String>,
    #[serde(default)]
    acceptance_command_names: Vec<String>,
    #[serde(default)]
    context_repositories: Vec<ContextRepositoryRequest>,
    #[serde(default)]
    builder_budget: Option<pharness_core::RunBudget>,
    #[serde(default)]
    max_attempts: Option<u32>,
    #[serde(default)]
    planner_inference_policy: Option<pharness_core::InferencePolicyRef>,
    #[serde(default)]
    planner_execution_policy: Option<pharness_core::AgentExecutionPolicyRef>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateRepoWorkItemRequest {
    title: String,
    intent: String,
    repository_id: String,
    #[serde(default)]
    source_commit: Option<String>,
    #[serde(default)]
    acceptance_command_names: Vec<String>,
    #[serde(default)]
    context_repositories: Vec<ContextRepositoryRequest>,
    #[serde(default)]
    builder_budget: Option<pharness_core::RunBudget>,
    #[serde(default)]
    max_attempts: Option<u32>,
    #[serde(default)]
    planner_inference_policy: Option<pharness_core::InferencePolicyRef>,
    #[serde(default)]
    planner_execution_policy: Option<pharness_core::AgentExecutionPolicyRef>,
    preflight_hash: String,
    actor: String,
    reason: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct RepoWorkItemPreflightResponse {
    product_id: String,
    repository_id: String,
    source_repo: String,
    source_ref: String,
    source_commit: String,
    product_model_snapshot_id: String,
    product_model_snapshot_hash: String,
    repository_contract_version_id: Option<String>,
    repository_contract_hash: Option<String>,
    environment_profile_id: Option<String>,
    selected_acceptance: Vec<Value>,
    context_repositories: Vec<Value>,
    builder_budget: pharness_core::RunBudget,
    max_attempts: u32,
    planner_inference: Value,
    planner_execution: Value,
    readiness_assessment_id: Option<String>,
    blockers: Vec<Value>,
    warnings: Vec<Value>,
    predicted_mutations: Vec<String>,
    authorization_boundaries: Vec<Value>,
    workflow_policy: Option<pharness_core::hosted_sdlc::HostedWorkflowPolicySnapshot>,
    preflight_hash: String,
}

pub(super) async fn preflight_repo_work_item(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
    Json(request): Json<RepoWorkItemPreflightRequest>,
) -> Result<Json<RepoWorkItemPreflightResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    Ok(Json(
        build_repo_work_item_preflight(&state, &product_id, &request).await?,
    ))
}

pub(super) async fn create_repo_work_item(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
    Json(request): Json<CreateRepoWorkItemRequest>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let actor = required_text(request.actor, "actor")?;
    let reason = required_text(request.reason, "reason")?;
    if actor.len() > 200 || reason.len() > 1_000 {
        return Err(ApiError::bad_request(
            "actor or reason exceeds its length limit",
        ));
    }
    let preflight_request = RepoWorkItemPreflightRequest {
        title: request.title,
        intent: request.intent,
        repository_id: request.repository_id,
        source_commit: request.source_commit,
        acceptance_command_names: request.acceptance_command_names,
        context_repositories: request.context_repositories,
        builder_budget: request.builder_budget,
        max_attempts: request.max_attempts,
        planner_inference_policy: request.planner_inference_policy,
        planner_execution_policy: request.planner_execution_policy,
    };
    let preflight = build_repo_work_item_preflight(&state, &product_id, &preflight_request).await?;
    if request.preflight_hash != preflight.preflight_hash {
        return Err(ApiError::conflict(
            "Repo WorkItem preflight is stale; refresh and retry",
        ));
    }
    if !preflight.blockers.is_empty() {
        return Err(ApiError::conflict(format!(
            "Repo WorkItem creation is blocked: {}",
            preflight
                .blockers
                .iter()
                .filter_map(|blocker| blocker.get("code").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let contract_version_id = preflight
        .repository_contract_version_id
        .clone()
        .ok_or_else(|| ApiError::conflict("current RepositoryContract version is missing"))?;
    let contract_version = state
        .store
        .get_repository_contract_version(&contract_version_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository_contract_version", &contract_version_id))?;
    let work_item_id = new_prefixed_id("witem");
    let work_item = state
        .store
        .create_repo_work_item(CreateRepoWorkItem {
            id: work_item_id.clone(),
            product_id: product_id.clone(),
            repository_id: preflight.repository_id.clone(),
            product_model_snapshot_id: preflight.product_model_snapshot_id.clone(),
            product_model_snapshot_hash: preflight.product_model_snapshot_hash.clone(),
            repository_contract_version_id: contract_version_id,
            contract_version: "pharness.dev/v1alpha1".into(),
            title: preflight_request.title.trim().into(),
            intent: preflight_request.intent.trim().into(),
            acceptance_command_names: preflight
                .selected_acceptance
                .iter()
                .filter_map(|entry| entry.get("name").and_then(Value::as_str))
                .map(str::to_string)
                .collect(),
            acceptance_commands: preflight
                .selected_acceptance
                .iter()
                .filter_map(|entry| entry.get("command").and_then(Value::as_str))
                .map(str::to_string)
                .collect(),
            context_repositories: Value::Array(preflight.context_repositories.clone()),
            source_repo: preflight.source_repo.clone(),
            source_ref: preflight.source_ref.clone(),
            source_commit: preflight.source_commit.clone(),
            environment_profile_id: preflight
                .environment_profile_id
                .clone()
                .ok_or_else(|| ApiError::conflict("EnvironmentProfile is missing"))?,
            run_budget: preflight.builder_budget.clone(),
            max_attempts: preflight.max_attempts,
            repository_contract_json: contract_version.contract.clone(),
            repository_contract_hash: preflight
                .repository_contract_hash
                .clone()
                .ok_or_else(|| ApiError::conflict("RepositoryContract hash is missing"))?,
            workflow_policy: preflight.workflow_policy.clone(),
            actor: actor.clone(),
        })
        .await?;
    let planner_profile = if let Some(policy) = &preflight.workflow_policy {
        policy
            .agent_profiles
            .iter()
            .find(|profile| profile.id == "repo-planner")
            .cloned()
            .ok_or_else(|| ApiError::conflict("hosted Planner profile is unavailable"))?
    } else {
        state
            .compiled_agent_profiles(
                state
                    .worker
                    .config_json()
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("unconfigured"),
            )
            .into_iter()
            .find(|profile| profile.id == "repo-planner")
            .ok_or_else(|| ApiError::internal("compiled repo-planner profile is unavailable"))?
    };
    let pinned_planner_policy = preflight
        .workflow_policy
        .as_ref()
        .map(|policy| {
            serde_json::from_value::<pharness_core::InferencePolicyRef>(
                policy.stage_inference["plan"]["policy"].clone(),
            )
        })
        .transpose()
        .map_err(|_| ApiError::conflict("hosted Planner policy is invalid"))?;
    let planner_execution_selection = if preflight.workflow_policy.is_some() {
        None
    } else {
        crate::app::agent_hosts::create_planned_execution_selection(
            &state,
            crate::app::agent_hosts::PlannedExecutionSelectionRequest {
                subject_kind: "work_item",
                subject_id: &work_item_id,
                stage_key: "plan",
                stage: pharness_core::InferenceStage::Plan,
                environment_profile_id: preflight
                    .environment_profile_id
                    .as_deref()
                    .ok_or_else(|| ApiError::conflict("EnvironmentProfile is missing"))?,
                requested: preflight_request.planner_execution_policy.as_ref(),
                actor: &actor,
                reason: &reason,
                state_hash: &preflight.preflight_hash,
            },
        )
        .await?
    };
    let planner_selection = if planner_execution_selection.is_none() && state.inference.enabled {
        Some(
            crate::app::inference::create_planned_selection(
                &state,
                crate::app::inference::PlannedSelectionRequest {
                    subject_kind: "work_item",
                    subject_id: &work_item_id,
                    stage: pharness_core::InferenceStage::Plan,
                    profile: &serde_json::to_value(&planner_profile)
                        .map_err(|error| ApiError::internal(error.to_string()))?,
                    requested: pinned_planner_policy
                        .as_ref()
                        .or(preflight_request.planner_inference_policy.as_ref()),
                    actor: &actor,
                    reason: &reason,
                    state_hash: &preflight.preflight_hash,
                },
            )
            .await?,
        )
    } else {
        None
    };

    let discover_execution_id = new_prefixed_id("stageexec");
    let readiness_id = preflight
        .readiness_assessment_id
        .clone()
        .ok_or_else(|| ApiError::conflict("readiness assessment is missing"))?;
    let discover_inputs = json!({
        "source_commit": preflight.source_commit,
        "product_model_snapshot_id": preflight.product_model_snapshot_id,
        "product_model_snapshot_hash": preflight.product_model_snapshot_hash,
        "repository_contract_version_id": preflight.repository_contract_version_id,
        "repository_contract_hash": preflight.repository_contract_hash,
        "readiness_assessment_id": readiness_id,
    });
    let discover_input_hash = canonical_material_hash(&discover_inputs)?;
    let execution = state
        .store
        .create_stage_execution(CreateStageExecution {
            id: discover_execution_id.clone(),
            work_item_id: work_item_id.clone(),
            stage_key: pharness_core::RepoStageKey::Discover.as_str().into(),
            sequence: 1,
            status: "succeeded".into(),
            agent_profile_id: None,
            agent_profile_version: None,
            agent_profile_hash: None,
            context_pack_id: None,
            run_id: None,
            workspace_id: None,
            input_snapshot: discover_inputs.clone(),
            input_hash: discover_input_hash,
        })
        .await?;
    let metadata = repo_metadata(&state, &work_item_id).await?;
    let outcome_document = pharness_core::StageOutcomeDocument {
        schema_version: pharness_core::STAGE_OUTCOME_SCHEMA.into(),
        work_item_id: work_item_id.clone(),
        stage_execution_id: execution.id.clone(),
        stage: pharness_core::RepoStageKey::Discover,
        origin: "controller".into(),
        status: pharness_core::StageTerminalStatus::Succeeded,
        objective: json!({"kind":"seal_current_repository_readiness"}),
        pinned_inputs: discover_inputs,
        verified_facts: vec![json!({
            "kind": "repository_readiness",
            "assessment_id": readiness_id,
            "contract_status": "ready",
            "coding_status": "ready",
        })],
        agent_claims: Vec::new(),
        outputs: vec![json!({"kind":"repository_discover_stage","status":"succeeded"})],
        acceptance: Vec::new(),
        decisions: vec![json!({"kind":"controller_seal","actor":actor,"reason":reason})],
        authorizations: Vec::new(),
        contradictions: Vec::new(),
        risks: Vec::new(),
        unavailable_capabilities: Vec::new(),
        recommendations: vec![json!({"next":"start_planner"})],
        stop_reason: "controller sealed current Repository readiness evidence".into(),
        sealed_state_version: metadata.state_version,
    };
    let outcome_value = serde_json::to_value(&outcome_document)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let outcome = state
        .store
        .seal_stage_outcome(SealStageOutcome {
            id: new_prefixed_id("stageout"),
            stage_execution_id: execution.id.clone(),
            work_item_id: work_item_id.clone(),
            stage_key: pharness_core::RepoStageKey::Discover.as_str().into(),
            status: "succeeded".into(),
            content_hash: canonical_material_hash(&outcome_value)?,
            outcome: outcome_value,
            state_version: metadata.state_version,
            supersedes_outcome_id: None,
            effective: true,
            actor: "controller".into(),
            reason: "validated current readiness evidence".into(),
        })
        .await?;
    let metadata = repo_metadata(&state, &work_item_id).await?;
    Ok(Json(json!({
        "work_item": work_item,
        "repo_mode": metadata,
        "state_hash": repo_work_item_state_hash(&metadata)?,
        "discover_execution": execution,
        "discover_outcome": outcome,
        "planner_inference_selection":planner_selection,
        "planner_execution_selection":planner_execution_selection,
    })))
}

async fn build_repo_work_item_preflight(
    state: &AppState,
    product_id: &str,
    request: &RepoWorkItemPreflightRequest,
) -> Result<RepoWorkItemPreflightResponse, ApiError> {
    let title = request.title.trim();
    let intent = request.intent.trim();
    if title.is_empty() || title.len() > 200 || intent.is_empty() || intent.len() > 8_000 {
        return Err(ApiError::bad_request(
            "title must be 1-200 characters and intent must be 1-8000 characters",
        ));
    }
    if request
        .source_commit
        .as_deref()
        .is_some_and(|sha| !is_git_sha(sha))
    {
        return Err(ApiError::bad_request(
            "source_commit must be a full 40-character Git object ID",
        ));
    }
    let unique_names = request
        .acceptance_command_names
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    if unique_names.len() != request.acceptance_command_names.len() {
        return Err(ApiError::bad_request(
            "acceptance command names must be unique",
        ));
    }
    if request.context_repositories.len() > 4 {
        return Err(ApiError::bad_request(
            "at most four context repositories are allowed",
        ));
    }
    let product = state
        .store
        .get_product(product_id)
        .await?
        .ok_or_else(|| ApiError::not_found("product", product_id))?;
    let product_snapshot = state
        .store
        .get_product_model_snapshot(&product.current_model_snapshot_id)
        .await?
        .ok_or_else(|| {
            ApiError::not_found("product_model_snapshot", &product.current_model_snapshot_id)
        })?;
    let repository = state
        .store
        .get_repository(&request.repository_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository", &request.repository_id))?;
    let binding = state
        .store
        .get_repository_binding(product_id, &repository.id)
        .await?;
    let source_commit = request
        .source_commit
        .as_deref()
        .unwrap_or(&repository.registered_commit)
        .to_ascii_lowercase();
    let readiness = state
        .store
        .latest_repository_readiness_assessment(&repository.id, &source_commit)
        .await?;
    let contract_version = state
        .store
        .latest_repository_contract_version(&repository.id, &source_commit)
        .await?;
    let mut blockers = Vec::<Value>::new();
    let mut warnings = Vec::<Value>::new();
    if binding.is_none() {
        blockers.push(json!({"code":"repository_not_bound_to_product","summary":"the mutable Repository is not actively bound to this Product"}));
    }
    if repository.registered_commit != source_commit {
        blockers.push(json!({
            "code":"repository_revision_not_registered",
            "summary":"the requested source commit is not the Repository's currently registered immutable revision",
            "registered_commit":repository.registered_commit,
        }));
    }
    let contract = contract_version
        .as_ref()
        .map(|version| {
            serde_json::from_value::<pharness_core::RepositoryContract>(version.contract.clone())
                .map_err(|error| {
                    ApiError::internal(format!("stored RepositoryContract is invalid: {error}"))
                })
        })
        .transpose()?;
    let mut selected_acceptance = Vec::new();
    if let Some(contract) = &contract {
        match state.environment_profiles.iter().find(|profile| {
            profile.active
                && profile.id == contract.environment_profile
                && profile.repository_allowlist.contains(&repository.canonical_url)
        }) {
            Some(profile) => {
                if let Err(error) = contract.validate_for_profile(profile) {
                    blockers.push(json!({"code":"environment_profile_contract_mismatch","summary":error.to_string()}));
                }
            }
            None => blockers.push(json!({"code":"environment_profile_unavailable","summary":"the active RepositoryContract profile is inactive or does not allow this repository"})),
        }
        let acceptance_names = if request.acceptance_command_names.is_empty() {
            contract
                .acceptance_commands
                .iter()
                .map(|command| command.name.clone())
                .collect()
        } else {
            request.acceptance_command_names.clone()
        };
        for name in &acceptance_names {
            if let Some(command) = contract.command(name) {
                selected_acceptance.push(json!({"name":command.name,"command":command.command}));
            } else {
                blockers.push(json!({"code":"acceptance_command_not_declared","summary":format!("acceptance command {name} is not declared by the active RepositoryContract"),"name":name}));
            }
        }
    } else {
        blockers.push(json!({"code":"canonical_contract_version_missing","summary":"no active canonical RepositoryContract exists for the exact source commit"}));
    }
    match (&readiness, &contract_version, &contract) {
        (Some(assessment), Some(version), Some(contract)) => {
            let mismatches = current_readiness_mismatches(
                state,
                &repository,
                &source_commit,
                version,
                contract,
                assessment,
            )
            .await?;
            if !mismatches.is_empty() {
                blockers.push(json!({
                    "code":"repository_readiness_not_current",
                    "summary":"the exact revision does not have a current fully bound contract and coding assessment",
                    "assessment_id":assessment.id,
                    "mismatches":mismatches,
                }));
            }
        }
        (None, _, _) => blockers.push(json!({"code":"repository_readiness_missing","summary":"refresh readiness for this exact source commit before creating a WorkItem"})),
        _ => {}
    }
    let budget = request.builder_budget.clone().unwrap_or_default();
    budget
        .validate()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let max_attempts = request.max_attempts.unwrap_or(2);
    if !(1..=3).contains(&max_attempts) {
        return Err(ApiError::bad_request(
            "Repo Mode max_attempts must be between one and three",
        ));
    }
    let planner_profile = state
        .compiled_agent_profiles(
            state
                .worker
                .config_json()
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("unconfigured"),
        )
        .into_iter()
        .find(|profile| profile.id == "repo-planner")
        .ok_or_else(|| ApiError::internal("compiled repo-planner profile is unavailable"))?;
    let environment_profile_id = contract
        .as_ref()
        .map(|contract| contract.environment_profile.as_str());
    if state.hosted_workflow.enabled && request.planner_execution_policy.is_some() {
        blockers.push(json!({"code":"hosted_gateway_required","summary":"hosted work uses the qualified gateway; native execution policies are deferred"}));
    }
    let planner_execution_binding =
        match environment_profile_id.filter(|_| !state.hosted_workflow.enabled) {
            Some(profile_id) => {
                crate::app::agent_hosts::resolve_execution_binding_auto_auth(
                    state,
                    pharness_core::InferenceStage::Plan,
                    profile_id,
                    request.planner_execution_policy.as_ref(),
                )
                .await?
            }
            None => None,
        };
    let planner_execution = match &planner_execution_binding {
        Some(binding) => json!({
            "mode":"codex_app_server",
            "policy_id":binding.policy.policy_id,
            "policy_revision":binding.policy.revision,
            "policy_hash":binding.policy.policy_hash,
            "model":binding.policy.model,
            "reasoning_effort":binding.policy.reasoning_effort,
            "host_pool":binding.host_pool,
            "runner_image":binding.runner_image,
            "binding_hash":binding.binding_hash,
        }),
        None => Value::Null,
    };
    let planner_inference = if planner_execution_binding.is_some() {
        json!({"mode":"not_selected","reason":"Planner uses an agent execution policy"})
    } else if state.inference.enabled {
        match crate::app::inference::preview_selection(
            state,
            pharness_core::InferenceStage::Plan,
            &serde_json::to_value(&planner_profile)
                .map_err(|error| ApiError::internal(error.to_string()))?,
            request.planner_inference_policy.as_ref(),
        )
        .await
        {
            Ok(selection) => selection,
            Err(error) if state.hosted_workflow.enabled => {
                blockers
                    .push(json!({"code":"hosted_planner_not_qualified","summary":error.message}));
                Value::Null
            }
            Err(error) => return Err(error),
        }
    } else {
        json!({"mode":"direct_fireworks","policy":{"policy_id":"fireworks-legacy-v1","revision":"v1"}})
    };
    let mut context_repositories = Vec::new();
    let mut context_ids = std::collections::BTreeSet::new();
    for context in &request.context_repositories {
        if !context_ids.insert(context.repository_id.as_str())
            || context.repository_id == repository.id
            || !is_git_sha(&context.source_commit)
        {
            blockers.push(json!({"code":"invalid_context_repository","summary":"context repositories must be unique, read-only, distinct from the mutable Repository, and pinned to a full commit SHA","repository_id":context.repository_id}));
            continue;
        }
        let registered = state.store.get_repository(&context.repository_id).await?;
        let bound = state
            .store
            .get_repository_binding(product_id, &context.repository_id)
            .await?;
        let discovered = state
            .store
            .latest_successful_repository_discovery(
                &context.repository_id,
                &context.source_commit.to_ascii_lowercase(),
            )
            .await?;
        match (registered, bound, discovered) {
            (Some(registered), Some(_), Some(discovery)) => context_repositories.push(json!({
                "repository_id":registered.id,
                "canonical_url":registered.canonical_url,
                "source_commit":context.source_commit.to_ascii_lowercase(),
                "discovery_id":discovery.id,
                "discovery_hash":discovery.content_hash,
                "access":"typed_bounded_read",
            })),
            _ => blockers.push(json!({"code":"context_repository_not_ready","summary":"context repository lacks an active Product binding or deterministic discovery at the exact revision","repository_id":context.repository_id})),
        }
    }
    let writer = state.worker.git_writer_settings();
    let observer = state.worker.git_observer_settings();
    if !writer
        .as_ref()
        .is_some_and(|settings| settings.allowed_repos.contains(&repository.canonical_url))
    {
        blockers.push(json!({"code":"source_writer_unavailable","summary":"the source writer is unavailable or this repository is outside its exact allowlist"}));
    }
    if !observer
        .as_ref()
        .is_some_and(|settings| settings.allowed_repos.contains(&repository.canonical_url))
    {
        blockers.push(json!({"code":"provider_observer_unavailable","summary":"the provider observer is unavailable or this repository is outside its exact allowlist"}));
    }
    if !state
        .worker
        .source_reader_allows_repository(&repository.canonical_url)
    {
        blockers.push(json!({"code":"source_reader_unavailable","summary":"the source reader is unavailable or this repository is outside its exact allowlist"}));
    }
    warnings.extend(
        readiness
            .as_ref()
            .and_then(|assessment| assessment.warnings.as_array())
            .cloned()
            .unwrap_or_default(),
    );
    let workflow_policy = match crate::app::hosted_workflow::resolve_policy(
        state,
        product_id,
        &repository,
        &budget,
        max_attempts,
        request.planner_inference_policy.as_ref(),
    )
    .await
    {
        Ok(policy) => policy,
        Err(error) => {
            blockers.push(json!({"code":"hosted_workflow_not_ready","summary":error.message}));
            None
        }
    };
    let predicted_mutations = if blockers.is_empty() && workflow_policy.is_some() {
        vec![
            "create_hosted_work_item".into(),
            "seal_discover_stage_from_readiness".into(),
            "schedule_authorized_workflow".into(),
        ]
    } else if blockers.is_empty() {
        vec![
            "create_repo_work_item".into(),
            "seal_discover_stage_from_readiness".into(),
            "await_explicit_planner_start".into(),
        ]
    } else {
        Vec::new()
    };
    let authorization_boundaries = if let Some(policy) = &workflow_policy {
        vec![
            json!({"boundary":"automatic_workflow","authorization":"immutable_workflow_policy","actions":policy.automatic_actions,"effect":"advance authorized engineering work, source delivery, build, staging, and observation within the recorded limits"}),
            json!({"boundary":"production_gitops_merge","authorization":"human_approval_of_exact_digest_diff_and_staging_evidence","effect":"production approval is required before merging the GitOps change"}),
            json!({"boundary":"release_recovery","authorization":policy.rollback,"effect":"at most one safe rollback to the preceding verified deployment; recovery cannot turn failed work into success"}),
        ]
    } else {
        vec![
            json!({
                "boundary":"planner_model_execution",
                "authorization":"explicit_work_item_action",
                "effect":"Run the pinned repo-planner profile against the sealed context pack",
            }),
            json!({
                "boundary":"stage_chain",
                "authorization":"approved_work_plan_and_exact_chain_grant",
                "effect":"Authorize one bounded Builder, Tester, and Verifier sequence",
            }),
            json!({
                "boundary":"workspace_write",
                "authorization":"attempt_scoped_writable_path_grant",
                "effect":"Write only inside RepositoryContract-declared paths in one durable workspace",
            }),
            json!({
                "boundary":"source_delivery",
                "authorization":"approved_change_set_and_source_mutation_grant",
                "effect":"Create one pull request for the exact approved head and patch",
            }),
            json!({
                "boundary":"merge",
                "authorization":"manual_provider_action",
                "effect":"PHarness observes but never performs the source merge",
            }),
        ]
    };
    let material = json!({
        "schema_version":"pharness.dev/repo-work-item-preflight/v1alpha1",
        "product_id":product_id,
        "product_state_version":product.state_version,
        "product_model_snapshot_id":product_snapshot.id,
        "product_model_snapshot_hash":product_snapshot.content_hash,
        "repository_id":repository.id,
        "source_repo":repository.canonical_url,
        "source_ref":repository.default_branch,
        "source_commit":source_commit,
        "repository_contract_version_id":contract_version.as_ref().map(|version| &version.id),
        "repository_contract_hash":contract_version.as_ref().map(|version| &version.content_hash),
        "environment_profile_id":contract.as_ref().map(|contract| &contract.environment_profile),
        "selected_acceptance":selected_acceptance,
        "context_repositories":context_repositories,
        "builder_budget":budget,
        "max_attempts":max_attempts,
        "planner_inference":planner_inference,
        "planner_execution":planner_execution,
        "readiness_assessment_id":readiness.as_ref().map(|assessment| &assessment.id),
        "readiness_input_hash":readiness.as_ref().map(|assessment| &assessment.input_hash),
        "blockers":blockers,
        "warnings":warnings,
        "predicted_mutations":predicted_mutations,
        "authorization_boundaries":authorization_boundaries,
        "workflow_policy":workflow_policy,
    });
    let preflight_hash = canonical_material_hash(&material)?;
    Ok(RepoWorkItemPreflightResponse {
        product_id: product_id.into(),
        repository_id: repository.id,
        source_repo: repository.canonical_url,
        source_ref: repository.default_branch,
        source_commit,
        product_model_snapshot_id: product_snapshot.id,
        product_model_snapshot_hash: product_snapshot.content_hash,
        repository_contract_version_id: contract_version.as_ref().map(|version| version.id.clone()),
        repository_contract_hash: contract_version
            .as_ref()
            .map(|version| version.content_hash.clone()),
        environment_profile_id: contract.map(|contract| contract.environment_profile),
        selected_acceptance,
        context_repositories,
        builder_budget: budget,
        max_attempts,
        planner_inference,
        planner_execution,
        readiness_assessment_id: readiness.map(|assessment| assessment.id),
        blockers,
        warnings,
        predicted_mutations,
        authorization_boundaries,
        workflow_policy,
        preflight_hash,
    })
}
