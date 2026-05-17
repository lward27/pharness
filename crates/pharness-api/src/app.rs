use crate::dto::{
    ApprovalDecision, ApprovalsResponse, ArtifactResponse, ArtifactsResponse, CreateRunRequest,
    DecideApprovalRequest, DecideApprovalResponse, EventsResponse, ExecuteCapabilityRequest,
    ExecuteCapabilityResponse, FileChangeResponse, RunDiffResponse, RunResponse,
};
use crate::worker::LocalWorker;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::stream::{self, Stream};
use pharness_core::{
    AgentAction, AgentEvent, EventId, EventKind, PolicyDecision, ReadOnlyClusterTools, RunId,
    SafetyPolicy, SessionId, ToolExecutor,
};
use pharness_store::{CreateRun, CreateSession, SqliteStore, StoreError};
use serde_json::json;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

#[derive(Clone)]
pub struct AppState {
    store: Arc<SqliteStore>,
    worker: Option<LocalWorker>,
}

pub fn router(store: Arc<SqliteStore>, worker: Option<LocalWorker>) -> Router {
    let state = AppState { store, worker };

    Router::new()
        .route("/health", get(health))
        .route("/api/config/effective", get(config_effective))
        .route("/api/capabilities/execute", post(execute_capability))
        .route("/api/runs", post(create_run))
        .route("/api/runs/:run_id", get(get_run))
        .route("/api/runs/:run_id/events", get(get_run_events))
        .route("/api/runs/:run_id/events/stream", get(stream_run_events))
        .route("/api/runs/:run_id/diff", get(get_run_diff))
        .route("/api/runs/:run_id/artifacts", get(list_run_artifacts))
        .route("/api/runs/:run_id/cancel", post(cancel_run))
        .route("/api/runs/:run_id/approvals", post(decide_run_approval))
        .route("/api/artifacts/:artifact_id", get(get_artifact))
        .route("/api/approvals", get(list_approvals))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true }))
}

async fn config_effective(State(state): State<AppState>) -> Json<serde_json::Value> {
    let worker = state.worker.as_ref().map(|worker| {
        let config = worker.config();
        json!({
            "enabled": config.enabled,
            "provider": config.provider,
            "model": config.model,
            "base_url": config.base_url,
        })
    });

    Json(json!({
        "api": {
            "name": "pharness-api",
        },
        "worker": worker.unwrap_or_else(|| {
            json!({
                "enabled": false,
                "provider": null,
                "model": null,
                "base_url": null,
            })
        }),
    }))
}

async fn execute_capability(
    Json(request): Json<ExecuteCapabilityRequest>,
) -> Result<Json<ExecuteCapabilityResponse>, ApiError> {
    let action = request.action;
    if !is_direct_capability_action(&action) {
        return Err(ApiError::bad_request(format!(
            "{} is not exposed through direct capability execution",
            action.kind_name()
        )));
    }

    let decision = SafetyPolicy::default().evaluate_action(&action);
    let response = match &decision {
        PolicyDecision::Allow { .. } => {
            let action_name = action.kind_name().to_string();
            match ReadOnlyClusterTools::from_env().execute(&action).await {
                Ok(result) => ExecuteCapabilityResponse {
                    status: "ok".to_string(),
                    action: action_name,
                    decision: decision.clone(),
                    executed: true,
                    result: Some(result),
                    error: None,
                },
                Err(error) => ExecuteCapabilityResponse {
                    status: "tool_error".to_string(),
                    action: action_name,
                    decision: decision.clone(),
                    executed: true,
                    result: None,
                    error: Some(error.to_string()),
                },
            }
        }
        PolicyDecision::Ask { .. } => ExecuteCapabilityResponse {
            status: "approval_required".to_string(),
            action: action.kind_name().to_string(),
            decision: decision.clone(),
            executed: false,
            result: None,
            error: None,
        },
        PolicyDecision::Deny { summary, .. } => ExecuteCapabilityResponse {
            status: "denied".to_string(),
            action: action.kind_name().to_string(),
            decision: decision.clone(),
            executed: false,
            result: None,
            error: Some(summary.clone()),
        },
    };

    Ok(Json(response))
}

fn is_direct_capability_action(action: &AgentAction) -> bool {
    matches!(
        action,
        AgentAction::KubernetesGet { .. }
            | AgentAction::ArgoGetApp { .. }
            | AgentAction::PrometheusQuery { .. }
            | AgentAction::TektonGetPipelineRuns { .. }
            | AgentAction::TektonGetTaskRuns { .. }
            | AgentAction::TektonAnalyzePipelineRun { .. }
    )
}

async fn create_run(
    State(state): State<AppState>,
    Json(request): Json<CreateRunRequest>,
) -> Result<Json<RunResponse>, ApiError> {
    let run_id = RunId::new(format!("run_{}", unique_suffix()));
    let session_id = SessionId::new(format!("ses_{}", run_id.as_str()));
    let cwd = request.cwd.unwrap_or_else(|| ".".to_string());
    let max_turns = request.max_turns.unwrap_or(40);

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
            execution_target_json: json!({ "kind": "local_process" }),
        })
        .await?;

    let queue_payload = state.worker.as_ref().map_or_else(
        || {
            json!({
                "source": "api",
                "worker": "disabled",
            })
        },
        |worker| {
            let config = worker.config();
            json!({
                "source": "api",
                "worker": "local",
                "provider": config.provider,
                "model": config.model,
            })
        },
    );

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

    if let Some(worker) = &state.worker {
        worker.spawn_run(run.clone(), cwd);
    }

    Ok(Json(run.into()))
}

async fn get_run(
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

async fn get_run_events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<EventsResponse>, ApiError> {
    let events = state.store.list_events(&RunId::new(run_id)).await?;
    Ok(Json(EventsResponse { events }))
}

async fn get_run_diff(
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

async fn list_run_artifacts(
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

async fn get_artifact(
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

async fn stream_run_events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let run_id = RunId::new(run_id);
    state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;

    let stream = event_stream(state.store, run_id, last_event_seq(&headers));
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

fn event_stream(
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

struct EventStreamState {
    store: Arc<SqliteStore>,
    run_id: RunId,
    last_seq: u64,
}

async fn next_event(
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

async fn run_is_terminal(store: &SqliteStore, run_id: &RunId) -> bool {
    store
        .get_run(run_id)
        .await
        .ok()
        .flatten()
        .is_some_and(|run| matches!(run.status.as_str(), "completed" | "failed" | "cancelled"))
}

fn last_event_seq(headers: &HeaderMap) -> u64 {
    headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(parse_last_event_id)
        .unwrap_or(0)
}

fn parse_last_event_id(value: &str) -> Option<u64> {
    value
        .parse()
        .ok()
        .or_else(|| value.rsplit_once('_')?.1.parse().ok())
}

fn sse_event(event: AgentEvent) -> Event {
    let event_id = event.event_id.to_string();
    let event_kind = event.kind.as_str();
    Event::default()
        .id(event_id)
        .event(event_kind)
        .json_data(event)
        .unwrap_or_else(sse_error_event)
}

fn sse_error_event(error: impl std::fmt::Display) -> Event {
    Event::default()
        .event("stream.error")
        .data(json!({ "error": error.to_string() }).to_string())
}

#[derive(Debug, Default, serde::Deserialize)]
struct ListApprovalsQuery {
    status: Option<String>,
    limit: Option<u32>,
}

async fn list_approvals(
    State(state): State<AppState>,
    Query(query): Query<ListApprovalsQuery>,
) -> Result<Json<ApprovalsResponse>, ApiError> {
    let approvals = state
        .store
        .list_approvals(query.status.as_deref(), query.limit.unwrap_or(50))
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(ApprovalsResponse { approvals }))
}

async fn cancel_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<RunResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    if let Some(worker) = &state.worker {
        worker.cancel(&run_id);
    }
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

async fn decide_run_approval(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(request): Json<DecideApprovalRequest>,
) -> Result<Json<DecideApprovalResponse>, ApiError> {
    let run_id = RunId::new(run_id);
    let pending = state
        .store
        .pending_approval_for_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::conflict("run has no pending approval"))?;

    match request.decision {
        ApprovalDecision::Deny => {
            let approval = state
                .store
                .decide_pending_approval(
                    &run_id,
                    "denied",
                    request.decided_by,
                    request.reason.clone(),
                )
                .await?;
            append_approval_decided_event(&state.store, &approval, "denied").await?;
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
            let Some(worker) = &state.worker else {
                return Err(ApiError::conflict(
                    "cannot approve without an enabled local worker",
                ));
            };

            let approval = state
                .store
                .decide_pending_approval(&run_id, "approved", request.decided_by, request.reason)
                .await?;
            append_approval_decided_event(&state.store, &approval, "approved").await?;
            let run = state.store.mark_run_running(&run_id).await?;
            worker.resume_run(run.clone(), approval.clone());

            Ok(Json(DecideApprovalResponse {
                approval: approval.into(),
                run: run.into(),
            }))
        }
    }
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
            }),
        })
        .await
}

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found(entity: &str, id: &str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: format!("{entity} not found: {id}"),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::NotFound { entity, id } => Self::not_found(&entity, &id),
            other => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: other.to_string(),
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": self.message,
            })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        cancel_run, config_effective, create_run, decide_run_approval, execute_capability,
        get_artifact, get_run, get_run_diff, get_run_events, last_event_seq, list_approvals,
        list_run_artifacts, parse_last_event_id, AppState, ListApprovalsQuery,
    };
    use crate::dto::{
        ApprovalDecision, CreateRunRequest, DecideApprovalRequest, ExecuteCapabilityRequest,
    };
    use axum::extract::{Path, Query, State};
    use axum::http::{HeaderMap, HeaderValue, StatusCode};
    use axum::Json;
    use pharness_core::AgentAction;
    use pharness_store::{CreateApproval, CreateArtifact, CreateFileChange, SqliteStore};
    use std::sync::Arc;

    #[tokio::test]
    async fn creates_gets_lists_events_and_cancels_run() {
        let state = AppState {
            store: Arc::new(SqliteStore::connect_in_memory().await.unwrap()),
            worker: None,
        };

        let Json(created) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "inspect app".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
            }),
        )
        .await
        .unwrap();

        assert_eq!(created.status, "queued");
        assert_eq!(created.max_turns, 12);

        let Json(fetched) = get_run(State(state.clone()), Path(created.id.to_string()))
            .await
            .unwrap();
        assert_eq!(fetched.id, created.id);

        let Json(events) = get_run_events(State(state.clone()), Path(created.id.to_string()))
            .await
            .unwrap();
        assert_eq!(events.events.len(), 1);

        let Json(cancelled) = cancel_run(State(state.clone()), Path(created.id.to_string()))
            .await
            .unwrap();
        assert_eq!(cancelled.status, "cancelled");
    }

    #[tokio::test]
    async fn reports_disabled_worker_config() {
        let state = AppState {
            store: Arc::new(SqliteStore::connect_in_memory().await.unwrap()),
            worker: None,
        };

        let Json(config) = config_effective(State(state)).await;

        assert_eq!(config["worker"]["enabled"], false);
        assert!(config["worker"]["model"].is_null());
    }

    #[tokio::test]
    async fn direct_capability_execution_denies_secret_reads() {
        let Json(response) = execute_capability(Json(ExecuteCapabilityRequest {
            action: AgentAction::KubernetesGet {
                id: "act_secret".into(),
                reason: "read secret".to_string(),
                resource: "secrets".to_string(),
                namespace: Some("argocd".to_string()),
                name: None,
                all_namespaces: false,
                label_selector: None,
            },
        }))
        .await
        .unwrap();

        assert_eq!(response.status, "denied");
        assert_eq!(response.action, "kubernetes_get");
        assert!(!response.executed);
        assert!(response.result.is_none());
    }

    #[tokio::test]
    async fn direct_capability_execution_denies_secret_shaped_tekton_reads() {
        let Json(response) = execute_capability(Json(ExecuteCapabilityRequest {
            action: AgentAction::TektonGetPipelineRuns {
                id: "act_tekton_secret".into(),
                reason: "read pipeline runs".to_string(),
                namespace: Some("token-store".to_string()),
                name: None,
                all_namespaces: false,
                label_selector: None,
            },
        }))
        .await
        .unwrap();

        assert_eq!(response.status, "denied");
        assert_eq!(response.action, "tekton_get_pipeline_runs");
        assert!(!response.executed);
        assert!(response.result.is_none());
    }

    #[tokio::test]
    async fn direct_capability_execution_denies_secret_shaped_tekton_task_reads() {
        let Json(response) = execute_capability(Json(ExecuteCapabilityRequest {
            action: AgentAction::TektonGetTaskRuns {
                id: "act_tekton_task_secret".into(),
                reason: "read task runs".to_string(),
                namespace: Some("token-store".to_string()),
                name: None,
                all_namespaces: false,
                label_selector: None,
            },
        }))
        .await
        .unwrap();

        assert_eq!(response.status, "denied");
        assert_eq!(response.action, "tekton_get_task_runs");
        assert!(!response.executed);
        assert!(response.result.is_none());
    }

    #[tokio::test]
    async fn direct_capability_execution_denies_secret_shaped_tekton_analysis() {
        let Json(response) = execute_capability(Json(ExecuteCapabilityRequest {
            action: AgentAction::TektonAnalyzePipelineRun {
                id: "act_tekton_analysis_secret".into(),
                reason: "analyze pipeline run".to_string(),
                namespace: "ci".to_string(),
                name: "token-build".to_string(),
            },
        }))
        .await
        .unwrap();

        assert_eq!(response.status, "denied");
        assert_eq!(response.action, "tekton_analyze_pipeline_run");
        assert!(!response.executed);
        assert!(response.result.is_none());
    }

    #[tokio::test]
    async fn direct_capability_execution_returns_tool_errors_as_json() {
        let Json(response) = execute_capability(Json(ExecuteCapabilityRequest {
            action: AgentAction::PrometheusQuery {
                id: "act_prom".into(),
                reason: "query".to_string(),
                query: "up".to_string(),
            },
        }))
        .await
        .unwrap();

        assert_eq!(response.status, "tool_error");
        assert_eq!(response.action, "prometheus_query");
        assert!(response.executed);
        assert!(response
            .error
            .as_deref()
            .unwrap()
            .contains("not configured"));
    }

    #[tokio::test]
    async fn direct_capability_execution_rejects_non_cluster_actions() {
        let error = execute_capability(Json(ExecuteCapabilityRequest {
            action: AgentAction::ListDir {
                id: "act_list".into(),
                reason: "list".to_string(),
                path: ".".into(),
                depth: 1,
            },
        }))
        .await
        .unwrap_err();

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn parses_sse_last_event_ids() {
        assert_eq!(parse_last_event_id("7"), Some(7));
        assert_eq!(
            parse_last_event_id("evt_run_1778887440941720000_12"),
            Some(12)
        );
        assert_eq!(parse_last_event_id("nonsense"), None);
    }

    #[test]
    fn reads_last_event_id_header() {
        let mut headers = HeaderMap::new();
        headers.insert("last-event-id", HeaderValue::from_static("evt_run_test_4"));

        assert_eq!(last_event_seq(&headers), 4);
    }

    #[tokio::test]
    async fn lists_pending_approvals() {
        let state = AppState {
            store: Arc::new(SqliteStore::connect_in_memory().await.unwrap()),
            worker: None,
        };

        let Json(created) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "write file".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
            }),
        )
        .await
        .unwrap();
        state
            .store
            .create_approval(CreateApproval {
                id: "appr_list".to_string(),
                session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
                run_id: created.id,
                status: "pending".to_string(),
                kind: "file_write".to_string(),
                summary: "write README.md".to_string(),
                risk_level: "medium".to_string(),
                action_json: None,
                resume_messages_json: None,
                turns_completed: 1,
            })
            .await
            .unwrap();

        let Json(response) = list_approvals(
            State(state),
            Query(ListApprovalsQuery {
                status: Some("pending".to_string()),
                limit: Some(50),
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.approvals.len(), 1);
        assert_eq!(response.approvals[0].id, "appr_list");
    }

    #[tokio::test]
    async fn returns_run_diff() {
        let state = AppState {
            store: Arc::new(SqliteStore::connect_in_memory().await.unwrap()),
            worker: None,
        };

        let Json(created) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "write file".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
            }),
        )
        .await
        .unwrap();
        state
            .store
            .create_file_change(CreateFileChange {
                id: "chg_test".to_string(),
                session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
                run_id: created.id.clone(),
                path: "README.md".to_string(),
                before_hash: None,
                after_hash: None,
                diff: "--- before\n+++ after".to_string(),
            })
            .await
            .unwrap();

        let Json(response) = get_run_diff(State(state), Path(created.id.to_string()))
            .await
            .unwrap();

        assert_eq!(response.changes.len(), 1);
        assert!(response.diff.contains("+++ after"));
    }

    #[tokio::test]
    async fn returns_run_artifacts_and_single_artifact() {
        let state = AppState {
            store: Arc::new(SqliteStore::connect_in_memory().await.unwrap()),
            worker: None,
        };

        let Json(created) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "observe".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
            }),
        )
        .await
        .unwrap();
        state
            .store
            .create_artifact(CreateArtifact {
                id: "art_test".to_string(),
                session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
                run_id: Some(created.id.clone()),
                kind: "tool_result".to_string(),
                label: "Prometheus query".to_string(),
                mime_type: Some("application/json".to_string()),
                path: None,
                content_text: None,
                content_json: Some(serde_json::json!({"result_count": 33})),
            })
            .await
            .unwrap();

        let Json(listed) = list_run_artifacts(State(state.clone()), Path(created.id.to_string()))
            .await
            .unwrap();
        let Json(fetched) = get_artifact(State(state), Path("art_test".to_string()))
            .await
            .unwrap();

        assert_eq!(listed.artifacts.len(), 1);
        assert_eq!(listed.artifacts[0].id, "art_test");
        assert_eq!(fetched.content_json.unwrap()["result_count"], 33);
    }

    #[tokio::test]
    async fn denial_decides_pending_approval_and_blocks_run() {
        let state = AppState {
            store: Arc::new(SqliteStore::connect_in_memory().await.unwrap()),
            worker: None,
        };

        let Json(created) = create_run(
            State(state.clone()),
            Json(CreateRunRequest {
                task: "write file".to_string(),
                cwd: Some(".".to_string()),
                max_turns: Some(12),
            }),
        )
        .await
        .unwrap();

        state
            .store
            .create_approval(CreateApproval {
                id: "appr_test".to_string(),
                session_id: pharness_core::SessionId::new(format!("ses_{}", created.id.as_str())),
                run_id: created.id.clone(),
                status: "pending".to_string(),
                kind: "file_write".to_string(),
                summary: "write README.md".to_string(),
                risk_level: "medium".to_string(),
                action_json: Some(
                    serde_json::to_value(AgentAction::WriteFile {
                        id: "act_write".into(),
                        reason: "test".to_string(),
                        path: "README.md".into(),
                        content: "hello".to_string(),
                    })
                    .unwrap(),
                ),
                resume_messages_json: Some(serde_json::json!([])),
                turns_completed: 1,
            })
            .await
            .unwrap();
        state
            .store
            .mark_run_approval_required(
                &created.id,
                serde_json::json!({
                    "status": "approval_required",
                    "approval_id": "appr_test"
                }),
            )
            .await
            .unwrap();

        let Json(response) = decide_run_approval(
            State(state.clone()),
            Path(created.id.to_string()),
            Json(DecideApprovalRequest {
                decision: ApprovalDecision::Deny,
                decided_by: Some("test".to_string()),
                reason: Some("not now".to_string()),
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.approval.status, "denied");
        assert_eq!(response.run.status, "failed");
        let events = state.store.list_events(&created.id).await.unwrap();
        assert!(events
            .iter()
            .any(|event| event.kind == pharness_core::EventKind::ApprovalDecided));
    }
}
