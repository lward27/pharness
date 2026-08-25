use super::super::approvals::{
    append_approval_gate_audit_event, create_permission_grant_record,
    ensure_approved_for_trusted_envelope, trusted_envelope_grant_request,
};
use super::super::audit::append_change_set_audit_event;
use super::super::clock::unique_suffix;
use super::super::hashing::material_hash;
use super::super::sdlc::{build_sdlc_flow, build_sdlc_readiness};
use super::super::validation::{clean_optional_text, ensure_json_object};
use super::super::{ApiError, AppState};
use super::work_plans::{
    stale_deployment_intent_for_pipeline_intent, stale_pipeline_intent_for_change_set,
    stale_registry_evidence_for_release, stale_release_for_deployment_intent,
    stale_trusted_envelopes_for_change_set,
};
use crate::dto::{
    ChangeSetResponse, ChangeSetsResponse, CreateChangeSetRequest, CreateChangeSetResponse,
    CreateTrustedEnvelopeRequest, ReviseChangeSetRequest, ReviseChangeSetResponse,
    SdlcFlowResponse, SdlcReadinessResponse, TransitionChangeSetRequest,
    TransitionChangeSetResponse, TrustedEnvelopeResponse,
};
use axum::extract::{Path, Query, State};
use axum::Json;
use pharness_core::{RunId, RunScope};
use pharness_store::{ChangeSetListFilter, CreateChangeSet, UpdateChangeSetRevision};
use serde_json::json;

#[derive(Debug, Default, serde::Deserialize)]
pub(in crate::app) struct ListChangeSetsQuery {
    pub(in crate::app) work_item_id: Option<String>,
    pub(in crate::app) work_plan_id: Option<String>,
    pub(in crate::app) remediation_plan_id: Option<String>,
    pub(in crate::app) incident_id: Option<String>,
    pub(in crate::app) run_id: Option<String>,
    pub(in crate::app) status: Option<String>,
    pub(in crate::app) risk_level: Option<String>,
    pub(in crate::app) resource_namespace: Option<String>,
    pub(in crate::app) resource_kind: Option<String>,
    pub(in crate::app) resource_name: Option<String>,
    pub(in crate::app) created_after_ms: Option<i64>,
    pub(in crate::app) created_before_ms: Option<i64>,
    pub(in crate::app) limit: Option<u32>,
    pub(in crate::app) offset: Option<u32>,
}

pub(in crate::app) async fn list_change_sets(
    State(state): State<AppState>,
    Query(query): Query<ListChangeSetsQuery>,
) -> Result<Json<ChangeSetsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let change_sets = state
        .store
        .list_change_sets(ChangeSetListFilter {
            work_item_id: clean_optional_text(query.work_item_id),
            work_plan_id: clean_optional_text(query.work_plan_id),
            remediation_plan_id: clean_optional_text(query.remediation_plan_id),
            incident_id: clean_optional_text(query.incident_id),
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            status: clean_optional_text(query.status),
            risk_level: clean_optional_text(query.risk_level),
            resource_namespace: clean_optional_text(query.resource_namespace),
            resource_kind: clean_optional_text(query.resource_kind),
            resource_name: clean_optional_text(query.resource_name),
            created_after_ms: query.created_after_ms,
            created_before_ms: query.created_before_ms,
            limit,
            offset,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let count = change_sets.len();

    Ok(Json(ChangeSetsResponse {
        change_sets,
        count,
        limit,
        offset,
    }))
}

pub(in crate::app) async fn get_change_set(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
) -> Result<Json<ChangeSetResponse>, ApiError> {
    let change_set = state
        .store
        .get_change_set(&change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &change_set_id))?;

    Ok(Json(change_set.into()))
}

pub(in crate::app) async fn change_set_readiness(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
) -> Result<Json<SdlcReadinessResponse>, ApiError> {
    let change_set = state
        .store
        .get_change_set(&change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &change_set_id))?;
    let work_plan = state
        .store
        .get_work_plan(&change_set.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &change_set.work_plan_id))?;
    let resource_id = change_set.id.clone();

    build_sdlc_readiness(
        &state.store,
        "change_set",
        &resource_id,
        work_plan,
        Some(change_set),
    )
    .await
    .map(Json)
}

pub(in crate::app) async fn change_set_flow(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
) -> Result<Json<SdlcFlowResponse>, ApiError> {
    let change_set = state
        .store
        .get_change_set(&change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &change_set_id))?;
    let work_plan = state
        .store
        .get_work_plan(&change_set.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &change_set.work_plan_id))?;
    let resource_id = change_set.id.clone();
    build_sdlc_flow(
        &state.store,
        "change_set",
        &resource_id,
        work_plan,
        Some(change_set),
    )
    .await
    .map(Json)
}

pub(in crate::app) async fn create_change_set(
    State(state): State<AppState>,
    Json(request): Json<CreateChangeSetRequest>,
) -> Result<Json<CreateChangeSetResponse>, ApiError> {
    ensure_json_object(&request.change_set_json, "change_set_json")?;
    let work_plan_id = clean_optional_text(Some(request.work_plan_id))
        .ok_or_else(|| ApiError::bad_request("work_plan_id is required"))?;
    if let Some(existing) = state
        .store
        .get_change_set_by_work_plan(&work_plan_id)
        .await?
    {
        return Ok(Json(CreateChangeSetResponse {
            change_set: existing.into(),
            created: false,
        }));
    }
    let work_plan = state
        .store
        .get_work_plan(&work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &work_plan_id))?;
    let (Some(remediation_plan_id), Some(incident_id)) = (
        work_plan.remediation_plan_id.clone(),
        work_plan.incident_id.clone(),
    ) else {
        return Err(ApiError::conflict(
            "WorkItem-backed ChangeSets require captured workspace Git diff provenance",
        ));
    };
    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let material_hash = material_hash(&request.change_set_json)?;
    let change_set = state
        .store
        .create_change_set(CreateChangeSet {
            id: format!("cset_{}", unique_suffix()),
            work_item_id: None,
            work_plan_id: work_plan.id.clone(),
            remediation_plan_id: Some(remediation_plan_id),
            incident_id: Some(incident_id),
            session_id: work_plan.session_id.clone(),
            run_id: work_plan.run_id.clone(),
            status: "draft".to_string(),
            title: clean_optional_text(request.title)
                .unwrap_or_else(|| format!("ChangeSet: {}", work_plan.title)),
            summary: clean_optional_text(request.summary).unwrap_or(work_plan.summary),
            risk_level: clean_optional_text(request.risk_level).unwrap_or(work_plan.risk_level),
            material_hash,
            resource_namespace: work_plan.resource_namespace,
            resource_kind: work_plan.resource_kind,
            resource_name: work_plan.resource_name,
            change_set_json: request.change_set_json,
        })
        .await?;
    append_change_set_audit_event(
        &state.store,
        &change_set,
        "change_set.created",
        actor,
        reason,
        json!({ "created": true }),
    )
    .await?;

    Ok(Json(CreateChangeSetResponse {
        change_set: change_set.into(),
        created: true,
    }))
}

pub(in crate::app) async fn transition_change_set(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
    Json(request): Json<TransitionChangeSetRequest>,
) -> Result<Json<TransitionChangeSetResponse>, ApiError> {
    let current = state
        .store
        .get_change_set(&change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &change_set_id))?;
    let target = ChangeSetStatus::parse(&request.target_status)?;
    let current_status = ChangeSetStatus::parse(&current.status)?;
    current_status.ensure_can_transition_to(target)?;

    let change_set = state
        .store
        .update_change_set_status(
            &change_set_id,
            target.as_str(),
            clean_optional_text(request.actor.clone()),
            clean_optional_text(request.reason.clone()),
        )
        .await?;
    append_change_set_audit_event(
        &state.store,
        &change_set,
        &format!("change_set.{}", target.as_str()),
        clean_optional_text(request.actor),
        clean_optional_text(request.reason),
        json!({
            "previous_status": current.status,
            "target_status": target.as_str(),
        }),
    )
    .await?;

    Ok(Json(TransitionChangeSetResponse {
        change_set: change_set.into(),
    }))
}

pub(in crate::app) fn coding_run_scope_matches_source(
    run_scope: &RunScope,
    work_item_id: &str,
    workspace_id: &str,
    source_repo: &str,
    branch: &str,
    production_impacting: bool,
) -> bool {
    run_scope.work_item_id.as_deref() == Some(work_item_id)
        && run_scope.workspace_id.as_deref() == Some(workspace_id)
        && run_scope.repo.as_deref() == Some(source_repo)
        && run_scope.branch.as_deref() == Some(branch)
        && run_scope.production_impacting == production_impacting
}

pub(in crate::app) async fn revise_change_set(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
    Json(request): Json<ReviseChangeSetRequest>,
) -> Result<Json<ReviseChangeSetResponse>, ApiError> {
    ensure_json_object(&request.change_set_json, "change_set_json")?;
    let current = state
        .store
        .get_change_set(&change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &change_set_id))?;
    if current.status == "applied" {
        return Err(ApiError::conflict("applied change sets cannot be revised"));
    }

    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let material_hash = material_hash(&request.change_set_json)?;
    let material_hash_changed = current.material_hash != material_hash;
    let change_set = state
        .store
        .revise_change_set(
            &change_set_id,
            UpdateChangeSetRevision {
                title: clean_optional_text(request.title),
                summary: clean_optional_text(request.summary),
                risk_level: clean_optional_text(request.risk_level),
                material_hash,
                change_set_json: request.change_set_json,
                session_id: None,
                run_id: None,
                status: None,
                actor: actor.clone(),
                reason: reason.clone(),
            },
        )
        .await?;
    let invalidated_gates = if request.material_change && material_hash_changed {
        if let Some(remediation_plan_id) = &change_set.remediation_plan_id {
            state
                .store
                .stale_approval_gates_for_remediation_plan(
                    remediation_plan_id,
                    actor.clone(),
                    reason.clone().or_else(|| {
                        Some(format!(
                            "change set {} revised from revision {} to {}",
                            change_set.id, current.revision, change_set.revision
                        ))
                    }),
                )
                .await?
        } else if let Some(work_item_id) = &change_set.work_item_id {
            state
                .store
                .stale_approval_gates_for_work_item(
                    work_item_id,
                    actor.clone(),
                    reason.clone().or_else(|| {
                        Some(format!(
                            "change set {} revised from revision {} to {}",
                            change_set.id, current.revision, change_set.revision
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
    let invalidated_trusted_envelopes = if request.material_change && material_hash_changed {
        stale_trusted_envelopes_for_change_set(
            &state.store,
            &change_set.id,
            actor.clone(),
            reason.clone().or_else(|| {
                Some(format!(
                    "change set {} revised from revision {} to {}",
                    change_set.id, current.revision, change_set.revision
                ))
            }),
        )
        .await?
    } else {
        Vec::new()
    };
    let invalidated_pipeline_intent = if request.material_change && material_hash_changed {
        stale_pipeline_intent_for_change_set(
            &state.store,
            &change_set.id,
            actor.clone(),
            reason.clone().or_else(|| {
                Some(format!(
                    "change set {} revised from revision {} to {}",
                    change_set.id, current.revision, change_set.revision
                ))
            }),
        )
        .await?
    } else {
        None
    };
    let invalidated_deployment_intent = if let Some(intent) = &invalidated_pipeline_intent {
        stale_deployment_intent_for_pipeline_intent(
            &state.store,
            &intent.id,
            actor.clone(),
            reason.clone().or_else(|| {
                Some(format!(
                    "pipeline intent {} staled after change set {} revised",
                    intent.id, change_set.id
                ))
            }),
            "pipeline_intent_stale",
        )
        .await?
    } else {
        None
    };
    let invalidated_release = if let Some(intent) = &invalidated_deployment_intent {
        stale_release_for_deployment_intent(
            &state.store,
            &intent.id,
            actor.clone(),
            reason.clone().or_else(|| {
                Some(format!(
                    "deployment intent {} staled after change set {} revised",
                    intent.id, change_set.id
                ))
            }),
            "deployment_intent_stale",
        )
        .await?
    } else {
        None
    };
    let invalidated_registry_evidence = if let Some(release) = &invalidated_release {
        stale_registry_evidence_for_release(
            &state.store,
            &release.id,
            actor.clone(),
            reason.clone().or_else(|| {
                Some(format!(
                    "release {} staled after change set {} revised",
                    release.id, change_set.id
                ))
            }),
            "release_stale",
        )
        .await?
    } else {
        None
    };
    append_change_set_audit_event(
        &state.store,
        &change_set,
        "change_set.revised",
        actor,
        reason,
        json!({
            "previous_revision": current.revision,
            "revision": change_set.revision,
            "previous_material_hash": current.material_hash,
            "material_hash": change_set.material_hash,
            "material_hash_changed": material_hash_changed,
            "material_change": request.material_change,
            "invalidated_gate_ids": invalidated_gates
                .iter()
                .map(|gate| gate.id.clone())
                .collect::<Vec<_>>(),
            "invalidated_permission_grant_ids": invalidated_trusted_envelopes
                .iter()
                .map(|grant| grant.id.clone())
                .collect::<Vec<_>>(),
            "invalidated_pipeline_intent_id": invalidated_pipeline_intent
                .as_ref()
                .map(|intent| intent.id.clone()),
            "invalidated_deployment_intent_id": invalidated_deployment_intent
                .as_ref()
                .map(|intent| intent.id.clone()),
            "invalidated_release_id": invalidated_release
                .as_ref()
                .map(|release| release.id.clone()),
        }),
    )
    .await?;

    Ok(Json(ReviseChangeSetResponse {
        change_set: change_set.into(),
        material_hash_changed,
        invalidated_gates: invalidated_gates.into_iter().map(Into::into).collect(),
        invalidated_pipeline_intent: invalidated_pipeline_intent.map(Into::into),
        invalidated_deployment_intent: invalidated_deployment_intent.map(Into::into),
        invalidated_release: invalidated_release.map(Into::into),
        invalidated_registry_evidence: invalidated_registry_evidence.map(Into::into),
    }))
}

pub(in crate::app) async fn create_change_set_trusted_envelope(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
    Json(request): Json<CreateTrustedEnvelopeRequest>,
) -> Result<Json<TrustedEnvelopeResponse>, ApiError> {
    let change_set = state
        .store
        .get_change_set(&change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &change_set_id))?;
    let work_plan = state
        .store
        .get_work_plan(&change_set.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &change_set.work_plan_id))?;
    ensure_approved_for_trusted_envelope("work_plan", &work_plan.id, &work_plan.status)?;
    ensure_approved_for_trusted_envelope("change_set", &change_set.id, &change_set.status)?;
    let grant_request =
        trusted_envelope_grant_request(&change_set.work_plan_id, Some(&change_set.id), &request)?;
    let actor = clean_optional_text(request.created_by.clone());
    let reason = clean_optional_text(Some(request.reason.clone()));
    let grant = create_permission_grant_record(&state.store, grant_request).await?;
    append_change_set_audit_event(
        &state.store,
        &change_set,
        "change_set.trusted_envelope_created",
        actor,
        reason,
        json!({
            "permission_grant_id": grant.id,
            "work_plan_id": change_set.work_plan_id,
            "change_set_id": change_set.id,
        }),
    )
    .await?;

    Ok(Json(TrustedEnvelopeResponse {
        grant: grant.into(),
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChangeSetStatus {
    Draft,
    Proposed,
    Approved,
    Applied,
    Rejected,
    Stale,
}

impl ChangeSetStatus {
    fn parse(value: &str) -> Result<Self, ApiError> {
        match value {
            "draft" => Ok(Self::Draft),
            "proposed" => Ok(Self::Proposed),
            "approved" => Ok(Self::Approved),
            "applied" => Ok(Self::Applied),
            "rejected" => Ok(Self::Rejected),
            "stale" => Ok(Self::Stale),
            other => Err(ApiError::bad_request(format!(
                "unsupported change set status: {other}"
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Proposed => "proposed",
            Self::Approved => "approved",
            Self::Applied => "applied",
            Self::Rejected => "rejected",
            Self::Stale => "stale",
        }
    }

    fn ensure_can_transition_to(self, target: Self) -> Result<(), ApiError> {
        let allowed = match self {
            Self::Draft => matches!(target, Self::Proposed | Self::Rejected),
            Self::Proposed => matches!(target, Self::Approved | Self::Rejected | Self::Draft),
            Self::Approved => matches!(target, Self::Applied | Self::Rejected | Self::Draft),
            Self::Applied | Self::Rejected | Self::Stale => false,
        };
        if allowed {
            Ok(())
        } else {
            Err(ApiError::conflict(format!(
                "cannot transition change set from {} to {}",
                self.as_str(),
                target.as_str()
            )))
        }
    }
}
