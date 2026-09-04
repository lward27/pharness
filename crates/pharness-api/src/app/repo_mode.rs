use super::approvals::create_permission_grant_record;
use super::clock::current_millis;
use super::hashing::canonical_material_hash;
use super::identifiers::{is_git_sha, new_prefixed_id};
use super::products::ensure_repo_mode_enabled;
use super::system::capability_verification_summary;
use super::validation::required_text;
use super::{ApiError, AppState};
use crate::dispatch::{SourceDeliveryExecutionRequest, SourceDeliveryObservationRequest};
use crate::dto::{
    CreatePermissionGrantRequest, DeliverySegmentResourceResponse, DeliverySegmentResponse,
    GitDeliveryContextResponse, GitDeliveryObservationContextResponse,
    GitDeliveryObservationOutcomeRequest, GitDeliveryOutcomeRequest, ReconcileWorkItemResponse,
    WorkItemActionResponse, WorkItemFlowResponse,
};
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pharness_core::{
    AgentEvent, EventId, EventKind, RunBudgetConsumption, RunId, RunScope, SessionId,
};
use pharness_store::{
    ChangeSetListFilter, CreateAgentContextPack, CreateAuditEvent, CreateCapabilityVerification,
    CreateEnvironmentPreparation, CreateEvidenceValidation, CreateOperatorAnnotation,
    CreateOperatorAnnotationDecision, CreateProviderCheckSetObservation, CreateRepoWorkItem,
    CreateRun, CreateSession, CreateSourceDeliveryIntent, CreateStageChainAuthorization,
    CreateStageExecution, CreateWorkspace, RunListFilter, SealStageOutcome, StoredBudgetExtension,
    StoredChangeSet, StoredOperatorAnnotation, StoredOperatorAnnotationDecision,
    StoredRepoWorkItemMetadata, StoredRun, StoredSourceDeliveryIntent, StoredStageOutcome,
    UpdateEnvironmentPreparation, UpdateWorkspaceExecution, WorkPlanListFilter,
    WorkspaceListFilter,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

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
        .route(
            "/api/work-items/:work_item_id/evidence",
            get(list_work_item_evidence),
        )
        .route(
            "/api/evidence-validations/:evidence_validation_id",
            get(get_evidence_validation),
        )
}

pub(in crate::app) async fn is_repo_work_item(
    state: &AppState,
    work_item_id: &str,
) -> Result<bool, ApiError> {
    Ok(state
        .store
        .get_repo_work_item_metadata(work_item_id)
        .await?
        .is_some())
}

pub(in crate::app) async fn repo_work_item_flow(
    state: &AppState,
    work_item_id: &str,
) -> Result<WorkItemFlowResponse, ApiError> {
    ensure_repo_mode_enabled(state)?;
    let metadata = repo_metadata(state, work_item_id).await?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let work_plan = state.store.get_work_plan_by_work_item(work_item_id).await?;
    let change_set = match work_plan.as_ref() {
        Some(plan) => state.store.get_change_set_by_work_plan(&plan.id).await?,
        None => None,
    };
    let source_delivery_intent = match change_set.as_ref() {
        Some(change_set) => {
            state
                .store
                .get_source_delivery_intent_by_subject("work_item_change_set", &change_set.id)
                .await?
        }
        None => None,
    };
    let executions = state.store.list_stage_executions(work_item_id).await?;
    let outcomes = state
        .store
        .list_effective_stage_outcomes(work_item_id)
        .await?;
    let all_outcomes = state.store.list_stage_outcomes(work_item_id).await?;
    let work_plan_history = state
        .store
        .list_work_plans(WorkPlanListFilter {
            work_item_id: Some(work_item_id.into()),
            limit: 200,
            ..WorkPlanListFilter::default()
        })
        .await?;
    let change_set_history = state
        .store
        .list_change_sets(ChangeSetListFilter {
            work_item_id: Some(work_item_id.into()),
            limit: 200,
            ..ChangeSetListFilter::default()
        })
        .await?;
    let run_history = state
        .store
        .list_runs(RunListFilter {
            work_item_id: Some(work_item_id.into()),
            limit: 200,
            ..RunListFilter::default()
        })
        .await?;
    let product = state.store.get_product(&metadata.product_id).await?;
    let repository = state.store.get_repository(&metadata.repository_id).await?;
    let binding = state
        .store
        .get_repository_binding(&metadata.product_id, &metadata.repository_id)
        .await?;
    let binding_revision = match binding.as_ref() {
        Some(binding) => {
            state
                .store
                .get_repository_binding_revision(&binding.current_revision_id)
                .await?
        }
        None => None,
    };
    let service_ids = binding_revision
        .as_ref()
        .map(|revision| {
            revision
                .service_ids
                .iter()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
        })
        .unwrap_or_default();
    let services = state
        .store
        .list_product_services(&metadata.product_id)
        .await?
        .into_iter()
        .filter(|service| service_ids.contains(&service.id))
        .collect::<Vec<_>>();
    let chain = state
        .store
        .active_stage_chain_authorization(work_item_id)
        .await?;
    let inference_selections = state
        .store
        .list_stage_inference_selections("work_item", work_item_id)
        .await?;
    let inference_selection_views = inference_selections
        .iter()
        .map(|selection| {
            json!({
                "id":selection.id,
                "stage_key":selection.stage_key,
                "stage_execution_id":selection.stage_execution_id,
                "run_id":selection.run_id,
                "supersedes_selection_id":selection.supersedes_selection_id,
                "created_at":selection.created_at,
                "binding":super::inference::sanitized_binding(&selection.resolved_binding),
            })
        })
        .collect::<Vec<_>>();
    let agent_execution_selections = state
        .store
        .list_agent_execution_selections("work_item", work_item_id)
        .await?;
    let agent_execution_selection_views = agent_execution_selections
        .iter()
        .map(|selection| {
            json!({
                "id":selection.id,
                "stage_key":selection.stage_key,
                "stage_execution_id":selection.stage_execution_id,
                "run_id":selection.run_id,
                "supersedes_selection_id":selection.supersedes_selection_id,
                "created_at":selection.created_at,
                "binding":selection.resolved_binding,
                "binding_hash":selection.binding_hash,
            })
        })
        .collect::<Vec<_>>();
    let mut execution_views = Vec::with_capacity(executions.len());
    for execution in &executions {
        let mut value = serde_json::to_value(execution)
            .map_err(|error| ApiError::internal(error.to_string()))?;
        if let Some(object) = value.as_object_mut() {
            let provenance = match execution.run_id.as_ref() {
                Some(run_id) => {
                    super::agent_hosts::sanitized_run_agent_execution(state, run_id).await?
                }
                None => None,
            };
            object.insert("agent_execution".into(), provenance.unwrap_or(Value::Null));
        }
        execution_views.push(value);
    }
    let correction_lineage = executions
        .iter()
        .filter_map(|execution| {
            let correction_of = execution.input_snapshot.get("correction_of");
            let diagnosis_of = execution.input_snapshot.get("diagnosis_of");
            if correction_of.is_none() && diagnosis_of.is_none() {
                return None;
            }
            Some(json!({
                "stage_execution_id":execution.id,
                "stage_key":execution.stage_key,
                "run_id":execution.run_id,
                "profile_id":execution.agent_profile_id,
                "correction_of":correction_of,
                "diagnosis_of":diagnosis_of,
            }))
        })
        .collect::<Vec<_>>();
    let internal_corrections_used = executions
        .iter()
        .filter(|execution| execution.stage_key == "implement")
        .count()
        .saturating_sub(1);
    let deterministic_test = chain
        .as_ref()
        .and_then(|authorization| {
            authorization
                .budget_chain
                .get("deterministic_test")
                .and_then(Value::as_bool)
        })
        .unwrap_or(state.repo_mode.coding_reliability_v2_enabled);
    let max_internal_corrections = chain
        .as_ref()
        .and_then(|authorization| {
            authorization
                .budget_chain
                .get("max_internal_corrections")
                .and_then(Value::as_u64)
        })
        .unwrap_or(u64::from(state.repo_mode.coding_reliability_v2_enabled));
    let annotations = state.store.list_operator_annotations(work_item_id).await?;
    let annotation_decisions = state
        .store
        .list_operator_annotation_decisions(work_item_id)
        .await?;
    let pending_annotation_effects =
        pending_annotation_effects(&annotations, &annotation_decisions);
    let action_run_id =
        repo_action_run_id(&metadata, &executions, work_item.current_run_id.as_ref());
    let current_run = match action_run_id {
        Some(run_id) => state.store.get_run(run_id).await?,
        None => None,
    };
    let pending_budget_extension = match current_run.as_ref() {
        Some(run) => {
            state
                .store
                .pending_budget_extension_for_run(&run.id)
                .await?
        }
        None => None,
    };
    let retryable_budget_extension = match current_run.as_ref().filter(|run| {
        run.status == "failed"
            && run
                .error
                .as_deref()
                .is_some_and(|error| error.starts_with("failed to launch worker job:"))
            && run
                .result_json
                .as_ref()
                .and_then(|result| result.get("budget_extension"))
                .is_some()
    }) {
        Some(run) => {
            state
                .store
                .latest_approved_budget_extension_for_run(&run.id)
                .await?
        }
        None => None,
    };
    let action_rail = derive_repo_actions(
        &metadata,
        RepoActionInputs {
            attempts: (work_item.attempt_count, work_item.max_attempts),
            work_plan: work_plan.as_ref(),
            change_set: change_set.as_ref(),
            source_delivery_intent: source_delivery_intent.as_ref(),
            executions: &executions,
            chain: chain.as_ref(),
            pending_annotation_effects: &pending_annotation_effects,
            pending_budget_extension: pending_budget_extension.as_ref(),
            current_run: current_run.as_ref(),
            retryable_budget_extension: retryable_budget_extension.as_ref(),
        },
    )?;
    let workspaces = state
        .store
        .list_workspaces(WorkspaceListFilter {
            work_item_id: Some(work_item_id.into()),
            limit: 100,
            ..WorkspaceListFilter::default()
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let audit_events: Vec<crate::dto::AuditEventResponse> = state
        .store
        .list_audit_events(Some("work_item"), Some(work_item_id), None, 100)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    let first_action = action_rail.first();
    let safe_advance = first_action
        .map(|action| {
            let eligible = action.status == "ready"
                && !action.approval_required
                && action.effect_class == "controller_internal";
            json!({
                "eligible": eligible,
                "action_id": eligible.then_some(action.id.as_str()),
                "state_hash": eligible.then_some(action.state_hash.as_str()),
                "summary": if eligible {
                    action.external_effect_summary.as_str()
                } else {
                    "No safe internal Repo Mode action is currently eligible"
                },
                "blockers": if eligible {
                    Vec::<String>::new()
                } else {
                    vec!["The next boundary requires human review, model execution, or an external effect".to_string()]
                },
            })
        })
        .unwrap_or_else(|| {
            json!({
                "eligible": false,
                "action_id": Value::Null,
                "state_hash": Value::Null,
                "summary": "No controller action is currently available",
                "blockers": ["The WorkItem is waiting or closed"],
            })
        });
    let work_item_response: crate::dto::WorkItemResponse =
        crate::dto::WorkItemResponse::from(work_item.clone()).with_repo_metadata(&metadata);
    let reconcile_preview = ReconcileWorkItemResponse {
        action: first_action
            .map(|action| action.id.clone())
            .unwrap_or_else(|| "wait".into()),
        applied: false,
        work_item: work_item_response.clone(),
        work_plan: work_plan.clone().map(Into::into),
        workspace: workspaces.last().cloned(),
        run: None,
        change_set: change_set.clone().map(Into::into),
        git_delivery_preflight: None,
        pipeline_intent: None,
        pipeline_execution_preflight: None,
        deployment_intent: None,
        deployment_execution_preflight: None,
        deployment_delivery: None,
        gitops_change_set: None,
        gitops_delivery: None,
        gitops_delivery_preflight: None,
        controller_wait: None,
        message: first_action
            .map(|action| action.external_effect_summary.clone())
            .unwrap_or_else(|| {
                "Repo Mode is waiting for the current stage or external boundary".into()
            }),
        boundary: first_action
            .map(|action| action.lifecycle_stage.clone())
            .unwrap_or_else(|| work_item.status.clone()),
        can_apply: first_action.is_some_and(|action| action.status == "ready"),
        effect_summary: first_action
            .map(|action| action.external_effect_summary.clone())
            .unwrap_or_else(|| "No controller action is currently available".into()),
        blockers: first_action
            .map(|action| action.blockers.clone())
            .unwrap_or_default(),
        authorization_checks: Vec::new(),
    };
    Ok(WorkItemFlowResponse {
        work_item: work_item_response,
        reconcile_preview,
        sdlc_flow: None,
        delivery_segments: repo_delivery_segments(&executions, &outcomes),
        workspaces: workspaces.clone(),
        controller_waits: Vec::new(),
        audit_events: audit_events.clone(),
        action_rail,
        delivery_configuration: json!({
            "kind":"repo_mode_source_only",
            "repository_id":metadata.repository_id,
            "source_commit":work_item.source_commit,
            "release":"inapplicable",
            "observe":"inapplicable",
        }),
        repo_mode: Some(json!({
            "metadata":metadata,
            "state_hash":repo_work_item_state_hash(&metadata)?,
            "ownership":{
                "product":product,
                "repository":repository,
                "repository_binding":binding,
                "repository_binding_revision":binding_revision,
                "services":services,
            },
            "stage_executions":execution_views,
            "lifecycle_timeline":super::lifecycle_timeline::project(
                &executions, &all_outcomes, &outcomes, source_delivery_intent.as_ref(),
                metadata.current_stage_execution_id.as_deref(), metadata.closed_at.as_deref(),
                super::clock::current_millis().to_string(),
            ),
            "effective_stage_outcomes":outcomes,
            "history":{
                "stage_outcomes":all_outcomes,
                "work_plans":work_plan_history,
                "change_sets":change_set_history,
                "runs":run_history,
                "workspaces":workspaces,
                "audit_events":audit_events,
            },
            "stage_chain_authorization":chain,
            "operator_annotations":annotations,
            "operator_annotation_decisions":annotation_decisions,
            "product_model_snapshot":{
                "id":metadata.product_model_snapshot_id,
                "hash":metadata.product_model_snapshot_hash,
            },
            "repository_contract_version_id":metadata.repository_contract_version_id,
            "change_set":change_set,
            "source_delivery_intent":source_delivery_intent,
            "coding_reliability":{
                "enabled":state.repo_mode.coding_reliability_v2_enabled,
                "deterministic_test":deterministic_test,
                "max_internal_corrections":max_internal_corrections,
                "internal_corrections_used":internal_corrections_used,
                "correction_lineage":correction_lineage,
                "inference_selections":inference_selection_views,
                "agent_execution_selections":agent_execution_selection_views,
            },
            "safe_advance":safe_advance,
        })),
    })
}

fn pending_annotation_effects<'a>(
    annotations: &'a [StoredOperatorAnnotation],
    decisions: &[StoredOperatorAnnotationDecision],
) -> Vec<&'a StoredOperatorAnnotation> {
    annotations
        .iter()
        .filter(|annotation| annotation.requested_effect != "add_context")
        .filter(|annotation| {
            !decisions
                .iter()
                .any(|decision| decision.annotation_id == annotation.id)
        })
        .collect()
}

struct RepoActionInputs<'a> {
    attempts: (u32, u32),
    work_plan: Option<&'a pharness_store::StoredWorkPlan>,
    change_set: Option<&'a StoredChangeSet>,
    source_delivery_intent: Option<&'a StoredSourceDeliveryIntent>,
    executions: &'a [pharness_store::StoredStageExecution],
    chain: Option<&'a pharness_store::StoredStageChainAuthorization>,
    pending_annotation_effects: &'a [&'a StoredOperatorAnnotation],
    pending_budget_extension: Option<&'a StoredBudgetExtension>,
    current_run: Option<&'a StoredRun>,
    retryable_budget_extension: Option<&'a StoredBudgetExtension>,
}

fn repo_action_run_id<'a>(
    metadata: &StoredRepoWorkItemMetadata,
    executions: &'a [pharness_store::StoredStageExecution],
    fallback_run_id: Option<&'a RunId>,
) -> Option<&'a RunId> {
    metadata
        .current_stage_execution_id
        .as_deref()
        .and_then(|current_execution_id| {
            executions
                .iter()
                .find(|execution| execution.id == current_execution_id)
                .and_then(|execution| execution.run_id.as_ref())
        })
        .or(fallback_run_id)
}

fn rejected_change_set_precedes_work_plan(
    change_set: &StoredChangeSet,
    work_plan: &pharness_store::StoredWorkPlan,
    source_delivery_intent: Option<&StoredSourceDeliveryIntent>,
) -> bool {
    change_set.status == "rejected"
        && source_delivery_intent.is_none()
        && change_set
            .change_set_json
            .pointer("/work_plan/id")
            .and_then(Value::as_str)
            == Some(work_plan.id.as_str())
        && change_set
            .change_set_json
            .pointer("/work_plan/revision")
            .and_then(Value::as_i64)
            .is_some_and(|revision| revision < work_plan.revision)
}

fn source_writer_failure_is_retryable(intent: &StoredSourceDeliveryIntent) -> bool {
    intent.status == "failed"
        && intent.pull_request.is_none()
        && matches!(
            intent.status_reason.as_deref(),
            Some(
                "git_push_authentication_failed"
                    | "git_push_permission_denied"
                    | "git_push_transport_failed"
            )
        )
}

#[derive(Debug, PartialEq, Eq)]
struct ChangeSetProvenanceRepair<'a> {
    material_builder_run_id: &'a str,
    verification_run_id: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
enum ChangeSetOutcomeBinding {
    Current,
    HistoricalVerifier { id: String, hash: String },
}

fn validate_change_set_outcome_binding(
    material_outcomes: &[Value],
    effective_outcomes: &[StoredStageOutcome],
) -> Result<ChangeSetOutcomeBinding, ApiError> {
    let material_matches = |outcome: &StoredStageOutcome| {
        material_outcomes.iter().any(|material| {
            material.get("id").and_then(Value::as_str) == Some(outcome.id.as_str())
                && material.get("stage").and_then(Value::as_str) == Some(outcome.stage_key.as_str())
                && material.get("hash").and_then(Value::as_str)
                    == Some(outcome.content_hash.as_str())
                && material.get("status").and_then(Value::as_str) == Some(outcome.status.as_str())
        })
    };
    for outcome in effective_outcomes
        .iter()
        .filter(|outcome| outcome.stage_key != "verify")
    {
        if !material_matches(outcome) {
            return Err(ApiError::conflict(
                "ChangeSet material does not bind the current effective stage outcomes",
            ));
        }
    }
    let current_verify = effective_outcomes
        .iter()
        .find(|outcome| outcome.stage_key == "verify" && outcome.status == "succeeded")
        .ok_or_else(|| ApiError::conflict("effective Verify outcome is unavailable"))?;
    let unmatched_material = material_outcomes
        .iter()
        .filter(|material| {
            !effective_outcomes.iter().any(|outcome| {
                material.get("id").and_then(Value::as_str) == Some(outcome.id.as_str())
                    && material.get("stage").and_then(Value::as_str)
                        == Some(outcome.stage_key.as_str())
                    && material.get("hash").and_then(Value::as_str)
                        == Some(outcome.content_hash.as_str())
                    && material.get("status").and_then(Value::as_str)
                        == Some(outcome.status.as_str())
            })
        })
        .collect::<Vec<_>>();
    if material_matches(current_verify) {
        if !unmatched_material.is_empty() {
            return Err(ApiError::conflict(
                "ChangeSet material contains stale outcomes outside the current effective set",
            ));
        }
        return Ok(ChangeSetOutcomeBinding::Current);
    }
    if unmatched_material.len() != 1
        || unmatched_material[0].get("stage").and_then(Value::as_str) != Some("verify")
        || unmatched_material[0].get("status").and_then(Value::as_str) != Some("succeeded")
    {
        return Err(ApiError::conflict(
            "ChangeSet material has ambiguous historical Verify outcome provenance",
        ));
    }
    let historical = unmatched_material[0];
    let id = historical
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("historical Verify outcome ID is unavailable"))?;
    let hash = historical
        .get("hash")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("historical Verify outcome hash is unavailable"))?;
    Ok(ChangeSetOutcomeBinding::HistoricalVerifier {
        id: id.to_string(),
        hash: hash.to_string(),
    })
}

fn change_set_provenance_repair(
    change_set: &StoredChangeSet,
) -> Option<ChangeSetProvenanceRepair<'_>> {
    if change_set.status != "approved" {
        return None;
    }
    let material_builder_run_id = change_set
        .change_set_json
        .pointer("/source_provenance/run_id")
        .and_then(Value::as_str)?;
    let verification_run_id = change_set
        .change_set_json
        .pointer("/verification_run_id")
        .and_then(Value::as_str)?;
    if change_set.run_id.as_ref().map(RunId::as_str) == Some(material_builder_run_id) {
        return None;
    }
    Some(ChangeSetProvenanceRepair {
        material_builder_run_id,
        verification_run_id,
    })
}

fn derive_repo_actions(
    metadata: &StoredRepoWorkItemMetadata,
    inputs: RepoActionInputs<'_>,
) -> Result<Vec<WorkItemActionResponse>, ApiError> {
    let RepoActionInputs {
        attempts,
        work_plan,
        change_set,
        source_delivery_intent,
        executions,
        chain,
        pending_annotation_effects,
        pending_budget_extension,
        current_run,
        retryable_budget_extension,
    } = inputs;
    let change_set = change_set.filter(|change_set| {
        !work_plan.is_some_and(|work_plan| {
            rejected_change_set_precedes_work_plan(change_set, work_plan, source_delivery_intent)
        })
    });
    let (attempt_count, max_attempts) = attempts;
    if metadata.closed_at.is_some() {
        return Ok(Vec::new());
    }
    let state_hash = repo_work_item_state_hash(metadata)?;
    if let Some(extension) = pending_budget_extension {
        let (turn_increment, token_increment) = current_run
            .filter(|run| run.id == extension.run_id)
            .map(|run| {
                (
                    extension.turn_increment.min(
                        run.run_budget
                            .hard_turns
                            .saturating_sub(run.budget_consumption.allowed_turns),
                    ),
                    extension.token_increment.min(
                        run.run_budget
                            .hard_tokens
                            .saturating_sub(run.budget_consumption.allowed_tokens),
                    ),
                )
            })
            .unwrap_or((extension.turn_increment, extension.token_increment));
        return Ok(vec![WorkItemActionResponse {
            id: "approve_budget_extension".into(),
            lifecycle_stage: "implement".into(),
            resource: extension.id.clone(),
            status: "ready".into(),
            effect_class: "approval_boundary".into(),
            blockers: Vec::new(),
            approval_required: true,
            approval_requirements: vec!["budget_extension".into()],
            external_effect_summary: format!(
                "Resume the current Repo Mode stage on its preserved workspace with exactly {} additional turns and {} additional tokens.",
                turn_increment, token_increment
            ),
            state_hash: extension.state_hash.clone(),
        }]);
    }
    if let (Some(run), Some(extension)) = (current_run, retryable_budget_extension) {
        return Ok(vec![repo_action(
            RepoActionSpec {
                id: "retry_budget_extension_dispatch",
                lifecycle_stage: "implement",
                resource: &extension.id,
                status: "ready",
                effect_class: "model_execution",
                approval_required: true,
                summary: "Retry the previously approved budget-extension dispatch on the same Run, transcript, and workspace. This grants no additional budget.",
            },
            &state_hash,
            json!({
                "run_id":run.id,
                "run_status":run.status,
                "run_error":run.error,
                "budget_extension_id":extension.id,
                "budget_extension_state_hash":extension.state_hash,
                "budget_consumption":run.budget_consumption,
            }),
        )?]);
    }
    if let Some((run, execution)) =
        recoverable_repo_stage_startup(metadata, current_run, executions, chain)
    {
        return Ok(vec![repo_action(
            RepoActionSpec {
                id: "recover_stage_startup",
                lifecycle_stage: "implement",
                resource: &execution.id,
                status: "ready",
                effect_class: "controller_internal",
                approval_required: true,
                summary: "Seal the zero-turn Builder startup failure, restore the unused WorkItem attempt, and preserve the existing workspace for an explicit correction.",
            },
            &state_hash,
            json!({
                "run_id":run.id,
                "run_status":run.status,
                "run_stop_reason":run.stop_reason,
                "budget_consumption":run.budget_consumption,
                "stage_execution_id":execution.id,
                "stage_execution_status":execution.status,
                "attempt_count":attempt_count,
                "max_attempts":max_attempts,
            }),
        )?]);
    }
    if let Some((run, execution)) =
        recoverable_repo_followup_stage_startup(metadata, current_run, executions, chain)
    {
        let summary = if execution.stage_key == pharness_core::RepoStageKey::Test.as_str() {
            "Retry the zero-turn Tester startup on the preserved workspace from the sealed Implement outcome. This does not consume another WorkItem attempt."
        } else {
            "Retry the zero-turn Verifier startup on the preserved workspace from the sealed Test outcome. This does not consume another WorkItem attempt."
        };
        return Ok(vec![repo_action(
            RepoActionSpec {
                id: "retry_stage_startup",
                lifecycle_stage: &execution.stage_key,
                resource: &execution.id,
                status: "ready",
                effect_class: "model_execution",
                approval_required: true,
                summary,
            },
            &state_hash,
            json!({
                "run_id":run.id,
                "run_status":run.status,
                "run_error":run.error,
                "budget_consumption":run.budget_consumption,
                "stage_execution_id":execution.id,
                "failed_stage":execution.stage_key,
                "attempt_count":attempt_count,
                "max_attempts":max_attempts,
                "attempt_budget_consumed":false,
            }),
        )?]);
    }
    let plan_execution = executions
        .iter()
        .rev()
        .find(|execution| execution.stage_key == pharness_core::RepoStageKey::Plan.as_str());
    let mut actions = Vec::new();
    if let Some(annotation) = pending_annotation_effects.first() {
        let active_execution = executions
            .iter()
            .any(|execution| matches!(execution.status.as_str(), "queued" | "running" | "paused"));
        let delivery_started = change_set.is_some() || source_delivery_intent.is_some();
        let available = !active_execution && !delivery_started && attempt_count < max_attempts;
        let repeats_stage = annotation.requested_effect == "repeat_stage";
        actions.push(repo_action(
            RepoActionSpec {
                id: "apply_annotation_effect",
                lifecycle_stage: if repeats_stage { "implement" } else { "plan" },
                resource: &annotation.id,
                status: if available { "ready" } else { "blocked" },
                effect_class: "model_execution",
                approval_required: true,
                summary: if available {
                    if repeats_stage {
                        "Apply the operator annotation by authorizing a fresh Builder-Tester-Verifier chain on the preserved workspace."
                    } else {
                        "Apply the operator annotation by starting a fresh Planner execution; the sealed evidence remains immutable."
                    }
                } else if delivery_started {
                    "Source delivery has begun; this WorkItem cannot repeat or replan. Create a linked WorkItem for the requested change."
                } else if active_execution {
                    "The current stage must reach a terminal boundary before this annotation effect can be applied."
                } else {
                    "The WorkItem attempt limit is exhausted; create a linked WorkItem for the requested change."
                },
            },
            &state_hash,
            json!({
                "annotation_id":annotation.id,
                "requested_effect":annotation.requested_effect,
                "target_kind":annotation.target_kind,
                "target_id":annotation.target_id,
                "attempt_count":attempt_count,
                "max_attempts":max_attempts,
                "delivery_started":delivery_started,
                "active_execution":active_execution,
            }),
        )?);
        return Ok(actions);
    }
    if let Some(change_set) = change_set {
        if change_set.status == "proposed" {
            for approve in [true, false] {
                actions.push(repo_action(
                    RepoActionSpec {
                        id: if approve { "approve_change_set" } else { "reject_change_set" },
                        lifecycle_stage: "verify",
                        resource: &change_set.id,
                        status: "ready",
                        effect_class: "human_review",
                        approval_required: true,
                        summary: if approve {
                            "Approve the exact controller-derived ChangeSet. This does not create a branch or pull request."
                        } else {
                            "Reject the exact controller-derived ChangeSet and stop before source mutation."
                        },
                    },
                    &state_hash,
                    json!({"change_set_id":change_set.id,"revision":change_set.revision,"material_hash":change_set.material_hash}),
                )?);
            }
            return Ok(actions);
        }
        if change_set.status == "approved" {
            if source_delivery_intent.is_none() {
                if let Some(repair) = change_set_provenance_repair(change_set) {
                    actions.push(repo_action(
                        RepoActionSpec {
                            id: "repair_change_set_provenance",
                            lifecycle_stage: "verify",
                            resource: &change_set.id,
                            status: "ready",
                            effect_class: "controller_internal",
                            approval_required: true,
                            summary: "Revalidate the approved immutable patch and effective stage outcomes, then rebind only the stored ChangeSet session and Builder Run provenance. This does not change material, revision, approval, or external state.",
                        },
                        &state_hash,
                        json!({
                            "change_set_id":change_set.id,
                            "revision":change_set.revision,
                            "material_hash":change_set.material_hash,
                            "stored_run_id":change_set.run_id,
                            "material_builder_run_id":repair.material_builder_run_id,
                            "verification_run_id":repair.verification_run_id,
                        }),
                    )?);
                    return Ok(actions);
                }
            }
            match source_delivery_intent {
                None => actions.push(repo_action(
                    RepoActionSpec {
                        id: "authorize_source_delivery",
                        lifecycle_stage: "source_delivery",
                        resource: &change_set.id,
                        status: "ready",
                        effect_class: "external_source_mutation",
                        approval_required: true,
                        summary: "Authorize one exact GitHub branch, commit, and source pull request from the approved ChangeSet. Manual merge remains required.",
                    },
                    &state_hash,
                    json!({"change_set_id":change_set.id,"revision":change_set.revision,"material_hash":change_set.material_hash}),
                )?),
                Some(intent) if matches!(intent.status.as_str(), "pull_request_open" | "waiting_checks" | "waiting_merge" | "head_drift") => actions.push(repo_action(
                    RepoActionSpec {
                        id: "observe_source_delivery",
                        lifecycle_stage: "source_delivery",
                        resource: &intent.id,
                        status: "ready",
                        effect_class: "external_observation",
                        approval_required: true,
                        summary: "Observe the exact pull-request head, active required checks, and merge provenance with the isolated GitHub observer.",
                    },
                    &state_hash,
                    json!({"source_delivery_intent_id":intent.id,"intent_state_version":intent.state_version,"status":intent.status}),
                )?),
                Some(intent) if source_writer_failure_is_retryable(intent) => actions.push(repo_action(
                    RepoActionSpec {
                        id: "retry_source_delivery",
                        lifecycle_stage: "source_delivery",
                        resource: &intent.id,
                        status: "ready",
                        effect_class: "external_source_mutation",
                        approval_required: true,
                        summary: "Reverify the exact repository with the isolated source writer, then retry the same immutable SourceDeliveryIntent. The base commit, patch hash, and head branch cannot change.",
                    },
                    &state_hash,
                    json!({
                        "source_delivery_intent_id":intent.id,
                        "intent_state_version":intent.state_version,
                        "status":intent.status,
                        "failure_reason":intent.status_reason,
                        "base_commit":intent.base_commit,
                        "patch_hash":intent.patch_hash,
                        "head_branch":intent.head_branch,
                    }),
                )?),
                Some(intent) if intent.status == "pull_request_closed" => actions.push(repo_action(
                    RepoActionSpec {
                        id: "replan_work_item",
                        lifecycle_stage: "plan",
                        resource: &intent.id,
                        status: if attempt_count < max_attempts { "ready" } else { "blocked" },
                        effect_class: "model_execution",
                        approval_required: true,
                        summary: if attempt_count < max_attempts {
                            "The prior pull request is confirmed closed. Start a new Planner execution; any later source delivery will use a new ChangeSet and SourceDeliveryIntent."
                        } else {
                            "The prior pull request is closed, but the WorkItem attempt limit is exhausted; create a linked WorkItem."
                        },
                    },
                    &state_hash,
                    json!({"source_delivery_intent_id":intent.id,"status":intent.status,"attempt_count":attempt_count,"max_attempts":max_attempts}),
                )?),
                _ => {}
            }
            return Ok(actions);
        }
        if change_set.status == "rejected" && source_delivery_intent.is_none() {
            actions.push(repo_action(
                RepoActionSpec {
                    id: "replan_work_item",
                    lifecycle_stage: "plan",
                    resource: &metadata.work_item_id,
                    status: if attempt_count < max_attempts {
                        "ready"
                    } else {
                        "blocked"
                    },
                    effect_class: "model_execution",
                    approval_required: true,
                    summary: if attempt_count < max_attempts {
                        "Start a new Planner execution. A later approved plan creates a fresh workspace and stage-chain authorization."
                    } else {
                        "The WorkItem attempt limit is exhausted; create a linked WorkItem for any changed intent or acceptance contract."
                    },
                },
                &state_hash,
                json!({"change_set_id":change_set.id,"status":change_set.status,"attempt_count":attempt_count,"max_attempts":max_attempts}),
            )?);
        }
        return Ok(actions);
    }
    if plan_execution.is_none() {
        actions.push(repo_action(
            RepoActionSpec {
                id: "start_planner",
                lifecycle_stage: "plan",
                resource: &metadata.work_item_id,
                status: "ready",
                effect_class: "model_execution",
                approval_required: true,
                summary:
                    "Start one immutable repo-planner AgentRun from the sealed Discover evidence.",
            },
            &state_hash,
            json!({"stage":"plan"}),
        )?);
        return Ok(actions);
    }
    if plan_execution.is_some_and(|execution| {
        matches!(execution.status.as_str(), "queued" | "running" | "paused")
    }) {
        return Ok(actions);
    }
    let failed_chain_execution = executions
        .iter()
        .rev()
        .find(|execution| {
            matches!(
                execution.stage_key.as_str(),
                "implement" | "test" | "verify"
            ) && matches!(
                execution.status.as_str(),
                "failed" | "blocked" | "cancelled"
            )
        })
        .filter(|failed| {
            plan_execution
                .map(|plan| plan.created_at <= failed.created_at)
                .unwrap_or(true)
        });
    if let Some(execution) = failed_chain_execution {
        let available = attempt_count < max_attempts;
        for (id, summary) in [
            (
                "correct_stage_chain",
                "Authorize a fresh Builder-Tester-Verifier chain on the preserved workspace using the same approved WorkPlan.",
            ),
            (
                "replan_work_item",
                "Start a new Planner execution. A later approved plan creates a fresh workspace and stage-chain authorization.",
            ),
        ] {
            actions.push(repo_action(
                RepoActionSpec {
                    id,
                    lifecycle_stage: if id == "correct_stage_chain" {
                        "implement"
                    } else {
                        "plan"
                    },
                    resource: &execution.id,
                    status: if available { "ready" } else { "blocked" },
                    effect_class: "model_execution",
                    approval_required: true,
                    summary: if available {
                        summary
                    } else {
                        "The WorkItem attempt limit is exhausted; create a linked WorkItem before further model execution."
                    },
                },
                &state_hash,
                json!({"failed_stage_execution_id":execution.id,"failed_stage":execution.stage_key,"attempt_count":attempt_count,"max_attempts":max_attempts}),
            )?);
        }
        return Ok(actions);
    }
    if let Some(plan) = work_plan.filter(|plan| plan.status == "proposed") {
        for approve in [true, false] {
            actions.push(repo_action(
                RepoActionSpec {
                    id: if approve { "approve_work_plan" } else { "reject_work_plan" },
                    lifecycle_stage: "plan",
                    resource: &plan.id,
                    status: "ready",
                    effect_class: "human_review",
                    approval_required: true,
                    summary: if approve {
                        "Approve the exact Planner-submitted WorkPlan revision. This does not start coding."
                    } else {
                        "Reject the exact Planner-submitted WorkPlan revision and block the WorkItem for correction."
                    },
                },
                &state_hash,
                json!({"work_plan_id":plan.id,"revision":plan.revision,"status":plan.status}),
            )?);
        }
        return Ok(actions);
    }
    if work_plan.is_some_and(|plan| plan.status == "rejected")
        || plan_execution.is_some_and(|execution| {
            matches!(
                execution.status.as_str(),
                "failed" | "blocked" | "cancelled"
            )
        })
    {
        actions.push(repo_action(
            RepoActionSpec {
                id: "replan_work_item",
                lifecycle_stage: "plan",
                resource: &metadata.work_item_id,
                status: "ready",
                effect_class: "model_execution",
                approval_required: true,
                summary: "Start a fresh Planner execution from sealed evidence and operator annotations.",
            },
            &state_hash,
            json!({"prior_plan_execution_id":plan_execution.map(|execution| &execution.id)}),
        )?);
        return Ok(actions);
    }
    if let Some(plan) = work_plan.filter(|plan| plan.status == "approved") {
        if chain.is_none() {
            actions.push(repo_action(
                RepoActionSpec {
                    id: "authorize_stage_chain",
                    lifecycle_stage: "implement",
                    resource: &plan.id,
                    status: "ready",
                    effect_class: "model_execution",
                    approval_required: true,
                    summary: "Create one four-hour workspace grant and bind the Builder, Tester, and Verifier profiles to the approved WorkPlan. This does not authorize Git or provider mutation.",
                },
                &state_hash,
                json!({"work_plan_id":plan.id,"revision":plan.revision}),
            )?);
        }
    }
    Ok(actions)
}

fn recoverable_repo_stage_startup<'a>(
    metadata: &StoredRepoWorkItemMetadata,
    current_run: Option<&'a StoredRun>,
    executions: &'a [pharness_store::StoredStageExecution],
    chain: Option<&pharness_store::StoredStageChainAuthorization>,
) -> Option<(&'a StoredRun, &'a pharness_store::StoredStageExecution)> {
    let run = current_run?;
    if chain.is_some()
        || run.status != "preparing"
        || run.stop_reason.is_some()
        || run.budget_consumption.turns_used != 0
        || run.budget_consumption.tokens_used != 0
    {
        return None;
    }
    let execution = executions.iter().rev().find(|execution| {
        execution.stage_key == pharness_core::RepoStageKey::Implement.as_str()
            && execution.status == "preparing"
            && execution.run_id.as_ref() == Some(&run.id)
            && execution.workspace_id.is_some()
    })?;
    if metadata.current_stage_execution_id.as_deref() != Some(execution.id.as_str())
        || run
            .execution_target_json
            .pointer("/repo_mode/stage_execution_id")
            .and_then(Value::as_str)
            != Some(execution.id.as_str())
        || run
            .execution_target_json
            .pointer("/repo_mode/stage")
            .and_then(Value::as_str)
            != Some(pharness_core::RepoStageKey::Implement.as_str())
    {
        return None;
    }
    Some((run, execution))
}

fn recoverable_repo_followup_stage_startup<'a>(
    metadata: &StoredRepoWorkItemMetadata,
    current_run: Option<&'a StoredRun>,
    executions: &'a [pharness_store::StoredStageExecution],
    chain: Option<&pharness_store::StoredStageChainAuthorization>,
) -> Option<(&'a StoredRun, &'a pharness_store::StoredStageExecution)> {
    let run = current_run?;
    let worker_boundary_error = run.error.as_deref().or_else(|| {
        run.result_json
            .as_ref()
            .and_then(|result| result.get("error"))
            .and_then(Value::as_str)
    }) == Some("worker job failed before reporting a durable outcome");
    if chain.is_some()
        || run.status != "failed"
        || !worker_boundary_error
        || run.budget_consumption.turns_used != 0
        || run.budget_consumption.tokens_used != 0
    {
        return None;
    }
    let execution = executions.iter().rev().find(|execution| {
        matches!(execution.stage_key.as_str(), "test" | "verify")
            && execution.status == "failed"
            && execution.run_id.as_ref() == Some(&run.id)
            && execution.workspace_id.is_some()
    })?;
    if metadata.current_stage_execution_id.as_deref() != Some(execution.id.as_str())
        || run
            .execution_target_json
            .pointer("/repo_mode/stage_execution_id")
            .and_then(Value::as_str)
            != Some(execution.id.as_str())
        || run
            .execution_target_json
            .pointer("/repo_mode/stage")
            .and_then(Value::as_str)
            != Some(execution.stage_key.as_str())
        || run
            .execution_target_json
            .pointer("/repo_mode/chain_authorization_id")
            .and_then(Value::as_str)
            != execution
                .input_snapshot
                .get("chain_authorization_id")
                .and_then(Value::as_str)
    {
        return None;
    }
    Some((run, execution))
}

struct RepoActionSpec<'a> {
    id: &'a str,
    lifecycle_stage: &'a str,
    resource: &'a str,
    status: &'a str,
    effect_class: &'a str,
    approval_required: bool,
    summary: &'a str,
}

fn repo_action(
    spec: RepoActionSpec<'_>,
    work_item_state_hash: &str,
    bound_state: Value,
) -> Result<WorkItemActionResponse, ApiError> {
    let RepoActionSpec {
        id,
        lifecycle_stage,
        resource,
        status,
        effect_class,
        approval_required,
        summary,
    } = spec;
    Ok(WorkItemActionResponse {
        id: id.into(),
        lifecycle_stage: lifecycle_stage.into(),
        resource: resource.into(),
        status: status.into(),
        effect_class: effect_class.into(),
        blockers: Vec::new(),
        approval_required,
        approval_requirements: if approval_required {
            vec![id.into()]
        } else {
            Vec::new()
        },
        external_effect_summary: summary.into(),
        state_hash: canonical_material_hash(&json!({
            "action":id,
            "work_item_state_hash":work_item_state_hash,
            "bound_state":bound_state,
        }))?,
    })
}

fn repo_delivery_segments(
    executions: &[pharness_store::StoredStageExecution],
    outcomes: &[StoredStageOutcome],
) -> Vec<DeliverySegmentResponse> {
    [
        ("discover", "Discover"),
        ("plan", "Plan"),
        ("implement", "Implement"),
        ("test", "Test"),
        ("verify", "Verify"),
        ("source_delivery", "Source Delivery"),
        ("release", "Release"),
        ("observe", "Observe"),
    ]
    .into_iter()
    .map(|(key, label)| {
        let outcome = outcomes.iter().find(|outcome| outcome.stage_key == key);
        let execution = executions
            .iter()
            .rev()
            .find(|execution| execution.stage_key == key);
        let status = outcome
            .map(|outcome| outcome.status.as_str())
            .or_else(|| execution.map(|execution| execution.status.as_str()))
            .unwrap_or("pending");
        DeliverySegmentResponse {
            key: key.into(),
            label: label.into(),
            status: status.into(),
            summary: outcome
                .and_then(|outcome| outcome.outcome.get("stop_reason"))
                .and_then(Value::as_str)
                .unwrap_or("Awaiting its Repo Mode lifecycle boundary")
                .into(),
            stopping_reason: outcome
                .filter(|outcome| outcome.status != "succeeded")
                .and_then(|outcome| outcome.outcome.get("stop_reason"))
                .and_then(Value::as_str)
                .map(str::to_string),
            resources: execution
                .map(|execution| {
                    vec![DeliverySegmentResourceResponse {
                        kind: "stage_execution".into(),
                        id: execution.id.clone(),
                        label: format!("{} execution {}", label, execution.sequence),
                        summary: execution.stop_reason.clone(),
                    }]
                })
                .unwrap_or_default(),
        }
    })
    .collect()
}

struct AgentEvidenceBundle {
    catalog: Vec<Value>,
    payloads: Vec<Value>,
}

async fn agent_evidence_bundle(
    state: &AppState,
    metadata: &StoredRepoWorkItemMetadata,
    outcomes: &[StoredStageOutcome],
) -> Result<AgentEvidenceBundle, ApiError> {
    let mut catalog = outcomes
        .iter()
        .map(|outcome| {
            json!({
                "id":outcome.id,
                "kind":"stage_outcome",
                "version":outcome.schema_version,
                "hash":outcome.content_hash,
                "stage":outcome.stage_key,
                "status":outcome.status,
            })
        })
        .collect::<Vec<_>>();
    let mut payloads = outcomes
        .iter()
        .map(|outcome| {
            json!({
                "id":outcome.id,
                "kind":"stage_outcome",
                "version":outcome.schema_version,
                "hash":outcome.content_hash,
                "payload":outcome.outcome,
            })
        })
        .collect::<Vec<_>>();
    for context in metadata
        .context_repositories
        .as_array()
        .into_iter()
        .flatten()
    {
        let discovery_id = context
            .get("discovery_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::internal("context Repository has no discovery ID"))?;
        let expected_hash = context
            .get("discovery_hash")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::internal("context Repository has no discovery hash"))?;
        let discovery = state
            .store
            .get_repository_discovery(discovery_id)
            .await?
            .filter(|discovery| {
                discovery.status == "succeeded"
                    && discovery.content_hash.as_deref() == Some(expected_hash)
            })
            .ok_or_else(|| {
                ApiError::conflict(
                    "context Repository discovery is missing or no longer matches its pin",
                )
            })?;
        let projection = bounded_context_discovery_projection(context, &discovery);
        let projection_hash = canonical_material_hash(&projection)?;
        catalog.push(json!({
            "id":discovery.id,
            "kind":"context_repository_discovery",
            "version":"pharness.dev/context-repository-evidence/v1alpha1",
            "hash":projection_hash,
            "repository_id":context.get("repository_id"),
            "source_commit":context.get("source_commit"),
            "source_discovery_hash":expected_hash,
        }));
        payloads.push(json!({
            "id":discovery.id,
            "kind":"context_repository_discovery",
            "version":"pharness.dev/context-repository-evidence/v1alpha1",
            "hash":projection_hash,
            "payload":projection,
        }));
    }
    Ok(AgentEvidenceBundle { catalog, payloads })
}

fn bounded_context_discovery_projection(
    context: &Value,
    discovery: &pharness_store::StoredRepositoryDiscovery,
) -> Value {
    let inventory = discovery.inventory_json.as_ref().and_then(Value::as_object);
    let mut bounded = serde_json::Map::new();
    for key in [
        "identity",
        "contract_files",
        "language_indicators",
        "build_indicators",
        "dependency_candidates",
        "lock_candidates",
        "command_candidates",
        "roots",
        "automation_references",
        "conflicts",
        "limits",
    ] {
        let Some(value) = inventory.and_then(|inventory| inventory.get(key)) else {
            continue;
        };
        let value = match value {
            Value::Array(entries) if entries.len() > 100 => {
                Value::Array(entries.iter().take(100).cloned().collect())
            }
            other => other.clone(),
        };
        bounded.insert(key.into(), value);
    }
    json!({
        "schema_version":"pharness.dev/context-repository-evidence/v1alpha1",
        "repository_id":context.get("repository_id"),
        "canonical_url":context.get("canonical_url"),
        "source_commit":context.get("source_commit"),
        "discovery_id":discovery.id,
        "discovery_hash":discovery.content_hash,
        "bounded_inventory":bounded,
        "limits":{"maximum_items_per_inventory_field":100,"raw_repository_content_included":false},
    })
}

fn annotation_context(annotations: &[pharness_store::StoredOperatorAnnotation]) -> Vec<Value> {
    annotations
        .iter()
        .map(|annotation| {
            json!({
                "kind":"operator_annotation",
                "id":annotation.id,
                "target":{"kind":annotation.target_kind,"id":annotation.target_id},
                "statement":annotation.statement,
                "evidence_refs":annotation.evidence_refs,
                "requested_effect":annotation.requested_effect,
                "actor":annotation.actor,
                "reason":annotation.reason,
            })
        })
        .collect()
}

fn annotation_contradictions(
    annotations: &[pharness_store::StoredOperatorAnnotation],
) -> Vec<Value> {
    annotations
        .iter()
        .filter(|annotation| annotation.requested_effect == "mark_evidence_stale")
        .map(|annotation| {
            json!({
                "kind":"operator_marked_evidence_stale",
                "annotation_id":annotation.id,
                "target_kind":annotation.target_kind,
                "target_id":annotation.target_id,
                "statement":annotation.statement,
            })
        })
        .collect()
}

pub(in crate::app) struct RepoWorkItemActionExecutionRequest {
    pub actor: String,
    pub reason: String,
    pub state_hash: String,
    pub inference_policies: Option<crate::dto::StageChainInferencePolicyRequest>,
    pub execution_policies: Option<crate::dto::StageChainExecutionPolicyRequest>,
}

pub(in crate::app) async fn execute_repo_work_item_action(
    state: &AppState,
    work_item_id: &str,
    action_id: &str,
    request: RepoWorkItemActionExecutionRequest,
) -> Result<Value, ApiError> {
    let RepoWorkItemActionExecutionRequest {
        actor,
        reason,
        state_hash,
        inference_policies,
        execution_policies,
    } = request;
    ensure_repo_mode_enabled(state)?;
    let metadata = repo_metadata(state, work_item_id).await?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let work_plan = state.store.get_work_plan_by_work_item(work_item_id).await?;
    let change_set = match work_plan.as_ref() {
        Some(plan) => state.store.get_change_set_by_work_plan(&plan.id).await?,
        None => None,
    };
    let source_delivery_intent = match change_set.as_ref() {
        Some(change_set) => {
            state
                .store
                .get_source_delivery_intent_by_subject("work_item_change_set", &change_set.id)
                .await?
        }
        None => None,
    };
    let executions = state.store.list_stage_executions(work_item_id).await?;
    let chain = state
        .store
        .active_stage_chain_authorization(work_item_id)
        .await?;
    let annotations = state.store.list_operator_annotations(work_item_id).await?;
    let annotation_decisions = state
        .store
        .list_operator_annotation_decisions(work_item_id)
        .await?;
    let pending_annotation_effects =
        pending_annotation_effects(&annotations, &annotation_decisions);
    let action_run_id =
        repo_action_run_id(&metadata, &executions, work_item.current_run_id.as_ref());
    let current_run = match action_run_id {
        Some(run_id) => state.store.get_run(run_id).await?,
        None => None,
    };
    let pending_budget_extension = match current_run.as_ref() {
        Some(run) => {
            state
                .store
                .pending_budget_extension_for_run(&run.id)
                .await?
        }
        None => None,
    };
    let retryable_budget_extension = match current_run.as_ref().filter(|run| {
        run.status == "failed"
            && run
                .error
                .as_deref()
                .is_some_and(|error| error.starts_with("failed to launch worker job:"))
            && run
                .result_json
                .as_ref()
                .and_then(|result| result.get("budget_extension"))
                .is_some()
    }) {
        Some(run) => {
            state
                .store
                .latest_approved_budget_extension_for_run(&run.id)
                .await?
        }
        None => None,
    };
    let action = derive_repo_actions(
        &metadata,
        RepoActionInputs {
            attempts: (work_item.attempt_count, work_item.max_attempts),
            work_plan: work_plan.as_ref(),
            change_set: change_set.as_ref(),
            source_delivery_intent: source_delivery_intent.as_ref(),
            executions: &executions,
            chain: chain.as_ref(),
            pending_annotation_effects: &pending_annotation_effects,
            pending_budget_extension: pending_budget_extension.as_ref(),
            current_run: current_run.as_ref(),
            retryable_budget_extension: retryable_budget_extension.as_ref(),
        },
    )?
    .into_iter()
    .find(|action| action.id == action_id)
    .ok_or_else(|| ApiError::conflict("Repo Mode action is no longer available"))?;
    if action.state_hash != state_hash {
        return Err(ApiError::conflict(
            "Repo Mode action preview is stale; refresh and retry",
        ));
    }
    if action.status != "ready" {
        return Err(ApiError::conflict("Repo Mode action is blocked"));
    }
    match action_id {
        "recover_stage_startup" => {
            let (run, execution) = recoverable_repo_stage_startup(
                &metadata,
                current_run.as_ref(),
                &executions,
                chain.as_ref(),
            )
            .ok_or_else(|| {
                ApiError::conflict("Repo Mode stage startup is no longer recoverable")
            })?;
            if state
                .store
                .get_environment_preparation_by_run(&run.id)
                .await?
                .is_some()
            {
                return Err(ApiError::conflict(
                    "Repo Mode stage startup already has durable preparation state",
                ));
            }
            let run_id = run.id.clone();
            let execution_id = execution.id.clone();
            state
                .store
                .refund_unstarted_work_item_attempt(
                    work_item_id,
                    &run_id,
                    Some(actor.clone()),
                    Some(reason.clone()),
                )
                .await?;
            crate::worker::fail_run_from_dispatch(
                &state.store,
                &run_id,
                "controller sealed a zero-turn Builder startup failure before preparation was created"
                    .into(),
            )
            .await?;
            append_repo_audit(
                state,
                work_item_id,
                "repo.stage_startup.recovered",
                &actor,
                &reason,
                json!({
                    "run_id":run_id,
                    "stage_execution_id":execution_id,
                    "attempt_budget_restored":true,
                    "model_turns_consumed":0,
                    "workspace_preserved":true,
                }),
            )
            .await?;
            let run = state
                .store
                .get_run(&run_id)
                .await?
                .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
            let execution = state
                .store
                .get_stage_execution(&execution_id)
                .await?
                .ok_or_else(|| ApiError::not_found("stage_execution", &execution_id))?;
            let work_item = state
                .store
                .get_work_item(work_item_id)
                .await?
                .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
            Ok(json!({
                "work_item":work_item,
                "run":run,
                "stage_execution":execution,
                "attempt_budget_restored":true,
            }))
        }
        "retry_stage_startup" => {
            let (run, execution) = recoverable_repo_followup_stage_startup(
                &metadata,
                current_run.as_ref(),
                &executions,
                chain.as_ref(),
            )
            .ok_or_else(|| {
                ApiError::conflict("Repo Mode follow-up stage startup is no longer recoverable")
            })?;
            retry_repo_followup_stage_startup(state, work_item_id, run, execution, &actor, &reason)
                .await
        }
        "apply_annotation_effect" => {
            let annotation = pending_annotation_effects
                .first()
                .copied()
                .ok_or_else(|| ApiError::conflict("annotation effect is no longer pending"))?;
            let repeats_stage = annotation.requested_effect == "repeat_stage";
            let result = if repeats_stage {
                apply_repo_stage_correction(state, work_item_id, &actor, &reason).await?
            } else {
                apply_repo_replan(state, work_item_id, &actor, &reason).await?
            };
            let decision = state
                .store
                .create_operator_annotation_decision(CreateOperatorAnnotationDecision {
                    id: new_prefixed_id("annotdec"),
                    annotation_id: annotation.id.clone(),
                    work_item_id: work_item_id.into(),
                    decision: if repeats_stage {
                        "stage_repeat_started".into()
                    } else {
                        "replan_started".into()
                    },
                    action_id: action_id.into(),
                    actor,
                    reason,
                    state_hash,
                })
                .await?;
            Ok(json!({"annotation_decision":decision,"result":result}))
        }
        "start_planner" => start_repo_planner(state, work_item_id, &actor, &reason).await,
        "approve_work_plan" | "reject_work_plan" => {
            let plan = work_plan.ok_or_else(|| ApiError::conflict("WorkPlan is unavailable"))?;
            if plan.status != "proposed" {
                return Err(ApiError::conflict("WorkPlan is no longer proposed"));
            }
            let target = if action_id == "approve_work_plan" {
                "approved"
            } else {
                "rejected"
            };
            let plan = state
                .store
                .update_work_plan_status(
                    &plan.id,
                    target,
                    Some(actor.clone()),
                    Some(reason.clone()),
                )
                .await?;
            let status = if target == "approved" {
                "awaiting_approval"
            } else {
                "blocked"
            };
            let item = state
                .store
                .update_repo_work_item_status(
                    work_item_id,
                    status,
                    &actor,
                    &format!("WorkPlan {} by {actor}: {reason}", target),
                    false,
                )
                .await?;
            Ok(json!({"work_plan":plan,"work_item":item}))
        }
        "authorize_stage_chain" => {
            authorize_repo_stage_chain(
                state,
                work_item_id,
                &actor,
                &reason,
                None,
                inference_policies.as_ref(),
                execution_policies.as_ref(),
            )
            .await
        }
        "approve_budget_extension" => {
            let extension = pending_budget_extension.ok_or_else(|| {
                ApiError::conflict("Repo Mode budget extension is no longer pending")
            })?;
            let (extension, run) = state
                .store
                .approve_budget_extension(&extension.id, &state_hash, &actor, &reason)
                .await?;
            state.worker.spawn_run(run.clone(), run.cwd.clone());
            Ok(json!({
                "budget_extension": crate::dto::BudgetExtensionResponse::from(extension),
            }))
        }
        "retry_budget_extension_dispatch" => {
            let extension = retryable_budget_extension.ok_or_else(|| {
                ApiError::conflict("Repo Mode budget-extension dispatch is no longer retryable")
            })?;
            let (extension, run) = state
                .store
                .retry_approved_budget_extension_dispatch(&extension.id, &extension.state_hash)
                .await?;
            state
                .store
                .create_audit_event(CreateAuditEvent {
                    id: new_prefixed_id("audit"),
                    kind: "repo_mode.budget_extension_dispatch_retried".into(),
                    actor: Some(actor.clone()),
                    resource_kind: "work_item".into(),
                    resource_id: work_item_id.into(),
                    run_id: Some(run.id.clone()),
                    payload_json: json!({
                        "reason":reason,
                        "budget_extension_id":extension.id,
                        "run_id":run.id,
                        "additional_budget_granted":false,
                    }),
                })
                .await?;
            state.worker.spawn_run(run.clone(), run.cwd.clone());
            Ok(json!({
                "budget_extension":crate::dto::BudgetExtensionResponse::from(extension),
                "run":crate::dto::RunResponse::from(run),
            }))
        }
        "correct_stage_chain" => {
            apply_repo_stage_correction(state, work_item_id, &actor, &reason).await
        }
        "replan_work_item" => apply_repo_replan(state, work_item_id, &actor, &reason).await,
        "approve_change_set" | "reject_change_set" => {
            let change_set =
                change_set.ok_or_else(|| ApiError::conflict("ChangeSet is unavailable"))?;
            if change_set.status != "proposed" {
                return Err(ApiError::conflict("ChangeSet is no longer proposed"));
            }
            let target = if action_id == "approve_change_set" {
                "approved"
            } else {
                "rejected"
            };
            let change_set = state
                .store
                .update_change_set_status(
                    &change_set.id,
                    target,
                    Some(actor.clone()),
                    Some(reason.clone()),
                )
                .await?;
            let item = state
                .store
                .update_repo_work_item_status(
                    work_item_id,
                    if target == "approved" {
                        "awaiting_approval"
                    } else {
                        "blocked"
                    },
                    &actor,
                    &format!("ChangeSet {target} by {actor}: {reason}"),
                    false,
                )
                .await?;
            Ok(json!({"change_set":change_set,"work_item":item}))
        }
        "repair_change_set_provenance" => {
            repair_approved_change_set_provenance(state, work_item_id, &actor, &reason).await
        }
        "authorize_source_delivery" => {
            authorize_and_dispatch_source_delivery(state, work_item_id, &actor, &reason).await
        }
        "retry_source_delivery" => {
            retry_repo_source_delivery(state, work_item_id, &actor, &reason).await
        }
        "observe_source_delivery" => {
            dispatch_source_delivery_observation(state, work_item_id, &actor, &reason).await
        }
        _ => Err(ApiError::conflict("unsupported Repo Mode action")),
    }
}

async fn repair_approved_change_set_provenance(
    state: &AppState,
    work_item_id: &str,
    actor: &str,
    reason: &str,
) -> Result<Value, ApiError> {
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let plan = state
        .store
        .get_work_plan_by_work_item(work_item_id)
        .await?
        .filter(|plan| plan.status == "approved")
        .ok_or_else(|| ApiError::conflict("approved WorkPlan is required"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&plan.id)
        .await?
        .filter(|change_set| change_set.status == "approved")
        .ok_or_else(|| ApiError::conflict("approved ChangeSet is required"))?;
    if state
        .store
        .get_source_delivery_intent_by_subject("work_item_change_set", &change_set.id)
        .await?
        .is_some()
    {
        return Err(ApiError::conflict(
            "ChangeSet provenance cannot be repaired after source delivery begins",
        ));
    }
    let repair = change_set_provenance_repair(&change_set).ok_or_else(|| {
        ApiError::conflict("ChangeSet provenance is already current or cannot be repaired")
    })?;
    let material_builder_run_id = RunId::new(repair.material_builder_run_id.to_string());
    let verification_run_id = RunId::new(repair.verification_run_id.to_string());
    let builder_run = state
        .store
        .get_run(&material_builder_run_id)
        .await?
        .ok_or_else(|| {
            ApiError::conflict("ChangeSet material Builder Run provenance is unavailable")
        })?;
    let verification_run = state
        .store
        .get_run(&verification_run_id)
        .await?
        .ok_or_else(|| {
            ApiError::conflict("ChangeSet material Verifier Run provenance is unavailable")
        })?;

    if canonical_material_hash(&change_set.change_set_json)? != change_set.material_hash {
        return Err(ApiError::conflict(
            "ChangeSet material does not match its immutable hash",
        ));
    }
    if change_set.work_item_id.as_deref() != Some(work_item_id)
        || change_set
            .change_set_json
            .get("work_item_id")
            .and_then(Value::as_str)
            != Some(work_item_id)
        || change_set
            .change_set_json
            .pointer("/work_plan/id")
            .and_then(Value::as_str)
            != Some(plan.id.as_str())
        || change_set
            .change_set_json
            .pointer("/work_plan/revision")
            .and_then(Value::as_i64)
            != Some(plan.revision)
    {
        return Err(ApiError::conflict(
            "ChangeSet WorkItem or WorkPlan provenance is stale",
        ));
    }
    let source_commit = work_item
        .source_commit
        .as_deref()
        .ok_or_else(|| ApiError::conflict("immutable WorkItem source commit is unavailable"))?;
    if change_set
        .change_set_json
        .pointer("/source_provenance/source_commit")
        .and_then(Value::as_str)
        != Some(source_commit)
    {
        return Err(ApiError::conflict(
            "ChangeSet source provenance does not match the WorkItem",
        ));
    }

    let implement_stage_execution_id = change_set
        .change_set_json
        .pointer("/source_provenance/stage_execution_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("ChangeSet Implement execution is unavailable"))?;
    let implement_outcome_id = change_set
        .change_set_json
        .pointer("/source_provenance/implement_outcome_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("ChangeSet Implement outcome is unavailable"))?;
    let implement_outcome_hash = change_set
        .change_set_json
        .pointer("/source_provenance/implement_outcome_hash")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("ChangeSet Implement outcome hash is unavailable"))?;
    let verification_stage_execution_id = change_set
        .change_set_json
        .get("verification_stage_execution_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("ChangeSet Verify execution is unavailable"))?;
    let effective_outcomes = state
        .store
        .list_effective_stage_outcomes(work_item_id)
        .await?;
    if !effective_outcomes.iter().any(|outcome| {
        outcome.stage_key == "implement"
            && outcome.status == "succeeded"
            && outcome.id == implement_outcome_id
            && outcome.content_hash == implement_outcome_hash
            && outcome.stage_execution_id == implement_stage_execution_id
    }) {
        return Err(ApiError::conflict(
            "ChangeSet does not match the effective Implement outcome",
        ));
    }
    if !effective_outcomes.iter().any(|outcome| {
        outcome.stage_key == "verify"
            && outcome.status == "succeeded"
            && outcome.stage_execution_id == verification_stage_execution_id
    }) {
        return Err(ApiError::conflict(
            "ChangeSet does not match the effective Verify outcome",
        ));
    }
    let material_effective_outcomes = change_set
        .change_set_json
        .get("effective_outcomes")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::conflict("ChangeSet effective outcomes are unavailable"))?;
    let outcome_binding =
        validate_change_set_outcome_binding(material_effective_outcomes, &effective_outcomes)?;
    let historical_verifier_outcome_list_stale = match outcome_binding {
        ChangeSetOutcomeBinding::Current => false,
        ChangeSetOutcomeBinding::HistoricalVerifier { id, hash } => {
            state
                .store
                .get_stage_outcome(&id)
                .await?
                .filter(|outcome| {
                    outcome.work_item_id == work_item_id
                        && outcome.stage_key == "verify"
                        && outcome.status == "succeeded"
                        && outcome.content_hash == hash
                        && outcome.stage_execution_id != verification_stage_execution_id
                })
                .ok_or_else(|| {
                    ApiError::conflict(
                        "ChangeSet historical Verify outcome provenance is not immutable evidence",
                    )
                })?;
            true
        }
    };
    let implement_execution = state
        .store
        .get_stage_execution(implement_stage_execution_id)
        .await?
        .filter(|execution| {
            execution.work_item_id == work_item_id
                && execution.stage_key == "implement"
                && execution.status == "succeeded"
                && execution.run_id.as_ref() == Some(&material_builder_run_id)
        })
        .ok_or_else(|| {
            ApiError::conflict("ChangeSet Builder StageExecution provenance is unavailable")
        })?;
    let verification_execution = state
        .store
        .get_stage_execution(verification_stage_execution_id)
        .await?
        .filter(|execution| {
            execution.work_item_id == work_item_id
                && execution.stage_key == "verify"
                && execution.status == "succeeded"
                && execution.run_id.as_ref() == Some(&verification_run_id)
        })
        .ok_or_else(|| {
            ApiError::conflict("ChangeSet Verifier StageExecution provenance is unavailable")
        })?;
    if builder_run.id != material_builder_run_id
        || verification_run.id != verification_run_id
        || implement_execution.workspace_id.is_none()
        || verification_execution.workspace_id != implement_execution.workspace_id
    {
        return Err(ApiError::conflict(
            "ChangeSet stage run or workspace provenance is inconsistent",
        ));
    }

    let patch_artifact_id = change_set
        .change_set_json
        .pointer("/patch/artifact_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("ChangeSet patch artifact provenance is unavailable"))?;
    let patch_hash = change_set
        .change_set_json
        .pointer("/patch/hash")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("ChangeSet patch hash provenance is unavailable"))?;
    let patch = state
        .store
        .list_artifacts(&material_builder_run_id)
        .await?
        .into_iter()
        .find(|artifact| artifact.id == patch_artifact_id && artifact.kind == "workspace_git_diff")
        .ok_or_else(|| ApiError::conflict("ChangeSet patch artifact is unavailable"))?;
    let diff = patch
        .content_text
        .as_deref()
        .filter(|diff| !diff.is_empty())
        .ok_or_else(|| ApiError::conflict("ChangeSet patch artifact is empty"))?;
    if format!("sha256:{:x}", Sha256::digest(diff.as_bytes())) != patch_hash {
        return Err(ApiError::conflict(
            "ChangeSet patch artifact does not match its immutable hash",
        ));
    }

    let prior_session_id = change_set.session_id.clone();
    let prior_run_id = change_set.run_id.clone();
    let repaired = state
        .store
        .rebind_approved_change_set_provenance(
            &change_set.id,
            change_set.revision,
            &change_set.material_hash,
            &verification_run.session_id,
            &material_builder_run_id,
        )
        .await?;
    append_repo_audit(
        state,
        work_item_id,
        "repo.change_set.provenance_repaired",
        actor,
        reason,
        json!({
            "change_set_id":repaired.id,
            "revision":repaired.revision,
            "material_hash":repaired.material_hash,
            "prior_session_id":prior_session_id,
            "prior_run_id":prior_run_id,
            "session_id":repaired.session_id,
            "run_id":repaired.run_id,
            "material_unchanged":true,
            "revision_unchanged":true,
            "approval_unchanged":true,
            "external_effect":false,
            "historical_verifier_outcome_list_stale":historical_verifier_outcome_list_stale,
        }),
    )
    .await?;
    Ok(json!({
        "change_set":repaired,
        "material_unchanged":true,
        "revision_unchanged":true,
        "external_effect":false,
        "historical_verifier_outcome_list_stale":historical_verifier_outcome_list_stale,
    }))
}

async fn apply_repo_stage_correction(
    state: &AppState,
    work_item_id: &str,
    actor: &str,
    reason: &str,
) -> Result<Value, ApiError> {
    if let Some(chain) = state
        .store
        .active_stage_chain_authorization(work_item_id)
        .await?
    {
        state
            .store
            .revoke_stage_chain_authorization(
                &chain.id,
                "operator authorized a fresh correction chain",
            )
            .await?;
    }
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let workspace = state
        .store
        .list_workspaces(WorkspaceListFilter {
            work_item_id: Some(work_item_id.into()),
            limit: 100,
            ..WorkspaceListFilter::default()
        })
        .await?
        .into_iter()
        .find(|workspace| workspace.resolved_commit == work_item.source_commit)
        .ok_or_else(|| ApiError::conflict("preserved correction workspace is unavailable"))?;
    authorize_repo_stage_chain(
        state,
        work_item_id,
        actor,
        reason,
        Some(workspace),
        None,
        None,
    )
    .await
}

async fn retry_repo_followup_stage_startup(
    state: &AppState,
    work_item_id: &str,
    failed_run: &StoredRun,
    failed_execution: &pharness_store::StoredStageExecution,
    actor: &str,
    reason: &str,
) -> Result<Value, ApiError> {
    let upstream_stage = match failed_execution.stage_key.as_str() {
        "test" => "implement",
        "verify" => "test",
        _ => {
            return Err(ApiError::conflict(
                "only zero-turn Tester or Verifier startup is recoverable",
            ))
        }
    };
    let prior_chain_id = failed_run
        .execution_target_json
        .pointer("/repo_mode/chain_authorization_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("failed stage has no chain authorization"))?;
    let prior_chain = state
        .store
        .get_stage_chain_authorization(prior_chain_id)
        .await?
        .filter(|authorization| {
            authorization.work_item_id == work_item_id
                && authorization.id
                    == failed_execution
                        .input_snapshot
                        .get("chain_authorization_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
        })
        .ok_or_else(|| {
            ApiError::conflict("failed stage chain authorization provenance is unavailable")
        })?;
    if prior_chain.status == "active" {
        return Err(ApiError::conflict(
            "failed stage chain authorization is still active",
        ));
    }
    let metadata = repo_metadata(state, work_item_id).await?;
    if metadata.current_stage_execution_id.as_deref() != Some(failed_execution.id.as_str()) {
        return Err(ApiError::conflict(
            "failed stage is no longer the current WorkItem boundary",
        ));
    }
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let plan = state
        .store
        .get_work_plan_by_work_item(work_item_id)
        .await?
        .filter(|plan| {
            plan.status == "approved"
                && plan.id == prior_chain.work_plan_id
                && plan.revision == prior_chain.work_plan_revision
        })
        .ok_or_else(|| ApiError::conflict("approved WorkPlan is no longer current"))?;
    if prior_chain.product_model_snapshot_id != metadata.product_model_snapshot_id
        || prior_chain.product_model_snapshot_hash != metadata.product_model_snapshot_hash
        || prior_chain.repository_id != metadata.repository_id
        || work_item.source_commit.as_deref() != Some(prior_chain.source_commit.as_str())
        || failed_execution.workspace_id.as_deref() != Some(prior_chain.workspace_id.as_str())
    {
        return Err(ApiError::conflict(
            "failed stage authorization no longer matches the pinned WorkItem state",
        ));
    }
    let upstream_outcome = state
        .store
        .list_effective_stage_outcomes(work_item_id)
        .await?
        .into_iter()
        .find(|outcome| outcome.stage_key == upstream_stage && outcome.status == "succeeded")
        .ok_or_else(|| {
            ApiError::conflict(format!(
                "a sealed successful {upstream_stage} outcome is required"
            ))
        })?;
    let upstream_execution = state
        .store
        .get_stage_execution(&upstream_outcome.stage_execution_id)
        .await?
        .filter(|execution| {
            execution.work_item_id == work_item_id
                && execution.stage_key == upstream_stage
                && execution.status == "succeeded"
                && execution.workspace_id.as_deref() == Some(prior_chain.workspace_id.as_str())
        })
        .ok_or_else(|| ApiError::conflict("sealed upstream StageExecution is unavailable"))?;
    let upstream_run_id = upstream_execution
        .run_id
        .as_ref()
        .ok_or_else(|| ApiError::conflict("sealed upstream AgentRun is unavailable"))?;
    let upstream_run = state
        .store
        .get_run(upstream_run_id)
        .await?
        .filter(|run| run.status == "completed")
        .ok_or_else(|| ApiError::conflict("sealed upstream AgentRun is not completed"))?;
    let authorization = state
        .store
        .create_stage_chain_authorization(CreateStageChainAuthorization {
            id: new_prefixed_id("chain"),
            work_item_id: work_item_id.into(),
            work_plan_id: plan.id,
            work_plan_revision: plan.revision,
            product_model_snapshot_id: prior_chain.product_model_snapshot_id,
            product_model_snapshot_hash: prior_chain.product_model_snapshot_hash,
            repository_id: prior_chain.repository_id,
            source_commit: prior_chain.source_commit,
            workspace_id: prior_chain.workspace_id,
            writable_paths: prior_chain.writable_paths,
            profile_chain: prior_chain.profile_chain,
            budget_chain: prior_chain.budget_chain,
            state_hash: repo_work_item_state_hash(&metadata)?,
            created_by: actor.into(),
            creation_reason: reason.into(),
            expires_at: (current_millis() + 4 * 60 * 60 * 1_000).to_string(),
        })
        .await?;
    let started = match start_repo_followup_stage(
        state,
        &upstream_run,
        failed_execution.stage_key.as_str(),
        None,
    )
    .await
    {
        Ok(started) => started,
        Err(error) => {
            state
                .store
                .revoke_stage_chain_authorization(
                    &authorization.id,
                    "explicit follow-up stage startup retry could not be dispatched",
                )
                .await?;
            return Err(error);
        }
    };
    let work_item = state
        .store
        .update_repo_work_item_status(
            work_item_id,
            "executing",
            actor,
            "operator retried a zero-turn follow-up stage startup failure",
            false,
        )
        .await?;
    append_repo_audit(
        state,
        work_item_id,
        "repo.followup_stage_startup.retried",
        actor,
        reason,
        json!({
            "failed_run_id":failed_run.id,
            "failed_stage_execution_id":failed_execution.id,
            "stage":failed_execution.stage_key,
            "upstream_stage_execution_id":upstream_execution.id,
            "upstream_outcome_id":upstream_outcome.id,
            "replacement_chain_authorization_id":authorization.id,
            "attempt_budget_consumed":false,
            "model_turns_previously_consumed":0,
            "workspace_preserved":true,
        }),
    )
    .await?;
    Ok(json!({
        "work_item":work_item,
        "stage_chain_authorization":authorization,
        "retry":started,
        "attempt_budget_consumed":false,
    }))
}

async fn apply_repo_replan(
    state: &AppState,
    work_item_id: &str,
    actor: &str,
    reason: &str,
) -> Result<Value, ApiError> {
    if let Some(chain) = state
        .store
        .active_stage_chain_authorization(work_item_id)
        .await?
    {
        state
            .store
            .revoke_stage_chain_authorization(&chain.id, "operator requested full Repo Mode replan")
            .await?;
    }
    if let Some(plan) = state.store.get_work_plan_by_work_item(work_item_id).await? {
        state
            .store
            .update_work_plan_status(
                &plan.id,
                "superseded",
                Some(actor.into()),
                Some(reason.into()),
            )
            .await?;
    }
    start_repo_planner(state, work_item_id, actor, reason).await
}

async fn authorize_and_dispatch_source_delivery(
    state: &AppState,
    work_item_id: &str,
    actor: &str,
    reason: &str,
) -> Result<Value, ApiError> {
    if !state.worker.supports_remote_workspace() {
        return Err(ApiError::conflict(
            "Repo Mode source delivery requires kubernetes_job worker mode",
        ));
    }
    let metadata = repo_metadata(state, work_item_id).await?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let plan = state
        .store
        .get_work_plan_by_work_item(work_item_id)
        .await?
        .filter(|plan| plan.status == "approved")
        .ok_or_else(|| ApiError::conflict("approved WorkPlan is required"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&plan.id)
        .await?
        .filter(|change_set| change_set.status == "approved")
        .ok_or_else(|| ApiError::conflict("approved ChangeSet is required"))?;
    if state
        .store
        .get_source_delivery_intent_by_subject("work_item_change_set", &change_set.id)
        .await?
        .is_some()
    {
        return Err(ApiError::conflict(
            "source delivery is already bound to this ChangeSet",
        ));
    }
    let repository = state
        .store
        .get_repository(&metadata.repository_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repository", &metadata.repository_id))?;
    if repository.canonical_url != work_item.source_repo {
        return Err(ApiError::conflict(
            "registered Repository does not match the WorkItem source",
        ));
    }
    let settings = state
        .worker
        .git_writer_settings()
        .ok_or_else(|| ApiError::conflict("Git writer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|allowed| allowed == &repository.canonical_url)
    {
        return Err(ApiError::conflict(
            "Repository is not allowlisted for the isolated Git writer",
        ));
    }
    let source_commit = work_item
        .source_commit
        .clone()
        .filter(|commit| is_git_sha(commit))
        .ok_or_else(|| ApiError::conflict("immutable source commit is unavailable"))?;
    let patch_artifact_id = change_set
        .change_set_json
        .pointer("/patch/artifact_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("ChangeSet has no patch artifact provenance"))?;
    let patch_hash = change_set
        .change_set_json
        .pointer("/patch/hash")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("ChangeSet has no patch hash provenance"))?;
    let run_id = change_set
        .run_id
        .as_ref()
        .ok_or_else(|| ApiError::conflict("ChangeSet has no Builder Run provenance"))?;
    let patch = state
        .store
        .list_artifacts(run_id)
        .await?
        .into_iter()
        .find(|artifact| artifact.id == patch_artifact_id && artifact.kind == "workspace_git_diff")
        .ok_or_else(|| ApiError::conflict("ChangeSet patch artifact is unavailable"))?;
    let diff = patch
        .content_text
        .as_deref()
        .filter(|diff| !diff.is_empty())
        .ok_or_else(|| ApiError::conflict("ChangeSet patch artifact is empty"))?;
    if format!("sha256:{:x}", Sha256::digest(diff.as_bytes())) != patch_hash {
        return Err(ApiError::conflict(
            "ChangeSet patch artifact does not match its immutable hash",
        ));
    }
    let intent_id = new_prefixed_id("srcintent");
    let execution_id = new_prefixed_id("srcexec");
    let head_branch = format!(
        "pharness/{}/{}",
        work_item_id,
        &change_set.material_hash.trim_start_matches("sha256:")[..12]
    );
    let authorization = json!({
        "schema_version":"pharness.dev/source-delivery-authorization/v1alpha1",
        "actor":actor,
        "reason":reason,
        "work_item_id":work_item_id,
        "work_item_state_hash":repo_work_item_state_hash(&metadata)?,
        "work_plan":{"id":plan.id,"revision":plan.revision},
        "change_set":{"id":change_set.id,"revision":change_set.revision,"material_hash":change_set.material_hash},
        "repository_id":repository.id,
        "source_repo":repository.canonical_url,
        "base_ref":repository.default_branch,
        "base_commit":source_commit,
        "head_branch":head_branch,
        "patch_hash":patch_hash,
        "external_effect":"create one GitHub branch, commit, and pull request; merge is not authorized",
    });
    let intent = state
        .store
        .create_source_delivery_intent(CreateSourceDeliveryIntent {
            id: intent_id,
            subject_kind: "work_item_change_set".into(),
            subject_id: change_set.id.clone(),
            repository_id: repository.id,
            source_repo: repository.canonical_url,
            base_ref: repository.default_branch,
            base_commit: source_commit,
            head_branch,
            patch_artifact_id: Some(patch.id),
            patch_hash: patch_hash.into(),
            authorization,
            created_by: actor.into(),
            creation_reason: reason.into(),
        })
        .await?;
    match state
        .worker
        .dispatch_source_delivery(SourceDeliveryExecutionRequest {
            source_delivery_intent_id: intent.id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => {
            let intent = state
                .store
                .update_source_delivery_intent(
                    &intent.id,
                    intent.state_version,
                    "writer_dispatched",
                    Some(&execution_id),
                    None,
                    None,
                    None,
                    None,
                    actor,
                    reason,
                )
                .await?;
            let item = state
                .store
                .update_repo_work_item_status(
                    work_item_id,
                    "executing",
                    actor,
                    "isolated Git writer dispatched from exact SourceDeliveryIntent",
                    false,
                )
                .await?;
            append_repo_audit(
                state,
                work_item_id,
                "repo.source_delivery.writer_dispatched",
                actor,
                reason,
                json!({"source_delivery_intent_id":intent.id,"execution_id":execution_id,"job_name":receipt.job_name}),
            )
            .await?;
            Ok(
                json!({"source_delivery_intent":intent,"work_item":item,"job_name":receipt.job_name}),
            )
        }
        Err(error) => {
            let intent = state
                .store
                .update_source_delivery_intent(
                    &intent.id,
                    intent.state_version,
                    "failed",
                    Some(&execution_id),
                    None,
                    None,
                    None,
                    None,
                    "controller:repo-mode",
                    "Git writer dispatch failed",
                )
                .await?;
            let item = state
                .store
                .update_repo_work_item_status(
                    work_item_id,
                    "blocked",
                    "controller:repo-mode",
                    "Git writer dispatch failed before any source mutation was confirmed",
                    false,
                )
                .await?;
            tracing::warn!(source_delivery_intent_id=%intent.id, %error, "Repo Mode Git writer dispatch failed");
            Ok(json!({"source_delivery_intent":intent,"work_item":item,"status":"dispatch_failed"}))
        }
    }
}

async fn retry_repo_source_delivery(
    state: &AppState,
    work_item_id: &str,
    actor: &str,
    reason: &str,
) -> Result<Value, ApiError> {
    if !state.worker.supports_remote_workspace() {
        return Err(ApiError::conflict(
            "Repo Mode source delivery requires kubernetes_job worker mode",
        ));
    }
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let plan = state
        .store
        .get_work_plan_by_work_item(work_item_id)
        .await?
        .filter(|plan| plan.status == "approved")
        .ok_or_else(|| ApiError::conflict("approved WorkPlan is required"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&plan.id)
        .await?
        .filter(|change_set| change_set.status == "approved")
        .ok_or_else(|| ApiError::conflict("approved ChangeSet is required"))?;
    let intent = state
        .store
        .get_source_delivery_intent_by_subject("work_item_change_set", &change_set.id)
        .await?
        .ok_or_else(|| ApiError::conflict("SourceDeliveryIntent is unavailable"))?;
    if !source_writer_failure_is_retryable(&intent) {
        return Err(ApiError::conflict(
            "source writer failure is not eligible for an in-place retry",
        ));
    }
    if intent.subject_id != change_set.id
        || intent.source_repo != work_item.source_repo
        || work_item.source_commit.as_deref() != Some(intent.base_commit.as_str())
    {
        return Err(ApiError::conflict(
            "SourceDeliveryIntent no longer matches the approved WorkItem provenance",
        ));
    }
    let settings = state
        .worker
        .git_writer_settings()
        .ok_or_else(|| ApiError::conflict("Git writer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|allowed| allowed == &intent.source_repo)
    {
        return Err(ApiError::conflict(
            "Repository is not allowlisted for the isolated Git writer",
        ));
    }
    let artifact_id = intent
        .patch_artifact_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("SourceDeliveryIntent patch artifact is unavailable"))?;
    let run_id = change_set
        .run_id
        .as_ref()
        .ok_or_else(|| ApiError::conflict("ChangeSet has no Builder Run provenance"))?;
    let patch = state
        .store
        .list_artifacts(run_id)
        .await?
        .into_iter()
        .find(|artifact| artifact.id == artifact_id && artifact.kind == "workspace_git_diff")
        .ok_or_else(|| ApiError::conflict("SourceDeliveryIntent patch artifact is unavailable"))?;
    let diff = patch
        .content_text
        .as_deref()
        .filter(|diff| !diff.is_empty())
        .ok_or_else(|| ApiError::conflict("SourceDeliveryIntent patch artifact is empty"))?;
    if format!("sha256:{:x}", Sha256::digest(diff.as_bytes())) != intent.patch_hash {
        return Err(ApiError::conflict(
            "SourceDeliveryIntent patch artifact no longer matches its immutable hash",
        ));
    }

    let now = current_millis();
    let outcome = state
        .worker
        .verify_capability("source_writer", Some(&intent.source_repo))
        .await;
    let (status, summary, principal, repository, permission) = match outcome {
        Ok(outcome) => {
            let status = if outcome.available {
                "available"
            } else {
                "unavailable"
            };
            (
                status,
                capability_verification_summary(&outcome),
                outcome.principal,
                outcome.repository,
                outcome.permission,
            )
        }
        Err(_) => (
            "unavailable",
            "Isolated source writer verification could not complete for the exact repository"
                .to_string(),
            None,
            Some(intent.source_repo.clone()),
            None,
        ),
    };
    let verification = state
        .store
        .create_capability_verification(CreateCapabilityVerification {
            id: new_prefixed_id("capverify"),
            capability: "source_writer".into(),
            status: status.into(),
            summary,
            principal,
            repository,
            permission,
            verified_at: now.to_string(),
            expires_at: (now + 15 * 60 * 1_000).to_string(),
        })
        .await?;
    if verification.status != "available"
        || verification.repository.as_deref() != Some(intent.source_repo.as_str())
    {
        return Err(ApiError::conflict(format!(
            "exact source writer verification failed: {}",
            verification.summary
        )));
    }

    let execution_id = new_prefixed_id("srcexec");
    let receipt = state
        .worker
        .dispatch_source_delivery(SourceDeliveryExecutionRequest {
            source_delivery_intent_id: intent.id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
        .map_err(|_| ApiError::conflict("Git writer retry dispatch could not complete"))?;
    let prior_failure = intent.status_reason.clone();
    let intent = state
        .store
        .update_source_delivery_intent(
            &intent.id,
            intent.state_version,
            "writer_dispatched",
            Some(&execution_id),
            None,
            None,
            None,
            None,
            actor,
            reason,
        )
        .await?;
    let item = state
        .store
        .update_repo_work_item_status(
            work_item_id,
            "executing",
            actor,
            "isolated Git writer retry dispatched from the unchanged SourceDeliveryIntent",
            false,
        )
        .await?;
    append_repo_audit(
        state,
        work_item_id,
        "repo.source_delivery.writer_retry_dispatched",
        actor,
        reason,
        json!({
            "source_delivery_intent_id":intent.id,
            "execution_id":execution_id,
            "job_name":receipt.job_name,
            "capability_verification_id":verification.id,
            "repository":intent.source_repo,
            "prior_failure":prior_failure,
            "base_commit":intent.base_commit,
            "patch_hash":intent.patch_hash,
            "head_branch":intent.head_branch,
        }),
    )
    .await?;
    Ok(json!({
        "source_delivery_intent":intent,
        "work_item":item,
        "capability_verification":verification,
        "job_name":receipt.job_name,
    }))
}

async fn dispatch_source_delivery_observation(
    state: &AppState,
    work_item_id: &str,
    actor: &str,
    reason: &str,
) -> Result<Value, ApiError> {
    let plan = state
        .store
        .get_work_plan_by_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("WorkPlan is unavailable"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&plan.id)
        .await?
        .ok_or_else(|| ApiError::conflict("ChangeSet is unavailable"))?;
    let intent = state
        .store
        .get_source_delivery_intent_by_subject("work_item_change_set", &change_set.id)
        .await?
        .ok_or_else(|| ApiError::conflict("SourceDeliveryIntent is unavailable"))?;
    if !matches!(
        intent.status.as_str(),
        "pull_request_open" | "waiting_checks" | "waiting_merge" | "head_drift"
    ) {
        return Err(ApiError::conflict(
            "SourceDeliveryIntent is not ready for observation",
        ));
    }
    if intent.pull_request.is_none() {
        return Err(ApiError::conflict(
            "SourceDeliveryIntent has no pull-request provenance",
        ));
    }
    let settings = state
        .worker
        .git_observer_settings()
        .ok_or_else(|| ApiError::conflict("Git observer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|allowed| allowed == &intent.source_repo)
    {
        return Err(ApiError::conflict(
            "Repository is not allowlisted for the isolated Git observer",
        ));
    }
    let execution_id = new_prefixed_id("srcobserve");
    let dispatched = state
        .store
        .update_source_delivery_intent(
            &intent.id,
            intent.state_version,
            "observer_dispatched",
            None,
            Some(&execution_id),
            None,
            None,
            None,
            actor,
            reason,
        )
        .await?;
    match state
        .worker
        .dispatch_source_delivery_observation(SourceDeliveryObservationRequest {
            source_delivery_intent_id: intent.id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => Ok(json!({"source_delivery_intent":dispatched,"job_name":receipt.job_name})),
        Err(error) => {
            let restored = state
                .store
                .update_source_delivery_intent(
                    &dispatched.id,
                    dispatched.state_version,
                    &intent.status,
                    None,
                    None,
                    None,
                    None,
                    None,
                    "controller:repo-mode",
                    "Git observer dispatch failed; observation remains retryable",
                )
                .await?;
            tracing::warn!(source_delivery_intent_id=%restored.id, %error, "Repo Mode Git observer dispatch failed");
            Ok(json!({"source_delivery_intent":restored,"status":"dispatch_failed"}))
        }
    }
}

#[derive(Debug, Deserialize)]
pub(in crate::app) struct InternalSourceDeliveryQuery {
    execution_id: String,
}

pub(in crate::app) async fn internal_source_delivery_context(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Query(query): Query<InternalSourceDeliveryQuery>,
) -> Result<Json<GitDeliveryContextResponse>, ApiError> {
    let intent = current_source_delivery_writer(&state, &intent_id, &query.execution_id).await?;
    let artifact_id = intent
        .patch_artifact_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("SourceDeliveryIntent has no patch artifact"))?;
    let (diff, subject, commit_body, pull_request_body) = match intent.subject_kind.as_str() {
        "work_item_change_set" => {
            let change_set = state
                .store
                .get_change_set(&intent.subject_id)
                .await?
                .ok_or_else(|| {
                    ApiError::conflict("SourceDeliveryIntent ChangeSet is unavailable")
                })?;
            let run_id = change_set
                .run_id
                .as_ref()
                .ok_or_else(|| ApiError::conflict("SourceDeliveryIntent has no Builder Run"))?;
            let diff = state
                .store
                .list_artifacts(run_id)
                .await?
                .into_iter()
                .find(|artifact| {
                    artifact.id == artifact_id && artifact.kind == "workspace_git_diff"
                })
                .and_then(|artifact| artifact.content_text)
                .ok_or_else(|| {
                    ApiError::conflict("SourceDeliveryIntent patch evidence is invalid")
                })?;
            let subject = change_set.title.trim().replace(['\r', '\n'], " ");
            let commit_body = format!(
                "PHarness WorkItem {}\n\nChangeSet: {}",
                change_set.work_item_id.as_deref().unwrap_or("unknown"),
                change_set.id
            );
            let pull_request_body = format!(
                "Controller-derived source delivery for ChangeSet `{}`. Manual merge is required.",
                change_set.id
            );
            (diff, subject, commit_body, pull_request_body)
        }
        "repository_onboarding_proposal" => {
            let proposal = state
                .store
                .get_repository_onboarding_proposal(&intent.subject_id)
                .await?
                .filter(|proposal| proposal.status == "approved")
                .ok_or_else(|| {
                    ApiError::conflict("SourceDeliveryIntent onboarding proposal is unavailable")
                })?;
            let onboarding = state
                .store
                .get_repository_onboarding(&proposal.onboarding_id)
                .await?
                .filter(|onboarding| {
                    onboarding.source_delivery_intent_id.as_deref() == Some(intent.id.as_str())
                        && onboarding.approved_proposal_hash.as_deref()
                            == Some(proposal.content_hash.as_str())
                })
                .ok_or_else(|| {
                    ApiError::conflict("SourceDeliveryIntent onboarding provenance is unavailable")
                })?;
            let diff = state
                .store
                .get_artifact(artifact_id)
                .await?
                .filter(|artifact| artifact.kind == "repository_onboarding_patch")
                .and_then(|artifact| artifact.content_text)
                .ok_or_else(|| {
                    ApiError::conflict("SourceDeliveryIntent onboarding patch is unavailable")
                })?;
            let subject = format!("Onboard repository with PHarness ({})", onboarding.id);
            let commit_body = format!(
                "PHarness Repository onboarding\n\nOnboarding: {}\nProposal: {}",
                onboarding.id, proposal.id
            );
            let pull_request_body = format!(
                "Controller-materialized onboarding contract for proposal `{}`. Manual merge is required.",
                proposal.id
            );
            (diff, subject, commit_body, pull_request_body)
        }
        _ => {
            return Err(ApiError::conflict(
                "SourceDeliveryIntent subject kind is unsupported",
            ))
        }
    };
    if format!("sha256:{:x}", Sha256::digest(diff.as_bytes())) != intent.patch_hash {
        return Err(ApiError::conflict(
            "SourceDeliveryIntent patch evidence hash is invalid",
        ));
    }
    let settings = state
        .worker
        .git_writer_settings()
        .ok_or_else(|| ApiError::conflict("Git writer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &intent.source_repo)
    {
        return Err(ApiError::conflict(
            "SourceDeliveryIntent repository is not writer-allowlisted",
        ));
    }
    Ok(Json(GitDeliveryContextResponse {
        execution_id: query.execution_id,
        repository: intent.source_repo,
        base_ref: intent.base_ref,
        base_commit: intent.base_commit,
        head_branch: intent.head_branch,
        diff,
        commit_subject: subject.clone(),
        commit_body,
        pull_request_title: subject,
        pull_request_body,
        github_api_url: settings.github_api_url,
        author_name: settings.author_name,
        author_email: settings.author_email,
    }))
}

pub(in crate::app) async fn internal_source_delivery_writer_outcome(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Json(request): Json<GitDeliveryOutcomeRequest>,
) -> Result<Json<Value>, ApiError> {
    let intent = current_source_delivery_writer(&state, &intent_id, &request.execution_id).await?;
    let subject = source_delivery_subject(&state, &intent).await?;
    match request.status.as_str() {
        "completed" => {
            let branch = request
                .branch
                .filter(|value| value == &intent.head_branch)
                .ok_or_else(|| {
                    ApiError::conflict(
                        "writer outcome branch does not match the SourceDeliveryIntent",
                    )
                })?;
            let commit_sha = request
                .commit_sha
                .filter(|value| is_git_sha(value))
                .ok_or_else(|| {
                    ApiError::bad_request("writer outcome requires a full commit SHA")
                })?;
            let pull_request_url = request
                .pull_request_url
                .filter(|value| super::identifiers::is_github_pr_url(value))
                .ok_or_else(|| {
                    ApiError::bad_request("writer outcome requires a valid GitHub pull-request URL")
                })?;
            let pull_request_number = request.pull_request_number.ok_or_else(|| {
                ApiError::bad_request("writer outcome requires a pull-request number")
            })?;
            let expected_prefix = format!("{}/pull/", intent.source_repo.trim_end_matches(".git"));
            if !pull_request_url.starts_with(&expected_prefix)
                || !pull_request_url.ends_with(&format!("/{pull_request_number}"))
            {
                return Err(ApiError::conflict("writer outcome pull request does not match the SourceDeliveryIntent repository"));
            }
            let pull_request = json!({
                "url":pull_request_url,
                "number":pull_request_number,
                "head_branch":branch,
                "head_sha":commit_sha,
            });
            let intent = state
                .store
                .update_source_delivery_intent(
                    &intent.id,
                    intent.state_version,
                    "pull_request_open",
                    None,
                    None,
                    Some(&pull_request),
                    None,
                    None,
                    "agent:git-writer",
                    "isolated writer reported exact pull-request provenance",
                )
                .await?;
            let subject_response = match subject {
                SourceDeliverySubject::WorkItem(work_item_id) => {
                    let item = state.store.update_repo_work_item_status(
                        &work_item_id, "waiting_external", "controller:repo-mode",
                        "source pull request is open; authoritative checks and manual merge are pending", false,
                    ).await?;
                    json!({"work_item":item})
                }
                SourceDeliverySubject::Onboarding(onboarding_id) => {
                    let onboarding = state.store.update_repository_onboarding_source_delivery(
                        &onboarding_id, &intent.id, "waiting_external", None,
                        "controller:repo-mode", "onboarding pull request is open; authoritative checks and manual merge are pending",
                    ).await?;
                    json!({"onboarding":onboarding})
                }
            };
            Ok(Json(
                json!({"source_delivery_intent":intent,"subject":subject_response}),
            ))
        }
        "failed" => {
            let error = request
                .error_code
                .unwrap_or_else(|| "git_writer_failed".into());
            let intent = state
                .store
                .update_source_delivery_intent(
                    &intent.id,
                    intent.state_version,
                    "failed",
                    None,
                    None,
                    None,
                    None,
                    None,
                    "agent:git-writer",
                    &error,
                )
                .await?;
            let subject_response = match subject {
                SourceDeliverySubject::WorkItem(work_item_id) => {
                    let item = state
                        .store
                        .update_repo_work_item_status(
                            &work_item_id,
                            "blocked",
                            "controller:repo-mode",
                            "source writer failed before pull-request provenance was confirmed",
                            false,
                        )
                        .await?;
                    json!({"work_item":item})
                }
                SourceDeliverySubject::Onboarding(onboarding_id) => {
                    let onboarding = state.store.update_repository_onboarding_source_delivery(
                        &onboarding_id, &intent.id, "delivery_failed", None,
                        "controller:repo-mode", "onboarding source writer failed before pull-request provenance was confirmed",
                    ).await?;
                    json!({"onboarding":onboarding})
                }
            };
            Ok(Json(
                json!({"source_delivery_intent":intent,"subject":subject_response}),
            ))
        }
        _ => Err(ApiError::bad_request(
            "source delivery writer status must be completed or failed",
        )),
    }
}

pub(in crate::app) async fn internal_source_delivery_observation_context(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Query(query): Query<InternalSourceDeliveryQuery>,
) -> Result<Json<GitDeliveryObservationContextResponse>, ApiError> {
    let intent = current_source_delivery_observer(&state, &intent_id, &query.execution_id).await?;
    let pull_request = intent
        .pull_request
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApiError::conflict("SourceDeliveryIntent pull-request provenance is unavailable")
        })?;
    let settings = state
        .worker
        .git_observer_settings()
        .ok_or_else(|| ApiError::conflict("Git observer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &intent.source_repo)
    {
        return Err(ApiError::conflict(
            "SourceDeliveryIntent repository is not observer-allowlisted",
        ));
    }
    Ok(Json(GitDeliveryObservationContextResponse {
        execution_id: query.execution_id,
        repository: intent.source_repo,
        base_ref: intent.base_ref,
        head_branch: pull_request
            .get("head_branch")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::conflict("pull-request head branch is unavailable"))?
            .into(),
        source_commit_sha: pull_request
            .get("head_sha")
            .and_then(Value::as_str)
            .filter(|sha| is_git_sha(sha))
            .ok_or_else(|| ApiError::conflict("pull-request head SHA is unavailable"))?
            .into(),
        pull_request_url: pull_request
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::conflict("pull-request URL is unavailable"))?
            .into(),
        pull_request_number: pull_request
            .get("number")
            .and_then(Value::as_u64)
            .ok_or_else(|| ApiError::conflict("pull-request number is unavailable"))?,
        github_api_url: settings.github_api_url,
    }))
}

pub(in crate::app) async fn internal_source_delivery_observation_outcome(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Json(request): Json<GitDeliveryObservationOutcomeRequest>,
) -> Result<Json<Value>, ApiError> {
    let intent =
        current_source_delivery_observer(&state, &intent_id, &request.execution_id).await?;
    let subject = source_delivery_subject(&state, &intent).await?;
    if request.status == "failed" {
        let restored = state
            .store
            .update_source_delivery_intent(
                &intent.id,
                intent.state_version,
                "pull_request_open",
                None,
                None,
                None,
                None,
                None,
                "agent:git-observer",
                request
                    .error_code
                    .as_deref()
                    .unwrap_or("git_observer_failed"),
            )
            .await?;
        if let SourceDeliverySubject::Onboarding(onboarding_id) = &subject {
            state
                .store
                .update_repository_onboarding_source_delivery(
                    onboarding_id,
                    &intent.id,
                    "waiting_external",
                    None,
                    "controller:repo-mode",
                    "Git observer failed; onboarding observation remains retryable",
                )
                .await?;
        }
        return Ok(Json(
            json!({"source_delivery_intent":restored,"status":"observation_failed"}),
        ));
    }
    if request.status != "observed" || !request.authoritative_rules_succeeded {
        return Err(ApiError::conflict(
            "authoritative GitHub branch-rule observation is required",
        ));
    }
    let pull_request = intent
        .pull_request
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApiError::conflict("SourceDeliveryIntent pull-request provenance is unavailable")
        })?;
    let expected_head = pull_request
        .get("head_sha")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("SourceDeliveryIntent expected head is unavailable"))?;
    let head_sha = request
        .head_commit_sha
        .as_deref()
        .filter(|sha| is_git_sha(sha))
        .ok_or_else(|| ApiError::bad_request("observation requires a full head SHA"))?;
    let merged = request
        .merged
        .ok_or_else(|| ApiError::bad_request("observation requires merged"))?;
    let pull_request_state = request
        .pull_request_state
        .as_deref()
        .filter(|state| matches!(*state, "open" | "closed"))
        .ok_or_else(|| {
            ApiError::bad_request("observation requires an open or closed pull-request state")
        })?;
    let provider_status = derive_provider_check_status(&request.required_checks)?;
    if request.provider_check_status.as_deref() != Some(provider_status) {
        return Err(ApiError::conflict(
            "provider-check result does not match controller derivation",
        ));
    }
    if !request.check_runs.is_array() || !request.commit_statuses.is_array() {
        return Err(ApiError::bad_request(
            "provider-check evidence must be bounded arrays",
        ));
    }
    let required_set_hash = canonical_material_hash(&request.required_checks)?;
    let observation_material = json!({
        "source_delivery_intent_id":intent.id,
        "phase":if merged {"merge"} else {"pre_merge"},
        "head_sha":head_sha,
        "required_set_hash":required_set_hash,
        "status":provider_status,
        "required_checks":request.required_checks,
        "check_runs":request.check_runs,
        "commit_statuses":request.commit_statuses,
    });
    let provider_observation = state
        .store
        .create_provider_check_set_observation(CreateProviderCheckSetObservation {
            id: new_prefixed_id("providerchecks"),
            source_delivery_intent_id: intent.id.clone(),
            phase: if merged {
                "merge".into()
            } else {
                "pre_merge".into()
            },
            repository_id: intent.repository_id.clone(),
            pull_request_number: pull_request
                .get("number")
                .and_then(Value::as_u64)
                .ok_or_else(|| ApiError::conflict("pull-request number is unavailable"))?,
            head_sha: head_sha.into(),
            required_set_hash: required_set_hash.clone(),
            authoritative_rules_succeeded: true,
            status: provider_status.into(),
            required_checks: request.required_checks.clone(),
            check_runs: request.check_runs.clone(),
            commit_statuses: request.commit_statuses.clone(),
            content_hash: canonical_material_hash(&observation_material)?,
            expires_at: (current_millis() + 15 * 60 * 1_000).to_string(),
        })
        .await?;
    let checks_summary = json!({"observation_id":provider_observation.id,"required_set_hash":required_set_hash,"status":provider_status,"expires_at":provider_observation.expires_at});

    if head_sha != expected_head {
        if !merged && pull_request_state == "closed" {
            let intent = state
                .store
                .update_source_delivery_intent(
                    &intent.id,
                    intent.state_version,
                    "pull_request_closed",
                    None,
                    None,
                    None,
                    None,
                    Some(&checks_summary),
                    "controller:repo-mode",
                    "drifted pull request was closed without merge",
                )
                .await?;
            let subject_response = match &subject {
                SourceDeliverySubject::WorkItem(work_item_id) => {
                    let item = state
                        .store
                        .update_repo_work_item_status(
                            work_item_id,
                            "blocked",
                            "controller:repo-mode",
                            "drifted source pull request is closed; explicit replan is available",
                            false,
                        )
                        .await?;
                    json!({"work_item":item})
                }
                SourceDeliverySubject::Onboarding(onboarding_id) => {
                    let onboarding = state
                        .store
                        .update_repository_onboarding_source_delivery(
                            onboarding_id,
                            &intent.id,
                            "blocked",
                            None,
                            "controller:repo-mode",
                            "drifted onboarding pull request was closed; start a new onboarding",
                        )
                        .await?;
                    json!({"onboarding":onboarding})
                }
            };
            return Ok(Json(json!({
                "source_delivery_intent":intent,
                "subject":subject_response,
                "provider_checks":provider_observation,
            })));
        }
        let terminal = merged;
        if terminal {
            if let SourceDeliverySubject::WorkItem(work_item_id) = &subject {
                seal_source_delivery_closure(
                    &state,
                    work_item_id,
                    &intent,
                    &provider_observation,
                    "failed",
                    "merged pull-request head does not match approved source provenance",
                    request.merge_commit_sha.as_deref(),
                )
                .await?;
            }
        }
        let drift_provenance = terminal.then(|| {
            json!({
                "merge_commit_sha":request.merge_commit_sha,
                "head_sha":head_sha,
            })
        });
        let intent = state
            .store
            .update_source_delivery_intent(
                &intent.id,
                intent.state_version,
                if terminal { "failed" } else { "head_drift" },
                None,
                None,
                None,
                drift_provenance.as_ref(),
                Some(&checks_summary),
                "controller:repo-mode",
                "pull-request head drifted from approved provenance",
            )
            .await?;
        let subject_response = match &subject {
            SourceDeliverySubject::WorkItem(work_item_id) => {
                let item = state
                    .store
                    .update_repo_work_item_status(
                        work_item_id,
                        if terminal { "failed" } else { "blocked" },
                        "controller:repo-mode",
                        if terminal {
                            "merged source provenance does not match the approved ChangeSet"
                        } else {
                            "unapproved pull-request head drift; close the PR before correction"
                        },
                        terminal,
                    )
                    .await?;
                json!({"work_item":item})
            }
            SourceDeliverySubject::Onboarding(onboarding_id) => {
                let onboarding = state.store.update_repository_onboarding_source_delivery(
                    onboarding_id, &intent.id, if terminal { "delivery_failed" } else { "blocked" },
                    request.merge_commit_sha.as_deref(), "controller:repo-mode",
                    if terminal { "merged onboarding head does not match approved proposal provenance" } else { "unapproved onboarding pull-request head drift; close the PR before correction" },
                ).await?;
                json!({"onboarding":onboarding})
            }
        };
        return Ok(Json(
            json!({"source_delivery_intent":intent,"subject":subject_response,"provider_checks":provider_observation}),
        ));
    }
    if !merged {
        let next_status = if pull_request_state == "closed" {
            "pull_request_closed"
        } else if provider_status == "passing" {
            "waiting_merge"
        } else {
            "waiting_checks"
        };
        let intent = state
            .store
            .update_source_delivery_intent(
                &intent.id,
                intent.state_version,
                next_status,
                None,
                None,
                None,
                None,
                Some(&checks_summary),
                "agent:git-observer",
                "fresh pre-merge provider observation recorded",
            )
            .await?;
        let subject_response = match &subject {
            SourceDeliverySubject::WorkItem(work_item_id) => {
                let item = state
                    .store
                    .update_repo_work_item_status(
                        work_item_id,
                        if pull_request_state == "closed" {
                            "blocked"
                        } else {
                            "waiting_external"
                        },
                        "controller:repo-mode",
                        if pull_request_state == "closed" {
                            "source pull request closed without merge"
                        } else {
                            "manual merge and provider checks remain external"
                        },
                        false,
                    )
                    .await?;
                json!({"work_item":item})
            }
            SourceDeliverySubject::Onboarding(onboarding_id) => {
                let onboarding_status = if pull_request_state == "closed" {
                    "blocked"
                } else if provider_status == "passing" {
                    "waiting_merge"
                } else {
                    "waiting_checks"
                };
                let onboarding = state
                    .store
                    .update_repository_onboarding_source_delivery(
                        onboarding_id,
                        &intent.id,
                        onboarding_status,
                        None,
                        "controller:repo-mode",
                        if pull_request_state == "closed" {
                            "onboarding pull request closed without merge"
                        } else {
                            "manual onboarding merge and provider checks remain external"
                        },
                    )
                    .await?;
                json!({"onboarding":onboarding})
            }
        };
        return Ok(Json(
            json!({"source_delivery_intent":intent,"subject":subject_response,"provider_checks":provider_observation}),
        ));
    }
    let merge_sha = request
        .merge_commit_sha
        .as_deref()
        .filter(|sha| is_git_sha(sha));
    let pre_merge = state
        .store
        .latest_provider_check_set_observation(&intent.id, "pre_merge")
        .await?;
    let current = current_millis();
    let delivery_succeeded = pull_request_state == "closed"
        && merge_sha.is_some()
        && provider_status == "passing"
        && pre_merge.as_ref().is_some_and(|observation| {
            observation.authoritative_rules_succeeded
                && observation.status == "passing"
                && observation.head_sha == head_sha
                && observation.required_set_hash == required_set_hash
                && observation
                    .expires_at
                    .parse::<u128>()
                    .is_ok_and(|expiry| expiry >= current)
        });
    let terminal_status = if delivery_succeeded {
        "succeeded"
    } else {
        "failed"
    };
    let stop_reason = if delivery_succeeded {
        "manual merge matched the approved head and fresh authoritative required checks"
    } else {
        "merge occurred without matching fresh passing pre-merge provider evidence"
    };
    if let SourceDeliverySubject::WorkItem(work_item_id) = &subject {
        seal_source_delivery_closure(
            &state,
            work_item_id,
            &intent,
            &provider_observation,
            terminal_status,
            stop_reason,
            merge_sha,
        )
        .await?;
    }
    let provenance = json!({
        "pull_request":pull_request,
        "head_sha":head_sha,
        "merge_commit_sha":merge_sha,
        "required_set_hash":required_set_hash,
        "pre_merge_observation_id":pre_merge.as_ref().map(|observation| &observation.id),
        "merge_observation_id":provider_observation.id,
        "status":terminal_status,
    });
    let intent = state
        .store
        .update_source_delivery_intent(
            &intent.id,
            intent.state_version,
            if delivery_succeeded {
                "merged"
            } else {
                "failed"
            },
            None,
            None,
            None,
            Some(&provenance),
            Some(&checks_summary),
            "controller:repo-mode",
            stop_reason,
        )
        .await?;
    let subject_response = match &subject {
        SourceDeliverySubject::WorkItem(work_item_id) => {
            let item = state
                .store
                .update_repo_work_item_status(
                    work_item_id,
                    if delivery_succeeded {
                        "completed"
                    } else {
                        "failed"
                    },
                    "controller:repo-mode",
                    stop_reason,
                    true,
                )
                .await?;
            json!({"work_item":item})
        }
        SourceDeliverySubject::Onboarding(onboarding_id) => {
            let onboarding = state.store.update_repository_onboarding_source_delivery(
                onboarding_id, &intent.id,
                if delivery_succeeded { "merge_observed" } else { "delivery_failed" },
                merge_sha, "controller:repo-mode",
                if delivery_succeeded { "onboarding merge matched approved provenance; canonical contract validation is required" } else { stop_reason },
            ).await?;
            json!({"onboarding":onboarding})
        }
    };
    Ok(Json(
        json!({"source_delivery_intent":intent,"subject":subject_response,"provider_checks":provider_observation,"delivery_status":terminal_status}),
    ))
}

async fn current_source_delivery_writer(
    state: &AppState,
    intent_id: &str,
    execution_id: &str,
) -> Result<StoredSourceDeliveryIntent, ApiError> {
    state
        .store
        .get_source_delivery_intent(intent_id)
        .await?
        .filter(|intent| {
            intent.status == "writer_dispatched"
                && intent.writer_execution_id.as_deref() == Some(execution_id)
        })
        .ok_or_else(|| ApiError::conflict("source delivery writer execution is not current"))
}

async fn current_source_delivery_observer(
    state: &AppState,
    intent_id: &str,
    execution_id: &str,
) -> Result<StoredSourceDeliveryIntent, ApiError> {
    state
        .store
        .get_source_delivery_intent(intent_id)
        .await?
        .filter(|intent| {
            intent.status == "observer_dispatched"
                && intent.observer_execution_id.as_deref() == Some(execution_id)
        })
        .ok_or_else(|| ApiError::conflict("source delivery observer execution is not current"))
}

enum SourceDeliverySubject {
    WorkItem(String),
    Onboarding(String),
}

async fn source_delivery_subject(
    state: &AppState,
    intent: &StoredSourceDeliveryIntent,
) -> Result<SourceDeliverySubject, ApiError> {
    match intent.subject_kind.as_str() {
        "work_item_change_set" => state
            .store
            .get_change_set(&intent.subject_id)
            .await?
            .and_then(|change_set| change_set.work_item_id)
            .map(SourceDeliverySubject::WorkItem)
            .ok_or_else(|| {
                ApiError::conflict("SourceDeliveryIntent WorkItem provenance is unavailable")
            }),
        "repository_onboarding_proposal" => {
            let proposal = state
                .store
                .get_repository_onboarding_proposal(&intent.subject_id)
                .await?
                .filter(|proposal| proposal.status == "approved")
                .ok_or_else(|| {
                    ApiError::conflict("SourceDeliveryIntent onboarding proposal is unavailable")
                })?;
            state
                .store
                .get_repository_onboarding(&proposal.onboarding_id)
                .await?
                .filter(|onboarding| {
                    onboarding.source_delivery_intent_id.as_deref() == Some(intent.id.as_str())
                        && onboarding.approved_proposal_hash.as_deref()
                            == Some(proposal.content_hash.as_str())
                })
                .map(|onboarding| SourceDeliverySubject::Onboarding(onboarding.id))
                .ok_or_else(|| {
                    ApiError::conflict("SourceDeliveryIntent onboarding provenance is unavailable")
                })
        }
        _ => Err(ApiError::conflict(
            "SourceDeliveryIntent subject kind is unsupported",
        )),
    }
}

fn derive_provider_check_status(required_checks: &Value) -> Result<&'static str, ApiError> {
    let checks = required_checks
        .as_array()
        .ok_or_else(|| ApiError::bad_request("required_checks must be an array"))?;
    if checks.len() > 100 {
        return Err(ApiError::bad_request(
            "required_checks exceeds the bounded provider inventory",
        ));
    }
    let mut status = "passing";
    for check in checks {
        match check.get("status").and_then(Value::as_str) {
            Some("failed") => return Ok("failed"),
            Some("passing") => {}
            Some("pending") => status = "pending",
            _ => {
                return Err(ApiError::bad_request(
                    "required check has an invalid status",
                ))
            }
        }
    }
    Ok(status)
}

async fn seal_source_delivery_closure(
    state: &AppState,
    work_item_id: &str,
    intent: &StoredSourceDeliveryIntent,
    provider: &pharness_store::StoredProviderCheckSetObservation,
    status: &str,
    stop_reason: &str,
    merge_commit_sha: Option<&str>,
) -> Result<(), ApiError> {
    let existing = state
        .store
        .list_effective_stage_outcomes(work_item_id)
        .await?;
    if existing
        .iter()
        .any(|outcome| outcome.stage_key == "source_delivery")
    {
        seal_repo_inapplicable_tail(&state.store, work_item_id).await?;
        return Ok(());
    }
    let input = json!({
        "source_delivery_intent_id":intent.id,
        "subject_kind":intent.subject_kind,
        "subject_id":intent.subject_id,
        "base_commit":intent.base_commit,
        "approved_head_sha":intent.pull_request.as_ref().and_then(|pr| pr.get("head_sha")),
        "provider_check_observation_id":provider.id,
        "provider_check_observation_hash":provider.content_hash,
        "merge_commit_sha":merge_commit_sha,
    });
    let execution = state
        .store
        .create_stage_execution(CreateStageExecution {
            id: new_prefixed_id("stageexec"),
            work_item_id: work_item_id.into(),
            stage_key: "source_delivery".into(),
            sequence: 1,
            status: status.into(),
            agent_profile_id: None,
            agent_profile_version: None,
            agent_profile_hash: None,
            context_pack_id: None,
            run_id: None,
            workspace_id: None,
            input_hash: canonical_material_hash(&input)?,
            input_snapshot: input.clone(),
        })
        .await?;
    let metadata = repo_metadata(state, work_item_id).await?;
    let outcome = json!({
        "schema_version":pharness_core::STAGE_OUTCOME_SCHEMA,
        "work_item_id":work_item_id,
        "stage_execution_id":execution.id,
        "stage":"source_delivery",
        "status":status,
        "objective":{"kind":"deliver_reviewed_source_change"},
        "pinned_inputs":input,
        "verified_facts":[{"kind":"provider_check_set","id":provider.id,"hash":provider.content_hash,"status":provider.status}],
        "agent_claims":[],
        "outputs":[{"kind":"source_delivery_intent","id":intent.id}],
        "acceptance":[],"decisions":[],"authorizations":[intent.authorization],
        "contradictions":if status == "succeeded" {json!([])} else {json!([{"kind":"source_delivery_failure","reason":stop_reason}])},
        "risks":[],"unavailable_capabilities":[],"recommendations":[],
        "stop_reason":stop_reason,"sealed_state_version":metadata.state_version,
    });
    state.store.create_evidence_validation(CreateEvidenceValidation {
        id:new_prefixed_id("evalid"), work_item_id:work_item_id.into(), stage_execution_id:Some(execution.id.clone()),
        validator_key:"source_delivery_merge_provenance".into(), status:if status == "succeeded" {"valid".into()} else {"invalid".into()},
        subject:json!({"source_delivery_intent_id":intent.id}),
        evidence_refs:json!([{"kind":"provider_check_set_observation","id":provider.id,"hash":provider.content_hash}]),
        facts:json!({"head_sha":provider.head_sha,"required_set_hash":provider.required_set_hash,"merge_commit_sha":merge_commit_sha}),
        contradictions:outcome.get("contradictions").cloned().unwrap_or_else(|| json!([])),
        content_hash:canonical_material_hash(&json!({"provider":provider.content_hash,"status":status,"merge_commit_sha":merge_commit_sha}))?,
    }).await?;
    state
        .store
        .seal_stage_outcome(SealStageOutcome {
            id: new_prefixed_id("stageout"),
            stage_execution_id: execution.id,
            work_item_id: work_item_id.into(),
            stage_key: "source_delivery".into(),
            status: status.into(),
            content_hash: canonical_material_hash(&outcome)?,
            outcome,
            state_version: metadata.state_version,
            supersedes_outcome_id: None,
            effective: true,
            actor: "controller:repo-mode".into(),
            reason: stop_reason.into(),
        })
        .await?;
    seal_repo_inapplicable_tail(&state.store, work_item_id).await?;
    Ok(())
}

async fn append_repo_audit(
    state: &AppState,
    work_item_id: &str,
    kind: &str,
    actor: &str,
    reason: &str,
    payload: Value,
) -> Result<(), ApiError> {
    state
        .store
        .create_audit_event(CreateAuditEvent {
            id: new_prefixed_id("audit"),
            kind: kind.into(),
            actor: Some(actor.into()),
            resource_kind: "work_item".into(),
            resource_id: work_item_id.into(),
            run_id: None,
            payload_json: json!({"reason":reason,"details":payload}),
        })
        .await?;
    seal_repo_inapplicable_tail(&state.store, work_item_id).await?;
    Ok(())
}

pub(in crate::app) async fn seal_repo_inapplicable_tail(
    store: &pharness_store::SqliteStore,
    work_item_id: &str,
) -> Result<(), ApiError> {
    let existing = store.list_effective_stage_outcomes(work_item_id).await?;
    for stage in [
        pharness_core::RepoStageKey::Release,
        pharness_core::RepoStageKey::Observe,
    ] {
        if existing
            .iter()
            .any(|outcome| outcome.stage_key == stage.as_str())
        {
            continue;
        }
        let input = json!({
            "mode":"repo",
            "source_only":true,
            "upstream_stage":"source_delivery",
        });
        let execution = store
            .create_stage_execution(CreateStageExecution {
                id: new_prefixed_id("stageexec"),
                work_item_id: work_item_id.into(),
                stage_key: stage.as_str().into(),
                sequence: 1,
                status: "inapplicable".into(),
                agent_profile_id: None,
                agent_profile_version: None,
                agent_profile_hash: None,
                context_pack_id: None,
                run_id: None,
                workspace_id: None,
                input_hash: canonical_material_hash(&input)?,
                input_snapshot: input.clone(),
            })
            .await?;
        let metadata = store
            .get_repo_work_item_metadata(work_item_id)
            .await?
            .ok_or_else(|| ApiError::not_found("repo_work_item", work_item_id))?;
        let document = pharness_core::StageOutcomeDocument {
            schema_version: pharness_core::STAGE_OUTCOME_SCHEMA.into(),
            work_item_id: work_item_id.into(),
            stage_execution_id: execution.id.clone(),
            stage,
            origin: "controller".into(),
            status: pharness_core::StageTerminalStatus::Inapplicable,
            objective: json!({"kind":"repo_mode_source_only_boundary"}),
            pinned_inputs: input,
            verified_facts: vec![json!({
                "kind":"mode_contract",
                "mode":"repo",
                "source_delivery_only":true,
            })],
            agent_claims: Vec::new(),
            outputs: Vec::new(),
            acceptance: Vec::new(),
            decisions: vec![json!({
                "kind":"controller_applicability",
                "status":"inapplicable",
            })],
            authorizations: Vec::new(),
            contradictions: Vec::new(),
            risks: Vec::new(),
            unavailable_capabilities: Vec::new(),
            recommendations: Vec::new(),
            stop_reason: "Repo Mode V1 closes after observed source merge; deployment and post-deploy observation are out of scope".into(),
            sealed_state_version: metadata.state_version,
        };
        let value = serde_json::to_value(document)
            .map_err(|error| ApiError::internal(error.to_string()))?;
        store
            .seal_stage_outcome(SealStageOutcome {
                id: new_prefixed_id("stageout"),
                stage_execution_id: execution.id,
                work_item_id: work_item_id.into(),
                stage_key: stage.as_str().into(),
                status: "inapplicable".into(),
                content_hash: canonical_material_hash(&value)?,
                outcome: value,
                state_version: metadata.state_version,
                supersedes_outcome_id: None,
                effective: true,
                actor: "controller:repo-mode".into(),
                reason: "Repo Mode V1 source-only lifecycle boundary".into(),
            })
            .await?;
    }
    Ok(())
}

async fn start_repo_planner(
    state: &AppState,
    work_item_id: &str,
    actor: &str,
    reason: &str,
) -> Result<Value, ApiError> {
    let planned_execution = super::agent_hosts::latest_planned_execution_selection(
        state,
        "work_item",
        work_item_id,
        "plan",
    )
    .await?;
    if planned_execution.is_none() && !state.worker.supports_remote_workspace() {
        return Err(ApiError::unavailable(
            "Repo Mode planner execution requires kubernetes_job worker mode",
        ));
    }
    let metadata = repo_metadata(state, work_item_id).await?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let outcomes = state
        .store
        .list_effective_stage_outcomes(work_item_id)
        .await?;
    let annotations = state.store.list_operator_annotations(work_item_id).await?;
    let evidence = agent_evidence_bundle(state, &metadata, &outcomes).await?;
    let model = state
        .worker
        .config_json()
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unconfigured")
        .to_string();
    let mut profile = state
        .compiled_agent_profiles(&model)
        .into_iter()
        .find(|profile| profile.id == "repo-planner")
        .ok_or_else(|| ApiError::internal("compiled repo-planner profile is unavailable"))?;
    let stage_execution_id = new_prefixed_id("stageexec");
    let context_pack_id = new_prefixed_id("context");
    let run_id = RunId::new(new_prefixed_id("run"));
    let session_id = SessionId::new(new_prefixed_id("ses"));
    let plan_sequence = state
        .store
        .list_stage_executions(work_item_id)
        .await?
        .iter()
        .filter(|execution| execution.stage_key == pharness_core::RepoStageKey::Plan.as_str())
        .count() as u64
        + 1;
    let context = json!({
        "schema_version":pharness_core::AGENT_CONTEXT_SCHEMA,
        "current_intent":{"title":work_item.title,"intent":work_item.intent,"acceptance":metadata.acceptance_command_names},
        "pinned_product":{"snapshot_id":metadata.product_model_snapshot_id,"snapshot_hash":metadata.product_model_snapshot_hash},
        "pinned_repository":{"repository_id":metadata.repository_id,"source_commit":work_item.source_commit,"contract_version_id":metadata.repository_contract_version_id},
        "pinned_context_repositories":metadata.context_repositories,
        "upstream_outcomes":outcomes.iter().map(|outcome| json!({"id":outcome.id,"stage":outcome.stage_key,"status":outcome.status,"hash":outcome.content_hash})).collect::<Vec<_>>(),
        "remaining_budgets":profile.budget,
        "policies":{"source_only":true,"manual_merge":true,"pipeline":false,"deployment":false},
        "grants":[],
        "contradictions":annotation_contradictions(&annotations),
        "risks":[],
        "operator_decisions":annotation_context(&annotations),
        "evidence_catalog":evidence.catalog,
    });
    let estimated_tokens = u64::try_from(context.to_string().len() / 4).unwrap_or(u64::MAX);
    if estimated_tokens > 16_000 {
        return Err(ApiError::conflict(
            "mandatory Planner context exceeds the 16,000-token context-pack limit",
        ));
    }
    let planner_workspace = if planned_execution.is_some() {
        Some(
            state
                .store
                .create_workspace(CreateWorkspace {
                    id: new_prefixed_id("ws"),
                    work_item_id: work_item_id.into(),
                    run_id: Some(run_id.clone()),
                    status: "provisioning".into(),
                    source_repo: work_item.source_repo.clone(),
                    source_ref: work_item.source_ref.clone(),
                    resolved_commit: work_item.source_commit.clone(),
                    branch: Some(format!("pharness/{work_item_id}/planner-{plan_sequence}")),
                    retention_status: "retained".into(),
                    actor: Some(actor.into()),
                    reason: Some(reason.into()),
                })
                .await?,
        )
    } else {
        None
    };
    let cwd = if planned_execution.is_some() {
        "/workspace".to_string()
    } else {
        state.worker.effective_cwd("/workspace")
    };
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: format!("Repo Planner: {}", work_item.title),
            cwd: cwd.clone(),
        })
        .await?;
    let scope = RunScope {
        run_id: Some(run_id.to_string()),
        work_item_id: Some(work_item_id.into()),
        repo: Some(work_item.source_repo.clone()),
        branch: Some(work_item.source_ref.clone()),
        workspace_id: planner_workspace
            .as_ref()
            .map(|workspace| workspace.id.clone()),
        ..RunScope::default()
    };
    let (agent_execution_marker, inference_marker, resolved_profile) = if let Some(selection) =
        &planned_execution
    {
        (
            super::agent_hosts::execution_marker(selection),
            json!({"mode":"not_selected","reason":"Planner uses codex_app_server"}),
            Some((
                selection.binding_hash.clone(),
                selection.resolved_binding.policy.model.clone(),
                selection.resolved_binding.policy.prompt_revision.clone(),
            )),
        )
    } else if state.inference.enabled {
        let selection =
            super::inference::latest_planned_selection(state, "work_item", work_item_id, "plan")
                .await?
                .ok_or_else(|| {
                    ApiError::conflict(
                        "Planner inference selection was not pinned at WorkItem creation",
                    )
                })?;
        (
            Value::Null,
            super::inference::execution_marker_for_selection(state, &selection),
            Some((
                selection.resolved_binding.agent_profile_hash.clone(),
                selection.resolved_binding.target.upstream_model.clone(),
                profile.prompt_version.clone(),
            )),
        )
    } else {
        (
            Value::Null,
            super::inference::execution_marker(state, None),
            None,
        )
    };
    if let Some((profile_hash, model, prompt_version)) = resolved_profile {
        profile.profile_hash = profile_hash;
        profile.model = model;
        profile.prompt_version = prompt_version;
    }
    let runner_profile = if planned_execution.is_some() {
        Some(
            super::environment::select_profile(
                &state.environment_profiles,
                work_item
                    .environment_profile_id
                    .as_deref()
                    .ok_or_else(|| ApiError::conflict("EnvironmentProfile is unavailable"))?,
                &work_item.source_repo,
            )
            .map_err(ApiError::conflict)?
            .clone(),
        )
    } else {
        None
    };
    let workspace_source =
        planner_workspace
            .as_ref()
            .map(|workspace| pharness_runhost::WorkspaceSourceSpec {
                workspace_id: workspace.id.clone(),
                source_repo: workspace.source_repo.clone(),
                source_ref: workspace.source_ref.clone(),
                source_commit: work_item.source_commit.clone(),
                branch: workspace.branch.clone().unwrap_or_default(),
                resolved_commit: None,
            });
    let run = state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: format!(
                "Produce a bounded WorkPlan for this exact intent and acceptance contract: {}",
                work_item.intent
            ),
            cwd: cwd.clone(),
            max_turns: profile.budget.initial_turns,
            initial_status: "queued".into(),
            execution_target_json: json!({
                "kind":if planned_execution.is_some() {"agent_host_workspace"} else {state.worker.execution_target_kind()},
                "agent_execution":agent_execution_marker,
                "inference":inference_marker,
                "repo_mode":{"stage_execution_id":stage_execution_id,"stage":"plan","context_pack_id":context_pack_id,"workspace_access":"read_only"},
                "agent_profile":profile,
                "agent_context":context,
                "agent_evidence_payloads":evidence.payloads,
                "run_scope":scope.to_optional_json(),
                "run_budget":profile.budget,
                "workspace_source":workspace_source,
                "environment_profile_id":work_item.environment_profile_id,
                "repository_contract":work_item.repository_contract_json,
                "selected_acceptance_commands":work_item.acceptance_criteria,
                "runner_profile":runner_profile,
            }),
        })
        .await?;
    let run = state
        .store
        .set_run_budget(
            &run.id,
            &profile.budget,
            &RunBudgetConsumption {
                allowed_turns: profile.budget.initial_turns,
                allowed_tokens: profile.budget.initial_tokens,
                ..RunBudgetConsumption::default()
            },
        )
        .await?;
    let run = state.store.set_run_origin(&run.id, "controller").await?;
    let run = state
        .store
        .set_run_created_by(&run.id, Some(actor.into()))
        .await?;
    let input_snapshot = json!({
        "context_pack_id":context_pack_id,
        "context_hash":canonical_material_hash(&context)?,
        "profile_id":profile.id,
        "profile_version":profile.version,
        "profile_hash":profile.profile_hash,
        "source_commit":work_item.source_commit,
    });
    let execution = state
        .store
        .create_stage_execution(CreateStageExecution {
            id: stage_execution_id.clone(),
            work_item_id: work_item_id.into(),
            stage_key: pharness_core::RepoStageKey::Plan.as_str().into(),
            sequence: plan_sequence,
            status: "queued".into(),
            agent_profile_id: Some(profile.id.clone()),
            agent_profile_version: Some(profile.version.clone()),
            agent_profile_hash: Some(profile.profile_hash.clone()),
            context_pack_id: None,
            run_id: Some(run.id.clone()),
            workspace_id: planner_workspace
                .as_ref()
                .map(|workspace| workspace.id.clone()),
            input_hash: canonical_material_hash(&input_snapshot)?,
            input_snapshot,
        })
        .await?;
    let pack = state
        .store
        .create_agent_context_pack(CreateAgentContextPack {
            id: context_pack_id,
            work_item_id: work_item_id.into(),
            stage_execution_id: execution.id.clone(),
            content_hash: canonical_material_hash(&context)?,
            context,
            estimated_tokens,
        })
        .await?;
    state
        .store
        .append_event(&AgentEvent {
            event_id: EventId::new(new_prefixed_id("evt")),
            session_id,
            run_id: run.id.clone(),
            seq: 1,
            kind: EventKind::RunQueued,
            payload: json!({"source":"repo_mode_controller","stage":"plan","stage_execution_id":execution.id,"actor":actor,"reason":reason}),
        })
        .await?;
    let item = state
        .store
        .update_repo_work_item_status(
            work_item_id,
            "executing",
            actor,
            "repo-planner AgentRun started",
            false,
        )
        .await?;
    let lease = if let (Some(planned), Some(workspace)) = (planned_execution, &planner_workspace) {
        Some(
            super::agent_hosts::queue_bound_run(
                state,
                planned,
                &run,
                &execution.id,
                &workspace.id,
                None,
            )
            .await?,
        )
    } else {
        state.worker.spawn_run(run.clone(), cwd);
        None
    };
    Ok(
        json!({"work_item":item,"stage_execution":execution,"context_pack":pack,"run":run,"workspace":planner_workspace,"agent_lease":lease}),
    )
}

async fn authorize_repo_stage_chain(
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
    let profiles = state
        .compiled_agent_profiles(&model)
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
    for (profile_id, stage, requested) in requested_execution {
        if let Some(selection) = super::agent_hosts::create_planned_execution_selection(
            state,
            super::agent_hosts::PlannedExecutionSelectionRequest {
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
            if diagnosis.is_some() {
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
            planned_inference.push(
                super::inference::create_planned_selection(
                    state,
                    super::inference::PlannedSelectionRequest {
                        subject_kind: "work_item",
                        subject_id: work_item_id,
                        stage,
                        profile: &serde_json::to_value(profile)
                            .map_err(|error| ApiError::internal(error.to_string()))?,
                        requested,
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
                "requested_repair_policy":inference_policies.and_then(|value| value.repair.as_ref()),
                "requested_test_diagnosis_policy":inference_policies.and_then(|value| value.test_diagnosis.as_ref()).or_else(|| inference_policies.and_then(|value| value.test.as_ref())),
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

#[allow(clippy::too_many_arguments)]
async fn start_repo_builder(
    state: &AppState,
    metadata: &StoredRepoWorkItemMetadata,
    work_item: &pharness_store::StoredWorkItem,
    plan: &pharness_store::StoredWorkPlan,
    workspace: &pharness_store::StoredWorkspace,
    authorization: &pharness_store::StoredStageChainAuthorization,
    contract: &pharness_core::RepositoryContract,
    actor: &str,
    reason: &str,
    reuse_prepared_workspace: bool,
    builder_profile_id: &str,
    correction_of: Option<&pharness_store::StoredStageOutcome>,
) -> Result<Value, ApiError> {
    let planned_execution = super::agent_hosts::latest_planned_execution_selection(
        state,
        "work_item",
        &work_item.id,
        builder_profile_id,
    )
    .await?;
    if planned_execution.is_none() && !state.worker.enabled() {
        return Err(ApiError::unavailable(
            "model execution worker is unavailable",
        ));
    }
    let mut profile = agent_profile_from_chain(&authorization.profile_chain, builder_profile_id)
        .ok_or_else(|| {
            ApiError::conflict(format!(
                "chain authorization has no {builder_profile_id} profile"
            ))
        })?;
    let effective_budget = if builder_profile_id == "repo-builder" {
        work_item.run_budget.clone()
    } else {
        profile.budget.clone()
    };
    let environment_profile = super::environment::select_profile(
        &state.environment_profiles,
        &contract.environment_profile,
        &work_item.source_repo,
    )
    .map_err(ApiError::conflict)?
    .clone();
    contract
        .validate_for_profile(&environment_profile)
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    let reused_environment_snapshot = if reuse_prepared_workspace {
        latest_correction_environment_snapshot(state, work_item, workspace, &environment_profile)
            .await?
    } else {
        None
    };
    let reuse_prepared_environment = reused_environment_snapshot.is_some();
    if planned_execution.is_none() && !state.worker.supports_remote_workspace() {
        return Err(ApiError::conflict(
            "Repo Mode V1 immutable runner preparation requires kubernetes_job worker mode",
        ));
    }
    let run_id = RunId::new(new_prefixed_id("run"));
    let session_id = SessionId::new(new_prefixed_id("ses"));
    let stage_execution_id = new_prefixed_id("stageexec");
    let context_pack_id = new_prefixed_id("context");
    let branch = workspace
        .branch
        .clone()
        .ok_or_else(|| ApiError::conflict("authorized workspace has no branch"))?;
    let source_commit = work_item
        .source_commit
        .clone()
        .ok_or_else(|| ApiError::conflict("Repo Mode Builder requires source_commit"))?;
    let source = pharness_runhost::WorkspaceSourceSpec {
        workspace_id: workspace.id.clone(),
        source_repo: work_item.source_repo.clone(),
        source_ref: work_item.source_ref.clone(),
        source_commit: Some(source_commit.clone()),
        branch: branch.clone(),
        // A correction always keeps the existing detached-base checkout and
        // uncommitted Builder diff. Setting the resolved commit makes the
        // preparation worker verify that preserved checkout rather than
        // trying to clone over a nonempty PVC.
        resolved_commit: reuse_prepared_workspace.then(|| source_commit.clone()),
    };
    state
        .workspace
        .remote_source_allowed(&source)
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    let outcomes = state
        .store
        .list_effective_stage_outcomes(&work_item.id)
        .await?;
    let annotations = state.store.list_operator_annotations(&work_item.id).await?;
    let evidence = agent_evidence_bundle(state, metadata, &outcomes).await?;
    let plan_snapshot = json!({
        "id":plan.id,
        "revision":plan.revision,
        "status":plan.status,
        "title":plan.title,
        "summary":plan.summary,
        "risk_level":plan.risk_level,
        "work_plan":plan.work_plan_json,
    });
    let plan_hash = canonical_material_hash(&plan_snapshot)?;
    let mut operator_decisions =
        vec![json!({"kind":"work_plan_approval","actor":actor,"reason":reason})];
    operator_decisions.extend(annotation_context(&annotations));
    let context = json!({
        "schema_version":pharness_core::AGENT_CONTEXT_SCHEMA,
        "current_intent":{"title":work_item.title,"intent":work_item.intent,"acceptance_names":metadata.acceptance_command_names,"acceptance_commands":work_item.acceptance_criteria},
        "pinned_product":{"snapshot_id":metadata.product_model_snapshot_id,"snapshot_hash":metadata.product_model_snapshot_hash},
        "pinned_repository":{"repository_id":metadata.repository_id,"source_commit":source_commit,"contract_version_id":metadata.repository_contract_version_id,"contract_hash":work_item.repository_contract_hash},
        "pinned_context_repositories":metadata.context_repositories,
        "approved_work_plan":{"snapshot":plan_snapshot,"hash":plan_hash},
        "upstream_outcomes":outcomes.iter().map(|outcome| json!({"id":outcome.id,"stage":outcome.stage_key,"status":outcome.status,"hash":outcome.content_hash})).collect::<Vec<_>>(),
        "remaining_budgets":effective_budget,
        "correction":correction_of.map(|outcome| json!({
            "outcome_id":outcome.id,
            "stage":outcome.stage_key,
            "status":outcome.status,
            "content_hash":outcome.content_hash,
            "findings":outcome.outcome,
        })),
        "policies":{"source_only":true,"manual_merge":true,"agent_network":"denied","package_installation":"preparation_only"},
        "grants":[{"kind":"stage_chain","id":authorization.id,"expires_at":authorization.expires_at,"workspace_id":workspace.id,"writable_paths":contract.writable_paths}],
        "contradictions":annotation_contradictions(&annotations),
        "risks":[],
        "operator_decisions":operator_decisions,
        "evidence_catalog":evidence.catalog,
    });
    let estimated_tokens = u64::try_from(context.to_string().len() / 4).unwrap_or(u64::MAX);
    if estimated_tokens > 16_000 {
        return Err(ApiError::conflict(
            "mandatory Builder context exceeds the 16,000-token context-pack limit",
        ));
    }
    let cwd = if planned_execution.is_some() {
        "/workspace".to_string()
    } else {
        state.worker.effective_cwd("/workspace")
    };
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: format!("Repo Builder: {}", work_item.title),
            cwd: cwd.clone(),
        })
        .await?;
    let scope = RunScope {
        run_id: Some(run_id.to_string()),
        repo: Some(work_item.source_repo.clone()),
        branch: Some(branch.clone()),
        work_item_id: Some(work_item.id.clone()),
        workspace_id: Some(workspace.id.clone()),
        work_plan_id: Some(plan.id.clone()),
        production_impacting: false,
        ..RunScope::default()
    };
    let grant = create_permission_grant_record(
        &state.store,
        CreatePermissionGrantRequest {
            subject: state.policy.subject.clone(),
            created_by: Some(actor.into()),
            reason: format!(
                "Repo Mode Builder grant for WorkItem {} chain {}",
                work_item.id, authorization.id
            ),
            scope: json!({
                "environment":state.policy.environment,
                "capability_kinds":["filesystem"],
                "actions":["write_file","patch_file","apply_patch","create_directory"],
                "max_risk":"medium",
                "repos":[work_item.source_repo],
                "branches":[branch],
                "run_ids":[run_id.to_string()],
                "workspace_ids":[workspace.id],
                "writable_path_globs":contract.writable_paths,
                "work_item_ids":[work_item.id],
                "work_plan_ids":[plan.id],
                "production_impacting":false,
            }),
            policy: json!({"policy_mode":"trusted_writes"}),
            expires_at: Some(authorization.expires_at.clone()),
        },
    )
    .await?;
    let mut policy = super::policy::run_policy(&state.policy, None);
    policy.permission_grants = super::approvals::active_permission_grants(&state.store).await?;
    let (agent_execution_marker, inference_marker, resolved_profile) =
        if let Some(selection) = &planned_execution {
            (
                super::agent_hosts::execution_marker(selection),
                json!({"mode":"not_selected","reason":"stage uses codex_app_server"}),
                Some((
                    selection.binding_hash.clone(),
                    selection.resolved_binding.policy.model.clone(),
                    selection.resolved_binding.policy.prompt_revision.clone(),
                )),
            )
        } else if state.inference.enabled {
            let selection = super::inference::latest_planned_selection_for_profile(
                state,
                "work_item",
                &work_item.id,
                "implement",
                builder_profile_id,
            )
            .await?
            .ok_or_else(|| {
                ApiError::conflict(format!(
                    "{builder_profile_id} inference selection is unavailable"
                ))
            })?;
            (
                Value::Null,
                super::inference::execution_marker_for_selection(state, &selection),
                Some((
                    selection.resolved_binding.agent_profile_hash.clone(),
                    selection.resolved_binding.target.upstream_model.clone(),
                    profile.prompt_version.clone(),
                )),
            )
        } else {
            (
                Value::Null,
                super::inference::execution_marker(state, None),
                None,
            )
        };
    if let Some((profile_hash, model, prompt_version)) = resolved_profile {
        profile.profile_hash = profile_hash;
        profile.model = model;
        profile.prompt_version = prompt_version;
    }
    let mut execution_target = json!({
        "kind":if planned_execution.is_some() {"agent_host_workspace"} else {"kubernetes_workspace"},
        "agent_execution":agent_execution_marker,
        "inference":inference_marker,
        "repo_mode":{"stage_execution_id":stage_execution_id,"stage":"implement","context_pack_id":context_pack_id,"chain_authorization_id":authorization.id},
        "agent_profile":profile,
        "agent_context":context,
        "agent_evidence_payloads":evidence.payloads,
        "policy":policy,
        "run_scope":scope.to_optional_json(),
        "workspace":{"base_commit":source_commit,"branch":branch},
        "workspace_source":source,
        "run_budget":effective_budget,
        "environment_profile_id":work_item.environment_profile_id,
        "repository_contract":work_item.repository_contract_json,
        "selected_acceptance_commands":work_item.acceptance_criteria,
        "runner_profile":environment_profile,
        "environment_preparation_required":!reuse_prepared_environment,
    });
    if let Some(snapshot) = reused_environment_snapshot.clone() {
        execution_target["environment_snapshot"] = snapshot;
    }
    let run = state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: if builder_profile_id == "repo-repair" {
                format!(
                    "Repair the existing implementation using the exact sealed failure findings for this Repo Mode intent: {}",
                    work_item.intent
                )
            } else {
                format!(
                    "Implement the approved WorkPlan for this exact Repo Mode intent: {}",
                    work_item.intent
                )
            },
            cwd: cwd.clone(),
            max_turns: effective_budget.initial_turns,
            initial_status: if reuse_prepared_environment {
                "queued".into()
            } else {
                "preparing".into()
            },
            execution_target_json: execution_target,
        })
        .await?;
    let run = state
        .store
        .set_run_budget(
            &run.id,
            &effective_budget,
            &RunBudgetConsumption {
                allowed_turns: effective_budget.initial_turns,
                allowed_tokens: effective_budget.initial_tokens,
                ..RunBudgetConsumption::default()
            },
        )
        .await?;
    let run = state.store.set_run_origin(&run.id, "controller").await?;
    let run = state
        .store
        .set_run_created_by(&run.id, Some(actor.into()))
        .await?;
    let input_snapshot = json!({
        "chain_authorization_id":authorization.id,
        "chain_state_hash":authorization.state_hash,
        "context_pack_id":context_pack_id,
        "context_hash":canonical_material_hash(&context)?,
        "profile_id":profile.id,
        "profile_version":profile.version,
        "profile_hash":profile.profile_hash,
        "work_plan_id":plan.id,
        "work_plan_revision":plan.revision,
        "work_plan_hash":plan_hash,
        "correction_of":correction_of.map(|outcome| json!({"outcome_id":outcome.id,"content_hash":outcome.content_hash,"stage":outcome.stage_key})),
        "source_commit":source_commit,
        "workspace_id":workspace.id,
    });
    let implement_sequence = state
        .store
        .list_stage_executions(&work_item.id)
        .await?
        .iter()
        .filter(|execution| execution.stage_key == pharness_core::RepoStageKey::Implement.as_str())
        .count() as u64
        + 1;
    let execution = state
        .store
        .create_stage_execution(CreateStageExecution {
            id: stage_execution_id,
            work_item_id: work_item.id.clone(),
            stage_key: pharness_core::RepoStageKey::Implement.as_str().into(),
            sequence: implement_sequence,
            status: if reuse_prepared_environment {
                "queued".into()
            } else {
                "preparing".into()
            },
            agent_profile_id: Some(profile.id.clone()),
            agent_profile_version: Some(profile.version.clone()),
            agent_profile_hash: Some(profile.profile_hash.clone()),
            context_pack_id: None,
            run_id: Some(run.id.clone()),
            workspace_id: Some(workspace.id.clone()),
            input_hash: canonical_material_hash(&input_snapshot)?,
            input_snapshot,
        })
        .await?;
    let pack = state
        .store
        .create_agent_context_pack(CreateAgentContextPack {
            id: context_pack_id,
            work_item_id: work_item.id.clone(),
            stage_execution_id: execution.id.clone(),
            content_hash: canonical_material_hash(&context)?,
            context,
            estimated_tokens,
        })
        .await?;
    state
        .store
        .create_evidence_validation(CreateEvidenceValidation {
            id: new_prefixed_id("evalid"),
            work_item_id: work_item.id.clone(),
            stage_execution_id: Some(execution.id.clone()),
            validator_key: "approved_work_plan_snapshot".into(),
            status: "valid".into(),
            subject: json!({"work_plan_id":plan.id,"revision":plan.revision}),
            evidence_refs: json!([]),
            facts: json!({"snapshot_hash":plan_hash,"status":plan.status}),
            contradictions: json!([]),
            content_hash: canonical_material_hash(&plan_snapshot)?,
        })
        .await?;
    let workspace = state
        .store
        .update_workspace_execution(
            &workspace.id,
            UpdateWorkspaceExecution {
                run_id: Some(run.id.clone()),
                status: if reuse_prepared_environment {
                    "running".into()
                } else {
                    "preparing".into()
                },
                resolved_commit: Some(source_commit.clone()),
                branch: Some(branch),
                actor: Some(actor.into()),
                reason: Some(reason.into()),
            },
        )
        .await?;
    if correction_of.is_some() {
        state
            .store
            .start_work_item_internal_correction(
                &work_item.id,
                &run.id,
                Some(actor.into()),
                Some(reason.into()),
            )
            .await?;
    } else {
        state
            .store
            .start_work_item_attempt(
                &work_item.id,
                &run.id,
                Some(actor.into()),
                Some(reason.into()),
            )
            .await?;
    }
    if let Some(planned) = planned_execution {
        let pinned_host_id = if reuse_prepared_workspace {
            super::agent_hosts::sticky_workspace_host(state, &workspace.id).await?
        } else {
            None
        };
        let lease = super::agent_hosts::queue_bound_run(
            state,
            planned,
            &run,
            &execution.id,
            &workspace.id,
            pinned_host_id,
        )
        .await?;
        let preparation = if reuse_prepared_environment {
            None
        } else {
            Some(
                state
                    .store
                    .create_environment_preparation(CreateEnvironmentPreparation {
                        id: new_prefixed_id("prep"),
                        work_item_id: work_item.id.clone(),
                        workspace_id: workspace.id.clone(),
                        run_id: Some(run.id.clone()),
                        status: "queued".into(),
                        environment_profile_id: environment_profile.id.clone(),
                        source_commit,
                    })
                    .await?,
            )
        };
        return Ok(json!({
            "run":run,
            "stage_execution":execution,
            "context_pack":pack,
            "workspace":workspace,
            "permission_grant":grant,
            "environment_preparation":preparation,
            "reused_environment_snapshot":reuse_prepared_environment,
            "agent_lease":lease,
        }));
    }
    if reuse_prepared_environment {
        state.worker.spawn_run(run.clone(), cwd);
        return Ok(json!({
            "run":run,
            "stage_execution":execution,
            "context_pack":pack,
            "workspace":workspace,
            "permission_grant":grant,
            "environment_preparation":null,
            "reused_environment_snapshot":true,
        }));
    }
    let preparation = state
        .store
        .create_environment_preparation(CreateEnvironmentPreparation {
            id: new_prefixed_id("prep"),
            work_item_id: work_item.id.clone(),
            workspace_id: workspace.id.clone(),
            run_id: Some(run.id.clone()),
            status: "queued".into(),
            environment_profile_id: environment_profile.id.clone(),
            source_commit,
        })
        .await?;
    let receipt = state
        .worker
        .dispatch_environment_preparation(&run, &environment_profile)
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let preparation = state
        .store
        .update_environment_preparation(UpdateEnvironmentPreparation {
            id: preparation.id,
            status: "running".into(),
            project_contract_json: None,
            project_contract_hash: None,
            environment_snapshot_json: None,
            logs_json: json!([{"step":"dispatch","status":"succeeded","job_name":receipt.job_name}]),
            error: None,
        })
        .await?;
    Ok(json!({
        "run":run,
        "stage_execution":execution,
        "context_pack":pack,
        "workspace":workspace,
        "permission_grant":grant,
        "environment_preparation":preparation,
        "refreshed_environment_snapshot":reuse_prepared_workspace,
    }))
}

fn reusable_correction_environment_snapshot(
    snapshot: Value,
    source_commit: &str,
    repository_contract_hash: &str,
    environment_profile: &pharness_core::EnvironmentProfile,
) -> Result<Option<Value>, ApiError> {
    let typed: pharness_core::EnvironmentSnapshot = serde_json::from_value(snapshot.clone())
        .map_err(|error| {
            ApiError::conflict(format!(
                "correction EnvironmentSnapshot is invalid: {error}"
            ))
        })?;
    if typed.source_sha != source_commit || typed.manifest_sha256 != repository_contract_hash {
        return Err(ApiError::conflict(
            "correction EnvironmentSnapshot no longer matches the pinned source or contract",
        ));
    }
    if typed.runner_image_digest != environment_profile.image
        || typed.runner_revision != environment_profile.revision
    {
        // Runner provenance changed after the original attempt. Preserve the
        // exact source PVC, but require a new isolated preparation Job to
        // verify the checkout and seal a snapshot for the current runner.
        return Ok(None);
    }
    Ok(Some(snapshot))
}

fn correction_environment_snapshot_for_reuse(
    snapshot: Option<Value>,
    source_commit: &str,
    repository_contract_hash: &str,
    environment_profile: &pharness_core::EnvironmentProfile,
) -> Result<Option<Value>, ApiError> {
    let Some(snapshot) = snapshot else {
        // A preparation failure can occur after the exact checkout is written
        // to the durable PVC but before an EnvironmentSnapshot is sealed. A
        // correction must preserve that workspace and run preparation again;
        // there is no prior environment provenance that is safe to reuse.
        return Ok(None);
    };
    reusable_correction_environment_snapshot(
        snapshot,
        source_commit,
        repository_contract_hash,
        environment_profile,
    )
}

async fn latest_correction_environment_snapshot(
    state: &AppState,
    work_item: &pharness_store::StoredWorkItem,
    workspace: &pharness_store::StoredWorkspace,
    environment_profile: &pharness_core::EnvironmentProfile,
) -> Result<Option<Value>, ApiError> {
    let executions = state.store.list_stage_executions(&work_item.id).await?;
    for execution in executions.iter().rev().filter(|execution| {
        execution.stage_key == pharness_core::RepoStageKey::Implement.as_str()
            && execution.workspace_id.as_deref() == Some(workspace.id.as_str())
    }) {
        let Some(run_id) = execution.run_id.as_ref() else {
            continue;
        };
        let Some(run) = state.store.get_run(run_id).await? else {
            continue;
        };
        let Some(snapshot) = run
            .execution_target_json
            .get("environment_snapshot")
            .filter(|snapshot| !snapshot.is_null())
            .cloned()
        else {
            continue;
        };
        return correction_environment_snapshot_for_reuse(
            Some(snapshot),
            work_item.source_commit.as_deref().unwrap_or_default(),
            work_item
                .repository_contract_hash
                .as_deref()
                .unwrap_or_default(),
            environment_profile,
        );
    }
    correction_environment_snapshot_for_reuse(
        None,
        work_item.source_commit.as_deref().unwrap_or_default(),
        work_item
            .repository_contract_hash
            .as_deref()
            .unwrap_or_default(),
        environment_profile,
    )
}

fn agent_profile_from_chain(
    profile_chain: &Value,
    profile_id: &str,
) -> Option<pharness_core::AgentProfile> {
    profile_chain
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| {
            serde_json::from_value::<pharness_core::AgentProfile>(value.clone()).ok()
        })
        .find(|profile| profile.id == profile_id)
}

pub(in crate::app) async fn continue_repo_stage_chain(
    state: &AppState,
    completed_run: &pharness_store::StoredRun,
) -> Result<Option<Value>, ApiError> {
    let Some(stage) = completed_run
        .execution_target_json
        .pointer("/repo_mode/stage")
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };
    if completed_run.status != "completed" {
        return Ok(None);
    }
    let Some(execution_id) = completed_run
        .execution_target_json
        .pointer("/repo_mode/stage_execution_id")
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };
    let outcome = state
        .store
        .get_stage_outcome_for_execution(execution_id)
        .await?
        .ok_or_else(|| ApiError::conflict("completed Repo Mode Run has no sealed outcome"))?;
    if completed_run
        .execution_target_json
        .pointer("/repo_mode/test_diagnosis")
        .and_then(Value::as_bool)
        == Some(true)
    {
        if outcome.status != "succeeded" {
            return Ok(None);
        }
        let failed_outcome_id = completed_run
            .execution_target_json
            .pointer("/repo_mode/diagnosis_of_outcome_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::conflict("Test diagnosis has no failed-outcome binding"))?;
        let failed_outcome = state
            .store
            .get_stage_outcome(failed_outcome_id)
            .await?
            .ok_or_else(|| ApiError::conflict("diagnosed Test outcome is unavailable"))?;
        return start_repo_automatic_repair(state, completed_run, &failed_outcome)
            .await
            .map(Some);
    }
    if outcome.status != "succeeded" {
        if state.repo_mode.coding_reliability_v2_enabled
            && matches!(stage, "test" | "verify")
            && repairable_repo_stage_failure(state, completed_run, &outcome).await?
        {
            if stage == "test"
                && state.inference.enabled
                && super::inference::latest_planned_selection_for_profile(
                    state,
                    "work_item",
                    &outcome.work_item_id,
                    "test",
                    "repo-test-diagnoser",
                )
                .await?
                .is_some()
            {
                return start_repo_followup_stage(state, completed_run, "test", Some(&outcome))
                    .await
                    .map(Some);
            }
            return start_repo_automatic_repair(state, completed_run, &outcome)
                .await
                .map(Some);
        }
        return Ok(None);
    }
    match stage {
        "implement" => start_repo_followup_stage(state, completed_run, "test", None)
            .await
            .map(Some),
        "test" => start_repo_followup_stage(state, completed_run, "verify", None)
            .await
            .map(Some),
        _ => Ok(None),
    }
}

async fn repairable_repo_stage_failure(
    state: &AppState,
    run: &pharness_store::StoredRun,
    outcome: &pharness_store::StoredStageOutcome,
) -> Result<bool, ApiError> {
    let executions = state
        .store
        .list_stage_executions(&outcome.work_item_id)
        .await?;
    let implement_count = executions
        .iter()
        .filter(|execution| execution.stage_key == "implement")
        .count();
    if implement_count != 1 {
        return Ok(false);
    }
    match outcome.stage_key.as_str() {
        "test" => {
            if run
                .execution_target_json
                .pointer("/repo_mode/deterministic_test")
                .and_then(Value::as_bool)
                != Some(true)
            {
                return Ok(false);
            }
            let results = state
                .store
                .list_events(&run.id)
                .await?
                .into_iter()
                .filter(|event| {
                    event.kind == EventKind::ToolFinished
                        && event
                            .payload
                            .pointer("/content/acceptance_command")
                            .and_then(Value::as_bool)
                            == Some(true)
                })
                .collect::<Vec<_>>();
            Ok(!results.is_empty()
                && results.iter().all(|event| {
                    event
                        .payload
                        .pointer("/content/exit_code")
                        .and_then(Value::as_i64)
                        .is_some_and(|code| !matches!(code, 126 | 127))
                })
                && results.iter().any(|event| {
                    event
                        .payload
                        .pointer("/content/exit_code")
                        .and_then(Value::as_i64)
                        != Some(0)
                }))
        }
        "verify" => Ok(run.status == "completed"
            && outcome
                .outcome
                .pointer("/verified_facts/0/typed_decision")
                .and_then(Value::as_str)
                .is_some_and(|decision| decision != "approved")),
        _ => Ok(false),
    }
}

async fn start_repo_automatic_repair(
    state: &AppState,
    completed_run: &pharness_store::StoredRun,
    failed_outcome: &pharness_store::StoredStageOutcome,
) -> Result<Value, ApiError> {
    let work_item_id = &failed_outcome.work_item_id;
    let metadata = repo_metadata(state, work_item_id).await?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let plan = state
        .store
        .get_work_plan_by_work_item(work_item_id)
        .await?
        .filter(|plan| plan.status == "approved")
        .ok_or_else(|| ApiError::conflict("approved WorkPlan is no longer current"))?;
    let authorization = state
        .store
        .active_stage_chain_authorization(work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("stage-chain authorization is unavailable"))?;
    let workspace = state
        .store
        .get_workspace(&authorization.workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("workspace", &authorization.workspace_id))?;
    let contract: pharness_core::RepositoryContract = serde_json::from_value(
        work_item
            .repository_contract_json
            .clone()
            .ok_or_else(|| ApiError::conflict("RepositoryContract is unavailable"))?,
    )
    .map_err(|error| ApiError::internal(error.to_string()))?;
    append_repo_audit(
        state,
        work_item_id,
        "repo.stage_chain.automatic_repair_started",
        "controller:repo-mode",
        "one bounded repair execution after a repairable deterministic finding",
        json!({
            "trigger_run_id":completed_run.id,
            "failed_outcome_id":failed_outcome.id,
            "failed_stage_execution_id":failed_outcome.stage_execution_id,
            "failed_stage":failed_outcome.stage_key,
            "max_internal_corrections":1,
        }),
    )
    .await?;
    start_repo_builder(
        state,
        &metadata,
        &work_item,
        &plan,
        &workspace,
        &authorization,
        &contract,
        "controller:repo-mode",
        "automatic bounded repair from sealed stage findings",
        true,
        "repo-repair",
        Some(failed_outcome),
    )
    .await
}

pub(in crate::app) async fn record_repo_chain_continuation_failure(
    state: &AppState,
    completed_run: &pharness_store::StoredRun,
) -> Result<(), ApiError> {
    let Some(work_item_id) = completed_run
        .execution_target_json
        .pointer("/run_scope/work_item_id")
        .and_then(Value::as_str)
    else {
        return Ok(());
    };
    if let Some(chain) = state
        .store
        .active_stage_chain_authorization(work_item_id)
        .await?
    {
        state
            .store
            .revoke_stage_chain_authorization(
                &chain.id,
                "authorized stage continuation could not be dispatched",
            )
            .await?;
    }
    state
        .store
        .update_repo_work_item_status(
            work_item_id,
            "blocked",
            "controller:repo-mode",
            "authorized stage continuation failed and requires operator correction",
            false,
        )
        .await?;
    append_repo_audit(
        state,
        work_item_id,
        "repo.stage_chain.continuation_failed",
        "controller:repo-mode",
        "automatic dispatch failed after the previous Run was durably finalized",
        json!({"run_id":completed_run.id,"error_code":"stage_continuation_dispatch_failed"}),
    )
    .await
}

async fn start_repo_followup_stage(
    state: &AppState,
    completed_run: &pharness_store::StoredRun,
    stage: &str,
    diagnosis_of: Option<&pharness_store::StoredStageOutcome>,
) -> Result<Value, ApiError> {
    let work_item_id = completed_run
        .execution_target_json
        .pointer("/run_scope/work_item_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("Repo Mode Run has no WorkItem scope"))?;
    let metadata = repo_metadata(state, work_item_id).await?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let plan = state
        .store
        .get_work_plan_by_work_item(work_item_id)
        .await?
        .filter(|plan| plan.status == "approved")
        .ok_or_else(|| ApiError::conflict("approved WorkPlan is no longer current"))?;
    let authorization = state
        .store
        .active_stage_chain_authorization(work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("stage-chain authorization is unavailable"))?;
    let now = current_millis();
    if authorization
        .expires_at
        .parse::<u128>()
        .ok()
        .map_or(true, |expires_at| expires_at <= now)
    {
        return Err(ApiError::conflict(
            "stage-chain authorization expired before the next stage",
        ));
    }
    if authorization.work_plan_id != plan.id
        || authorization.work_plan_revision != plan.revision
        || authorization.product_model_snapshot_id != metadata.product_model_snapshot_id
        || authorization.product_model_snapshot_hash != metadata.product_model_snapshot_hash
        || authorization.repository_id != metadata.repository_id
        || work_item.source_commit.as_deref() != Some(authorization.source_commit.as_str())
    {
        return Err(ApiError::conflict(
            "stage-chain authorization no longer matches the pinned WorkItem state",
        ));
    }
    let workspace = state
        .store
        .get_workspace(&authorization.workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("workspace", &authorization.workspace_id))?;
    let test_diagnosis = diagnosis_of.is_some();
    let deterministic_test =
        state.repo_mode.coding_reliability_v2_enabled && stage == "test" && !test_diagnosis;
    let profile_id = match (stage, test_diagnosis) {
        ("test", true) => "repo-test-diagnoser",
        ("test", false) if deterministic_test => "controller-deterministic-test",
        ("test", false) => "repo-tester",
        ("verify", false) => "repo-verifier",
        _ => return Err(ApiError::internal("unsupported Repo Mode follow-up stage")),
    };
    let planned_execution = if deterministic_test || test_diagnosis {
        None
    } else {
        super::agent_hosts::latest_planned_execution_selection(
            state,
            "work_item",
            work_item_id,
            profile_id,
        )
        .await?
    };
    let sticky_host = super::agent_hosts::sticky_workspace_host(state, &workspace.id).await?;
    let controller_test_on_agent_host = deterministic_test && sticky_host.is_some();
    let mut profile = if deterministic_test {
        let budget = pharness_core::RunBudget {
            initial_turns: 1,
            hard_turns: 1,
            initial_tokens: 1,
            hard_tokens: 1,
            active_execution_seconds: 900,
            recoverable_tool_errors: 0,
            identical_failures: 1,
            verification_reserve_turns: 0,
        };
        let material = json!({
            "id":profile_id,
            "version":"v2",
            "origin":"controller",
            "deterministic_test":true,
            "budget":budget,
        });
        pharness_core::AgentProfile {
            id: profile_id.into(),
            version: "v2".into(),
            profile_hash: canonical_material_hash(&material)?,
            prompt_version: "controller-deterministic-v1".into(),
            model: "none".into(),
            tools: Vec::new(),
            budget,
        }
    } else {
        agent_profile_from_chain(&authorization.profile_chain, profile_id).ok_or_else(|| {
            ApiError::conflict(format!("chain authorization has no {profile_id} profile"))
        })?
    };
    let runner_profile = completed_run
        .execution_target_json
        .get("runner_profile")
        .cloned()
        .ok_or_else(|| ApiError::conflict("prepared runner profile is unavailable"))?;
    let environment_snapshot = completed_run
        .execution_target_json
        .get("environment_snapshot")
        .cloned()
        .ok_or_else(|| ApiError::conflict("prepared EnvironmentSnapshot is unavailable"))?;
    let repository_contract = work_item
        .repository_contract_json
        .clone()
        .ok_or_else(|| ApiError::conflict("RepositoryContract is unavailable"))?;
    let outcomes = state
        .store
        .list_effective_stage_outcomes(work_item_id)
        .await?;
    let annotations = state.store.list_operator_annotations(work_item_id).await?;
    let evidence = agent_evidence_bundle(state, &metadata, &outcomes).await?;
    let mut contradictions = outcomes
        .iter()
        .flat_map(|outcome| {
            outcome
                .outcome
                .get("contradictions")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    contradictions.extend(annotation_contradictions(&annotations));
    let mut operator_decisions = vec![json!({
        "kind":"work_plan_approval",
        "work_plan_id":plan.id,
        "revision":plan.revision,
    })];
    operator_decisions.extend(annotation_context(&annotations));
    let context = json!({
        "schema_version":pharness_core::AGENT_CONTEXT_SCHEMA,
        "current_intent":{"title":work_item.title,"intent":work_item.intent,"acceptance_names":metadata.acceptance_command_names,"acceptance_commands":work_item.acceptance_criteria},
        "pinned_product":{"snapshot_id":metadata.product_model_snapshot_id,"snapshot_hash":metadata.product_model_snapshot_hash},
        "pinned_repository":{"repository_id":metadata.repository_id,"source_commit":authorization.source_commit,"contract_version_id":metadata.repository_contract_version_id},
        "pinned_context_repositories":metadata.context_repositories,
        "effective_upstream_outcomes":outcomes.iter().map(|outcome| json!({"id":outcome.id,"stage":outcome.stage_key,"status":outcome.status,"hash":outcome.content_hash})).collect::<Vec<_>>(),
        "diagnosis_of":diagnosis_of.map(|outcome| json!({
            "outcome_id":outcome.id,
            "stage_execution_id":outcome.stage_execution_id,
            "content_hash":outcome.content_hash,
            "findings":outcome.outcome,
        })),
        "remaining_budgets":profile.budget,
        "policies":{"source_only":true,"workspace_access":if deterministic_test {"ephemeral_copy"} else {"read_only"},"deterministic_test":deterministic_test,"test_diagnosis":test_diagnosis},
        "grants":[{"kind":"stage_chain","id":authorization.id,"expires_at":authorization.expires_at}],
        "contradictions":contradictions,
        "risks":outcomes.iter().flat_map(|outcome| outcome.outcome.get("risks").and_then(Value::as_array).cloned().unwrap_or_default()).collect::<Vec<_>>(),
        "operator_decisions":operator_decisions,
        "evidence_catalog":evidence.catalog,
    });
    let estimated_tokens = u64::try_from(context.to_string().len() / 4).unwrap_or(u64::MAX);
    if estimated_tokens > 16_000 {
        return Err(ApiError::conflict(
            "mandatory follow-up context exceeds the 16,000-token context-pack limit",
        ));
    }
    let run_id = RunId::new(new_prefixed_id("run"));
    let session_id = SessionId::new(new_prefixed_id("ses"));
    let execution_id = new_prefixed_id("stageexec");
    let context_id = new_prefixed_id("context");
    let source = pharness_runhost::WorkspaceSourceSpec {
        workspace_id: workspace.id.clone(),
        source_repo: workspace.source_repo.clone(),
        source_ref: workspace.source_ref.clone(),
        source_commit: Some(authorization.source_commit.clone()),
        branch: workspace
            .branch
            .clone()
            .ok_or_else(|| ApiError::conflict("workspace branch is unavailable"))?,
        resolved_commit: Some(authorization.source_commit.clone()),
    };
    let cwd = if planned_execution.is_some() || controller_test_on_agent_host {
        "/workspace".to_string()
    } else {
        state.worker.effective_cwd("/workspace")
    };
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: format!("Repo {}: {}", profile_id, work_item.title),
            cwd: cwd.clone(),
        })
        .await?;
    let scope = RunScope {
        run_id: Some(run_id.to_string()),
        repo: Some(work_item.source_repo.clone()),
        branch: workspace.branch.clone(),
        work_item_id: Some(work_item_id.into()),
        workspace_id: Some(workspace.id.clone()),
        work_plan_id: Some(plan.id.clone()),
        production_impacting: false,
        ..RunScope::default()
    };
    let (agent_execution_marker, inference_marker, resolved_profile) =
        if let Some(selection) = &planned_execution {
            (
                super::agent_hosts::execution_marker(selection),
                json!({"mode":"not_selected","reason":"stage uses codex_app_server"}),
                Some((
                    selection.binding_hash.clone(),
                    selection.resolved_binding.policy.model.clone(),
                    selection.resolved_binding.policy.prompt_revision.clone(),
                )),
            )
        } else if deterministic_test {
            (
                if controller_test_on_agent_host {
                    json!({"mode":"controller_deterministic_test","host_pool":"sticky_workspace"})
                } else {
                    Value::Null
                },
                super::inference::execution_marker(state, None),
                None,
            )
        } else if state.inference.enabled {
            let selection = super::inference::latest_planned_selection_for_profile(
                state,
                "work_item",
                work_item_id,
                stage,
                profile_id,
            )
            .await?
            .ok_or_else(|| {
                ApiError::conflict(format!("{profile_id} inference selection is unavailable"))
            })?;
            (
                Value::Null,
                super::inference::execution_marker_for_selection(state, &selection),
                Some((
                    selection.resolved_binding.agent_profile_hash.clone(),
                    selection.resolved_binding.target.upstream_model.clone(),
                    profile.prompt_version.clone(),
                )),
            )
        } else {
            (
                Value::Null,
                super::inference::execution_marker(state, None),
                None,
            )
        };
    if let Some((profile_hash, model, prompt_version)) = resolved_profile {
        profile.profile_hash = profile_hash;
        profile.model = model;
        profile.prompt_version = prompt_version;
    }
    let run = state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: if test_diagnosis {
                "Diagnose the exact controller-recorded deterministic Test failure without modifying source; submit a typed diagnosis."
            } else if deterministic_test {
                "Controller-owned deterministic Test execution."
            } else if stage == "test" {
                "Execute every selected RepositoryContract acceptance command, report exact evidence, and submit the typed Test outcome."
            } else {
                "Verify the approved plan, Builder diff, changed paths, and Test evidence; submit the typed verification decision."
            }
            .into(),
            cwd: cwd.clone(),
            max_turns: profile.budget.initial_turns,
            initial_status: "queued".into(),
            execution_target_json: json!({
                "kind":if planned_execution.is_some() || controller_test_on_agent_host {"agent_host_workspace"} else {"kubernetes_workspace"},
                "agent_execution":agent_execution_marker,
                "inference":inference_marker,
                "repo_mode":{"stage_execution_id":execution_id,"stage":stage,"context_pack_id":context_id,"chain_authorization_id":authorization.id,"workspace_access":if deterministic_test {"ephemeral_copy"} else {"read_only"},"deterministic_test":deterministic_test,"test_diagnosis":test_diagnosis,"diagnosis_of_outcome_id":diagnosis_of.map(|outcome| outcome.id.as_str())},
                "agent_profile":profile,
                "agent_context":context,
                "agent_evidence_payloads":evidence.payloads,
                "run_scope":scope.to_optional_json(),
                "workspace":{"base_commit":authorization.source_commit,"branch":workspace.branch},
                "workspace_source":source,
                "run_budget":profile.budget,
                "environment_profile_id":work_item.environment_profile_id,
                "repository_contract":repository_contract,
                "selected_acceptance_commands":work_item.acceptance_criteria,
                "environment_snapshot":environment_snapshot,
                "runner_profile":runner_profile,
            }),
        })
        .await?;
    let run = state
        .store
        .set_run_budget(
            &run.id,
            &profile.budget,
            &RunBudgetConsumption {
                allowed_turns: profile.budget.initial_turns,
                allowed_tokens: profile.budget.initial_tokens,
                ..RunBudgetConsumption::default()
            },
        )
        .await?;
    let run = state.store.set_run_origin(&run.id, "controller").await?;
    let run = state
        .store
        .set_run_created_by(&run.id, Some("controller:repo-mode".into()))
        .await?;
    let input = json!({
        "chain_authorization_id":authorization.id,
        "context_pack_id":context_id,
        "context_hash":canonical_material_hash(&context)?,
        "profile_id":profile.id,
        "profile_version":profile.version,
        "profile_hash":profile.profile_hash,
        "source_commit":authorization.source_commit,
        "workspace_id":workspace.id,
        "upstream_outcome_hashes":outcomes.iter().map(|outcome| &outcome.content_hash).collect::<Vec<_>>(),
        "diagnosis_of":diagnosis_of.map(|outcome| json!({"outcome_id":outcome.id,"content_hash":outcome.content_hash})),
    });
    let sequence = state
        .store
        .list_stage_executions(work_item_id)
        .await?
        .iter()
        .filter(|execution| execution.stage_key == stage)
        .count() as u64
        + 1;
    let execution = state
        .store
        .create_stage_execution(CreateStageExecution {
            id: execution_id,
            work_item_id: work_item_id.into(),
            stage_key: stage.into(),
            sequence,
            status: "queued".into(),
            agent_profile_id: (!deterministic_test).then(|| profile.id.clone()),
            agent_profile_version: (!deterministic_test).then(|| profile.version.clone()),
            agent_profile_hash: (!deterministic_test).then(|| profile.profile_hash.clone()),
            context_pack_id: None,
            run_id: Some(run.id.clone()),
            workspace_id: Some(workspace.id.clone()),
            input_hash: canonical_material_hash(&input)?,
            input_snapshot: input,
        })
        .await?;
    let pack = state
        .store
        .create_agent_context_pack(CreateAgentContextPack {
            id: context_id,
            work_item_id: work_item_id.into(),
            stage_execution_id: execution.id.clone(),
            content_hash: canonical_material_hash(&context)?,
            context,
            estimated_tokens,
        })
        .await?;
    let workspace = state
        .store
        .update_workspace_execution(
            &workspace.id,
            UpdateWorkspaceExecution {
                run_id: Some(run.id.clone()),
                status: stage.into(),
                resolved_commit: Some(authorization.source_commit.clone()),
                branch: workspace.branch.clone(),
                actor: Some("controller:repo-mode".into()),
                reason: Some(if test_diagnosis {
                    "automatic authorized test-diagnosis dispatch".into()
                } else {
                    format!("automatic authorized {stage} dispatch")
                }),
            },
        )
        .await?;
    state
        .store
        .append_event(&AgentEvent {
            event_id: EventId::new(new_prefixed_id("evt")),
            session_id,
            run_id: run.id.clone(),
            seq: 1,
            kind: EventKind::RunQueued,
            payload: json!({"source":"repo_mode_controller","stage":stage,"test_diagnosis":test_diagnosis,"stage_execution_id":execution.id,"chain_authorization_id":authorization.id}),
        })
        .await?;
    let lease = if let Some(planned) = planned_execution {
        Some(
            super::agent_hosts::queue_bound_run(
                state,
                planned,
                &run,
                &execution.id,
                &workspace.id,
                sticky_host,
            )
            .await?,
        )
    } else if controller_test_on_agent_host {
        Some(
            super::agent_hosts::queue_controller_stage_on_sticky_host(
                state,
                &run,
                &execution.id,
                &workspace.id,
                stage,
            )
            .await?,
        )
    } else {
        state
            .worker
            .spawn_chained_run(run.clone(), cwd, completed_run.id.as_str());
        None
    };
    Ok(
        json!({"run":run,"stage_execution":execution,"context_pack":pack,"workspace":workspace,"agent_lease":lease}),
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
    #[serde(default)]
    planner_inference_policy: Option<pharness_core::InferencePolicyRef>,
    #[serde(default)]
    planner_execution_policy: Option<pharness_core::AgentExecutionPolicyRef>,
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
    #[serde(default)]
    planner_inference_policy: Option<pharness_core::InferencePolicyRef>,
    #[serde(default)]
    planner_execution_policy: Option<pharness_core::AgentExecutionPolicyRef>,
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
    planner_inference: Value,
    planner_execution: Value,
    readiness_assessment_id: Option<String>,
    blockers: Vec<Value>,
    warnings: Vec<Value>,
    predicted_mutations: Vec<String>,
    authorization_boundaries: Vec<Value>,
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
    let planner_execution_selection = super::agent_hosts::create_planned_execution_selection(
        &state,
        super::agent_hosts::PlannedExecutionSelectionRequest {
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
    .await?;
    let planner_selection = if planner_execution_selection.is_none() && state.inference.enabled {
        Some(
            super::inference::create_planned_selection(
                &state,
                super::inference::PlannedSelectionRequest {
                    subject_kind: "work_item",
                    subject_id: &work_item_id,
                    stage: pharness_core::InferenceStage::Plan,
                    profile: &serde_json::to_value(&planner_profile)
                        .map_err(|error| ApiError::internal(error.to_string()))?,
                    requested: preflight_request.planner_inference_policy.as_ref(),
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
    let planner_execution_binding = match environment_profile_id {
        Some(profile_id) => {
            super::agent_hosts::resolve_execution_binding_auto_auth(
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
        super::inference::preview_selection(
            state,
            pharness_core::InferenceStage::Plan,
            &serde_json::to_value(&planner_profile)
                .map_err(|error| ApiError::internal(error.to_string()))?,
            request.planner_inference_policy.as_ref(),
        )
        .await?
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
    let predicted_mutations = if blockers.is_empty() {
        vec![
            "create_repo_work_item".into(),
            "seal_discover_stage_from_readiness".into(),
            "await_explicit_planner_start".into(),
        ]
    } else {
        Vec::new()
    };
    let authorization_boundaries = vec![
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
    ];
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
        preflight_hash,
    })
}

pub(in crate::app) async fn current_readiness_mismatches(
    state: &AppState,
    repository: &pharness_store::StoredRepository,
    source_commit: &str,
    version: &pharness_store::StoredRepositoryContractVersion,
    contract: &pharness_core::RepositoryContract,
    assessment: &pharness_store::StoredRepositoryReadinessAssessment,
) -> Result<Vec<String>, ApiError> {
    let mut mismatches = Vec::new();
    if assessment.contract_status != "ready" || assessment.coding_status != "ready" {
        mismatches.push("assessment_not_ready".into());
    }
    if assessment.source_commit != source_commit
        || assessment.contract_version_id.as_deref() != Some(version.id.as_str())
        || assessment.contract_hash.as_deref() != Some(version.content_hash.as_str())
        || assessment.dependency_lock_hash.as_deref()
            != Some(contract.dependency_lock.sha256.as_str())
        || assessment.validation_policy_version != "repo-mode-v1"
    {
        mismatches.push("contract_or_policy_tuple_changed".into());
    }
    let profile = state.environment_profiles.iter().find(|profile| {
        profile.active
            && profile.id == contract.environment_profile
            && profile
                .repository_allowlist
                .contains(&repository.canonical_url)
    });
    let Some(profile) = profile else {
        mismatches.push("environment_profile_unavailable".into());
        return Ok(mismatches);
    };
    if contract.validate_for_profile(profile).is_err() {
        mismatches.push("environment_profile_contract_mismatch".into());
    }
    let current_digest = profile.image.split_once('@').map(|(_, digest)| digest);
    if assessment.environment_profile_id.as_deref() != Some(profile.id.as_str())
        || assessment.environment_profile_revision.as_deref() != Some(profile.revision.as_str())
        || assessment.runner_image_digest.as_deref() != current_digest
    {
        mismatches.push("environment_profile_tuple_changed".into());
    }
    let now = current_millis();
    if assessment
        .expires_at
        .as_deref()
        .and_then(|expiry| expiry.parse::<u128>().ok())
        .is_some_and(|expiry| expiry <= now)
    {
        mismatches.push("assessment_expired".into());
    }
    let evidence = assessment
        .evidence_refs
        .as_array()
        .cloned()
        .unwrap_or_default();
    let source_evidence_id = evidence.iter().find_map(|entry| {
        (entry.get("kind").and_then(Value::as_str) == Some("capability_verification")
            && entry.get("capability").and_then(Value::as_str) == Some("source_reader"))
        .then(|| entry.get("id").and_then(Value::as_str))
        .flatten()
    });
    let profile_capability = format!("environment_profile:{}", profile.id);
    let profile_evidence_id = evidence.iter().find_map(|entry| {
        (entry.get("kind").and_then(Value::as_str) == Some("capability_verification")
            && entry.get("capability").and_then(Value::as_str) == Some(profile_capability.as_str()))
        .then(|| entry.get("id").and_then(Value::as_str))
        .flatten()
    });
    let source_verification = state
        .store
        .latest_capability_verification_for_repository("source_reader", &repository.canonical_url)
        .await?;
    let profile_verification = state
        .store
        .latest_capability_verification(&profile_capability)
        .await?;
    let source_current = source_verification.as_ref().is_some_and(|verification| {
        source_evidence_id == Some(verification.id.as_str())
            && verification.status == "available"
            && verification.repository.as_deref() == Some(repository.canonical_url.as_str())
            && verification
                .expires_at
                .parse::<u128>()
                .is_ok_and(|expiry| expiry > now)
    });
    let profile_current = profile_verification.as_ref().is_some_and(|verification| {
        profile_evidence_id == Some(verification.id.as_str())
            && verification.status == "available"
            && verification
                .expires_at
                .parse::<u128>()
                .is_ok_and(|expiry| expiry > now)
    });
    if !source_current {
        mismatches.push("source_reader_evidence_stale".into());
    }
    if !profile_current {
        mismatches.push("runner_profile_evidence_stale".into());
    }
    if let (Some(source_verification), Some(profile_verification)) =
        (source_verification, profile_verification)
    {
        let expected_input = json!({
            "schema_version":"pharness.dev/repository-readiness-input/v1alpha1",
            "repository_id":repository.id,
            "source_commit":source_commit,
            "contract_version_id":version.id,
            "contract_hash":version.content_hash,
            "dependency_lock_hash":contract.dependency_lock.sha256,
            "environment_profile_id":profile.id,
            "environment_profile_revision":profile.revision,
            "runner_image":profile.image,
            "validation_policy_version":"repo-mode-v1",
            "required_executables":profile.required_executables,
            "acceptance_commands":contract.acceptance_commands,
            "capability_evidence":{
                "source_reader":{"id":source_verification.id,"verified_at":source_verification.verified_at,"expires_at":source_verification.expires_at},
                "environment_profile":{"id":profile_verification.id,"verified_at":profile_verification.verified_at,"expires_at":profile_verification.expires_at},
            },
        });
        if canonical_material_hash(&expected_input)? != assessment.input_hash {
            mismatches.push("readiness_input_hash_mismatch".into());
        }
    }
    Ok(mismatches)
}

async fn list_stage_executions(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    repo_metadata(&state, &work_item_id).await?;
    let executions = state.store.list_stage_executions(&work_item_id).await?;
    let mut views = Vec::with_capacity(executions.len());
    for execution in &executions {
        views.push(stage_execution_view(&state, execution).await?);
    }
    Ok(Json(json!({
        "stage_executions": views,
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
    Ok(Json(
        json!({"stage_execution": stage_execution_view(&state, &execution).await?}),
    ))
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
    let agent_execution = match execution.run_id.as_ref() {
        Some(run_id) => super::agent_hosts::sanitized_run_agent_execution(&state, run_id).await?,
        None => None,
    };
    Ok(Json(json!({
        "stage_execution_id": execution.id,
        "outcome": outcome,
        "agent_execution":agent_execution,
    })))
}

async fn stage_execution_view(
    state: &AppState,
    execution: &pharness_store::StoredStageExecution,
) -> Result<Value, ApiError> {
    let mut value =
        serde_json::to_value(execution).map_err(|error| ApiError::internal(error.to_string()))?;
    if let Some(object) = value.as_object_mut() {
        let provenance = match execution.run_id.as_ref() {
            Some(run_id) => {
                super::agent_hosts::sanitized_run_agent_execution(state, run_id).await?
            }
            None => None,
        };
        object.insert("agent_execution".into(), provenance.unwrap_or(Value::Null));
    }
    Ok(value)
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
    let decisions = state
        .store
        .list_operator_annotation_decisions(&work_item_id)
        .await?;
    Ok(Json(json!({
        "annotations": annotations,
        "count": annotations.len(),
        "decisions":decisions,
    })))
}

async fn list_work_item_evidence(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    repo_metadata(&state, &work_item_id).await?;
    let validations = state.store.list_evidence_validations(&work_item_id).await?;
    let mut validation_references = Vec::with_capacity(validations.len());
    for validation in &validations {
        validation_references.push(json!({
            "evidence_validation_id": validation.id,
            "typed_references": state
                .store
                .list_evidence_validation_references(&validation.id)
                .await?,
        }));
    }
    let outcomes = state
        .store
        .list_effective_stage_outcomes(&work_item_id)
        .await?;
    Ok(Json(json!({
        "work_item_id": work_item_id,
        "evidence_validations": validations,
        "validation_references": validation_references,
        "effective_stage_outcomes": outcomes,
        "count": validations.len(),
    })))
}

async fn get_evidence_validation(
    State(state): State<AppState>,
    Path(evidence_validation_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    ensure_repo_mode_enabled(&state)?;
    let validation = state
        .store
        .get_evidence_validation(&evidence_validation_id)
        .await?
        .ok_or_else(|| ApiError::not_found("evidence_validation", &evidence_validation_id))?;
    let typed_references = state
        .store
        .list_evidence_validation_references(&evidence_validation_id)
        .await?;
    Ok(Json(json!({
        "evidence_validation": validation,
        "typed_references": typed_references,
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
    if metadata.closed_at.is_some()
        && matches!(request.requested_effect.as_str(), "repeat_stage" | "replan")
    {
        return Err(ApiError::conflict(
            "closed Repo Mode WorkItems retain annotations as evidence but cannot repeat or replan",
        ));
    }
    let expected_hash = repo_work_item_state_hash(&metadata)?;
    if request.state_hash != expected_hash {
        return Err(ApiError::conflict(
            "Repo WorkItem changed after annotation preview; refresh and retry",
        ));
    }
    if request.target_kind == "work_item" && request.target_id != work_item_id {
        return Err(ApiError::not_found("work_item", &request.target_id));
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
        if request.requested_effect == "repeat_stage"
            && (!matches!(
                execution.stage_key.as_str(),
                "implement" | "test" | "verify"
            ) || !matches!(
                execution.status.as_str(),
                "succeeded" | "failed" | "blocked" | "cancelled"
            ))
        {
            return Err(ApiError::conflict(
                "repeat_stage requires a terminal Implement, Test, or Verify StageExecution",
            ));
        }
    } else if request.requested_effect == "repeat_stage" {
        return Err(ApiError::bad_request(
            "repeat_stage requires target_kind stage_execution",
        ));
    }
    if request.target_kind == "stage_outcome" {
        let outcome = state
            .store
            .get_stage_outcome(&request.target_id)
            .await?
            .ok_or_else(|| ApiError::not_found("stage_outcome", &request.target_id))?;
        if outcome.work_item_id != work_item_id {
            return Err(ApiError::not_found("stage_outcome", &request.target_id));
        }
    }
    let annotation = state
        .store
        .create_operator_annotation(CreateOperatorAnnotation {
            id: new_prefixed_id("annot"),
            work_item_id: work_item_id.clone(),
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
    let updated_metadata = repo_metadata(&state, &work_item_id).await?;
    Ok(Json(json!({
        "annotation": annotation,
        "work_item_state_hash":repo_work_item_state_hash(&updated_metadata)?,
    })))
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

#[cfg(test)]
mod tests {
    use super::*;
    use pharness_core::{RunId, SessionId};

    fn correction_environment_profile(
        image: &str,
        revision: &str,
    ) -> pharness_core::EnvironmentProfile {
        serde_json::from_value(json!({
            "id":"python-3.11",
            "active":true,
            "image":image,
            "revision":revision,
            "platform":"linux/amd64",
            "required_executables":["pharness-worker","git","python","pip"],
            "preparation_strategy":"python_hashed_requirements",
            "service_account":"pharness-python-runner",
            "repository_allowlist":["https://github.com/example/repo.git"],
            "limits":{"cpu":"1","memory":"1Gi","ephemeral_storage":"2Gi"},
        }))
        .unwrap()
    }

    fn correction_environment_snapshot(image: &str, revision: &str) -> Value {
        json!({
            "source_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "manifest_sha256":format!("sha256:{}", "b".repeat(64)),
            "dependency_lock_sha256":format!("sha256:{}", "c".repeat(64)),
            "runner_image_digest":image,
            "runner_revision":revision,
            "os":"linux",
            "architecture":"x86_64",
            "effective_user":"65532",
            "python_version":"Python 3.11.16",
            "python_path":"/workspace/.pharness-runtime/venv/bin/python",
            "writable_paths":["src/**"],
            "unavailable_tools":["docker"],
            "agent_network":"denied",
            "package_installation":"preparation_only",
            "acceptance_commands":[{"name":"unit","command":"python -m unittest"}],
            "preparation_evidence":{},
        })
    }

    fn metadata() -> StoredRepoWorkItemMetadata {
        StoredRepoWorkItemMetadata {
            work_item_id: "witem_repo".into(),
            mode: "repo".into(),
            product_id: "prod_repo".into(),
            repository_id: "repo_repo".into(),
            product_model_snapshot_id: "pmodel_repo".into(),
            product_model_snapshot_hash: "sha256:model".into(),
            repository_contract_version_id: "rcontract_repo".into(),
            contract_version: "pharness.dev/v1alpha1".into(),
            acceptance_command_names: vec!["unit".into()],
            context_repositories: json!([]),
            current_stage_execution_id: Some("stageexec_verify".into()),
            state_version: 8,
            closed_at: None,
            closure_reason: None,
        }
    }

    fn proposed_change_set() -> StoredChangeSet {
        StoredChangeSet {
            id: "cset_repo".into(),
            work_item_id: Some("witem_repo".into()),
            work_plan_id: "wplan_repo".into(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: SessionId::new("ses_repo"),
            run_id: Some(RunId::new("run_repo")),
            status: "proposed".into(),
            title: "Source change".into(),
            summary: "Verified change".into(),
            risk_level: "medium".into(),
            material_hash: format!("sha256:{}", "a".repeat(64)),
            revision: 1,
            resource_namespace: None,
            resource_kind: Some("Repository".into()),
            resource_name: Some("https://github.com/example/repo.git".into()),
            change_set_json: json!({}),
            created_at: "1".into(),
            updated_at: Some("1".into()),
            status_changed_at: Some("1".into()),
            status_changed_by: None,
            status_reason: None,
        }
    }

    fn proposed_work_plan(revision: i64) -> pharness_store::StoredWorkPlan {
        pharness_store::StoredWorkPlan {
            id: "wplan_repo".into(),
            work_item_id: Some("witem_repo".into()),
            remediation_plan_id: None,
            incident_id: None,
            session_id: SessionId::new("ses_repo"),
            run_id: Some(RunId::new("run_plan")),
            status: "proposed".into(),
            title: "Plan".into(),
            summary: "Correct the rejected source change".into(),
            risk_level: "medium".into(),
            requires_approval: true,
            resource_namespace: None,
            resource_kind: Some("Repository".into()),
            resource_name: Some("https://github.com/example/repo.git".into()),
            work_plan_json: json!({}),
            created_at: "1".into(),
            updated_at: Some("3".into()),
            revision,
            status_changed_at: Some("3".into()),
            status_changed_by: Some("controller".into()),
            status_reason: Some("new Planner submission".into()),
            created_by: Some("operator".into()),
            origin: "operator".into(),
        }
    }

    fn stage_execution(
        id: &str,
        stage_key: &str,
        status: &str,
        created_at: &str,
    ) -> pharness_store::StoredStageExecution {
        pharness_store::StoredStageExecution {
            id: id.into(),
            work_item_id: "witem_repo".into(),
            stage_key: stage_key.into(),
            sequence: 1,
            status: status.into(),
            origin: "controller".into(),
            agent_profile_id: None,
            agent_profile_version: None,
            agent_profile_hash: None,
            context_pack_id: None,
            run_id: None,
            workspace_id: Some("workspace_repo".into()),
            input_snapshot: json!({}),
            input_hash: format!("sha256:{}", "b".repeat(64)),
            stop_reason: None,
            created_at: created_at.into(),
            started_at: None,
            finished_at: None,
        }
    }

    fn stage_outcome(id: &str, stage_key: &str) -> StoredStageOutcome {
        StoredStageOutcome {
            id: id.into(),
            stage_execution_id: format!("stageexec_{id}"),
            work_item_id: "witem_repo".into(),
            stage_key: stage_key.into(),
            status: "succeeded".into(),
            origin: "agent".into(),
            schema_version: pharness_core::STAGE_OUTCOME_SCHEMA.into(),
            outcome: json!({}),
            content_hash: format!("sha256:{id}"),
            state_version: 1,
            supersedes_outcome_id: None,
            sealed_by: "controller".into(),
            sealed_at: "1".into(),
        }
    }

    fn outcome_reference(outcome: &StoredStageOutcome) -> Value {
        json!({
            "id":outcome.id,
            "stage":outcome.stage_key,
            "status":outcome.status,
            "hash":outcome.content_hash,
        })
    }

    fn source_delivery_intent(status: &str) -> StoredSourceDeliveryIntent {
        StoredSourceDeliveryIntent {
            id: "srcintent_repo".into(),
            subject_kind: "work_item_change_set".into(),
            subject_id: "cset_repo".into(),
            repository_id: "repo_repo".into(),
            source_repo: "https://github.com/example/repo.git".into(),
            base_ref: "main".into(),
            base_commit: "a".repeat(40),
            head_branch: "pharness/witem_repo".into(),
            patch_artifact_id: Some("artifact_repo".into()),
            patch_hash: format!("sha256:{}", "c".repeat(64)),
            status: status.into(),
            state_version: 4,
            authorization: json!({}),
            writer_execution_id: Some("writer_repo".into()),
            observer_execution_id: None,
            pull_request: Some(json!({"number":7,"head_sha":"d".repeat(40)})),
            merge_provenance: None,
            provider_checks: None,
            created_by: "operator".into(),
            creation_reason: "test".into(),
            created_at: "1".into(),
            updated_at: "1".into(),
            status_changed_at: "1".into(),
            status_changed_by: None,
            status_reason: None,
        }
    }

    #[test]
    fn stage_chain_profile_lookup_finds_every_compiled_profile() {
        let profiles = pharness_core::compiled_agent_profiles("test-model", "test-prompt")
            .into_iter()
            .filter(|profile| {
                matches!(
                    profile.id.as_str(),
                    "repo-builder" | "repo-tester" | "repo-verifier"
                )
            })
            .collect::<Vec<_>>();
        let profile_chain = serde_json::to_value(profiles).unwrap();

        for profile_id in ["repo-builder", "repo-tester", "repo-verifier"] {
            assert_eq!(
                agent_profile_from_chain(&profile_chain, profile_id).map(|profile| profile.id),
                Some(profile_id.to_string())
            );
        }
        assert!(agent_profile_from_chain(&profile_chain, "repo-unknown").is_none());
    }

    #[test]
    fn context_repository_projection_is_bounded_and_contains_no_raw_source() {
        let discovery = pharness_store::StoredRepositoryDiscovery {
            id: "rdisc_context".into(),
            onboarding_id: "ronb_context".into(),
            source_commit: "a".repeat(40),
            resolved_commit: Some("a".repeat(40)),
            status: "succeeded".into(),
            schema_version: "pharness.dev/repository-discovery/v1alpha1".into(),
            inventory_json: Some(json!({
                "command_candidates":(0..150).map(|index| json!({"name":format!("command-{index}")})).collect::<Vec<_>>(),
                "raw_source":"must-not-be-exposed",
                "limits":{"entries":20_000},
            })),
            content_hash: Some("sha256:discovery".into()),
            error_code: None,
            error_summary: None,
            started_at: Some("1".into()),
            finished_at: Some("2".into()),
            created_at: "1".into(),
            updated_at: "2".into(),
        };
        let projection = bounded_context_discovery_projection(
            &json!({
                "repository_id":"repo_context",
                "canonical_url":"https://github.com/example/context.git",
                "source_commit":"a".repeat(40),
            }),
            &discovery,
        );
        assert_eq!(
            projection
                .pointer("/bounded_inventory/command_candidates")
                .and_then(Value::as_array)
                .unwrap()
                .len(),
            100
        );
        assert!(projection
            .pointer("/bounded_inventory/raw_source")
            .is_none());
        assert_eq!(
            projection.pointer("/limits/raw_repository_content_included"),
            Some(&json!(false))
        );
    }

    #[test]
    fn proposed_change_set_replaces_stage_chain_reauthorization_actions() {
        let change_set = proposed_change_set();
        let actions = derive_repo_actions(
            &metadata(),
            RepoActionInputs {
                attempts: (0, 2),
                work_plan: None,
                change_set: Some(&change_set),
                source_delivery_intent: None,
                executions: &[],
                chain: None,
                pending_annotation_effects: &[],
                pending_budget_extension: None,
                current_run: None,
                retryable_budget_extension: None,
            },
        )
        .unwrap();
        assert_eq!(
            actions
                .iter()
                .map(|action| action.id.as_str())
                .collect::<Vec<_>>(),
            vec!["approve_change_set", "reject_change_set"]
        );
    }

    #[test]
    fn approved_change_set_repairs_stored_run_provenance_before_source_authorization() {
        let mut change_set = proposed_change_set();
        change_set.status = "approved".into();
        change_set.run_id = Some(RunId::new("run_builder_stale"));
        change_set.change_set_json = json!({
            "source_provenance":{"run_id":"run_builder_current"},
            "verification_run_id":"run_verifier_current",
        });

        let actions = derive_repo_actions(
            &metadata(),
            RepoActionInputs {
                attempts: (1, 3),
                work_plan: None,
                change_set: Some(&change_set),
                source_delivery_intent: None,
                executions: &[],
                chain: None,
                pending_annotation_effects: &[],
                pending_budget_extension: None,
                current_run: None,
                retryable_budget_extension: None,
            },
        )
        .unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, "repair_change_set_provenance");
        assert_eq!(actions[0].effect_class, "controller_internal");
        assert!(actions[0].approval_required);
        assert!(!actions[0]
            .external_effect_summary
            .contains("create a branch"));

        change_set.run_id = Some(RunId::new("run_builder_current"));
        let actions = derive_repo_actions(
            &metadata(),
            RepoActionInputs {
                attempts: (1, 3),
                work_plan: None,
                change_set: Some(&change_set),
                source_delivery_intent: None,
                executions: &[],
                chain: None,
                pending_annotation_effects: &[],
                pending_budget_extension: None,
                current_run: None,
                retryable_budget_extension: None,
            },
        )
        .unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, "authorize_source_delivery");
    }

    #[test]
    fn legacy_outcome_binding_allows_only_one_immutable_historical_verifier() {
        let implement = stage_outcome("implement_current", "implement");
        let test = stage_outcome("test_current", "test");
        let verify = stage_outcome("verify_current", "verify");
        let effective = vec![implement.clone(), test.clone(), verify.clone()];
        let current_material = effective.iter().map(outcome_reference).collect::<Vec<_>>();
        assert_eq!(
            validate_change_set_outcome_binding(&current_material, &effective).unwrap(),
            ChangeSetOutcomeBinding::Current
        );

        let historical_verify = stage_outcome("verify_historical", "verify");
        let historical_material = vec![
            outcome_reference(&implement),
            outcome_reference(&test),
            outcome_reference(&historical_verify),
        ];
        assert_eq!(
            validate_change_set_outcome_binding(&historical_material, &effective).unwrap(),
            ChangeSetOutcomeBinding::HistoricalVerifier {
                id: historical_verify.id.clone(),
                hash: historical_verify.content_hash.clone(),
            }
        );

        assert!(validate_change_set_outcome_binding(
            &[
                outcome_reference(&implement),
                outcome_reference(&historical_verify),
            ],
            &effective,
        )
        .is_err());
        assert!(validate_change_set_outcome_binding(
            &[
                outcome_reference(&implement),
                outcome_reference(&test),
                outcome_reference(&historical_verify),
                outcome_reference(&stage_outcome("verify_other", "verify")),
            ],
            &effective,
        )
        .is_err());
        let mut extra_stale_non_verify = historical_material;
        extra_stale_non_verify.push(outcome_reference(&stage_outcome(
            "implement_historical",
            "implement",
        )));
        assert!(validate_change_set_outcome_binding(&extra_stale_non_verify, &effective).is_err());
    }

    #[test]
    fn newer_proposed_work_plan_preempts_a_rejected_change_set_revision() {
        let plan = proposed_work_plan(2);
        let mut change_set = proposed_change_set();
        change_set.status = "rejected".into();
        change_set.change_set_json = json!({"work_plan":{"id":"wplan_repo","revision":1}});
        let executions = vec![stage_execution(
            "stageexec_plan_2",
            "plan",
            "succeeded",
            "3",
        )];

        assert!(rejected_change_set_precedes_work_plan(
            &change_set,
            &plan,
            None
        ));
        let actions = derive_repo_actions(
            &metadata(),
            RepoActionInputs {
                attempts: (1, 3),
                work_plan: Some(&plan),
                change_set: Some(&change_set),
                source_delivery_intent: None,
                executions: &executions,
                chain: None,
                pending_annotation_effects: &[],
                pending_budget_extension: None,
                current_run: None,
                retryable_budget_extension: None,
            },
        )
        .unwrap();

        assert_eq!(
            actions
                .iter()
                .map(|action| action.id.as_str())
                .collect::<Vec<_>>(),
            vec!["approve_work_plan", "reject_work_plan"]
        );
    }

    #[test]
    fn provider_check_status_is_controller_derived() {
        assert_eq!(derive_provider_check_status(&json!([])).unwrap(), "passing");
        assert_eq!(
            derive_provider_check_status(&json!([
                {"status":"passing"},
                {"status":"pending"}
            ]))
            .unwrap(),
            "pending"
        );
        assert_eq!(
            derive_provider_check_status(&json!([
                {"status":"passing"},
                {"status":"failed"}
            ]))
            .unwrap(),
            "failed"
        );
    }

    #[test]
    fn terminal_stage_failure_offers_bounded_same_workspace_correction_and_replan() {
        let executions = vec![
            stage_execution("stageexec_plan", "plan", "succeeded", "1"),
            stage_execution("stageexec_implement", "implement", "failed", "2"),
        ];
        let actions = derive_repo_actions(
            &metadata(),
            RepoActionInputs {
                attempts: (1, 2),
                work_plan: None,
                change_set: None,
                source_delivery_intent: None,
                executions: &executions,
                chain: None,
                pending_annotation_effects: &[],
                pending_budget_extension: None,
                current_run: None,
                retryable_budget_extension: None,
            },
        )
        .unwrap();
        assert_eq!(
            actions
                .iter()
                .map(|action| (action.id.as_str(), action.status.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("correct_stage_chain", "ready"),
                ("replan_work_item", "ready")
            ]
        );

        let exhausted = derive_repo_actions(
            &metadata(),
            RepoActionInputs {
                attempts: (2, 2),
                work_plan: None,
                change_set: None,
                source_delivery_intent: None,
                executions: &executions,
                chain: None,
                pending_annotation_effects: &[],
                pending_budget_extension: None,
                current_run: None,
                retryable_budget_extension: None,
            },
        )
        .unwrap();
        assert!(exhausted.iter().all(|action| action.status == "blocked"));
    }

    #[test]
    fn zero_turn_builder_startup_failure_preempts_attempt_exhaustion_for_recovery() {
        let run_id = RunId::new("run_startup_recovery");
        let mut execution =
            stage_execution("stageexec_startup_recovery", "implement", "preparing", "3");
        execution.run_id = Some(run_id.clone());
        let mut metadata = metadata();
        metadata.current_stage_execution_id = Some(execution.id.clone());
        let run = StoredRun {
            id: run_id,
            session_id: SessionId::new("ses_startup_recovery"),
            cwd: "/workspace".into(),
            status: "preparing".into(),
            user_task: "start the bounded Builder".into(),
            max_turns: 48,
            started_at: "3".into(),
            finished_at: None,
            cancel_requested_at: None,
            error: None,
            result_json: None,
            execution_target_json: json!({
                "kind":"kubernetes_workspace",
                "repo_mode":{
                    "stage":"implement",
                    "stage_execution_id":execution.id,
                },
            }),
            origin: "controller".into(),
            created_by: Some("operator".into()),
            run_budget: pharness_core::RunBudget::default(),
            budget_consumption: RunBudgetConsumption {
                allowed_turns: 48,
                allowed_tokens: 400_000,
                ..RunBudgetConsumption::default()
            },
            stop_reason: None,
            retention_state: "retained".into(),
            sealed_summary: None,
        };
        let actions = derive_repo_actions(
            &metadata,
            RepoActionInputs {
                attempts: (2, 2),
                work_plan: None,
                change_set: None,
                source_delivery_intent: None,
                executions: &[execution.clone()],
                chain: None,
                pending_annotation_effects: &[],
                pending_budget_extension: None,
                current_run: Some(&run),
                retryable_budget_extension: None,
            },
        )
        .unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, "recover_stage_startup");
        assert_eq!(actions[0].status, "ready");
        assert_eq!(actions[0].effect_class, "controller_internal");

        let mut consumed = run;
        consumed.budget_consumption.turns_used = 1;
        let actions = derive_repo_actions(
            &metadata,
            RepoActionInputs {
                attempts: (2, 2),
                work_plan: None,
                change_set: None,
                source_delivery_intent: None,
                executions: &[execution],
                chain: None,
                pending_annotation_effects: &[],
                pending_budget_extension: None,
                current_run: Some(&consumed),
                retryable_budget_extension: None,
            },
        )
        .unwrap();
        assert!(actions
            .iter()
            .all(|action| action.id != "recover_stage_startup"));
    }

    #[test]
    fn zero_turn_followup_startup_failure_retries_without_an_attempt_budget() {
        let run_id = RunId::new("run_tester_startup_recovery");
        let mut execution =
            stage_execution("stageexec_tester_startup_recovery", "test", "failed", "4");
        execution.run_id = Some(run_id.clone());
        execution.input_snapshot = json!({"chain_authorization_id":"chain_failed"});
        let mut metadata = metadata();
        metadata.current_stage_execution_id = Some(execution.id.clone());
        let run = StoredRun {
            id: run_id,
            session_id: SessionId::new("ses_tester_startup_recovery"),
            cwd: "/workspace".into(),
            status: "failed".into(),
            user_task: "run the bounded Tester".into(),
            max_turns: 8,
            started_at: "4".into(),
            finished_at: Some("5".into()),
            cancel_requested_at: None,
            error: Some("worker job failed before reporting a durable outcome".into()),
            result_json: Some(json!({
                "status":"failed",
                "turns":0,
                "error":"worker job failed before reporting a durable outcome",
            })),
            execution_target_json: json!({
                "kind":"kubernetes_workspace",
                "repo_mode":{
                    "stage":"test",
                    "stage_execution_id":execution.id,
                    "chain_authorization_id":"chain_failed",
                },
            }),
            origin: "controller".into(),
            created_by: Some("controller:repo-mode".into()),
            run_budget: pharness_core::RunBudget::default(),
            budget_consumption: RunBudgetConsumption {
                allowed_turns: 8,
                allowed_tokens: 80_000,
                ..RunBudgetConsumption::default()
            },
            stop_reason: Some("worker job failed before reporting a durable outcome".into()),
            retention_state: "retained".into(),
            sealed_summary: None,
        };
        let actions = derive_repo_actions(
            &metadata,
            RepoActionInputs {
                attempts: (3, 3),
                work_plan: None,
                change_set: None,
                source_delivery_intent: None,
                executions: &[execution.clone()],
                chain: None,
                pending_annotation_effects: &[],
                pending_budget_extension: None,
                current_run: Some(&run),
                retryable_budget_extension: None,
            },
        )
        .unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, "retry_stage_startup");
        assert_eq!(actions[0].status, "ready");
        assert_eq!(actions[0].effect_class, "model_execution");
        assert!(actions[0]
            .external_effect_summary
            .contains("does not consume another WorkItem attempt"));

        let mut consumed = run;
        consumed.budget_consumption.turns_used = 1;
        let actions = derive_repo_actions(
            &metadata,
            RepoActionInputs {
                attempts: (3, 3),
                work_plan: None,
                change_set: None,
                source_delivery_intent: None,
                executions: &[execution],
                chain: None,
                pending_annotation_effects: &[],
                pending_budget_extension: None,
                current_run: Some(&consumed),
                retryable_budget_extension: None,
            },
        )
        .unwrap();
        assert!(actions
            .iter()
            .all(|action| action.id != "retry_stage_startup"));
    }

    #[test]
    fn correction_reuses_an_exact_environment_snapshot() {
        let revision = "d".repeat(40);
        let image = format!("registry.example/runner@sha256:{}", "e".repeat(64));
        let profile = correction_environment_profile(&image, &revision);
        let snapshot = correction_environment_snapshot(&image, &revision);

        assert_eq!(
            reusable_correction_environment_snapshot(
                snapshot.clone(),
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                &format!("sha256:{}", "b".repeat(64)),
                &profile,
            )
            .unwrap(),
            Some(snapshot)
        );
    }

    #[test]
    fn correction_refreshes_runner_provenance_on_the_preserved_workspace() {
        let old_revision = "d".repeat(40);
        let current_revision = "e".repeat(40);
        let old_image = format!("registry.example/runner@sha256:{}", "f".repeat(64));
        let current_image = format!("registry.example/runner@sha256:{}", "1".repeat(64));
        let profile = correction_environment_profile(&current_image, &current_revision);
        let snapshot = correction_environment_snapshot(&old_image, &old_revision);

        assert!(reusable_correction_environment_snapshot(
            snapshot,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            &format!("sha256:{}", "b".repeat(64)),
            &profile,
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn correction_reprepares_when_prior_attempt_has_no_sealed_snapshot() {
        let revision = "d".repeat(40);
        let image = format!("registry.example/runner@sha256:{}", "e".repeat(64));
        let profile = correction_environment_profile(&image, &revision);

        assert!(correction_environment_snapshot_for_reuse(
            None,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            &format!("sha256:{}", "b".repeat(64)),
            &profile,
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn correction_never_refreshes_around_source_or_contract_drift() {
        let revision = "d".repeat(40);
        let image = format!("registry.example/runner@sha256:{}", "e".repeat(64));
        let profile = correction_environment_profile(&image, &revision);
        let snapshot = correction_environment_snapshot(&image, &revision);

        assert!(reusable_correction_environment_snapshot(
            snapshot.clone(),
            "ffffffffffffffffffffffffffffffffffffffff",
            &format!("sha256:{}", "b".repeat(64)),
            &profile,
        )
        .is_err());
        assert!(reusable_correction_environment_snapshot(
            snapshot,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            &format!("sha256:{}", "9".repeat(64)),
            &profile,
        )
        .is_err());
    }

    #[test]
    fn pending_budget_extension_preempts_stale_failed_attempt_actions() {
        let executions = vec![
            stage_execution("stageexec_plan", "plan", "succeeded", "1"),
            stage_execution("stageexec_prior", "implement", "failed", "2"),
            stage_execution("stageexec_current", "implement", "paused", "3"),
        ];
        let extension = StoredBudgetExtension {
            id: "budgetext_repo".into(),
            work_item_id: "witem_repo".into(),
            run_id: RunId::new("run_current"),
            status: "pending".into(),
            turn_increment: 20,
            token_increment: 200_000,
            state_hash: "sha256:budget-extension-state".into(),
            requested_at: "4".into(),
            approved_at: None,
            approved_by: None,
            approval_reason: None,
        };

        let actions = derive_repo_actions(
            &metadata(),
            RepoActionInputs {
                attempts: (2, 2),
                work_plan: None,
                change_set: None,
                source_delivery_intent: None,
                executions: &executions,
                chain: None,
                pending_annotation_effects: &[],
                pending_budget_extension: Some(&extension),
                current_run: None,
                retryable_budget_extension: None,
            },
        )
        .unwrap();

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, "approve_budget_extension");
        assert_eq!(actions[0].resource, extension.id);
        assert_eq!(actions[0].status, "ready");
        assert_eq!(actions[0].state_hash, extension.state_hash);
        assert!(actions[0]
            .external_effect_summary
            .contains("200000 additional tokens"));
    }

    #[test]
    fn pending_budget_extension_describes_only_remaining_hard_limit_headroom() {
        let executions = vec![stage_execution(
            "stageexec_current",
            "implement",
            "paused",
            "3",
        )];
        let run_id = RunId::new("run_current");
        let extension = StoredBudgetExtension {
            id: "budgetext_capped".into(),
            work_item_id: "witem_repo".into(),
            run_id: run_id.clone(),
            status: "pending".into(),
            turn_increment: 20,
            token_increment: 200_000,
            state_hash: "sha256:capped-extension-state".into(),
            requested_at: "4".into(),
            approved_at: None,
            approved_by: None,
            approval_reason: None,
        };
        let run = StoredRun {
            id: run_id,
            session_id: SessionId::new("session_current"),
            cwd: "/workspace".into(),
            status: "budget_extension_required".into(),
            user_task: "finish within the hard budget".into(),
            max_turns: 95,
            started_at: "1".into(),
            finished_at: None,
            cancel_requested_at: None,
            error: None,
            result_json: None,
            execution_target_json: json!({"kind":"kubernetes_workspace"}),
            origin: "controller".into(),
            created_by: Some("controller:repo-mode".into()),
            run_budget: pharness_core::RunBudget::default(),
            budget_consumption: RunBudgetConsumption {
                allowed_turns: 95,
                allowed_tokens: 900_000,
                ..RunBudgetConsumption::default()
            },
            stop_reason: Some("budget extension required".into()),
            retention_state: "retained".into(),
            sealed_summary: None,
        };

        let actions = derive_repo_actions(
            &metadata(),
            RepoActionInputs {
                attempts: (2, 2),
                work_plan: None,
                change_set: None,
                source_delivery_intent: None,
                executions: &executions,
                chain: None,
                pending_annotation_effects: &[],
                pending_budget_extension: Some(&extension),
                current_run: Some(&run),
                retryable_budget_extension: None,
            },
        )
        .unwrap();

        assert_eq!(actions.len(), 1);
        assert!(actions[0]
            .external_effect_summary
            .contains("exactly 5 additional turns and 100000 additional tokens"));
    }

    #[test]
    fn repo_actions_follow_the_current_stage_run_after_automatic_dispatch() {
        let fallback_run_id = RunId::new("run_builder");
        let mut builder = stage_execution("stageexec_builder", "implement", "succeeded", "1");
        builder.run_id = Some(fallback_run_id.clone());
        let mut verifier = stage_execution("stageexec_verify", "verify", "queued", "2");
        verifier.run_id = Some(RunId::new("run_verifier"));
        let executions = vec![builder, verifier];

        let selected = repo_action_run_id(&metadata(), &executions, Some(&fallback_run_id));

        assert_eq!(selected.map(RunId::as_str), Some("run_verifier"));
    }

    #[test]
    fn failed_approved_budget_extension_dispatch_offers_exact_same_run_retry() {
        let executions = vec![
            stage_execution("stageexec_prior", "implement", "failed", "1"),
            stage_execution("stageexec_current", "implement", "queued", "2"),
        ];
        let run = StoredRun {
            id: RunId::new("run_current"),
            session_id: SessionId::new("session_current"),
            cwd: "/workspace".into(),
            status: "failed".into(),
            user_task: "finish the approved builder stage".into(),
            max_turns: 68,
            started_at: "1".into(),
            finished_at: Some("3".into()),
            cancel_requested_at: None,
            error: Some(
                "failed to launch worker job: jobs.batch pharness-run-current-i already exists"
                    .into(),
            ),
            result_json: Some(json!({
                "status":"budget_extension_required",
                "budget_extension":{
                    "resume_messages":[],
                    "turns_completed":22,
                },
            })),
            execution_target_json: json!({"kind":"kubernetes_job"}),
            origin: "controller".into(),
            created_by: Some("operator".into()),
            run_budget: pharness_core::RunBudget::default(),
            budget_consumption: RunBudgetConsumption {
                allowed_turns: 68,
                allowed_tokens: 600_000,
                turns_used: 22,
                tokens_used: 420_894,
                active_execution_seconds_used: 159,
                extensions: 1,
            },
            stop_reason: None,
            retention_state: "retained".into(),
            sealed_summary: None,
        };
        let extension = StoredBudgetExtension {
            id: "budgetext_repo_approved".into(),
            work_item_id: "witem_repo".into(),
            run_id: run.id.clone(),
            status: "approved".into(),
            turn_increment: 20,
            token_increment: 200_000,
            state_hash: "sha256:approved-extension-state".into(),
            requested_at: "2".into(),
            approved_at: Some("3".into()),
            approved_by: Some("operator".into()),
            approval_reason: Some("finish evidence".into()),
        };

        let actions = derive_repo_actions(
            &metadata(),
            RepoActionInputs {
                attempts: (2, 2),
                work_plan: None,
                change_set: None,
                source_delivery_intent: None,
                executions: &executions,
                chain: None,
                pending_annotation_effects: &[],
                pending_budget_extension: None,
                current_run: Some(&run),
                retryable_budget_extension: Some(&extension),
            },
        )
        .unwrap();

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, "retry_budget_extension_dispatch");
        assert_eq!(actions[0].resource, extension.id);
        assert_eq!(actions[0].status, "ready");
        assert!(actions[0]
            .external_effect_summary
            .contains("grants no additional budget"));
    }

    #[test]
    fn annotation_effect_is_state_hashed_and_cannot_cross_source_delivery() {
        let annotation = StoredOperatorAnnotation {
            id: "annot_replan".into(),
            work_item_id: "witem_repo".into(),
            target_kind: "work_item".into(),
            target_id: "witem_repo".into(),
            statement: "The evidence requires a new plan".into(),
            evidence_refs: json!([]),
            requested_effect: "replan".into(),
            actor: "operator".into(),
            reason: "reviewed contradiction".into(),
            state_hash: "sha256:annotation-preview".into(),
            created_at: "1".into(),
        };
        let actions = derive_repo_actions(
            &metadata(),
            RepoActionInputs {
                attempts: (1, 2),
                work_plan: None,
                change_set: None,
                source_delivery_intent: None,
                executions: &[],
                chain: None,
                pending_annotation_effects: &[&annotation],
                pending_budget_extension: None,
                current_run: None,
                retryable_budget_extension: None,
            },
        )
        .unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, "apply_annotation_effect");
        assert_eq!(actions[0].status, "ready");

        let change_set = proposed_change_set();
        let blocked = derive_repo_actions(
            &metadata(),
            RepoActionInputs {
                attempts: (1, 2),
                work_plan: None,
                change_set: Some(&change_set),
                source_delivery_intent: None,
                executions: &[],
                chain: None,
                pending_annotation_effects: &[&annotation],
                pending_budget_extension: None,
                current_run: None,
                retryable_budget_extension: None,
            },
        )
        .unwrap();
        assert_eq!(blocked[0].id, "apply_annotation_effect");
        assert_eq!(blocked[0].status, "blocked");
    }

    #[test]
    fn source_head_drift_remains_observable_until_closed_then_offers_replan() {
        let mut change_set = proposed_change_set();
        change_set.status = "approved".into();
        let drifting = source_delivery_intent("head_drift");
        let actions = derive_repo_actions(
            &metadata(),
            RepoActionInputs {
                attempts: (1, 2),
                work_plan: None,
                change_set: Some(&change_set),
                source_delivery_intent: Some(&drifting),
                executions: &[],
                chain: None,
                pending_annotation_effects: &[],
                pending_budget_extension: None,
                current_run: None,
                retryable_budget_extension: None,
            },
        )
        .unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, "observe_source_delivery");

        let closed = source_delivery_intent("pull_request_closed");
        let actions = derive_repo_actions(
            &metadata(),
            RepoActionInputs {
                attempts: (1, 2),
                work_plan: None,
                change_set: Some(&change_set),
                source_delivery_intent: Some(&closed),
                executions: &[],
                chain: None,
                pending_annotation_effects: &[],
                pending_budget_extension: None,
                current_run: None,
                retryable_budget_extension: None,
            },
        )
        .unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, "replan_work_item");
        assert_eq!(actions[0].status, "ready");
    }

    #[test]
    fn failed_source_writer_permission_offers_only_an_exact_intent_retry() {
        let mut change_set = proposed_change_set();
        change_set.status = "approved".into();
        let mut failed = source_delivery_intent("failed");
        failed.pull_request = None;
        failed.status_reason = Some("git_push_permission_denied".into());

        let actions = derive_repo_actions(
            &metadata(),
            RepoActionInputs {
                attempts: (1, 2),
                work_plan: None,
                change_set: Some(&change_set),
                source_delivery_intent: Some(&failed),
                executions: &[],
                chain: None,
                pending_annotation_effects: &[],
                pending_budget_extension: None,
                current_run: None,
                retryable_budget_extension: None,
            },
        )
        .unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, "retry_source_delivery");
        assert_eq!(actions[0].effect_class, "external_source_mutation");
        assert!(actions[0]
            .external_effect_summary
            .contains("same immutable SourceDeliveryIntent"));

        failed.status_reason = Some("git_push_policy_rejected".into());
        let actions = derive_repo_actions(
            &metadata(),
            RepoActionInputs {
                attempts: (1, 2),
                work_plan: None,
                change_set: Some(&change_set),
                source_delivery_intent: Some(&failed),
                executions: &[],
                chain: None,
                pending_annotation_effects: &[],
                pending_budget_extension: None,
                current_run: None,
                retryable_budget_extension: None,
            },
        )
        .unwrap();
        assert!(actions.is_empty());
    }
}
