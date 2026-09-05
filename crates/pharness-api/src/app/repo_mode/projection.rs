use super::state::{repo_metadata, repo_work_item_state_hash};
use crate::app::hashing::canonical_material_hash;
use crate::app::repository_readiness::ensure_repo_mode_enabled;
use crate::app::{ApiError, AppState};
use crate::dto::{
    DeliverySegmentResourceResponse, DeliverySegmentResponse, ReconcileWorkItemResponse,
    WorkItemActionResponse, WorkItemFlowResponse,
};
use pharness_core::RunId;
use pharness_store::{
    ChangeSetListFilter, RunListFilter, StoredBudgetExtension, StoredChangeSet,
    StoredOperatorAnnotation, StoredOperatorAnnotationDecision, StoredRepoWorkItemMetadata,
    StoredRun, StoredSourceDeliveryIntent, StoredStageOutcome, WorkPlanListFilter,
    WorkspaceListFilter,
};
use serde_json::{json, Value};

pub(in crate::app) async fn repo_work_item_flow(
    state: &AppState,
    work_item_id: &str,
) -> Result<WorkItemFlowResponse, ApiError> {
    build_repo_work_item_flow(state, work_item_id, true).await
}

// The controller uses the same eligibility calculation as historical manual
// actions, while the operator projection exposes only workflow controls.
pub(in crate::app) async fn repo_controller_actions(
    state: &AppState,
    work_item_id: &str,
) -> Result<Vec<WorkItemActionResponse>, ApiError> {
    Ok(build_repo_work_item_flow(state, work_item_id, false)
        .await?
        .action_rail)
}

async fn build_repo_work_item_flow(
    state: &AppState,
    work_item_id: &str,
    show_workflow_controls: bool,
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
                "binding":crate::app::inference::sanitized_binding(&selection.resolved_binding),
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
                    crate::app::agent_hosts::sanitized_run_agent_execution(state, run_id).await?
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
    let mut action_rail = derive_repo_actions(
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
    let workflow_control = state
        .store
        .get_workflow_reconciliation(work_item_id)
        .await?;
    if let Some(control) = workflow_control.as_ref().filter(|_| show_workflow_controls) {
        action_rail =
            crate::app::hosted_controller::control_actions(control, metadata.closed_at.is_some())?;
    }
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
    let mut reconcile_preview = ReconcileWorkItemResponse {
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
    if let Some(control) = workflow_control.as_ref().filter(|_| show_workflow_controls) {
        reconcile_preview.action = "controller_wait".into();
        reconcile_preview.message = control.condition_reason.clone();
        reconcile_preview.boundary = control.condition.clone();
        reconcile_preview.can_apply = false;
        reconcile_preview.effect_summary =
            "Authorized progression is owned by the hosted controller".into();
    }
    Ok(WorkItemFlowResponse {
        work_item: work_item_response,
        reconcile_preview,
        sdlc_flow: None,
        delivery_segments: repo_delivery_segments(&executions, &outcomes),
        workspaces: workspaces.clone(),
        controller_waits: Vec::new(),
        audit_events: audit_events.clone(),
        action_rail,
        delivery_configuration: crate::app::hosted_workflow::projection::delivery_configuration(
            &metadata,
            work_item.source_commit.as_deref(),
        ),
        repo_mode: Some(json!({
            "metadata":metadata,
            "workflow_control":workflow_control.as_ref().map(crate::app::hosted_controller::public_state),
            "state_hash":repo_work_item_state_hash(&metadata)?,
            "ownership":{
                "product":product,
                "repository":repository,
                "repository_binding":binding,
                "repository_binding_revision":binding_revision,
                "services":services,
            },
            "stage_executions":execution_views,
            "lifecycle_timeline":crate::app::lifecycle_timeline::project(
                &executions, &all_outcomes, &outcomes, source_delivery_intent.as_ref(),
                metadata.current_stage_execution_id.as_deref(), metadata.closed_at.as_deref(),
                crate::app::clock::current_millis().to_string(),
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

pub(super) fn pending_annotation_effects<'a>(
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

pub(super) struct RepoActionInputs<'a> {
    pub(super) attempts: (u32, u32),
    pub(super) work_plan: Option<&'a pharness_store::StoredWorkPlan>,
    pub(super) change_set: Option<&'a StoredChangeSet>,
    pub(super) source_delivery_intent: Option<&'a StoredSourceDeliveryIntent>,
    pub(super) executions: &'a [pharness_store::StoredStageExecution],
    pub(super) chain: Option<&'a pharness_store::StoredStageChainAuthorization>,
    pub(super) pending_annotation_effects: &'a [&'a StoredOperatorAnnotation],
    pub(super) pending_budget_extension: Option<&'a StoredBudgetExtension>,
    pub(super) current_run: Option<&'a StoredRun>,
    pub(super) retryable_budget_extension: Option<&'a StoredBudgetExtension>,
}

pub(super) fn repo_action_run_id<'a>(
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

pub(super) fn rejected_change_set_precedes_work_plan(
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

pub(super) fn source_writer_failure_is_retryable(intent: &StoredSourceDeliveryIntent) -> bool {
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
pub(super) struct ChangeSetProvenanceRepair<'a> {
    pub(super) material_builder_run_id: &'a str,
    pub(super) verification_run_id: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
pub(in crate::app) enum ChangeSetOutcomeBinding {
    Current,
    HistoricalVerifier { id: String, hash: String },
}

pub(in crate::app) fn validate_change_set_outcome_binding(
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

pub(super) fn change_set_provenance_repair(
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

pub(super) fn derive_repo_actions(
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

pub(super) fn recoverable_repo_stage_startup<'a>(
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

pub(super) fn recoverable_repo_followup_stage_startup<'a>(
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

pub(super) struct AgentEvidenceBundle {
    pub(super) catalog: Vec<Value>,
    pub(super) payloads: Vec<Value>,
}

pub(super) async fn agent_evidence_bundle(
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

pub(super) fn bounded_context_discovery_projection(
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

pub(super) fn annotation_context(
    annotations: &[pharness_store::StoredOperatorAnnotation],
) -> Vec<Value> {
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

pub(super) fn annotation_contradictions(
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
