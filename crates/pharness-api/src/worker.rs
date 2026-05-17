use pharness_core::{
    AgentAction, AgentEvent, AgentRuntime, ApprovedAction, CancellationFlag, CapabilityKind,
    CompositeToolExecutor, EventId, EventKind, EventSink, LocalReadOnlyFsTools, LocalShellTools,
    ModelMessage, PolicyMode, ReadOnlyClusterTools, RunConfig, RunId, RunStatus, SafetyPolicy,
    ToolProtocolMode, ToolSpec,
};
use pharness_fireworks::{FireworksClient, FireworksProviderConfig, DEFAULT_FIREWORKS_BASE_URL};
use pharness_store::{
    CreateApproval, CreateArtifact, CreateFileChange, SqliteStore, StoreError, StoredApproval,
    StoredRun,
};
use secrecy::SecretString;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

const DEFAULT_FIREWORKS_MODEL: &str = "accounts/fireworks/models/kimi-k2p5";

#[derive(Clone)]
pub struct LocalWorker {
    store: Arc<SqliteStore>,
    provider: FireworksClient,
    model: String,
    base_url: String,
    cancellations: Arc<Mutex<HashMap<RunId, CancellationFlag>>>,
}

impl LocalWorker {
    pub fn from_env(store: Arc<SqliteStore>) -> anyhow::Result<Option<Self>> {
        let Ok(api_key) = std::env::var("FIREWORKS_API_KEY") else {
            return Ok(None);
        };

        let model = std::env::var("PHARNESS_FIREWORKS_MODEL")
            .unwrap_or_else(|_| DEFAULT_FIREWORKS_MODEL.to_string());
        let base_url = std::env::var("PHARNESS_FIREWORKS_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_FIREWORKS_BASE_URL.to_string());

        let provider = FireworksClient::new(
            SecretString::new(api_key),
            FireworksProviderConfig {
                base_url: base_url.clone(),
                model: model.clone(),
            },
        )?;

        Ok(Some(Self {
            store,
            provider,
            model,
            base_url,
            cancellations: Arc::new(Mutex::new(HashMap::new())),
        }))
    }

    pub fn config(&self) -> LocalWorkerConfig {
        LocalWorkerConfig {
            enabled: true,
            provider: "fireworks".to_string(),
            model: self.model.clone(),
            base_url: self.base_url.clone(),
        }
    }

    pub fn spawn_run(&self, run: StoredRun, cwd: impl Into<PathBuf>) {
        self.spawn_task(run, cwd.into(), None);
    }

    pub fn resume_run(&self, run: StoredRun, approval: StoredApproval) {
        self.spawn_task(run.clone(), PathBuf::from(run.cwd.clone()), Some(approval));
    }

    pub fn cancel(&self, run_id: &RunId) -> bool {
        let Some(flag) = self
            .cancellations
            .lock()
            .expect("cancellation registry mutex should not be poisoned")
            .get(run_id)
            .cloned()
        else {
            return false;
        };

        flag.cancel();
        true
    }

    fn spawn_task(&self, run: StoredRun, cwd: PathBuf, approval: Option<StoredApproval>) {
        let store = self.store.clone();
        let provider = self.provider.clone();
        let cancellations = self.cancellations.clone();
        let cancellation = CancellationFlag::default();

        cancellations
            .lock()
            .expect("cancellation registry mutex should not be poisoned")
            .insert(run.id.clone(), cancellation.clone());

        tokio::spawn(async move {
            let run_id = run.id.clone();
            let result = match approval {
                Some(approval) => {
                    resume_local_agent(store.clone(), provider, run, cwd, cancellation, approval)
                        .await
                }
                None => run_local_agent(store.clone(), provider, run, cwd, cancellation).await,
            };

            cancellations
                .lock()
                .expect("cancellation registry mutex should not be poisoned")
                .remove(&run_id);

            if let Err(error) = result {
                let _ = fail_before_runtime(&store, &run_id, error.to_string()).await;
            }
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocalWorkerConfig {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
    pub base_url: String,
}

async fn run_local_agent(
    store: Arc<SqliteStore>,
    provider: FireworksClient,
    run: StoredRun,
    cwd: PathBuf,
    cancellation: CancellationFlag,
) -> anyhow::Result<()> {
    store.mark_run_running(&run.id).await?;

    let seq_start = store.list_events(&run.id).await?.len() as u64;
    let (sender, receiver) = mpsc::unbounded_channel();
    let event_writer = tokio::spawn(persist_events(store.clone(), receiver));
    let sink = ChannelEventSink { sender };
    let tools = CompositeToolExecutor::new(
        CompositeToolExecutor::new(
            LocalReadOnlyFsTools::new(&cwd)?,
            ReadOnlyClusterTools::from_env(),
        ),
        LocalShellTools::new(&cwd)?,
    );
    let runtime = AgentRuntime::with_tools(provider, sink, tools);

    let outcome = runtime
        .run(
            RunConfig {
                session_id: run.session_id.clone(),
                run_id: run.id.clone(),
                messages: vec![
                    ModelMessage::system(system_prompt()),
                    ModelMessage::user(run.user_task.clone()),
                ],
                tools: worker_tool_specs(),
                tool_protocol: ToolProtocolMode::NativeTools,
                temperature: 0.1,
                max_tokens: 4096,
                max_turns: run.max_turns,
                policy: SafetyPolicy {
                    mode: PolicyMode::Default,
                    ..SafetyPolicy::default()
                },
                event_seq_start: seq_start,
            },
            cancellation,
        )
        .await;

    drop(runtime);
    event_writer.await??;

    finish_run_with_outcome(&store, &run, outcome).await
}

async fn resume_local_agent(
    store: Arc<SqliteStore>,
    provider: FireworksClient,
    run: StoredRun,
    cwd: PathBuf,
    cancellation: CancellationFlag,
    approval: StoredApproval,
) -> anyhow::Result<()> {
    store.mark_run_running(&run.id).await?;

    let action_json = approval
        .action_json
        .clone()
        .ok_or_else(|| anyhow::anyhow!("approval has no reviewed action payload"))?;
    let messages_json = approval
        .resume_messages_json
        .clone()
        .ok_or_else(|| anyhow::anyhow!("approval has no resumable message transcript"))?;
    let approved = ApprovedAction {
        approval_id: approval.id.clone(),
        action: serde_json::from_value::<AgentAction>(action_json)?,
        resume_messages: serde_json::from_value::<Vec<ModelMessage>>(messages_json)?,
        turns_completed: approval.turns_completed,
    };

    let seq_start = store.list_events(&run.id).await?.len() as u64;
    let (sender, receiver) = mpsc::unbounded_channel();
    let event_writer = tokio::spawn(persist_events(store.clone(), receiver));
    let sink = ChannelEventSink { sender };
    let tools = local_tool_executor(&cwd)?;
    let runtime = AgentRuntime::with_tools(provider, sink, tools);

    let outcome = runtime
        .resume_after_approval(
            RunConfig {
                session_id: run.session_id.clone(),
                run_id: run.id.clone(),
                messages: Vec::new(),
                tools: worker_tool_specs(),
                tool_protocol: ToolProtocolMode::NativeTools,
                temperature: 0.1,
                max_tokens: 4096,
                max_turns: run.max_turns,
                policy: SafetyPolicy {
                    mode: PolicyMode::Default,
                    ..SafetyPolicy::default()
                },
                event_seq_start: seq_start,
            },
            cancellation,
            approved,
        )
        .await;

    drop(runtime);
    event_writer.await??;

    finish_run_with_outcome(&store, &run, outcome).await
}

async fn finish_run_with_outcome(
    store: &SqliteStore,
    run: &StoredRun,
    outcome: pharness_core::RunOutcome,
) -> anyhow::Result<()> {
    let status = outcome.status;
    let summary = outcome.summary.clone();
    let error = outcome.error.clone();
    let approval_id = if status == RunStatus::ApprovalRequired {
        if let Some(approval) = &outcome.approval {
            Some(create_pending_approval(store, run, approval).await?.id)
        } else {
            None
        }
    } else {
        None
    };
    let result_json = serde_json::json!({
        "status": run_status_str(status),
        "turns": outcome.turns,
        "summary": summary,
        "error": error,
        "approval_id": approval_id,
    });

    match status {
        RunStatus::Completed => {
            store
                .complete_run(&run.id, "completed", result_json, None)
                .await?;
        }
        RunStatus::ApprovalRequired => {
            store
                .mark_run_approval_required(&run.id, result_json)
                .await?;
        }
        RunStatus::Failed => {
            store
                .complete_run(&run.id, "failed", result_json, error)
                .await?;
        }
        RunStatus::Cancelled => {
            store
                .complete_run(&run.id, "cancelled", result_json, error)
                .await?;
        }
    }

    Ok(())
}

fn local_tool_executor(
    cwd: &PathBuf,
) -> Result<
    CompositeToolExecutor<
        CompositeToolExecutor<LocalReadOnlyFsTools, ReadOnlyClusterTools>,
        LocalShellTools,
    >,
    pharness_core::ToolError,
> {
    Ok(CompositeToolExecutor::new(
        CompositeToolExecutor::new(
            LocalReadOnlyFsTools::new(cwd)?,
            ReadOnlyClusterTools::from_env(),
        ),
        LocalShellTools::new(cwd)?,
    ))
}

async fn create_pending_approval(
    store: &SqliteStore,
    run: &StoredRun,
    approval: &pharness_core::PendingApproval,
) -> Result<StoredApproval, StoreError> {
    store
        .create_approval(CreateApproval {
            id: format!("appr_{}_{}", run.id.as_str(), unique_suffix()),
            session_id: run.session_id.clone(),
            run_id: run.id.clone(),
            status: "pending".to_string(),
            kind: json_string(approval.approval_kind),
            summary: approval.summary.clone(),
            risk_level: json_string(approval.risk),
            action_json: approval
                .action
                .as_ref()
                .map(serde_json::to_value)
                .transpose()?,
            resume_messages_json: Some(serde_json::to_value(&approval.resume_messages)?),
            turns_completed: approval.turns_completed,
        })
        .await
}

fn json_string<T>(value: T) -> String
where
    T: serde::Serialize,
{
    serde_json::to_value(value)
        .and_then(serde_json::from_value)
        .unwrap_or_else(|_| "unknown".to_string())
}

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

async fn persist_events(
    store: Arc<SqliteStore>,
    mut receiver: mpsc::UnboundedReceiver<AgentEvent>,
) -> Result<(), StoreError> {
    while let Some(event) = receiver.recv().await {
        store.append_event(&event).await?;
        if let Some(change) = file_change_from_event(&event) {
            store.create_file_change(change).await?;
        }
        if let Some(artifact) = artifact_from_event(&event) {
            store.create_artifact(artifact).await?;
        }
    }

    Ok(())
}

fn file_change_from_event(event: &AgentEvent) -> Option<CreateFileChange> {
    if event.kind != EventKind::ToolFinished {
        return None;
    }

    let content = event.payload.get("content")?;
    let path = content.get("path")?.as_str()?;
    let diff = content.get("diff")?.as_str()?;

    Some(CreateFileChange {
        id: format!("chg_{}", event.event_id.as_str()),
        session_id: event.session_id.clone(),
        run_id: event.run_id.clone(),
        path: path.to_string(),
        before_hash: None,
        after_hash: None,
        diff: diff.to_string(),
    })
}

fn artifact_from_event(event: &AgentEvent) -> Option<CreateArtifact> {
    if event.kind != EventKind::ToolFinished {
        return None;
    }

    let content = event.payload.get("content")?;
    let source = content.get("source")?.as_str()?;
    if !matches!(source, "kubernetes" | "argocd" | "prometheus" | "tekton") {
        return None;
    }

    let kind = if source == "tekton"
        && content.get("resource").and_then(serde_json::Value::as_str)
            == Some("pipeline_run_analysis")
    {
        "pipeline_run_analysis".to_string()
    } else {
        format!("{source}_tool_result")
    };

    Some(CreateArtifact {
        id: format!("art_{}", event.event_id.as_str()),
        session_id: event.session_id.clone(),
        run_id: Some(event.run_id.clone()),
        kind,
        label: event
            .payload
            .get("summary")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("tool result")
            .to_string(),
        mime_type: Some("application/json".to_string()),
        path: None,
        content_text: None,
        content_json: Some(content.clone()),
    })
}

async fn fail_before_runtime(
    store: &SqliteStore,
    run_id: &RunId,
    message: String,
) -> Result<(), StoreError> {
    let seq = store.list_events(run_id).await?.len() as u64 + 1;
    let Some(run) = store.get_run(run_id).await? else {
        return Ok(());
    };

    store
        .append_event(&AgentEvent {
            event_id: EventId::new(format!("evt_{}_{}", run_id.as_str(), seq)),
            session_id: run.session_id,
            run_id: run_id.clone(),
            seq,
            kind: EventKind::RunFailed,
            payload: serde_json::json!({ "error": message }),
        })
        .await?;

    store
        .complete_run(
            run_id,
            "failed",
            serde_json::json!({
                "status": "failed",
                "turns": 0,
                "summary": null,
                "error": message,
            }),
            Some(message),
        )
        .await?;

    Ok(())
}

#[derive(Clone)]
struct ChannelEventSink {
    sender: mpsc::UnboundedSender<AgentEvent>,
}

impl EventSink for ChannelEventSink {
    fn append(&self, event: AgentEvent) {
        let _ = self.sender.send(event);
    }
}

fn system_prompt() -> &'static str {
    r#"You are the pharness local SDLC agent worker for lucas_engineering.
Use exactly one tool call per turn. Do not answer with prose unless you call the respond tool.
Available action tools are: respond, finish, list_dir, read_file, search_files, write_file, patch_file, run_shell, git_diff, git_status, kubernetes_get, argo_get_app, prometheus_query, tekton_get_pipeline_runs, tekton_get_task_runs, tekton_analyze_pipeline_run.
Prefer read-only repo inspection first. Never read secrets, .env files, private keys, kubeconfigs, tokens, or credential files.
File writes, destructive commands, network commands, and production mutations are policy-gated and may pause for approval.
For available policy-gated actions, call the concrete tool. The runtime will pause for approval before execution.
Use patch_file for small existing-file text edits when an exact find/replace patch is safer than rewriting the whole file.
Use typed read-only actions for Kubernetes, Argo CD, and Prometheus inspection:
- kubernetes_get fields: resource, namespace, name, all_namespaces, label_selector.
- argo_get_app fields: app.
- prometheus_query fields: query.
- tekton_get_pipeline_runs fields: namespace, name, all_namespaces, label_selector.
- tekton_get_task_runs fields: namespace, name, all_namespaces, label_selector.
- tekton_analyze_pipeline_run fields: namespace, name.
Never request Kubernetes Secret resources or secret-shaped names, labels, or metric queries.
For registry, database, or any unavailable cluster mutation, use respond to explain that the capability is not exposed yet.
When done, use finish with success and a concise summary."#
}

fn worker_tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec::new(
            "respond",
            "Return a non-final message to the operator when more information is needed.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "message"],
                "properties": {
                    "reason": { "type": "string" },
                    "message": { "type": "string" }
                }
            }),
            CapabilityKind::AgentControl,
        ),
        ToolSpec::new(
            "finish",
            "Finish the run with a concise machine-readable summary.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "summary", "success"],
                "properties": {
                    "reason": { "type": "string" },
                    "summary": { "type": "string" },
                    "success": { "type": "boolean" }
                }
            }),
            CapabilityKind::AgentControl,
        ),
        ToolSpec::new(
            "list_dir",
            "List files and directories under a workspace path.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "path", "depth"],
                "properties": {
                    "reason": { "type": "string" },
                    "path": { "type": "string" },
                    "depth": { "type": "integer", "minimum": 0, "maximum": 3 }
                }
            }),
            CapabilityKind::Filesystem,
        ),
        ToolSpec::new(
            "read_file",
            "Read a UTF-8 file inside the workspace. Do not read secrets or credential files.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "path"],
                "properties": {
                    "reason": { "type": "string" },
                    "path": { "type": "string" },
                    "max_bytes": { "type": ["integer", "null"], "minimum": 1, "maximum": 262144 }
                }
            }),
            CapabilityKind::Filesystem,
        ),
        ToolSpec::new(
            "search_files",
            "Search UTF-8 files inside the workspace for a string.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "query"],
                "properties": {
                    "reason": { "type": "string" },
                    "query": { "type": "string" },
                    "path": { "type": ["string", "null"] },
                    "glob": { "type": ["string", "null"] }
                }
            }),
            CapabilityKind::Filesystem,
        ),
        ToolSpec::new(
            "write_file",
            "Write a UTF-8 file inside the workspace. This is policy-gated and requires approval in default mode.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "path", "content"],
                "properties": {
                    "reason": { "type": "string" },
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                }
            }),
            CapabilityKind::Filesystem,
        ),
        ToolSpec::new(
            "patch_file",
            "Apply an exact UTF-8 find/replace patch to an existing workspace file. This is policy-gated and requires approval in default mode.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "path", "patch"],
                "properties": {
                    "reason": { "type": "string" },
                    "path": { "type": "string" },
                    "patch": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["find", "replace"],
                        "properties": {
                            "find": { "type": "string", "minLength": 1 },
                            "replace": { "type": "string" },
                            "replace_all": { "type": "boolean" }
                        }
                    }
                }
            }),
            CapabilityKind::Filesystem,
        ),
        ToolSpec::new(
            "run_shell",
            "Run a policy-gated local shell command inside the workspace. Non-zero exit is returned as structured output.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "cmd", "dry_run"],
                "properties": {
                    "reason": { "type": "string" },
                    "cmd": { "type": "string" },
                    "cwd": { "type": ["string", "null"] },
                    "timeout_ms": { "type": ["integer", "null"] },
                    "dry_run": { "type": "boolean" }
                }
            }),
            CapabilityKind::Shell,
        ),
        ToolSpec::new(
            "git_status",
            "Read git status for the workspace.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason"],
                "properties": {
                    "reason": { "type": "string" }
                }
            }),
            CapabilityKind::Git,
        ),
        ToolSpec::new(
            "git_diff",
            "Read git diff for the workspace, optionally scoped by pathspec.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason"],
                "properties": {
                    "reason": { "type": "string" },
                    "pathspec": { "type": ["string", "null"] }
                }
            }),
            CapabilityKind::Git,
        ),
        ToolSpec::new(
            "kubernetes_get",
            "Read Kubernetes resources with kubectl get -o json. Secret-shaped resources are denied.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "resource", "all_namespaces"],
                "properties": {
                    "reason": { "type": "string" },
                    "resource": { "type": "string" },
                    "namespace": { "type": ["string", "null"] },
                    "name": { "type": ["string", "null"] },
                    "all_namespaces": { "type": "boolean" },
                    "label_selector": { "type": ["string", "null"] }
                }
            }),
            CapabilityKind::KubernetesRead,
        ),
        ToolSpec::new(
            "argo_get_app",
            "Read an Argo CD Application CRD from the configured Argo CD namespace.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "app"],
                "properties": {
                    "reason": { "type": "string" },
                    "app": { "type": "string" }
                }
            }),
            CapabilityKind::ArgoRead,
        ),
        ToolSpec::new(
            "prometheus_query",
            "Run a read-only Prometheus instant query against PHARNESS_PROMETHEUS_URL.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "query"],
                "properties": {
                    "reason": { "type": "string" },
                    "query": { "type": "string" }
                }
            }),
            CapabilityKind::ObservabilityRead,
        ),
        ToolSpec::new(
            "tekton_get_pipeline_runs",
            "Read Tekton PipelineRuns through the Kubernetes API. Secret-shaped names and labels are denied.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "all_namespaces"],
                "properties": {
                    "reason": { "type": "string" },
                    "namespace": { "type": ["string", "null"] },
                    "name": { "type": ["string", "null"] },
                    "all_namespaces": { "type": "boolean" },
                    "label_selector": { "type": ["string", "null"] }
                }
            }),
            CapabilityKind::TektonRead,
        ),
        ToolSpec::new(
            "tekton_get_task_runs",
            "Read Tekton TaskRuns through the Kubernetes API. Secret-shaped names and labels are denied.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "all_namespaces"],
                "properties": {
                    "reason": { "type": "string" },
                    "namespace": { "type": ["string", "null"] },
                    "name": { "type": ["string", "null"] },
                    "all_namespaces": { "type": "boolean" },
                    "label_selector": { "type": ["string", "null"] }
                }
            }),
            CapabilityKind::TektonRead,
        ),
        ToolSpec::new(
            "tekton_analyze_pipeline_run",
            "Read one Tekton PipelineRun and its related TaskRuns, then return a normalized PipelineRunAnalysis summary.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "namespace", "name"],
                "properties": {
                    "reason": { "type": "string" },
                    "namespace": { "type": "string" },
                    "name": { "type": "string" }
                }
            }),
            CapabilityKind::TektonRead,
        ),
    ]
}

fn run_status_str(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Completed => "completed",
        RunStatus::ApprovalRequired => "approval_required",
        RunStatus::Failed => "failed",
        RunStatus::Cancelled => "cancelled",
    }
}

#[cfg(test)]
mod tests {
    use super::{artifact_from_event, file_change_from_event, worker_tool_specs};
    use pharness_core::{AgentEvent, EventId, EventKind, RunId, SessionId};
    use std::collections::HashSet;

    #[test]
    fn worker_tool_schema_contains_terminal_and_read_only_actions() {
        let names = worker_tool_specs()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<HashSet<_>>();

        for expected in [
            "respond",
            "finish",
            "list_dir",
            "read_file",
            "search_files",
            "write_file",
            "patch_file",
            "run_shell",
            "git_status",
            "git_diff",
            "kubernetes_get",
            "argo_get_app",
            "prometheus_query",
            "tekton_get_pipeline_runs",
            "tekton_get_task_runs",
            "tekton_analyze_pipeline_run",
        ] {
            assert!(names.contains(expected), "missing tool spec for {expected}");
        }
    }

    #[test]
    fn worker_tool_schema_does_not_expose_non_resumable_approval_by_default() {
        let names = worker_tool_specs()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<HashSet<_>>();

        assert!(!names.contains("request_approval"));
    }

    #[test]
    fn extracts_file_change_from_write_tool_event() {
        let event = AgentEvent {
            event_id: EventId::new("evt_run_test_8"),
            session_id: SessionId::new("ses_test"),
            run_id: RunId::new("run_test"),
            seq: 8,
            kind: EventKind::ToolFinished,
            payload: serde_json::json!({
                "status": "ok",
                "summary": "wrote file",
                "content": {
                    "path": "README.md",
                    "diff": "--- before\n+++ after"
                }
            }),
        };

        let change = file_change_from_event(&event).unwrap();

        assert_eq!(change.id, "chg_evt_run_test_8");
        assert_eq!(change.path, "README.md");
        assert!(change.diff.contains("+++ after"));
    }

    #[test]
    fn extracts_cluster_artifact_from_tool_event() {
        let event = AgentEvent {
            event_id: EventId::new("evt_run_test_8"),
            session_id: SessionId::new("ses_test"),
            run_id: RunId::new("run_test"),
            seq: 8,
            kind: EventKind::ToolFinished,
            payload: serde_json::json!({
                "status": "ok",
                "summary": "read Prometheus instant query",
                "content": {
                    "source": "prometheus",
                    "query": "up",
                    "response": {
                        "data": {
                            "result_count": 33
                        }
                    }
                }
            }),
        };

        let artifact = artifact_from_event(&event).unwrap();

        assert_eq!(artifact.id, "art_evt_run_test_8");
        assert_eq!(artifact.kind, "prometheus_tool_result");
        assert_eq!(artifact.label, "read Prometheus instant query");
        assert_eq!(
            artifact.content_json.unwrap()["response"]["data"]["result_count"],
            33
        );
    }

    #[test]
    fn extracts_pipeline_run_analysis_artifact_from_tool_event() {
        let event = AgentEvent {
            event_id: EventId::new("evt_run_test_9"),
            session_id: SessionId::new("ses_test"),
            run_id: RunId::new("run_test"),
            seq: 9,
            kind: EventKind::ToolFinished,
            payload: serde_json::json!({
                "status": "ok",
                "summary": "analyzed Tekton PipelineRun ci/build-app",
                "content": {
                    "source": "tekton",
                    "resource": "pipeline_run_analysis",
                    "analysis": {
                        "kind": "PipelineRunAnalysis",
                        "summary": {
                            "status": "failed"
                        }
                    }
                }
            }),
        };

        let artifact = artifact_from_event(&event).unwrap();

        assert_eq!(artifact.id, "art_evt_run_test_9");
        assert_eq!(artifact.kind, "pipeline_run_analysis");
        assert_eq!(artifact.label, "analyzed Tekton PipelineRun ci/build-app");
        assert_eq!(
            artifact.content_json.unwrap()["analysis"]["summary"]["status"],
            "failed"
        );
    }
}
