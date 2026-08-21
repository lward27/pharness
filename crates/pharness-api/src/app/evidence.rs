use super::*;
use axum::routing::{get, post};
use axum::Router;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/artifacts/:artifact_id", get(get_artifact))
        .route(
            "/api/observations",
            get(list_observations).post(create_observation),
        )
        .route("/api/observations/:observation_id", get(get_observation))
        .route("/api/incidents", get(list_incidents).post(create_incident))
        .route("/api/incidents/:incident_id", get(get_incident))
        .route(
            "/api/remediation-plans",
            get(list_remediation_plans).post(create_remediation_plan),
        )
        .route("/api/remediation-plans/:plan_id", get(get_remediation_plan))
        .route(
            "/api/remediation-plans/:plan_id/transition",
            post(transition_remediation_plan),
        )
        .route("/api/audit-events", get(list_audit_events))
}

pub(super) async fn get_artifact(
    State(state): State<AppState>,
    Path(artifact_id): Path<String>,
) -> Result<Json<ArtifactResponse>, ApiError> {
    let artifact = state
        .store
        .get_artifact(&artifact_id)
        .await?
        .ok_or_else(|| ApiError::not_found("artifact", &artifact_id))?;

    Ok(Json(artifact.into()))
}

#[derive(Debug, Default, serde::Deserialize)]
pub(super) struct ListObservationsQuery {
    pub(super) run_id: Option<String>,
    pub(super) source: Option<String>,
    pub(super) kind: Option<String>,
    pub(super) subject: Option<String>,
    pub(super) resource_namespace: Option<String>,
    pub(super) resource_kind: Option<String>,
    pub(super) resource_name: Option<String>,
    pub(super) observed_after_ms: Option<i64>,
    pub(super) observed_before_ms: Option<i64>,
    pub(super) limit: Option<u32>,
    pub(super) offset: Option<u32>,
}

pub(super) async fn list_observations(
    State(state): State<AppState>,
    Query(query): Query<ListObservationsQuery>,
) -> Result<Json<ObservationsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let observations = state
        .store
        .list_observations(ObservationListFilter {
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            source: clean_optional_text(query.source),
            kind: clean_optional_text(query.kind),
            subject: clean_optional_text(query.subject),
            resource_namespace: clean_optional_text(query.resource_namespace),
            resource_kind: clean_optional_text(query.resource_kind),
            resource_name: clean_optional_text(query.resource_name),
            observed_after_ms: query.observed_after_ms,
            observed_before_ms: query.observed_before_ms,
            limit,
            offset,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<ObservationResponse>>();
    let count = observations.len();

    Ok(Json(ObservationsResponse {
        observations,
        count,
        limit: Some(limit),
        offset: Some(offset),
    }))
}

pub(super) async fn get_observation(
    State(state): State<AppState>,
    Path(observation_id): Path<String>,
) -> Result<Json<ObservationResponse>, ApiError> {
    let observation = state
        .store
        .get_observation(&observation_id)
        .await?
        .ok_or_else(|| ApiError::not_found("observation", &observation_id))?;

    Ok(Json(observation.into()))
}

pub(super) async fn create_observation(
    State(state): State<AppState>,
    Json(request): Json<CreateObservationRequest>,
) -> Result<Json<ObservationResponse>, ApiError> {
    let source = required_text(request.source, "source")?;
    let kind = required_text(request.kind, "kind")?;
    let subject = required_text(request.subject, "subject")?;
    let summary = required_text(request.summary, "summary")?;
    let data_json = request.data_json.unwrap_or_else(|| json!({}));
    ensure_json_object(&data_json, "data_json")?;
    if let Some(resource_ref) = &request.resource_ref {
        ensure_json_object(resource_ref, "resource_ref")?;
    }
    if let Some(artifact_id) = clean_optional_text(request.artifact_id.clone()) {
        state
            .store
            .get_artifact(&artifact_id)
            .await?
            .ok_or_else(|| ApiError::not_found("artifact", &artifact_id))?;
    }

    let (session_id, run_id) = root_session_for_request(
        &state.store,
        clean_optional_text(request.session_id),
        request.run_id,
        "control-plane observation",
    )
    .await?;
    let observation = state
        .store
        .create_observation(CreateObservation {
            id: clean_optional_text(request.id)
                .unwrap_or_else(|| format!("obs_{}", unique_suffix())),
            session_id,
            run_id,
            source,
            kind,
            subject,
            summary,
            resource_namespace: clean_optional_text(request.resource_namespace),
            resource_kind: clean_optional_text(request.resource_kind),
            resource_name: clean_optional_text(request.resource_name),
            resource_ref_json: request.resource_ref,
            artifact_id: clean_optional_text(request.artifact_id),
            data_json,
        })
        .await?;
    append_observation_audit_event(
        &state.store,
        &observation,
        "observation.created",
        clean_optional_text(request.actor),
        clean_optional_text(request.reason),
    )
    .await?;

    Ok(Json(observation.into()))
}

#[derive(Debug, Default, serde::Deserialize)]
pub(super) struct ListIncidentsQuery {
    pub(super) run_id: Option<String>,
    pub(super) status: Option<String>,
    pub(super) severity: Option<String>,
    pub(super) resource_namespace: Option<String>,
    pub(super) resource_kind: Option<String>,
    pub(super) resource_name: Option<String>,
    pub(super) created_after_ms: Option<i64>,
    pub(super) created_before_ms: Option<i64>,
    pub(super) limit: Option<u32>,
    pub(super) offset: Option<u32>,
}

pub(super) async fn list_incidents(
    State(state): State<AppState>,
    Query(query): Query<ListIncidentsQuery>,
) -> Result<Json<IncidentsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let incidents = state
        .store
        .list_incidents(IncidentListFilter {
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            status: clean_optional_text(query.status),
            severity: clean_optional_text(query.severity),
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
    let count = incidents.len();

    Ok(Json(IncidentsResponse {
        incidents,
        count,
        limit,
        offset,
    }))
}

pub(super) async fn get_incident(
    State(state): State<AppState>,
    Path(incident_id): Path<String>,
) -> Result<Json<IncidentResponse>, ApiError> {
    let incident = state
        .store
        .get_incident(&incident_id)
        .await?
        .ok_or_else(|| ApiError::not_found("incident", &incident_id))?;

    Ok(Json(incident.into()))
}

pub(super) async fn create_incident(
    State(state): State<AppState>,
    Json(request): Json<CreateIncidentRequest>,
) -> Result<Json<IncidentResponse>, ApiError> {
    let observation_id = required_text(request.observation_id, "observation_id")?;
    let observation = state
        .store
        .get_observation(&observation_id)
        .await?
        .ok_or_else(|| ApiError::not_found("observation", &observation_id))?;
    let status = clean_optional_text(request.status).unwrap_or_else(|| "candidate".to_string());
    validate_allowed_value(
        "status",
        &status,
        &[
            "candidate",
            "open",
            "investigating",
            "mitigated",
            "resolved",
            "dismissed",
        ],
    )?;
    let severity = required_text(request.severity, "severity")?;
    validate_allowed_value(
        "severity",
        &severity,
        &["info", "low", "medium", "high", "critical"],
    )?;
    let data_json = request.data_json.unwrap_or_else(|| json!({}));
    ensure_json_object(&data_json, "data_json")?;

    let incident = state
        .store
        .create_incident(CreateIncident {
            id: clean_optional_text(request.id)
                .unwrap_or_else(|| format!("inc_{}", unique_suffix())),
            observation_id: observation.id.clone(),
            session_id: observation.session_id.clone(),
            run_id: observation.run_id.clone(),
            status,
            severity,
            title: required_text(request.title, "title")?,
            summary: required_text(request.summary, "summary")?,
            resource_namespace: clean_optional_text(request.resource_namespace)
                .or(observation.resource_namespace),
            resource_kind: clean_optional_text(request.resource_kind).or(observation.resource_kind),
            resource_name: clean_optional_text(request.resource_name).or(observation.resource_name),
            data_json,
        })
        .await?;
    append_incident_audit_event(
        &state.store,
        &incident,
        "incident.created",
        clean_optional_text(request.actor),
        clean_optional_text(request.reason),
    )
    .await?;

    Ok(Json(incident.into()))
}

#[derive(Debug, Default, serde::Deserialize)]
pub(super) struct ListRemediationPlansQuery {
    pub(super) incident_id: Option<String>,
    pub(super) run_id: Option<String>,
    pub(super) status: Option<String>,
    pub(super) risk_level: Option<String>,
    pub(super) resource_namespace: Option<String>,
    pub(super) resource_kind: Option<String>,
    pub(super) resource_name: Option<String>,
    pub(super) created_after_ms: Option<i64>,
    pub(super) created_before_ms: Option<i64>,
    pub(super) limit: Option<u32>,
    pub(super) offset: Option<u32>,
}

pub(super) async fn list_remediation_plans(
    State(state): State<AppState>,
    Query(query): Query<ListRemediationPlansQuery>,
) -> Result<Json<RemediationPlansResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let remediation_plans = state
        .store
        .list_remediation_plans(RemediationPlanListFilter {
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
    let count = remediation_plans.len();

    Ok(Json(RemediationPlansResponse {
        remediation_plans,
        count,
        limit,
        offset,
    }))
}

pub(super) async fn get_remediation_plan(
    State(state): State<AppState>,
    Path(plan_id): Path<String>,
) -> Result<Json<RemediationPlanResponse>, ApiError> {
    let plan = state
        .store
        .get_remediation_plan(&plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("remediation_plan", &plan_id))?;

    Ok(Json(plan.into()))
}

pub(super) async fn create_remediation_plan(
    State(state): State<AppState>,
    Json(request): Json<CreateRemediationPlanRequest>,
) -> Result<Json<RemediationPlanResponse>, ApiError> {
    let incident_id = required_text(request.incident_id, "incident_id")?;
    let incident = state
        .store
        .get_incident(&incident_id)
        .await?
        .ok_or_else(|| ApiError::not_found("incident", &incident_id))?;
    let status = clean_optional_text(request.status).unwrap_or_else(|| "draft".to_string());
    validate_allowed_value(
        "status",
        &status,
        &[
            "draft",
            "proposed",
            "approved",
            "executing",
            "blocked",
            "completed",
            "rejected",
            "stale",
        ],
    )?;
    let risk_level = required_text(request.risk_level, "risk_level")?;
    validate_allowed_value(
        "risk_level",
        &risk_level,
        &["low", "medium", "high", "critical"],
    )?;
    let plan_json = request.plan_json.unwrap_or_else(|| json!({}));
    ensure_json_object(&plan_json, "plan_json")?;

    let plan = state
        .store
        .create_remediation_plan(CreateRemediationPlan {
            id: clean_optional_text(request.id)
                .unwrap_or_else(|| format!("rplan_{}", unique_suffix())),
            incident_id: incident.id.clone(),
            session_id: incident.session_id.clone(),
            run_id: incident.run_id.clone(),
            status,
            title: required_text(request.title, "title")?,
            summary: required_text(request.summary, "summary")?,
            risk_level,
            requires_approval: request.requires_approval.unwrap_or(true),
            resource_namespace: clean_optional_text(request.resource_namespace)
                .or(incident.resource_namespace),
            resource_kind: clean_optional_text(request.resource_kind).or(incident.resource_kind),
            resource_name: clean_optional_text(request.resource_name).or(incident.resource_name),
            plan_json,
        })
        .await?;
    append_remediation_plan_audit_event(
        &state.store,
        &plan,
        "remediation_plan.created",
        clean_optional_text(request.actor),
        clean_optional_text(request.reason),
    )
    .await?;
    for gate in approval_gates_from_remediation_plan(&plan) {
        let gate = state.store.create_approval_gate(gate).await?;
        append_approval_gate_audit_event(&state.store, &gate, "approval_gate.created", "created")
            .await?;
    }

    Ok(Json(plan.into()))
}

pub(super) async fn transition_remediation_plan(
    State(state): State<AppState>,
    Path(plan_id): Path<String>,
    Json(request): Json<TransitionRemediationPlanRequest>,
) -> Result<Json<TransitionRemediationPlanResponse>, ApiError> {
    let current = state
        .store
        .get_remediation_plan(&plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("remediation_plan", &plan_id))?;
    let target = RemediationPlanStatus::parse(&request.target_status)?;
    let current_status = RemediationPlanStatus::parse(&current.status)?;
    current_status.ensure_can_transition_to(target)?;
    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    if target == RemediationPlanStatus::Approved
        && current.requires_approval
        && (actor.is_none() || reason.is_none())
    {
        return Err(ApiError::bad_request(
            "approving a remediation plan requires actor and reason",
        ));
    }
    let remediation_plan = state
        .store
        .update_remediation_plan_status(&plan_id, target.as_str())
        .await?;
    append_remediation_plan_audit_event(
        &state.store,
        &remediation_plan,
        &format!("remediation_plan.{}", target.as_str()),
        actor,
        reason,
    )
    .await?;

    Ok(Json(TransitionRemediationPlanResponse {
        remediation_plan: remediation_plan.into(),
    }))
}

#[derive(Debug, Default, serde::Deserialize)]

pub(super) struct ListAuditEventsQuery {
    pub(super) kind: Option<String>,
    pub(super) actor: Option<String>,
    pub(super) origin: Option<String>,
    pub(super) resource_kind: Option<String>,
    pub(super) resource_id: Option<String>,
    pub(super) run_id: Option<String>,
    pub(super) namespace: Option<String>,
    pub(super) repo: Option<String>,
    pub(super) branch: Option<String>,
    pub(super) production_impacting: Option<bool>,
    pub(super) search: Option<String>,
    pub(super) limit: Option<u32>,
}

pub(super) async fn list_audit_events(
    State(state): State<AppState>,
    Query(query): Query<ListAuditEventsQuery>,
) -> Result<Json<AuditEventsResponse>, ApiError> {
    let events = state
        .store
        .query_audit_events(AuditEventListFilter {
            kind: clean_optional_text(query.kind),
            actor: clean_optional_text(query.actor),
            origin: clean_optional_text(query.origin),
            resource_kind: clean_optional_text(query.resource_kind),
            resource_id: clean_optional_text(query.resource_id),
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            namespace: clean_optional_text(query.namespace),
            repo: clean_optional_text(query.repo),
            branch: clean_optional_text(query.branch),
            production_impacting: query.production_impacting,
            search: clean_optional_text(query.search),
            limit: query.limit.unwrap_or(50),
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(AuditEventsResponse { events }))
}
