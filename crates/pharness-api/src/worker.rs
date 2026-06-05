use pharness_core::{
    simple_text_diff, AgentAction, AgentEvent, AgentRuntime, ApprovedAction, CancellationFlag,
    CapabilityKind, CompositeToolExecutor, EventId, EventKind, EventSink, LocalReadOnlyFsTools,
    LocalShellTools, ModelMessage, ReadOnlyClusterTools, ResourceRef, RunConfig, RunId, RunScope,
    RunStatus, SafetyPolicy, TextPatch, ToolProtocolMode, ToolSpec,
};
use pharness_fireworks::{FireworksClient, FireworksProviderConfig};
use pharness_store::{
    CreateApproval, CreateApprovalGate, CreateArtifact, CreateAuditEvent, CreateFileChange,
    CreateIncident, CreateObservation, CreateRemediationPlan, SqliteStore, StoreError,
    StoredApproval, StoredIncident, StoredObservation, StoredRemediationPlan, StoredRun,
};
use secrecy::SecretString;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct LocalWorker {
    store: Arc<SqliteStore>,
    provider: FireworksClient,
    model: String,
    base_url: String,
    cluster_tools: ReadOnlyClusterTools,
    default_policy: SafetyPolicy,
    cancellations: Arc<Mutex<HashMap<RunId, CancellationFlag>>>,
}

impl LocalWorker {
    pub fn from_options(
        store: Arc<SqliteStore>,
        options: LocalWorkerOptions,
    ) -> anyhow::Result<Option<Self>> {
        let Some(api_key) = options.api_key else {
            return Ok(None);
        };
        let model = options.model;
        let base_url = options.base_url;
        let cluster_tools = options.cluster_tools;
        let default_policy = options.default_policy;

        let provider = FireworksClient::new(
            api_key,
            FireworksProviderConfig {
                base_url: base_url.clone(),
                model: model.clone(),
            },
        )?;

        Ok(Some(Self {
            store,
            model,
            base_url,
            provider,
            cluster_tools,
            default_policy,
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
        let cluster_tools = self.cluster_tools.clone();
        let default_policy = self.default_policy.clone();
        let cancellations = self.cancellations.clone();
        let cancellation = CancellationFlag::default();

        cancellations
            .lock()
            .expect("cancellation registry mutex should not be poisoned")
            .insert(run.id.clone(), cancellation.clone());

        tokio::spawn(async move {
            let run_id = run.id.clone();
            let execution = LocalRunExecution {
                store: store.clone(),
                provider,
                cluster_tools,
                default_policy,
                cancellation,
            };
            let result = match approval {
                Some(approval) => resume_local_agent(execution, run, cwd, approval).await,
                None => run_local_agent(execution, run, cwd).await,
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

#[derive(Clone)]
pub struct LocalWorkerOptions {
    pub api_key: Option<SecretString>,
    pub model: String,
    pub base_url: String,
    pub cluster_tools: ReadOnlyClusterTools,
    pub default_policy: SafetyPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocalWorkerConfig {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
    pub base_url: String,
}

struct LocalRunExecution {
    store: Arc<SqliteStore>,
    provider: FireworksClient,
    cluster_tools: ReadOnlyClusterTools,
    default_policy: SafetyPolicy,
    cancellation: CancellationFlag,
}

async fn run_local_agent(
    execution: LocalRunExecution,
    run: StoredRun,
    cwd: PathBuf,
) -> anyhow::Result<()> {
    let store = execution.store;
    store.mark_run_running(&run.id).await?;

    let seq_start = store.list_events(&run.id).await?.len() as u64;
    let (sender, receiver) = mpsc::unbounded_channel();
    let event_writer = tokio::spawn(persist_events(store.clone(), receiver));
    let sink = ChannelEventSink { sender };
    let tools = CompositeToolExecutor::new(
        CompositeToolExecutor::new(LocalReadOnlyFsTools::new(&cwd)?, execution.cluster_tools),
        LocalShellTools::new(&cwd)?,
    );
    let runtime = AgentRuntime::with_tools(execution.provider, sink, tools);

    let policy = policy_for_run(&run, &execution.default_policy)?;
    let run_scope = run_scope_for_run(&run);
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
                policy,
                run_scope,
                event_seq_start: seq_start,
            },
            execution.cancellation,
        )
        .await;

    drop(runtime);
    event_writer.await??;

    finish_run_with_outcome(&store, &run, outcome).await
}

async fn resume_local_agent(
    execution: LocalRunExecution,
    run: StoredRun,
    cwd: PathBuf,
    approval: StoredApproval,
) -> anyhow::Result<()> {
    let store = execution.store;
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
    let tools = local_tool_executor(&cwd, execution.cluster_tools)?;
    let runtime = AgentRuntime::with_tools(execution.provider, sink, tools);

    let policy = policy_for_run(&run, &execution.default_policy)?;
    let run_scope = run_scope_for_run(&run);
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
                policy,
                run_scope,
                event_seq_start: seq_start,
            },
            execution.cancellation,
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
    let result_json = result_json_for_outcome(run, &outcome, approval_id);

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
    cluster_tools: ReadOnlyClusterTools,
) -> Result<
    CompositeToolExecutor<
        CompositeToolExecutor<LocalReadOnlyFsTools, ReadOnlyClusterTools>,
        LocalShellTools,
    >,
    pharness_core::ToolError,
> {
    Ok(CompositeToolExecutor::new(
        CompositeToolExecutor::new(LocalReadOnlyFsTools::new(cwd)?, cluster_tools),
        LocalShellTools::new(cwd)?,
    ))
}

fn policy_for_run(run: &StoredRun, default_policy: &SafetyPolicy) -> anyhow::Result<SafetyPolicy> {
    let Some(policy_json) = run.execution_target_json.get("policy") else {
        return Ok(default_policy.clone());
    };

    serde_json::from_value(policy_json.clone())
        .map_err(|error| anyhow::anyhow!("run has invalid persisted policy: {error}"))
}

fn run_scope_for_run(run: &StoredRun) -> RunScope {
    RunScope::from_execution_target(&run.execution_target_json).unwrap_or_default()
}

fn result_json_for_outcome(
    run: &StoredRun,
    outcome: &pharness_core::RunOutcome,
    approval_id: Option<String>,
) -> serde_json::Value {
    let run_scope = run_scope_for_run(run);
    serde_json::json!({
        "status": run_status_str(outcome.status),
        "turns": outcome.turns,
        "summary": &outcome.summary,
        "error": &outcome.error,
        "approval_id": approval_id,
        "run_scope": run_scope.to_optional_json(),
    })
}

async fn create_pending_approval(
    store: &SqliteStore,
    run: &StoredRun,
    approval: &pharness_core::PendingApproval,
) -> Result<StoredApproval, StoreError> {
    let run_scope = run_scope_for_run(run);
    let run_scope_json = run_scope.to_optional_json();

    store
        .create_approval(CreateApproval {
            id: format!("appr_{}_{}", run.id.as_str(), unique_suffix()),
            session_id: run.session_id.clone(),
            run_id: run.id.clone(),
            status: "pending".to_string(),
            kind: json_string(approval.approval_kind),
            summary: approval.summary.clone(),
            risk_level: json_string(approval.risk),
            run_scope_json,
            action_json: approval
                .action
                .as_ref()
                .map(serde_json::to_value)
                .transpose()?,
            preview_json: approval_preview_for_action(&run.cwd, approval.action.as_ref()),
            resume_messages_json: Some(serde_json::to_value(&approval.resume_messages)?),
            turns_completed: approval.turns_completed,
        })
        .await
}

const MAX_APPROVAL_PREVIEW_DIFF_BYTES: usize = 64 * 1024;

fn approval_preview_for_action(
    cwd: &str,
    action: Option<&AgentAction>,
) -> Option<serde_json::Value> {
    match action? {
        AgentAction::WriteFile { path, content, .. } => {
            Some(write_file_approval_preview(cwd, path.as_str(), content))
        }
        AgentAction::PatchFile { path, patch, .. } => {
            Some(patch_file_approval_preview(cwd, path.as_str(), patch))
        }
        _ => None,
    }
}

fn write_file_approval_preview(cwd: &str, path: &str, content: &str) -> serde_json::Value {
    if is_secret_shaped_path(path) {
        return approval_preview_error(
            "write_file",
            path,
            "preview skipped for secret-shaped path",
        );
    }

    let target = match resolve_preview_write_path(cwd, path) {
        Ok(target) => target,
        Err(error) => return approval_preview_error("write_file", path, error),
    };
    let existed = target.path.exists();
    let before = if existed {
        match fs::read_to_string(&target.path) {
            Ok(content) => Some(content),
            Err(error) => {
                return approval_preview_error(
                    "write_file",
                    path,
                    format!("failed to read existing file for preview: {error}"),
                );
            }
        }
    } else {
        None
    };
    let (diff, diff_truncated) = bounded_preview_diff(before.as_deref(), content);

    serde_json::json!({
        "kind": "file_write",
        "action": "write_file",
        "path": target.display_path(),
        "status": "ok",
        "existed": existed,
        "before_bytes": before.as_ref().map(|value| value.len()),
        "after_bytes": content.len(),
        "diff": diff,
        "diff_truncated": diff_truncated
    })
}

fn patch_file_approval_preview(cwd: &str, path: &str, patch: &TextPatch) -> serde_json::Value {
    if is_secret_shaped_path(path) {
        return approval_preview_error(
            "patch_file",
            path,
            "preview skipped for secret-shaped path",
        );
    }
    if patch.find.is_empty() {
        return approval_preview_error("patch_file", path, "patch.find must not be empty");
    }

    let target = match resolve_preview_existing_path(cwd, path) {
        Ok(target) => target,
        Err(error) => return approval_preview_error("patch_file", path, error),
    };
    if !target.path.is_file() {
        return approval_preview_error("patch_file", path, "patch target is not a file");
    }

    let before = match fs::read_to_string(&target.path) {
        Ok(content) => content,
        Err(error) => {
            return approval_preview_error(
                "patch_file",
                path,
                format!("failed to read target file for preview: {error}"),
            );
        }
    };
    let matches = before.matches(&patch.find).count();
    if matches == 0 {
        return patch_preview_match_error(path, "patch.find did not match target file", matches);
    }
    if !patch.replace_all && matches != 1 {
        return patch_preview_match_error(
            path,
            format!(
                "patch.find matched {matches} times; set replace_all=true to preview execution"
            ),
            matches,
        );
    }

    let after = if patch.replace_all {
        before.replace(&patch.find, &patch.replace)
    } else {
        before.replacen(&patch.find, &patch.replace, 1)
    };
    let replacements = if patch.replace_all { matches } else { 1 };
    let (diff, diff_truncated) = bounded_preview_diff(Some(&before), &after);

    serde_json::json!({
        "kind": "file_write",
        "action": "patch_file",
        "path": target.display_path(),
        "status": "ok",
        "replacements": replacements,
        "replace_all": patch.replace_all,
        "before_bytes": before.len(),
        "after_bytes": after.len(),
        "diff": diff,
        "diff_truncated": diff_truncated
    })
}

fn patch_preview_match_error(
    path: &str,
    error: impl Into<String>,
    matches: usize,
) -> serde_json::Value {
    let mut preview = approval_preview_error("patch_file", path, error);
    if let Some(object) = preview.as_object_mut() {
        object.insert("matches".to_string(), serde_json::json!(matches));
    }
    preview
}

fn approval_preview_error(action: &str, path: &str, error: impl Into<String>) -> serde_json::Value {
    serde_json::json!({
        "kind": "file_write",
        "action": action,
        "path": path,
        "status": "error",
        "error": error.into()
    })
}

fn bounded_preview_diff(before: Option<&str>, after: &str) -> (String, bool) {
    let diff = simple_text_diff(before, after);
    if diff.len() <= MAX_APPROVAL_PREVIEW_DIFF_BYTES {
        return (diff, false);
    }

    (truncate_utf8(&diff, MAX_APPROVAL_PREVIEW_DIFF_BYTES), true)
}

fn truncate_utf8(input: &str, max_bytes: usize) -> String {
    let end = input
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= max_bytes)
        .last()
        .unwrap_or(0);
    format!("{}\n[diff truncated]", &input[..end])
}

#[derive(Debug)]
struct PreviewPath {
    canonical_root: PathBuf,
    path: PathBuf,
}

impl PreviewPath {
    fn display_path(&self) -> String {
        self.path
            .strip_prefix(&self.canonical_root)
            .unwrap_or(&self.path)
            .to_string_lossy()
            .trim_start_matches('/')
            .to_string()
    }
}

fn resolve_preview_write_path(cwd: &str, path: &str) -> Result<PreviewPath, String> {
    let canonical_root = canonical_workspace_root(cwd)?;
    let candidate = candidate_path(&canonical_root, path);
    let parent = candidate
        .parent()
        .ok_or_else(|| format!("write path has no parent: {path}"))?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize parent for {path}: {error}"))?;
    if !canonical_parent.starts_with(&canonical_root) {
        return Err("path resolves outside the workspace".to_string());
    }

    Ok(PreviewPath {
        canonical_root,
        path: candidate,
    })
}

fn resolve_preview_existing_path(cwd: &str, path: &str) -> Result<PreviewPath, String> {
    let canonical_root = canonical_workspace_root(cwd)?;
    let candidate = candidate_path(&canonical_root, path);
    let canonical = candidate
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize {path}: {error}"))?;
    if !canonical.starts_with(&canonical_root) {
        return Err("path resolves outside the workspace".to_string());
    }

    Ok(PreviewPath {
        canonical_root,
        path: canonical,
    })
}

fn canonical_workspace_root(cwd: &str) -> Result<PathBuf, String> {
    Path::new(cwd)
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize workspace root: {error}"))
}

fn candidate_path(canonical_root: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        canonical_root.join(path)
    }
}

fn is_secret_shaped_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let file_name = Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase();

    file_name == ".env"
        || file_name.starts_with(".env.")
        || lower.contains("kubeconfig")
        || lower.contains("id_rsa")
        || lower.contains("id_ed25519")
        || lower.contains("secret")
        || lower.contains("token")
        || lower.contains("credential")
        || lower.ends_with(".pem")
        || lower.ends_with(".p12")
        || lower.ends_with(".pfx")
        || lower.ends_with(".key")
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
        let artifact_id = if let Some(artifact) = artifact_from_event(&event) {
            let artifact_id = artifact.id.clone();
            store.create_artifact(artifact).await?;
            Some(artifact_id)
        } else {
            None
        };
        if let Some(observation) = observation_from_event(&event, artifact_id) {
            let observation = store.create_observation(observation).await?;
            if let Some(incident) = incident_from_observation(&observation) {
                let incident = store.create_incident(incident).await?;
                if let Some(plan) = remediation_plan_from_incident(&incident) {
                    let plan = store.create_remediation_plan(plan).await?;
                    for gate in approval_gates_from_remediation_plan(&plan) {
                        store.create_approval_gate(gate).await?;
                    }
                }
            }
        }
        if let Some(audit_event) = grant_used_audit_event_from_event(&event) {
            store.create_audit_event(audit_event).await?;
        }
    }

    Ok(())
}

fn grant_used_audit_event_from_event(event: &AgentEvent) -> Option<CreateAuditEvent> {
    if event.kind != EventKind::PolicyEvaluated {
        return None;
    }

    let grant_id = event.payload.get("decision")?.get("grant_id")?.as_str()?;

    Some(CreateAuditEvent {
        id: format!("aud_{}_grant_used", event.event_id.as_str()),
        kind: "permission_grant.used".to_string(),
        actor: Some("agent:local-worker".to_string()),
        resource_kind: "permission_grant".to_string(),
        resource_id: grant_id.to_string(),
        run_id: Some(event.run_id.clone()),
        payload_json: serde_json::json!({
            "grant_id": grant_id,
            "session_id": event.session_id.as_str(),
            "run_id": event.run_id.as_str(),
            "source_event_id": event.event_id.as_str(),
            "action": event.payload.get("action"),
            "decision": event.payload.get("decision"),
            "run_scope": event.payload.get("run_scope"),
        }),
    })
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
    if !matches!(
        source,
        "kubernetes" | "argocd" | "prometheus" | "loki" | "tekton"
    ) {
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

fn observation_from_event(
    event: &AgentEvent,
    artifact_id: Option<String>,
) -> Option<CreateObservation> {
    if event.kind != EventKind::ToolFinished {
        return None;
    }

    let content = event.payload.get("content")?;
    let source = content.get("source")?.as_str()?;
    if !matches!(
        source,
        "kubernetes" | "argocd" | "prometheus" | "loki" | "tekton"
    ) {
        return None;
    }

    let kind = observation_kind(content, source);
    let subject = observation_subject(content, source, &kind);
    let identity = observation_resource_identity(content, source, &kind, &subject);
    let summary = event
        .payload
        .get("summary")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("observed tool result")
        .to_string();
    let resource_ref_json = observation_resource_ref(event, content, source, &kind, &subject);

    Some(CreateObservation {
        id: format!("obs_{}", event.event_id.as_str()),
        session_id: event.session_id.clone(),
        run_id: Some(event.run_id.clone()),
        source: source.to_string(),
        kind,
        subject,
        summary,
        resource_namespace: identity.namespace,
        resource_kind: identity.kind,
        resource_name: identity.name,
        resource_ref_json,
        artifact_id,
        data_json: observation_data(content),
    })
}

fn observation_kind(content: &serde_json::Value, source: &str) -> String {
    content
        .get("resource")
        .and_then(serde_json::Value::as_str)
        .or_else(|| content.get("action").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| format!("{source}_read"))
}

fn observation_subject(content: &serde_json::Value, source: &str, kind: &str) -> String {
    if let Some(query) = content.get("query").and_then(serde_json::Value::as_str) {
        return query.to_string();
    }
    if let Some(name) = content.get("name").and_then(serde_json::Value::as_str) {
        return name.to_string();
    }
    if let Some(namespace) = content.get("namespace").and_then(serde_json::Value::as_str) {
        return format!("{namespace}/{kind}");
    }
    format!("{source}/{kind}")
}

#[derive(Debug, Default)]
struct ObservationResourceIdentity {
    namespace: Option<String>,
    kind: Option<String>,
    name: Option<String>,
}

fn observation_resource_identity(
    content: &serde_json::Value,
    source: &str,
    kind: &str,
    subject: &str,
) -> ObservationResourceIdentity {
    ObservationResourceIdentity {
        namespace: first_string(&[
            content.pointer("/namespace"),
            content.pointer("/output/metadata/namespace"),
            content.pointer("/analysis/pipeline_run/namespace"),
        ]),
        kind: observation_resource_kind(content, source, kind),
        name: first_string(&[
            content.pointer("/name"),
            content.pointer("/output/metadata/name"),
            content.pointer("/analysis/pipeline_run/name"),
        ])
        .or_else(|| normalized_resource_name(source, kind, subject)),
    }
}

fn observation_resource_kind(
    content: &serde_json::Value,
    source: &str,
    kind: &str,
) -> Option<String> {
    let output_kind = content
        .pointer("/output/kind")
        .and_then(serde_json::Value::as_str);
    if output_kind.is_some_and(|value| value != "List") {
        return output_kind.map(str::to_string);
    }
    if source == "tekton" && kind == "pipeline_run_analysis" {
        return Some("PipelineRun".to_string());
    }

    first_string(&[
        content.pointer("/analysis/pipeline_run/kind"),
        content.pointer("/resource"),
    ])
    .or_else(|| normalized_resource_kind(source, kind))
}

fn normalized_resource_kind(source: &str, kind: &str) -> Option<String> {
    match (source, kind) {
        ("argocd", _) => Some("Application".to_string()),
        ("prometheus", "inventory") => Some("inventory".to_string()),
        ("prometheus", _) => Some("query".to_string()),
        ("loki", "log_summary") => Some("log_summary".to_string()),
        ("tekton", "pipeline_run_analysis") => Some("PipelineRun".to_string()),
        (_, value) if !value.trim().is_empty() => Some(value.to_string()),
        _ => None,
    }
}

fn normalized_resource_name(source: &str, kind: &str, subject: &str) -> Option<String> {
    match (source, kind) {
        ("prometheus", "inventory") => Some("inventory".to_string()),
        ("loki", "log_summary") => Some("log_summary".to_string()),
        _ if !subject.trim().is_empty() && !subject.contains('/') => Some(subject.to_string()),
        _ => None,
    }
}

fn first_string(values: &[Option<&serde_json::Value>]) -> Option<String> {
    values
        .iter()
        .filter_map(|value| value.and_then(serde_json::Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

fn observation_resource_ref(
    event: &AgentEvent,
    content: &serde_json::Value,
    source: &str,
    kind: &str,
    subject: &str,
) -> Option<serde_json::Value> {
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        "event_id".to_string(),
        serde_json::Value::String(event.event_id.to_string()),
    );
    metadata.insert(
        "run_id".to_string(),
        serde_json::Value::String(event.run_id.to_string()),
    );

    let mut resource =
        ResourceRef::new(source, kind, subject).with_metadata(serde_json::Value::Object(metadata));
    if let Some(namespace) = content.get("namespace").and_then(serde_json::Value::as_str) {
        resource = resource.with_namespace(namespace);
    }

    serde_json::to_value(resource).ok()
}

fn observation_data(content: &serde_json::Value) -> serde_json::Value {
    let mut data = serde_json::Map::new();
    copy_observation_field(&mut data, content, "source");
    copy_observation_field(&mut data, content, "resource");
    copy_observation_field(&mut data, content, "namespace");
    copy_observation_field(&mut data, content, "name");
    copy_observation_field(&mut data, content, "query");
    copy_observation_field(&mut data, content, "output");
    copy_observation_field(&mut data, content, "response");
    copy_observation_field(&mut data, content, "analysis");

    serde_json::Value::Object(data)
}

fn incident_from_observation(observation: &StoredObservation) -> Option<CreateIncident> {
    if observation.source != "tekton" || observation.kind != "pipeline_run_analysis" {
        return None;
    }

    let analysis = observation.data_json.get("analysis")?;
    let reasons = pipeline_run_incident_reasons(analysis);
    if reasons.is_empty() {
        return None;
    }

    let severity = pipeline_run_incident_severity(&reasons);
    let resource = observation_resource_label(observation);
    let summary = reasons.join("; ");

    Some(CreateIncident {
        id: format!("inc_{}", observation.id),
        observation_id: observation.id.clone(),
        session_id: observation.session_id.clone(),
        run_id: observation.run_id.clone(),
        status: "candidate".to_string(),
        severity: severity.to_string(),
        title: format!("Tekton PipelineRun issue: {resource}"),
        summary: summary.clone(),
        resource_namespace: observation.resource_namespace.clone(),
        resource_kind: observation.resource_kind.clone(),
        resource_name: observation.resource_name.clone(),
        data_json: serde_json::json!({
            "source": "observation",
            "observation_id": observation.id.clone(),
            "reasons": reasons,
            "summary": summary,
        }),
    })
}

fn remediation_plan_from_incident(incident: &StoredIncident) -> Option<CreateRemediationPlan> {
    if incident.status != "candidate" {
        return None;
    }

    let resource = incident_resource_label(incident);
    Some(CreateRemediationPlan {
        id: format!("rplan_{}", incident.id),
        incident_id: incident.id.clone(),
        session_id: incident.session_id.clone(),
        run_id: incident.run_id.clone(),
        status: "draft".to_string(),
        title: format!("Draft remediation for {resource}"),
        summary: "Review the incident evidence, run read-only checks, then require approval before any write, pipeline, or cluster mutation.".to_string(),
        risk_level: incident.severity.clone(),
        requires_approval: true,
        resource_namespace: incident.resource_namespace.clone(),
        resource_kind: incident.resource_kind.clone(),
        resource_name: incident.resource_name.clone(),
        plan_json: remediation_plan_json(incident, &resource),
    })
}

fn remediation_plan_json(incident: &StoredIncident, resource: &str) -> serde_json::Value {
    let reasons = incident
        .data_json
        .get("reasons")
        .cloned()
        .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
    serde_json::json!({
        "mode": "read_only_draft",
        "incident_id": incident.id.clone(),
        "resource": {
            "namespace": incident.resource_namespace.clone(),
            "kind": incident.resource_kind.clone(),
            "name": incident.resource_name.clone(),
            "label": resource,
        },
        "evidence": {
            "summary": incident.summary.clone(),
            "reasons": reasons,
        },
        "steps": [
            {
                "order": 1,
                "kind": "read_only",
                "capability": "tekton_analyze_pipeline_run",
                "summary": "Re-read PipelineRun, TaskRuns, Deployment health, Argo health, and image alignment before deciding on any action."
            },
            {
                "order": 2,
                "kind": "read_only",
                "capability": "loki_log_summary",
                "summary": "Inspect bounded application and controller logs for the affected namespace if Loki is configured."
            },
            {
                "order": 3,
                "kind": "proposal",
                "capability": "worktree_change",
                "summary": "If evidence points to repo configuration or application code, prepare a ChangeSet and require approval before file writes."
            },
            {
                "order": 4,
                "kind": "proposal",
                "capability": "pipeline_or_deployment_action",
                "summary": "If evidence points to stale deployment state, propose rerun, sync, rollback, or restart intent and require explicit approval before mutation."
            }
        ],
        "approval_gates": [
            {
                "kind": "file_write",
                "required_before": "creating or patching a ChangeSet"
            },
            {
                "kind": "pipeline_mutation",
                "required_before": "rerunning or cancelling Tekton resources"
            },
            {
                "kind": "cluster_mutation",
                "required_before": "Argo sync, rollback, restart, scale, or Kubernetes write"
            },
            {
                "kind": "production_impact",
                "required_before": "any action against production-impacting scope"
            }
        ],
        "non_goals": [
            "No automatic mutation in V1",
            "No ticket creation",
            "No notification dispatch",
            "No secret reads"
        ]
    })
}

fn approval_gates_from_remediation_plan(plan: &StoredRemediationPlan) -> Vec<CreateApprovalGate> {
    let gates = plan
        .plan_json
        .get("approval_gates")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();

    gates
        .into_iter()
        .enumerate()
        .filter_map(|(index, gate_json)| {
            let gate_kind = approval_gate_kind(&gate_json)?;
            let gate_order = i64::try_from(index).ok()?.saturating_add(1);
            let required_before = gate_json
                .get("required_before")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("executing a risky action");
            let title = format!("Approve {}", gate_kind.replace('_', " "));
            Some(CreateApprovalGate {
                id: format!(
                    "agate_{}_{}_{}",
                    plan.id,
                    gate_order,
                    safe_id_fragment(&gate_kind)
                ),
                remediation_plan_id: plan.id.clone(),
                incident_id: plan.incident_id.clone(),
                session_id: plan.session_id.clone(),
                run_id: plan.run_id.clone(),
                status: "pending".to_string(),
                gate_kind: gate_kind.clone(),
                gate_order,
                title,
                summary: format!("Approval required before {required_before}."),
                risk_level: plan.risk_level.clone(),
                resource_namespace: plan.resource_namespace.clone(),
                resource_kind: plan.resource_kind.clone(),
                resource_name: plan.resource_name.clone(),
                gate_json,
            })
        })
        .collect()
}

fn approval_gate_kind(gate_json: &serde_json::Value) -> Option<String> {
    gate_json
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .or_else(|| gate_json.as_str())
        .map(str::trim)
        .filter(|kind| !kind.is_empty())
        .map(ToOwned::to_owned)
}

fn safe_id_fragment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn pipeline_run_incident_reasons(analysis: &serde_json::Value) -> Vec<String> {
    let mut reasons = Vec::new();
    if let Some(status) = analysis
        .pointer("/summary/status")
        .and_then(serde_json::Value::as_str)
    {
        if !matches!(status, "succeeded" | "running") {
            reasons.push(format!("PipelineRun status is {status}"));
        }
    }
    if let Some(status) = analysis
        .pointer("/deployment/status")
        .and_then(serde_json::Value::as_str)
    {
        if status != "healthy" && status != "skipped" {
            reasons.push(format!("Deployment status is {status}"));
        }
    }
    if let Some(status) = analysis
        .pointer("/summary/argo_sync_status")
        .and_then(serde_json::Value::as_str)
    {
        if status != "Synced" {
            reasons.push(format!("Argo sync status is {status}"));
        }
    }
    if let Some(status) = analysis
        .pointer("/summary/argo_health_status")
        .and_then(serde_json::Value::as_str)
    {
        if status != "Healthy" {
            reasons.push(format!("Argo health status is {status}"));
        }
    }
    if let Some(status) = analysis
        .pointer("/summary/image_alignment/status")
        .and_then(serde_json::Value::as_str)
    {
        if !matches!(status, "exact_match" | "registry_alias_match" | "unknown") {
            reasons.push(format!("Image alignment is {status}"));
        }
    }

    reasons
}

fn pipeline_run_incident_severity(reasons: &[String]) -> &'static str {
    if reasons
        .iter()
        .any(|reason| reason.contains("failed") || reason.contains("error"))
    {
        "high"
    } else {
        "medium"
    }
}

fn observation_resource_label(observation: &StoredObservation) -> String {
    match (
        observation.resource_namespace.as_deref(),
        observation.resource_name.as_deref(),
    ) {
        (Some(namespace), Some(name)) => format!("{namespace}/{name}"),
        (_, Some(name)) => name.to_string(),
        _ => observation.subject.clone(),
    }
}

fn incident_resource_label(incident: &StoredIncident) -> String {
    match (
        incident.resource_namespace.as_deref(),
        incident.resource_name.as_deref(),
    ) {
        (Some(namespace), Some(name)) => format!("{namespace}/{name}"),
        (_, Some(name)) => name.to_string(),
        _ => incident.title.clone(),
    }
}

fn copy_observation_field(
    data: &mut serde_json::Map<String, serde_json::Value>,
    content: &serde_json::Value,
    field: &str,
) {
    if let Some(value) = content.get(field) {
        data.insert(field.to_string(), value.clone());
    }
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
Available action tools are: respond, finish, list_dir, read_file, search_files, write_file, patch_file, run_shell, git_diff, git_status, kubernetes_get, argo_get_app, prometheus_query, prometheus_inventory, loki_log_summary, tekton_get_pipeline_runs, tekton_get_task_runs, tekton_analyze_pipeline_run.
Prefer read-only repo inspection first. Never read secrets, .env files, private keys, kubeconfigs, tokens, or credential files.
File writes, destructive commands, network commands, and production mutations are policy-gated and may pause for approval.
For available policy-gated actions, call the concrete tool. The runtime will pause for approval before execution.
Use patch_file for small existing-file text edits when an exact find/replace patch is safer than rewriting the whole file.
Use typed read-only actions for Kubernetes, Argo CD, and Prometheus inspection:
- kubernetes_get fields: resource, namespace, name, all_namespaces, label_selector.
- argo_get_app fields: app.
- prometheus_query fields: query.
- prometheus_inventory fields: none beyond reason.
- loki_log_summary fields: query, since_seconds, limit.
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
            "prometheus_inventory",
            "Read bounded Prometheus targets, rules, and active alerts from PHARNESS_PROMETHEUS_URL.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason"],
                "properties": {
                    "reason": { "type": "string" }
                }
            }),
            CapabilityKind::ObservabilityRead,
        ),
        ToolSpec::new(
            "loki_log_summary",
            "Read bounded Loki log lines from PHARNESS_LOKI_URL with compacted, redacted output.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["reason", "query"],
                "properties": {
                    "reason": { "type": "string" },
                    "query": { "type": "string" },
                    "since_seconds": {
                        "type": ["integer", "null"],
                        "minimum": 60,
                        "maximum": 86400
                    },
                    "limit": {
                        "type": ["integer", "null"],
                        "minimum": 1,
                        "maximum": 100
                    }
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
    use super::{
        approval_gates_from_remediation_plan, approval_preview_for_action, artifact_from_event,
        file_change_from_event, grant_used_audit_event_from_event, incident_from_observation,
        observation_from_event, remediation_plan_from_incident, result_json_for_outcome,
        unique_suffix, worker_tool_specs,
    };
    use pharness_core::{
        AgentAction, AgentEvent, EventId, EventKind, RunId, RunOutcome, RunStatus, SessionId,
        TextPatch,
    };
    use pharness_store::{StoredIncident, StoredObservation, StoredRemediationPlan, StoredRun};
    use std::collections::HashSet;
    use std::fs;
    use std::path::PathBuf;

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
            "prometheus_inventory",
            "loki_log_summary",
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
    fn result_json_uses_null_for_absent_run_scope() {
        let run = stored_run(serde_json::json!({
            "kind": "local_process",
            "run_scope": null,
        }));
        let outcome = RunOutcome {
            status: RunStatus::Completed,
            turns: 2,
            summary: Some("done".to_string()),
            error: None,
            approval: None,
        };

        let result = result_json_for_outcome(&run, &outcome, None);

        assert!(result["run_scope"].is_null());
        assert_eq!(result["status"], "completed");
    }

    #[test]
    fn result_json_preserves_non_empty_run_scope() {
        let run = stored_run(serde_json::json!({
            "kind": "local_process",
            "run_scope": {
                "namespace": "apps-dev",
                "repo": "git@example.test/team/app.git",
                "branch": "feature/pharness",
                "production_impacting": false
            },
        }));
        let outcome = RunOutcome {
            status: RunStatus::Completed,
            turns: 2,
            summary: Some("done".to_string()),
            error: None,
            approval: None,
        };

        let result = result_json_for_outcome(&run, &outcome, None);

        assert_eq!(result["run_scope"]["namespace"], "apps-dev");
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
    fn previews_write_file_approval_with_diff() {
        let temp = temp_dir("write-preview");
        fs::write(temp.join("README.md"), "old\n").unwrap();
        let action = AgentAction::WriteFile {
            id: "act_write".into(),
            reason: "test".to_string(),
            path: "README.md".into(),
            content: "new\n".to_string(),
        };

        let preview = approval_preview_for_action(temp.to_str().unwrap(), Some(&action)).unwrap();

        assert_eq!(preview["status"], "ok");
        assert_eq!(preview["action"], "write_file");
        assert_eq!(preview["path"], "README.md");
        assert_eq!(preview["existed"], true);
        assert!(preview["diff"].as_str().unwrap().contains("-old"));
        assert!(preview["diff"].as_str().unwrap().contains("+new"));
    }

    #[test]
    fn previews_patch_file_approval_with_diff() {
        let temp = temp_dir("patch-preview");
        fs::write(temp.join("README.md"), "alpha\nbeta\n").unwrap();
        let action = AgentAction::PatchFile {
            id: "act_patch".into(),
            reason: "test".to_string(),
            path: "README.md".into(),
            patch: TextPatch {
                find: "beta".to_string(),
                replace: "gamma".to_string(),
                replace_all: false,
            },
        };

        let preview = approval_preview_for_action(temp.to_str().unwrap(), Some(&action)).unwrap();

        assert_eq!(preview["status"], "ok");
        assert_eq!(preview["action"], "patch_file");
        assert_eq!(preview["replacements"], 1);
        assert!(preview["diff"].as_str().unwrap().contains("-beta"));
        assert!(preview["diff"].as_str().unwrap().contains("+gamma"));
    }

    #[test]
    fn skips_secret_shaped_approval_preview() {
        let temp = temp_dir("secret-preview");
        let action = AgentAction::WriteFile {
            id: "act_write".into(),
            reason: "test".to_string(),
            path: ".env".into(),
            content: "TOKEN=value\n".to_string(),
        };

        let preview = approval_preview_for_action(temp.to_str().unwrap(), Some(&action)).unwrap();

        assert_eq!(preview["status"], "error");
        assert_eq!(preview["action"], "write_file");
        assert!(preview["diff"].is_null());
        assert!(preview["error"]
            .as_str()
            .unwrap()
            .contains("secret-shaped path"));
    }

    #[test]
    fn extracts_permission_grant_used_audit_event() {
        let event = AgentEvent {
            event_id: EventId::new("evt_run_test_7"),
            session_id: SessionId::new("ses_test"),
            run_id: RunId::new("run_test"),
            seq: 7,
            kind: EventKind::PolicyEvaluated,
            payload: serde_json::json!({
                "action": "write_file",
                "decision": {
                    "decision": "allow",
                    "risk": "medium",
                    "summary": "allowed by grant",
                    "grant_id": "pgrant_test"
                },
                "run_scope": {
                    "namespace": "apps-dev",
                    "repo": "git@example.test/team/app.git",
                    "branch": "feature/pharness",
                    "production_impacting": false
                }
            }),
        };

        let audit_event = grant_used_audit_event_from_event(&event).unwrap();

        assert_eq!(audit_event.kind, "permission_grant.used");
        assert_eq!(audit_event.resource_id, "pgrant_test");
        assert_eq!(audit_event.run_id.as_ref().unwrap().as_str(), "run_test");
        assert_eq!(
            audit_event.payload_json["run_scope"]["namespace"],
            "apps-dev"
        );
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
    fn extracts_observation_from_tool_event() {
        let event = AgentEvent {
            event_id: EventId::new("evt_run_test_11"),
            session_id: SessionId::new("ses_test"),
            run_id: RunId::new("run_test"),
            seq: 11,
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

        let observation =
            observation_from_event(&event, Some("art_evt_run_test_11".to_string())).unwrap();

        assert_eq!(observation.id, "obs_evt_run_test_11");
        assert_eq!(observation.source, "prometheus");
        assert_eq!(observation.kind, "prometheus_read");
        assert_eq!(observation.subject, "up");
        assert_eq!(observation.resource_kind.as_deref(), Some("query"));
        assert_eq!(observation.resource_name.as_deref(), Some("up"));
        assert_eq!(
            observation.artifact_id.as_deref(),
            Some("art_evt_run_test_11")
        );
        assert_eq!(
            observation.data_json["response"]["data"]["result_count"],
            33
        );
    }

    #[test]
    fn extracts_loki_artifact_from_tool_event() {
        let event = AgentEvent {
            event_id: EventId::new("evt_run_test_10"),
            session_id: SessionId::new("ses_test"),
            run_id: RunId::new("run_test"),
            seq: 10,
            kind: EventKind::ToolFinished,
            payload: serde_json::json!({
                "status": "ok",
                "summary": "read Loki log summary",
                "content": {
                    "source": "loki",
                    "resource": "log_summary",
                    "response": {
                        "data": {
                            "entry_count": 3
                        }
                    }
                }
            }),
        };

        let artifact = artifact_from_event(&event).unwrap();

        assert_eq!(artifact.id, "art_evt_run_test_10");
        assert_eq!(artifact.kind, "loki_tool_result");
        assert_eq!(artifact.label, "read Loki log summary");
        assert_eq!(
            artifact.content_json.unwrap()["response"]["data"]["entry_count"],
            3
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
                    "namespace": "ci",
                    "name": "build-app",
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
        let observation = observation_from_event(&event, Some(artifact.id.clone())).unwrap();

        assert_eq!(artifact.id, "art_evt_run_test_9");
        assert_eq!(artifact.kind, "pipeline_run_analysis");
        assert_eq!(artifact.label, "analyzed Tekton PipelineRun ci/build-app");
        assert_eq!(
            artifact.content_json.unwrap()["analysis"]["summary"]["status"],
            "failed"
        );
        assert_eq!(observation.resource_namespace.as_deref(), Some("ci"));
        assert_eq!(observation.resource_kind.as_deref(), Some("PipelineRun"));
        assert_eq!(observation.resource_name.as_deref(), Some("build-app"));
    }

    #[test]
    fn extracts_incident_candidate_from_failed_pipeline_observation() {
        let observation = StoredObservation {
            id: "obs_test".to_string(),
            session_id: SessionId::new("ses_test"),
            run_id: Some(RunId::new("run_test")),
            source: "tekton".to_string(),
            kind: "pipeline_run_analysis".to_string(),
            subject: "build-app".to_string(),
            summary: "analyzed Tekton PipelineRun ci/build-app".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-app".to_string()),
            resource_ref_json: None,
            artifact_id: Some("art_test".to_string()),
            data_json: serde_json::json!({
                "analysis": {
                    "summary": {
                        "status": "failed",
                        "argo_sync_status": "OutOfSync",
                        "argo_health_status": "Degraded",
                        "image_alignment": {
                            "status": "registry_mismatch"
                        }
                    },
                    "deployment": {
                        "status": "progressing"
                    }
                }
            }),
            observed_at: "1".to_string(),
        };

        let incident = incident_from_observation(&observation).unwrap();

        assert_eq!(incident.id, "inc_obs_test");
        assert_eq!(incident.status, "candidate");
        assert_eq!(incident.severity, "high");
        assert_eq!(incident.resource_namespace.as_deref(), Some("ci"));
        assert_eq!(incident.resource_kind.as_deref(), Some("PipelineRun"));
        assert_eq!(incident.resource_name.as_deref(), Some("build-app"));
        assert!(
            incident
                .data_json
                .get("reasons")
                .and_then(serde_json::Value::as_array)
                .unwrap()
                .len()
                >= 4
        );
    }

    #[test]
    fn extracts_draft_remediation_plan_from_incident_candidate() {
        let incident = StoredIncident {
            id: "inc_obs_test".to_string(),
            observation_id: "obs_test".to_string(),
            session_id: SessionId::new("ses_test"),
            run_id: Some(RunId::new("run_test")),
            status: "candidate".to_string(),
            severity: "high".to_string(),
            title: "Tekton PipelineRun issue: ci/build-app".to_string(),
            summary: "PipelineRun status is failed".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-app".to_string()),
            data_json: serde_json::json!({
                "reasons": ["PipelineRun status is failed"]
            }),
            created_at: "1".to_string(),
        };

        let plan = remediation_plan_from_incident(&incident).unwrap();

        assert_eq!(plan.id, "rplan_inc_obs_test");
        assert_eq!(plan.incident_id, "inc_obs_test");
        assert_eq!(plan.status, "draft");
        assert_eq!(plan.risk_level, "high");
        assert!(plan.requires_approval);
        assert_eq!(plan.resource_namespace.as_deref(), Some("ci"));
        assert_eq!(
            plan.plan_json["approval_gates"]
                .as_array()
                .map(Vec::len)
                .unwrap_or_default(),
            4
        );
        assert_eq!(plan.plan_json["mode"], "read_only_draft");

        let stored_plan = StoredRemediationPlan {
            id: plan.id,
            incident_id: plan.incident_id,
            session_id: plan.session_id,
            run_id: plan.run_id,
            status: plan.status,
            title: plan.title,
            summary: plan.summary,
            risk_level: plan.risk_level,
            requires_approval: plan.requires_approval,
            resource_namespace: plan.resource_namespace,
            resource_kind: plan.resource_kind,
            resource_name: plan.resource_name,
            plan_json: plan.plan_json,
            created_at: "1".to_string(),
        };
        let gates = approval_gates_from_remediation_plan(&stored_plan);

        assert_eq!(gates.len(), 4);
        assert_eq!(gates[0].id, "agate_rplan_inc_obs_test_1_file_write");
        assert_eq!(gates[0].gate_kind, "file_write");
        assert_eq!(gates[0].gate_order, 1);
        assert_eq!(gates[0].status, "pending");
        assert_eq!(gates[0].risk_level, "high");
        assert_eq!(gates[0].resource_namespace.as_deref(), Some("ci"));
    }

    fn stored_run(execution_target_json: serde_json::Value) -> StoredRun {
        StoredRun {
            id: RunId::new("run_test"),
            session_id: SessionId::new("ses_test"),
            cwd: ".".to_string(),
            status: "queued".to_string(),
            user_task: "test".to_string(),
            max_turns: 40,
            started_at: "0".to_string(),
            finished_at: None,
            cancel_requested_at: None,
            error: None,
            result_json: None,
            execution_target_json,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("pharness-worker-{name}-{}", unique_suffix()));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
