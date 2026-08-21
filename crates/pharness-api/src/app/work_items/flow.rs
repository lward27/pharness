use super::super::approval_policy::approval_gate_uses_dedicated_lifecycle_action;
use super::super::approvals::{approval_gate_lifecycle_readiness, approval_gate_lifecycle_stage};
use super::super::delivery_segments::work_item_delivery_segments;
use super::super::gitops::observation::gitops_observation_closed_unmerged;
use super::super::identifiers::is_git_sha;
use super::super::pipeline::state::{pipeline_execution_attempt, MAX_PIPELINE_EXECUTION_ATTEMPTS};
use super::super::sdlc::build_sdlc_flow;
use super::super::system::{
    PROTECTED_ARGO_APPLICATION, PROTECTED_GITOPS_REPO, PROTECTED_ROLLBACK_OWNER,
    PROTECTED_WORKLOAD_NAME,
};
use super::super::validation::clean_optional_text;
use super::super::{ApiError, AppState};
use super::lifecycle::work_item_gate_scope_matches;
use super::reconcile::reconcile_work_item;
use super::reconcile_model::WorkItemReconcileAction;
use super::rollback_state::latest_rollback_intent;
use crate::dto::{
    ReconcileBlockerResponse, ReconcileWorkItemRequest, ReconcileWorkItemResponse,
    WorkItemActionResponse, WorkItemFlowResponse, WorkItemOperatorStateResponse, WorkItemResponse,
    WorkItemsResponse, WorkspaceResponse,
};
use axum::extract::{Path, Query, State};
use axum::Json;
use pharness_store::{
    ApprovalGateListFilter, ControllerWaitListFilter, WorkItemListFilter, WorkspaceListFilter,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Default, serde::Deserialize)]
pub(in crate::app) struct ListWorkItemsQuery {
    pub(in crate::app) status: Option<String>,
    pub(in crate::app) source_repo: Option<String>,
    pub(in crate::app) target_environment: Option<String>,
    pub(in crate::app) target_namespace: Option<String>,
    pub(in crate::app) production_impacting: Option<bool>,
    pub(in crate::app) actor: Option<String>,
    pub(in crate::app) origin: Option<String>,
    pub(in crate::app) include: Option<String>,
    pub(in crate::app) limit: Option<u32>,
    pub(in crate::app) offset: Option<u32>,
}

pub(in crate::app) async fn list_work_items(
    State(state): State<AppState>,
    Query(query): Query<ListWorkItemsQuery>,
) -> Result<Json<WorkItemsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let filter = WorkItemListFilter {
        status: clean_optional_text(query.status),
        source_repo: clean_optional_text(query.source_repo),
        target_environment: clean_optional_text(query.target_environment),
        target_namespace: clean_optional_text(query.target_namespace),
        production_impacting: query.production_impacting,
        created_by: clean_optional_text(query.actor),
        origin: clean_optional_text(query.origin),
        limit,
        offset,
    };
    let count = state.store.count_work_items(filter.clone()).await?;
    let stored_work_items = state.store.list_work_items(filter).await?;
    let include_operator_state = clean_optional_text(query.include).is_some_and(|include| {
        include
            .split(',')
            .any(|value| value.trim() == "operator_state")
    });
    let mut operator_state = BTreeMap::new();
    if include_operator_state {
        for item in &stored_work_items {
            let active_wait = state
                .store
                .get_active_controller_wait_for_work_item(&item.id)
                .await?;
            operator_state.insert(
                item.id.clone(),
                WorkItemOperatorStateResponse {
                    current_boundary: active_wait
                        .as_ref()
                        .map(|wait| format!("waiting for {}", wait.wait_kind))
                        .unwrap_or_else(|| item.status.clone()),
                    attempts_remaining: item.max_attempts.saturating_sub(item.attempt_count),
                    attention_reason: item.status_reason.clone(),
                    active_wait: active_wait.map(Into::into),
                },
            );
        }
    }
    let work_items = stored_work_items
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    Ok(Json(WorkItemsResponse {
        work_items,
        count,
        limit,
        offset,
        operator_state: include_operator_state.then_some(operator_state),
    }))
}

pub(in crate::app) async fn get_work_item(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
) -> Result<Json<WorkItemResponse>, ApiError> {
    let work_item = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    Ok(Json(work_item.into()))
}

pub(in crate::app) async fn work_item_flow(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
) -> Result<Json<WorkItemFlowResponse>, ApiError> {
    let work_item = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&work_item_id)
        .await?;
    let change_set = match work_plan.as_ref() {
        Some(plan) => state.store.get_change_set_by_work_plan(&plan.id).await?,
        None => None,
    };
    let sdlc_flow = match work_plan.clone() {
        Some(plan) => Some(
            build_sdlc_flow(
                &state.store,
                "work_item",
                &work_item_id,
                plan,
                change_set.clone(),
            )
            .await?,
        ),
        None => None,
    };
    let Json(reconcile_preview) = reconcile_work_item(
        State(state.clone()),
        None,
        Path(work_item_id.clone()),
        Json(ReconcileWorkItemRequest {
            apply: false,
            actor: None,
            reason: None,
            max_turns: None,
        }),
    )
    .await?;
    let workspaces: Vec<WorkspaceResponse> = state
        .store
        .list_workspaces(WorkspaceListFilter {
            work_item_id: Some(work_item_id.clone()),
            limit: 100,
            ..WorkspaceListFilter::default()
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    let controller_waits = state
        .store
        .list_controller_waits(ControllerWaitListFilter {
            work_item_id: Some(work_item_id.clone()),
            limit: 100,
            ..ControllerWaitListFilter::default()
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    let delivery_segments = work_item_delivery_segments(&sdlc_flow, workspaces.last());
    let audit_events = state
        .store
        .list_audit_events(Some("work_item"), Some(&work_item_id), None, 100)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    let mut action_rail = Vec::new();
    if let Some(plan) = work_plan.as_ref() {
        if plan.status == "proposed" {
            action_rail.push(resource_review_action(
                "planning",
                "work_plan",
                &plan.id,
                &plan.status,
                plan.updated_at.as_deref(),
                plan.revision,
                true,
            ));
            action_rail.push(resource_review_action(
                "planning",
                "work_plan",
                &plan.id,
                &plan.status,
                plan.updated_at.as_deref(),
                plan.revision,
                false,
            ));
        } else if plan.status == "approved"
            && work_item.status == "awaiting_approval"
            && reconcile_preview.action == WorkItemReconcileAction::StartCodingAttempt.as_str()
        {
            action_rail.push(WorkItemActionResponse {
                id: "authorize_workspace_and_start".to_string(),
                lifecycle_stage: "attempt".to_string(),
                resource: plan.id.clone(),
                status: "ready".to_string(),
                effect_class: "model_execution".to_string(),
                blockers: Vec::new(),
                approval_required: true,
                approval_requirements: vec!["attempt_workspace_write".to_string()],
                external_effect_summary: format!(
                    "Authorize one coding attempt for repository {} using only the declared writable paths, then start model execution.",
                    work_item.source_repo
                ),
                state_hash: lifecycle_action_hash(json!({
                    "action": "authorize_workspace_and_start",
                    "work_item": work_item.id,
                    "work_item_updated_at": work_item.updated_at,
                    "work_plan": plan.id,
                    "work_plan_revision": plan.revision,
                    "attempt_count": work_item.attempt_count,
                })),
            });
        }
    }
    if let Some(flow) = sdlc_flow.as_ref() {
        if let Some(change_set) = flow
            .change_set
            .as_ref()
            .filter(|item| item.status == "proposed")
        {
            action_rail.extend(resource_review_actions(
                "change_set",
                "source",
                &change_set.id,
                &change_set.status,
                change_set.updated_at.as_deref(),
                change_set.revision,
            ));
        }
        if let Some(intent) = flow
            .pipeline_intent
            .as_ref()
            .filter(|item| item.status == "proposed")
        {
            action_rail.extend(resource_review_actions(
                "pipeline_intent",
                "pipeline",
                &intent.id,
                &intent.status,
                intent.updated_at.as_deref(),
                0,
            ));
        }
        if let Some(intent) = flow
            .pipeline_intent
            .as_ref()
            .filter(|item| item.status == "failed")
        {
            let execution_attempt = pipeline_execution_attempt(&intent.intent_json)?;
            let failed_execution_id = intent
                .intent_json
                .pointer("/execution_state/execution_id")
                .and_then(Value::as_str);
            let failed_pipeline_run = intent
                .intent_json
                .pointer("/execution_state/pipeline_run_name")
                .and_then(Value::as_str);
            let failure_artifact_id = intent
                .intent_json
                .pointer("/execution_evidence/artifact_id")
                .and_then(Value::as_str);
            let source_merge_sha = intent
                .intent_json
                .pointer("/source_provenance/merge_commit_sha")
                .and_then(Value::as_str);
            let mut blockers = Vec::new();
            if execution_attempt >= MAX_PIPELINE_EXECUTION_ATTEMPTS {
                blockers.push(ReconcileBlockerResponse {
                    code: "pipeline_retry_budget_exhausted".to_string(),
                    summary: format!(
                        "PipelineIntent has used all {MAX_PIPELINE_EXECUTION_ATTEMPTS} supervised execution attempts."
                    ),
                });
            }
            if intent
                .intent_json
                .pointer("/execution_evidence/status")
                .and_then(Value::as_str)
                != Some("failed")
                || failed_execution_id.is_none()
                || failed_pipeline_run.is_none()
                || failure_artifact_id.is_none()
            {
                blockers.push(ReconcileBlockerResponse {
                    code: "pipeline_failure_evidence_missing".to_string(),
                    summary: "A supervised retry requires durable failed execution evidence and the exact failed PipelineRun identity."
                        .to_string(),
                });
            }
            if source_merge_sha.map_or(true, |sha| !is_git_sha(sha)) {
                blockers.push(ReconcileBlockerResponse {
                    code: "pipeline_retry_source_provenance_missing".to_string(),
                    summary: "A supervised retry requires the original immutable source merge SHA."
                        .to_string(),
                });
            }
            if flow
                .change_set
                .as_ref()
                .map_or(true, |change_set| change_set.status != "approved")
            {
                blockers.push(ReconcileBlockerResponse {
                    code: "pipeline_retry_change_set_not_approved".to_string(),
                    summary: "The source ChangeSet must remain approved before a PipelineIntent can be retried."
                        .to_string(),
                });
            }
            if flow.deployment_intent.is_some() || flow.gitops_change_set.is_some() {
                blockers.push(ReconcileBlockerResponse {
                    code: "pipeline_retry_downstream_delivery_started".to_string(),
                    summary: "Pipeline retry is disabled after DeploymentIntent or GitOps delivery records exist."
                        .to_string(),
                });
            }
            let mut approval_requirements = vec![
                "pipeline_retry".to_string(),
                "pipeline_mutation".to_string(),
            ];
            if work_item.production_impacting {
                approval_requirements.push("production_impact".to_string());
            }
            action_rail.push(WorkItemActionResponse {
                id: "retry_pipeline_intent".to_string(),
                lifecycle_stage: "pipeline".to_string(),
                resource: intent.id.clone(),
                status: if blockers.is_empty() { "ready" } else { "blocked" }.to_string(),
                effect_class: "approval_boundary".to_string(),
                blockers,
                approval_required: true,
                approval_requirements,
                external_effect_summary: format!(
                    "After review, repropose PipelineIntent {} for supervised execution attempt {} of {}/{} at source {}. Failed PipelineRun {} and artifact {} remain durable. This action does not start Tekton; PipelineIntent approval, a fresh grant, and explicit execution remain separate boundaries.",
                    intent.id,
                    execution_attempt + 1,
                    intent.intent_json.pointer("/execution/namespace").and_then(Value::as_str).unwrap_or("unknown"),
                    intent.intent_json.pointer("/execution/pipeline_ref").and_then(Value::as_str).unwrap_or("unknown"),
                    source_merge_sha.unwrap_or("unknown"),
                    failed_pipeline_run.unwrap_or("unknown"),
                    failure_artifact_id.unwrap_or("unknown"),
                ),
                state_hash: lifecycle_action_hash(json!({
                    "action": "retry_pipeline_intent",
                    "pipeline_intent": intent.id,
                    "status": intent.status,
                    "updated_at": intent.updated_at,
                    "execution_attempt": execution_attempt,
                    "failed_execution_id": failed_execution_id,
                    "failed_pipeline_run": failed_pipeline_run,
                    "failure_artifact_id": failure_artifact_id,
                    "source_merge_sha": source_merge_sha,
                })),
            });
        }
        if let Some(intent) = flow.pipeline_intent.as_ref().filter(|item| {
            item.status == "approved"
                && reconcile_preview.action
                    == WorkItemReconcileAction::AwaitingPipelineExecutionAuthorization.as_str()
        }) {
            let mut blockers = Vec::new();
            let preflight_checks = reconcile_preview
                .pipeline_execution_preflight
                .as_ref()
                .map(|preflight| preflight.checks.clone())
                .unwrap_or_default();
            if reconcile_preview.pipeline_execution_preflight.is_none() {
                blockers.push(ReconcileBlockerResponse {
                    code: "pipeline_execution_preflight_unavailable".to_string(),
                    summary: "Pipeline execution preflight is unavailable; authorization cannot be scoped safely."
                        .to_string(),
                });
            }
            blockers.extend(preflight_checks.iter().filter_map(|check| {
                let code = check.get("code").and_then(Value::as_str)?;
                let passed = check.get("passed").and_then(Value::as_bool) == Some(true);
                if passed || code == "trusted_execution_envelope" {
                    return None;
                }
                Some(ReconcileBlockerResponse {
                    code: code.to_string(),
                    summary: check
                        .get("summary")
                        .and_then(Value::as_str)
                        .unwrap_or("Pipeline execution preflight failed")
                        .to_string(),
                })
            }));
            let namespace = intent
                .intent_json
                .pointer("/execution/namespace")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let pipeline_ref = intent
                .intent_json
                .pointer("/execution/pipeline_ref")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let source_merge_sha = intent
                .intent_json
                .pointer("/source_provenance/merge_commit_sha")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let execution_attempt = pipeline_execution_attempt(&intent.intent_json)?;
            let mut approval_requirements = vec![
                "pipeline_execution_authorization".to_string(),
                "pipeline_mutation".to_string(),
            ];
            if work_item.production_impacting {
                approval_requirements.push("production_impact".to_string());
            }
            action_rail.push(WorkItemActionResponse {
                id: "authorize_pipeline_execution".to_string(),
                lifecycle_stage: "pipeline".to_string(),
                resource: intent.id.clone(),
                status: if blockers.is_empty() { "ready" } else { "blocked" }.to_string(),
                effect_class: "approval_boundary".to_string(),
                blockers,
                approval_required: true,
                approval_requirements,
                external_effect_summary: format!(
                    "Authorize one supervised Tekton execution attempt {execution_attempt} for exact PipelineIntent {} using {namespace}/{pipeline_ref} at immutable source {source_merge_sha}. The grant expires within 30 minutes for production. This action does not start Tekton.",
                    intent.id,
                ),
                state_hash: lifecycle_action_hash(json!({
                    "action": "authorize_pipeline_execution",
                    "work_item": work_item.id,
                    "work_item_updated_at": work_item.updated_at,
                    "pipeline_intent": intent.id,
                    "status": intent.status,
                    "updated_at": intent.updated_at,
                    "execution_attempt": execution_attempt,
                    "namespace": namespace,
                    "pipeline_ref": pipeline_ref,
                    "source_merge_sha": source_merge_sha,
                    "preflight_checks": preflight_checks,
                    "permission_grant_id": reconcile_preview.pipeline_execution_preflight.as_ref().and_then(|preflight| preflight.permission_grant_id.as_deref()),
                })),
            });
        }
        if let Some(intent) = flow
            .deployment_intent
            .as_ref()
            .filter(|item| item.status == "proposed")
        {
            action_rail.extend(resource_review_actions(
                "deployment_intent",
                "deployment",
                &intent.id,
                &intent.status,
                intent.updated_at.as_deref(),
                0,
            ));
        }
        if let Some(change_set) = flow
            .gitops_change_set
            .as_ref()
            .filter(|item| item.status == "proposed")
        {
            action_rail.extend(resource_review_actions(
                "gitops_change_set",
                "gitops",
                &change_set.id,
                &change_set.status,
                change_set.updated_at.as_deref(),
                change_set.revision,
            ));
        }
        if let Some(change_set) = flow.gitops_change_set.as_ref().filter(|item| {
            item.status == "approved"
                && reconcile_preview.action
                    == WorkItemReconcileAction::AwaitingGitOpsDeliveryAuthorization.as_str()
        }) {
            let gitops_gate = state
                .store
                .list_approval_gates(ApprovalGateListFilter {
                    work_item_id: Some(work_item_id.clone()),
                    gate_kind: Some("gitops_mutation".to_string()),
                    limit: 20,
                    ..ApprovalGateListFilter::default()
                })
                .await?
                .into_iter()
                .find(|gate| {
                    work_plan.as_ref().is_some_and(|plan| {
                        work_item_gate_scope_matches(gate, &work_item, plan, "gitops_mutation")
                    })
                });
            let gate_ready = gitops_gate
                .as_ref()
                .is_some_and(|gate| matches!(gate.status.as_str(), "satisfied" | "waived"));
            let writer_ready = state
                .worker
                .gitops_writer_settings()
                .is_some_and(|settings| {
                    settings
                        .allowed_repos
                        .iter()
                        .any(|repo| repo == &change_set.gitops_repo)
                });
            let plan_id = flow
                .gitops_delivery
                .as_ref()
                .map(|delivery| delivery.plan.id.as_str());
            let mut blockers = Vec::new();
            if !gate_ready {
                blockers.push(ReconcileBlockerResponse {
                    code: "gitops_mutation_gate_pending".to_string(),
                    summary: "Satisfy the exact WorkItem gitops_mutation gate before authorizing the dedicated GitOps writer."
                        .to_string(),
                });
            }
            if plan_id.is_none() {
                blockers.push(ReconcileBlockerResponse {
                    code: "gitops_delivery_plan_missing".to_string(),
                    summary:
                        "The immutable base-revision-bound GitOps delivery plan is unavailable."
                            .to_string(),
                });
            }
            if !writer_ready {
                blockers.push(ReconcileBlockerResponse {
                    code: "gitops_writer_unavailable".to_string(),
                    summary: format!(
                        "The dedicated GitOps writer is not configured for {}.",
                        change_set.gitops_repo
                    ),
                });
            }
            let mut approval_requirements = vec![
                "gitops_delivery_authorization".to_string(),
                "gitops_mutation".to_string(),
            ];
            if work_item.production_impacting {
                approval_requirements.push("production_impact".to_string());
            }
            action_rail.push(WorkItemActionResponse {
                id: "authorize_gitops_delivery".to_string(),
                lifecycle_stage: "gitops".to_string(),
                resource: change_set.id.clone(),
                status: if blockers.is_empty() { "ready" } else { "blocked" }.to_string(),
                effect_class: "approval_boundary".to_string(),
                blockers,
                approval_required: true,
                approval_requirements,
                external_effect_summary: format!(
                    "Authorize one isolated GitOps branch-and-pull-request writer for {} at immutable plan {}. The production grant expires within 30 minutes. This action does not create the branch or pull request.",
                    change_set.gitops_repo,
                    plan_id.unwrap_or("unavailable"),
                ),
                state_hash: lifecycle_action_hash(json!({
                    "action": "authorize_gitops_delivery",
                    "work_item": work_item.id,
                    "work_item_updated_at": work_item.updated_at,
                    "gitops_change_set": change_set.id,
                    "status": change_set.status,
                    "updated_at": change_set.updated_at,
                    "revision": change_set.revision,
                    "material_hash": change_set.material_hash,
                    "repository": change_set.gitops_repo,
                    "head_branch": change_set.head_branch,
                    "plan_id": plan_id,
                    "gate_id": gitops_gate.as_ref().map(|gate| gate.id.as_str()),
                    "gate_status": gitops_gate.as_ref().map(|gate| gate.status.as_str()),
                    "writer_ready": writer_ready,
                })),
            });
        }
        if let Some(intent) = flow.deployment_intent.as_ref().filter(|item| {
            item.status == "approved"
                && reconcile_preview.action
                    == WorkItemReconcileAction::AwaitingDeploymentAuthorization.as_str()
        }) {
            let preflight = reconcile_preview.deployment_execution_preflight.as_ref();
            let preflight_checks = preflight
                .map(|preflight| preflight.checks.clone())
                .unwrap_or_default();
            let mut blockers = Vec::new();
            if preflight.is_none() {
                blockers.push(ReconcileBlockerResponse {
                    code: "deployment_execution_preflight_unavailable".to_string(),
                    summary: "Deployment execution preflight is unavailable; the production window cannot be bound safely."
                        .to_string(),
                });
            }
            blockers.extend(preflight_checks.iter().filter_map(|check| {
                let code = check.get("code").and_then(Value::as_str)?;
                let passed = check.get("passed").and_then(Value::as_bool) == Some(true);
                if passed || code == "trusted_execution_envelope" {
                    return None;
                }
                Some(ReconcileBlockerResponse {
                    code: code.to_string(),
                    summary: check
                        .get("summary")
                        .and_then(Value::as_str)
                        .unwrap_or("Deployment execution preflight failed")
                        .to_string(),
                })
            }));
            let mut approval_requirements = vec![
                "deployment_execution_authorization".to_string(),
                "cluster_mutation".to_string(),
            ];
            if work_item.production_impacting {
                approval_requirements.extend([
                    "production_impact".to_string(),
                    "production_deployment".to_string(),
                ]);
            }
            action_rail.push(WorkItemActionResponse {
                id: "authorize_deployment_execution".to_string(),
                lifecycle_stage: "deployment".to_string(),
                resource: intent.id.clone(),
                status: if blockers.is_empty() { "ready" } else { "blocked" }.to_string(),
                effect_class: "approval_boundary".to_string(),
                blockers,
                approval_required: true,
                approval_requirements,
                external_effect_summary: format!(
                    "Open one production authorization window, lasting at most 30 minutes, for exact Argo Application {} in {}/{}. This action does not dispatch an Argo sync.",
                    intent.argo_application.as_deref().unwrap_or("unavailable"),
                    intent.target_environment.as_deref().unwrap_or("unavailable"),
                    intent.target_namespace.as_deref().unwrap_or("unavailable"),
                ),
                state_hash: lifecycle_action_hash(json!({
                    "action": "authorize_deployment_execution",
                    "work_item": work_item.id,
                    "work_item_updated_at": work_item.updated_at,
                    "deployment_intent": intent.id,
                    "status": intent.status,
                    "updated_at": intent.updated_at,
                    "target_environment": intent.target_environment,
                    "target_namespace": intent.target_namespace,
                    "argo_application": intent.argo_application,
                    "preflight_checks": preflight_checks,
                    "permission_grant_id": preflight.and_then(|preflight| preflight.permission_grant.as_ref()).map(|grant| grant.id.as_str()),
                })),
            });
        }
        if let Some(release) = flow
            .release
            .as_ref()
            .filter(|item| item.status == "proposed")
        {
            action_rail.extend(resource_review_actions(
                "release",
                "release",
                &release.id,
                &release.status,
                release.updated_at.as_deref(),
                0,
            ));
        }
    }
    let lifecycle_gates = state
        .store
        .list_approval_gates(ApprovalGateListFilter {
            work_item_id: Some(work_item_id.clone()),
            limit: 200,
            ..ApprovalGateListFilter::default()
        })
        .await?;
    for gate in lifecycle_gates.iter().filter(|gate| {
        gate.status == "pending" && !approval_gate_uses_dedicated_lifecycle_action(&gate.gate_kind)
    }) {
        let (eligible, boundary_summary) = approval_gate_lifecycle_readiness(&state, gate).await?;
        action_rail.push(WorkItemActionResponse {
            id: format!("satisfy_approval_gate:{}", gate.id),
            lifecycle_stage: approval_gate_lifecycle_stage(&gate.gate_kind).to_string(),
            resource: gate.id.clone(),
            status: if eligible { "ready" } else { "blocked" }.to_string(),
            effect_class: "approval_boundary".to_string(),
            blockers: (!eligible)
                .then(|| ReconcileBlockerResponse {
                    code: "future_lifecycle_gate".to_string(),
                    summary: boundary_summary.clone(),
                })
                .into_iter()
                .collect(),
            approval_required: true,
            approval_requirements: vec![gate.gate_kind.clone()],
            external_effect_summary: if eligible {
                gate.summary.clone()
            } else {
                format!("Future gate: {} {boundary_summary}", gate.summary)
            },
            state_hash: lifecycle_action_hash(json!({
                "action": "satisfy_approval_gate",
                "gate_id": gate.id,
                "gate_status": gate.status,
                "gate_kind": gate.gate_kind,
                "eligible": eligible,
                "boundary": boundary_summary,
                "work_item_updated_at": work_item.updated_at,
            })),
        });
    }
    if let Some(run_id) = workspaces
        .last()
        .and_then(|workspace| workspace.run_id.as_ref())
    {
        if let Some(extension) = state.store.pending_budget_extension_for_run(run_id).await? {
            action_rail.push(WorkItemActionResponse {
                id: "approve_budget_extension".to_string(),
                lifecycle_stage: "attempt".to_string(),
                resource: extension.id,
                status: "ready".to_string(),
                effect_class: "approval_boundary".to_string(),
                blockers: Vec::new(),
                approval_required: true,
                approval_requirements: vec!["budget_extension".to_string()],
                external_effect_summary: format!(
                    "Resume the existing workspace with exactly {} additional turns and {} additional tokens.",
                    extension.turn_increment, extension.token_increment
                ),
                state_hash: extension.state_hash,
            });
        }
    }
    if matches!(work_item.status.as_str(), "blocked" | "failed") {
        let mut blockers = Vec::new();
        if work_item.attempt_count >= work_item.max_attempts {
            blockers.push(ReconcileBlockerResponse {
                code: "attempt_budget_exhausted".to_string(),
                summary: "The WorkItem has no remaining attempt budget.".to_string(),
            });
        }
        if change_set.is_some() {
            blockers.push(ReconcileBlockerResponse {
                code: "source_delivery_started".to_string(),
                summary:
                    "Replanning is disabled after a ChangeSet exists; review or roll back instead."
                        .to_string(),
            });
        }
        action_rail.push(WorkItemActionResponse {
            id: "replan_work_item".to_string(),
            lifecycle_stage: "planning".to_string(),
            resource: work_item.id.clone(),
            status: if blockers.is_empty() { "ready" } else { "blocked" }.to_string(),
            effect_class: "internal".to_string(),
            blockers,
            approval_required: false,
            approval_requirements: Vec::new(),
            external_effect_summary: "Create a fresh isolated workspace for another explicit coding attempt; no attempt starts automatically.".to_string(),
            state_hash: lifecycle_action_hash(json!({
                "action": "replan_work_item",
                "work_item": work_item.id,
                "status": work_item.status,
                "updated_at": work_item.updated_at,
                "attempt_count": work_item.attempt_count,
                "max_attempts": work_item.max_attempts,
                "change_set": change_set.as_ref().map(|value| &value.id),
            })),
        });
    }
    if reconcile_preview.action == WorkItemReconcileAction::GitOpsDeliveryFailed.as_str() {
        let change_set = sdlc_flow
            .as_ref()
            .and_then(|flow| flow.gitops_change_set.as_ref());
        let delivery = sdlc_flow
            .as_ref()
            .and_then(|flow| flow.gitops_delivery.as_ref());
        if let (Some(change_set), Some(delivery)) = (change_set, delivery) {
            let closed_observation = delivery.latest_observation.as_ref().filter(|observation| {
                gitops_observation_closed_unmerged(observation.content_json.as_ref())
            });
            let terminal_evidence = closed_observation.or(delivery.latest_result.as_ref());
            if let Some(terminal_evidence) = terminal_evidence {
                let terminal_summary = if closed_observation.is_some() {
                    format!(
                        "closed, unmerged pull request observation {}",
                        terminal_evidence.id
                    )
                } else {
                    format!("failed delivery {}", terminal_evidence.id)
                };
                action_rail.push(WorkItemActionResponse {
                    id: "repropose_gitops_change_set".to_string(),
                    lifecycle_stage: "gitops".to_string(),
                    resource: change_set.id.clone(),
                    status: "ready".to_string(),
                    effect_class: "approval_boundary".to_string(),
                    blockers: Vec::new(),
                    approval_required: true,
                    approval_requirements: vec!["gitops_retry_review".to_string()],
                    external_effect_summary: format!(
                        "Re-propose GitOps ChangeSet {} as reviewed revision {} after {}; use a fresh revision-scoped branch, revoke the previous writer grant, and require fresh base-revision evidence, review, and authorization. This action does not contact GitHub.",
                        change_set.id,
                        change_set.revision + 1,
                        terminal_summary,
                    ),
                    state_hash: lifecycle_action_hash(json!({
                        "action": "repropose_gitops_change_set",
                        "gitops_change_set_id": change_set.id,
                        "status": change_set.status,
                        "revision": change_set.revision,
                        "updated_at": change_set.updated_at,
                        "terminal_evidence_id": terminal_evidence.id,
                        "terminal_evidence": terminal_evidence.content_json,
                    })),
                });
            }
        }
    }
    action_rail.push(work_item_action_response(&reconcile_preview));
    let rollback_intent = latest_rollback_intent(&state, &work_item, None).await?;
    let rollback_writer_base_ready = sdlc_flow
        .as_ref()
        .and_then(|flow| flow.gitops_delivery.as_ref())
        .and_then(|delivery| delivery.latest_merge.as_ref())
        .is_some();
    if let Some(rollback) = rollback_intent.as_ref() {
        let rollback_id = rollback
            .pointer("/content/rollback_intent_id")
            .and_then(Value::as_str)
            .unwrap_or("rollback_unavailable");
        let status = rollback
            .pointer("/content/status")
            .and_then(Value::as_str)
            .unwrap_or("unavailable");
        let (action_id, action_status, effect_class, summary, approval_requirements) = match status {
            "prepared" if rollback_writer_base_ready => ("approve_rollback", "ready", "approval_boundary", format!("Approve the digest-bound RollbackIntent {rollback_id}; no rollback runs automatically."), vec!["production_rollback".to_string()]),
            "prepared" => ("approve_rollback", "blocked", "external_wait", format!("RollbackIntent {rollback_id} is prepared, but its writer cannot be authorized until the deployment GitOps pull request has an observed immutable merge."), vec!["production_rollback".to_string()]),
            "approved" => ("execute_rollback_gitops_pr", "ready", "external_effect", format!("Create the digest-only rollback pull request in {PROTECTED_GITOPS_REPO}; merge remains manual."), Vec::new()),
            "awaiting_manual_merge" => ("observe_rollback_merge", "ready", "external_effect", format!("Observe the rollback pull request in {PROTECTED_GITOPS_REPO}; merge remains manual."), Vec::new()),
            "ready_for_argo_sync" => ("approve_rollback_argo_sync", "ready", "approval_boundary", format!("Open a fresh production rollback window before syncing exact Argo Application {PROTECTED_ARGO_APPLICATION}."), vec!["production_rollback_deployment".to_string(), "cluster_mutation".to_string(), "production_impact".to_string()]),
            "argo_approved" => ("execute_rollback_argo_sync", "ready", "external_effect", format!("Sync exact Argo Application {PROTECTED_ARGO_APPLICATION} to the observed rollback merge; no other target may be supplied."), Vec::new()),
            "argo_syncing" => ("observe_rollback_argo_sync", "ready", "external_effect", format!("Observe the isolated Argo executor and verify {PROTECTED_WORKLOAD_NAME} against the captured baseline digest."), Vec::new()),
            "verified" => ("rollback_verified", "completed", "internal", format!("RollbackIntent {rollback_id} was explicitly synced and verified."), Vec::new()),
            _ => ("inspect_rollback", "blocked", "internal", format!("Inspect RollbackIntent {rollback_id} with owner {PROTECTED_ROLLBACK_OWNER}."), Vec::new()),
        };
        let state_hash = format!("{:x}", Sha256::digest(rollback.to_string().as_bytes()));
        action_rail.push(WorkItemActionResponse {
            id: action_id.to_string(),
            lifecycle_stage: "rollback".to_string(),
            resource: rollback_id.to_string(),
            status: action_status.to_string(),
            effect_class: effect_class.to_string(),
            blockers: Vec::new(),
            approval_required: !approval_requirements.is_empty(),
            approval_requirements,
            external_effect_summary: summary,
            state_hash,
        });
    }

    let desired_digest = sdlc_flow
        .as_ref()
        .and_then(|flow| flow.pipeline_intent.as_ref())
        .and_then(|intent| intent.intent_json.pointer("/build_output/image_digest"))
        .and_then(Value::as_str);
    let current_digest = rollback_intent
        .as_ref()
        .and_then(|rollback| rollback.pointer("/content/baseline/image_digest"))
        .and_then(Value::as_str);
    let desired_gitops_revision = sdlc_flow
        .as_ref()
        .and_then(|flow| flow.gitops_delivery.as_ref())
        .and_then(|delivery| delivery.latest_merge.as_ref())
        .and_then(|artifact| artifact.content_json.as_ref())
        .and_then(|content| content.get("merge_commit_sha"))
        .and_then(Value::as_str);
    let release_evidence = sdlc_flow
        .as_ref()
        .and_then(|flow| flow.release.as_ref())
        .map(|release| &release.release_json);
    let delivery_configuration = json!({
        "pipeline_contract_id": work_item.pipeline_contract_id,
        "deployment_contract_id": work_item.deployment_contract_id,
        "gitops": {
            "repository": work_item.gitops_repo,
            "ref": work_item.gitops_ref,
            "kustomization_path": work_item.gitops_kustomization_path,
            "image_name": work_item.gitops_image_name,
            "desired_revision": desired_gitops_revision,
        },
        "target": {
            "environment": work_item.target_environment,
            "namespace": work_item.target_namespace,
            "argo_application": work_item.argo_application,
            "workload_kind": work_item.workload_kind,
            "workload_name": work_item.workload_name,
        },
        "current_digest": current_digest,
        "desired_digest": desired_digest,
        "argo": {
            "sync_status": release_evidence.and_then(|evidence| evidence.pointer("/post_sync_verification/argo/sync_status")).and_then(Value::as_str),
            "health_status": release_evidence.and_then(|evidence| evidence.pointer("/post_sync_verification/argo/health_status")).and_then(Value::as_str),
        },
        "production_window_expires_at": rollback_intent.as_ref().and_then(|rollback| rollback.pointer("/content/argo_authorization_expires_at").or_else(|| rollback.pointer("/content/authorization_expires_at"))).and_then(Value::as_str),
        "baseline_digest": current_digest,
        "rollback_owner": work_item.rollback_owner,
        "rollback_intent_id": rollback_intent.as_ref().and_then(|rollback| rollback.pointer("/content/rollback_intent_id")).and_then(Value::as_str),
        "rollback_status": rollback_intent.as_ref().and_then(|rollback| rollback.pointer("/content/status")).and_then(Value::as_str),
    });

    Ok(Json(WorkItemFlowResponse {
        work_item: work_item.into(),
        reconcile_preview,
        sdlc_flow,
        delivery_segments,
        workspaces,
        controller_waits,
        audit_events,
        action_rail,
        delivery_configuration,
    }))
}

pub(in crate::app) fn lifecycle_action_hash(payload: Value) -> String {
    format!("{:x}", Sha256::digest(payload.to_string().as_bytes()))
}

pub(in crate::app) fn resource_review_actions(
    resource_kind: &str,
    lifecycle_stage: &str,
    resource_id: &str,
    status: &str,
    updated_at: Option<&str>,
    revision: i64,
) -> Vec<WorkItemActionResponse> {
    vec![
        resource_review_action(
            lifecycle_stage,
            resource_kind,
            resource_id,
            status,
            updated_at,
            revision,
            true,
        ),
        resource_review_action(
            lifecycle_stage,
            resource_kind,
            resource_id,
            status,
            updated_at,
            revision,
            false,
        ),
    ]
}

pub(in crate::app) fn resource_review_action(
    lifecycle_stage: &str,
    resource_kind: &str,
    resource_id: &str,
    status: &str,
    updated_at: Option<&str>,
    revision: i64,
    approve: bool,
) -> WorkItemActionResponse {
    let action_id = format!(
        "{}_{resource_kind}",
        if approve { "approve" } else { "reject" }
    );
    WorkItemActionResponse {
        id: action_id.clone(),
        lifecycle_stage: lifecycle_stage.to_string(),
        resource: resource_id.to_string(),
        status: "ready".to_string(),
        effect_class: "approval_boundary".to_string(),
        blockers: Vec::new(),
        approval_required: true,
        approval_requirements: vec![format!("{resource_kind}_review")],
        external_effect_summary: format!(
            "{} {resource_kind} {resource_id}. This review changes durable PHarness state only.",
            if approve { "Approve" } else { "Reject" }
        ),
        state_hash: lifecycle_action_hash(json!({
            "action": &action_id,
            "resource_kind": resource_kind,
            "resource_id": resource_id,
            "status": status,
            "updated_at": updated_at,
            "revision": revision,
        })),
    }
}

pub(in crate::app) fn work_item_action_response(
    preview: &ReconcileWorkItemResponse,
) -> WorkItemActionResponse {
    let action = preview.action.as_str();
    let lifecycle_stage = if action.contains("pipeline") {
        "pipeline"
    } else if action.contains("gitops") {
        "gitops"
    } else if action.contains("rollback") {
        "rollback"
    } else if action.contains("deployment") || action.contains("argo") {
        "deployment"
    } else if action.contains("release") || action == "complete_work_item" {
        "release"
    } else {
        "source"
    };
    let effect_class = if action == "start_coding_attempt" {
        "model_execution"
    } else if action.contains("approval") || action.contains("authorization") {
        "approval_boundary"
    } else if action == "awaiting_gitops_pull_request_merge" {
        "external_effect"
    } else if action.starts_with("wait_") || action.contains("merge") {
        "external_wait"
    } else if matches!(
        action,
        "awaiting_git_delivery_execution"
            | "awaiting_pull_request_observation"
            | "awaiting_pipeline_execution"
            | "awaiting_gitops_base_revision"
            | "prepare_rollback_intent"
            | "awaiting_gitops_delivery_execution"
            | "awaiting_gitops_pull_request_observation"
            | "awaiting_deployment_execution"
            | "awaiting_release_verification"
    ) {
        "external_effect"
    } else {
        "internal"
    };
    let hash_payload = json!({
        "work_item_id": preview.work_item.id,
        "work_item_updated_at": preview.work_item.updated_at,
        "action": preview.action,
        "can_apply": preview.can_apply,
        "blockers": preview.blockers,
        "authorization_checks": preview.authorization_checks,
    });
    let approval_requirements = preview
        .authorization_checks
        .iter()
        .filter(|check| matches!(check.status.as_str(), "missing" | "blocked" | "unavailable"))
        .map(|check| check.kind.clone())
        .collect::<Vec<_>>();
    WorkItemActionResponse {
        id: preview.action.clone(),
        lifecycle_stage: lifecycle_stage.to_string(),
        resource: preview.work_item.id.clone(),
        status: if preview.can_apply {
            "ready"
        } else {
            "blocked"
        }
        .to_string(),
        effect_class: effect_class.to_string(),
        blockers: preview.blockers.clone(),
        approval_required: effect_class == "approval_boundary" || !approval_requirements.is_empty(),
        approval_requirements,
        external_effect_summary: preview.effect_summary.clone(),
        state_hash: format!("{:x}", Sha256::digest(hash_payload.to_string().as_bytes())),
    }
}
