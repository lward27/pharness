use super::super::approvals::{
    append_approval_gate_audit_event, append_permission_grant_audit_event,
    create_permission_grant_record, ensure_approved_for_trusted_envelope,
    trusted_envelope_grant_request,
};
use super::super::audit::{
    append_change_set_audit_event, append_deployment_intent_audit_event,
    append_pipeline_intent_audit_event, append_registry_evidence_audit_event,
    append_release_audit_event, append_work_item_audit_event, append_work_plan_audit_event,
    append_workspace_audit_event,
};
use super::super::auth::OperatorIdentity;
use super::super::clock::unique_suffix;
use super::super::operator::{
    all_work_plans_for_operator_groups, group_operator_records, operator_resource_label,
};
use super::super::sdlc::{build_sdlc_flow, build_sdlc_readiness};
use super::super::validation::clean_optional_text;
use super::super::work_items::lifecycle::{
    approval_gates_from_work_item, work_item_approval_gate_specs,
};
use super::super::{ApiError, AppState};
use crate::dto::{
    CreateTrustedEnvelopeRequest, CreateWorkPlanFromRemediationPlanRequest, CreateWorkPlanResponse,
    ReviseWorkPlanRequest, ReviseWorkPlanResponse, SdlcFlowResponse, SdlcReadinessResponse,
    TransitionWorkPlanRequest, TransitionWorkPlanResponse, TrustedEnvelopeResponse,
    WorkPlanResponse, WorkPlansResponse,
};
use axum::extract::{Path, Query, State};
use axum::{Extension, Json};
use pharness_core::{PermissionGrantScope, RunId, SessionId};
use pharness_store::{
    CreateSession, CreateWorkPlan, CreateWorkspace, SqliteStore, StoreError, StoredChangeSet,
    StoredDeploymentIntent, StoredPermissionGrant, StoredPipelineIntent, StoredRegistryEvidence,
    StoredRelease, StoredRemediationPlan, StoredWorkItem, UpdateWorkPlanRevision,
    WorkPlanListFilter,
};
use serde_json::json;

pub(in crate::app) async fn create_work_plan_from_work_item(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(work_item_id): Path<String>,
) -> Result<Json<CreateWorkPlanResponse>, ApiError> {
    if let Some(existing) = state
        .store
        .get_work_plan_by_work_item(&work_item_id)
        .await?
    {
        return Ok(Json(CreateWorkPlanResponse {
            work_plan: existing.into(),
            created: false,
        }));
    }
    let work_item = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    if work_item.status != "planning" {
        return Err(ApiError::conflict(
            "a WorkItem must be planning before it can create a WorkPlan",
        ));
    }
    let actor = identity.map(|Extension(OperatorIdentity(name))| name);
    let session_id = SessionId::new(format!("ses_work_item_{}", unique_suffix()));
    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: format!("WorkItem: {}", work_item.title),
            cwd: format!("work-item/{}", work_item.id),
        })
        .await?;
    let work_plan = state
        .store
        .create_work_plan(work_plan_from_work_item(
            &work_item,
            session_id,
            format!("wplan_{}", unique_suffix()),
        ))
        .await?;
    for gate in approval_gates_from_work_item(&work_item, &work_plan) {
        let gate = state.store.create_approval_gate(gate).await?;
        append_approval_gate_audit_event(&state.store, &gate, "approval_gate.created", "created")
            .await?;
    }
    let workspace = state
        .store
        .create_workspace(CreateWorkspace {
            id: format!("ws_{}", unique_suffix()),
            work_item_id: work_item.id.clone(),
            run_id: None,
            status: "declared".to_string(),
            source_repo: work_item.source_repo.clone(),
            source_ref: work_item.source_ref.clone(),
            resolved_commit: None,
            branch: None,
            retention_status: "ephemeral".to_string(),
            actor: actor.clone(),
            reason: Some("WorkItem planning declared an isolated workspace".to_string()),
        })
        .await?;
    append_work_item_audit_event(
        &state.store,
        &work_item,
        "work_item.work_plan_created",
        actor.clone(),
        json!({ "work_plan_id": work_plan.id, "workspace_id": workspace.id }),
    )
    .await?;
    append_workspace_audit_event(&state.store, &workspace, "workspace.declared", actor).await?;

    let work_item = state
        .store
        .update_work_item_status(
            &work_item.id,
            "awaiting_approval",
            None,
            Some("WorkPlan and workspace are ready for review".to_string()),
        )
        .await?;
    append_work_item_audit_event(
        &state.store,
        &work_item,
        "work_item.awaiting_approval",
        None,
        json!({ "work_plan_id": work_plan.id, "workspace_id": workspace.id }),
    )
    .await?;

    Ok(Json(CreateWorkPlanResponse {
        work_plan: work_plan.into(),
        created: true,
    }))
}

#[derive(Debug, Default, serde::Deserialize)]
pub(in crate::app) struct ListWorkPlansQuery {
    pub(in crate::app) work_item_id: Option<String>,
    pub(in crate::app) remediation_plan_id: Option<String>,
    pub(in crate::app) incident_id: Option<String>,
    pub(in crate::app) run_id: Option<String>,
    pub(in crate::app) status: Option<String>,
    pub(in crate::app) origin: Option<String>,
    pub(in crate::app) actor: Option<String>,
    pub(in crate::app) risk_level: Option<String>,
    pub(in crate::app) resource_namespace: Option<String>,
    pub(in crate::app) resource_kind: Option<String>,
    pub(in crate::app) resource_name: Option<String>,
    pub(in crate::app) created_after_ms: Option<i64>,
    pub(in crate::app) created_before_ms: Option<i64>,
    pub(in crate::app) limit: Option<u32>,
    pub(in crate::app) offset: Option<u32>,
}

pub(in crate::app) async fn list_work_plans(
    State(state): State<AppState>,
    Query(query): Query<ListWorkPlansQuery>,
) -> Result<Json<WorkPlansResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let filter = WorkPlanListFilter {
        work_item_id: clean_optional_text(query.work_item_id),
        remediation_plan_id: clean_optional_text(query.remediation_plan_id),
        incident_id: clean_optional_text(query.incident_id),
        run_id: clean_optional_text(query.run_id).map(RunId::new),
        status: clean_optional_text(query.status),
        origin: clean_optional_text(query.origin),
        created_by: clean_optional_text(query.actor),
        risk_level: clean_optional_text(query.risk_level),
        resource_namespace: clean_optional_text(query.resource_namespace),
        resource_kind: clean_optional_text(query.resource_kind),
        resource_name: clean_optional_text(query.resource_name),
        created_after_ms: query.created_after_ms,
        created_before_ms: query.created_before_ms,
        limit,
        offset,
    };
    let work_plans = state
        .store
        .list_work_plans(filter.clone())
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<WorkPlanResponse>>();
    let count = work_plans.len();
    let group_work_plans = all_work_plans_for_operator_groups(state.store.as_ref(), filter).await?;
    let groups = group_operator_records(group_work_plans.iter().map(|plan| {
        (
            plan.id.clone(),
            plan.created_at.clone(),
            plan.title.clone(),
            operator_resource_label(
                plan.resource_namespace.as_deref(),
                plan.resource_kind.as_deref(),
                plan.resource_name.as_deref(),
            ),
            plan.status.clone(),
        )
    }));

    Ok(Json(WorkPlansResponse {
        work_plans,
        groups,
        count,
        limit,
        offset,
    }))
}

pub(in crate::app) async fn get_work_plan(
    State(state): State<AppState>,
    Path(work_plan_id): Path<String>,
) -> Result<Json<WorkPlanResponse>, ApiError> {
    let work_plan = state
        .store
        .get_work_plan(&work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &work_plan_id))?;

    Ok(Json(work_plan.into()))
}

pub(in crate::app) async fn work_plan_readiness(
    State(state): State<AppState>,
    Path(work_plan_id): Path<String>,
) -> Result<Json<SdlcReadinessResponse>, ApiError> {
    let work_plan = state
        .store
        .get_work_plan(&work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &work_plan_id))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?;
    let resource_id = work_plan.id.clone();

    build_sdlc_readiness(
        &state.store,
        "work_plan",
        &resource_id,
        work_plan,
        change_set,
    )
    .await
    .map(Json)
}

pub(in crate::app) async fn work_plan_flow(
    State(state): State<AppState>,
    Path(work_plan_id): Path<String>,
) -> Result<Json<SdlcFlowResponse>, ApiError> {
    let work_plan = state
        .store
        .get_work_plan(&work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &work_plan_id))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?;
    let resource_id = work_plan.id.clone();
    build_sdlc_flow(
        &state.store,
        "work_plan",
        &resource_id,
        work_plan,
        change_set,
    )
    .await
    .map(Json)
}

pub(in crate::app) async fn transition_work_plan(
    State(state): State<AppState>,
    Path(work_plan_id): Path<String>,
    Json(request): Json<TransitionWorkPlanRequest>,
) -> Result<Json<TransitionWorkPlanResponse>, ApiError> {
    let current = state
        .store
        .get_work_plan(&work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &work_plan_id))?;
    let target = WorkPlanStatus::parse(&request.target_status)?;
    let current_status = WorkPlanStatus::parse(&current.status)?;
    current_status.ensure_can_transition_to(target)?;

    let work_plan = state
        .store
        .update_work_plan_status(
            &work_plan_id,
            target.as_str(),
            clean_optional_text(request.actor.clone()),
            clean_optional_text(request.reason.clone()),
        )
        .await?;
    append_work_plan_audit_event(
        &state.store,
        &work_plan,
        &format!("work_plan.{}", target.as_str()),
        clean_optional_text(request.actor),
        clean_optional_text(request.reason),
        json!({
            "previous_status": current.status,
            "target_status": target.as_str(),
        }),
    )
    .await?;

    Ok(Json(TransitionWorkPlanResponse {
        work_plan: work_plan.into(),
    }))
}

pub(in crate::app) async fn revise_work_plan(
    State(state): State<AppState>,
    Path(work_plan_id): Path<String>,
    Json(request): Json<ReviseWorkPlanRequest>,
) -> Result<Json<ReviseWorkPlanResponse>, ApiError> {
    let current = state
        .store
        .get_work_plan(&work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &work_plan_id))?;
    if current.status == "completed" {
        return Err(ApiError::conflict("completed work plans cannot be revised"));
    }

    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let work_plan = state
        .store
        .revise_work_plan(
            &work_plan_id,
            UpdateWorkPlanRevision {
                title: clean_optional_text(request.title),
                summary: clean_optional_text(request.summary),
                risk_level: clean_optional_text(request.risk_level),
                requires_approval: request.requires_approval,
                work_plan_json: request.work_plan_json,
                actor: actor.clone(),
                reason: reason.clone(),
            },
        )
        .await?;
    let invalidated_gates = if request.material_change {
        if let Some(remediation_plan_id) = &work_plan.remediation_plan_id {
            state
                .store
                .stale_approval_gates_for_remediation_plan(
                    remediation_plan_id,
                    actor.clone(),
                    reason.clone().or_else(|| {
                        Some(format!(
                            "work plan {} revised from revision {} to {}",
                            work_plan.id, current.revision, work_plan.revision
                        ))
                    }),
                )
                .await?
        } else if let Some(work_item_id) = &work_plan.work_item_id {
            state
                .store
                .stale_approval_gates_for_work_item(
                    work_item_id,
                    actor.clone(),
                    reason.clone().or_else(|| {
                        Some(format!(
                            "work plan {} revised from revision {} to {}",
                            work_plan.id, current.revision, work_plan.revision
                        ))
                    }),
                )
                .await?
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    for gate in &invalidated_gates {
        append_approval_gate_audit_event(&state.store, gate, "approval_gate.stale", "stale")
            .await?;
    }
    let invalidated_trusted_envelopes = if request.material_change {
        stale_trusted_envelopes_for_work_plan(
            &state.store,
            &work_plan.id,
            actor.clone(),
            reason.clone().or_else(|| {
                Some(format!(
                    "work plan {} revised from revision {} to {}",
                    work_plan.id, current.revision, work_plan.revision
                ))
            }),
        )
        .await?
    } else {
        Vec::new()
    };
    let invalidated_change_set = if request.material_change {
        stale_change_set_for_work_plan(
            &state.store,
            &work_plan.id,
            actor.clone(),
            reason.clone().or_else(|| {
                Some(format!(
                    "work plan {} revised from revision {} to {}",
                    work_plan.id, current.revision, work_plan.revision
                ))
            }),
        )
        .await?
    } else {
        None
    };
    if let Some(change_set) = &invalidated_change_set {
        append_change_set_audit_event(
            &state.store,
            change_set,
            "change_set.stale",
            actor.clone(),
            reason.clone(),
            json!({
                "source": "work_plan_revision",
                "work_plan_id": work_plan.id,
                "work_plan_revision": work_plan.revision,
            }),
        )
        .await?;
    }
    append_work_plan_audit_event(
        &state.store,
        &work_plan,
        "work_plan.revised",
        actor,
        reason,
        json!({
            "previous_revision": current.revision,
            "revision": work_plan.revision,
            "material_change": request.material_change,
            "invalidated_gate_ids": invalidated_gates
                .iter()
                .map(|gate| gate.id.clone())
                .collect::<Vec<_>>(),
            "invalidated_change_set_id": invalidated_change_set
                .as_ref()
                .map(|change_set| change_set.id.clone()),
            "invalidated_permission_grant_ids": invalidated_trusted_envelopes
                .iter()
                .map(|grant| grant.id.clone())
                .collect::<Vec<_>>(),
        }),
    )
    .await?;

    Ok(Json(ReviseWorkPlanResponse {
        work_plan: work_plan.into(),
        invalidated_gates: invalidated_gates.into_iter().map(Into::into).collect(),
        invalidated_change_set: invalidated_change_set.map(Into::into),
    }))
}

pub(in crate::app) async fn stale_change_set_for_work_plan(
    store: &SqliteStore,
    work_plan_id: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<Option<StoredChangeSet>, StoreError> {
    let Some(change_set) = store.get_change_set_by_work_plan(work_plan_id).await? else {
        return Ok(None);
    };
    if !matches!(
        change_set.status.as_str(),
        "draft" | "proposed" | "approved"
    ) {
        return Ok(None);
    }

    store
        .update_change_set_status(&change_set.id, "stale", actor, reason)
        .await
        .map(Some)
}

pub(in crate::app) async fn stale_trusted_envelopes_for_work_plan(
    store: &SqliteStore,
    work_plan_id: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<Vec<StoredPermissionGrant>, ApiError> {
    stale_trusted_envelopes_matching(store, actor, reason, |scope| {
        !scope.work_plan_ids.is_empty() && scope.work_plan_ids.iter().any(|id| id == work_plan_id)
    })
    .await
}

pub(in crate::app) async fn stale_trusted_envelopes_for_change_set(
    store: &SqliteStore,
    change_set_id: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<Vec<StoredPermissionGrant>, ApiError> {
    stale_trusted_envelopes_matching(store, actor, reason, |scope| {
        !scope.change_set_ids.is_empty()
            && scope.change_set_ids.iter().any(|id| id == change_set_id)
    })
    .await
}

pub(in crate::app) async fn stale_pipeline_intent_for_change_set(
    store: &SqliteStore,
    change_set_id: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<Option<StoredPipelineIntent>, ApiError> {
    let Some(intent) = store
        .get_pipeline_intent_by_change_set(change_set_id)
        .await?
    else {
        return Ok(None);
    };
    if intent.status == "stale" {
        return Ok(None);
    }

    let previous_status = intent.status.clone();
    let intent = store
        .update_pipeline_intent_status(&intent.id, "stale", actor.clone(), reason.clone())
        .await?;
    append_pipeline_intent_audit_event(
        store,
        &intent,
        "pipeline_intent.stale",
        actor,
        reason,
        json!({
            "previous_status": previous_status,
            "source": "change_set_revision",
            "change_set_id": change_set_id,
        }),
    )
    .await?;

    Ok(Some(intent))
}

pub(in crate::app) async fn stale_deployment_intent_for_pipeline_intent(
    store: &SqliteStore,
    pipeline_intent_id: &str,
    actor: Option<String>,
    reason: Option<String>,
    source: &'static str,
) -> Result<Option<StoredDeploymentIntent>, ApiError> {
    let Some(intent) = store
        .get_deployment_intent_by_pipeline_intent(pipeline_intent_id)
        .await?
    else {
        return Ok(None);
    };
    if intent.status == "stale" {
        return Ok(None);
    }

    let previous_status = intent.status.clone();
    let intent = store
        .update_deployment_intent_status(&intent.id, "stale", actor.clone(), reason.clone())
        .await?;
    append_deployment_intent_audit_event(
        store,
        &intent,
        "deployment_intent.stale",
        actor,
        reason,
        json!({
            "previous_status": previous_status,
            "source": source,
            "pipeline_intent_id": pipeline_intent_id,
        }),
    )
    .await?;

    Ok(Some(intent))
}

pub(in crate::app) async fn stale_release_for_deployment_intent(
    store: &SqliteStore,
    deployment_intent_id: &str,
    actor: Option<String>,
    reason: Option<String>,
    source: &'static str,
) -> Result<Option<StoredRelease>, ApiError> {
    let Some(release) = store
        .get_release_by_deployment_intent(deployment_intent_id)
        .await?
    else {
        return Ok(None);
    };
    if release.status == "stale" {
        return Ok(None);
    }

    let previous_status = release.status.clone();
    let release = store
        .update_release_status(&release.id, "stale", actor.clone(), reason.clone())
        .await?;
    append_release_audit_event(
        store,
        &release,
        "release.stale",
        actor,
        reason,
        json!({
            "previous_status": previous_status,
            "source": source,
            "deployment_intent_id": deployment_intent_id,
        }),
    )
    .await?;

    Ok(Some(release))
}

pub(in crate::app) async fn stale_registry_evidence_for_release(
    store: &SqliteStore,
    release_id: &str,
    actor: Option<String>,
    reason: Option<String>,
    source: &'static str,
) -> Result<Option<StoredRegistryEvidence>, ApiError> {
    let Some(evidence) = store.get_registry_evidence_by_release(release_id).await? else {
        return Ok(None);
    };
    if evidence.status == "stale" {
        return Ok(None);
    }

    let previous_status = evidence.status.clone();
    let evidence = store
        .update_registry_evidence_status(&evidence.id, "stale", actor.clone(), reason.clone())
        .await?;
    append_registry_evidence_audit_event(
        store,
        &evidence,
        "registry_evidence.stale",
        actor,
        reason,
        json!({
            "previous_status": previous_status,
            "source": source,
            "release_id": release_id,
        }),
    )
    .await?;

    Ok(Some(evidence))
}

pub(in crate::app) async fn stale_trusted_envelopes_matching(
    store: &SqliteStore,
    actor: Option<String>,
    reason: Option<String>,
    matches_scope: impl Fn(&PermissionGrantScope) -> bool,
) -> Result<Vec<StoredPermissionGrant>, ApiError> {
    let active_grants = store.list_permission_grants(Some("active"), 200).await?;
    let mut staled = Vec::new();
    for grant in active_grants {
        let scope = serde_json::from_value::<PermissionGrantScope>(grant.scope_json.clone())
            .map_err(|error| {
                ApiError::internal(format!(
                    "permission grant {} has invalid scope: {error}",
                    grant.id
                ))
            })?;
        if !matches_scope(&scope) {
            continue;
        }

        let grant = store
            .stale_permission_grant(&grant.id, actor.clone(), reason.clone())
            .await?;
        append_permission_grant_audit_event(store, "permission_grant.stale", &grant, actor.clone())
            .await?;
        staled.push(grant);
    }

    Ok(staled)
}

pub(in crate::app) async fn create_work_plan_from_remediation_plan(
    State(state): State<AppState>,
    Json(request): Json<CreateWorkPlanFromRemediationPlanRequest>,
) -> Result<Json<CreateWorkPlanResponse>, ApiError> {
    let remediation_plan_id = clean_optional_text(Some(request.remediation_plan_id))
        .ok_or_else(|| ApiError::bad_request("remediation_plan_id is required"))?;
    if let Some(existing) = state
        .store
        .get_work_plan_by_remediation_plan(&remediation_plan_id)
        .await?
    {
        return Ok(Json(CreateWorkPlanResponse {
            work_plan: existing.into(),
            created: false,
        }));
    }

    let remediation_plan = state
        .store
        .get_remediation_plan(&remediation_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("remediation_plan", &remediation_plan_id))?;
    if remediation_plan.requires_approval && remediation_plan.status != "approved" {
        return Err(ApiError::conflict(format!(
            "WorkPlan derivation requires an approved remediation plan; {} is {}",
            remediation_plan.id, remediation_plan.status
        )));
    }
    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let work_plan = state
        .store
        .create_work_plan(work_plan_from_remediation_plan(
            &remediation_plan,
            format!("wplan_{}", unique_suffix()),
        ))
        .await?;
    append_work_plan_audit_event(
        &state.store,
        &work_plan,
        "work_plan.created_from_remediation_plan",
        actor,
        reason,
        json!({
            "remediation_plan_id": remediation_plan.id,
            "incident_id": remediation_plan.incident_id,
            "remediation_plan_status": remediation_plan.status,
            "execution_enabled": false,
        }),
    )
    .await?;

    Ok(Json(CreateWorkPlanResponse {
        work_plan: work_plan.into(),
        created: true,
    }))
}

pub(in crate::app) fn work_plan_from_remediation_plan(
    plan: &StoredRemediationPlan,
    id: String,
) -> CreateWorkPlan {
    let steps = plan
        .plan_json
        .get("steps")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let approval_gates = plan
        .plan_json
        .get("approval_gates")
        .cloned()
        .unwrap_or_else(|| json!([]));
    CreateWorkPlan {
        id,
        work_item_id: None,
        remediation_plan_id: Some(plan.id.clone()),
        incident_id: Some(plan.incident_id.clone()),
        session_id: plan.session_id.clone(),
        run_id: plan.run_id.clone(),
        status: "proposed".to_string(),
        title: format!("WorkPlan: {}", plan.title),
        summary: plan.summary.clone(),
        risk_level: plan.risk_level.clone(),
        requires_approval: plan.requires_approval,
        resource_namespace: plan.resource_namespace.clone(),
        resource_kind: plan.resource_kind.clone(),
        resource_name: plan.resource_name.clone(),
        work_plan_json: json!({
            "source": {
                "kind": "remediation_plan",
                "id": plan.id.clone(),
                "incident_id": plan.incident_id.clone(),
            },
            "status": "proposed",
            "execution": {
                "enabled": false,
                "reason": "work plan execution is not implemented",
            },
            "approval_gates": approval_gates,
            "steps": steps,
            "remediation_plan": plan.plan_json.clone(),
        }),
    }
}

pub(in crate::app) fn work_plan_from_work_item(
    item: &StoredWorkItem,
    session_id: SessionId,
    id: String,
) -> CreateWorkPlan {
    CreateWorkPlan {
        id,
        work_item_id: Some(item.id.clone()),
        remediation_plan_id: None,
        incident_id: None,
        session_id,
        run_id: None,
        status: "proposed".to_string(),
        title: format!("WorkPlan: {}", item.title),
        summary: item.intent.clone(),
        risk_level: if item.production_impacting {
            "high".to_string()
        } else {
            "medium".to_string()
        },
        requires_approval: true,
        resource_namespace: item.target_namespace.clone(),
        resource_kind: Some("application".to_string()),
        resource_name: item.argo_application.clone(),
        work_plan_json: json!({
            "source": { "kind": "work_item", "id": item.id },
            "intent": item.intent,
            "acceptance_criteria": item.acceptance_criteria,
            "source_repository": { "repo": item.source_repo, "ref": item.source_ref },
            "gitops_repository": { "repo": item.gitops_repo, "ref": item.gitops_ref },
            "target": {
                "environment": item.target_environment,
                "namespace": item.target_namespace,
                "argo_application": item.argo_application,
                "production_impacting": item.production_impacting,
            },
            "execution": {
                "enabled": false,
                "reason": "workspace execution requires a real pinned Git workspace",
            },
            "approval_gates": work_item_approval_gate_specs(item),
        }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkPlanStatus {
    Draft,
    Proposed,
    Approved,
    Executing,
    Blocked,
    Completed,
    Rejected,
}

impl WorkPlanStatus {
    fn parse(value: &str) -> Result<Self, ApiError> {
        match value {
            "draft" => Ok(Self::Draft),
            "proposed" => Ok(Self::Proposed),
            "approved" => Ok(Self::Approved),
            "executing" => Ok(Self::Executing),
            "blocked" => Ok(Self::Blocked),
            "completed" => Ok(Self::Completed),
            "rejected" => Ok(Self::Rejected),
            other => Err(ApiError::bad_request(format!(
                "unsupported work plan status: {other}"
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Proposed => "proposed",
            Self::Approved => "approved",
            Self::Executing => "executing",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::Rejected => "rejected",
        }
    }

    fn ensure_can_transition_to(self, target: Self) -> Result<(), ApiError> {
        let allowed = match self {
            Self::Draft => matches!(target, Self::Proposed | Self::Rejected),
            Self::Proposed => matches!(target, Self::Approved | Self::Rejected | Self::Draft),
            Self::Approved => matches!(target, Self::Executing | Self::Rejected | Self::Draft),
            Self::Executing => matches!(target, Self::Blocked | Self::Completed),
            Self::Blocked => matches!(target, Self::Executing | Self::Rejected | Self::Draft),
            Self::Completed | Self::Rejected => false,
        };
        if allowed {
            Ok(())
        } else {
            Err(ApiError::conflict(format!(
                "cannot transition work plan from {} to {}",
                self.as_str(),
                target.as_str()
            )))
        }
    }
}

pub(in crate::app) async fn create_work_plan_trusted_envelope(
    State(state): State<AppState>,
    Path(work_plan_id): Path<String>,
    Json(request): Json<CreateTrustedEnvelopeRequest>,
) -> Result<Json<TrustedEnvelopeResponse>, ApiError> {
    let work_plan = state
        .store
        .get_work_plan(&work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &work_plan_id))?;
    ensure_approved_for_trusted_envelope("work_plan", &work_plan.id, &work_plan.status)?;
    let grant_request = trusted_envelope_grant_request(&work_plan.id, None, &request)?;
    let actor = clean_optional_text(request.created_by.clone());
    let reason = clean_optional_text(Some(request.reason.clone()));
    let grant = create_permission_grant_record(&state.store, grant_request).await?;
    append_work_plan_audit_event(
        &state.store,
        &work_plan,
        "work_plan.trusted_envelope_created",
        actor,
        reason,
        json!({
            "permission_grant_id": grant.id,
            "work_plan_id": work_plan.id,
        }),
    )
    .await?;

    Ok(Json(TrustedEnvelopeResponse {
        grant: grant.into(),
    }))
}
