use super::approvals::create_permission_grant_record;
use super::clock::current_millis;
use super::hashing::canonical_material_hash;
use super::identifiers::{is_git_sha, new_prefixed_id};
use super::products::ensure_repo_mode_enabled;
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
    CreateAgentContextPack, CreateAuditEvent, CreateEnvironmentPreparation,
    CreateEvidenceValidation, CreateOperatorAnnotation, CreateOperatorAnnotationDecision,
    CreateProviderCheckSetObservation, CreateRepoWorkItem, CreateRun, CreateSession,
    CreateSourceDeliveryIntent, CreateStageChainAuthorization, CreateStageExecution,
    CreateWorkspace, SealStageOutcome, StoredBudgetExtension, StoredChangeSet,
    StoredOperatorAnnotation, StoredOperatorAnnotationDecision, StoredRepoWorkItemMetadata,
    StoredRun, StoredSourceDeliveryIntent, StoredStageOutcome, UpdateEnvironmentPreparation,
    UpdateWorkspaceExecution, WorkspaceListFilter,
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
    let current_run = match work_item.current_run_id.as_ref() {
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
    let audit_events = state
        .store
        .list_audit_events(Some("work_item"), Some(work_item_id), None, 100)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    let first_action = action_rail.first();
    let work_item_response: crate::dto::WorkItemResponse = work_item.clone().into();
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
        workspaces,
        controller_waits: Vec::new(),
        audit_events,
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
            "stage_executions":executions,
            "effective_stage_outcomes":outcomes,
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
    let (attempt_count, max_attempts) = attempts;
    if metadata.closed_at.is_some() {
        return Ok(Vec::new());
    }
    let state_hash = repo_work_item_state_hash(metadata)?;
    if let Some(extension) = pending_budget_extension {
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
                extension.turn_increment, extension.token_increment
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

pub(in crate::app) async fn execute_repo_work_item_action(
    state: &AppState,
    work_item_id: &str,
    action_id: &str,
    actor: String,
    reason: String,
    state_hash: String,
) -> Result<Value, ApiError> {
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
    let current_run = match work_item.current_run_id.as_ref() {
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
            authorize_repo_stage_chain(state, work_item_id, &actor, &reason, None).await
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
        "authorize_source_delivery" => {
            authorize_and_dispatch_source_delivery(state, work_item_id, &actor, &reason).await
        }
        "observe_source_delivery" => {
            dispatch_source_delivery_observation(state, work_item_id, &actor, &reason).await
        }
        _ => Err(ApiError::conflict("unsupported Repo Mode action")),
    }
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
    authorize_repo_stage_chain(state, work_item_id, actor, reason, Some(workspace)).await
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
            actor: "controller:repo-mode".into(),
            reason: stop_reason.into(),
        })
        .await?;
    for stage in ["release", "observe"] {
        let input =
            json!({"reason":"Repo Mode V1 is source-only","source_delivery_intent_id":intent.id});
        let execution = state
            .store
            .create_stage_execution(CreateStageExecution {
                id: new_prefixed_id("stageexec"),
                work_item_id: work_item_id.into(),
                stage_key: stage.into(),
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
        let metadata = repo_metadata(state, work_item_id).await?;
        let outcome = json!({"schema_version":pharness_core::STAGE_OUTCOME_SCHEMA,"work_item_id":work_item_id,
            "stage_execution_id":execution.id,"stage":stage,"status":"inapplicable","objective":{"kind":"source_only_repo_mode"},
            "pinned_inputs":input,"verified_facts":[],"agent_claims":[],"outputs":[],"acceptance":[],"decisions":[],
            "authorizations":[],"contradictions":[],"risks":[],"unavailable_capabilities":[],"recommendations":[],
            "stop_reason":"Repo Mode V1 does not create deployment Release or Observe work","sealed_state_version":metadata.state_version});
        state
            .store
            .seal_stage_outcome(SealStageOutcome {
                id: new_prefixed_id("stageout"),
                stage_execution_id: execution.id,
                work_item_id: work_item_id.into(),
                stage_key: stage.into(),
                status: "inapplicable".into(),
                content_hash: canonical_material_hash(&outcome)?,
                outcome,
                state_version: metadata.state_version,
                supersedes_outcome_id: None,
                actor: "controller:repo-mode".into(),
                reason: "source-only contract".into(),
            })
            .await?;
    }
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

async fn seal_repo_inapplicable_tail(
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
    if !state.worker.supports_remote_workspace() {
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
    let profile =
        pharness_core::compiled_agent_profiles(&model, pharness_runhost::SYSTEM_PROMPT_VERSION)
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
    let cwd = state.worker.effective_cwd("/workspace");
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
        ..RunScope::default()
    };
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
                "kind":state.worker.execution_target_kind(),
                "repo_mode":{"stage_execution_id":stage_execution_id,"stage":"plan","context_pack_id":context_pack_id},
                "agent_profile":profile,
                "agent_context":context,
                "agent_evidence_payloads":evidence.payloads,
                "run_scope":scope.to_optional_json(),
                "run_budget":profile.budget,
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
            workspace_id: None,
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
    state.worker.spawn_run(run.clone(), cwd);
    Ok(json!({"work_item":item,"stage_execution":execution,"context_pack":pack,"run":run}))
}

async fn authorize_repo_stage_chain(
    state: &AppState,
    work_item_id: &str,
    actor: &str,
    reason: &str,
    reuse_workspace: Option<pharness_store::StoredWorkspace>,
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
    let profiles =
        pharness_core::compiled_agent_profiles(&model, pharness_runhost::SYSTEM_PROMPT_VERSION)
            .into_iter()
            .filter(|profile| {
                matches!(
                    profile.id.as_str(),
                    "repo-builder" | "repo-tester" | "repo-verifier"
                )
            })
            .collect::<Vec<_>>();
    if profiles.len() != 3 {
        return Err(ApiError::internal(
            "compiled Repo Mode stage chain is incomplete",
        ));
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
                "repo-builder":work_item.run_budget,
                "repo-tester":profiles.iter().find(|profile| profile.id == "repo-tester").map(|profile| &profile.budget),
                "repo-verifier":profiles.iter().find(|profile| profile.id == "repo-verifier").map(|profile| &profile.budget),
            }),
            state_hash: repo_work_item_state_hash(&metadata)?,
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
    )
    .await
    {
        Ok(started) => Ok(json!({
            "stage_chain_authorization":authorization,
            "workspace":workspace,
            "builder":started,
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
) -> Result<Value, ApiError> {
    if !state.worker.enabled() {
        return Err(ApiError::unavailable(
            "model execution worker is unavailable",
        ));
    }
    let profile = authorization
        .profile_chain
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|value| serde_json::from_value::<pharness_core::AgentProfile>(value.clone()).ok())
        .filter(|profile| profile.id == "repo-builder")
        .ok_or_else(|| ApiError::conflict("chain authorization has no repo-builder profile"))?;
    let environment_profile = super::environment::select_profile(
        &state.environment_profiles,
        &contract.environment_profile,
        &work_item.source_repo,
    )
    .map_err(ApiError::conflict)?
    .clone();
    let reused_environment_snapshot = if reuse_prepared_workspace {
        let prior_run_id = workspace
            .run_id
            .as_ref()
            .ok_or_else(|| ApiError::conflict("correction workspace has no prior prepared Run"))?;
        let prior_run = state
            .store
            .get_run(prior_run_id)
            .await?
            .ok_or_else(|| ApiError::not_found("run", prior_run_id.as_str()))?;
        let snapshot = prior_run
            .execution_target_json
            .get("environment_snapshot")
            .filter(|snapshot| !snapshot.is_null())
            .cloned()
            .ok_or_else(|| {
                ApiError::conflict("correction workspace has no reusable EnvironmentSnapshot")
            })?;
        let typed: pharness_core::EnvironmentSnapshot = serde_json::from_value(snapshot.clone())
            .map_err(|error| {
                ApiError::conflict(format!(
                    "correction EnvironmentSnapshot is invalid: {error}"
                ))
            })?;
        if typed.source_sha != work_item.source_commit.clone().unwrap_or_default()
            || typed.runner_image_digest != environment_profile.image
            || typed.runner_revision != environment_profile.revision
            || typed.manifest_sha256
                != work_item
                    .repository_contract_hash
                    .clone()
                    .unwrap_or_default()
        {
            return Err(ApiError::conflict(
                "correction EnvironmentSnapshot no longer matches the pinned source, contract, or runner",
            ));
        }
        Some(snapshot)
    } else {
        None
    };
    if !state.worker.supports_remote_workspace() {
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
        "remaining_budgets":work_item.run_budget,
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
    let cwd = state.worker.effective_cwd("/workspace");
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
                "actions":["write_file","patch_file","create_directory"],
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
    let mut execution_target = json!({
        "kind":"kubernetes_workspace",
        "repo_mode":{"stage_execution_id":stage_execution_id,"stage":"implement","context_pack_id":context_pack_id,"chain_authorization_id":authorization.id},
        "agent_profile":profile,
        "agent_context":context,
        "agent_evidence_payloads":evidence.payloads,
        "policy":policy,
        "run_scope":scope.to_optional_json(),
        "workspace":{"base_commit":source_commit,"branch":branch},
        "workspace_source":source,
        "run_budget":work_item.run_budget,
        "environment_profile_id":work_item.environment_profile_id,
        "repository_contract":work_item.repository_contract_json,
        "selected_acceptance_commands":work_item.acceptance_criteria,
        "runner_profile":environment_profile,
    });
    if let Some(snapshot) = reused_environment_snapshot.clone() {
        execution_target["environment_snapshot"] = snapshot;
    }
    let run = state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: format!(
                "Implement the approved WorkPlan for this exact Repo Mode intent: {}",
                work_item.intent
            ),
            cwd: cwd.clone(),
            max_turns: work_item.run_budget.initial_turns,
            initial_status: if reuse_prepared_workspace {
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
            &work_item.run_budget,
            &RunBudgetConsumption {
                allowed_turns: work_item.run_budget.initial_turns,
                allowed_tokens: work_item.run_budget.initial_tokens,
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
            status: if reuse_prepared_workspace {
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
                status: if reuse_prepared_workspace {
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
    state
        .store
        .start_work_item_attempt(
            &work_item.id,
            &run.id,
            Some(actor.into()),
            Some(reason.into()),
        )
        .await?;
    if reuse_prepared_workspace {
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
    }))
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
    if outcome.status != "succeeded" {
        return Ok(None);
    }
    match stage {
        "implement" => start_repo_followup_stage(state, completed_run, "test")
            .await
            .map(Some),
        "test" => start_repo_followup_stage(state, completed_run, "verify")
            .await
            .map(Some),
        _ => Ok(None),
    }
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
    let profile_id = match stage {
        "test" => "repo-tester",
        "verify" => "repo-verifier",
        _ => return Err(ApiError::internal("unsupported Repo Mode follow-up stage")),
    };
    let profile = authorization
        .profile_chain
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|value| serde_json::from_value::<pharness_core::AgentProfile>(value.clone()).ok())
        .filter(|profile| profile.id == profile_id)
        .ok_or_else(|| {
            ApiError::conflict(format!("chain authorization has no {profile_id} profile"))
        })?;
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
        "remaining_budgets":profile.budget,
        "policies":{"source_only":true,"workspace_access":if stage == "test" {"ephemeral_copy"} else {"read_only"}},
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
    let cwd = state.worker.effective_cwd("/workspace");
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
    let run = state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: if stage == "test" {
                "Execute every selected RepositoryContract acceptance command, report exact evidence, and submit the typed Test outcome."
            } else {
                "Verify the approved plan, Builder diff, changed paths, and Test evidence; submit the typed verification decision."
            }
            .into(),
            cwd: cwd.clone(),
            max_turns: profile.budget.initial_turns,
            initial_status: "queued".into(),
            execution_target_json: json!({
                "kind":"kubernetes_workspace",
                "repo_mode":{"stage_execution_id":execution_id,"stage":stage,"context_pack_id":context_id,"chain_authorization_id":authorization.id,"workspace_access":if stage == "test" {"ephemeral_copy"} else {"read_only"}},
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
            agent_profile_id: Some(profile.id.clone()),
            agent_profile_version: Some(profile.version.clone()),
            agent_profile_hash: Some(profile.profile_hash.clone()),
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
                reason: Some(format!("automatic authorized {stage} dispatch")),
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
            payload: json!({"source":"repo_mode_controller","stage":stage,"stage_execution_id":execution.id,"chain_authorization_id":authorization.id}),
        })
        .await?;
    state.worker.spawn_run(run.clone(), cwd);
    Ok(json!({"run":run,"stage_execution":execution,"context_pack":pack,"workspace":workspace}))
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

async fn current_readiness_mismatches(
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
            result_json: Some(json!({"status":"failed"})),
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
}
