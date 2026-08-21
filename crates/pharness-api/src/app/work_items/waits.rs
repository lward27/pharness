use super::super::audit::{
    append_controller_wait_audit_event, append_deployment_intent_audit_event,
    append_observation_audit_event, append_pipeline_intent_audit_event,
    append_work_item_audit_event,
};
use super::super::auth::OperatorIdentity;
use super::super::clock::current_millis;
use super::super::deployment::execution::internal_argo_sync_outcome;
use super::super::deployment::target::deployment_target;
use super::super::pipeline::execution::internal_pipeline_intent_execution_outcome;
use super::super::validation::{
    clean_optional_text, required_json_string, validate_kubernetes_name,
};
use super::super::{ApiError, AppState, CONTROLLER_WAIT_INTERVAL_MS};
use super::reconcile::reconcile_work_item;
use crate::dto::{
    ArgoSyncOutcomeRequest, AuditEventsResponse, ControllerWaitTickResult, ControllerWaitsResponse,
    PipelineIntentExecutionOutcomeRequest, ReconcileDueControllerWaitsRequest,
    ReconcileDueControllerWaitsResponse, ReconcileWorkItemRequest,
};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use pharness_core::{ActionId, AgentAction, PolicyDecision, ToolExecutor};
use pharness_store::{
    ControllerWaitListFilter, CreateObservation, StoredControllerWait, StoredDeploymentIntent,
    StoredPipelineIntent, StoredWorkItem,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(in crate::app) async fn list_work_item_events(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
) -> Result<Json<AuditEventsResponse>, ApiError> {
    state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    let events = state
        .store
        .list_audit_events(Some("work_item"), Some(&work_item_id), None, 200)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(AuditEventsResponse { events }))
}

#[derive(Debug, Default, serde::Deserialize)]
pub(in crate::app) struct ListControllerWaitsQuery {
    status: Option<String>,
    wait_kind: Option<String>,
    due_before_ms: Option<i64>,
    limit: Option<u32>,
    offset: Option<u32>,
}

pub(in crate::app) async fn list_work_item_controller_waits(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
    Query(query): Query<ListControllerWaitsQuery>,
) -> Result<Json<ControllerWaitsResponse>, ApiError> {
    state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let controller_waits = state
        .store
        .list_controller_waits(ControllerWaitListFilter {
            work_item_id: Some(work_item_id),
            status: clean_optional_text(query.status),
            wait_kind: clean_optional_text(query.wait_kind),
            due_before_ms: query.due_before_ms,
            limit,
            offset,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let count = controller_waits.len();
    Ok(Json(ControllerWaitsResponse {
        controller_waits,
        count,
        limit,
        offset,
    }))
}

/// Reconcile due controller waits against already-persisted delivery evidence.
/// This endpoint never calls a provider, creates a worker Job, retries an
/// action, merges a pull request, or mutates an external target.
pub(in crate::app) async fn reconcile_due_controller_waits(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Json(request): Json<ReconcileDueControllerWaitsRequest>,
) -> Result<Json<ReconcileDueControllerWaitsResponse>, ApiError> {
    let evaluated_at = current_millis();
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let waits = state
        .store
        .list_controller_waits(ControllerWaitListFilter {
            status: Some("active".to_string()),
            due_before_ms: Some(
                i64::try_from(evaluated_at)
                    .map_err(|_| ApiError::internal("controller clock exceeds SQLite range"))?,
            ),
            limit: request.limit.unwrap_or(25).clamp(1, 50),
            ..ControllerWaitListFilter::default()
        })
        .await?;
    let checked = waits.len();
    let mut pending = 0;
    let mut progressed = 0;
    let mut blocked = 0;
    let mut results = Vec::with_capacity(waits.len());

    for wait in waits {
        let work_item = state
            .store
            .get_work_item(&wait.work_item_id)
            .await?
            .ok_or_else(|| ApiError::not_found("work_item", &wait.work_item_id))?;
        if matches!(
            work_item.status.as_str(),
            "completed" | "cancelled" | "failed"
        ) {
            let reason = format!("WorkItem is terminal ({})", work_item.status);
            let resolved = state
                .store
                .resolve_controller_wait(&wait.id, "resolved", reason.clone())
                .await?;
            append_controller_wait_audit_event(
                &state.store,
                &resolved,
                "controller_wait.resolved",
                actor.clone(),
                Some(reason.clone()),
            )
            .await?;
            results.push(ControllerWaitTickResult {
                controller_wait: resolved.into(),
                outcome: "resolved".to_string(),
                next_action: None,
                work_item: work_item.into(),
                message: reason,
            });
            continue;
        }

        let deadline_at = wait.deadline_at.parse::<u128>().unwrap_or(0);
        if deadline_at <= evaluated_at || wait.check_count >= wait.max_checks {
            let reason = if deadline_at <= evaluated_at {
                "controller wait deadline elapsed without observed progress".to_string()
            } else {
                format!(
                    "controller wait exhausted {} observation checks without observed progress",
                    wait.max_checks
                )
            };
            let expired = state
                .store
                .resolve_controller_wait(&wait.id, "expired", reason.clone())
                .await?;
            append_controller_wait_audit_event(
                &state.store,
                &expired,
                "controller_wait.expired",
                actor.clone(),
                Some(reason.clone()),
            )
            .await?;
            let work_item = block_work_item_from_controller_wait_expiry(
                &state,
                &work_item,
                &expired,
                actor.clone(),
                reason.clone(),
            )
            .await?;
            blocked += 1;
            results.push(ControllerWaitTickResult {
                controller_wait: expired.into(),
                outcome: "blocked".to_string(),
                next_action: None,
                work_item: work_item.into(),
                message: reason,
            });
            continue;
        }

        if let Err(error) = observe_due_controller_wait(&state, &wait, actor.clone()).await {
            append_controller_wait_audit_event(
                &state.store,
                &wait,
                "controller_wait.observation_failed",
                actor.clone(),
                Some(format!(
                    "read-only PipelineRun observation was unavailable ({})",
                    controller_wait_observation_failure_reason(&error)
                )),
            )
            .await?;
        }

        let Json(snapshot) = reconcile_work_item(
            State(state.clone()),
            None,
            Path(work_item.id.clone()),
            Json(ReconcileWorkItemRequest {
                apply: false,
                actor: actor.clone(),
                reason: None,
                max_turns: None,
            }),
        )
        .await?;
        let expected_action = wait
            .data_json
            .get("controller_action")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if snapshot.action == expected_action {
            let checked_wait = state
                .store
                .record_controller_wait_check(
                    &wait.id,
                    evaluated_at
                        .saturating_add(CONTROLLER_WAIT_INTERVAL_MS)
                        .to_string(),
                )
                .await?;
            append_controller_wait_audit_event(
                &state.store,
                &checked_wait,
                "controller_wait.checked",
                actor.clone(),
                Some("no new durable evidence observed".to_string()),
            )
            .await?;
            pending += 1;
            results.push(ControllerWaitTickResult {
                controller_wait: checked_wait.into(),
                outcome: "pending".to_string(),
                next_action: Some(snapshot.action),
                work_item: snapshot.work_item,
                message: "no new durable evidence observed; scheduled the next bounded check"
                    .to_string(),
            });
        } else {
            let reason = format!(
                "durable evidence advanced controller action from {expected_action} to {}",
                snapshot.action
            );
            let resolved = state
                .store
                .resolve_controller_wait(&wait.id, "resolved", reason.clone())
                .await?;
            append_controller_wait_audit_event(
                &state.store,
                &resolved,
                "controller_wait.progressed",
                actor.clone(),
                Some(reason.clone()),
            )
            .await?;
            progressed += 1;
            results.push(ControllerWaitTickResult {
                controller_wait: resolved.into(),
                outcome: "progressed".to_string(),
                next_action: Some(snapshot.action),
                work_item: snapshot.work_item,
                message: reason,
            });
        }
    }

    Ok(Json(ReconcileDueControllerWaitsResponse {
        evaluated_at: evaluated_at.to_string(),
        checked,
        pending,
        progressed,
        blocked,
        results,
    }))
}

/// Refreshes only the PipelineRun already named in durable PipelineIntent execution state.
/// This is an observation adapter: it never dispatches Tekton work, retries an execution, or
/// otherwise mutates an external system. Terminal evidence uses the existing outcome boundary;
/// the normal controller reconciliation decides whether that evidence made progress.
pub(in crate::app) async fn observe_due_controller_wait(
    state: &AppState,
    wait: &StoredControllerWait,
    actor: Option<String>,
) -> Result<(), ApiError> {
    match wait.wait_kind.as_str() {
        "pipeline_execution" => observe_due_pipeline_execution_wait(state, wait, actor).await,
        "deployment_execution" => observe_due_deployment_execution_wait(state, wait, actor).await,
        _ => Ok(()),
    }
}

pub(in crate::app) async fn observe_due_pipeline_execution_wait(
    state: &AppState,
    wait: &StoredControllerWait,
    actor: Option<String>,
) -> Result<(), ApiError> {
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&wait.work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("controller wait has no WorkPlan lineage"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?
        .ok_or_else(|| ApiError::conflict("controller wait has no ChangeSet lineage"))?;
    let intent = state
        .store
        .get_pipeline_intent_by_change_set(&change_set.id)
        .await?
        .ok_or_else(|| ApiError::conflict("controller wait has no PipelineIntent lineage"))?;
    if intent.status != "executing" {
        return Ok(());
    }

    let execution_id = intent
        .intent_json
        .pointer("/execution_state/execution_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::conflict("PipelineIntent execution has no execution_id"))?
        .to_string();
    let namespace = intent
        .intent_json
        .pointer("/execution_state/pipeline_run_namespace")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::conflict("PipelineIntent execution has no PipelineRun namespace"))?
        .to_string();
    let name = intent
        .intent_json
        .pointer("/execution_state/pipeline_run_name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::conflict("PipelineIntent execution has no PipelineRun name"))?
        .to_string();
    validate_kubernetes_name("PipelineIntent PipelineRun namespace", &namespace)?;
    validate_kubernetes_name("PipelineIntent PipelineRun name", &name)?;

    let action = AgentAction::TektonAnalyzePipelineRun {
        id: ActionId::new(format!("controller.wait.{}.tekton", wait.id)),
        reason: "Observe the exact PipelineRun recorded by a durable controller wait".to_string(),
        namespace: namespace.clone(),
        name: name.clone(),
    };
    if !matches!(
        state.policy.evaluate_action(&action),
        PolicyDecision::Allow { .. }
    ) {
        return Err(ApiError::conflict(
            "controller policy does not permit read-only Tekton PipelineRun observation",
        ));
    }
    let result = state
        .cluster_tools
        .execute(&action)
        .await
        .map_err(|_| ApiError::internal("read-only Tekton PipelineRun observation failed"))?;
    let analysis = result
        .content
        .get("analysis")
        .cloned()
        .ok_or_else(|| ApiError::internal("Tekton observation returned no analysis"))?;
    validate_pipeline_run_analysis_target(&namespace, &name, &analysis)?;
    let observed_status = analysis
        .pointer("/summary/status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    match observed_status {
        "succeeded" | "failed" | "cancelled" => {
            let outcome_status = if observed_status == "succeeded" {
                "completed"
            } else {
                "failed"
            };
            let Json(updated) = internal_pipeline_intent_execution_outcome(
                State(state.clone()),
                Path(intent.id.clone()),
                Json(PipelineIntentExecutionOutcomeRequest {
                    execution_id: execution_id.clone(),
                    status: outcome_status.to_string(),
                    pipeline_run_namespace: Some(namespace.clone()),
                    pipeline_run_name: Some(name.clone()),
                    error: (observed_status != "succeeded")
                        .then(|| format!("PipelineRun reached terminal {observed_status} status")),
                    pipeline_run_analysis: Some(analysis.clone()),
                    analysis_error: None,
                }),
            )
            .await?;
            let updated_intent = state
                .store
                .get_pipeline_intent(&updated.id)
                .await?
                .ok_or_else(|| ApiError::not_found("pipeline_intent", &updated.id))?;
            append_pipeline_intent_audit_event(
                &state.store,
                &updated_intent,
                "pipeline_intent.execution_observed",
                actor.or_else(|| Some("controller:tekton-observer".to_string())),
                Some("recorded terminal PipelineRun evidence from an exact typed read".to_string()),
                json!({
                    "execution_id": execution_id,
                    "pipeline_run_namespace": namespace,
                    "pipeline_run_name": name,
                    "observed_status": observed_status,
                }),
            )
            .await?;
        }
        "running" | "unknown" => {
            persist_pipeline_execution_wait_observation(
                state,
                &intent,
                PipelineExecutionWaitObservationInput {
                    execution_id: &execution_id,
                    namespace: &namespace,
                    name: &name,
                    observed_status,
                    analysis: &analysis,
                },
                actor,
            )
            .await?;
        }
        _ => {
            return Err(ApiError::internal(
                "Tekton observation returned an unsupported PipelineRun status",
            ));
        }
    }

    Ok(())
}

/// Reads one Argo CD Application already bound to an active Argo execution.
/// It never initiates reconciliation: terminal evidence is accepted only from
/// Argo's compact operation phase and exact declared application target.
pub(in crate::app) async fn observe_due_deployment_execution_wait(
    state: &AppState,
    wait: &StoredControllerWait,
    actor: Option<String>,
) -> Result<(), ApiError> {
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&wait.work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("controller wait has no WorkPlan lineage"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?
        .ok_or_else(|| ApiError::conflict("controller wait has no ChangeSet lineage"))?;
    let pipeline_intent = state
        .store
        .get_pipeline_intent_by_change_set(&change_set.id)
        .await?
        .ok_or_else(|| ApiError::conflict("controller wait has no PipelineIntent lineage"))?;
    let intent = state
        .store
        .get_deployment_intent_by_pipeline_intent(&pipeline_intent.id)
        .await?
        .ok_or_else(|| ApiError::conflict("controller wait has no DeploymentIntent lineage"))?;
    if intent.status != "approved" {
        return Ok(());
    }
    let target = deployment_target(&intent)?;
    validate_kubernetes_name("DeploymentIntent argo_application", &target.application)?;
    let run_id = intent
        .run_id
        .clone()
        .ok_or_else(|| ApiError::conflict("DeploymentIntent has no coding run provenance"))?;
    let artifacts = state.store.list_artifacts(&run_id).await?;
    let execution = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "argo_sync_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("deployment_intent_id").and_then(Value::as_str)
                        == Some(intent.id.as_str())
                })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .ok_or_else(|| ApiError::conflict("DeploymentIntent has no active Argo sync execution"))?;
    let execution_content = execution
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Argo sync execution has no structured content"))?;
    let execution_id =
        required_json_string(execution_content, "execution_id", "Argo sync execution")?;
    let execution_target = execution_content
        .get("target")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Argo sync execution has no immutable target"))?;
    if execution_target.get("environment").and_then(Value::as_str) != Some(&target.environment)
        || execution_target.get("namespace").and_then(Value::as_str) != Some(&target.namespace)
        || execution_target
            .get("argo_application")
            .and_then(Value::as_str)
            != Some(&target.application)
    {
        return Err(ApiError::conflict(
            "Argo sync execution target no longer matches DeploymentIntent",
        ));
    }

    let action = AgentAction::ArgoGetApp {
        id: ActionId::new(format!("controller.wait.{}.argo", wait.id)),
        reason: "Observe the exact Argo CD Application recorded by a durable controller wait"
            .to_string(),
        app: target.application.clone(),
    };
    if !matches!(
        state.policy.evaluate_action(&action),
        PolicyDecision::Allow { .. }
    ) {
        return Err(ApiError::conflict(
            "controller policy does not permit read-only Argo Application observation",
        ));
    }
    let result = state
        .cluster_tools
        .execute(&action)
        .await
        .map_err(|_| ApiError::internal("read-only Argo Application observation failed"))?;
    let analysis = result
        .content
        .get("analysis")
        .cloned()
        .ok_or_else(|| ApiError::internal("Argo observation returned no analysis"))?;
    if analysis.get("kind").and_then(Value::as_str) != Some("Application")
        || analysis.get("name").and_then(Value::as_str) != Some(target.application.as_str())
    {
        return Err(ApiError::internal(
            "Argo observation did not match the durable Application target",
        ));
    }
    let sync_status = analysis.get("sync_status").and_then(Value::as_str);
    let health_status = analysis
        .get("health_status")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let operation_phase = analysis.get("operation_phase").and_then(Value::as_str);
    let revision = analysis
        .get("operation_revision")
        .and_then(Value::as_str)
        .or_else(|| analysis.get("revision").and_then(Value::as_str))
        .map(ToOwned::to_owned);

    let terminal_status = match operation_phase {
        Some("Succeeded") if sync_status == Some("Synced") => Some("completed"),
        Some("Failed" | "Error") => Some("failed"),
        Some("Terminated") => Some("cancelled"),
        _ => None,
    };
    if let Some(status) = terminal_status {
        let Json(result) = internal_argo_sync_outcome(
            State(state.clone()),
            Path(intent.id.clone()),
            Json(ArgoSyncOutcomeRequest {
                execution_id: execution_id.clone(),
                status: status.to_string(),
                sync_status: sync_status.map(ToOwned::to_owned),
                health_status,
                operation_phase: operation_phase.map(ToOwned::to_owned),
                revision,
                error_code: (status != "completed").then(|| {
                    if status == "cancelled" {
                        "cancelled".to_string()
                    } else {
                        "argo_operation_failed".to_string()
                    }
                }),
            }),
        )
        .await?;
        append_deployment_intent_audit_event(
            &state.store,
            &intent,
            "deployment_intent.execution_observed",
            actor.or_else(|| Some("controller:argo-observer".to_string())),
            Some(
                "recorded terminal Argo Application evidence from an exact typed read".to_string(),
            ),
            json!({
                "execution_id": execution_id,
                "argo_application": target.application,
                "observed_status": status,
                "operation_phase": operation_phase,
                "sync_status": sync_status,
                "result_artifact_id": result.id,
            }),
        )
        .await?;
    } else {
        persist_deployment_execution_wait_observation(
            state,
            &intent,
            DeploymentExecutionWaitObservationInput {
                execution_id: &execution_id,
                application: &target.application,
                observed_operation_phase: operation_phase,
                observed_sync_status: sync_status,
                analysis: &analysis,
            },
            actor,
        )
        .await?;
    }
    Ok(())
}

pub(in crate::app) struct DeploymentExecutionWaitObservationInput<'a> {
    execution_id: &'a str,
    application: &'a str,
    observed_operation_phase: Option<&'a str>,
    observed_sync_status: Option<&'a str>,
    analysis: &'a Value,
}

pub(in crate::app) async fn persist_deployment_execution_wait_observation(
    state: &AppState,
    intent: &StoredDeploymentIntent,
    input: DeploymentExecutionWaitObservationInput<'_>,
    actor: Option<String>,
) -> Result<(), ApiError> {
    let phase = input.observed_operation_phase.unwrap_or("unknown");
    let observation_id = format!(
        "obs_argo_wait_{}_{}",
        safe_controller_wait_id_fragment(input.execution_id),
        safe_controller_wait_id_fragment(phase)
    );
    let observation = match state.store.get_observation(&observation_id).await? {
        Some(existing) => existing,
        None => {
            let created = state
                .store
                .create_observation(CreateObservation {
                    id: observation_id,
                    session_id: intent.session_id.clone(),
                    run_id: intent.run_id.clone(),
                    source: "argocd".to_string(),
                    kind: "argo_sync_wait_observation".to_string(),
                    subject: input.application.to_string(),
                    summary: format!(
                        "Argo Application {} remains {} during bounded controller wait",
                        input.application, phase
                    ),
                    resource_namespace: input
                        .analysis
                        .get("namespace")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    resource_kind: Some("Application".to_string()),
                    resource_name: Some(input.application.to_string()),
                    resource_ref_json: Some(json!({
                        "apiVersion": "argoproj.io/v1alpha1",
                        "kind": "Application",
                        "name": input.application,
                    })),
                    artifact_id: None,
                    data_json: json!({
                        "execution_id": input.execution_id,
                        "operation_phase": input.observed_operation_phase,
                        "sync_status": input.observed_sync_status,
                        "analysis": input.analysis,
                    }),
                })
                .await?;
            append_observation_audit_event(
                &state.store,
                &created,
                "observation.created",
                actor
                    .clone()
                    .or_else(|| Some("controller:argo-observer".to_string())),
                Some("recorded nonterminal exact Argo Application observation".to_string()),
            )
            .await?;
            created
        }
    };
    append_deployment_intent_audit_event(
        &state.store,
        intent,
        "deployment_intent.execution_observed",
        actor.or_else(|| Some("controller:argo-observer".to_string())),
        Some(
            "recorded nonterminal Argo Application observation from an exact typed read"
                .to_string(),
        ),
        json!({
            "execution_id": input.execution_id,
            "argo_application": input.application,
            "operation_phase": input.observed_operation_phase,
            "sync_status": input.observed_sync_status,
            "observation_id": observation.id,
        }),
    )
    .await?;
    Ok(())
}

pub(in crate::app) struct PipelineExecutionWaitObservationInput<'a> {
    execution_id: &'a str,
    namespace: &'a str,
    name: &'a str,
    observed_status: &'a str,
    analysis: &'a Value,
}

pub(in crate::app) async fn persist_pipeline_execution_wait_observation(
    state: &AppState,
    intent: &StoredPipelineIntent,
    input: PipelineExecutionWaitObservationInput<'_>,
    actor: Option<String>,
) -> Result<(), ApiError> {
    let observation_id = format!(
        "obs_pipeline_wait_{}_{}",
        safe_controller_wait_id_fragment(input.execution_id),
        input.observed_status
    );
    let observation = match state.store.get_observation(&observation_id).await? {
        Some(existing) => existing,
        None => {
            let created = state
                .store
                .create_observation(CreateObservation {
                    id: observation_id,
                    session_id: intent.session_id.clone(),
                    run_id: intent.run_id.clone(),
                    source: "tekton".to_string(),
                    kind: "pipeline_run_wait_observation".to_string(),
                    subject: format!("{}/{}", input.namespace, input.name),
                    summary: format!(
                        "PipelineRun {}/{} remains {} during bounded controller wait",
                        input.namespace, input.name, input.observed_status
                    ),
                    resource_namespace: Some(input.namespace.to_string()),
                    resource_kind: Some("PipelineRun".to_string()),
                    resource_name: Some(input.name.to_string()),
                    resource_ref_json: Some(json!({
                        "apiVersion": "tekton.dev/v1",
                        "kind": "PipelineRun",
                        "namespace": input.namespace,
                        "name": input.name,
                    })),
                    artifact_id: None,
                    data_json: json!({
                        "execution_id": input.execution_id,
                        "observed_status": input.observed_status,
                        "analysis": input.analysis,
                    }),
                })
                .await?;
            append_observation_audit_event(
                &state.store,
                &created,
                "observation.created",
                actor
                    .clone()
                    .or_else(|| Some("controller:tekton-observer".to_string())),
                Some("recorded nonterminal exact PipelineRun observation".to_string()),
            )
            .await?;
            created
        }
    };
    append_pipeline_intent_audit_event(
        &state.store,
        intent,
        "pipeline_intent.execution_observed",
        actor.or_else(|| Some("controller:tekton-observer".to_string())),
        Some("recorded nonterminal PipelineRun observation from an exact typed read".to_string()),
        json!({
            "execution_id": input.execution_id,
            "pipeline_run_namespace": input.namespace,
            "pipeline_run_name": input.name,
            "observed_status": input.observed_status,
            "observation_id": observation.id,
        }),
    )
    .await?;
    Ok(())
}

pub(in crate::app) fn validate_pipeline_run_analysis_target(
    namespace: &str,
    name: &str,
    analysis: &Value,
) -> Result<(), ApiError> {
    if analysis.get("kind").and_then(Value::as_str) != Some("PipelineRunAnalysis") {
        return Err(ApiError::internal(
            "Tekton observation returned an invalid PipelineRun analysis",
        ));
    }
    if analysis
        .pointer("/pipeline_run/namespace")
        .and_then(Value::as_str)
        != Some(namespace)
        || analysis
            .pointer("/pipeline_run/name")
            .and_then(Value::as_str)
            != Some(name)
    {
        return Err(ApiError::internal(
            "Tekton observation did not match the durable PipelineRun target",
        ));
    }
    Ok(())
}

pub(in crate::app) fn safe_controller_wait_id_fragment(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    format!("{:x}", digest)[..16].to_string()
}

pub(in crate::app) fn controller_wait_observation_failure_reason(error: &ApiError) -> &'static str {
    match error.status {
        StatusCode::CONFLICT => "target or policy validation failed",
        _ => "typed observer did not return usable evidence",
    }
}

/// Stop a non-terminal WorkItem when the controller can no longer observe a
/// required external result within its bounded wait budget. This creates no
/// remediation, retry, rollback, or external side effect.
pub(in crate::app) async fn block_work_item_from_controller_wait_expiry(
    state: &AppState,
    work_item: &StoredWorkItem,
    wait: &StoredControllerWait,
    actor: Option<String>,
    reason: String,
) -> Result<StoredWorkItem, ApiError> {
    if matches!(
        work_item.status.as_str(),
        "blocked" | "completed" | "cancelled" | "failed"
    ) {
        return Ok(work_item.clone());
    }
    let blocked = state
        .store
        .update_work_item_status(
            &work_item.id,
            "blocked",
            actor.clone(),
            Some(reason.clone()),
        )
        .await?;
    append_work_item_audit_event(
        &state.store,
        &blocked,
        "work_item.controller_wait_blocked",
        actor,
        json!({
            "source": "controller_wait.reconcile_due",
            "previous_status": work_item.status,
            "controller_wait_id": wait.id,
            "wait_kind": wait.wait_kind,
            "deadline_at": wait.deadline_at,
            "max_checks": wait.max_checks,
            "check_count": wait.check_count,
            "reason": reason,
            "automatic_retry": false,
            "automatic_rollback": false,
            "mutation_performed": false,
        }),
    )
    .await?;
    Ok(blocked)
}
