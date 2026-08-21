use super::approval_policy::approval_gate_uses_dedicated_lifecycle_action;
use super::clock::{current_millis, unique_suffix};
use super::gitops::deployment_evidence::observed_gitops_merge_for_deployment;
use super::operator::{
    all_approval_gates_for_operator_groups, all_approvals_for_operator_groups,
    group_operator_records, operator_resource_label,
};
use super::validation::clean_optional_text;
use super::{ApiError, AppState};
use crate::dto::{
    ApprovalDecision, ApprovalGateResponse, ApprovalGateSummaryResponse, ApprovalGatesResponse,
    ApprovalResponse, ApprovalSummaryResponse, ApprovalsResponse, BatchDecideApprovalGatesRequest,
    BatchDecideApprovalGatesResponse, CreatePermissionGrantRequest, CreateTrustedEnvelopeRequest,
    DecideApprovalGateRequest, DecideApprovalGateResponse, DecideApprovalResponse,
    PermissionGrantResponse, PermissionGrantsResponse, ReviewApprovalRequest,
    RevokePermissionGrantRequest,
};
use axum::extract::{Path, Query, State};
use axum::Json;
use pharness_core::{
    AgentEvent, EventId, EventKind, PermissionGrant, PermissionGrantPolicy, PermissionGrantScope,
    RunId,
};
use pharness_store::{
    ApprovalGateListFilter, ApprovalGateSummaryFilter, ApprovalListFilter, ApprovalSummaryFilter,
    CreateAuditEvent, CreatePermissionGrant, SqliteStore, StoreError, StoredApprovalGate,
    StoredPermissionGrant,
};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

const DEFAULT_POLICY_SUBJECT: &str = "agent:local-worker";
const DEFAULT_TRUSTED_ENVELOPE_ENVIRONMENT: &str = "local";

pub(in crate::app) fn ensure_approved_for_trusted_envelope(
    resource_kind: &str,
    resource_id: &str,
    status: &str,
) -> Result<(), ApiError> {
    if status == "approved" {
        return Ok(());
    }

    Err(ApiError::conflict(format!(
        "{resource_kind} {resource_id} must be approved before creating a trusted envelope"
    )))
}

pub(in crate::app) fn trusted_envelope_grant_request(
    work_plan_id: &str,
    change_set_id: Option<&str>,
    request: &CreateTrustedEnvelopeRequest,
) -> Result<CreatePermissionGrantRequest, ApiError> {
    let reason = clean_optional_text(Some(request.reason.clone()))
        .ok_or_else(|| ApiError::bad_request("trusted envelope reason is required"))?;
    let subject = clean_optional_text(request.subject.clone())
        .unwrap_or_else(|| DEFAULT_POLICY_SUBJECT.to_string());
    let environment = clean_optional_text(request.environment.clone())
        .unwrap_or_else(|| DEFAULT_TRUSTED_ENVELOPE_ENVIRONMENT.to_string());
    let mut scope = Map::new();
    scope.insert("environment".to_string(), json!(environment));
    scope.insert("capability_kinds".to_string(), json!(["filesystem"]));
    scope.insert("actions".to_string(), json!(["write_file", "patch_file"]));
    scope.insert("max_risk".to_string(), json!("medium"));
    scope.insert("work_plan_ids".to_string(), json!([work_plan_id]));
    if let Some(change_set_id) = change_set_id {
        scope.insert("change_set_ids".to_string(), json!([change_set_id]));
    }
    insert_optional_scope_array(&mut scope, "namespaces", request.namespace.clone());
    insert_optional_scope_array(&mut scope, "repos", request.repo.clone());
    insert_optional_scope_array(&mut scope, "branches", request.branch.clone());
    scope.insert(
        "production_impacting".to_string(),
        json!(request.production_impacting.unwrap_or(false)),
    );

    Ok(CreatePermissionGrantRequest {
        subject,
        created_by: clean_optional_text(request.created_by.clone()),
        reason,
        scope: Value::Object(scope),
        policy: json!({ "policy_mode": "trusted_writes" }),
        expires_at: request.expires_at.clone(),
    })
}

fn insert_optional_scope_array(scope: &mut Map<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = clean_optional_text(value) {
        scope.insert(key.to_string(), json!([value]));
    }
}
use axum::routing::{get, post};
use axum::Router;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/approval-gates", get(list_approval_gates))
        .route("/api/approval-gates/summary", get(approval_gate_summary))
        .route(
            "/api/approval-gates/batch-decide",
            post(batch_decide_approval_gates),
        )
        .route("/api/approval-gates/:gate_id", get(get_approval_gate))
        .route(
            "/api/approval-gates/:gate_id/satisfy",
            post(satisfy_approval_gate),
        )
        .route(
            "/api/approval-gates/:gate_id/waive",
            post(waive_approval_gate),
        )
        .route(
            "/api/approval-gates/:gate_id/reject",
            post(reject_approval_gate),
        )
        .route("/api/approvals", get(list_approvals))
        .route("/api/approvals/summary", get(approval_summary))
        .route("/api/approvals/:approval_id", get(get_approval))
        .route(
            "/api/approvals/:approval_id/approve",
            post(approve_approval),
        )
        .route("/api/approvals/:approval_id/deny", post(deny_approval))
        .route(
            "/api/permission-grants",
            get(list_permission_grants).post(create_permission_grant),
        )
        .route(
            "/api/permission-grants/:grant_id",
            get(get_permission_grant),
        )
        .route(
            "/api/permission-grants/:grant_id/revoke",
            post(revoke_permission_grant),
        )
}

pub(super) async fn active_permission_grants(
    store: &SqliteStore,
) -> Result<Vec<PermissionGrant>, ApiError> {
    let now = current_millis();
    let grants = store
        .list_permission_grants(Some("active"), 200)
        .await?
        .into_iter()
        .filter(|grant| grant_is_unexpired(grant, now))
        .map(permission_grant_snapshot)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(grants)
}

fn permission_grant_snapshot(grant: StoredPermissionGrant) -> Result<PermissionGrant, ApiError> {
    let scope =
        serde_json::from_value::<PermissionGrantScope>(grant.scope_json).map_err(|error| {
            ApiError::internal(format!(
                "permission grant {} has invalid scope: {error}",
                grant.id
            ))
        })?;
    let policy =
        serde_json::from_value::<PermissionGrantPolicy>(grant.policy_json).map_err(|error| {
            ApiError::internal(format!(
                "permission grant {} has invalid policy: {error}",
                grant.id
            ))
        })?;

    Ok(PermissionGrant {
        id: grant.id,
        subject: grant.subject,
        scope,
        policy,
        expires_at: grant.expires_at,
    })
}

pub(super) fn grant_is_unexpired(grant: &StoredPermissionGrant, now_millis: u128) -> bool {
    grant
        .expires_at
        .as_deref()
        .map(|expires_at| {
            expires_at
                .parse::<u128>()
                .map(|expires_at| expires_at > now_millis)
                .unwrap_or(false)
        })
        .unwrap_or(true)
}

#[derive(Debug, Default, serde::Deserialize)]
pub(super) struct ListApprovalGatesQuery {
    pub(super) search: Option<String>,
    pub(super) work_item_id: Option<String>,
    pub(super) remediation_plan_id: Option<String>,
    pub(super) incident_id: Option<String>,
    pub(super) run_id: Option<String>,
    pub(super) status: Option<String>,
    pub(super) origin: Option<String>,
    pub(super) actor: Option<String>,
    pub(super) gate_kind: Option<String>,
    pub(super) risk_level: Option<String>,
    pub(super) resource_namespace: Option<String>,
    pub(super) resource_kind: Option<String>,
    pub(super) resource_name: Option<String>,
    pub(super) created_after_ms: Option<i64>,
    pub(super) created_before_ms: Option<i64>,
    pub(super) limit: Option<u32>,
    pub(super) offset: Option<u32>,
}

#[derive(Debug, Default, serde::Deserialize)]
pub(super) struct ApprovalGateSummaryQuery {
    pub(super) work_item_id: Option<String>,
    pub(super) remediation_plan_id: Option<String>,
    pub(super) incident_id: Option<String>,
    pub(super) run_id: Option<String>,
    pub(super) status: Option<String>,
    pub(super) gate_kind: Option<String>,
    pub(super) risk_level: Option<String>,
    pub(super) resource_namespace: Option<String>,
    pub(super) resource_kind: Option<String>,
    pub(super) resource_name: Option<String>,
    pub(super) created_after_ms: Option<i64>,
    pub(super) created_before_ms: Option<i64>,
}

pub(super) async fn list_approval_gates(
    State(state): State<AppState>,
    Query(query): Query<ListApprovalGatesQuery>,
) -> Result<Json<ApprovalGatesResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let filter = ApprovalGateListFilter {
        search: clean_optional_text(query.search),
        work_item_id: clean_optional_text(query.work_item_id),
        remediation_plan_id: clean_optional_text(query.remediation_plan_id),
        incident_id: clean_optional_text(query.incident_id),
        run_id: clean_optional_text(query.run_id).map(RunId::new),
        status: clean_optional_text(query.status),
        origin: clean_optional_text(query.origin),
        created_by: clean_optional_text(query.actor),
        gate_kind: clean_optional_text(query.gate_kind),
        risk_level: clean_optional_text(query.risk_level),
        resource_namespace: clean_optional_text(query.resource_namespace),
        resource_kind: clean_optional_text(query.resource_kind),
        resource_name: clean_optional_text(query.resource_name),
        created_after_ms: query.created_after_ms,
        created_before_ms: query.created_before_ms,
        limit,
        offset,
    };
    let stored_approval_gates = state.store.list_approval_gates(filter.clone()).await?;
    let mut approval_gates = Vec::with_capacity(stored_approval_gates.len());
    for gate in stored_approval_gates {
        approval_gates.push(approval_gate_response(&state, gate).await?);
    }
    let group_approval_gates =
        all_approval_gates_for_operator_groups(state.store.as_ref(), filter).await?;
    let count = group_approval_gates.len();
    let groups = group_operator_records(group_approval_gates.iter().map(|gate| {
        (
            gate.id.clone(),
            gate.created_at.clone(),
            gate.title.clone(),
            operator_resource_label(
                gate.resource_namespace.as_deref(),
                gate.resource_kind.as_deref(),
                gate.resource_name.as_deref(),
            ),
            gate.status.clone(),
        )
    }));

    Ok(Json(ApprovalGatesResponse {
        approval_gates,
        groups,
        count,
        limit,
        offset,
    }))
}

pub(super) async fn approval_gate_summary(
    State(state): State<AppState>,
    Query(query): Query<ApprovalGateSummaryQuery>,
) -> Result<Json<ApprovalGateSummaryResponse>, ApiError> {
    let summary = state
        .store
        .approval_gate_summary(ApprovalGateSummaryFilter {
            work_item_id: clean_optional_text(query.work_item_id),
            remediation_plan_id: clean_optional_text(query.remediation_plan_id),
            incident_id: clean_optional_text(query.incident_id),
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            status: clean_optional_text(query.status),
            gate_kind: clean_optional_text(query.gate_kind),
            risk_level: clean_optional_text(query.risk_level),
            resource_namespace: clean_optional_text(query.resource_namespace),
            resource_kind: clean_optional_text(query.resource_kind),
            resource_name: clean_optional_text(query.resource_name),
            created_after_ms: query.created_after_ms,
            created_before_ms: query.created_before_ms,
        })
        .await?;

    Ok(Json(ApprovalGateSummaryResponse { summary }))
}

pub(super) async fn get_approval_gate(
    State(state): State<AppState>,
    Path(gate_id): Path<String>,
) -> Result<Json<ApprovalGateResponse>, ApiError> {
    let gate = state
        .store
        .get_approval_gate(&gate_id)
        .await?
        .ok_or_else(|| ApiError::not_found("approval_gate", &gate_id))?;

    Ok(Json(approval_gate_response(&state, gate).await?))
}

pub(super) fn approval_gate_lifecycle_stage(gate_kind: &str) -> &'static str {
    match gate_kind {
        "source_mutation" | "git_mutation" => "source",
        "pipeline_mutation" | "production_impact" => "pipeline",
        "gitops_mutation" => "gitops",
        "cluster_mutation" | "production_deployment" => "deployment",
        _ => "planning",
    }
}

pub(super) async fn approval_gate_lifecycle_readiness(
    state: &AppState,
    gate: &StoredApprovalGate,
) -> Result<(bool, String), ApiError> {
    if approval_gate_uses_dedicated_lifecycle_action(&gate.gate_kind) {
        return Ok((
            false,
            "Use the digest-bound RollbackIntent approval action so gate satisfaction and the expiring writer grant are committed together."
                .to_string(),
        ));
    }
    let Some(work_item_id) = gate.work_item_id.as_deref() else {
        return Ok((
            true,
            "The gate is at its declared review boundary.".to_string(),
        ));
    };
    let item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    if !item.production_impacting {
        return Ok((
            true,
            "The development gate is ready for review.".to_string(),
        ));
    }
    let Some(plan) = state.store.get_work_plan_by_work_item(work_item_id).await? else {
        return Ok((false, "A WorkPlan has not been created yet.".to_string()));
    };
    if plan.status != "approved" {
        return Ok((
            false,
            "The proposed WorkPlan must be approved before production gates become actionable."
                .to_string(),
        ));
    }
    let change_set = state.store.get_change_set_by_work_plan(&plan.id).await?;
    if matches!(gate.gate_kind.as_str(), "source_mutation" | "git_mutation") {
        return Ok(match change_set {
            Some(change_set) if change_set.status == "approved" => (
                true,
                "The approved ChangeSet is at the source-delivery boundary.".to_string(),
            ),
            _ => (
                false,
                "An approved ChangeSet is required before source delivery gates can be decided."
                    .to_string(),
            ),
        });
    }
    let Some(change_set) = change_set else {
        return Ok((
            false,
            "Source delivery has not produced an approved ChangeSet.".to_string(),
        ));
    };
    let pipeline_intent = state
        .store
        .get_pipeline_intent_by_change_set(&change_set.id)
        .await?;
    if matches!(
        gate.gate_kind.as_str(),
        "pipeline_mutation" | "production_impact"
    ) {
        return Ok(match pipeline_intent {
            Some(intent) if intent.status == "approved" => (
                true,
                "The approved PipelineIntent is at the immutable build boundary.".to_string(),
            ),
            _ => (
                false,
                "An approved PipelineIntent is required before pipeline production gates can be decided."
                    .to_string(),
            ),
        });
    }
    let Some(pipeline_intent) = pipeline_intent else {
        return Ok((
            false,
            "The immutable build boundary has not been reached.".to_string(),
        ));
    };
    let gitops_change_set = state
        .store
        .get_gitops_change_set_by_pipeline_intent(&pipeline_intent.id)
        .await?;
    if gate.gate_kind == "gitops_mutation" {
        return Ok(match gitops_change_set {
            Some(change_set) if change_set.status == "approved" => (
                true,
                "The approved GitOps ChangeSet is at the GitOps writer boundary.".to_string(),
            ),
            _ => (
                false,
                "An approved GitOps ChangeSet is required before the GitOps mutation gate can be decided."
                    .to_string(),
            ),
        });
    }
    let deployment_intent = state
        .store
        .get_deployment_intent_by_pipeline_intent(&pipeline_intent.id)
        .await?;
    if !deployment_intent
        .as_ref()
        .is_some_and(|intent| intent.status == "approved")
    {
        return Ok((
            false,
            "An approved DeploymentIntent is required before cluster production gates can be decided."
                .to_string(),
        ));
    }
    let merge_ready = observed_gitops_merge_for_deployment(&state.store, &item, &pipeline_intent)
        .await
        .ok()
        .flatten()
        .is_some();
    if !merge_ready {
        return Ok((
            false,
            "The exact GitOps pull request must be observed merged before cluster production gates can be decided."
                .to_string(),
        ));
    }
    Ok((
        true,
        "The approved DeploymentIntent and immutable GitOps merge are at the explicit Argo boundary."
            .to_string(),
    ))
}

async fn approval_gate_response(
    state: &AppState,
    gate: StoredApprovalGate,
) -> Result<ApprovalGateResponse, ApiError> {
    let mut response: ApprovalGateResponse = gate.clone().into();
    if gate.status == "pending" {
        let (actionable, reason) = approval_gate_lifecycle_readiness(state, &gate).await?;
        response.actionable = actionable;
        if !actionable {
            response.lifecycle_blocker = Some(reason);
        }
    }
    Ok(response)
}

pub(super) async fn satisfy_approval_gate(
    State(state): State<AppState>,
    Path(gate_id): Path<String>,
    Json(request): Json<DecideApprovalGateRequest>,
) -> Result<Json<DecideApprovalGateResponse>, ApiError> {
    decide_approval_gate(state, gate_id, "satisfied", request).await
}

async fn waive_approval_gate(
    State(state): State<AppState>,
    Path(gate_id): Path<String>,
    Json(request): Json<DecideApprovalGateRequest>,
) -> Result<Json<DecideApprovalGateResponse>, ApiError> {
    decide_approval_gate(state, gate_id, "waived", request).await
}

async fn reject_approval_gate(
    State(state): State<AppState>,
    Path(gate_id): Path<String>,
    Json(request): Json<DecideApprovalGateRequest>,
) -> Result<Json<DecideApprovalGateResponse>, ApiError> {
    decide_approval_gate(state, gate_id, "rejected", request).await
}

pub(super) async fn decide_approval_gate(
    state: AppState,
    gate_id: String,
    status: &str,
    request: DecideApprovalGateRequest,
) -> Result<Json<DecideApprovalGateResponse>, ApiError> {
    let current = state
        .store
        .get_approval_gate(&gate_id)
        .await?
        .ok_or_else(|| ApiError::not_found("approval_gate", &gate_id))?;
    if current.status != "pending" {
        return Err(ApiError::conflict("approval gate is not pending"));
    }
    let (eligible, blocker) = approval_gate_lifecycle_readiness(&state, &current).await?;
    if !eligible {
        return Err(ApiError::conflict(blocker));
    }

    let gate = state
        .store
        .decide_approval_gate(
            &gate_id,
            status,
            clean_optional_text(request.decided_by.clone()),
            clean_optional_text(request.reason.clone()),
        )
        .await?;
    append_approval_gate_audit_event(
        &state.store,
        &gate,
        &format!("approval_gate.{status}"),
        status,
    )
    .await?;

    Ok(Json(DecideApprovalGateResponse {
        approval_gate: gate.into(),
    }))
}

pub(super) async fn batch_decide_approval_gates(
    State(state): State<AppState>,
    Json(request): Json<BatchDecideApprovalGatesRequest>,
) -> Result<Json<BatchDecideApprovalGatesResponse>, ApiError> {
    if request.gate_ids.is_empty() || request.gate_ids.len() > 100 {
        return Err(ApiError::bad_request(
            "batch approval gate decisions require between 1 and 100 gate IDs",
        ));
    }
    let decision = request.decision.trim();
    if !matches!(decision, "satisfied" | "waived" | "rejected") {
        return Err(ApiError::bad_request(
            "batch approval gate decision must be satisfied, waived, or rejected",
        ));
    }
    let decided_by = request.decided_by.trim();
    let reason = request.reason.trim();
    if decided_by.is_empty() || reason.is_empty() {
        return Err(ApiError::bad_request(
            "batch approval gate decisions require an actor and reason",
        ));
    }
    let gate_ids = request
        .gate_ids
        .into_iter()
        .map(|gate_id| gate_id.trim().to_string())
        .collect::<Vec<_>>();
    if gate_ids.iter().any(String::is_empty)
        || gate_ids.iter().collect::<BTreeSet<_>>().len() != gate_ids.len()
    {
        return Err(ApiError::bad_request(
            "batch approval gate IDs must be non-empty and unique",
        ));
    }

    let mut current_gates = Vec::with_capacity(gate_ids.len());
    for gate_id in &gate_ids {
        let gate = state
            .store
            .get_approval_gate(gate_id)
            .await?
            .ok_or_else(|| ApiError::not_found("approval_gate", gate_id))?;
        if gate.status != "pending" {
            return Err(ApiError::conflict(format!(
                "approval gate is not pending: {gate_id}"
            )));
        }
        let (eligible, blocker) = approval_gate_lifecycle_readiness(&state, &gate).await?;
        if !eligible {
            return Err(ApiError::conflict(format!(
                "approval gate {gate_id} is not at its lifecycle boundary: {blocker}"
            )));
        }
        current_gates.push(gate);
    }

    let batch_audit_event_id = format!("aud_approval_gate_batch_{}", unique_suffix());
    let mut audit_events = current_gates
        .iter()
        .map(|gate| {
            let mut decided_gate = gate.clone();
            decided_gate.status = decision.to_string();
            decided_gate.decided_by = Some(decided_by.to_string());
            decided_gate.decision_reason = Some(reason.to_string());
            approval_gate_audit_event(
                &decided_gate,
                &format!("approval_gate.{decision}"),
                decision,
            )
        })
        .collect::<Vec<_>>();
    audit_events.push(CreateAuditEvent {
        id: batch_audit_event_id.clone(),
        kind: "approval_gate.batch_decided".to_string(),
        actor: Some(decided_by.to_string()),
        resource_kind: "approval_gate_batch".to_string(),
        resource_id: batch_audit_event_id.clone(),
        run_id: None,
        payload_json: json!({
            "approval_gate_ids": gate_ids,
            "decision": decision,
            "decided_by": decided_by,
            "reason": reason,
            "count": current_gates.len(),
        }),
    });
    let decided = state
        .store
        .decide_pending_approval_gates(
            &gate_ids,
            decision,
            Some(decided_by.to_string()),
            Some(reason.to_string()),
            audit_events,
        )
        .await?;

    Ok(Json(BatchDecideApprovalGatesResponse {
        approval_gates: decided.into_iter().map(Into::into).collect(),
        batch_audit_event_id,
    }))
}

#[derive(Debug, Default, serde::Deserialize)]
pub(super) struct ListApprovalsQuery {
    pub(super) search: Option<String>,
    pub(super) status: Option<String>,
    pub(super) origin: Option<String>,
    pub(super) actor: Option<String>,
    pub(super) namespace: Option<String>,
    pub(super) repo: Option<String>,
    pub(super) branch: Option<String>,
    pub(super) production_impacting: Option<bool>,
    pub(super) requested_after_ms: Option<i64>,
    pub(super) requested_before_ms: Option<i64>,
    pub(super) limit: Option<u32>,
    pub(super) offset: Option<u32>,
}

pub(super) async fn list_approvals(
    State(state): State<AppState>,
    Query(query): Query<ListApprovalsQuery>,
) -> Result<Json<ApprovalsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let filter = ApprovalListFilter {
        search: clean_optional_text(query.search),
        status: clean_optional_text(query.status),
        origin: clean_optional_text(query.origin),
        created_by: clean_optional_text(query.actor),
        namespace: clean_optional_text(query.namespace),
        repo: clean_optional_text(query.repo),
        branch: clean_optional_text(query.branch),
        production_impacting: query.production_impacting,
        requested_after_ms: query.requested_after_ms,
        requested_before_ms: query.requested_before_ms,
        limit,
        offset,
    };
    let approvals = state
        .store
        .list_approvals(filter.clone())
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<ApprovalResponse>>();
    let group_approvals = all_approvals_for_operator_groups(state.store.as_ref(), filter).await?;
    let count = group_approvals.len();
    let groups = group_operator_records(group_approvals.iter().map(|approval| {
        let resource = approval
            .scope
            .as_ref()
            .and_then(|scope| scope.repo.as_deref())
            .unwrap_or("unscoped")
            .to_string();
        (
            approval.id.clone(),
            approval.requested_at.clone(),
            approval.kind.clone(),
            resource,
            approval.status.clone(),
        )
    }));
    Ok(Json(ApprovalsResponse {
        approvals,
        groups,
        count,
        limit,
        offset,
    }))
}

pub(super) async fn approval_summary(
    State(state): State<AppState>,
    Query(query): Query<ApprovalSummaryQuery>,
) -> Result<Json<ApprovalSummaryResponse>, ApiError> {
    let summary = state
        .store
        .approval_summary(ApprovalSummaryFilter {
            status: clean_optional_text(query.status),
            namespace: clean_optional_text(query.namespace),
            repo: clean_optional_text(query.repo),
            branch: clean_optional_text(query.branch),
            production_impacting: query.production_impacting,
            requested_after_ms: query.requested_after_ms,
            requested_before_ms: query.requested_before_ms,
        })
        .await?;

    Ok(Json(ApprovalSummaryResponse { summary }))
}

pub(super) async fn get_approval(
    State(state): State<AppState>,
    Path(approval_id): Path<String>,
) -> Result<Json<crate::dto::ApprovalResponse>, ApiError> {
    let approval = state
        .store
        .get_approval(&approval_id)
        .await?
        .ok_or_else(|| ApiError::not_found("approval", &approval_id))?;

    Ok(Json(approval.into()))
}

#[derive(Debug, Default, serde::Deserialize)]
pub(super) struct ApprovalSummaryQuery {
    pub(super) status: Option<String>,
    pub(super) namespace: Option<String>,
    pub(super) repo: Option<String>,
    pub(super) branch: Option<String>,
    pub(super) production_impacting: Option<bool>,
    pub(super) requested_after_ms: Option<i64>,
    pub(super) requested_before_ms: Option<i64>,
}

#[derive(Debug, Default, serde::Deserialize)]
pub(super) struct ListPermissionGrantsQuery {
    pub(super) status: Option<String>,
    pub(super) limit: Option<u32>,
}

pub(super) async fn list_permission_grants(
    State(state): State<AppState>,
    Query(query): Query<ListPermissionGrantsQuery>,
) -> Result<Json<PermissionGrantsResponse>, ApiError> {
    let grants = state
        .store
        .list_permission_grants(query.status.as_deref(), query.limit.unwrap_or(50))
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(PermissionGrantsResponse { grants }))
}

pub(super) async fn get_permission_grant(
    State(state): State<AppState>,
    Path(grant_id): Path<String>,
) -> Result<Json<PermissionGrantResponse>, ApiError> {
    let grant = state
        .store
        .get_permission_grant(&grant_id)
        .await?
        .ok_or_else(|| ApiError::not_found("permission grant", &grant_id))?;

    Ok(Json(grant.into()))
}

pub(super) async fn create_permission_grant(
    State(state): State<AppState>,
    Json(request): Json<CreatePermissionGrantRequest>,
) -> Result<Json<PermissionGrantResponse>, ApiError> {
    let grant = create_permission_grant_record(&state.store, request).await?;

    Ok(Json(grant.into()))
}

pub(super) async fn create_permission_grant_record(
    store: &SqliteStore,
    request: CreatePermissionGrantRequest,
) -> Result<StoredPermissionGrant, ApiError> {
    validate_permission_grant_request(&request)?;
    let created_by = clean_optional_text(request.created_by.clone());
    let grant = store
        .create_permission_grant(CreatePermissionGrant {
            id: format!("pgrant_{}", unique_suffix()),
            subject: request.subject,
            reason: request.reason,
            scope_json: request.scope,
            policy_json: request.policy,
            expires_at: request.expires_at,
        })
        .await?;
    append_permission_grant_audit_event(store, "permission_grant.created", &grant, created_by)
        .await?;

    Ok(grant)
}

pub(super) async fn revoke_permission_grant(
    State(state): State<AppState>,
    Path(grant_id): Path<String>,
    Json(request): Json<RevokePermissionGrantRequest>,
) -> Result<Json<PermissionGrantResponse>, ApiError> {
    let grant = state
        .store
        .revoke_permission_grant(&grant_id, request.revoked_by.clone(), request.reason)
        .await?;
    append_permission_grant_audit_event(
        &state.store,
        "permission_grant.revoked",
        &grant,
        request.revoked_by,
    )
    .await?;

    Ok(Json(grant.into()))
}

pub(super) async fn append_permission_grant_audit_event(
    store: &SqliteStore,
    kind: &str,
    grant: &StoredPermissionGrant,
    actor: Option<String>,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", grant.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.or_else(|| Some("api".to_string())),
            resource_kind: "permission_grant".to_string(),
            resource_id: grant.id.clone(),
            run_id: None,
            payload_json: json!({
                "grant_id": grant.id,
                "subject": grant.subject,
                "status": grant.status,
                "reason": grant.reason,
                "scope": grant.scope_json,
                "policy": grant.policy_json,
                "expires_at": grant.expires_at,
                "revoked_at": grant.revoked_at,
                "revoked_by": grant.revoked_by,
                "revoke_reason": grant.revoke_reason,
            }),
        })
        .await
        .map(|_| ())
}

pub(super) async fn append_approval_gate_audit_event(
    store: &SqliteStore,
    gate: &StoredApprovalGate,
    kind: &str,
    decision: &str,
) -> Result<(), StoreError> {
    store
        .create_audit_event(approval_gate_audit_event(gate, kind, decision))
        .await
        .map(|_| ())
}

fn approval_gate_audit_event(
    gate: &StoredApprovalGate,
    kind: &str,
    decision: &str,
) -> CreateAuditEvent {
    CreateAuditEvent {
        id: format!("aud_{}_{}", gate.id, unique_suffix()),
        kind: kind.to_string(),
        actor: gate
            .stale_by
            .clone()
            .or_else(|| gate.decided_by.clone())
            .or_else(|| Some("api".to_string())),
        resource_kind: "approval_gate".to_string(),
        resource_id: gate.id.clone(),
        run_id: gate.run_id.clone(),
        payload_json: json!({
            "approval_gate_id": gate.id,
            "remediation_plan_id": gate.remediation_plan_id,
            "incident_id": gate.incident_id,
            "run_id": gate.run_id.as_ref().map(RunId::as_str),
            "status": gate.status,
            "decision": decision,
            "gate_kind": gate.gate_kind,
            "gate_order": gate.gate_order,
            "risk_level": gate.risk_level,
            "summary": gate.summary,
            "resource": {
                "namespace": gate.resource_namespace,
                "kind": gate.resource_kind,
                "name": gate.resource_name,
            },
            "decided_at": gate.decided_at,
            "decided_by": gate.decided_by,
            "reason": gate.decision_reason,
            "stale_at": gate.stale_at,
            "stale_by": gate.stale_by,
            "stale_reason": gate.stale_reason,
        }),
    }
}

pub(super) fn validate_permission_grant_request(
    request: &CreatePermissionGrantRequest,
) -> Result<(), ApiError> {
    if request.subject.trim().is_empty() {
        return Err(ApiError::bad_request(
            "permission grant subject is required",
        ));
    }
    if request.reason.trim().is_empty() {
        return Err(ApiError::bad_request("permission grant reason is required"));
    }
    if request
        .created_by
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(ApiError::bad_request(
            "permission grant created_by cannot be blank",
        ));
    }
    if !request.scope.is_object() {
        return Err(ApiError::bad_request(
            "permission grant scope must be a JSON object",
        ));
    }
    if !request.policy.is_object() {
        return Err(ApiError::bad_request(
            "permission grant policy must be a JSON object",
        ));
    }
    let scope =
        serde_json::from_value::<PermissionGrantScope>(request.scope.clone()).map_err(|error| {
            ApiError::bad_request(format!("permission grant scope is invalid: {error}"))
        })?;
    if scope
        .environment
        .as_deref()
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        return Err(ApiError::bad_request(
            "permission grant scope.environment is required",
        ));
    }
    serde_json::from_value::<PermissionGrantPolicy>(request.policy.clone()).map_err(|error| {
        ApiError::bad_request(format!("permission grant policy is invalid: {error}"))
    })?;
    if let Some(expires_at) = &request.expires_at {
        expires_at.parse::<u128>().map_err(|_| {
            ApiError::bad_request("permission grant expires_at must be unix milliseconds")
        })?;
    }

    Ok(())
}

async fn approve_approval(
    State(state): State<AppState>,
    Path(approval_id): Path<String>,
    Json(request): Json<ReviewApprovalRequest>,
) -> Result<Json<DecideApprovalResponse>, ApiError> {
    decide_approval_by_id(
        state,
        approval_id,
        ApprovalDecisionInput {
            decision: ApprovalDecision::Approve,
            decided_by: request.decided_by,
            reason: request.reason,
        },
    )
    .await
}

pub(super) async fn deny_approval(
    State(state): State<AppState>,
    Path(approval_id): Path<String>,
    Json(request): Json<ReviewApprovalRequest>,
) -> Result<Json<DecideApprovalResponse>, ApiError> {
    decide_approval_by_id(
        state,
        approval_id,
        ApprovalDecisionInput {
            decision: ApprovalDecision::Deny,
            decided_by: request.decided_by,
            reason: request.reason,
        },
    )
    .await
}

async fn decide_approval_by_id(
    state: AppState,
    approval_id: String,
    input: ApprovalDecisionInput,
) -> Result<Json<DecideApprovalResponse>, ApiError> {
    let approval = state
        .store
        .get_approval(&approval_id)
        .await?
        .ok_or_else(|| ApiError::not_found("approval", &approval_id))?;
    if approval.status != "pending" {
        return Err(ApiError::conflict("approval is not pending"));
    }

    let run_id = approval.run_id.clone();
    decide_current_run_approval(state, run_id, input, Some(approval_id.as_str())).await
}

pub(super) struct ApprovalDecisionInput {
    pub(super) decision: ApprovalDecision,
    pub(super) decided_by: Option<String>,
    pub(super) reason: Option<String>,
}

pub(super) async fn decide_current_run_approval(
    state: AppState,
    run_id: RunId,
    input: ApprovalDecisionInput,
    expected_approval_id: Option<&str>,
) -> Result<Json<DecideApprovalResponse>, ApiError> {
    let pending = state
        .store
        .pending_approval_for_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::conflict("run has no pending approval"))?;
    if let Some(expected_approval_id) = expected_approval_id {
        if pending.id != expected_approval_id {
            return Err(ApiError::conflict(
                "approval is not the current pending approval for its run",
            ));
        }
    }

    let decided_by = input.decided_by;
    let reason = input.reason;

    match input.decision {
        ApprovalDecision::Deny => {
            let approval = state
                .store
                .decide_pending_approval(&run_id, "denied", decided_by.clone(), reason.clone())
                .await?;
            append_approval_decided_event(&state.store, &approval, "denied").await?;
            append_approval_decision_audit_event(
                &state.store,
                &approval,
                "approval.denied",
                "denied",
                decided_by,
                reason,
            )
            .await?;
            let run = state
                .store
                .complete_run(
                    &run_id,
                    "failed",
                    json!({
                        "status": "failed",
                        "turns": approval.turns_completed,
                        "summary": approval.summary,
                        "error": "approval denied",
                        "approval_id": approval.id,
                        "run_scope": approval.run_scope_json,
                    }),
                    Some("approval denied".to_string()),
                )
                .await?;

            Ok(Json(DecideApprovalResponse {
                approval: approval.into(),
                run: run.into(),
            }))
        }
        ApprovalDecision::Approve => {
            if pending.action_json.is_none() {
                return Err(ApiError::conflict(
                    "pending approval has no reviewed action to resume",
                ));
            }
            if !state.worker.enabled() {
                return Err(ApiError::conflict(
                    "cannot approve without an enabled run worker",
                ));
            }

            let approval = state
                .store
                .decide_pending_approval(&run_id, "approved", decided_by.clone(), reason.clone())
                .await?;
            append_approval_decided_event(&state.store, &approval, "approved").await?;
            append_approval_decision_audit_event(
                &state.store,
                &approval,
                "approval.approved",
                "approved",
                decided_by,
                reason,
            )
            .await?;
            let run = state.store.mark_run_running(&run_id).await?;
            state.worker.resume_run(run.clone(), approval.clone());

            Ok(Json(DecideApprovalResponse {
                approval: approval.into(),
                run: run.into(),
            }))
        }
    }
}

async fn append_approval_decision_audit_event(
    store: &SqliteStore,
    approval: &pharness_store::StoredApproval,
    kind: &str,
    decision: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<(), StoreError> {
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", approval.id, unique_suffix()),
            kind: kind.to_string(),
            actor: actor.clone().or_else(|| Some("api".to_string())),
            resource_kind: "approval".to_string(),
            resource_id: approval.id.clone(),
            run_id: Some(approval.run_id.clone()),
            payload_json: json!({
                "approval_id": approval.id,
                "run_id": approval.run_id.as_str(),
                "decision": decision,
                "kind": approval.kind,
                "summary": approval.summary,
                "risk_level": approval.risk_level,
                "turns_completed": approval.turns_completed,
                "action": approval_action_kind(approval),
                "run_scope": approval.run_scope_json,
                "decided_by": actor,
                "reason": reason,
            }),
        })
        .await
        .map(|_| ())
}

fn approval_action_kind(approval: &pharness_store::StoredApproval) -> Option<&str> {
    approval
        .action_json
        .as_ref()
        .and_then(|action| action.get("action"))
        .and_then(serde_json::Value::as_str)
}

async fn append_approval_decided_event(
    store: &SqliteStore,
    approval: &pharness_store::StoredApproval,
    decision: &str,
) -> Result<(), StoreError> {
    let seq = store.list_events(&approval.run_id).await?.len() as u64 + 1;
    store
        .append_event(&AgentEvent {
            event_id: EventId::new(format!("evt_{}_{}", approval.run_id.as_str(), seq)),
            session_id: approval.session_id.clone(),
            run_id: approval.run_id.clone(),
            seq,
            kind: EventKind::ApprovalDecided,
            payload: json!({
                "approval_id": approval.id,
                "decision": decision,
                "kind": approval.kind,
                "run_scope": approval.run_scope_json,
            }),
        })
        .await
}
