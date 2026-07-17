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
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::process::Command;
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
    /// Source checkout instructions issued by the API for a bounded remote
    /// workspace. Model prompts and ambient environment variables cannot
    /// supply or alter this contract.
    pub workspace_source: Option<WorkspaceSourceSpec>,
}

/// Typed remote source checkout contract for one workspace attempt.
///
/// The API validates the repository against its configured allowlist before
/// issuing this spec. The worker validates the shape again before invoking
/// Git, providing defense in depth against malformed durable state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceSourceSpec {
    pub workspace_id: String,
    pub source_repo: String,
    pub source_ref: String,
    pub branch: String,
    /// Filled by the worker after checkout and before model execution.
    #[serde(default)]
    pub resolved_commit: Option<String>,
}

impl WorkspaceSourceSpec {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.workspace_id.trim().is_empty() {
            anyhow::bail!("workspace source workspace_id must not be blank");
        }
        validate_https_git_url(&self.source_repo)?;
        validate_git_ref(&self.source_ref, "source_ref")?;
        validate_git_ref(&self.branch, "branch")?;
        if let Some(commit) = &self.resolved_commit {
            validate_commit_id(commit)?;
        }
        Ok(())
    }
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
    #[serde(default)]
    pub workspace_evidence: Option<WorkspaceGitEvidence>,
}

/// Bounded Git evidence collected by the process that owns the workspace.
/// It is carried to the API with the terminal outcome because the API cannot
/// inspect a Kubernetes `emptyDir` after its Job exits.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceGitEvidence {
    pub workspace_id: String,
    pub base_commit: String,
    pub branch: String,
    pub status: String,
    pub diff: String,
    pub changed_paths: Vec<String>,
}

impl AttemptOutcome {
    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            status: "failed".to_string(),
            turns: 0,
            summary: None,
            error: Some(error.into()),
            approval: None,
            workspace_evidence: None,
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

fn validate_https_git_url(value: &str) -> anyhow::Result<()> {
    let value = value.trim();
    let Some(remainder) = value.strip_prefix("https://") else {
        anyhow::bail!("workspace source repository must use https");
    };
    if remainder.is_empty()
        || remainder.starts_with('/')
        || remainder.contains('@')
        || remainder.contains('?')
        || remainder.contains('#')
        || remainder
            .split('/')
            .any(|part| part.is_empty() || part == "..")
    {
        anyhow::bail!("workspace source repository is invalid or contains credentials");
    }
    Ok(())
}

fn validate_git_ref(value: &str, label: &str) -> anyhow::Result<()> {
    let value = value.trim();
    if value.is_empty()
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains("..")
        || value.contains("@{")
        || value.contains("//")
        || value.ends_with('.')
        || value.chars().any(|character| {
            !(character.is_ascii_alphanumeric() || matches!(character, '/' | '_' | '-' | '.'))
        })
    {
        anyhow::bail!("workspace source {label} is not a safe Git ref");
    }
    Ok(())
}

fn validate_commit_id(value: &str) -> anyhow::Result<()> {
    let value = value.trim();
    if !matches!(value.len(), 40 | 64)
        || !value.bytes().all(|character| character.is_ascii_hexdigit())
    {
        anyhow::bail!("workspace source resolved_commit is not an immutable Git object ID");
    }
    Ok(())
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
    if let Some(source) = &spec.run.workspace_source {
        source.validate()?;
    }
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
        .finish(attempt_outcome(&spec.run, outcome).await?)
        .await
}

async fn attempt_outcome(run: &RunSpec, outcome: RunOutcome) -> anyhow::Result<AttemptOutcome> {
    let approval = if outcome.status == RunStatus::ApprovalRequired {
        match &outcome.approval {
            Some(approval) => {
                let preview_json = approval_preview_for_action(&run.cwd, approval.action.as_ref());
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

    let status = run_status_str(outcome.status).to_string();
    let workspace_evidence = match (&run.workspace_source, status.as_str()) {
        (Some(source), "completed") => {
            Some(collect_workspace_git_evidence(&run.cwd, source).await?)
        }
        _ => None,
    };

    Ok(AttemptOutcome {
        status,
        turns: outcome.turns,
        summary: outcome.summary,
        error: outcome.error,
        approval,
        workspace_evidence,
    })
}

async fn collect_workspace_git_evidence(
    cwd: &str,
    source: &WorkspaceSourceSpec,
) -> anyhow::Result<WorkspaceGitEvidence> {
    let base_commit = source
        .resolved_commit
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("workspace source has no resolved commit"))?;
    let root = Path::new(cwd);
    let untracked = git_output(root, &["ls-files", "--others", "--exclude-standard"]).await?;
    let untracked_paths = nonempty_lines(&untracked);
    if untracked_paths.iter().any(|path| secret_shaped_path(path)) {
        anyhow::bail!("workspace contains an untracked secret-shaped path");
    }
    if !untracked_paths.is_empty() {
        let mut args = vec!["add", "--intent-to-add", "--"];
        args.extend(untracked_paths.iter().map(String::as_str));
        git_output(root, &args).await?;
    }
    let status = git_output(root, &["status", "--short"]).await?;
    let changed_paths =
        nonempty_lines(&git_output(root, &["diff", "--name-only", base_commit]).await?);
    if changed_paths.iter().any(|path| secret_shaped_path(path)) {
        anyhow::bail!("workspace diff includes a secret-shaped path");
    }
    let diff = git_output(root, &["diff", "--no-ext-diff", "--binary", base_commit]).await?;
    if diff.len() > 512 * 1024 {
        anyhow::bail!("workspace Git diff exceeds the 512 KiB capture limit");
    }
    Ok(WorkspaceGitEvidence {
        workspace_id: source.workspace_id.clone(),
        base_commit: base_commit.to_string(),
        branch: source.branch.clone(),
        status,
        diff,
        changed_paths,
    })
}

async fn git_output(cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .await
        .map_err(|error| anyhow::anyhow!("could not execute Git workspace command: {error}"))?;
    if !output.status.success() {
        anyhow::bail!("Git workspace evidence command failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn nonempty_lines(value: &str) -> Vec<String> {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn secret_shaped_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    name == ".env"
        || name.starts_with(".env.")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.contains("kubeconfig")
        || name.contains("credential")
        || name.contains("secret")
        || name.contains("token")
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

#[cfg(test)]
mod workspace_source_tests {
    use super::{collect_workspace_git_evidence, WorkspaceSourceSpec};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn accepts_a_safe_https_repository_and_refs() {
        WorkspaceSourceSpec {
            workspace_id: "ws_123".to_string(),
            source_repo: "https://github.com/example/finance-app.git".to_string(),
            source_ref: "main".to_string(),
            branch: "pharness/witem-123/attempt-1".to_string(),
            resolved_commit: None,
        }
        .validate()
        .unwrap();
    }

    #[test]
    fn rejects_credentials_and_unsafe_refs() {
        let mut source = WorkspaceSourceSpec {
            workspace_id: "ws_123".to_string(),
            source_repo: "https://token@example.test/team/app.git".to_string(),
            source_ref: "main".to_string(),
            branch: "pharness/witem-123/attempt-1".to_string(),
            resolved_commit: None,
        };
        assert!(source.validate().is_err());

        source.source_repo = "https://example.test/team/app.git".to_string();
        source.source_ref = "main..other".to_string();
        assert!(source.validate().is_err());

        source.source_ref = "main".to_string();
        source.resolved_commit = Some("a1b2c3d4".to_string());
        assert!(source.validate().is_err());
    }

    #[tokio::test]
    async fn collects_bounded_evidence_against_the_pinned_commit() {
        let root = std::env::temp_dir().join(format!(
            "pharness-runhost-evidence-{}-{}",
            std::process::id(),
            NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["config", "user.email", "test@example.invalid"]);
        std::fs::write(root.join("README.md"), "before\n").unwrap();
        git(&root, &["add", "README.md"]);
        git(&root, &["commit", "-m", "base"]);
        let base_commit = git(&root, &["rev-parse", "HEAD"]);
        std::fs::write(root.join("README.md"), "after\n").unwrap();

        let evidence = collect_workspace_git_evidence(
            root.to_str().unwrap(),
            &WorkspaceSourceSpec {
                workspace_id: "ws_test".to_string(),
                source_repo: "https://github.com/example/finance-app.git".to_string(),
                source_ref: "main".to_string(),
                branch: "pharness/test/attempt-1".to_string(),
                resolved_commit: Some(base_commit),
            },
        )
        .await
        .unwrap();

        assert_eq!(evidence.changed_paths, vec!["README.md"]);
        assert!(evidence.diff.contains("-before"));
        assert!(evidence.diff.contains("+after"));
        let _ = std::fs::remove_dir_all(root);
    }

    fn git(cwd: &std::path::Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }
}
