use super::identifiers::new_prefixed_id;
use super::{ApiError, AppState};
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::StreamExt;
use pharness_core::{
    canonical_json_sha256, inference_qualification_suite_hash, sign_model_grant, CapabilityKind,
    InferencePolicyRef, InferenceStage, InferenceTargetRef, ModelGrantClaims, ModelMessage,
    ModelRequest, ResolvedInferenceBinding, RunId, SessionId, ToolProtocolMode, ToolSpec,
    MODEL_GRANT_SCHEMA, RESOLVED_INFERENCE_BINDING_SCHEMA,
};
use pharness_openai_compatible::{
    build_chat_request, OpenAiStreamAggregate, SseDecoder, StreamChunk,
};
use pharness_runhost::{RunInferenceSpec, SYSTEM_PROMPT_VERSION};
use pharness_store::{
    CreateInferenceEvaluation, CreateInferenceEvaluationGrantIssuance,
    CreateInferencePolicyQualification, CreateInferenceTargetVerification,
    CreateModelGrantIssuance, CreateStageInferenceSelection, StoredInferenceEvaluation,
    StoredInferenceTargetVerification, StoredRun,
};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::timeout;

#[derive(Debug, Default, Deserialize)]
pub(super) struct StageQuery {
    stage: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigurationActionRequest {
    actor: String,
    reason: String,
    config_hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QualificationStartRequest {
    actor: String,
    reason: String,
    config_hash: String,
    #[serde(default = "default_qualification_attempts")]
    attempts: u32,
}

fn default_qualification_attempts() -> u32 {
    2
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InferenceEvaluationOutcomeRequest {
    report: Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelGrantRequest {
    selection_id: String,
    stage_execution_id: String,
    request_sequence: u32,
    request_body_hash: String,
}

#[derive(Debug, Serialize)]
struct ModelGrantResponse {
    token: String,
    expires_at_epoch_seconds: u64,
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/inference-targets", get(list_targets))
        .route(
            "/api/inference-targets/:target_id/revisions/:revision/preflight",
            post(preflight_target),
        )
        .route(
            "/api/inference-targets/:target_id/revisions/:revision/verifications",
            get(list_target_verifications),
        )
        .route("/api/inference-policies", get(list_policies))
        .route(
            "/api/inference-policies/:policy_id/revisions/:revision/qualifications",
            post(create_policy_qualification).get(list_policy_qualifications),
        )
        .route(
            "/api/inference-evaluations/:evaluation_id",
            get(get_inference_evaluation),
        )
}

pub(super) fn internal_router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/internal/runs/:run_id/model-grants",
            post(issue_model_grant),
        )
        .route(
            "/api/internal/inference-evaluations/:evaluation_id/context",
            get(internal_inference_evaluation_context),
        )
        .route(
            "/api/internal/inference-evaluations/:evaluation_id/outcome",
            post(internal_inference_evaluation_outcome),
        )
}

pub(super) async fn ensure_run_inference_selection(
    state: &AppState,
    run: &StoredRun,
) -> Result<Option<RunInferenceSpec>, ApiError> {
    if !state.inference.enabled {
        return Ok(None);
    }
    let existing = state
        .store
        .get_stage_inference_selection_for_run(run.id.as_str())
        .await?;
    if existing.is_none()
        && run
            .execution_target_json
            .pointer("/inference/mode")
            .and_then(Value::as_str)
            != Some("gateway")
    {
        return Ok(None);
    }
    let selection = match existing {
        Some(selection) => selection,
        None => create_run_selection(state, run).await?,
    };
    let events = state.store.list_events(&run.id).await?;
    let next_request_sequence = events
        .iter()
        .filter(|event| event.kind == pharness_core::EventKind::ModelRequestStarted)
        .count()
        .saturating_add(1);
    Ok(Some(RunInferenceSpec {
        selection_id: selection.id,
        stage_execution_id: selection
            .stage_execution_id
            .unwrap_or_else(|| format!("standalone:{}", run.id)),
        binding: selection.resolved_binding,
        next_request_sequence: u32::try_from(next_request_sequence)
            .map_err(|_| ApiError::conflict("Run model request sequence exceeded its bound"))?,
    }))
}

pub(super) fn execution_marker(
    state: &AppState,
    requested_policy: Option<&InferencePolicyRef>,
) -> Value {
    if state.inference.enabled {
        json!({
            "mode":"gateway",
            "registry_hash":state.inference.registry.config_hash,
            "policy":requested_policy,
        })
    } else {
        json!({"mode":"direct_fireworks"})
    }
}

async fn create_run_selection(
    state: &AppState,
    run: &StoredRun,
) -> Result<pharness_store::StoredStageInferenceSelection, ApiError> {
    let (stage, stage_key) = inference_stage_for_run(run)?;
    if let Some(selection_id) = run
        .execution_target_json
        .pointer("/inference/planned_selection_id")
        .and_then(Value::as_str)
    {
        let planned = state
            .store
            .get_stage_inference_selection(selection_id)
            .await?
            .ok_or_else(|| ApiError::conflict("planned inference selection is unavailable"))?;
        if planned.run_id.is_some() || planned.stage_key != stage_key {
            return Err(ApiError::conflict(
                "planned inference selection does not match the Run stage",
            ));
        }
        verify_binding_is_configured(state, &planned.resolved_binding)?;
        let stage_execution_id = run
            .execution_target_json
            .pointer("/repo_mode/stage_execution_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        return state
            .store
            .create_stage_inference_selection(CreateStageInferenceSelection {
                id: new_prefixed_id("inferselect"),
                subject_kind: planned.subject_kind,
                subject_id: planned.subject_id,
                stage_key: planned.stage_key,
                effective_settings: planned.effective_settings,
                resolved_binding: planned.resolved_binding,
                actor: "controller".into(),
                reason: "bound planned inference selection to queued Run".into(),
                state_hash: canonical_json_sha256(&run.execution_target_json).map_err(|error| {
                    ApiError::internal(format!("failed to hash Run selection state: {error}"))
                })?,
                supersedes_selection_id: Some(planned.id),
                stage_execution_id,
                run_id: Some(run.id.to_string()),
            })
            .await
            .map_err(Into::into);
    }
    let profile_id = run
        .execution_target_json
        .pointer("/agent_profile/id")
        .and_then(Value::as_str)
        .unwrap_or("repo-builder");
    let compiled_profile = pharness_core::compiled_agent_profiles(
        state
            .worker
            .config_json()
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("unconfigured"),
        SYSTEM_PROMPT_VERSION,
    )
    .into_iter()
    .find(|profile| profile.id == profile_id)
    .ok_or_else(|| ApiError::conflict("Run AgentProfile is unavailable"))?;
    let profile = run
        .execution_target_json
        .get("agent_profile")
        .cloned()
        .unwrap_or_else(|| serde_json::to_value(&compiled_profile).unwrap_or(Value::Null));
    let base_agent_profile_hash = profile
        .get("base_profile_hash")
        .or_else(|| profile.get("profile_hash"))
        .and_then(Value::as_str)
        .filter(|hash| *hash == compiled_profile.profile_hash)
        .unwrap_or(&compiled_profile.profile_hash);
    let tools = profile_tools(&profile);
    let budget = profile
        .get("budget")
        .cloned()
        .or_else(|| run.execution_target_json.get("run_budget").cloned())
        .unwrap_or(Value::Null);
    let requested = run
        .execution_target_json
        .pointer("/inference/policy")
        .cloned()
        .filter(|value| !value.is_null())
        .map(serde_json::from_value::<InferencePolicyRef>)
        .transpose()
        .map_err(|error| ApiError::conflict(format!("Run inference policy is invalid: {error}")))?;
    let binding = resolve_binding(
        state,
        stage,
        profile_id,
        base_agent_profile_hash,
        &tools,
        &budget,
        requested.as_ref(),
    )
    .await?;
    let stage_execution_id = run
        .execution_target_json
        .pointer("/repo_mode/stage_execution_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let (subject_kind, subject_id) = if let Some(work_item_id) = run
        .execution_target_json
        .pointer("/run_scope/work_item_id")
        .and_then(Value::as_str)
    {
        ("work_item", work_item_id.to_string())
    } else if let Some(onboarding_id) = run
        .execution_target_json
        .pointer("/onboarding/onboarding_id")
        .and_then(Value::as_str)
    {
        ("repository_onboarding", onboarding_id.to_string())
    } else {
        ("run", run.id.to_string())
    };
    state
        .store
        .create_stage_inference_selection(CreateStageInferenceSelection {
            id: new_prefixed_id("inferselect"),
            subject_kind: subject_kind.into(),
            subject_id,
            stage_key: stage_key.into(),
            effective_settings: effective_settings(&binding),
            resolved_binding: binding,
            actor: "controller".into(),
            reason: "qualified stage default selected before model dispatch".into(),
            state_hash: canonical_json_sha256(&run.execution_target_json).map_err(|error| {
                ApiError::internal(format!("failed to hash Run selection state: {error}"))
            })?,
            supersedes_selection_id: None,
            stage_execution_id,
            run_id: Some(run.id.to_string()),
        })
        .await
        .map_err(Into::into)
}

pub(super) struct PlannedSelectionRequest<'a> {
    pub subject_kind: &'a str,
    pub subject_id: &'a str,
    pub stage: InferenceStage,
    pub profile: &'a Value,
    pub requested: Option<&'a InferencePolicyRef>,
    pub actor: &'a str,
    pub reason: &'a str,
    pub state_hash: &'a str,
}

pub(super) async fn create_planned_selection(
    state: &AppState,
    request: PlannedSelectionRequest<'_>,
) -> Result<pharness_store::StoredStageInferenceSelection, ApiError> {
    let profile_id = request
        .profile
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::internal("AgentProfile ID is unavailable"))?;
    let base_agent_profile_hash = request
        .profile
        .get("profile_hash")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::internal("AgentProfile hash is unavailable"))?;
    let tools = profile_tools(request.profile);
    let budget = request
        .profile
        .get("budget")
        .cloned()
        .unwrap_or(Value::Null);
    let binding = resolve_binding(
        state,
        request.stage,
        profile_id,
        base_agent_profile_hash,
        &tools,
        &budget,
        request.requested,
    )
    .await?;
    let stage_key = inference_stage_key(request.stage);
    let supersedes_selection_id =
        latest_planned_selection(state, request.subject_kind, request.subject_id, stage_key)
            .await?
            .map(|selection| selection.id);
    state
        .store
        .create_stage_inference_selection(CreateStageInferenceSelection {
            id: new_prefixed_id("inferselect"),
            subject_kind: request.subject_kind.into(),
            subject_id: request.subject_id.into(),
            stage_key: stage_key.into(),
            effective_settings: effective_settings(&binding),
            resolved_binding: binding,
            actor: request.actor.into(),
            reason: request.reason.into(),
            state_hash: request.state_hash.into(),
            supersedes_selection_id,
            stage_execution_id: None,
            run_id: None,
        })
        .await
        .map_err(Into::into)
}

pub(super) async fn preview_selection(
    state: &AppState,
    stage: InferenceStage,
    profile: &Value,
    requested: Option<&InferencePolicyRef>,
) -> Result<Value, ApiError> {
    let profile_id = profile
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::internal("AgentProfile ID is unavailable"))?;
    let base_agent_profile_hash = profile
        .get("profile_hash")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::internal("AgentProfile hash is unavailable"))?;
    let binding = resolve_binding(
        state,
        stage,
        profile_id,
        base_agent_profile_hash,
        &profile_tools(profile),
        &profile.get("budget").cloned().unwrap_or(Value::Null),
        requested,
    )
    .await?;
    Ok(json!({
        "policy":{"policy_id":binding.policy.policy_id,"revision":binding.policy.revision},
        "policy_hash":binding.policy.policy_hash,
        "target":{"target_id":binding.target.target_id,"revision":binding.target.revision},
        "target_hash":binding.target.config_hash,
        "binding_hash":binding.binding_hash,
        "base_agent_profile_hash":binding.base_agent_profile_hash,
        "agent_profile_hash":binding.agent_profile_hash,
        "display_name":binding.policy.display_name,
        "backend_kind":binding.target.backend_kind,
        "model":binding.target.upstream_model,
        "reasoning":binding.policy.reasoning,
        "temperature":binding.policy.temperature(),
        "maximum_output_tokens":binding.policy.max_output_tokens,
    }))
}

pub(super) fn execution_marker_for_selection(
    state: &AppState,
    selection: &pharness_store::StoredStageInferenceSelection,
) -> Value {
    if !state.inference.enabled {
        return json!({"mode":"direct_fireworks"});
    }
    json!({
        "mode":"gateway",
        "registry_hash":state.inference.registry.config_hash,
        "planned_selection_id":selection.id,
        "policy":{"policy_id":selection.policy_id,"revision":selection.policy_revision},
        "policy_hash":selection.policy_hash,
        "target_hash":selection.target_hash,
        "binding_hash":selection.binding_hash,
        "resolved":sanitized_binding(&selection.resolved_binding),
    })
}

fn sanitized_binding(binding: &ResolvedInferenceBinding) -> Value {
    json!({
        "backend_kind":binding.target.backend_kind,
        "model":binding.target.upstream_model,
        "target_id":binding.target.target_id,
        "target_revision":binding.target.revision,
        "target_hash":binding.target.config_hash,
        "policy_id":binding.policy.policy_id,
        "policy_revision":binding.policy.revision,
        "policy_hash":binding.policy.policy_hash,
        "reasoning":binding.policy.reasoning,
        "temperature":binding.policy.temperature(),
        "maximum_output_tokens":binding.policy.max_output_tokens,
        "context_assembly_limit":binding.policy.max_input_tokens,
        "transport_retry_attempts":binding.policy.transport_max_attempts,
        "prompt_version":binding.prompt_version,
        "tool_schema_hash":binding.tool_schema_hash,
        "profile_budget_hash":binding.profile_budget_hash,
        "base_agent_profile_hash":binding.base_agent_profile_hash,
        "agent_profile_hash":binding.agent_profile_hash,
        "binding_hash":binding.binding_hash,
    })
}

pub(super) async fn latest_planned_selection(
    state: &AppState,
    subject_kind: &str,
    subject_id: &str,
    stage_key: &str,
) -> Result<Option<pharness_store::StoredStageInferenceSelection>, ApiError> {
    Ok(state
        .store
        .list_stage_inference_selections(subject_kind, subject_id)
        .await?
        .into_iter()
        .rfind(|selection| selection.run_id.is_none() && selection.stage_key == stage_key))
}

async fn resolve_binding(
    state: &AppState,
    stage: InferenceStage,
    profile_id: &str,
    base_agent_profile_hash: &str,
    tools: &Value,
    budget: &Value,
    requested: Option<&InferencePolicyRef>,
) -> Result<ResolvedInferenceBinding, ApiError> {
    let policy_ref = match requested {
        Some(value) => value,
        None => state
            .inference
            .registry
            .defaults
            .get(&stage)
            .ok_or_else(|| {
                ApiError::unavailable(
                    "no qualified default inference policy is configured for this stage",
                )
            })?,
    };
    let policy = state
        .inference
        .registry
        .policy(&policy_ref.policy_id, &policy_ref.revision)
        .ok_or_else(|| ApiError::conflict("selected inference policy revision is unavailable"))?
        .clone();
    if !policy.selectable
        || !policy.eligible_stages.contains(&stage)
        || !policy
            .eligible_profiles
            .iter()
            .any(|value| value == profile_id)
    {
        return Err(ApiError::conflict(format!(
            "selected inference policy is not active for AgentProfile {profile_id}"
        )));
    }
    if policy.policy_id != "fireworks-legacy-v1" {
        let qualified = state
            .store
            .list_inference_policy_qualifications(&policy.policy_id, &policy.revision)
            .await?
            .into_iter()
            .next()
            .is_some_and(|value| {
                value.verdict == "passed"
                    && value.policy_hash == policy.policy_hash
                    && value.target_hash == policy.target_hash
            });
        if !qualified {
            return Err(ApiError::conflict(
                "selected inference policy has no passing qualification for its current hashes",
            ));
        }
    }
    let target = state
        .inference
        .registry
        .target(&policy.target.target_id, &policy.target.revision)
        .ok_or_else(|| ApiError::conflict("selected inference target revision is unavailable"))?
        .clone();
    if !target.selectable || !target.allowed_stages.contains(&stage) {
        return Err(ApiError::conflict(
            "selected inference target is not active for this stage",
        ));
    }
    let mut binding = ResolvedInferenceBinding {
        schema_version: RESOLVED_INFERENCE_BINDING_SCHEMA.into(),
        target,
        policy,
        prompt_version: SYSTEM_PROMPT_VERSION.into(),
        base_agent_profile_hash: base_agent_profile_hash.into(),
        agent_profile_hash: String::new(),
        tool_schema_hash: canonical_json_sha256(tools).map_err(|error| {
            ApiError::internal(format!("failed to hash profile tools: {error}"))
        })?,
        profile_budget_hash: canonical_json_sha256(budget).map_err(|error| {
            ApiError::internal(format!("failed to hash profile budget: {error}"))
        })?,
        binding_hash: String::new(),
    };
    binding.agent_profile_hash = binding.computed_agent_profile_hash().map_err(|error| {
        ApiError::internal(format!("failed to hash resolved AgentProfile: {error}"))
    })?;
    binding.binding_hash = binding.computed_hash().map_err(|error| {
        ApiError::internal(format!("failed to hash inference binding: {error}"))
    })?;
    binding.validate().map_err(|error| {
        ApiError::internal(format!("resolved inference binding is invalid: {error}"))
    })?;
    Ok(binding)
}

fn verify_binding_is_configured(
    state: &AppState,
    binding: &ResolvedInferenceBinding,
) -> Result<(), ApiError> {
    binding.validate().map_err(|error| {
        ApiError::conflict(format!("stored inference binding is invalid: {error}"))
    })?;
    let target = state
        .inference
        .registry
        .target(&binding.target.target_id, &binding.target.revision);
    let policy = state
        .inference
        .registry
        .policy(&binding.policy.policy_id, &binding.policy.revision);
    if match target {
        Some(value) => value.config_hash != binding.target.config_hash,
        None => true,
    } || match policy {
        Some(value) => value.policy_hash != binding.policy.policy_hash,
        None => true,
    } {
        return Err(ApiError::conflict(
            "planned inference binding no longer matches the configured registry",
        ));
    }
    Ok(())
}

fn effective_settings(binding: &ResolvedInferenceBinding) -> Value {
    json!({
        "backend_kind":binding.target.backend_kind,
        "model":binding.target.upstream_model,
        "reasoning":binding.policy.reasoning,
        "temperature":binding.policy.temperature(),
        "maximum_output_tokens":binding.policy.max_output_tokens,
        "context_assembly_limit":binding.policy.max_input_tokens,
        "tool_protocol":binding.policy.tool_protocol,
        "transport_retry_attempts":binding.policy.transport_max_attempts,
        "base_agent_profile_hash":binding.base_agent_profile_hash,
        "agent_profile_hash":binding.agent_profile_hash,
    })
}

fn profile_tools(profile: &Value) -> Value {
    profile
        .get("tools")
        .or_else(|| profile.get("allowed_tools"))
        .cloned()
        .unwrap_or(Value::Null)
}

fn inference_stage_key(stage: InferenceStage) -> &'static str {
    match stage {
        InferenceStage::Onboarding => "onboarding",
        InferenceStage::Plan => "plan",
        InferenceStage::Implement => "implement",
        InferenceStage::Test => "test",
        InferenceStage::Verify => "verify",
    }
}

fn inference_stage_for_run(run: &StoredRun) -> Result<(InferenceStage, &'static str), ApiError> {
    if run
        .execution_target_json
        .pointer("/onboarding/onboarding_id")
        .and_then(Value::as_str)
        .is_some()
    {
        return Ok((InferenceStage::Onboarding, "onboarding"));
    }
    match run
        .execution_target_json
        .pointer("/repo_mode/stage")
        .and_then(Value::as_str)
        .unwrap_or("implement")
    {
        "plan" => Ok((InferenceStage::Plan, "plan")),
        "implement" => Ok((InferenceStage::Implement, "implement")),
        "test" => Ok((InferenceStage::Test, "test")),
        "verify" => Ok((InferenceStage::Verify, "verify")),
        _ => Err(ApiError::conflict("Run stage is not model-backed")),
    }
}

async fn list_targets(
    State(state): State<AppState>,
    Query(query): Query<StageQuery>,
) -> Result<Json<Value>, ApiError> {
    let stage = query.stage.as_deref().map(parse_stage).transpose()?;
    let mut targets = Vec::new();
    for target in state
        .inference
        .registry
        .targets
        .iter()
        .filter(|target| match stage {
            Some(stage) => target.allowed_stages.contains(&stage),
            None => true,
        })
    {
        let latest_verification = state
            .store
            .list_inference_target_verifications(&target.target_id, &target.revision)
            .await?
            .into_iter()
            .next();
        targets.push(json!({
                "target_id":target.target_id,
                "revision":target.revision,
                "display_name":target.display_name,
                "backend_kind":target.backend_kind,
                "protocol":target.protocol,
                "upstream_model":target.upstream_model,
                "authentication_configured":target.authentication_binding.is_some(),
                "transport":{
                    "scheme":url::Url::parse(&target.upstream_base_url).ok().map(|value| value.scheme().to_string()),
                    "connect_timeout_seconds":target.transport.connect_timeout_seconds,
                    "first_response_timeout_seconds":target.transport.first_response_timeout_seconds,
                    "stream_idle_timeout_seconds":target.transport.stream_idle_timeout_seconds,
                    "max_attempts":target.transport.max_attempts,
                    "private_network":target.transport.allow_insecure_private_http,
                },
                "capabilities":target.capabilities,
                "context_limit_tokens":target.context_limit_tokens,
                "output_limit_tokens":target.output_limit_tokens,
                "allowed_stages":target.allowed_stages,
                "selectable":target.selectable,
                "config_hash":target.config_hash,
                "latest_verification":latest_verification,
            }));
    }
    let gateway = gateway_readiness(&state).await;
    Ok(Json(json!({
        "gateway_enabled":state.inference.enabled,
        "registry_hash":state.inference.registry.config_hash,
        "gateway":gateway,
        "targets":targets,
    })))
}

async fn list_policies(
    State(state): State<AppState>,
    Query(query): Query<StageQuery>,
) -> Result<Json<Value>, ApiError> {
    let stage = query.stage.as_deref().map(parse_stage).transpose()?;
    let mut policies = Vec::new();
    for policy in state
        .inference
        .registry
        .policies
        .iter()
        .filter(|policy| match stage {
            Some(stage) => policy.eligible_stages.contains(&stage),
            None => true,
        })
    {
        let qualification_contract = policy
            .eligible_stages
            .iter()
            .find_map(|stage| qualification_contract_for_stage(*stage).ok())
            .and_then(|(suite_id, profile_id)| {
                let target = state
                    .inference
                    .registry
                    .target(&policy.target.target_id, &policy.target.revision)?;
                let profile = pharness_core::compiled_agent_profiles(
                    &target.upstream_model,
                    SYSTEM_PROMPT_VERSION,
                )
                .into_iter()
                .find(|profile| profile.id == profile_id)?;
                Some(json!({
                    "suite_id":suite_id,
                    "agent_profile_id":profile.id,
                    "agent_profile_hash":profile.profile_hash,
                }))
            });
        let latest_qualification = state
            .store
            .list_inference_policy_qualifications(&policy.policy_id, &policy.revision)
            .await?
            .into_iter()
            .next();
        let latest_evaluation = state
            .store
            .list_inference_evaluations(&policy.policy_id, &policy.revision)
            .await?
            .into_iter()
            .next();
        let is_default = policy.eligible_stages.iter().any(|stage| {
            state.inference.registry.defaults.get(stage)
                == Some(&InferencePolicyRef {
                    policy_id: policy.policy_id.clone(),
                    revision: policy.revision.clone(),
                })
        });
        let is_legacy_baseline = policy.policy_id == "fireworks-legacy-v1";
        let qualified = is_legacy_baseline
            || latest_qualification.as_ref().is_some_and(|value| {
                value.verdict == "passed"
                    && value.policy_hash == policy.policy_hash
                    && value.target_hash == policy.target_hash
            });
        policies.push(json!({
                "policy_id":policy.policy_id,
                "revision":policy.revision,
                "display_name":policy.display_name,
                "eligible_profiles":policy.eligible_profiles,
                "eligible_stages":policy.eligible_stages,
                "target":policy.target,
                "target_hash":policy.target_hash,
                "reasoning":policy.reasoning,
                "temperature":policy.temperature(),
                "maximum_output_tokens":policy.max_output_tokens,
                "context_assembly_limit":policy.max_input_tokens,
                "tool_protocol":policy.tool_protocol,
                "transport_retry_attempts":policy.transport_max_attempts,
                "selectable":policy.selectable,
                "is_default":is_default,
                "policy_hash":policy.policy_hash,
                "qualified":qualified,
                "qualification_status":if is_legacy_baseline { "accepted_legacy_baseline" } else if qualified { "passed" } else { "not_qualified" },
                "latest_qualification":latest_qualification,
                "latest_evaluation":latest_evaluation,
                "qualification_contract":qualification_contract,
            }));
    }
    Ok(Json(json!({
        "registry_hash":state.inference.registry.config_hash,
        "policies":policies,
    })))
}

pub(super) async fn gateway_readiness(state: &AppState) -> Value {
    if !state.inference.enabled {
        return json!({
            "status":"disabled",
            "api_registry_hash":state.inference.registry.config_hash,
            "gateway_registry_hash":null,
            "registry_aligned":false,
            "direct_fireworks_enabled":state.inference.direct_fireworks_enabled,
            "blocker":"Inference Gateway is disabled; new Runs remain on the direct Fireworks rollback path.",
        });
    }
    let result = async {
        let base = url::Url::parse(&state.inference.gateway_url)
            .map_err(|error| format!("gateway URL is invalid: {error}"))?;
        let response = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| format!("gateway client failed: {error}"))?
            .get(base.join("readyz").map_err(|error| error.to_string())?)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|error| format!("gateway readiness failed: {error}"))?
            .error_for_status()
            .map_err(|error| format!("gateway readiness failed: {error}"))?;
        response
            .json::<Value>()
            .await
            .map_err(|error| format!("gateway readiness response was invalid: {error}"))
    }
    .await;
    match result {
        Ok(ready) => {
            let gateway_hash = ready
                .get("registry_hash")
                .and_then(Value::as_str)
                .map(str::to_string);
            let aligned =
                gateway_hash.as_deref() == Some(state.inference.registry.config_hash.as_str());
            json!({
                "status":if aligned { "available" } else { "mismatch" },
                "api_registry_hash":state.inference.registry.config_hash,
                "gateway_registry_hash":gateway_hash,
                "registry_aligned":aligned,
                "direct_fireworks_enabled":state.inference.direct_fireworks_enabled,
                "blocker":if aligned { Value::Null } else { json!("Gateway and API inference registries do not match.") },
            })
        }
        Err(message) => json!({
            "status":"unavailable",
            "api_registry_hash":state.inference.registry.config_hash,
            "gateway_registry_hash":null,
            "registry_aligned":false,
            "direct_fireworks_enabled":state.inference.direct_fireworks_enabled,
            "blocker":sanitize_failure(&message),
        }),
    }
}

async fn preflight_target(
    State(state): State<AppState>,
    Path((target_id, revision)): Path<(String, String)>,
    Json(request): Json<ConfigurationActionRequest>,
) -> Result<Json<StoredInferenceTargetVerification>, ApiError> {
    validate_configuration_action(&state, &request)?;
    let target = state
        .inference
        .registry
        .target(&target_id, &revision)
        .ok_or_else(|| {
            ApiError::not_found(
                "inference_target_revision",
                &format!("{target_id}@{revision}"),
            )
        })?
        .clone();
    let policy = state
        .inference
        .registry
        .policies
        .iter()
        .find(|policy| policy.target.target_id == target_id && policy.target.revision == revision)
        .ok_or_else(|| ApiError::conflict("target has no compatible configured inference policy"))?
        .clone();
    let verification_id = new_prefixed_id("inferverify");
    let now = epoch_seconds();
    let result = verify_target_protocol(&state, &verification_id, &target, &policy).await;
    let (status, reachability, model_visible, streaming_compatible, tool_compatible, failure) =
        match result {
            Ok(()) => ("passed", "reachable", true, true, true, None),
            Err(message) => (
                "failed",
                "unavailable",
                false,
                false,
                false,
                Some(sanitize_failure(&message)),
            ),
        };
    let verification = state
        .store
        .create_inference_target_verification(CreateInferenceTargetVerification {
            id: verification_id,
            target_id: target.target_id,
            target_revision: target.revision,
            target_hash: target.config_hash,
            status: status.into(),
            reachability: reachability.into(),
            model_visible,
            streaming_compatible,
            tool_compatible,
            observed_capabilities: json!({
                "protocol":"openai_chat_completions_v1",
                "streaming":streaming_compatible,
                "native_tools":tool_compatible,
                "registry_hash":state.inference.registry.config_hash,
            }),
            sanitized_failure: failure,
            actor: request.actor,
            reason: request.reason,
            config_hash: request.config_hash,
            expires_at: now.saturating_add(900).to_string(),
        })
        .await?;
    Ok(Json(verification))
}

async fn list_target_verifications(
    State(state): State<AppState>,
    Path((target_id, revision)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    if state
        .inference
        .registry
        .target(&target_id, &revision)
        .is_none()
    {
        return Err(ApiError::not_found(
            "inference_target_revision",
            &format!("{target_id}@{revision}"),
        ));
    }
    let verifications = state
        .store
        .list_inference_target_verifications(&target_id, &revision)
        .await?;
    Ok(Json(json!({"verifications":verifications})))
}

async fn create_policy_qualification(
    State(state): State<AppState>,
    Path((policy_id, revision)): Path<(String, String)>,
    Json(request): Json<QualificationStartRequest>,
) -> Result<Json<StoredInferenceEvaluation>, ApiError> {
    validate_configuration_hash(&state, &request.config_hash)?;
    validate_actor_reason(&request.actor, &request.reason)?;
    if request.attempts == 0 || request.attempts > 2 {
        return Err(ApiError::bad_request(
            "qualification attempts must be between one and two",
        ));
    }
    let policy = state
        .inference
        .registry
        .policy(&policy_id, &revision)
        .ok_or_else(|| {
            ApiError::not_found(
                "inference_policy_revision",
                &format!("{policy_id}@{revision}"),
            )
        })?;
    let target = state
        .inference
        .registry
        .target(&policy.target.target_id, &policy.target.revision)
        .ok_or_else(|| ApiError::internal("policy target is absent from the validated registry"))?;
    if policy.policy_id == "fireworks-legacy-v1" || !policy.selectable || !target.selectable {
        return Err(ApiError::conflict(
            "only active non-legacy candidate policies may be qualified",
        ));
    }
    let stage = policy
        .eligible_stages
        .iter()
        .copied()
        .find(|stage| qualification_contract_for_stage(*stage).is_ok())
        .ok_or_else(|| ApiError::conflict("policy has no supported qualification stage"))?;
    let (suite_id, expected_profile_id) = qualification_contract_for_stage(stage)?;
    if !policy.eligible_stages.contains(&stage)
        || !policy
            .eligible_profiles
            .iter()
            .any(|profile| profile == expected_profile_id)
    {
        return Err(ApiError::conflict(
            "qualification suite, stage, profile, and policy eligibility do not match",
        ));
    }
    let profile =
        pharness_core::compiled_agent_profiles(&target.upstream_model, SYSTEM_PROMPT_VERSION)
            .into_iter()
            .find(|profile| profile.id == expected_profile_id)
            .ok_or_else(|| ApiError::internal("compiled qualification AgentProfile is missing"))?;
    let suite_hash = inference_qualification_suite_hash(suite_id).map_err(|error| {
        ApiError::internal(format!("failed to hash qualification suite: {error}"))
    })?;
    let fresh_target_verification = state
        .store
        .list_inference_target_verifications(&target.target_id, &target.revision)
        .await?
        .into_iter()
        .any(|verification| {
            verification.target_hash == target.config_hash
                && verification.status == "passed"
                && verification.model_visible
                && verification.streaming_compatible
                && verification.tool_compatible
                && verification
                    .expires_at
                    .parse::<u64>()
                    .ok()
                    .is_some_and(|expires_at| expires_at > epoch_seconds())
        });
    if !fresh_target_verification {
        return Err(ApiError::conflict(
            "inference target requires a fresh passing protocol verification before qualification",
        ));
    }
    let tools = serde_json::to_value(&profile.tools).map_err(|error| {
        ApiError::internal(format!("failed to serialize profile tools: {error}"))
    })?;
    let budget = serde_json::to_value(&profile.budget).map_err(|error| {
        ApiError::internal(format!("failed to serialize profile budget: {error}"))
    })?;
    let mut binding = ResolvedInferenceBinding {
        schema_version: RESOLVED_INFERENCE_BINDING_SCHEMA.into(),
        target: target.clone(),
        policy: policy.clone(),
        prompt_version: SYSTEM_PROMPT_VERSION.into(),
        base_agent_profile_hash: profile.profile_hash.clone(),
        agent_profile_hash: String::new(),
        tool_schema_hash: canonical_json_sha256(&tools).map_err(|error| {
            ApiError::internal(format!("failed to hash qualification tools: {error}"))
        })?,
        profile_budget_hash: canonical_json_sha256(&budget).map_err(|error| {
            ApiError::internal(format!("failed to hash qualification budget: {error}"))
        })?,
        binding_hash: String::new(),
    };
    binding.agent_profile_hash = binding.computed_agent_profile_hash().map_err(|error| {
        ApiError::internal(format!(
            "failed to hash qualification AgentProfile: {error}"
        ))
    })?;
    binding.binding_hash = binding.computed_hash().map_err(|error| {
        ApiError::internal(format!("failed to hash qualification binding: {error}"))
    })?;
    binding.validate().map_err(|error| {
        ApiError::internal(format!("qualification binding is invalid: {error}"))
    })?;
    let evaluation_id = new_prefixed_id("infeval");
    let resolved_agent_profile_hash = binding.agent_profile_hash.clone();
    state
        .store
        .create_inference_evaluation(CreateInferenceEvaluation {
            id: evaluation_id.clone(),
            suite_id: suite_id.into(),
            suite_hash,
            attempts: request.attempts,
            agent_profile_id: profile.id,
            agent_profile_hash: resolved_agent_profile_hash,
            resolved_binding: binding,
            runtime_revision: state.build.api_revision.clone(),
            actor: request.actor,
            reason: request.reason,
            config_hash: request.config_hash,
        })
        .await?;
    let receipt = match state
        .worker
        .dispatch_inference_evaluation(crate::dispatch::InferenceEvaluationExecutionRequest {
            evaluation_id: evaluation_id.clone(),
            gateway_url: state.inference.gateway_url.clone(),
        })
        .await
    {
        Ok(receipt) => receipt,
        Err(error) => {
            let message = sanitize_failure(&error.to_string());
            let failed = state
                .store
                .fail_inference_evaluation(&evaluation_id, &message)
                .await?;
            return Ok(Json(failed));
        }
    };
    let running = state
        .store
        .mark_inference_evaluation_running(&evaluation_id, &receipt.job_name)
        .await?;
    Ok(Json(running))
}

fn qualification_from_evaluation(
    evaluation: &StoredInferenceEvaluation,
    report: Value,
) -> Result<CreateInferencePolicyQualification, ApiError> {
    validate_qualification_report(evaluation, &report)?;
    let derived_verdict = if report["gate_passed"].as_bool() == Some(true) {
        "passed"
    } else {
        "failed"
    };
    Ok(CreateInferencePolicyQualification {
        id: new_prefixed_id("inferqual"),
        policy_id: evaluation.policy_id.clone(),
        policy_revision: evaluation.policy_revision.clone(),
        policy_hash: evaluation.policy_hash.clone(),
        target_id: evaluation.target_id.clone(),
        target_revision: evaluation.target_revision.clone(),
        target_hash: evaluation.target_hash.clone(),
        agent_profile_id: evaluation.agent_profile_id.clone(),
        agent_profile_hash: evaluation.agent_profile_hash.clone(),
        suite_id: evaluation.suite_id.clone(),
        suite_hash: evaluation.suite_hash.clone(),
        runtime_revision: evaluation.runtime_revision.clone(),
        attempts: evaluation.attempts,
        metrics: report,
        verdict: derived_verdict.into(),
        evidence_artifact_id: None,
        actor: evaluation.actor.clone(),
        reason: evaluation.reason.clone(),
    })
}

async fn list_policy_qualifications(
    State(state): State<AppState>,
    Path((policy_id, revision)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    if state
        .inference
        .registry
        .policy(&policy_id, &revision)
        .is_none()
    {
        return Err(ApiError::not_found(
            "inference_policy_revision",
            &format!("{policy_id}@{revision}"),
        ));
    }
    let qualifications = state
        .store
        .list_inference_policy_qualifications(&policy_id, &revision)
        .await?;
    Ok(Json(json!({"qualifications":qualifications})))
}

async fn get_inference_evaluation(
    State(state): State<AppState>,
    Path(evaluation_id): Path<String>,
) -> Result<Json<StoredInferenceEvaluation>, ApiError> {
    state
        .store
        .get_inference_evaluation(&evaluation_id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::not_found("inference_evaluation", &evaluation_id))
}

async fn internal_inference_evaluation_context(
    State(state): State<AppState>,
    Path(evaluation_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let evaluation = state
        .store
        .get_inference_evaluation(&evaluation_id)
        .await?
        .ok_or_else(|| ApiError::not_found("inference_evaluation", &evaluation_id))?;
    if evaluation.status != "running" {
        return Err(ApiError::conflict("inference evaluation is not running"));
    }
    Ok(Json(json!({
        "schema_version":"pharness.dev/inference-evaluation-context/v1alpha1",
        "evaluation_id":evaluation.id,
        "suite_id":evaluation.suite_id,
        "suite_hash":evaluation.suite_hash,
        "attempts":evaluation.attempts,
        "agent_profile_id":evaluation.agent_profile_id,
        "agent_profile_hash":evaluation.agent_profile_hash,
        "runtime_revision":evaluation.runtime_revision,
        "selection_id":format!("evaluation:{}",evaluation.id),
        "stage_execution_id":format!("evaluation:{}",evaluation.id),
        "resolved_binding":evaluation.resolved_binding,
    })))
}

async fn internal_inference_evaluation_outcome(
    State(state): State<AppState>,
    Path(evaluation_id): Path<String>,
    Json(request): Json<InferenceEvaluationOutcomeRequest>,
) -> Result<Json<StoredInferenceEvaluation>, ApiError> {
    let evaluation = state
        .store
        .get_inference_evaluation(&evaluation_id)
        .await?
        .ok_or_else(|| ApiError::not_found("inference_evaluation", &evaluation_id))?;
    if evaluation.status == "completed" {
        return Ok(Json(evaluation));
    }
    if evaluation.status != "running" {
        return Err(ApiError::conflict(
            "inference evaluation is not accepting an outcome",
        ));
    }
    let report_hash = canonical_json_sha256(&request.report).map_err(|error| {
        ApiError::internal(format!(
            "failed to hash inference evaluation report: {error}"
        ))
    })?;
    let qualification = qualification_from_evaluation(&evaluation, request.report.clone())?;
    let completed = state
        .store
        .complete_inference_evaluation(&evaluation_id, &request.report, &report_hash, qualification)
        .await?;
    Ok(Json(completed))
}

async fn issue_model_grant(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(request): Json<ModelGrantRequest>,
) -> Result<Json<ModelGrantResponse>, ApiError> {
    if !state.inference.enabled {
        return Err(ApiError::unavailable("model gateway is disabled"));
    }
    if request.request_sequence == 0 || !is_hex_sha256(&request.request_body_hash) {
        return Err(ApiError::bad_request(
            "model-grant request sequence or body hash is invalid",
        ));
    }
    let run_id_typed = RunId::new(run_id.clone());
    let Some(run) = state.store.get_run(&run_id_typed).await? else {
        return issue_evaluation_model_grant(&state, &run_id, request).await;
    };
    if !matches!(run.status.as_str(), "running" | "preparing")
        || run.cancel_requested_at.is_some()
        || run.budget_consumption.turns_used >= run.budget_consumption.allowed_turns
        || run.budget_consumption.tokens_used >= run.budget_consumption.allowed_tokens
    {
        return Err(ApiError::conflict(
            "Run is not eligible for another model request",
        ));
    }
    let selection = state
        .store
        .get_stage_inference_selection(&request.selection_id)
        .await?
        .ok_or_else(|| ApiError::not_found("stage_inference_selection", &request.selection_id))?;
    let expected_stage_execution_id = selection
        .stage_execution_id
        .clone()
        .unwrap_or_else(|| format!("standalone:{run_id}"));
    if selection.run_id.as_deref() != Some(&run_id)
        || expected_stage_execution_id != request.stage_execution_id
    {
        return Err(ApiError::conflict(
            "model-grant selection does not match the Run stage",
        ));
    }
    selection.resolved_binding.validate().map_err(|error| {
        ApiError::conflict(format!("stored inference binding is invalid: {error}"))
    })?;
    let target = state
        .inference
        .registry
        .target(&selection.target_id, &selection.target_revision)
        .ok_or_else(|| {
            ApiError::unavailable("selected inference target revision is no longer configured")
        })?;
    let policy = state
        .inference
        .registry
        .policy(&selection.policy_id, &selection.policy_revision)
        .ok_or_else(|| {
            ApiError::unavailable("selected inference policy revision is no longer configured")
        })?;
    if target.config_hash != selection.target_hash || policy.policy_hash != selection.policy_hash {
        return Err(ApiError::conflict(
            "configured inference revision does not match the Run snapshot",
        ));
    }
    let events = state.store.list_events(&run_id_typed).await?;
    let observed_requests = events
        .iter()
        .filter(|event| event.kind == pharness_core::EventKind::ModelRequestStarted)
        .count() as u32;
    if request.request_sequence != observed_requests
        && request.request_sequence != observed_requests.saturating_add(1)
    {
        return Err(ApiError::conflict(
            "model-grant request sequence does not match durable Run progress",
        ));
    }
    let key = state
        .inference
        .grant_hmac_key
        .as_ref()
        .ok_or_else(|| ApiError::unavailable("model-grant signing key is unavailable"))?;
    let now = epoch_seconds();
    let claims = ModelGrantClaims {
        schema_version: MODEL_GRANT_SCHEMA.into(),
        run_id,
        stage_execution_id: request.stage_execution_id,
        selection_id: selection.id,
        target: InferenceTargetRef {
            target_id: selection.target_id,
            revision: selection.target_revision,
        },
        target_hash: selection.target_hash,
        policy: InferencePolicyRef {
            policy_id: selection.policy_id,
            revision: selection.policy_revision,
        },
        policy_hash: selection.policy_hash,
        request_sequence: request.request_sequence,
        request_body_hash: request.request_body_hash,
        nonce: uuid::Uuid::now_v7().simple().to_string(),
        issued_at_epoch_seconds: now,
        expires_at_epoch_seconds: now.saturating_add(60),
    };
    let token = sign_model_grant(&claims, key.expose_secret().as_bytes())
        .map_err(|error| ApiError::internal(format!("failed to issue model grant: {error}")))?;
    state
        .store
        .create_model_grant_issuance(CreateModelGrantIssuance {
            run_id: claims.run_id.clone(),
            request_sequence: claims.request_sequence,
            selection_id: claims.selection_id.clone(),
            request_body_hash: claims.request_body_hash.clone(),
            nonce: claims.nonce.clone(),
            issued_at_epoch_seconds: claims.issued_at_epoch_seconds,
            expires_at_epoch_seconds: claims.expires_at_epoch_seconds,
        })
        .await
        .map_err(|error| match error {
            pharness_store::StoreError::Conflict(message) => ApiError::conflict(message),
            other => ApiError::from(other),
        })?;
    Ok(Json(ModelGrantResponse {
        token,
        expires_at_epoch_seconds: claims.expires_at_epoch_seconds,
    }))
}

async fn issue_evaluation_model_grant(
    state: &AppState,
    fixture_run_id: &str,
    request: ModelGrantRequest,
) -> Result<Json<ModelGrantResponse>, ApiError> {
    let evaluation_id = request
        .selection_id
        .strip_prefix("evaluation:")
        .ok_or_else(|| ApiError::not_found("run", fixture_run_id))?;
    let evaluation = state
        .store
        .get_inference_evaluation(evaluation_id)
        .await?
        .ok_or_else(|| ApiError::not_found("inference_evaluation", evaluation_id))?;
    if evaluation.status != "running"
        || request.stage_execution_id != format!("evaluation:{evaluation_id}")
        || !fixture_run_id.starts_with(&format!("eval-{}-", evaluation.suite_id))
    {
        return Err(ApiError::conflict(
            "model-grant request does not match an active inference evaluation fixture",
        ));
    }
    verify_binding_is_configured(state, &evaluation.resolved_binding)?;
    let key = state
        .inference
        .grant_hmac_key
        .as_ref()
        .ok_or_else(|| ApiError::unavailable("model-grant signing key is unavailable"))?;
    let now = epoch_seconds();
    let claims = ModelGrantClaims {
        schema_version: MODEL_GRANT_SCHEMA.into(),
        run_id: fixture_run_id.into(),
        stage_execution_id: request.stage_execution_id,
        selection_id: request.selection_id,
        target: InferenceTargetRef {
            target_id: evaluation.target_id.clone(),
            revision: evaluation.target_revision.clone(),
        },
        target_hash: evaluation.target_hash.clone(),
        policy: InferencePolicyRef {
            policy_id: evaluation.policy_id.clone(),
            revision: evaluation.policy_revision.clone(),
        },
        policy_hash: evaluation.policy_hash.clone(),
        request_sequence: request.request_sequence,
        request_body_hash: request.request_body_hash,
        nonce: uuid::Uuid::now_v7().simple().to_string(),
        issued_at_epoch_seconds: now,
        expires_at_epoch_seconds: now.saturating_add(60),
    };
    let token = sign_model_grant(&claims, key.expose_secret().as_bytes())
        .map_err(|error| ApiError::internal(format!("failed to issue model grant: {error}")))?;
    state
        .store
        .create_inference_evaluation_grant_issuance(CreateInferenceEvaluationGrantIssuance {
            evaluation_id: evaluation.id,
            fixture_run_id: claims.run_id.clone(),
            request_sequence: claims.request_sequence,
            request_body_hash: claims.request_body_hash.clone(),
            nonce: claims.nonce.clone(),
            issued_at_epoch_seconds: claims.issued_at_epoch_seconds,
            expires_at_epoch_seconds: claims.expires_at_epoch_seconds,
        })
        .await
        .map_err(|error| match error {
            pharness_store::StoreError::Conflict(message) => ApiError::conflict(message),
            other => ApiError::from(other),
        })?;
    Ok(Json(ModelGrantResponse {
        token,
        expires_at_epoch_seconds: claims.expires_at_epoch_seconds,
    }))
}

async fn verify_target_protocol(
    state: &AppState,
    verification_id: &str,
    target: &pharness_core::InferenceTargetRevision,
    policy: &pharness_core::StageInferencePolicyRevision,
) -> Result<(), String> {
    if !state.inference.enabled {
        return Err("model gateway is disabled".into());
    }
    let key = state
        .inference
        .grant_hmac_key
        .as_ref()
        .ok_or_else(|| "model-grant signing key is unavailable".to_string())?;
    let base = url::Url::parse(&state.inference.gateway_url)
        .map_err(|error| format!("gateway URL is invalid: {error}"))?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| format!("gateway client failed: {error}"))?;
    let ready: Value = client
        .get(base.join("readyz").map_err(|error| error.to_string())?)
        .send()
        .await
        .map_err(|error| format!("gateway readiness failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("gateway readiness failed: {error}"))?
        .json()
        .await
        .map_err(|error| format!("gateway readiness response was invalid: {error}"))?;
    if ready.get("registry_hash").and_then(Value::as_str)
        != Some(state.inference.registry.config_hash.as_str())
    {
        return Err("gateway registry hash does not match the API".into());
    }
    let models: Value = client
        .get(base.join("models").map_err(|error| error.to_string())?)
        .send()
        .await
        .map_err(|error| format!("gateway model visibility failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("gateway model visibility failed: {error}"))?
        .json()
        .await
        .map_err(|error| format!("gateway model visibility response was invalid: {error}"))?;
    let target_alias = format!("{}@{}", target.target_id, target.revision);
    if !models["data"].as_array().is_some_and(|models| {
        models
            .iter()
            .any(|model| model.get("id").and_then(Value::as_str) == Some(target_alias.as_str()))
    }) {
        return Err("configured target alias is not visible through the gateway".into());
    }
    let request = ModelRequest {
        session_id: SessionId::new(format!("verify_{verification_id}")),
        run_id: RunId::new(format!("verify_{verification_id}")),
        messages: vec![
            ModelMessage::system("Call verification_complete exactly once."),
            ModelMessage::user("Verify native streaming tool compatibility."),
        ],
        tools: vec![ToolSpec::new(
            "verification_complete",
            "Complete the protocol verification.",
            json!({"type":"object","properties":{},"additionalProperties":false}),
            CapabilityKind::AgentControl,
        )],
        mode: ToolProtocolMode::NativeTools,
        temperature: policy.temperature().unwrap_or(0.0),
        max_tokens: policy.max_output_tokens.min(256),
        reasoning: Some(policy.reasoning.clone()),
    };
    let mut wire = build_chat_request(
        target.backend_kind,
        format!("{}@{}", target.target_id, target.revision),
        request,
        policy,
        target.capabilities.stream_options,
        target
            .openrouter
            .as_ref()
            .map(|route| route.provider_slug.as_str()),
    );
    wire.max_tokens = policy.max_output_tokens;
    let value = serde_json::to_value(&wire).map_err(|error| error.to_string())?;
    let request_body_hash = canonical_json_sha256(&value).map_err(|error| error.to_string())?;
    let now = epoch_seconds();
    let claims = ModelGrantClaims {
        schema_version: MODEL_GRANT_SCHEMA.into(),
        run_id: format!("verify_{verification_id}"),
        stage_execution_id: format!("verify_{verification_id}"),
        selection_id: format!("verify_{verification_id}"),
        target: InferenceTargetRef {
            target_id: target.target_id.clone(),
            revision: target.revision.clone(),
        },
        target_hash: target.config_hash.clone(),
        policy: InferencePolicyRef {
            policy_id: policy.policy_id.clone(),
            revision: policy.revision.clone(),
        },
        policy_hash: policy.policy_hash.clone(),
        request_sequence: 1,
        request_body_hash,
        nonce: uuid::Uuid::now_v7().simple().to_string(),
        issued_at_epoch_seconds: now,
        expires_at_epoch_seconds: now.saturating_add(60),
    };
    let token = sign_model_grant(&claims, key.expose_secret().as_bytes())
        .map_err(|error| error.to_string())?;
    let response = timeout(
        Duration::from_secs(target.transport.first_response_timeout_seconds),
        client
            .post(
                base.join("chat/completions")
                    .map_err(|error| error.to_string())?,
            )
            .bearer_auth(token)
            .json(&wire)
            .send(),
    )
    .await
    .map_err(|_| "gateway protocol verification timed out".to_string())?
    .map_err(|error| format!("gateway protocol verification failed: {error}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "gateway protocol verification returned {status}: {}",
            body.chars().take(500).collect::<String>()
        ));
    }
    let mut decoder = SseDecoder::default();
    let mut aggregate = OpenAiStreamAggregate::default();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = timeout(
        Duration::from_secs(target.transport.stream_idle_timeout_seconds),
        stream.next(),
    )
    .await
    .map_err(|_| "gateway verification stream became idle".to_string())?
    {
        let chunk =
            chunk.map_err(|error| format!("gateway verification stream failed: {error}"))?;
        let text = std::str::from_utf8(&chunk)
            .map_err(|error| format!("gateway verification stream was not UTF-8: {error}"))?;
        for payload in decoder.push_str(text) {
            let chunk: StreamChunk = serde_json::from_str(&payload)
                .map_err(|error| format!("gateway verification SSE was invalid: {error}"))?;
            aggregate.push_chunk(chunk);
        }
    }
    for payload in decoder.finish() {
        let chunk: StreamChunk = serde_json::from_str(&payload)
            .map_err(|error| format!("gateway verification SSE was invalid: {error}"))?;
        aggregate.push_chunk(chunk);
    }
    if !aggregate.usable_stream_data
        || aggregate.tool_calls.len() != 1
        || aggregate.tool_calls[0].name.as_deref() != Some("verification_complete")
    {
        return Err("provider did not return exactly one expected native tool call".into());
    }
    Ok(())
}

fn validate_configuration_action(
    state: &AppState,
    request: &ConfigurationActionRequest,
) -> Result<(), ApiError> {
    validate_configuration_hash(state, &request.config_hash)?;
    validate_actor_reason(&request.actor, &request.reason)
}

fn validate_configuration_hash(state: &AppState, config_hash: &str) -> Result<(), ApiError> {
    if config_hash != state.inference.registry.config_hash {
        return Err(ApiError::conflict(
            "inference configuration changed; refresh and review the current registry",
        ));
    }
    Ok(())
}

fn validate_actor_reason(actor: &str, reason: &str) -> Result<(), ApiError> {
    if actor.trim().is_empty() || reason.trim().is_empty() {
        return Err(ApiError::bad_request("actor and reason are required"));
    }
    Ok(())
}

fn parse_stage(value: &str) -> Result<InferenceStage, ApiError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "onboarding" => Ok(InferenceStage::Onboarding),
        "plan" | "planner" => Ok(InferenceStage::Plan),
        "implement" | "builder" => Ok(InferenceStage::Implement),
        "test" | "tester" => Ok(InferenceStage::Test),
        "verify" | "verifier" => Ok(InferenceStage::Verify),
        _ => Err(ApiError::bad_request("unsupported inference stage")),
    }
}

#[cfg(test)]
fn qualification_suite_contract(
    suite_id: &str,
) -> Result<(InferenceStage, &'static str), ApiError> {
    match suite_id {
        "onboarding-v1" => Ok((InferenceStage::Onboarding, "repository-onboarding-proposer")),
        "planner-v1" => Ok((InferenceStage::Plan, "repo-planner")),
        "coding-v1" => Ok((InferenceStage::Implement, "repo-builder")),
        "tester-v1" => Ok((InferenceStage::Test, "repo-tester")),
        "verifier-v1" => Ok((InferenceStage::Verify, "repo-verifier")),
        _ => Err(ApiError::bad_request(
            "unsupported stage inference qualification suite",
        )),
    }
}

fn qualification_contract_for_stage(
    stage: InferenceStage,
) -> Result<(&'static str, &'static str), ApiError> {
    match stage {
        InferenceStage::Onboarding => Ok(("onboarding-v1", "repository-onboarding-proposer")),
        InferenceStage::Plan => Ok(("planner-v1", "repo-planner")),
        InferenceStage::Implement => Ok(("coding-v1", "repo-builder")),
        InferenceStage::Test => Ok(("tester-v1", "repo-tester")),
        InferenceStage::Verify => Ok(("verifier-v1", "repo-verifier")),
    }
}

fn validate_qualification_report(
    evaluation: &StoredInferenceEvaluation,
    report: &Value,
) -> Result<(), ApiError> {
    let metrics = report.as_object().ok_or_else(|| {
        ApiError::bad_request("qualification metrics must be a structured evaluation report")
    })?;
    let exact = [
        ("suite_id", evaluation.suite_id.as_str()),
        ("suite_hash", evaluation.suite_hash.as_str()),
        ("runtime_revision", evaluation.runtime_revision.as_str()),
        ("target_id", evaluation.target_id.as_str()),
        ("target_revision", evaluation.target_revision.as_str()),
        ("target_hash", evaluation.target_hash.as_str()),
        ("policy_id", evaluation.policy_id.as_str()),
        ("policy_revision", evaluation.policy_revision.as_str()),
        ("policy_hash", evaluation.policy_hash.as_str()),
        ("profile_hash", evaluation.agent_profile_hash.as_str()),
        ("binding_hash", evaluation.binding_hash.as_str()),
        (
            "prompt_version",
            evaluation.resolved_binding.prompt_version.as_str(),
        ),
        (
            "tool_schema_hash",
            evaluation.resolved_binding.tool_schema_hash.as_str(),
        ),
    ];
    if metrics.get("schema_version").and_then(Value::as_str)
        != Some("pharness.dev/inference-evaluation/v1alpha1")
        || exact
            .iter()
            .any(|(key, expected)| metrics.get(*key).and_then(Value::as_str) != Some(*expected))
        || metrics.get("attempts").and_then(Value::as_u64) != Some(u64::from(evaluation.attempts))
        || metrics
            .get("gate_passed")
            .and_then(Value::as_bool)
            .is_none()
    {
        return Err(ApiError::conflict(
            "qualification evaluation metadata does not match the exact suite, runtime, target, policy, profile, or attempts",
        ));
    }
    Ok(())
}

fn is_hex_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
fn is_prefixed_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(is_hex_sha256)
}

fn epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn sanitize_failure(message: &str) -> String {
    let message = message
        .replace("Authorization", "credential")
        .replace("Bearer", "credential");
    message.chars().take(1000).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_query_is_bounded() {
        assert_eq!(parse_stage("builder").unwrap(), InferenceStage::Implement);
        assert!(parse_stage("release").is_err());
    }

    #[test]
    fn body_hash_requires_exact_hex() {
        assert!(is_hex_sha256(&"a".repeat(64)));
        assert!(!is_hex_sha256("sha256:abc"));
        assert!(is_prefixed_sha256(&format!("sha256:{}", "b".repeat(64))));
    }

    #[test]
    fn qualification_metrics_are_bound_to_every_immutable_input() {
        let registry = pharness_config::InferenceGatewayConfig::legacy_default().registry;
        let target = registry.targets[0].clone();
        let policy = registry.policies[0].clone();
        let mut report = json!({
            "schema_version":"pharness.dev/inference-evaluation/v1alpha1",
            "suite_id":"planner-v1",
            "suite_hash":format!("sha256:{}", "a".repeat(64)),
            "runtime_revision":"runtime",
            "target_id":"target",
            "target_revision":"v1",
            "target_hash":"target",
            "policy_id":"policy",
            "policy_revision":"v1",
            "policy_hash":"policy",
            "profile_hash":"profile",
            "binding_hash":"binding",
            "prompt_version":"test",
            "tool_schema_hash":format!("sha256:{}", "b".repeat(64)),
            "attempts":2,
            "gate_passed":true,
        });
        let evaluation = StoredInferenceEvaluation {
            id: "infeval_test".into(),
            status: "running".into(),
            suite_id: "planner-v1".into(),
            suite_hash: format!("sha256:{}", "a".repeat(64)),
            attempts: 2,
            agent_profile_id: "repo-planner".into(),
            agent_profile_hash: "profile".into(),
            target_id: "target".into(),
            target_revision: "v1".into(),
            target_hash: "target".into(),
            policy_id: "policy".into(),
            policy_revision: "v1".into(),
            policy_hash: "policy".into(),
            resolved_binding: serde_json::from_value(json!({
                "schema_version":RESOLVED_INFERENCE_BINDING_SCHEMA,
                "target":target,
                "policy":policy,
                "prompt_version":"test",
                "base_agent_profile_hash":format!("sha256:{}", "e".repeat(64)),
                "agent_profile_hash":"profile",
                "tool_schema_hash":format!("sha256:{}", "b".repeat(64)),
                "profile_budget_hash":format!("sha256:{}", "c".repeat(64)),
                "binding_hash":format!("sha256:{}", "d".repeat(64))
            }))
            .unwrap(),
            binding_hash: "binding".into(),
            runtime_revision: "runtime".into(),
            actor: "operator".into(),
            reason: "qualify planner policy".into(),
            config_hash: "registry".into(),
            job_name: None,
            report: None,
            report_hash: None,
            failure: None,
            qualification_id: None,
            created_at: "now".into(),
            started_at: Some("now".into()),
            finished_at: None,
        };
        assert!(validate_qualification_report(&evaluation, &report).is_ok());
        report["policy_hash"] = json!("stale");
        assert!(validate_qualification_report(&evaluation, &report).is_err());
    }

    #[test]
    fn every_qualification_suite_has_one_stage_profile_contract() {
        assert_eq!(
            qualification_suite_contract("onboarding-v1").unwrap(),
            (InferenceStage::Onboarding, "repository-onboarding-proposer")
        );
        assert_eq!(
            qualification_suite_contract("coding-v1").unwrap(),
            (InferenceStage::Implement, "repo-builder")
        );
        assert!(qualification_suite_contract("release-v1").is_err());
    }
}
