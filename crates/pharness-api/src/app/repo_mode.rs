use super::hashing::canonical_material_hash;
use super::identifiers::{is_git_sha, new_prefixed_id};
use super::products::ensure_repo_mode_enabled;
use super::validation::required_text;
use super::{ApiError, AppState};
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pharness_store::{
    CreateOperatorAnnotation, CreateRepoWorkItem, CreateStageExecution, SealStageOutcome,
    StoredRepoWorkItemMetadata,
};
use serde::Deserialize;
use serde_json::{json, Value};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/products/:product_id/work-items/preflight",
            post(preflight_repo_work_item),
        )
        .route(
            "/api/products/:product_id/work-items",
            post(create_repo_work_item),
        )
        .route(
            "/api/work-items/:work_item_id/stage-executions",
            get(list_stage_executions),
        )
        .route(
            "/api/stage-executions/:stage_execution_id",
            get(get_stage_execution),
        )
        .route(
            "/api/stage-executions/:stage_execution_id/outcome",
            get(get_stage_outcome),
        )
        .route(
            "/api/stage-executions/:stage_execution_id/context-pack",
            get(get_stage_context_pack),
        )
        .route(
            "/api/work-items/:work_item_id/annotations",
            get(list_annotations).post(create_annotation),
        )
}

#[derive(Debug, Deserialize)]
struct CreateAnnotationRequest {
    target_kind: String,
    target_id: String,
    statement: String,
    #[serde(default = "empty_array")]
    evidence_refs: Value,
    requested_effect: String,
    actor: String,
    reason: String,
    state_hash: String,
}

fn empty_array() -> Value {
    Value::Array(Vec::new())
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct ContextRepositoryRequest {
    repository_id: String,
    source_commit: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepoWorkItemPreflightRequest {
    title: String,
    intent: String,
    repository_id: String,
    source_commit: String,
    acceptance_command_names: Vec<String>,
    #[serde(default)]
    context_repositories: Vec<ContextRepositoryRequest>,
    #[serde(default)]
    builder_budget: Option<pharness_core::RunBudget>,
    #[serde(default)]
    max_attempts: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRepoWorkItemRequest {
    title: String,
    intent: String,
    repository_id: String,
    source_commit: String,
    acceptance_command_names: Vec<String>,
    #[serde(default)]
    context_repositories: Vec<ContextRepositoryRequest>,
    #[serde(default)]
    builder_budget: Option<pharness_core::RunBudget>,
    #[serde(default)]
    max_attempts: Option<u32>,
    preflight_hash: String,
    actor: String,
    reason: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct RepoWorkItemPreflightResponse {
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
    readiness_assessment_id: Option<String>,
    blockers: Vec<Value>,
    warnings: Vec<Value>,
    predicted_mutations: Vec<String>,
    preflight_hash: String,
}

async fn preflight_repo_work_item(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
    Json(request): Json<RepoWorkItemPreflightRequest>,
) -> Result<Json<RepoWorkItemPreflightResponse>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    Ok(Json(
        build_repo_work_item_preflight(&state, &product_id, &request).await?,
    ))
}

async fn create_repo_work_item(
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
            acceptance_command_names: preflight_request.acceptance_command_names,
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
            actor: actor.clone(),
        })
        .await?;

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
    if !is_git_sha(&request.source_commit) {
        return Err(ApiError::bad_request(
            "source_commit must be a full 40-character Git object ID",
        ));
    }
    if request.acceptance_command_names.is_empty() {
        return Err(ApiError::bad_request(
            "at least one acceptance command name is required",
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
    let source_commit = request.source_commit.to_ascii_lowercase();
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
        for name in &request.acceptance_command_names {
            if let Some(command) = contract.command(name) {
                selected_acceptance.push(json!({"name":command.name,"command":command.command}));
            } else {
                blockers.push(json!({"code":"acceptance_command_not_declared","summary":format!("acceptance command {name} is not declared by the active RepositoryContract"),"name":name}));
            }
        }
    } else {
        blockers.push(json!({"code":"canonical_contract_version_missing","summary":"no active canonical RepositoryContract exists for the exact source commit"}));
    }
    match &readiness {
        Some(assessment)
            if assessment.contract_status == "ready"
                && assessment.coding_status == "ready"
                && contract_version
                    .as_ref()
                    .is_some_and(|version| assessment.contract_version_id.as_deref() == Some(version.id.as_str())
                        && assessment.contract_hash.as_deref() == Some(version.content_hash.as_str())) => {}
        Some(assessment) => blockers.push(json!({
            "code":"repository_readiness_not_current",
            "summary":"the exact revision does not have a current ready contract and coding assessment",
            "assessment_id":assessment.id,
            "contract_status":assessment.contract_status,
            "coding_status":assessment.coding_status,
        })),
        None => blockers.push(json!({"code":"repository_readiness_missing","summary":"refresh readiness for this exact source commit before creating a WorkItem"})),
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
    let predicted_mutations = if blockers.is_empty() {
        vec![
            "create_repo_work_item".into(),
            "seal_discover_stage_from_readiness".into(),
            "await_explicit_planner_start".into(),
        ]
    } else {
        Vec::new()
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
        "readiness_assessment_id":readiness.as_ref().map(|assessment| &assessment.id),
        "readiness_input_hash":readiness.as_ref().map(|assessment| &assessment.input_hash),
        "blockers":blockers,
        "warnings":warnings,
        "predicted_mutations":predicted_mutations,
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
        readiness_assessment_id: readiness.map(|assessment| assessment.id),
        blockers,
        warnings,
        predicted_mutations,
        preflight_hash,
    })
}

async fn list_stage_executions(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    repo_metadata(&state, &work_item_id).await?;
    let executions = state.store.list_stage_executions(&work_item_id).await?;
    Ok(Json(json!({
        "stage_executions": executions,
        "count": executions.len(),
    })))
}

async fn get_stage_execution(
    State(state): State<AppState>,
    Path(stage_execution_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let execution = state
        .store
        .get_stage_execution(&stage_execution_id)
        .await?
        .ok_or_else(|| ApiError::not_found("stage_execution", &stage_execution_id))?;
    Ok(Json(json!({"stage_execution": execution})))
}

async fn get_stage_outcome(
    State(state): State<AppState>,
    Path(stage_execution_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let execution = state
        .store
        .get_stage_execution(&stage_execution_id)
        .await?
        .ok_or_else(|| ApiError::not_found("stage_execution", &stage_execution_id))?;
    let outcome = state
        .store
        .get_stage_outcome_for_execution(&execution.id)
        .await?;
    Ok(Json(json!({
        "stage_execution_id": execution.id,
        "outcome": outcome,
    })))
}

async fn get_stage_context_pack(
    State(state): State<AppState>,
    Path(stage_execution_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let execution = state
        .store
        .get_stage_execution(&stage_execution_id)
        .await?
        .ok_or_else(|| ApiError::not_found("stage_execution", &stage_execution_id))?;
    let pack = match execution.context_pack_id.as_deref() {
        Some(id) => state.store.get_agent_context_pack(id).await?,
        None => None,
    };
    Ok(Json(json!({
        "stage_execution_id": execution.id,
        "context_pack": pack,
    })))
}

async fn list_annotations(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    repo_metadata(&state, &work_item_id).await?;
    let annotations = state.store.list_operator_annotations(&work_item_id).await?;
    Ok(Json(json!({
        "annotations": annotations,
        "count": annotations.len(),
    })))
}

async fn create_annotation(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
    Json(request): Json<CreateAnnotationRequest>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let statement = required_text(request.statement, "statement")?;
    let actor = required_text(request.actor, "actor")?;
    let reason = required_text(request.reason, "reason")?;
    if statement.len() > 4_000 || actor.len() > 200 || reason.len() > 1_000 {
        return Err(ApiError::bad_request(
            "annotation statement, actor, or reason exceeds its length limit",
        ));
    }
    if !matches!(
        request.target_kind.as_str(),
        "work_item" | "stage_execution" | "stage_outcome" | "evidence_validation"
    ) {
        return Err(ApiError::bad_request("unsupported annotation target_kind"));
    }
    if !matches!(
        request.requested_effect.as_str(),
        "add_context" | "mark_evidence_stale" | "repeat_stage" | "replan"
    ) {
        return Err(ApiError::bad_request(
            "requested_effect must add context, mark evidence stale, repeat a stage, or request replan",
        ));
    }
    if !request.evidence_refs.is_array() {
        return Err(ApiError::bad_request("evidence_refs must be an array"));
    }
    let metadata = repo_metadata(&state, &work_item_id).await?;
    let expected_hash = repo_work_item_state_hash(&metadata)?;
    if request.state_hash != expected_hash {
        return Err(ApiError::conflict(
            "Repo WorkItem changed after annotation preview; refresh and retry",
        ));
    }
    if request.target_kind == "stage_execution" {
        let execution = state
            .store
            .get_stage_execution(&request.target_id)
            .await?
            .ok_or_else(|| ApiError::not_found("stage_execution", &request.target_id))?;
        if execution.work_item_id != work_item_id {
            return Err(ApiError::not_found("stage_execution", &request.target_id));
        }
    }
    let annotation = state
        .store
        .create_operator_annotation(CreateOperatorAnnotation {
            id: new_prefixed_id("annot"),
            work_item_id,
            target_kind: request.target_kind,
            target_id: request.target_id,
            statement,
            evidence_refs: request.evidence_refs,
            requested_effect: request.requested_effect,
            actor,
            reason,
            state_hash: expected_hash,
        })
        .await?;
    Ok(Json(json!({"annotation": annotation})))
}

async fn repo_metadata(
    state: &AppState,
    work_item_id: &str,
) -> Result<StoredRepoWorkItemMetadata, ApiError> {
    state
        .store
        .get_repo_work_item_metadata(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repo_work_item", work_item_id))
}

pub(super) fn repo_work_item_state_hash(
    metadata: &StoredRepoWorkItemMetadata,
) -> Result<String, ApiError> {
    canonical_material_hash(&json!({
        "work_item_id": metadata.work_item_id,
        "state_version": metadata.state_version,
        "product_model_snapshot_id": metadata.product_model_snapshot_id,
        "product_model_snapshot_hash": metadata.product_model_snapshot_hash,
        "repository_contract_version_id": metadata.repository_contract_version_id,
        "current_stage_execution_id": metadata.current_stage_execution_id,
        "closed_at": metadata.closed_at,
    }))
}
