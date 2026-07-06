#![forbid(unsafe_code)]

//! Shared run-attempt host for pharness workers.
//!
//! One attempt executes the agent loop for a run from start (or resume after
//! approval) until a terminal state or an approval pause. The host is generic
//! over an [`AttemptBackend`] so the same loop runs in-process inside
//! `pharness-api` (direct store access) and inside the `pharness-worker`
//! binary (HTTP ingest against the API, which stays the sole store writer).

mod preview;
mod prompt;

pub use preview::approval_preview_for_action;
pub use prompt::{system_prompt, worker_tool_specs};

use pharness_core::{
    AgentEvent, AgentRuntime, ApprovedAction, CancellationFlag, CompositeToolExecutor, EventSink,
    LocalReadOnlyFsTools, LocalShellTools, ModelMessage, ReadOnlyClusterTools, RunConfig,
    RunOutcome, RunScope, RunStatus, SafetyPolicy, ToolProtocolMode,
};
use pharness_fireworks::FireworksClient;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

/// The run fields an attempt needs, independent of the store row shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSpec {
    pub run_id: String,
    pub session_id: String,
    pub cwd: String,
    pub user_task: String,
    pub max_turns: u32,
    pub execution_target_json: serde_json::Value,
}

/// Resume payload reconstructed from a decided approval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeSpec {
    pub approval_id: String,
    pub action_json: serde_json::Value,
    pub resume_messages_json: serde_json::Value,
    pub turns_completed: u32,
}

/// Everything one attempt needs to execute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptSpec {
    pub run: RunSpec,
    pub event_seq_start: u64,
    pub resume: Option<ResumeSpec>,
}

/// Approval request produced by an attempt that paused for approval.
///
/// The preview is computed attempt-side because only the worker process can
/// see the run workspace filesystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequestPayload {
    pub kind: String,
    pub risk: String,
    pub summary: String,
    pub action_json: Option<serde_json::Value>,
    pub resume_messages_json: serde_json::Value,
    pub turns_completed: u32,
    pub preview_json: Option<serde_json::Value>,
}

/// Terminal report for one attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptOutcome {
    pub status: String,
    pub turns: u32,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub approval: Option<ApprovalRequestPayload>,
}

impl AttemptOutcome {
    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            status: "failed".to_string(),
            turns: 0,
            summary: None,
            error: Some(error.into()),
            approval: None,
        }
    }
}

/// Where an attempt persists run state. Implementations must preserve event
/// ordering within one attempt.
#[async_trait::async_trait]
pub trait AttemptBackend: Send + Sync + 'static {
    async fn mark_running(&self) -> anyhow::Result<()>;
    async fn ingest_event(&self, event: &AgentEvent) -> anyhow::Result<()>;
    async fn finish(&self, outcome: AttemptOutcome) -> anyhow::Result<()>;
}

/// Provider and tool wiring shared by every attempt in one worker process.
#[derive(Clone)]
pub struct AttemptHost {
    pub provider: FireworksClient,
    pub cluster_tools: ReadOnlyClusterTools,
    pub default_policy: SafetyPolicy,
}

pub fn run_status_str(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Completed => "completed",
        RunStatus::ApprovalRequired => "approval_required",
        RunStatus::Failed => "failed",
        RunStatus::Cancelled => "cancelled",
    }
}

pub fn policy_for_spec(
    run: &RunSpec,
    default_policy: &SafetyPolicy,
) -> anyhow::Result<SafetyPolicy> {
    let Some(policy_json) = run.execution_target_json.get("policy") else {
        return Ok(default_policy.clone());
    };

    serde_json::from_value(policy_json.clone())
        .map_err(|error| anyhow::anyhow!("run has invalid persisted policy: {error}"))
}

pub fn run_scope_for_spec(run: &RunSpec) -> RunScope {
    RunScope::from_execution_target(&run.execution_target_json).unwrap_or_default()
}

pub fn json_string<T>(value: T) -> String
where
    T: serde::Serialize,
{
    serde_json::to_value(value)
        .and_then(serde_json::from_value)
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Execute one run attempt against the given backend.
///
/// The backend's `finish` is called exactly once on the success path. Callers
/// that see an error from this function must report the failure themselves
/// (for example through [`AttemptOutcome::failed`]).
pub async fn execute_attempt<B: AttemptBackend>(
    host: AttemptHost,
    backend: Arc<B>,
    spec: AttemptSpec,
    cancellation: CancellationFlag,
) -> anyhow::Result<()> {
    backend.mark_running().await?;

    let (sender, receiver) = mpsc::unbounded_channel();
    let event_writer = tokio::spawn(forward_events(backend.clone(), receiver));
    let sink = ChannelEventSink { sender };

    let cwd = PathBuf::from(&spec.run.cwd);
    let tools = CompositeToolExecutor::new(
        CompositeToolExecutor::new(LocalReadOnlyFsTools::new(&cwd)?, host.cluster_tools),
        LocalShellTools::new(&cwd)?,
    );
    let runtime = AgentRuntime::with_tools(host.provider, sink, tools);

    let policy = policy_for_spec(&spec.run, &host.default_policy)?;
    let run_scope = run_scope_for_spec(&spec.run);
    let session_id = pharness_core::SessionId::new(spec.run.session_id.clone());
    let run_id = pharness_core::RunId::new(spec.run.run_id.clone());

    let outcome = match &spec.resume {
        None => {
            let config = RunConfig {
                session_id,
                run_id,
                messages: vec![
                    ModelMessage::system(system_prompt()),
                    ModelMessage::user(spec.run.user_task.clone()),
                ],
                tools: worker_tool_specs(),
                tool_protocol: ToolProtocolMode::NativeTools,
                temperature: 0.1,
                max_tokens: 4096,
                max_turns: spec.run.max_turns,
                policy,
                run_scope,
                event_seq_start: spec.event_seq_start,
            };
            runtime.run(config, cancellation).await
        }
        Some(resume) => {
            let approved = ApprovedAction {
                approval_id: resume.approval_id.clone(),
                action: serde_json::from_value(resume.action_json.clone())?,
                resume_messages: serde_json::from_value::<Vec<ModelMessage>>(
                    resume.resume_messages_json.clone(),
                )?,
                turns_completed: resume.turns_completed,
            };
            let config = RunConfig {
                session_id,
                run_id,
                messages: Vec::new(),
                tools: worker_tool_specs(),
                tool_protocol: ToolProtocolMode::NativeTools,
                temperature: 0.1,
                max_tokens: 4096,
                max_turns: spec.run.max_turns,
                policy,
                run_scope,
                event_seq_start: spec.event_seq_start,
            };
            runtime
                .resume_after_approval(config, cancellation, approved)
                .await
        }
    };

    drop(runtime);
    event_writer.await??;

    backend
        .finish(attempt_outcome(&spec.run.cwd, outcome)?)
        .await
}

fn attempt_outcome(cwd: &str, outcome: RunOutcome) -> anyhow::Result<AttemptOutcome> {
    let approval = if outcome.status == RunStatus::ApprovalRequired {
        match &outcome.approval {
            Some(approval) => {
                let preview_json = approval_preview_for_action(cwd, approval.action.as_ref());
                Some(ApprovalRequestPayload {
                    kind: json_string(approval.approval_kind),
                    risk: json_string(approval.risk),
                    summary: approval.summary.clone(),
                    action_json: approval
                        .action
                        .as_ref()
                        .map(serde_json::to_value)
                        .transpose()?,
                    resume_messages_json: serde_json::to_value(&approval.resume_messages)?,
                    turns_completed: approval.turns_completed,
                    preview_json,
                })
            }
            None => None,
        }
    } else {
        None
    };

    Ok(AttemptOutcome {
        status: run_status_str(outcome.status).to_string(),
        turns: outcome.turns,
        summary: outcome.summary,
        error: outcome.error,
        approval,
    })
}

async fn forward_events<B: AttemptBackend>(
    backend: Arc<B>,
    mut receiver: mpsc::UnboundedReceiver<AgentEvent>,
) -> anyhow::Result<()> {
    while let Some(event) = receiver.recv().await {
        backend.ingest_event(&event).await?;
    }

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
