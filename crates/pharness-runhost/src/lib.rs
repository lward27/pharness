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
pub use prompt::{
    repo_system_prompt, stage_prompt_for_profile, system_prompt, tool_schema_hash_for_profile,
    worker_tool_specs, RELIABILITY_V2_PROMPT_BUNDLE_VERSION, SYSTEM_PROMPT_VERSION,
};

use pharness_core::{
    AgentEvent, AgentRuntime, ApprovedAction, BudgetResume, CancellationFlag,
    CompositeToolExecutor, ContextBudget, EnvironmentSnapshot, EventSink, LocalReadOnlyFsTools,
    LocalShellTools, ModelMessage, ModelProvider, ReadOnlyClusterTools, RecoveryPolicy,
    RepositoryContract, RepositoryInstruction, ResolvedInferenceBinding, RunBudget,
    RunBudgetConsumption, RunConfig, RunOutcome, RunScope, RunStatus, SafetyPolicy, TaskContract,
    ToolError, ToolExecutor, ToolProtocolMode, ToolResult,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
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
    #[serde(default)]
    pub task_contract: TaskContract,
    #[serde(default)]
    pub run_budget: Option<RunBudget>,
    #[serde(default)]
    pub budget_consumption: RunBudgetConsumption,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inference: Option<RunInferenceSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunInferenceSpec {
    pub selection_id: String,
    pub stage_execution_id: String,
    pub binding: ResolvedInferenceBinding,
    pub next_request_sequence: u32,
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
    /// Optional immutable object ID requested by the control plane. When set,
    /// the worker must check out this object rather than resolving source_ref.
    #[serde(default)]
    pub source_commit: Option<String>,
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
        if let Some(commit) = &self.source_commit {
            validate_commit_id(commit)?;
        }
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
    #[serde(default)]
    pub budget_resume: Option<BudgetResumeSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetResumeSpec {
    pub resume_messages_json: serde_json::Value,
    pub turns_completed: u32,
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
    #[serde(default)]
    pub budget_extension: Option<BudgetExtensionPayload>,
    #[serde(default)]
    pub consumption: RunBudgetConsumption,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetExtensionPayload {
    pub reason: String,
    pub resume_messages_json: serde_json::Value,
    pub turns_completed: u32,
    pub consumption: RunBudgetConsumption,
}

/// Bounded Git evidence collected by the process that owns the workspace.
/// It is carried to the API with the terminal outcome because the API cannot
/// directly inspect the isolated Kubernetes run workspace.
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
            budget_extension: None,
            consumption: RunBudgetConsumption::default(),
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
    pub provider: Arc<dyn ModelProvider>,
    pub cluster_tools: ReadOnlyClusterTools,
    pub default_policy: SafetyPolicy,
    pub context_budget: ContextBudget,
}

#[derive(Clone)]
struct ProfileRestrictedTools<T> {
    inner: T,
    allowed: Option<BTreeSet<String>>,
}

impl<T> ProfileRestrictedTools<T> {
    fn new(inner: T, allowed: Option<BTreeSet<String>>) -> Self {
        Self { inner, allowed }
    }
}

#[async_trait::async_trait]
impl<T: ToolExecutor> ToolExecutor for ProfileRestrictedTools<T> {
    async fn execute(&self, action: &pharness_core::AgentAction) -> Result<ToolResult, ToolError> {
        if self
            .allowed
            .as_ref()
            .is_some_and(|allowed| !allowed.contains(action.kind_name()))
        {
            return Err(ToolError::UnsupportedAction {
                action: action.kind_name().to_string(),
            });
        }
        self.inner.execute(action).await
    }
}

fn profile_tool_names(run: &RunSpec) -> anyhow::Result<Option<BTreeSet<String>>> {
    let Some(profile) = run.execution_target_json.get("agent_profile") else {
        return Ok(None);
    };
    let tools = profile
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("agent_profile has no tool allowlist"))?;
    let mut allowed = BTreeSet::new();
    for tool in tools {
        let name = tool
            .as_str()
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("agent_profile tool allowlist is invalid"))?;
        if !allowed.insert(name.to_string()) {
            anyhow::bail!("agent_profile tool allowlist contains duplicates");
        }
    }
    if allowed.is_empty() {
        anyhow::bail!("agent_profile tool allowlist is empty");
    }
    Ok(Some(allowed))
}

fn tool_specs_for_run(
    run: &RunSpec,
    allowed: Option<&BTreeSet<String>>,
) -> anyhow::Result<Vec<pharness_core::ToolSpec>> {
    let all = worker_tool_specs();
    let Some(allowed) = allowed else {
        return Ok(all);
    };
    let available = all
        .iter()
        .map(|spec| spec.name.as_str())
        .collect::<BTreeSet<_>>();
    let unknown = allowed
        .iter()
        .filter(|name| !available.contains(name.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        anyhow::bail!(
            "agent_profile requests unsupported tools: {}",
            unknown.join(", ")
        );
    }
    let filtered = all
        .into_iter()
        .filter(|spec| allowed.contains(&spec.name))
        .collect::<Vec<_>>();
    if filtered.is_empty() {
        anyhow::bail!("agent_profile exposes no tools");
    }
    let selected_commands = run
        .execution_target_json
        .get("selected_acceptance_commands")
        .cloned()
        .map(serde_json::from_value::<Vec<String>>)
        .transpose()?
        .unwrap_or_default();
    let contract = run
        .execution_target_json
        .get("repository_contract")
        .filter(|value| !value.is_null())
        .cloned()
        .map(serde_json::from_value::<RepositoryContract>)
        .transpose()?;
    let acceptance_names = contract
        .as_ref()
        .map(|contract| {
            contract
                .acceptance_commands
                .iter()
                .filter(|command| selected_commands.contains(&command.command))
                .map(|command| command.name.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let evidence_ids = run
        .execution_target_json
        .pointer("/agent_context/evidence_catalog")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("id").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .collect::<Vec<_>>();
    constrain_tool_specs(filtered, &acceptance_names, &evidence_ids)
}

fn constrain_tool_specs(
    mut specs: Vec<pharness_core::ToolSpec>,
    acceptance_names: &[String],
    evidence_ids: &[String],
) -> anyhow::Result<Vec<pharness_core::ToolSpec>> {
    for spec in &mut specs {
        match spec.name.as_str() {
            "run_acceptance_command" if !acceptance_names.is_empty() => {
                spec.parameters_schema["properties"]["name"]["enum"] =
                    serde_json::json!(acceptance_names);
            }
            "submit_implementation" if !acceptance_names.is_empty() => {
                spec.parameters_schema["properties"]["implementation"]["properties"]
                    ["acceptance_names"]["items"]["enum"] = serde_json::json!(acceptance_names);
            }
            "get_evidence" if !evidence_ids.is_empty() => {
                spec.parameters_schema["properties"]["evidence_id"]["enum"] =
                    serde_json::json!(evidence_ids);
            }
            _ => {}
        }
    }
    Ok(specs)
}

/// Hash the exact per-Run tool interface after controller-derived enum
/// constraints are applied. This is the value bound into reliability-v2
/// inference selections; a static profile allowlist is not sufficient because
/// acceptance names and evidence IDs are part of the executable interface.
pub fn constrained_tool_schema_hash(
    tool_names: &[String],
    acceptance_names: &[String],
    evidence_ids: &[String],
) -> anyhow::Result<String> {
    let allowed = tool_names.iter().cloned().collect::<BTreeSet<_>>();
    if allowed.len() != tool_names.len() || allowed.is_empty() {
        anyhow::bail!("tool allowlist is empty or contains duplicates");
    }
    let available = worker_tool_specs();
    let known = available
        .iter()
        .map(|spec| spec.name.as_str())
        .collect::<BTreeSet<_>>();
    let unknown = allowed
        .iter()
        .filter(|name| !known.contains(name.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        anyhow::bail!("profile requests unsupported tools: {}", unknown.join(", "));
    }
    let selected = available
        .into_iter()
        .filter(|spec| allowed.contains(&spec.name))
        .collect::<Vec<_>>();
    let constrained = constrain_tool_specs(selected, acceptance_names, evidence_ids)?;
    Ok(pharness_core::canonical_json_sha256(
        &serde_json::to_value(constrained)?,
    )?)
}

fn profile_instruction(run: &RunSpec) -> anyhow::Result<Option<String>> {
    let Some(profile) = run.execution_target_json.get("agent_profile") else {
        return Ok(None);
    };
    let id = profile
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("agent_profile has no id"))?;
    let is_reliability_v2 =
        profile.get("version").and_then(serde_json::Value::as_str) == Some("v2");
    // The stage is controller-owned execution state, not mutable profile
    // configuration. Legacy payloads may still carry it on the profile.
    let stage = profile
        .get("stage")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            run.execution_target_json
                .pointer("/repo_mode/stage")
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| {
            (id == "repository-onboarding-proposer"
                && run
                    .execution_target_json
                    .pointer("/onboarding/onboarding_id")
                    .and_then(serde_json::Value::as_str)
                    .is_some())
            .then_some("repository_onboarding")
        })
        .ok_or_else(|| anyhow::anyhow!("agent profile execution target has no stage"))?;
    let context = run
        .execution_target_json
        .get("agent_context")
        .ok_or_else(|| anyhow::anyhow!("agent_profile run has no AgentContext pack"))?;
    if context
        .get("schema_version")
        .and_then(serde_json::Value::as_str)
        != Some(pharness_core::AGENT_CONTEXT_SCHEMA)
    {
        anyhow::bail!("agent_profile run has an invalid AgentContext schema");
    }
    let serialized_context = serde_json::to_string_pretty(context)?;
    if serialized_context.len() / 4 > 16_000 {
        anyhow::bail!("agent_profile AgentContext exceeds the 16,000-token limit");
    }
    let profile_constraint = match id {
        "repository-onboarding-proposer" => {
            "\nRepository onboarding contract rule: candidate_contract.environment_profile must exactly copy one id from AgentContext.contract_constraints.compatible_environment_profiles and its dependency_lock.kind must match that descriptor's accepted_dependency_lock_kinds. Generic language names and shortened aliases are invalid."
        }
        "repo-verifier" if is_reliability_v2 => {
            "\nVerifier completion rule: begin with the sealed Test outcome, Git diff, and Git status. Read only files needed to investigate a concrete risk or contradiction; do not reread a path unless new evidence changed the question. You do not need to read every changed file when the diff and sealed evidence are sufficient. Preserve the final eight turns for the terminal submit_verification action and submit the typed verdict before the soft turn boundary."
        }
        "repo-verifier" => {
            "\nVerifier completion rule: begin with the sealed Test outcome, Git diff, and Git status. Read only files needed to investigate a concrete risk or contradiction; do not reread a path unless new evidence changed the question. You do not need to read every changed file when the diff and sealed evidence are sufficient. Preserve the final eight turns for submit_verification and finish, and submit the typed verdict before the soft turn boundary."
        }
        _ => "",
    };
    let completion_instruction = if is_reliability_v2 {
        "The typed stage submission is terminal; do not call finish after it."
    } else {
        "Submit the required typed stage document, then call finish."
    };
    Ok(Some(format!(
        "You are executing the immutable PHarness AgentProfile {id} for the {stage} stage. Use only the exposed tools. Treat verified facts as authoritative, keep agent claims explicitly separate, and retrieve only allowlisted evidence. {completion_instruction} You cannot authorize the next stage or declare controller success.{profile_constraint}\nAgentContext (controller-sealed, compact handoff):\n{serialized_context}"
    )))
}

fn reliability_v2_profile_id(run: &RunSpec) -> Option<&str> {
    let profile = run.execution_target_json.get("agent_profile")?;
    (profile.get("version").and_then(serde_json::Value::as_str) == Some("v2"))
        .then(|| profile.get("id").and_then(serde_json::Value::as_str))
        .flatten()
}

pub fn run_status_str(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Completed => "completed",
        RunStatus::ApprovalRequired => "approval_required",
        RunStatus::BudgetExtensionRequired => "budget_extension_required",
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

fn repository_instructions(cwd: &Path) -> anyhow::Result<(String, Vec<RepositoryInstruction>)> {
    const MAX_FILE_BYTES: usize = 16 * 1024;
    const MAX_TOTAL_BYTES: usize = 32 * 1024;
    let canonical_root = cwd.canonicalize()?;
    let mut total = 0usize;
    let mut sections = Vec::new();
    let mut files = Vec::new();
    for relative in ["AGENTS.md", "CLAUDE.md", ".pharness/instructions.md"] {
        let candidate = cwd.join(relative);
        if !candidate.exists() {
            continue;
        }
        let canonical = candidate.canonicalize().map_err(|error| {
            anyhow::anyhow!("failed to resolve repository instruction {relative}: {error}")
        })?;
        if !canonical.starts_with(&canonical_root) || secret_shaped_path(relative) {
            continue;
        }
        let content = std::fs::read_to_string(&canonical).map_err(|error| {
            anyhow::anyhow!("failed to read repository instruction {relative}: {error}")
        })?;
        let available = MAX_TOTAL_BYTES.saturating_sub(total);
        if available == 0 {
            break;
        }
        let bounded = truncate_utf8(&content, MAX_FILE_BYTES.min(available));
        total += bounded.len();
        files.push(RepositoryInstruction {
            filename: relative.to_string(),
            bytes: bounded.len(),
        });
        sections.push(format!(
            "Repository instructions from {relative}:\n{bounded}"
        ));
    }
    if sections.is_empty() {
        Ok((
            "No additional repository instructions were found.".to_string(),
            files,
        ))
    } else {
        Ok((sections.join("\n\n"), files))
    }
}

fn deterministic_repository_map(cwd: &Path, run: &RunSpec) -> anyhow::Result<String> {
    let head = std::process::Command::new("git")
        .args(["-C", cwd.to_string_lossy().as_ref(), "rev-parse", "HEAD"])
        .output()?;
    if !head.status.success() {
        anyhow::bail!("failed to resolve repository map source SHA");
    }
    let source_sha = String::from_utf8(head.stdout)?.trim().to_ascii_lowercase();
    if let Some(expected) = run
        .workspace_source
        .as_ref()
        .and_then(|source| source.source_commit.as_deref())
    {
        if !source_sha.eq_ignore_ascii_case(expected) {
            anyhow::bail!("repository map checkout SHA does not match the pinned source");
        }
    }
    let tracked = std::process::Command::new("git")
        .args(["-C", cwd.to_string_lossy().as_ref(), "ls-files", "-z"])
        .output()?;
    if !tracked.status.success() {
        anyhow::bail!("failed to enumerate tracked paths for repository map");
    }
    let mut paths = tracked
        .stdout
        .split(|byte| *byte == 0)
        .filter(|bytes| !bytes.is_empty())
        .map(|bytes| String::from_utf8(bytes.to_vec()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    if paths.len() > 20_000 {
        anyhow::bail!("repository map exceeds the 20,000 tracked-path limit");
    }
    let contract = run
        .execution_target_json
        .get("repository_contract")
        .filter(|value| !value.is_null())
        .cloned()
        .map(serde_json::from_value::<RepositoryContract>)
        .transpose()?;
    let mut manifests = Vec::new();
    let mut tests = Vec::new();
    let mut documentation = Vec::new();
    let mut symbols = Vec::new();
    let mut inspected_bytes = 0usize;
    for path in &paths {
        if secret_shaped_path(path) {
            continue;
        }
        let filename = path.rsplit('/').next().unwrap_or(path);
        if matches!(
            filename,
            "Cargo.toml"
                | "Cargo.lock"
                | "package.json"
                | "package-lock.json"
                | "pyproject.toml"
                | "requirements.txt"
                | "requirements.lock"
        ) {
            manifests.push(path.clone());
        }
        if contract.as_ref().is_some_and(|contract| {
            contract
                .roots
                .tests
                .iter()
                .any(|root| path == root || path.starts_with(&format!("{root}/")))
        }) || filename.contains("test")
        {
            tests.push(path.clone());
        }
        if contract.as_ref().is_some_and(|contract| {
            contract
                .roots
                .documentation
                .iter()
                .any(|root| path == root || path.starts_with(&format!("{root}/")))
        }) || matches!(
            filename.to_ascii_lowercase().as_str(),
            "readme.md" | "agents.md"
        ) {
            documentation.push(path.clone());
        }
        if symbols.len() >= 2_000 || inspected_bytes >= 4 * 1024 * 1024 {
            continue;
        }
        let inspectable = matches!(
            Path::new(path).extension().and_then(|value| value.to_str()),
            Some("rs" | "py" | "js" | "jsx" | "ts" | "tsx")
        );
        if !inspectable {
            continue;
        }
        let bytes = std::fs::read(cwd.join(path))?;
        if bytes.len() > 128 * 1024 || inspected_bytes.saturating_add(bytes.len()) > 4 * 1024 * 1024
        {
            continue;
        }
        let Ok(content) = std::str::from_utf8(&bytes) else {
            continue;
        };
        inspected_bytes += bytes.len();
        for (index, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            let (kind, name) = if let Some(rest) = trimmed.strip_prefix("fn ") {
                ("function", rest.split(['(', '<', ' ']).next())
            } else if let Some(rest) = trimmed.strip_prefix("pub fn ") {
                ("function", rest.split(['(', '<', ' ']).next())
            } else if let Some(rest) = trimmed.strip_prefix("def ") {
                ("function", rest.split(['(', ' ']).next())
            } else if let Some(rest) = trimmed.strip_prefix("class ") {
                ("class", rest.split(['(', ':', ' ']).next())
            } else if let Some(rest) = trimmed.strip_prefix("export function ") {
                ("function", rest.split(['(', ' ']).next())
            } else if let Some(rest) = trimmed.strip_prefix("export const ") {
                ("export", rest.split(['=', ' ', ':']).next())
            } else {
                continue;
            };
            if let Some(name) = name.filter(|name| !name.is_empty()) {
                symbols.push(serde_json::json!({
                    "path":path,
                    "line":index + 1,
                    "kind":kind,
                    "name":name,
                }));
                if symbols.len() >= 2_000 {
                    break;
                }
            }
        }
    }
    let mut map = serde_json::json!({
        "schema_version":"pharness.dev/repository-map/v1alpha1",
        "source_sha":source_sha,
        "tracked_paths":paths,
        "manifests":manifests,
        "test_files":tests,
        "documentation_files":documentation,
        "contract_roots":contract.as_ref().map(|contract| &contract.roots),
        "symbols":symbols,
        "limits":{"tracked_paths":20_000,"symbols":2_000,"inspected_text_bytes":4 * 1024 * 1024},
    });
    map["content_hash"] = serde_json::Value::String(pharness_core::canonical_json_sha256(&map)?);
    Ok(format!(
        "Controller deterministic repository map (authoritative at turn zero):\n{}",
        serde_json::to_string(&map)?
    ))
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    const MARKER: &str = "\n[instructions truncated]";
    if max_bytes <= MARKER.len() {
        return MARKER[..max_bytes].to_string();
    }
    let mut end = max_bytes - MARKER.len();
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{MARKER}", &value[..end])
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

    if spec
        .run
        .execution_target_json
        .pointer("/repo_mode/deterministic_test")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return execute_deterministic_test(backend, &spec.run, spec.event_seq_start, cancellation)
            .await;
    }

    let (sender, receiver) = mpsc::unbounded_channel();
    let event_writer = tokio::spawn(forward_events(backend.clone(), receiver));
    let sink = ChannelEventSink { sender };

    let cwd = PathBuf::from(&spec.run.cwd);
    let tools = CompositeToolExecutor::new(
        ProjectTools::for_run(&cwd, &spec.run)?,
        CompositeToolExecutor::new(
            CompositeToolExecutor::new(LocalReadOnlyFsTools::new(&cwd)?, host.cluster_tools),
            LocalShellTools::new(&cwd)?,
        ),
    );
    let allowed_tools = profile_tool_names(&spec.run)?;
    let run_tool_specs = tool_specs_for_run(&spec.run, allowed_tools.as_ref())?;
    if reliability_v2_profile_id(&spec.run).is_some() {
        let binding = spec
            .run
            .inference
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("reliability-v2 Run has no inference binding"))?;
        let actual_tool_schema_hash =
            pharness_core::canonical_json_sha256(&serde_json::to_value(&run_tool_specs)?)?;
        if actual_tool_schema_hash != binding.binding.tool_schema_hash {
            anyhow::bail!(
                "resolved inference binding tool schema does not match the exact Run tool interface"
            );
        }
    }
    let tools = ProfileRestrictedTools::new(tools, allowed_tools.clone());
    let runtime = AgentRuntime::with_tools(host.provider, sink, tools);

    let policy = policy_for_spec(&spec.run, &host.default_policy)?;
    let run_scope = run_scope_for_spec(&spec.run);
    let session_id = pharness_core::SessionId::new(spec.run.session_id.clone());
    let run_id = pharness_core::RunId::new(spec.run.run_id.clone());

    let recovery_policy = spec
        .run
        .run_budget
        .as_ref()
        .map(|budget| RecoveryPolicy {
            max_recoverable_errors: budget.recoverable_tool_errors,
            max_identical_failures: budget.identical_failures,
        })
        .unwrap_or_default();
    let tool_protocol = spec
        .run
        .inference
        .as_ref()
        .map(|inference| inference.binding.policy.tool_protocol)
        .unwrap_or(ToolProtocolMode::NativeTools);
    let tool_choice = spec
        .run
        .inference
        .as_ref()
        .map(|inference| inference.binding.policy.tool_choice)
        .unwrap_or(pharness_core::ToolChoiceMode::Required);
    let temperature = spec
        .run
        .inference
        .as_ref()
        .and_then(|inference| inference.binding.policy.temperature())
        .unwrap_or(0.1);
    let maximum_output_tokens = spec
        .run
        .inference
        .as_ref()
        .map(|inference| inference.binding.policy.max_output_tokens)
        .unwrap_or(4096);
    let context_budget = context_budget_for_run(&host.context_budget, spec.run.inference.as_ref());
    let outcome = match (&spec.resume, &spec.budget_resume) {
        (None, None) => {
            let (repository_instruction_content, repository_instruction_files) =
                repository_instructions(&cwd)?;
            let environment_content = environment_instructions(&spec.run)?;
            let reliability_profile = reliability_v2_profile_id(&spec.run);
            let mut messages = vec![
                ModelMessage::system(if reliability_profile.is_some() {
                    repo_system_prompt()
                } else {
                    system_prompt()
                }),
                ModelMessage::system(repository_instruction_content),
                ModelMessage::system(environment_content),
            ];
            if let Some(profile_id) = reliability_profile {
                messages.push(ModelMessage::system(deterministic_repository_map(
                    &cwd, &spec.run,
                )?));
                let stage_prompt = stage_prompt_for_profile(profile_id).ok_or_else(|| {
                    anyhow::anyhow!(
                        "reliability-v2 AgentProfile {profile_id} has no immutable stage prompt"
                    )
                })?;
                let revision = stage_prompt.revision_record();
                if let Some(expected) = spec
                    .run
                    .inference
                    .as_ref()
                    .and_then(|inference| inference.binding.stage_prompt.as_ref())
                {
                    if expected != &revision {
                        anyhow::bail!(
                            "resolved inference binding stage prompt does not match the runtime prompt"
                        );
                    }
                }
                messages.push(ModelMessage::system(format!(
                    "Stage prompt {}@{} ({}):\n{}",
                    stage_prompt.prompt_id,
                    stage_prompt.revision,
                    revision.content_hash,
                    stage_prompt.content
                )));
            }
            if let Some(instruction) = profile_instruction(&spec.run)? {
                messages.push(ModelMessage::system(instruction));
            }
            messages.push(ModelMessage::user(spec.run.user_task.clone()));
            let config = RunConfig {
                session_id,
                run_id,
                messages,
                tools: run_tool_specs.clone(),
                tool_protocol,
                tool_choice,
                temperature,
                max_tokens: maximum_output_tokens,
                max_turns: spec.run.max_turns,
                context_budget: context_budget.clone(),
                recovery_policy: recovery_policy.clone(),
                task_contract: spec.run.task_contract.clone(),
                repository_instruction_files,
                policy,
                run_scope,
                event_seq_start: spec.event_seq_start,
                run_budget: spec.run.run_budget.clone(),
                budget_consumption: spec.run.budget_consumption.clone(),
                inference_binding: spec
                    .run
                    .inference
                    .as_ref()
                    .map(|value| value.binding.clone()),
            };
            runtime.run(config, cancellation).await
        }
        (Some(resume), None) => {
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
                tools: run_tool_specs.clone(),
                tool_protocol,
                tool_choice,
                temperature,
                max_tokens: maximum_output_tokens,
                max_turns: spec.run.max_turns,
                context_budget: context_budget.clone(),
                recovery_policy: recovery_policy.clone(),
                task_contract: spec.run.task_contract.clone(),
                repository_instruction_files: Vec::new(),
                policy,
                run_scope,
                event_seq_start: spec.event_seq_start,
                run_budget: spec.run.run_budget.clone(),
                budget_consumption: spec.run.budget_consumption.clone(),
                inference_binding: spec
                    .run
                    .inference
                    .as_ref()
                    .map(|value| value.binding.clone()),
            };
            runtime
                .resume_after_approval(config, cancellation, approved)
                .await
        }
        (None, Some(resume)) => {
            let resume = BudgetResume {
                resume_messages: serde_json::from_value::<Vec<ModelMessage>>(
                    resume.resume_messages_json.clone(),
                )?,
                turns_completed: resume.turns_completed,
            };
            let config = RunConfig {
                session_id,
                run_id,
                messages: Vec::new(),
                tools: run_tool_specs,
                tool_protocol,
                tool_choice,
                temperature,
                max_tokens: maximum_output_tokens,
                max_turns: spec.run.max_turns,
                context_budget,
                recovery_policy,
                task_contract: spec.run.task_contract.clone(),
                repository_instruction_files: Vec::new(),
                policy,
                run_scope,
                event_seq_start: spec.event_seq_start,
                run_budget: spec.run.run_budget.clone(),
                budget_consumption: spec.run.budget_consumption.clone(),
                inference_binding: spec
                    .run
                    .inference
                    .as_ref()
                    .map(|value| value.binding.clone()),
            };
            runtime
                .resume_after_budget(config, cancellation, resume)
                .await
        }
        (Some(_), Some(_)) => {
            anyhow::bail!("attempt cannot resume approval and budget simultaneously")
        }
    };

    drop(runtime);
    event_writer.await??;

    backend
        .finish(attempt_outcome(&spec.run, outcome).await?)
        .await
}

fn context_budget_for_run(
    host_default: &ContextBudget,
    inference: Option<&RunInferenceSpec>,
) -> ContextBudget {
    let Some(inference) = inference else {
        return host_default.clone();
    };
    let max_input_tokens = inference.binding.policy.max_input_tokens;
    let reserved_output_tokens = inference.binding.policy.max_output_tokens;
    let usable_input = max_input_tokens.saturating_sub(reserved_output_tokens);
    ContextBudget {
        max_input_tokens,
        reserved_output_tokens,
        // Keep a deterministic margin for immutable intent, repository map,
        // tool schemas, and the execution ledger before retaining history.
        recent_message_tokens: usable_input.saturating_sub(8_192).max(8_192),
        max_tool_result_tokens: host_default.max_tool_result_tokens.max(4_096),
        characters_per_token: host_default.characters_per_token,
    }
}

async fn execute_deterministic_test<B: AttemptBackend>(
    backend: Arc<B>,
    run: &RunSpec,
    event_seq_start: u64,
    cancellation: CancellationFlag,
) -> anyhow::Result<()> {
    let started = std::time::Instant::now();
    let project = ProjectTools::for_run(Path::new(&run.cwd), run)?;
    let contract = project
        .contract
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("deterministic Test requires a RepositoryContract"))?;
    let selected = contract
        .acceptance_commands
        .iter()
        .filter(|command| {
            project
                .selected_acceptance_commands
                .iter()
                .any(|selected| selected == &command.command)
        })
        .map(|command| command.name.clone())
        .collect::<Vec<_>>();
    if selected.is_empty() || selected.len() != project.selected_acceptance_commands.len() {
        anyhow::bail!(
            "deterministic Test selection does not exactly match declared acceptance commands"
        );
    }

    let session_id = pharness_core::SessionId::new(run.session_id.clone());
    let run_id = pharness_core::RunId::new(run.run_id.clone());
    let mut sequence = event_seq_start;
    let mut passed = true;
    for name in selected {
        if cancellation.is_cancelled() {
            return backend
                .finish(AttemptOutcome {
                    status: "cancelled".into(),
                    turns: 0,
                    summary: Some("deterministic Test cancelled".into()),
                    error: Some("cancelled".into()),
                    approval: None,
                    workspace_evidence: None,
                    budget_extension: None,
                    consumption: RunBudgetConsumption {
                        active_execution_seconds_used: started.elapsed().as_secs(),
                        ..RunBudgetConsumption::default()
                    },
                })
                .await;
        }
        sequence += 1;
        backend
            .ingest_event(&AgentEvent {
                event_id: pharness_core::EventId::new(format!(
                    "evt_{}_deterministic_{sequence}",
                    run.run_id
                )),
                session_id: session_id.clone(),
                run_id: run_id.clone(),
                seq: sequence,
                kind: pharness_core::EventKind::ToolStarted,
                payload: serde_json::json!({
                    "action":"run_acceptance_command",
                    "name":name,
                    "origin":"controller",
                }),
            })
            .await?;
        let result = match project.run_acceptance(&name).await {
            Ok(result) => result,
            Err(error) => ToolResult::error(
                "deterministic acceptance command could not execute",
                serde_json::json!({
                    "acceptance_command":true,
                    "name":name,
                    "error_kind":error.kind_name(),
                    "message":error.to_string(),
                }),
            ),
        };
        passed &= result.status == pharness_core::ToolResultStatus::Ok
            && result
                .content
                .get("exit_code")
                .and_then(serde_json::Value::as_i64)
                == Some(0);
        sequence += 1;
        backend
            .ingest_event(&AgentEvent {
                event_id: pharness_core::EventId::new(format!(
                    "evt_{}_deterministic_{sequence}",
                    run.run_id
                )),
                session_id: session_id.clone(),
                run_id: run_id.clone(),
                seq: sequence,
                kind: pharness_core::EventKind::ToolFinished,
                payload: serde_json::to_value(&result)?,
            })
            .await?;
    }
    sequence += 1;
    backend
        .ingest_event(&AgentEvent {
            event_id: pharness_core::EventId::new(format!(
                "evt_{}_deterministic_{sequence}",
                run.run_id
            )),
            session_id,
            run_id,
            seq: sequence,
            kind: pharness_core::EventKind::RunFinished,
            payload: serde_json::json!({
                "success":passed,
                "origin":"controller",
                "stop_category":if passed {"acceptance_passed"} else {"acceptance_failure"},
            }),
        })
        .await?;

    let outcome = RunOutcome {
        // The controller seals the Test StageOutcome from exact command
        // events. A failing command is a completed deterministic execution,
        // not a model-runtime failure.
        status: RunStatus::Completed,
        turns: 0,
        summary: Some(if passed {
            "all deterministic acceptance commands passed".into()
        } else {
            "one or more deterministic acceptance commands failed".into()
        }),
        error: None,
        approval: None,
        budget_pause: None,
        consumption: RunBudgetConsumption {
            active_execution_seconds_used: started.elapsed().as_secs(),
            ..RunBudgetConsumption::default()
        },
    };
    backend.finish(attempt_outcome(run, outcome).await?).await
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
    let budget_extension = match outcome.budget_pause.as_ref() {
        Some(pause) => Some(BudgetExtensionPayload {
            reason: pause.reason.clone(),
            resume_messages_json: serde_json::to_value(&pause.resume_messages)?,
            turns_completed: pause.turns_completed,
            consumption: pause.consumption.clone(),
        }),
        None => None,
    };
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
        budget_extension,
        consumption: outcome.consumption,
    })
}

fn environment_instructions(run: &RunSpec) -> anyhow::Result<String> {
    let Some(snapshot) = run.execution_target_json.get("environment_snapshot") else {
        return Ok("No durable environment snapshot is available. Do not assume package installation or network access; use exposed tools only.".to_string());
    };
    let snapshot: pharness_core::EnvironmentSnapshot = serde_json::from_value(snapshot.clone())?;
    let contract = run
        .execution_target_json
        .get("repository_contract")
        .cloned()
        .map(serde_json::from_value::<pharness_core::RepositoryContract>)
        .transpose()?
        .ok_or_else(|| anyhow::anyhow!("prepared run has no repository contract"))?;
    Ok(format!(
        "Environment readiness snapshot and RepositoryContract were verified before turn zero. Treat these as authoritative; do not rediscover runtime, Docker, package-manager, operating-system, or network facts and do not request runtime package installation. Agent shell network is denied. Use only declared acceptance commands through the typed acceptance tool.\nEnvironmentSnapshot:\n{}\nRepositoryContract:\n{}",
        serde_json::to_string_pretty(&snapshot)?,
        serde_json::to_string_pretty(&contract)?,
    ))
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
    let diff = git_output_raw(root, &["diff", "--no-ext-diff", "--binary", base_commit]).await?;
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
    Ok(git_output_raw(cwd, args).await?.trim().to_string())
}

async fn git_output_raw(cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
    let command_args = git_evidence_args(cwd, args);
    let output = Command::new("git")
        .args(&command_args)
        .output()
        .await
        .map_err(|error| anyhow::anyhow!("could not execute Git workspace command: {error}"))?;
    if !output.status.success() {
        anyhow::bail!("Git workspace evidence command failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_evidence_args(cwd: &Path, args: &[&str]) -> Vec<String> {
    let mut command_args = vec![
        "-c".to_string(),
        format!("safe.directory={}", cwd.display()),
        "-C".to_string(),
        cwd.display().to_string(),
    ];
    command_args.extend(args.iter().map(|arg| (*arg).to_string()));
    command_args
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

#[derive(Debug, Clone)]
struct ProjectTools {
    workspace: PathBuf,
    canonical_workspace: PathBuf,
    workspace_files: LocalReadOnlyFsTools,
    contract: Option<RepositoryContract>,
    snapshot: Option<EnvironmentSnapshot>,
    selected_acceptance_commands: Vec<String>,
    evidence_catalog: Vec<serde_json::Value>,
    evidence_payloads: Vec<serde_json::Value>,
    onboarding_discovery: Option<(String, String)>,
}

impl ProjectTools {
    fn for_run(workspace: &Path, run: &RunSpec) -> anyhow::Result<Self> {
        let contract = run
            .execution_target_json
            .get("repository_contract")
            .filter(|value| !value.is_null())
            .cloned()
            .map(serde_json::from_value)
            .transpose()?;
        let snapshot = run
            .execution_target_json
            .get("environment_snapshot")
            .filter(|value| !value.is_null())
            .cloned()
            .map(serde_json::from_value)
            .transpose()?;
        let selected_acceptance_commands = run
            .execution_target_json
            .get("selected_acceptance_commands")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        let evidence_catalog = run
            .execution_target_json
            .pointer("/agent_context/evidence_catalog")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        let evidence_payloads = run
            .execution_target_json
            .get("agent_evidence_payloads")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        let onboarding_discovery = run
            .execution_target_json
            .pointer("/agent_context/discovery")
            .and_then(|discovery| {
                Some((
                    discovery.get("id")?.as_str()?.to_string(),
                    discovery.get("hash")?.as_str()?.to_string(),
                ))
            });
        Ok(Self {
            workspace: workspace.to_path_buf(),
            canonical_workspace: workspace.canonicalize()?,
            workspace_files: LocalReadOnlyFsTools::new(workspace)?,
            contract,
            snapshot,
            selected_acceptance_commands,
            evidence_catalog,
            evidence_payloads,
            onboarding_discovery,
        })
    }

    fn writable_path(&self, value: &camino::Utf8Path) -> Result<PathBuf, ToolError> {
        let path = value.as_str();
        if path.is_empty()
            || path.starts_with('/')
            || path.split('/').any(|part| part.is_empty() || part == "..")
            || secret_shaped_project_path(path)
        {
            return Err(ToolError::OutsideWorkspace {
                path: path.to_string(),
            });
        }
        let contract = self
            .contract
            .as_ref()
            .ok_or_else(|| ToolError::UnsupportedAction {
                action: "create_directory".to_string(),
            })?;
        if !contract
            .writable_paths
            .iter()
            .any(|pattern| project_path_glob_matches(pattern, path))
        {
            return Err(ToolError::OutsideWorkspace {
                path: path.to_string(),
            });
        }
        Ok(self.workspace.join(path))
    }

    async fn run_acceptance(&self, name: &str) -> Result<ToolResult, ToolError> {
        tokio::fs::create_dir_all(self.workspace.join(".pharness-runtime/home"))
            .await
            .map_err(|error| ToolError::Io {
                message: error.to_string(),
            })?;
        let contract = self
            .contract
            .as_ref()
            .ok_or_else(|| ToolError::UnsupportedAction {
                action: "run_acceptance_command".to_string(),
            })?;
        let declared = contract
            .command(name)
            .ok_or_else(|| ToolError::InvalidArguments {
                message: format!("acceptance command is not declared: {name}"),
            })?;
        if !self
            .selected_acceptance_commands
            .iter()
            .any(|command| command == &declared.command)
        {
            return Err(ToolError::InvalidArguments {
                message: format!("acceptance command was not selected by the WorkItem: {name}"),
            });
        }
        let existing_path = std::env::var("PATH").unwrap_or_default();
        let runtime = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.runtime.as_ref());
        let mut path_entries = runtime
            .map(|runtime| runtime.path_entries.clone())
            .unwrap_or_else(|| {
                vec![self
                    .workspace
                    .join(".pharness-runtime/venv/bin")
                    .to_string_lossy()
                    .to_string()]
            });
        path_entries.push(existing_path);
        let mut child = Command::new("/bin/sh");
        child
            .env_clear()
            .arg("-c")
            .arg(&declared.command)
            .current_dir(&self.workspace)
            .env("PATH", path_entries.join(":"))
            .env("HOME", self.workspace.join(".pharness-runtime/home"))
            .env("LANG", "C.UTF-8")
            .kill_on_drop(true);
        if runtime.is_some_and(|runtime| runtime.kind == "node") {
            for (key, value) in node_acceptance_environment(&self.workspace) {
                child.env(key, value);
            }
        }
        if runtime.is_some_and(|runtime| runtime.kind == "rust") {
            child.env("CARGO_NET_OFFLINE", "true").env(
                "CARGO_TARGET_DIR",
                self.workspace.join(".pharness-runtime/cargo-target"),
            );
        }
        if match runtime {
            Some(runtime) => runtime.kind == "python",
            None => true,
        } {
            let python_path = contract
                .roots
                .source
                .iter()
                .map(|root| self.workspace.join(root).to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join(":");
            child.env("PYTHONPATH", python_path);
        }
        let started = std::time::Instant::now();
        let output = tokio::time::timeout(std::time::Duration::from_secs(600), child.output())
            .await
            .map_err(|_| ToolError::TimedOut {
                command: declared.command.clone(),
                timeout_ms: 600_000,
            })?
            .map_err(|error| ToolError::Io {
                message: error.to_string(),
            })?;
        let content = serde_json::json!({
            "acceptance_command": true,
            "name": name,
            "command": declared.command,
            "exit_code": output.status.code(),
            "stdout": bounded_output(&output.stdout),
            "stderr": bounded_output(&output.stderr),
            "duration_ms": started.elapsed().as_millis(),
        });
        if output.status.success() {
            Ok(ToolResult::ok(
                format!("acceptance command {name} passed"),
                content,
            ))
        } else {
            Ok(ToolResult::error(
                format!("acceptance command {name} failed"),
                content,
            ))
        }
    }

    async fn apply_unified_patch(
        &self,
        patch: &str,
        preimage_sha256: &BTreeMap<String, String>,
    ) -> Result<ToolResult, ToolError> {
        if patch.is_empty() || patch.len() > 256 * 1024 || patch.contains('\0') {
            return Err(ToolError::InvalidArguments {
                message: "unified patch must contain between one byte and 256 KiB".into(),
            });
        }
        let paths = unified_diff_paths(patch)?;
        for path in &paths {
            let utf8 = camino::Utf8Path::new(path);
            let _ = self.writable_path(utf8)?;
            if self.workspace.join(path).is_file() && !preimage_sha256.contains_key(path) {
                return Err(ToolError::InvalidArguments {
                    message: format!("patch requires a preimage SHA-256 for existing path {path}"),
                });
            }
        }
        for (path, expected) in preimage_sha256 {
            if !paths.iter().any(|candidate| candidate == path)
                || !expected.strip_prefix("sha256:").is_some_and(|hash| {
                    hash.len() == 64
                        && hash
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                })
            {
                return Err(ToolError::InvalidArguments {
                    message: format!("invalid patch preimage binding for {path}"),
                });
            }
            let bytes = tokio::fs::read(self.workspace.join(path))
                .await
                .map_err(|error| ToolError::Io {
                    message: error.to_string(),
                })?;
            let actual = format!("sha256:{:x}", Sha256::digest(&bytes));
            if &actual != expected {
                return Err(ToolError::InvalidArguments {
                    message: format!("patch preimage changed for {path}"),
                });
            }
        }
        run_git_apply(&self.workspace, patch, true).await?;
        run_git_apply(&self.workspace, patch, false).await?;
        let mut after_sha256 = BTreeMap::new();
        for path in &paths {
            let target = self.workspace.join(path);
            let hash = match tokio::fs::read(&target).await {
                Ok(bytes) => Some(format!("sha256:{:x}", Sha256::digest(bytes))),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(ToolError::Io {
                        message: error.to_string(),
                    })
                }
            };
            after_sha256.insert(path.clone(), hash);
        }
        Ok(ToolResult::ok(
            format!("atomically applied patch to {} path(s)", paths.len()),
            serde_json::json!({
                "diff":"changed",
                "changed_paths":paths,
                "after_sha256":after_sha256,
                "patch_sha256":format!("sha256:{:x}", Sha256::digest(patch.as_bytes())),
            }),
        ))
    }

    async fn run_workspace_command(
        &self,
        executable: &str,
        args: &[String],
        cwd: Option<&camino::Utf8Path>,
        timeout_ms: Option<u64>,
    ) -> Result<ToolResult, ToolError> {
        tokio::fs::create_dir_all(self.workspace.join(".pharness-runtime/home"))
            .await
            .map_err(|error| ToolError::Io {
                message: error.to_string(),
            })?;
        validate_workspace_command(executable, args)?;
        let runtime = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.runtime.as_ref())
            .ok_or_else(|| ToolError::UnsupportedAction {
                action: "run_workspace_command".into(),
            })?;
        let executable_path = resolve_workspace_executable(runtime, executable)?;
        let command_cwd = resolve_command_cwd(&self.workspace, &self.canonical_workspace, cwd)?;
        let existing_path = std::env::var("PATH").unwrap_or_default();
        let mut path_entries = runtime.path_entries.clone();
        path_entries.push(existing_path);
        let mut command = Command::new(&executable_path);
        command
            .args(args)
            .current_dir(&command_cwd)
            .env_clear()
            .env("PATH", path_entries.join(":"))
            .env("HOME", self.workspace.join(".pharness-runtime/home"))
            .env("LANG", "C.UTF-8")
            .kill_on_drop(true);
        if runtime.kind == "node" {
            for (key, value) in node_acceptance_environment(&self.workspace) {
                command.env(key, value);
            }
        }
        if runtime.kind == "python" {
            let python_path = self
                .contract
                .as_ref()
                .map(|contract| {
                    contract
                        .roots
                        .source
                        .iter()
                        .map(|root| self.workspace.join(root).to_string_lossy().to_string())
                        .collect::<Vec<_>>()
                        .join(":")
                })
                .unwrap_or_default();
            command.env("PYTHONPATH", python_path);
        }
        if executable == "cargo" {
            command.env("CARGO_NET_OFFLINE", "true").env(
                "CARGO_TARGET_DIR",
                self.workspace.join(".pharness-runtime/cargo-target"),
            );
        }
        let timeout_ms = timeout_ms.unwrap_or(120_000).clamp(100, 120_000);
        let started = std::time::Instant::now();
        let output = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            command.output(),
        )
        .await
        .map_err(|_| ToolError::TimedOut {
            command: format!("{executable} {}", args.join(" ")),
            timeout_ms,
        })?
        .map_err(|error| ToolError::Io {
            message: error.to_string(),
        })?;
        let content = serde_json::json!({
            "workspace_command":true,
            "executable":executable,
            "args":args,
            "cwd":cwd.map(camino::Utf8Path::as_str).unwrap_or("."),
            "exit_code":output.status.code(),
            "stdout":bounded_output(&output.stdout),
            "stderr":bounded_output(&output.stderr),
            "stdout_sha256":format!("sha256:{:x}",Sha256::digest(&output.stdout)),
            "stderr_sha256":format!("sha256:{:x}",Sha256::digest(&output.stderr)),
            "artifact_content":{
                "stdout":bounded_artifact_output(&output.stdout),
                "stderr":bounded_artifact_output(&output.stderr),
                "stdout_bytes":output.stdout.len(),
                "stderr_bytes":output.stderr.len(),
                "artifact_limit_bytes":2 * 1024 * 1024,
                "stdout_truncated":output.stdout.len() > 2 * 1024 * 1024,
                "stderr_truncated":output.stderr.len() > 2 * 1024 * 1024,
            },
            "duration_ms":started.elapsed().as_millis(),
        });
        if output.status.success() {
            Ok(ToolResult::ok("workspace command passed", content))
        } else {
            Ok(ToolResult::error("workspace command failed", content))
        }
    }
}

fn unified_diff_paths(patch: &str) -> Result<Vec<String>, ToolError> {
    let mut paths = Vec::new();
    let mut header_paths = Vec::new();
    for line in patch.lines().filter(|line| line.starts_with("diff --git ")) {
        let mut parts = line.split_whitespace();
        let _ = parts.next();
        let _ = parts.next();
        let old = parts.next().unwrap_or_default();
        let new = parts.next().unwrap_or_default();
        if parts.next().is_some()
            || !old.starts_with("a/")
            || !new.starts_with("b/")
            || old[2..] != new[2..]
        {
            return Err(ToolError::InvalidArguments {
                message: "patch contains an unsupported or renamed path".into(),
            });
        }
        let path = new[2..].to_string();
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    for line in patch.lines() {
        let candidate = line
            .strip_prefix("--- a/")
            .or_else(|| line.strip_prefix("+++ b/"));
        if let Some(path) = candidate {
            let path = path.split('\t').next().unwrap_or(path).to_string();
            if !header_paths.contains(&path) {
                header_paths.push(path);
            }
        }
    }
    if paths.is_empty() || paths.len() > 200 {
        return Err(ToolError::InvalidArguments {
            message: "patch must contain between one and 200 standard diff paths".into(),
        });
    }
    if header_paths.iter().any(|path| !paths.contains(path))
        || paths.iter().any(|path| !header_paths.contains(path))
    {
        return Err(ToolError::InvalidArguments {
            message: "patch file headers do not exactly match its declared diff paths".into(),
        });
    }
    Ok(paths)
}

async fn run_git_apply(workspace: &Path, patch: &str, check_only: bool) -> Result<(), ToolError> {
    let mut command = Command::new("git");
    command
        .arg("-c")
        .arg(format!("safe.directory={}", workspace.display()))
        .arg("-C")
        .arg(workspace)
        .arg("apply")
        .arg("--whitespace=nowarn");
    if check_only {
        command.arg("--check");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| ToolError::Io {
            message: error.to_string(),
        })?;
    child
        .stdin
        .take()
        .ok_or_else(|| ToolError::Io {
            message: "git apply stdin is unavailable".into(),
        })?
        .write_all(patch.as_bytes())
        .await
        .map_err(|error| ToolError::Io {
            message: error.to_string(),
        })?;
    let output = child
        .wait_with_output()
        .await
        .map_err(|error| ToolError::Io {
            message: error.to_string(),
        })?;
    if !output.status.success() {
        return Err(ToolError::InvalidArguments {
            message: format!(
                "patch {} failed: {}",
                if check_only {
                    "validation"
                } else {
                    "application"
                },
                bounded_output(&output.stderr)
            ),
        });
    }
    Ok(())
}

fn resolve_command_cwd(
    workspace: &Path,
    canonical_workspace: &Path,
    cwd: Option<&camino::Utf8Path>,
) -> Result<PathBuf, ToolError> {
    let relative = cwd.map(camino::Utf8Path::as_str).unwrap_or(".");
    if relative.starts_with('/')
        || relative
            .split('/')
            .any(|part| part == ".." || part.is_empty())
        || secret_shaped_project_path(relative)
    {
        return Err(ToolError::OutsideWorkspace {
            path: relative.into(),
        });
    }
    let path = workspace.join(relative);
    let canonical = path.canonicalize().map_err(|error| ToolError::Io {
        message: error.to_string(),
    })?;
    if !canonical.starts_with(canonical_workspace) {
        return Err(ToolError::OutsideWorkspace {
            path: relative.into(),
        });
    }
    Ok(canonical)
}

fn resolve_workspace_executable(
    runtime: &pharness_core::EnvironmentRuntimeSnapshot,
    requested: &str,
) -> Result<String, ToolError> {
    let runtime_name = Path::new(&runtime.executable)
        .file_name()
        .and_then(|value| value.to_str());
    let package_manager_name = runtime
        .package_manager_executable
        .as_deref()
        .and_then(|value| Path::new(value).file_name())
        .and_then(|value| value.to_str());
    let allowed = runtime_name == Some(requested)
        || package_manager_name == Some(requested)
        || matches!(
            (runtime.kind.as_str(), requested),
            ("python", "python")
                | ("python", "python3")
                | ("python", "pytest")
                | ("node", "node")
                | ("node", "npm")
                | ("rust", "cargo")
                | ("rust", "rustc")
        );
    if !allowed || requested.contains('/') {
        return Err(ToolError::UnsupportedAction {
            action: format!("run_workspace_command:{requested}"),
        });
    }
    if runtime_name == Some(requested) {
        Ok(runtime.executable.clone())
    } else if package_manager_name == Some(requested) {
        Ok(runtime
            .package_manager_executable
            .clone()
            .expect("package manager name was present"))
    } else {
        Ok(requested.to_string())
    }
}

fn validate_workspace_command(executable: &str, args: &[String]) -> Result<(), ToolError> {
    if executable.is_empty()
        || executable.len() > 256
        || args.len() > 128
        || args.iter().any(|arg| {
            arg.len() > 4096
                || arg.contains('\0')
                || arg.contains('\n')
                || [";", "&&", "||", "`", "$(`", ">", "<"]
                    .iter()
                    .any(|token| arg.contains(token))
        })
    {
        return Err(ToolError::InvalidArguments {
            message: "workspace command contains shell composition or exceeds its bounds".into(),
        });
    }
    let denied = match executable {
        "pip" | "pip3" => true,
        "npm" => args.first().is_some_and(|arg| {
            matches!(
                arg.as_str(),
                "ci" | "install"
                    | "i"
                    | "add"
                    | "update"
                    | "uninstall"
                    | "publish"
                    | "exec"
                    | "init"
                    | "login"
                    | "logout"
                    | "owner"
                    | "token"
            )
        }),
        "python" | "python3" => {
            args.windows(2)
                .any(|pair| pair[0] == "-m" && matches!(pair[1].as_str(), "pip" | "ensurepip"))
                || contains_inline_program(args)
        }
        "node" => contains_inline_program(args),
        "cargo" => args.first().is_some_and(|arg| {
            matches!(
                arg.as_str(),
                "add"
                    | "fetch"
                    | "install"
                    | "login"
                    | "logout"
                    | "owner"
                    | "publish"
                    | "remove"
                    | "search"
                    | "update"
                    | "vendor"
                    | "yank"
            )
        }),
        _ => false,
    };
    if denied {
        return Err(ToolError::UnsupportedAction {
            action: format!("run_workspace_command:{executable}:package_or_network"),
        });
    }
    Ok(())
}

fn contains_inline_program(args: &[String]) -> bool {
    // Inline programs are not a useful inspection boundary: they can write
    // arbitrary files or construct a network client without a recognizable
    // command token. Repository reads belong to the typed list/read/search
    // tools; executable repository tests belong here.
    args.iter()
        .any(|arg| matches!(arg.as_str(), "-c" | "-e" | "--eval" | "--input-type"))
}

fn node_acceptance_environment(workspace: &Path) -> Vec<(&'static str, String)> {
    let runtime_root = workspace.join(".pharness-runtime");
    vec![
        (
            "NPM_CONFIG_CACHE",
            runtime_root.join("npm-cache").to_string_lossy().to_string(),
        ),
        ("NPM_CONFIG_UPDATE_NOTIFIER", "false".into()),
        ("NPM_CONFIG_AUDIT", "false".into()),
        ("NPM_CONFIG_FUND", "false".into()),
        ("NPM_CONFIG_OFFLINE", "true".into()),
        (
            "HOME",
            runtime_root.join("home").to_string_lossy().to_string(),
        ),
        (
            "XDG_CACHE_HOME",
            runtime_root.join("cache").to_string_lossy().to_string(),
        ),
    ]
}

#[async_trait::async_trait]
impl ToolExecutor for ProjectTools {
    async fn execute(&self, action: &pharness_core::AgentAction) -> Result<ToolResult, ToolError> {
        match action {
            pharness_core::AgentAction::EnvironmentInfo { .. } => {
                let (Some(snapshot), Some(contract)) = (&self.snapshot, &self.contract) else {
                    return Err(ToolError::UnsupportedAction {
                        action: action.kind_name().to_string(),
                    });
                };
                Ok(ToolResult::ok(
                    "returned durable environment information",
                    serde_json::json!({
                        "environment_snapshot": snapshot,
                        "project_contract": contract,
                    }),
                ))
            }
            pharness_core::AgentAction::CreateDirectory { path, .. } => {
                let target = self.writable_path(path)?;
                tokio::fs::create_dir_all(&target)
                    .await
                    .map_err(|error| ToolError::Io {
                        message: error.to_string(),
                    })?;
                let canonical = target.canonicalize().map_err(|error| ToolError::Io {
                    message: error.to_string(),
                })?;
                if !canonical.starts_with(&self.canonical_workspace) {
                    return Err(ToolError::OutsideWorkspace {
                        path: path.to_string(),
                    });
                }
                Ok(ToolResult::ok(
                    format!("created directory {path}"),
                    serde_json::json!({ "path": path }),
                ))
            }
            pharness_core::AgentAction::WriteFile { path, .. }
            | pharness_core::AgentAction::PatchFile { path, .. }
                if self.contract.is_some() =>
            {
                let _ = self.writable_path(path)?;
                self.workspace_files.execute(action).await
            }
            pharness_core::AgentAction::ApplyPatch {
                patch,
                preimage_sha256,
                ..
            } => self.apply_unified_patch(patch, preimage_sha256).await,
            pharness_core::AgentAction::RunAcceptanceCommand { name, .. } => {
                self.run_acceptance(name).await
            }
            pharness_core::AgentAction::RunWorkspaceCommand {
                executable,
                args,
                cwd,
                timeout_ms,
                ..
            } => {
                self.run_workspace_command(executable, args, cwd.as_deref(), *timeout_ms)
                    .await
            }
            pharness_core::AgentAction::GetEvidence { evidence_id, .. } => {
                let catalog_entry = self
                    .evidence_catalog
                    .iter()
                    .find(|entry| {
                        entry.get("id").and_then(serde_json::Value::as_str)
                            == Some(evidence_id.as_str())
                    })
                    .ok_or_else(|| ToolError::InvalidArguments {
                        message: "evidence_id is outside the context-pack allowlist".into(),
                    })?;
                let evidence = self
                    .evidence_payloads
                    .iter()
                    .find(|entry| {
                        entry.get("id").and_then(serde_json::Value::as_str)
                            == Some(evidence_id.as_str())
                    })
                    .ok_or_else(|| ToolError::InvalidArguments {
                        message: "allowlisted evidence payload is unavailable".into(),
                    })?;
                let payload =
                    evidence
                        .get("payload")
                        .ok_or_else(|| ToolError::InvalidArguments {
                            message: "allowlisted evidence payload is malformed".into(),
                        })?;
                let returned_hash =
                    pharness_core::canonical_json_sha256(payload).map_err(|error| {
                        ToolError::InvalidArguments {
                            message: format!(
                                "allowlisted evidence payload cannot be hashed: {error}"
                            ),
                        }
                    })?;
                if catalog_entry
                    .get("hash")
                    .and_then(serde_json::Value::as_str)
                    != Some(returned_hash.as_str())
                    || evidence.get("hash").and_then(serde_json::Value::as_str)
                        != Some(returned_hash.as_str())
                {
                    return Err(ToolError::InvalidArguments {
                        message:
                            "allowlisted evidence payload hash does not match the context pack"
                                .into(),
                    });
                }
                Ok(ToolResult::ok(
                    format!("returned allowlisted evidence {evidence_id}"),
                    serde_json::json!({
                        "evidence_id": evidence_id,
                        "evidence_kind":catalog_entry.get("kind"),
                        "evidence_version":catalog_entry.get("version"),
                        "returned_hash":returned_hash,
                        "evidence": payload,
                    }),
                ))
            }
            pharness_core::AgentAction::SubmitOnboardingProposal { proposal, .. } => {
                onboarding_submission(self, proposal)
            }
            pharness_core::AgentAction::SubmitWorkPlan { work_plan, .. } => {
                structured_submission("work_plan", work_plan)
            }
            pharness_core::AgentAction::SubmitImplementation { implementation, .. } => {
                structured_submission("implementation", implementation)
            }
            pharness_core::AgentAction::SubmitTestOutcome { outcome, .. } => {
                structured_submission("test_outcome", outcome)
            }
            pharness_core::AgentAction::SubmitTestDiagnosis { diagnosis, .. } => {
                structured_submission("test_diagnosis", diagnosis)
            }
            pharness_core::AgentAction::SubmitVerification { verification, .. } => {
                structured_submission("verification", verification)
            }
            _ => Err(ToolError::UnsupportedAction {
                action: action.kind_name().to_string(),
            }),
        }
    }
}

fn onboarding_submission(
    tools: &ProjectTools,
    document: &serde_json::Value,
) -> Result<ToolResult, ToolError> {
    let proposal: pharness_core::RepositoryOnboardingProposal =
        serde_json::from_value(document.clone()).map_err(|error| ToolError::InvalidArguments {
            message: format!("repository onboarding proposal is invalid: {error}"),
        })?;
    if proposal.schema_version != pharness_core::ONBOARDING_PROPOSAL_SCHEMA {
        return Err(ToolError::InvalidArguments {
            message: "repository onboarding proposal has the wrong schema_version".into(),
        });
    }
    let Some((discovery_id, discovery_hash)) = &tools.onboarding_discovery else {
        return Err(ToolError::InvalidArguments {
            message: "repository onboarding proposal has no controller-bound discovery".into(),
        });
    };
    if &proposal.discovery_id != discovery_id || &proposal.discovery_hash != discovery_hash {
        return Err(ToolError::InvalidArguments {
            message: "repository onboarding proposal does not match its controller-bound discovery"
                .into(),
        });
    }
    let contract: RepositoryContract = serde_json::from_value(proposal.candidate_contract.clone())
        .map_err(|error| ToolError::InvalidArguments {
            message: format!("candidate repository contract is invalid: {error}"),
        })?;
    contract
        .validate_candidate()
        .map_err(|error| ToolError::InvalidArguments {
            message: error.to_string(),
        })?;
    structured_submission("repository_onboarding_proposal", document)
}

fn structured_submission(
    kind: &str,
    document: &serde_json::Value,
) -> Result<ToolResult, ToolError> {
    let object = document
        .as_object()
        .filter(|object| !object.is_empty())
        .ok_or_else(|| ToolError::InvalidArguments {
            message: format!("{kind} must be a non-empty JSON object"),
        })?;
    if object.len() > 128 || document.to_string().len() > 128 * 1024 {
        return Err(ToolError::InvalidArguments {
            message: format!("{kind} exceeds the structured submission limit"),
        });
    }
    Ok(ToolResult::ok(
        format!("accepted typed {kind} for controller validation"),
        serde_json::json!({
            "structured_submission": true,
            "kind": kind,
            "document": document,
        }),
    ))
}

fn project_path_glob_matches(pattern: &str, path: &str) -> bool {
    pattern
        .strip_suffix("/**")
        .map(|prefix| path == prefix || path.starts_with(&format!("{prefix}/")))
        .unwrap_or(pattern == path)
}

fn secret_shaped_project_path(path: &str) -> bool {
    path.to_ascii_lowercase().split('/').any(|part| {
        part == ".env"
            || part.starts_with(".env.")
            || part.ends_with(".pem")
            || part.ends_with(".key")
            || part.contains("secret")
            || part.contains("credential")
            || part.contains("token")
            || part.contains("kubeconfig")
    })
}

fn bounded_output(bytes: &[u8]) -> String {
    const MAX: usize = 64 * 1024;
    let text = String::from_utf8_lossy(bytes);
    if text.len() <= MAX {
        return text.into_owned();
    }
    let mut end = MAX;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...[truncated]", &text[..end])
}

fn bounded_artifact_output(bytes: &[u8]) -> String {
    const MAX: usize = 2 * 1024 * 1024;
    let bytes = &bytes[..bytes.len().min(MAX)];
    String::from_utf8_lossy(bytes).into_owned()
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
    use super::{
        collect_workspace_git_evidence, constrained_tool_schema_hash, git_evidence_args,
        node_acceptance_environment, profile_instruction, profile_tool_names,
        repository_instructions, tool_specs_for_run, validate_workspace_command,
        LocalReadOnlyFsTools, ProfileRestrictedTools, ProjectTools, RunSpec, WorkspaceSourceSpec,
    };
    use pharness_core::{
        AcceptanceCommand, ActionId, AgentAction, AgentNetworkPolicy, DependencyLock,
        EnvironmentRuntimeSnapshot, EnvironmentSnapshot, PackageInstallationPolicy, ProjectRoots,
        RepositoryContract, RunBudgetConsumption, TaskContract, ToolError, ToolExecutor,
        ToolResult,
    };
    use std::collections::BTreeSet;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(0);

    fn project_tools(root: &std::path::Path, writable_paths: &[&str]) -> ProjectTools {
        ProjectTools {
            workspace: root.to_path_buf(),
            canonical_workspace: root.canonicalize().unwrap(),
            workspace_files: LocalReadOnlyFsTools::new(root).unwrap(),
            contract: Some(RepositoryContract {
                api_version: "pharness.dev/v1alpha1".into(),
                environment_profile: "test".into(),
                dependency_lock: DependencyLock {
                    kind: "pip_requirements".into(),
                    path: "requirements.lock".into(),
                    sha256: "a".repeat(64),
                },
                writable_paths: writable_paths.iter().map(|value| (*value).into()).collect(),
                acceptance_commands: vec![AcceptanceCommand {
                    name: "unit".into(),
                    command: "echo test".into(),
                }],
                roots: ProjectRoots {
                    source: vec!["src".into()],
                    tests: vec!["tests".into()],
                    documentation: vec!["README.md".into()],
                },
                agent_network: AgentNetworkPolicy::Denied,
                package_installation: PackageInstallationPolicy::PreparationOnly,
            }),
            snapshot: Some(EnvironmentSnapshot {
                source_sha: "a".repeat(40),
                manifest_sha256: "b".repeat(64),
                dependency_lock_sha256: "a".repeat(64),
                runner_image_digest: format!("sha256:{}", "c".repeat(64)),
                runner_revision: "d".repeat(40),
                os: "linux".into(),
                architecture: "amd64".into(),
                effective_user: "65532".into(),
                runtime: Some(EnvironmentRuntimeSnapshot {
                    kind: "test".into(),
                    executable: "/bin/echo".into(),
                    version: "test".into(),
                    package_manager_executable: None,
                    package_manager_version: None,
                    path_entries: vec!["/bin".into()],
                }),
                python_version: None,
                python_path: None,
                writable_paths: writable_paths.iter().map(|value| (*value).into()).collect(),
                unavailable_tools: Vec::new(),
                agent_network: AgentNetworkPolicy::Denied,
                package_installation: PackageInstallationPolicy::PreparationOnly,
                acceptance_commands: vec![AcceptanceCommand {
                    name: "unit".into(),
                    command: "echo test".into(),
                }],
                preparation_evidence: serde_json::json!({}),
            }),
            selected_acceptance_commands: vec!["echo test".into()],
            evidence_catalog: Vec::new(),
            evidence_payloads: Vec::new(),
            onboarding_discovery: None,
        }
    }

    fn sha256(value: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        format!("sha256:{:x}", Sha256::digest(value))
    }

    #[test]
    fn node_acceptance_keeps_all_runtime_state_under_pharness_runtime() {
        let environment = node_acceptance_environment(std::path::Path::new("/workspace"))
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();

        assert_eq!(
            environment.get("NPM_CONFIG_CACHE").map(String::as_str),
            Some("/workspace/.pharness-runtime/npm-cache")
        );
        assert_eq!(
            environment.get("HOME").map(String::as_str),
            Some("/workspace/.pharness-runtime/home")
        );
        assert_eq!(
            environment.get("XDG_CACHE_HOME").map(String::as_str),
            Some("/workspace/.pharness-runtime/cache")
        );
        assert_eq!(
            environment
                .get("NPM_CONFIG_UPDATE_NOTIFIER")
                .map(String::as_str),
            Some("false")
        );
        assert_eq!(
            environment.get("NPM_CONFIG_OFFLINE").map(String::as_str),
            Some("true")
        );
        assert!(environment
            .values()
            .filter(|value| value.starts_with('/'))
            .all(|value| value.starts_with("/workspace/.pharness-runtime/")));
    }

    #[tokio::test]
    async fn atomic_patch_requires_preimages_and_never_partially_applies() {
        let root = std::env::temp_dir().join(format!(
            "pharness-atomic-patch-{}-{}",
            std::process::id(),
            NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/one.txt"), "one\n").unwrap();
        std::fs::write(root.join("src/two.txt"), "two\n").unwrap();
        let tools = project_tools(&root, &["src/**"]);
        let valid_patch = "diff --git a/src/one.txt b/src/one.txt\n--- a/src/one.txt\n+++ b/src/one.txt\n@@ -1 +1 @@\n-one\n+ONE\n";

        assert!(matches!(
            tools.apply_unified_patch(valid_patch, &std::collections::BTreeMap::new()).await,
            Err(ToolError::InvalidArguments { message }) if message.contains("preimage")
        ));
        assert_eq!(
            std::fs::read_to_string(root.join("src/one.txt")).unwrap(),
            "one\n"
        );

        let invalid_second_hunk = "diff --git a/src/one.txt b/src/one.txt\n--- a/src/one.txt\n+++ b/src/one.txt\n@@ -1 +1 @@\n-one\n+ONE\ndiff --git a/src/two.txt b/src/two.txt\n--- a/src/two.txt\n+++ b/src/two.txt\n@@ -1 +1 @@\n-missing\n+TWO\n";
        let hashes = std::collections::BTreeMap::from([
            ("src/one.txt".into(), sha256(b"one\n")),
            ("src/two.txt".into(), sha256(b"two\n")),
        ]);
        assert!(tools
            .apply_unified_patch(invalid_second_hunk, &hashes)
            .await
            .is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("src/one.txt")).unwrap(),
            "one\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("src/two.txt")).unwrap(),
            "two\n"
        );

        let result = tools
            .apply_unified_patch(
                valid_patch,
                &std::collections::BTreeMap::from([("src/one.txt".into(), sha256(b"one\n"))]),
            )
            .await
            .unwrap();
        assert_eq!(
            result.content["changed_paths"],
            serde_json::json!(["src/one.txt"])
        );
        assert_eq!(
            std::fs::read_to_string(root.join("src/one.txt")).unwrap(),
            "ONE\n"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn repository_contract_paths_are_enforced_by_the_workspace_executor() {
        let root = std::env::temp_dir().join(format!(
            "pharness-contract-write-{}-{}",
            std::process::id(),
            NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("src")).unwrap();
        let tools = project_tools(&root, &["src/**"]);

        let allowed = AgentAction::WriteFile {
            id: ActionId::new("write_allowed"),
            reason: "write declared source".into(),
            path: camino::Utf8PathBuf::from("src/lib.rs"),
            content: "pub fn ready() {}\n".into(),
        };
        tools.execute(&allowed).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("src/lib.rs")).unwrap(),
            "pub fn ready() {}\n"
        );

        let denied = AgentAction::WriteFile {
            id: ActionId::new("write_denied"),
            reason: "attempt undeclared toolchain override".into(),
            path: camino::Utf8PathBuf::from("rust-toolchain.toml"),
            content: "[toolchain]\nchannel = \"stable\"\n".into(),
        };
        assert!(matches!(
            tools.execute(&denied).await,
            Err(ToolError::OutsideWorkspace { path }) if path == "rust-toolchain.toml"
        ));
        assert!(!root.join("rust-toolchain.toml").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_command_rejects_package_network_and_shell_composition() {
        assert!(validate_workspace_command("npm", &["install".into()]).is_err());
        assert!(validate_workspace_command("pip", &["list".into()]).is_err());
        assert!(validate_workspace_command("pip3", &["--version".into()]).is_err());
        assert!(validate_workspace_command(
            "python",
            &[
                "-c".into(),
                "print('even non-network inline programs are denied')".into()
            ]
        )
        .is_err());
        assert!(validate_workspace_command(
            "node",
            &["--eval".into(), "console.log('denied')".into()]
        )
        .is_err());
        assert!(validate_workspace_command("cargo", &["vendor".into()]).is_err());
        assert!(
            validate_workspace_command("python", &["-m".into(), "pytest && id".into()]).is_err()
        );
        assert!(validate_workspace_command("python", &["-m".into(), "pytest".into()]).is_ok());
    }

    #[tokio::test]
    async fn workspace_command_executes_only_the_snapshot_runtime() {
        let root = std::env::temp_dir().join(format!(
            "pharness-workspace-command-{}-{}",
            std::process::id(),
            NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let tools = project_tools(&root, &["src/**"]);
        let result = tools
            .run_workspace_command("echo", &["bounded".into()], None, Some(5_000))
            .await
            .unwrap();
        assert_eq!(result.content["stdout"], "bounded\n");
        assert_eq!(result.content["artifact_content"]["stdout"], "bounded\n");
        assert_eq!(result.content["artifact_content"]["stdout_bytes"], 8);
        assert_eq!(
            result.content["artifact_content"]["stdout_truncated"],
            false
        );
        assert!(result.content["stdout_sha256"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:")));
        assert!(matches!(
            tools.run_workspace_command("curl", &[], None, None).await,
            Err(ToolError::UnsupportedAction { .. })
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[derive(Clone)]
    struct AcceptingTools;

    #[async_trait::async_trait]
    impl ToolExecutor for AcceptingTools {
        async fn execute(&self, action: &AgentAction) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::ok(
                format!("executed {}", action.kind_name()),
                serde_json::json!({}),
            ))
        }
    }

    fn profile_run(tools: &[&str]) -> RunSpec {
        RunSpec {
            run_id: "run_profile".into(),
            session_id: "ses_profile".into(),
            cwd: "/tmp/workspace".into(),
            user_task: "plan a bounded change".into(),
            max_turns: 24,
            execution_target_json: serde_json::json!({
                "repo_mode": {"stage": "plan"},
                "agent_profile": {
                    "id": "repo-planner",
                    "tools": tools,
                }
            }),
            workspace_source: None,
            task_contract: TaskContract::default(),
            run_budget: None,
            budget_consumption: RunBudgetConsumption::default(),
            inference: None,
        }
    }

    #[test]
    fn profile_tool_specs_expose_only_the_pinned_allowlist() {
        let run = profile_run(&["get_evidence", "submit_work_plan", "finish"]);
        let allowed = profile_tool_names(&run).unwrap().unwrap();
        let names = tool_specs_for_run(&run, Some(&allowed))
            .unwrap()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<BTreeSet<_>>();

        assert_eq!(
            names,
            BTreeSet::from([
                "finish".to_string(),
                "get_evidence".to_string(),
                "submit_work_plan".to_string(),
            ])
        );
    }

    #[test]
    fn reliability_v2_tool_hash_binds_exact_acceptance_and_evidence_ids() {
        let tool_names = vec![
            "get_evidence".to_string(),
            "run_acceptance_command".to_string(),
            "submit_implementation".to_string(),
        ];
        let acceptance_names = vec!["unit".to_string(), "compile".to_string()];
        let evidence_ids = vec!["evidence_contract".to_string()];
        let expected =
            constrained_tool_schema_hash(&tool_names, &acceptance_names, &evidence_ids).unwrap();

        assert_ne!(
            expected,
            constrained_tool_schema_hash(&tool_names, &["different".to_string()], &evidence_ids,)
                .unwrap()
        );
        assert_ne!(
            expected,
            constrained_tool_schema_hash(
                &tool_names,
                &acceptance_names,
                &["evidence_diff".to_string()],
            )
            .unwrap()
        );

        let mut run = profile_run(&[
            "get_evidence",
            "run_acceptance_command",
            "submit_implementation",
        ]);
        run.execution_target_json["selected_acceptance_commands"] =
            serde_json::json!(["python -m unittest", "python -m compileall src"]);
        run.execution_target_json["repository_contract"] =
            serde_json::to_value(RepositoryContract {
                api_version: "pharness.dev/v1alpha1".into(),
                environment_profile: "python-3.11".into(),
                dependency_lock: DependencyLock {
                    kind: "pip_requirements".into(),
                    path: "requirements.lock".into(),
                    sha256: "a".repeat(64),
                },
                writable_paths: vec!["src/**".into(), "tests/**".into()],
                acceptance_commands: vec![
                    AcceptanceCommand {
                        name: "unit".into(),
                        command: "python -m unittest".into(),
                    },
                    AcceptanceCommand {
                        name: "compile".into(),
                        command: "python -m compileall src".into(),
                    },
                ],
                roots: ProjectRoots {
                    source: vec!["src".into()],
                    tests: vec!["tests".into()],
                    documentation: vec!["README.md".into()],
                },
                agent_network: AgentNetworkPolicy::Denied,
                package_installation: PackageInstallationPolicy::PreparationOnly,
            })
            .unwrap();
        run.execution_target_json["agent_context"] = serde_json::json!({
            "evidence_catalog": [{"id": "evidence_contract"}],
        });
        let allowed = profile_tool_names(&run).unwrap().unwrap();
        let actual_specs = tool_specs_for_run(&run, Some(&allowed)).unwrap();
        let actual =
            pharness_core::canonical_json_sha256(&serde_json::to_value(&actual_specs).unwrap())
                .unwrap();

        assert_eq!(actual, expected);
        let acceptance = actual_specs
            .iter()
            .find(|tool| tool.name == "run_acceptance_command")
            .unwrap();
        assert_eq!(
            acceptance.parameters_schema["properties"]["name"]["enum"],
            serde_json::json!(["unit", "compile"])
        );
        let evidence = actual_specs
            .iter()
            .find(|tool| tool.name == "get_evidence")
            .unwrap();
        assert_eq!(
            evidence.parameters_schema["properties"]["evidence_id"]["enum"],
            serde_json::json!(["evidence_contract"])
        );
    }

    #[test]
    fn verifier_profile_preserves_submission_turns_and_avoids_redundant_reads() {
        let mut run = profile_run(&[
            "get_evidence",
            "read_file",
            "git_diff",
            "git_status",
            "submit_verification",
            "finish",
        ]);
        run.execution_target_json["repo_mode"]["stage"] = serde_json::json!("verify");
        run.execution_target_json["agent_profile"]["id"] = serde_json::json!("repo-verifier");
        run.execution_target_json["agent_context"] = serde_json::json!({
            "schema_version": pharness_core::AGENT_CONTEXT_SCHEMA,
            "current_intent": {"title": "verify a bounded change"},
            "evidence_catalog": [],
        });

        let instruction = profile_instruction(&run).unwrap().unwrap();
        assert!(
            instruction.contains("begin with the sealed Test outcome, Git diff, and Git status")
        );
        assert!(instruction.contains("do not reread a path"));
        assert!(instruction.contains("Preserve the final eight turns"));
        assert!(instruction.contains("submit the typed verdict before the soft turn boundary"));
    }

    #[test]
    fn reliability_v2_verifier_uses_terminal_submission_without_finish() {
        let mut run = profile_run(&[
            "get_evidence",
            "read_file",
            "git_diff",
            "git_status",
            "submit_verification",
        ]);
        run.execution_target_json["repo_mode"]["stage"] = serde_json::json!("verify");
        run.execution_target_json["agent_profile"]["id"] = serde_json::json!("repo-verifier");
        run.execution_target_json["agent_profile"]["version"] = serde_json::json!("v2");
        run.execution_target_json["agent_context"] = serde_json::json!({
            "schema_version": pharness_core::AGENT_CONTEXT_SCHEMA,
            "current_intent": {"title": "verify a bounded change"},
            "evidence_catalog": [],
        });

        let instruction = profile_instruction(&run).unwrap().unwrap();
        assert!(instruction.contains("terminal submit_verification"));
        assert!(instruction.contains("do not call finish after it"));
        assert!(!instruction.contains("submit_verification and finish"));
    }

    #[tokio::test]
    async fn profile_executor_rejects_tools_outside_the_pinned_allowlist() {
        let restricted = ProfileRestrictedTools::new(
            AcceptingTools,
            Some(BTreeSet::from(["get_evidence".to_string()])),
        );
        let allowed = AgentAction::GetEvidence {
            id: ActionId::new("act_evidence"),
            reason: "inspect evidence".into(),
            evidence_id: "ev_1".into(),
        };
        assert!(restricted.execute(&allowed).await.is_ok());

        let denied = AgentAction::EnvironmentInfo {
            id: ActionId::new("act_environment"),
            reason: "probe environment".into(),
        };
        assert!(matches!(
            restricted.execute(&denied).await,
            Err(ToolError::UnsupportedAction { action }) if action == "environment_info"
        ));
    }

    #[tokio::test]
    async fn profile_context_is_injected_and_evidence_retrieval_is_hash_bound() {
        let payload = serde_json::json!({
            "schema_version":pharness_core::STAGE_OUTCOME_SCHEMA,
            "stage":"discover",
            "status":"succeeded",
            "verified_facts":[{"source_commit":"a".repeat(40)}],
        });
        let hash = pharness_core::canonical_json_sha256(&payload).unwrap();
        let mut run = profile_run(&["get_evidence", "finish"]);
        run.cwd = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string();
        run.execution_target_json["agent_context"] = serde_json::json!({
            "schema_version":pharness_core::AGENT_CONTEXT_SCHEMA,
            "current_intent":{"title":"bounded change"},
            "evidence_catalog":[{
                "id":"stageout_discover",
                "kind":"stage_outcome",
                "version":pharness_core::STAGE_OUTCOME_SCHEMA,
                "hash":hash,
            }],
        });
        run.execution_target_json["agent_evidence_payloads"] = serde_json::json!([{
            "id":"stageout_discover",
            "kind":"stage_outcome",
            "version":pharness_core::STAGE_OUTCOME_SCHEMA,
            "hash":hash,
            "payload":payload,
        }]);

        let instruction = profile_instruction(&run).unwrap().unwrap();
        assert!(instruction.contains("AgentContext (controller-sealed, compact handoff)"));
        assert!(instruction.contains("stageout_discover"));

        let tools = ProjectTools::for_run(std::path::Path::new(&run.cwd), &run).unwrap();
        let result = tools
            .execute(&AgentAction::GetEvidence {
                id: ActionId::new("act_get_evidence"),
                reason: "inspect sealed discovery facts".into(),
                evidence_id: "stageout_discover".into(),
            })
            .await
            .unwrap();
        assert_eq!(result.content["returned_hash"], hash);
        assert_eq!(result.content["evidence"]["stage"], "discover");

        let mut tampered = run;
        tampered.execution_target_json["agent_evidence_payloads"][0]["payload"]["status"] =
            serde_json::json!("failed");
        let tools = ProjectTools::for_run(std::path::Path::new(&tampered.cwd), &tampered).unwrap();
        assert!(matches!(
            tools
                .execute(&AgentAction::GetEvidence {
                    id: ActionId::new("act_tampered"),
                    reason: "inspect sealed discovery facts".into(),
                    evidence_id: "stageout_discover".into(),
                })
                .await,
            Err(ToolError::InvalidArguments { .. })
        ));
    }

    #[tokio::test]
    async fn onboarding_profile_uses_its_controller_owned_subject_stage() {
        let mut run = profile_run(&["read_file", "submit_onboarding_proposal", "finish"]);
        run.cwd = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string();
        run.execution_target_json
            .as_object_mut()
            .unwrap()
            .remove("repo_mode");
        run.execution_target_json["agent_profile"]["id"] =
            serde_json::json!("repository-onboarding-proposer");
        run.execution_target_json["onboarding"] = serde_json::json!({"onboarding_id":"ronb_test"});
        run.execution_target_json["agent_context"] = serde_json::json!({
            "schema_version":pharness_core::AGENT_CONTEXT_SCHEMA,
            "subject":{"kind":"repository_onboarding","id":"ronb_test"},
            "discovery":{"id":"rdisc_test","hash":format!("sha256:{}", "a".repeat(64))},
            "contract_constraints":{
                "compatible_environment_profiles":[{
                    "id":"python-3.11",
                    "runtime_kind":"python",
                    "preparation_strategy":"python_hashed_requirements",
                    "accepted_dependency_lock_kinds":["pip_requirements"],
                }],
            },
        });

        let instruction = profile_instruction(&run).unwrap().unwrap();
        assert!(instruction.contains("repository_onboarding stage"));
        assert!(instruction.contains(
            "candidate_contract.environment_profile must exactly copy one id from AgentContext.contract_constraints.compatible_environment_profiles"
        ));
        assert!(instruction.contains("python-3.11"));

        let tools = ProjectTools::for_run(std::path::Path::new(&run.cwd), &run).unwrap();
        let proposal = serde_json::json!({
            "schema_version":pharness_core::ONBOARDING_PROPOSAL_SCHEMA,
            "discovery_id":"rdisc_test",
            "discovery_hash":format!("sha256:{}", "a".repeat(64)),
            "candidate_contract":{
                "api_version":"pharness.dev/v1alpha1",
                "environment_profile":"python-3.11",
                "dependency_lock":{"kind":"pip_requirements","path":"requirements.lock","sha256":"b".repeat(64)},
                "writable_paths":["src/**","tests/**","readme.md"],
                "acceptance_commands":[{"name":"unit-tests","command":"python -m unittest discover -s tests -v"}],
                "roots":{"source":["src"],"tests":["tests"],"documentation":["readme.md"]},
                "agent_network":"denied",
                "package_installation":"preparation_only"
            },
            "instructions":"Follow the repository contract.",
            "service_proposals":[],
            "binding_proposals":[],
            "assumptions":[],
            "conflicts":[],
            "blockers":[],
            "readiness_forecast":{}
        });
        let accepted = tools
            .execute(&AgentAction::SubmitOnboardingProposal {
                id: ActionId::new("act_onboarding"),
                reason: "submit bounded proposal".into(),
                proposal: proposal.clone(),
            })
            .await
            .unwrap();
        assert_eq!(accepted.content["structured_submission"], true);

        let mut invalid = proposal;
        invalid["candidate_contract"]["dependency_lock"]["sha256"] =
            serde_json::json!(format!("sha256:{}", "b".repeat(64)));
        assert!(matches!(
            tools
                .execute(&AgentAction::SubmitOnboardingProposal {
                    id: ActionId::new("act_invalid_onboarding"),
                    reason: "submit invalid proposal".into(),
                    proposal: invalid,
                })
                .await,
            Err(ToolError::InvalidArguments { .. })
        ));
    }

    #[test]
    fn accepts_a_safe_https_repository_and_refs() {
        WorkspaceSourceSpec {
            workspace_id: "ws_123".to_string(),
            source_repo: "https://github.com/example/finance-app.git".to_string(),
            source_ref: "main".to_string(),
            source_commit: None,
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
            source_commit: None,
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

    #[test]
    fn old_attempt_payload_deserializes_with_the_general_task_contract() {
        let spec: super::AttemptSpec = serde_json::from_value(serde_json::json!({
            "run": {
                "run_id": "run_legacy",
                "session_id": "ses_legacy",
                "cwd": "/tmp/workspace",
                "user_task": "inspect",
                "max_turns": 24,
                "execution_target_json": {},
                "workspace_source": null
            },
            "event_seq_start": 0,
            "resume": null
        }))
        .unwrap();

        assert_eq!(
            spec.run.task_contract.kind,
            pharness_core::TaskKind::General
        );
        assert!(!spec.run.task_contract.require_workspace_change);
        assert!(!spec.run.task_contract.require_post_change_diff);
    }

    #[test]
    fn repository_instructions_are_bounded_and_stay_inside_the_workspace() {
        let root = std::env::temp_dir().join(format!(
            "pharness-runhost-instructions-{}-{}",
            std::process::id(),
            NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join(".pharness")).unwrap();
        std::fs::write(root.join("AGENTS.md"), "a".repeat(20 * 1024)).unwrap();
        std::fs::write(root.join("CLAUDE.md"), "b".repeat(20 * 1024)).unwrap();
        std::fs::write(root.join(".pharness/instructions.md"), "third").unwrap();

        let (prompt, files) = repository_instructions(&root).unwrap();

        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|file| file.bytes <= 16 * 1024));
        assert!(files.iter().map(|file| file.bytes).sum::<usize>() <= 32 * 1024);
        assert!(prompt.contains("Repository instructions from AGENTS.md"));
        assert!(prompt.contains("Repository instructions from CLAUDE.md"));
        assert!(!prompt.contains("third"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn repository_instruction_symlink_escaping_workspace_is_ignored() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "pharness-runhost-instruction-symlink-{}-{}",
            std::process::id(),
            NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let outside = std::env::temp_dir().join(format!(
            "pharness-runhost-outside-instruction-{}-{}",
            std::process::id(),
            NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&outside, "outside instructions").unwrap();
        symlink(&outside, root.join("AGENTS.md")).unwrap();

        let (prompt, files) = repository_instructions(&root).unwrap();

        assert!(files.is_empty());
        assert!(!prompt.contains("outside instructions"));
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(outside);
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
                source_commit: None,
                branch: "pharness/test/attempt-1".to_string(),
                resolved_commit: Some(base_commit),
            },
        )
        .await
        .unwrap();

        assert_eq!(evidence.changed_paths, vec!["README.md"]);
        assert!(evidence.diff.contains("-before"));
        assert!(evidence.diff.contains("+after"));
        assert!(evidence.diff.ends_with('\n'));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn evidence_git_commands_trust_only_the_issued_workspace() {
        assert_eq!(
            git_evidence_args(
                std::path::Path::new("/workspace"),
                &["diff", "--no-ext-diff"]
            ),
            vec![
                "-c",
                "safe.directory=/workspace",
                "-C",
                "/workspace",
                "diff",
                "--no-ext-diff",
            ]
        );
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
