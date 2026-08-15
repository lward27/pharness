use super::*;

pub(super) fn internal_router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/internal/runs/:run_id/attempt-context",
            get(internal_attempt_context),
        )
        .route(
            "/api/internal/runs/:run_id/mark-running",
            post(internal_mark_running),
        )
        .route(
            "/api/internal/runs/:run_id/workspace-provisioned",
            post(internal_workspace_provisioned),
        )
        .route(
            "/api/internal/runs/:run_id/events",
            post(internal_ingest_events),
        )
        .route(
            "/api/internal/runs/:run_id/outcome",
            post(internal_ingest_outcome),
        )
        .route(
            "/api/internal/runs/:run_id/control",
            get(internal_run_control),
        )
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/runs", get(list_runs).post(create_operator_run))
        .route("/api/runs/summary", get(run_summary))
        .route("/api/runs/:run_id", get(get_run))
        .route("/api/runs/:run_id/events", get(get_run_events))
        .route("/api/runs/:run_id/events/stream", get(stream_run_events))
        .route("/api/runs/:run_id/diff", get(get_run_diff))
        .route("/api/runs/:run_id/artifacts", get(list_run_artifacts))
        .route("/api/runs/:run_id/observations", get(list_run_observations))
        .route("/api/runs/:run_id/cancel", post(cancel_run))
        .route("/api/runs/:run_id/approvals", post(decide_run_approval))
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct InternalAttemptContextQuery {
    approval_id: Option<String>,
}

pub(super) async fn internal_attempt_context(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<InternalAttemptContextQuery>,
) -> Result<Json<pharness_runhost::AttemptSpec>, ApiError> {
    let run_id = RunId::new(run_id);
    let run = state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;

    let approval = match &query.approval_id {
        Some(approval_id) => {
            let approval = state
                .store
                .get_approval(approval_id)
                .await?
                .ok_or_else(|| ApiError::not_found("approval", approval_id))?;
            if approval.run_id != run_id {
                return Err(ApiError::conflict(
                    "approval does not belong to the requested run",
                ));
            }
            if approval.status != "approved" {
                return Err(ApiError::conflict(
                    "attempt resume requires an approved approval",
                ));
            }
            Some(approval)
        }
        None => None,
    };

    let cwd = std::path::PathBuf::from(&run.cwd);
    let spec = attempt_spec_for_run(&state.store, &run, &cwd, approval.as_ref())
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if let Some(source) = &spec.run.workspace_source {
        state
            .workspace
            .remote_source_allowed(source)
            .map_err(|error| ApiError::conflict(error.to_string()))?;
    }

    Ok(Json(spec))
}

pub(super) async fn internal_mark_running(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<RunResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    let run = state.store.mark_run_running(&run_id).await?;

    Ok(Json(run.into()))
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct InternalWorkspaceProvisionedRequest {
    pub(super) workspace_id: String,
    pub(super) resolved_commit: String,
    pub(super) branch: String,
}

pub(super) async fn internal_workspace_provisioned(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(request): Json<InternalWorkspaceProvisionedRequest>,
) -> Result<Json<WorkspaceResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    let run = state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
    let mut source = run
        .execution_target_json
        .get("workspace_source")
        .cloned()
        .ok_or_else(|| ApiError::conflict("run has no typed workspace source"))
        .and_then(|value| {
            serde_json::from_value::<pharness_runhost::WorkspaceSourceSpec>(value).map_err(
                |error| ApiError::conflict(format!("run has invalid workspace source: {error}")),
            )
        })?;
    if source.workspace_id != request.workspace_id || source.branch != request.branch {
        return Err(ApiError::conflict(
            "workspace provision report does not match the issued source contract",
        ));
    }
    source.resolved_commit = Some(request.resolved_commit);
    source
        .validate()
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    state
        .workspace
        .remote_source_allowed(&source)
        .map_err(|error| ApiError::conflict(error.to_string()))?;

    let scope = RunScope::from_execution_target(&run.execution_target_json).unwrap_or_default();
    if scope.workspace_id.as_deref() != Some(source.workspace_id.as_str()) {
        return Err(ApiError::conflict(
            "workspace provision report does not match the run scope",
        ));
    }
    let workspace = state
        .store
        .get_workspace(&source.workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("workspace", &source.workspace_id))?;
    if workspace.run_id.as_ref() != Some(&run.id)
        || scope.work_item_id.as_deref() != Some(workspace.work_item_id.as_str())
        || workspace.source_repo != source.source_repo
        || workspace.source_ref != source.source_ref
    {
        return Err(ApiError::conflict(
            "workspace provision report is not authorized for this run",
        ));
    }
    if workspace.status == "executing"
        && workspace.resolved_commit.as_deref() == source.resolved_commit.as_deref()
        && workspace.branch.as_deref() == Some(source.branch.as_str())
    {
        return Ok(Json(workspace.into()));
    }
    if workspace.status != "provisioning" {
        return Err(ApiError::conflict(
            "workspace is not awaiting source provisioning",
        ));
    }
    let workspace = state
        .store
        .update_workspace_execution(
            &workspace.id,
            UpdateWorkspaceExecution {
                run_id: Some(run.id.clone()),
                status: "executing".to_string(),
                resolved_commit: source.resolved_commit.clone(),
                branch: Some(source.branch.clone()),
                actor: Some("agent:cluster-worker".to_string()),
                reason: Some("remote source pinned by worker".to_string()),
            },
        )
        .await?;
    append_workspace_audit_event(
        &state.store,
        &workspace,
        "workspace.provisioned",
        Some("agent:cluster-worker".to_string()),
    )
    .await?;
    Ok(Json(workspace.into()))
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct InternalIngestEventsRequest {
    events: Vec<AgentEvent>,
}

pub(super) async fn internal_ingest_events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(request): Json<InternalIngestEventsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let run_id = RunId::new(run_id);
    let mut ingested = 0usize;
    for event in &request.events {
        if event.run_id != run_id {
            return Err(ApiError::conflict(
                "event run_id does not match the ingest route",
            ));
        }
        ingest_agent_event(&state.store, event).await?;
        ingested += 1;
    }

    Ok(Json(json!({ "ingested": ingested })))
}

pub(super) async fn internal_ingest_outcome(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(outcome): Json<AttemptOutcome>,
) -> Result<Json<RunResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    let run = state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;

    if matches!(run.status.as_str(), "completed" | "failed" | "cancelled") {
        return Err(ApiError::conflict(format!(
            "run is already terminal with status {}",
            run.status
        )));
    }

    finish_run_from_attempt(&state.store, &run, outcome)
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?;

    let run = state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;

    Ok(Json(run.into()))
}

pub(super) async fn internal_run_control(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let run_id = RunId::new(run_id);
    let run = state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;

    let cancel_requested = run.cancel_requested_at.is_some() || run.status == "cancelled";

    Ok(Json(json!({
        "cancel_requested": cancel_requested,
        "status": run.status,
    })))
}

#[cfg(test)]
pub(super) async fn create_run(
    State(state): State<AppState>,
    Json(request): Json<CreateRunRequest>,
) -> Result<Json<RunResponse>, ApiError> {
    create_run_for_actor(state, request, None).await
}

pub(super) async fn create_operator_run(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Json(request): Json<CreateRunRequest>,
) -> Result<Json<RunResponse>, ApiError> {
    let actor = identity.map(|Extension(OperatorIdentity(name))| name);
    create_run_for_actor(state, request, actor).await
}

pub(super) async fn create_run_for_actor(
    state: AppState,
    request: CreateRunRequest,
    actor: Option<String>,
) -> Result<Json<RunResponse>, ApiError> {
    let run_id = RunId::new(format!("run_{}", unique_suffix()));
    let session_id = SessionId::new(format!("ses_{}", run_id.as_str()));
    let cwd = state
        .worker
        .effective_cwd(&request.cwd.unwrap_or_else(|| ".".to_string()));
    let max_turns = request.max_turns.unwrap_or(40);
    let run_scope = request.scope.unwrap_or_default();
    let run_scope_json = run_scope.to_optional_json();
    let mut policy = run_policy(&state.policy, request.policy_mode);
    policy.permission_grants = active_permission_grants(&state.store).await?;

    state
        .store
        .create_session(CreateSession {
            id: session_id.clone(),
            title: request.task.chars().take(80).collect(),
            cwd: cwd.clone(),
        })
        .await?;

    let run = state
        .store
        .create_run(CreateRun {
            id: run_id.clone(),
            session_id: session_id.clone(),
            user_task: request.task,
            cwd: cwd.clone(),
            max_turns,
            initial_status: "queued".to_string(),
            execution_target_json: json!({
                "kind": state.worker.execution_target_kind(),
                "policy": &policy,
                "run_scope": &run_scope_json,
            }),
        })
        .await?;
    let run = state.store.set_run_origin(&run.id, "operator").await?;
    let run = state.store.set_run_created_by(&run.id, actor).await?;

    let worker_config = state.worker.config_json();
    let queue_payload = json!({
        "source": "api",
        "worker": state.worker.mode(),
        "provider": worker_config.get("provider"),
        "model": worker_config.get("model"),
        "policy_mode": policy.mode,
        "policy_environment": &policy.environment,
        "run_scope": &run_scope_json,
    });

    state
        .store
        .append_event(&AgentEvent {
            event_id: EventId::new(format!("evt_{}_1", run_id.as_str())),
            session_id,
            run_id,
            seq: 1,
            kind: EventKind::RunQueued,
            payload: queue_payload,
        })
        .await?;

    state.worker.spawn_run(run.clone(), cwd);

    Ok(Json(run.into()))
}

#[derive(Debug, Default, serde::Deserialize)]
pub(super) struct ListRunsQuery {
    pub(super) search: Option<String>,
    pub(super) status: Option<String>,
    pub(super) origin: Option<String>,
    pub(super) actor: Option<String>,
    pub(super) namespace: Option<String>,
    pub(super) repo: Option<String>,
    pub(super) branch: Option<String>,
    pub(super) production_impacting: Option<bool>,
    pub(super) started_after_ms: Option<i64>,
    pub(super) started_before_ms: Option<i64>,
    pub(super) limit: Option<u32>,
    pub(super) offset: Option<u32>,
}

pub(super) async fn list_runs(
    State(state): State<AppState>,
    Query(query): Query<ListRunsQuery>,
) -> Result<Json<RunsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let filter = RunListFilter {
        search: clean_optional_text(query.search),
        status: clean_optional_text(query.status),
        origin: clean_optional_text(query.origin),
        created_by: clean_optional_text(query.actor),
        namespace: clean_optional_text(query.namespace),
        repo: clean_optional_text(query.repo),
        branch: clean_optional_text(query.branch),
        production_impacting: query.production_impacting,
        started_after_ms: query.started_after_ms,
        started_before_ms: query.started_before_ms,
        limit,
        offset,
    };
    let count = state.store.count_runs(filter.clone()).await?;
    let runs = state
        .store
        .list_runs(filter.clone())
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<RunResponse>>();
    let group_runs = all_runs_for_operator_groups(state.store.as_ref(), filter).await?;
    let groups = group_operator_records(group_runs.iter().map(|run| {
        (
            run.id.to_string(),
            run.started_at.clone(),
            run.task.clone(),
            run_group_resource(run),
            run.status.clone(),
        )
    }));

    Ok(Json(RunsResponse {
        runs,
        groups,
        count,
        limit,
        offset,
    }))
}

pub(super) async fn run_summary(
    State(state): State<AppState>,
    Query(query): Query<ListRunsQuery>,
) -> Result<Json<RunSummaryResponse>, ApiError> {
    let summary = state
        .store
        .run_summary(RunSummaryFilter {
            status: clean_optional_text(query.status),
            namespace: clean_optional_text(query.namespace),
            repo: clean_optional_text(query.repo),
            branch: clean_optional_text(query.branch),
            production_impacting: query.production_impacting,
            started_after_ms: query.started_after_ms,
            started_before_ms: query.started_before_ms,
        })
        .await?;

    Ok(Json(RunSummaryResponse { summary }))
}

pub(super) async fn get_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<RunResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    let run = state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
    Ok(Json(run.into()))
}

pub(super) async fn get_run_events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<EventsResponse>, ApiError> {
    let events = state.store.list_events(&RunId::new(run_id)).await?;
    Ok(Json(EventsResponse { events }))
}

pub(super) async fn get_run_diff(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<RunDiffResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
    let changes: Vec<FileChangeResponse> = state
        .store
        .list_file_changes(&run_id)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    let diff = changes
        .iter()
        .map(|change| change.diff.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    Ok(Json(RunDiffResponse {
        run_id,
        changes,
        diff,
    }))
}

pub(super) async fn list_run_artifacts(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<ArtifactsResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
    let artifacts = state
        .store
        .list_artifacts(&run_id)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();

    Ok(Json(ArtifactsResponse { artifacts }))
}

pub(super) async fn list_run_observations(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<ObservationsResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
    let observations = state
        .store
        .list_run_observations(&run_id)
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<ObservationResponse>>();
    let count = observations.len();

    Ok(Json(ObservationsResponse {
        observations,
        count,
        limit: None,
        offset: None,
    }))
}

pub(super) async fn stream_run_events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<StreamRunEventsQuery>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let run_id = RunId::new(run_id);
    state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;

    let stream = event_stream(state.store, run_id, stream_start_seq(&headers, &query));
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[derive(Debug, Default, serde::Deserialize)]
pub(super) struct StreamRunEventsQuery {
    pub(super) after_seq: Option<u64>,
}

pub(super) fn event_stream(
    store: Arc<SqliteStore>,
    run_id: RunId,
    last_seq: u64,
) -> impl Stream<Item = Result<Event, Infallible>> {
    stream::unfold(
        EventStreamState {
            store,
            run_id,
            last_seq,
        },
        |mut state| async move {
            loop {
                match next_event(&state.store, &state.run_id, state.last_seq).await {
                    Ok(Some(event)) => {
                        state.last_seq = event.seq;
                        return Some((Ok(sse_event(event)), state));
                    }
                    Ok(None) if run_is_terminal(&state.store, &state.run_id).await => {
                        return None;
                    }
                    Ok(None) => {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                    Err(error) => {
                        return Some((Ok(sse_error_event(error)), state));
                    }
                }
            }
        },
    )
}

pub(super) struct EventStreamState {
    store: Arc<SqliteStore>,
    run_id: RunId,
    last_seq: u64,
}

pub(super) async fn next_event(
    store: &SqliteStore,
    run_id: &RunId,
    last_seq: u64,
) -> Result<Option<AgentEvent>, StoreError> {
    Ok(store
        .list_events(run_id)
        .await?
        .into_iter()
        .find(|event| event.seq > last_seq))
}

pub(super) async fn run_is_terminal(store: &SqliteStore, run_id: &RunId) -> bool {
    store
        .get_run(run_id)
        .await
        .ok()
        .flatten()
        .is_some_and(|run| matches!(run.status.as_str(), "completed" | "failed" | "cancelled"))
}

pub(super) fn last_event_seq(headers: &HeaderMap) -> u64 {
    headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(parse_last_event_id)
        .unwrap_or(0)
}

pub(super) fn stream_start_seq(headers: &HeaderMap, query: &StreamRunEventsQuery) -> u64 {
    query.after_seq.unwrap_or_else(|| last_event_seq(headers))
}

pub(super) fn parse_last_event_id(value: &str) -> Option<u64> {
    value
        .parse()
        .ok()
        .or_else(|| value.rsplit_once('_')?.1.parse().ok())
}

pub(super) fn sse_event(event: AgentEvent) -> Event {
    let event_id = event.event_id.to_string();
    let event_kind = event.kind.as_str();
    Event::default()
        .id(event_id)
        .event(event_kind)
        .json_data(event)
        .unwrap_or_else(sse_error_event)
}

pub(super) fn sse_error_event(error: impl std::fmt::Display) -> Event {
    Event::default()
        .event("stream.error")
        .data(json!({ "error": error.to_string() }).to_string())
}

pub(super) async fn cancel_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<RunResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    state.worker.cancel(&run_id);
    let run = state.store.cancel_run(&run_id).await?;
    let seq = state.store.list_events(&run_id).await?.len() as u64 + 1;
    state
        .store
        .append_event(&AgentEvent {
            event_id: EventId::new(format!("evt_{}_{}", run_id.as_str(), seq)),
            session_id: run.session_id.clone(),
            run_id,
            seq,
            kind: EventKind::RunCancelled,
            payload: json!({ "source": "api" }),
        })
        .await?;

    Ok(Json(run.into()))
}

pub(super) async fn decide_run_approval(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(request): Json<DecideApprovalRequest>,
) -> Result<Json<DecideApprovalResponse>, ApiError> {
    decide_current_run_approval(
        state,
        RunId::new(run_id),
        ApprovalDecisionInput {
            decision: request.decision,
            decided_by: request.decided_by,
            reason: request.reason,
        },
        None,
    )
    .await
}
