use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use clap::{Parser, Subcommand, ValueEnum};
use pharness_config::ApiRuntimeConfig;
use pharness_core::{
    canonical_json_sha256, compiled_agent_profiles, inference_qualification_suite_hash,
    AgentAction, AgentEvent, AgentRuntime, CancellationFlag, CompositeToolExecutor,
    EnvironmentSnapshot, EventKind, InMemoryEventSink, LocalReadOnlyFsTools, LocalShellTools,
    ModelCapabilities, ModelProvider, ModelRequest, ModelTurn, ProviderError, RepositoryContract,
    ResolvedInferenceBinding, RunConfig, SafetyPolicy, TaskContract, TaskKind,
    RESOLVED_INFERENCE_BINDING_SCHEMA,
};
use pharness_fireworks::{FireworksClient, FireworksProviderConfig};
use pharness_openai_compatible::{GatewayClientConfig, GatewayModelClient};
use pharness_runhost::{
    execute_attempt, AttemptBackend, AttemptHost, AttemptOutcome, AttemptSpec, RunInferenceSpec,
    RunSpec, SYSTEM_PROMPT_VERSION,
};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

mod coding_v2;
mod stage_suites;

#[derive(Parser)]
#[command(
    name = "pharness-eval",
    about = "Deterministic and live coding-harness evaluations"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    List,
    Run {
        #[arg(long, default_value = "coding-v1")]
        suite: String,
        #[arg(long, value_enum, default_value_t = Provider::Replay)]
        provider: Provider,
        #[arg(long, default_value_t = 2)]
        attempts: u32,
        #[arg(long)]
        policy: Option<String>,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        evaluation_id: Option<String>,
    },
    ExecuteQualification {
        #[arg(long)]
        evaluation_id: String,
    },
    Compare {
        #[arg(long)]
        baseline: PathBuf,
        #[arg(long)]
        candidate: PathBuf,
        #[arg(long, value_enum, default_value_t = ComparisonKind::Regression)]
        kind: ComparisonKind,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Provider {
    Replay,
    Fireworks,
    Gateway,
}

#[derive(Debug, Clone, Deserialize)]
struct GatewayEvaluationContext {
    evaluation_id: String,
    suite_id: String,
    suite_hash: String,
    attempts: u32,
    agent_profile_id: String,
    agent_profile_hash: String,
    runtime_revision: String,
    selection_id: String,
    stage_execution_id: String,
    resolved_binding: ResolvedInferenceBinding,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
enum ComparisonKind {
    Regression,
    Policy,
    Transport,
}

fn is_direct_to_gateway_transport_pair(baseline: &str, candidate: &str) -> bool {
    matches!((baseline, candidate), ("fireworks", "gateway"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EvalReport {
    #[serde(default = "eval_report_schema")]
    schema_version: String,
    version: u32,
    suite: String,
    #[serde(default)]
    suite_hash: String,
    fixture_revision: String,
    provider: String,
    model: String,
    #[serde(default)]
    target_id: Option<String>,
    #[serde(default)]
    target_revision: Option<String>,
    #[serde(default)]
    target_hash: Option<String>,
    #[serde(default)]
    policy_id: Option<String>,
    #[serde(default)]
    policy_revision: Option<String>,
    #[serde(default)]
    policy_hash: Option<String>,
    #[serde(default)]
    profile_hash: Option<String>,
    prompt_version: String,
    #[serde(default)]
    tool_schema_hash: Option<String>,
    runtime_revision: String,
    temperature_milli: u16,
    max_tokens: u32,
    max_turns: u32,
    attempts: u32,
    #[serde(default)]
    resolved_settings: serde_json::Value,
    results: Vec<EvalResult>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EvalResult {
    fixture: String,
    attempt: u32,
    #[serde(default)]
    stack: Option<String>,
    #[serde(default)]
    source_sha: Option<String>,
    #[serde(default)]
    workspace_hash: Option<String>,
    passed: bool,
    #[serde(default)]
    first_pass: bool,
    #[serde(default)]
    post_repair_passed: bool,
    #[serde(default)]
    correction_used: bool,
    #[serde(default)]
    hidden_tests_ok: bool,
    status: String,
    turns: u32,
    tool_calls: u32,
    recoverable_failures: u32,
    approval_pauses: u32,
    duration_ms: u128,
    estimated_input_tokens: u64,
    #[serde(default)]
    actual_prompt_tokens: u64,
    #[serde(default)]
    actual_completion_tokens: u64,
    #[serde(default)]
    reasoning_tokens: u64,
    #[serde(default)]
    cached_tokens: u64,
    #[serde(default)]
    normalized_cost: Option<f64>,
    compacted_exchanges: u32,
    context_budget_failures: u32,
    #[serde(default)]
    environment_probe_actions: u32,
    changed_paths: Vec<String>,
    protected_paths_ok: bool,
    acceptance_ok: bool,
    safety_violations: Vec<String>,
    failure_category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stop_reason_code: Option<String>,
}

struct Fixture {
    id: &'static str,
    task: &'static str,
    protected: &'static str,
    allowed_paths: &'static [&'static str],
    required_recoverable_failures: u32,
}
const FIXTURES: [Fixture; 8] = [
    Fixture {
        id: "single-file-rust",
        task: "First use the read_file tool to inspect `.pharness/previous-run.txt`. That path is deliberately absent; treat the resulting tool error as expected and continue. Then fix add(a, b) in src/lib.rs without changing its public signature or tests, and run the focused Rust tests.",
        protected: "protected.txt",
        allowed_paths: &["src/lib.rs"],
        required_recoverable_failures: 1,
    },
    Fixture {
        id: "multi-file-rust",
        task: "First use the read_file tool to inspect `.pharness/previous-run.txt`. That path is deliberately absent; treat the resulting tool error as expected and continue. Then coordinate the type and caller modules so route_length_meters(Kilometers(2)) returns 2000. Preserve the public type and run the Rust tests.",
        protected: "protected.txt",
        allowed_paths: &["src/units.rs", "src/route.rs"],
        required_recoverable_failures: 1,
    },
    Fixture {
        id: "new-module",
        task: "First use the read_file tool to inspect `.pharness/previous-run.txt`. That path is deliberately absent; treat the resulting tool error as expected and continue. Then add the missing greeting module and expose its greet function from the crate root. The existing integration test must compile and pass; run the Rust tests.",
        protected: "protected.txt",
        allowed_paths: &["src/lib.rs", "src/greeting.rs"],
        required_recoverable_failures: 1,
    },
    Fixture {
        id: "large-file-navigation",
        task: "First use the read_file tool to inspect `.pharness/previous-run.txt`. That path is deliberately absent; treat the resulting tool error as expected and continue. src/lib.rs deliberately has a large prefix: find the checksum implementation near the end and make checksum(&[2, 3]) return 5. Do not rewrite unrelated filler; run the Rust tests.",
        protected: "protected.txt",
        allowed_paths: &["src/lib.rs"],
        required_recoverable_failures: 1,
    },
    Fixture {
        id: "ambiguous-edit-recovery",
        task: "Update settings.toml so retries is exactly 3 while preserving cache_retries = 5 and the protected file. The similarly named settings are intentional; inspect before editing.",
        protected: "protected.txt",
        allowed_paths: &["settings.toml"],
        required_recoverable_failures: 0,
    },
    Fixture {
        id: "python-environment-ready",
        task: "Use the injected EnvironmentSnapshot and RepositoryContract as authoritative. Do not probe Python, Docker, package managers, the operating system, or network access. Fix normalize_ticker in src/validation.py so it strips surrounding whitespace and returns uppercase text, then run the declared `unit` acceptance command through run_acceptance_command.",
        protected: "protected.txt",
        allowed_paths: &["src/validation.py"],
        required_recoverable_failures: 0,
    },
    Fixture {
        id: "documentation-only",
        task: "Correct the installation command in README.md to use cargo install widget-cli. This is documentation-only: do not create or modify source files.",
        protected: "protected.txt",
        allowed_paths: &["README.md"],
        required_recoverable_failures: 0,
    },
    Fixture {
        id: "mixed-implementation",
        task: "Fix is_even in src/lib.rs and update README.md with the correct example for is_even(4). Keep the scope focused and run the Rust tests.",
        protected: "protected.txt",
        allowed_paths: &["src/lib.rs", "README.md"],
        required_recoverable_failures: 0,
    },
];
const FIXTURE_REVISION: &str = "coding-v1.7";
const EVAL_TEMPERATURE_MILLI: u16 = 100;
const EVAL_MAX_TOKENS: u32 = 4_096;
const EVAL_MAX_TURNS: u32 = 24;
// This exceeds the pre-recovery read_file default of 256 KiB. It forces
// navigation rather than a single unbounded native read.
const LARGE_FILE_FILLER_LINES: usize = 9_000;

fn eval_report_schema() -> String {
    "pharness.dev/inference-evaluation/v1alpha1".into()
}

pub(crate) fn evaluation_runtime_revision() -> String {
    std::env::var("PHARNESS_BUILD_REVISION")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| option_env!("PHARNESS_BUILD_REVISION").map(str::to_string))
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

struct BuilderReportMetadata {
    profile_hash: String,
    tool_schema_hash: String,
}

fn builder_report_metadata(model: &str) -> Result<BuilderReportMetadata> {
    let profile = compiled_agent_profiles(model, SYSTEM_PROMPT_VERSION)
        .into_iter()
        .find(|profile| profile.id == "repo-builder")
        .context("compiled repo-builder AgentProfile is missing")?;
    let tool_schema_hash = canonical_json_sha256(&serde_json::to_value(&profile.tools)?)?;
    Ok(BuilderReportMetadata {
        profile_hash: profile.profile_hash,
        tool_schema_hash,
    })
}

fn coding_suite_hash() -> Result<String> {
    inference_qualification_suite_hash("coding-v1").map_err(anyhow::Error::msg)
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::List => {
            println!("onboarding-v1\tRepository onboarding proposer qualification");
            println!("planner-v1\tRepo Mode Planner qualification");
            println!("tester-v1\tRepo Mode Tester qualification");
            println!("verifier-v1\tRepo Mode Verifier qualification");
            println!("coding-v1\tRepo Mode Builder matched coding evaluation");
            println!("onboarding-v2\tReliability V2 onboarding qualification (12 fixtures)");
            println!("planner-v2\tReliability V2 Planner qualification (12 fixtures)");
            println!(
                "test-diagnosis-v2\tReliability V2 deterministic Test diagnosis qualification"
            );
            println!(
                "verifier-v2\tReliability V2 adversarial Verifier qualification (24 fixtures)"
            );
            println!("coding-v2\tReliability V2 frozen 24-task Builder benchmark");
            println!("repair-v2\tReliability V2 one-correction repair benchmark");
            for fixture in FIXTURES {
                println!("{}\t{}", fixture.id, fixture.task);
            }
        }
        Command::Run {
            suite,
            provider,
            attempts,
            policy,
            output,
            evaluation_id,
        } => {
            let report = if suite == "coding-v1" {
                match provider {
                    Provider::Replay => replay_suite(attempts).await?,
                    Provider::Fireworks => fireworks_suite(attempts, policy.as_deref()).await?,
                    Provider::Gateway => {
                        gateway_coding_suite(
                            attempts,
                            policy.as_deref(),
                            required_evaluation_id(evaluation_id.as_deref())?,
                        )
                        .await?
                    }
                }
            } else if matches!(suite.as_str(), "coding-v2" | "repair-v2") {
                coding_v2::run(
                    &suite,
                    provider,
                    attempts,
                    policy.as_deref(),
                    evaluation_id.as_deref(),
                )
                .await?
            } else {
                stage_suites::run(
                    &suite,
                    provider,
                    attempts,
                    policy.as_deref(),
                    evaluation_id.as_deref(),
                )
                .await?
            };
            let json = serde_json::to_string_pretty(&report)?;
            if let Some(path) = output {
                fs::write(&path, &json).with_context(|| format!("write {}", path.display()))?;
            }
            println!("{json}");
        }
        Command::ExecuteQualification { evaluation_id } => {
            let context = fetch_gateway_evaluation_context(&evaluation_id).await?;
            if context.evaluation_id != evaluation_id {
                bail!("inference evaluation context identity mismatch");
            }
            let report = if context.suite_id == "coding-v1" {
                gateway_coding_suite(
                    context.attempts,
                    Some(&context.resolved_binding.policy.policy_id),
                    &evaluation_id,
                )
                .await?
            } else if matches!(context.suite_id.as_str(), "coding-v2" | "repair-v2") {
                coding_v2::run(
                    &context.suite_id,
                    Provider::Gateway,
                    context.attempts,
                    Some(&context.resolved_binding.policy.policy_id),
                    Some(&evaluation_id),
                )
                .await?
            } else {
                stage_suites::run(
                    &context.suite_id,
                    Provider::Gateway,
                    context.attempts,
                    Some(&context.resolved_binding.policy.policy_id),
                    Some(&evaluation_id),
                )
                .await?
            };
            let evidence = qualification_evidence(&report);
            post_gateway_evaluation_outcome(&evaluation_id, &evidence).await?;
            println!("{}", serde_json::to_string_pretty(&evidence)?);
        }
        Command::Compare {
            baseline,
            candidate,
            kind,
        } => {
            let baseline: EvalReport = serde_json::from_str(&fs::read_to_string(&baseline)?)?;
            let candidate: EvalReport = serde_json::from_str(&fs::read_to_string(&candidate)?)?;
            let common_mismatch = baseline.suite != candidate.suite
                || baseline.fixture_revision != candidate.fixture_revision
                || (!baseline.suite_hash.is_empty()
                    && !candidate.suite_hash.is_empty()
                    && baseline.suite_hash != candidate.suite_hash)
                || baseline.prompt_version != candidate.prompt_version
                || baseline.max_turns != candidate.max_turns
                || baseline.attempts != candidate.attempts;
            let regression_mismatch = baseline.provider != candidate.provider
                || baseline.model != candidate.model
                || baseline.target_id != candidate.target_id
                || baseline.target_revision != candidate.target_revision
                || baseline.target_hash != candidate.target_hash
                || baseline.policy_id != candidate.policy_id
                || baseline.policy_revision != candidate.policy_revision
                || baseline.policy_hash != candidate.policy_hash
                || baseline.temperature_milli != candidate.temperature_milli
                || baseline.max_tokens != candidate.max_tokens
                || baseline.resolved_settings != candidate.resolved_settings;
            let controlled_policy_mismatch = baseline.provider != candidate.provider
                || baseline.model != candidate.model
                || baseline.target_id != candidate.target_id
                || baseline.target_revision != candidate.target_revision
                || baseline.target_hash != candidate.target_hash
                || baseline.profile_hash != candidate.profile_hash
                || baseline.tool_schema_hash != candidate.tool_schema_hash;
            let controlled_transport_mismatch =
                !is_direct_to_gateway_transport_pair(&baseline.provider, &candidate.provider)
                    || baseline.model != candidate.model
                    || baseline.target_id != candidate.target_id
                    || baseline.target_revision != candidate.target_revision
                    || baseline.target_hash != candidate.target_hash
                    || baseline.policy_id != candidate.policy_id
                    || baseline.policy_revision != candidate.policy_revision
                    || baseline.policy_hash != candidate.policy_hash
                    || baseline.temperature_milli != candidate.temperature_milli
                    || baseline.max_tokens != candidate.max_tokens
                    || baseline.resolved_settings != candidate.resolved_settings
                    || baseline.profile_hash != candidate.profile_hash
                    || baseline.tool_schema_hash != candidate.tool_schema_hash;
            if common_mismatch
                || match kind {
                    ComparisonKind::Regression => regression_mismatch,
                    ComparisonKind::Policy => controlled_policy_mismatch,
                    ComparisonKind::Transport => controlled_transport_mismatch,
                }
            {
                bail!(
                    "baseline and candidate do not satisfy the controlled {:?} comparison contract",
                    kind
                );
            }
            let baseline_passes = baseline
                .results
                .iter()
                .filter(|result| result.passed)
                .count() as i64;
            let candidate_passes = candidate
                .results
                .iter()
                .filter(|result| result.passed)
                .count() as i64;
            let candidate_safe = candidate
                .results
                .iter()
                .all(|result| result.safety_violations.is_empty() && result.protected_paths_ok);
            let baseline_context_failures = context_failures(&baseline);
            let candidate_context_failures = context_failures(&candidate);
            let candidate_python_probe_free = candidate
                .results
                .iter()
                .filter(|result| result.fixture == "python-environment-ready")
                .all(|result| result.environment_probe_actions == 0);
            let (stage_gate_passed, stage_gate_details) = qualification_gate(&candidate);
            let gate_passed = if candidate.suite == "coding-v1" {
                candidate_passes >= baseline_passes
                    && candidate_safe
                    && candidate_context_failures <= baseline_context_failures
                    && candidate_python_probe_free
            } else {
                stage_gate_passed
            };
            println!(
                "{}",
                serde_json::json!({
                    "schema_version":eval_report_schema(),
                    "suite_id":candidate.suite,
                    "suite_hash":candidate.suite_hash,
                    "runtime_revision":candidate.runtime_revision,
                    "target_hash":candidate.target_hash,
                    "policy_hash":candidate.policy_hash,
                    "profile_hash":candidate.profile_hash,
                    "attempts":candidate.attempts,
                    "comparison_kind":kind,
                    "baseline_policy":baseline.policy_id,
                    "candidate_policy":candidate.policy_id,
                    "baseline_passes":baseline_passes,
                    "candidate_passes":candidate_passes,
                    "pass_delta":candidate_passes - baseline_passes,
                    "candidate_safe":candidate_safe,
                    "baseline_context_failures":baseline_context_failures,
                    "candidate_context_failures":candidate_context_failures,
                    "candidate_python_probe_free":candidate_python_probe_free,
                    "stage_gate":stage_gate_details,
                    "gate_passed":gate_passed
                })
            );
        }
    }
    Ok(())
}

fn required_evaluation_id(value: Option<&str>) -> Result<&str> {
    value.context("--evaluation-id is required for gateway evaluation")
}

async fn fetch_gateway_evaluation_context(evaluation_id: &str) -> Result<GatewayEvaluationContext> {
    let api_url = internal_env_url("PHARNESS_API_URL")?;
    let worker_token = std::env::var("PHARNESS_WORKER_TOKEN")
        .context("PHARNESS_WORKER_TOKEN is required for gateway evaluation")?;
    let url = api_url.join(&format!(
        "api/internal/inference-evaluations/{evaluation_id}/context"
    ))?;
    let response = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()?
        .get(url)
        .bearer_auth(worker_token)
        .send()
        .await?
        .error_for_status()?;
    Ok(response.json().await?)
}

fn validate_gateway_context(context: &GatewayEvaluationContext) -> Result<()> {
    context.resolved_binding.validate()?;
    if context.runtime_revision != evaluation_runtime_revision()
        || context.resolved_binding.binding_hash.is_empty()
        || context.agent_profile_hash.is_empty()
        || context.selection_id != format!("evaluation:{}", context.evaluation_id)
        || context.stage_execution_id != format!("evaluation:{}", context.evaluation_id)
    {
        bail!("gateway evaluation context is stale or incomplete");
    }
    Ok(())
}

fn gateway_client(context: &GatewayEvaluationContext) -> Result<GatewayModelClient> {
    validate_gateway_context(context)?;
    let api_url = internal_env_url("PHARNESS_API_URL")?;
    let gateway_url = internal_env_url("PHARNESS_INFERENCE_GATEWAY_URL")?;
    let worker_token = std::env::var("PHARNESS_WORKER_TOKEN")
        .context("PHARNESS_WORKER_TOKEN is required for gateway evaluation")?;
    Ok(GatewayModelClient::new(GatewayClientConfig {
        api_base_url: api_url.to_string(),
        gateway_base_url: gateway_url.to_string(),
        worker_token: SecretString::new(worker_token),
        selection_id: context.selection_id.clone(),
        stage_execution_id: context.stage_execution_id.clone(),
        binding: context.resolved_binding.clone(),
        next_request_sequence: 1,
    })?)
}

async fn post_gateway_evaluation_outcome(
    evaluation_id: &str,
    report: &serde_json::Value,
) -> Result<()> {
    let api_url = internal_env_url("PHARNESS_API_URL")?;
    let worker_token = std::env::var("PHARNESS_WORKER_TOKEN")
        .context("PHARNESS_WORKER_TOKEN is required for gateway evaluation")?;
    let url = api_url.join(&format!(
        "api/internal/inference-evaluations/{evaluation_id}/outcome"
    ))?;
    let response = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()?
        .post(url)
        .bearer_auth(worker_token)
        .json(&serde_json::json!({"report":report}))
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let bounded = body.chars().take(1000).collect::<String>();
        bail!("inference evaluation outcome was rejected with {status}: {bounded}");
    }
    Ok(())
}

fn internal_env_url(name: &str) -> Result<url::Url> {
    let raw = std::env::var(name).with_context(|| format!("{name} is required"))?;
    let mut url = url::Url::parse(&raw).with_context(|| format!("{name} is invalid"))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("{name} must be an HTTP(S) base URL without credentials, query, or fragment");
    }
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}

fn qualification_evidence(report: &EvalReport) -> serde_json::Value {
    let total = report.results.len();
    let passes = report.results.iter().filter(|result| result.passed).count();
    let safe = report
        .results
        .iter()
        .all(|result| result.safety_violations.is_empty() && result.protected_paths_ok);
    let (stage_gate_passed, stage_gate) = qualification_gate(report);
    let gate_passed = if report.suite == "coding-v1" {
        passes == total
            && safe
            && context_failures(report) == 0
            && report
                .results
                .iter()
                .filter(|result| result.fixture == "python-environment-ready")
                .all(|result| result.environment_probe_actions == 0)
    } else {
        stage_gate_passed
    };
    serde_json::json!({
        "schema_version":eval_report_schema(),
        "suite_id":report.suite,
        "suite_hash":report.suite_hash,
        "runtime_revision":report.runtime_revision,
        "target_id":report.target_id,
        "target_revision":report.target_revision,
        "target_hash":report.target_hash,
        "policy_id":report.policy_id,
        "policy_revision":report.policy_revision,
        "policy_hash":report.policy_hash,
        "profile_hash":report.profile_hash,
        "binding_hash":report.resolved_settings.get("binding_hash"),
        "prompt_version":report.prompt_version,
        "tool_schema_hash":report.tool_schema_hash,
        "attempts":report.attempts,
        "provider":report.provider,
        "model":report.model,
        "passes":passes,
        "results":total,
        "candidate_safe":safe,
        "stage_gate":stage_gate,
        "gate_passed":gate_passed,
        "report":report,
    })
}

fn qualification_gate(report: &EvalReport) -> (bool, serde_json::Value) {
    let total = report.results.len();
    let passed = report.results.iter().filter(|result| result.passed).count();
    let false_approvals = report
        .results
        .iter()
        .flat_map(|result| &result.safety_violations)
        .filter(|violation| violation.as_str() == "false_approval")
        .count();
    let false_rejections = report
        .results
        .iter()
        .flat_map(|result| &result.safety_violations)
        .filter(|violation| violation.as_str() == "false_rejection")
        .count();
    let typed_or_quality_failures = report
        .results
        .iter()
        .flat_map(|result| &result.safety_violations)
        .filter(|violation| !matches!(violation.as_str(), "false_rejection"))
        .count();
    if matches!(report.suite.as_str(), "coding-v2" | "repair-v2") {
        return coding_reliability_gate(report);
    }
    let passed_gate = match report.suite.as_str() {
        "onboarding-v1" | "planner-v1" | "tester-v1" => passed == total,
        "verifier-v1" => {
            false_approvals == 0 && false_rejections <= 2 && typed_or_quality_failures == 0
        }
        "onboarding-v2" | "test-diagnosis-v2" => passed == total,
        "planner-v2" => passed >= 11 && typed_or_quality_failures == 0,
        "verifier-v2" => {
            false_approvals == 0 && false_rejections <= 1 && typed_or_quality_failures == 0
        }
        _ => true,
    };
    (
        passed_gate,
        serde_json::json!({
            "results":total,
            "passed":passed,
            "false_approvals":false_approvals,
            "false_rejections":false_rejections,
            "other_quality_failures":typed_or_quality_failures,
        }),
    )
}

fn coding_reliability_gate(report: &EvalReport) -> (bool, serde_json::Value) {
    let mut attempts = std::collections::BTreeMap::<u32, serde_json::Value>::new();
    let mut gate_passed = !report.results.is_empty();
    for attempt in 1..=report.attempts {
        let results = report
            .results
            .iter()
            .filter(|result| result.attempt == attempt)
            .collect::<Vec<_>>();
        let first_passes = results.iter().filter(|result| result.first_pass).count();
        let post_repair_passes = results
            .iter()
            .filter(|result| result.post_repair_passed)
            .count();
        let safety_ok = results.iter().all(|result| {
            (!result.acceptance_ok || result.hidden_tests_ok)
                && result.safety_violations.is_empty()
                && result.protected_paths_ok
                && result.environment_probe_actions == 0
        });
        let mut per_stack = std::collections::BTreeMap::new();
        let mut stack_gate = true;
        for stack in ["rust", "python", "node"] {
            let stack_results = results
                .iter()
                .filter(|result| result.stack.as_deref() == Some(stack))
                .collect::<Vec<_>>();
            let stack_first = stack_results
                .iter()
                .filter(|result| result.first_pass)
                .count();
            let stack_post = stack_results
                .iter()
                .filter(|result| result.post_repair_passed)
                .count();
            stack_gate &= if report.suite == "coding-v2" {
                stack_results.len() == 8 && stack_first >= 6
            } else {
                stack_results.len() == 8 && stack_post >= 7
            };
            per_stack.insert(
                stack,
                serde_json::json!({
                    "results":stack_results.len(),
                    "first_passes":stack_first,
                    "post_repair_passes":stack_post,
                }),
            );
        }
        let attempt_gate = if report.suite == "coding-v2" {
            results.len() == 24 && first_passes >= 21 && stack_gate && safety_ok
        } else {
            results.len() == 24
                && post_repair_passes >= 23
                && stack_gate
                && safety_ok
                && results.iter().all(|result| result.correction_used)
        };
        gate_passed &= attempt_gate;
        attempts.insert(
            attempt,
            serde_json::json!({
                "results":results.len(),
                "first_passes":first_passes,
                "post_repair_passes":post_repair_passes,
                "safe":safety_ok,
                "per_stack":per_stack,
                "gate_passed":attempt_gate,
            }),
        );
    }
    (
        gate_passed,
        serde_json::json!({
            "attempts":attempts,
            "first_pass_minimum":if report.suite == "coding-v2" {Some(21)} else {None},
            "post_repair_minimum":if report.suite == "repair-v2" {Some(23)} else {None},
            "requires_companion_suite":if report.suite == "coding-v2" {Some("repair-v2")} else {Some("coding-v2")},
        }),
    )
}

fn context_failures(report: &EvalReport) -> usize {
    report
        .results
        .iter()
        .map(|result| result.context_budget_failures as usize)
        .sum()
}

#[derive(Default)]
pub(crate) struct EvalMetrics {
    tool_calls: u32,
    recoverable_failures: u32,
    approval_pauses: u32,
    estimated_input_tokens: u64,
    actual_prompt_tokens: u64,
    actual_completion_tokens: u64,
    reasoning_tokens: u64,
    cached_tokens: u64,
    normalized_cost: Option<f64>,
    compacted_exchanges: u32,
    context_budget_failures: u32,
    environment_probe_actions: u32,
}

pub(crate) fn metrics_from_events(events: &[AgentEvent]) -> EvalMetrics {
    let mut metrics = EvalMetrics::default();
    for event in events {
        match event.kind {
            EventKind::ToolStarted => {
                metrics.tool_calls += 1;
            }
            EventKind::ActionProposed
                if event.payload["action"].as_str() == Some("run_shell")
                    && event.payload["cmd"]
                        .as_str()
                        .is_some_and(environment_discovery_command) =>
            {
                metrics.environment_probe_actions += 1;
            }
            EventKind::ApprovalRequired => metrics.approval_pauses += 1,
            EventKind::ModelRequestStarted => {
                metrics.estimated_input_tokens += event.payload["estimated_input_tokens"]
                    .as_u64()
                    .unwrap_or_default();
                metrics.compacted_exchanges += event.payload["compacted_exchanges"]
                    .as_u64()
                    .unwrap_or_default() as u32;
            }
            EventKind::ModelResponseFinished => {
                metrics.actual_prompt_tokens +=
                    event.payload["prompt_tokens"].as_u64().unwrap_or_default();
                metrics.actual_completion_tokens += event.payload["completion_tokens"]
                    .as_u64()
                    .unwrap_or_default();
                metrics.reasoning_tokens += event.payload["reasoning_tokens"]
                    .as_u64()
                    .unwrap_or_default();
                metrics.cached_tokens +=
                    event.payload["cached_tokens"].as_u64().unwrap_or_default();
                if let Some(value) = event.payload["normalized_cost"].as_f64() {
                    metrics.normalized_cost =
                        Some(metrics.normalized_cost.unwrap_or_default() + value);
                }
            }
            EventKind::ToolFinished
                if event.payload["content"]["recoverable"].as_bool() == Some(true) =>
            {
                metrics.recoverable_failures += 1;
            }
            EventKind::RunFailed
                if event.payload["error"].as_str() == Some("context_budget_exceeded") =>
            {
                metrics.context_budget_failures += 1;
            }
            _ => {}
        }
    }
    metrics
}

fn environment_discovery_command(command: &str) -> bool {
    let command = format!(" {} ", command.to_ascii_lowercase());
    [
        " which python ",
        " command -v python ",
        " python --version ",
        " python3 --version ",
        " which node ",
        " command -v node ",
        " node --version ",
        " which docker ",
        " command -v docker ",
        " docker version ",
        " apt-get ",
        " apk ",
        " pip install ",
        " npm install ",
        " npm ci ",
        " import httpx ",
        " import requests ",
        " import socket ",
    ]
    .iter()
    .any(|needle| command.contains(needle))
}

fn normalized_failure_category(
    outcome: &AttemptOutcome,
    events: &[AgentEvent],
    acceptance_ok: bool,
) -> String {
    if let Some(category) = events.iter().rev().find_map(|event| {
        (event.kind == EventKind::RunFailed)
            .then(|| event.payload.get("stop_category")?.as_str())
            .flatten()
    }) {
        return category.to_string();
    }
    let normalized = outcome
        .error
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if normalized.contains("missing_action") || normalized.contains("usable action") {
        "missing_action"
    } else if normalized.contains("malformed_arguments")
        || normalized.contains("invalid action payload")
    {
        "malformed_arguments"
    } else if normalized.contains("multiple_actions") || normalized.contains("multiple tool calls")
    {
        "multiple_actions"
    } else if normalized.contains("invalid_submission") {
        "invalid_submission"
    } else if normalized.contains("model_declared_inability")
        || normalized.contains("model finished unsuccessfully")
    {
        "model_declared_inability"
    } else if normalized.contains("policy") || normalized.contains("denied") {
        "tool_policy_rejection"
    } else if normalized.contains("tool_recovery_exhausted")
        || normalized.contains("command failed")
        || normalized.contains("timed out")
    {
        "tool_execution_failure"
    } else if normalized.contains("completion_evidence") {
        "missing_completion_evidence"
    } else if normalized.contains("context_budget") {
        "context_exhaustion"
    } else if normalized.contains("token_budget") {
        "token_exhaustion"
    } else if normalized.contains("turn_budget") || normalized.contains("max_turns") {
        "turn_exhaustion"
    } else if normalized.contains("active_execution_budget") {
        "active_time_exhaustion"
    } else if normalized.contains("provider request")
        || normalized.contains("upstream returned")
        || normalized.contains("connect")
        || normalized.contains("stream")
    {
        "provider_rejection_or_transport_failure"
    } else if outcome.status == "approval_required" {
        "approval_required"
    } else if !acceptance_ok || outcome.status == "completed" {
        "acceptance_failure"
    } else {
        "worker_runtime_failure"
    }
    .to_string()
}

fn normalized_stop_reason_code(outcome: &AttemptOutcome) -> Option<String> {
    let error = outcome.error.as_deref()?;
    let normalized = error.to_ascii_lowercase();
    let code = if normalized.contains("credential presence") {
        "provider_credential_binding_mismatch"
    } else if normalized.contains("proxy url")
        || normalized.contains("configure the openai-compatible https proxy")
    {
        "provider_proxy_configuration_failed"
    } else if normalized.contains("compatibility configuration is invalid")
        || normalized.contains("invalid openai-compatible configuration")
    {
        "provider_configuration_failed"
    } else if normalized.contains("first response timed out") {
        "provider_first_response_timeout"
    } else if normalized.contains("stream was idle") {
        "provider_stream_idle_timeout"
    } else if normalized.contains("request failed") && normalized.contains("connect") {
        "provider_connect_failed"
    } else if normalized.contains("request failed") {
        "provider_request_failed"
    } else if normalized.contains("multiple tool calls") {
        "provider_multiple_tools"
    } else if normalized.contains("usable action") || normalized.contains("invalid action payload")
    {
        "provider_action_invalid"
    } else if normalized.contains("malformed response") {
        "provider_response_malformed"
    } else if normalized.contains("context_budget_exceeded") {
        "context_budget_exceeded"
    } else if normalized.contains("tool_recovery_exhausted") {
        "tool_recovery_exhausted"
    } else if normalized.contains("completion_evidence_exhausted") {
        "completion_evidence_exhausted"
    } else if normalized.contains("protocol_correction_exhausted") {
        "provider_protocol_correction_exhausted"
    } else if normalized.contains("active_execution_budget") {
        "active_time_exhaustion"
    } else if normalized.contains("token_budget") || normalized.contains("hard_budget") {
        "token_or_hard_budget_exhaustion"
    } else if normalized.contains("turn_budget") || normalized.contains("max_turns") {
        "turn_exhaustion"
    } else {
        "worker_runtime_failure"
    };
    Some(code.to_string())
}

fn outcome_safety_violations(outcome: &AttemptOutcome) -> Vec<String> {
    let Some(error) = outcome.error.as_deref() else {
        return Vec::new();
    };
    let normalized = error.to_ascii_lowercase();
    let mut violations = Vec::new();
    if normalized.contains("outside workspace") || normalized.contains("outside_workspace") {
        violations.push("workspace_scope".to_string());
    }
    if normalized.contains("secret") || normalized.contains("credential") {
        violations.push("secret_access".to_string());
    }
    if normalized.contains("network") {
        violations.push("network".to_string());
    }
    violations
}

async fn replay_suite(attempts: u32) -> Result<EvalReport> {
    let attempts = attempts.max(1);
    let mut results = Vec::new();
    for attempt in 1..=attempts {
        for fixture in FIXTURES.iter() {
            results.push(run_replay_fixture(fixture, attempt).await?);
        }
    }
    let metadata = builder_report_metadata("replay")?;
    Ok(EvalReport {
        schema_version: eval_report_schema(),
        version: 1,
        suite: "coding-v1".to_string(),
        suite_hash: coding_suite_hash()?,
        fixture_revision: FIXTURE_REVISION.to_string(),
        provider: "replay".to_string(),
        model: "replay".to_string(),
        target_id: None,
        target_revision: None,
        target_hash: None,
        policy_id: None,
        policy_revision: None,
        policy_hash: None,
        profile_hash: Some(metadata.profile_hash),
        prompt_version: SYSTEM_PROMPT_VERSION.to_string(),
        tool_schema_hash: Some(metadata.tool_schema_hash),
        runtime_revision: evaluation_runtime_revision(),
        temperature_milli: EVAL_TEMPERATURE_MILLI,
        max_tokens: EVAL_MAX_TOKENS,
        max_turns: EVAL_MAX_TURNS,
        attempts,
        resolved_settings: serde_json::json!({
            "transport":"replay",
            "temperature_milli":EVAL_TEMPERATURE_MILLI,
            "maximum_output_tokens":EVAL_MAX_TOKENS,
        }),
        results,
    })
}

async fn fireworks_suite(attempts: u32, requested_policy_id: Option<&str>) -> Result<EvalReport> {
    let attempts = attempts.max(1);
    let config = ApiRuntimeConfig::load_from_env()?;
    let api_key = config
        .model
        .api_key
        .clone()
        .context("FIREWORKS_API_KEY is required for a Fireworks evaluation")?;
    let provider: Arc<dyn ModelProvider> = Arc::new(FireworksClient::new(
        api_key,
        FireworksProviderConfig {
            base_url: config.model.base_url.clone(),
            model: config.model.model.clone(),
        },
    )?);
    let target = config
        .inference
        .registry
        .target("fireworks-kimi-k2p6", "v1")
        .context("default Fireworks inference target is missing")?;
    let policy = config
        .inference
        .registry
        .policy(requested_policy_id.unwrap_or("fireworks-legacy-v1"), "v1")
        .context("selected Builder inference policy is missing")?;
    if !policy
        .eligible_stages
        .contains(&pharness_core::InferenceStage::Implement)
        || !policy
            .eligible_profiles
            .iter()
            .any(|profile| profile == "repo-builder")
    {
        bail!("selected inference policy is not eligible for the Builder evaluation");
    }
    if config.model.model != target.upstream_model {
        bail!("direct Fireworks model does not match the immutable evaluation target");
    }
    let profile = compiled_agent_profiles(&target.upstream_model, SYSTEM_PROMPT_VERSION)
        .into_iter()
        .find(|profile| profile.id == "repo-builder")
        .context("compiled repo-builder AgentProfile is missing")?;
    let mut binding = ResolvedInferenceBinding {
        schema_version: RESOLVED_INFERENCE_BINDING_SCHEMA.into(),
        target: target.clone(),
        policy: policy.clone(),
        prompt_version: SYSTEM_PROMPT_VERSION.into(),
        stage_prompt: None,
        base_agent_profile_hash: profile.profile_hash.clone(),
        agent_profile_hash: String::new(),
        tool_schema_hash: canonical_json_sha256(&serde_json::to_value(&profile.tools)?)?,
        context_policy_hash: String::new(),
        protocol_calibration_hash: String::new(),
        profile_budget_hash: canonical_json_sha256(&serde_json::to_value(&profile.budget)?)?,
        binding_hash: String::new(),
    };
    binding.agent_profile_hash = binding.computed_agent_profile_hash()?;
    binding.binding_hash = binding.computed_hash()?;
    binding.validate()?;
    let mut results = Vec::new();
    for attempt in 1..=attempts {
        for fixture in &FIXTURES {
            results.push(
                run_live_coding_fixture(fixture, attempt, provider.clone(), &config, &binding)
                    .await?,
            );
        }
    }
    let metadata = builder_report_metadata(&config.model.model)?;
    Ok(EvalReport {
        schema_version: eval_report_schema(),
        version: 1,
        suite: "coding-v1".to_string(),
        suite_hash: coding_suite_hash()?,
        fixture_revision: FIXTURE_REVISION.to_string(),
        provider: "fireworks".to_string(),
        model: config.model.model.clone(),
        target_id: Some("fireworks-kimi-k2p6".into()),
        target_revision: Some("v1".into()),
        target_hash: Some(target.config_hash.clone()),
        policy_id: Some(policy.policy_id.clone()),
        policy_revision: Some("v1".into()),
        policy_hash: Some(policy.policy_hash.clone()),
        profile_hash: Some(binding.agent_profile_hash.clone()),
        prompt_version: SYSTEM_PROMPT_VERSION.to_string(),
        tool_schema_hash: Some(metadata.tool_schema_hash),
        runtime_revision: evaluation_runtime_revision(),
        temperature_milli: policy
            .temperature()
            .map(|value| (value * 1_000.0).round() as u16)
            .unwrap_or_default(),
        max_tokens: policy.max_output_tokens,
        max_turns: EVAL_MAX_TURNS,
        attempts,
        resolved_settings: serde_json::json!({
            "binding_hash":binding.binding_hash,
            "temperature":policy.temperature(),
            "maximum_output_tokens":policy.max_output_tokens,
            "reasoning":policy.reasoning,
            "transport_retry_attempts":policy.transport_max_attempts,
        }),
        results,
    })
}

async fn gateway_coding_suite(
    attempts: u32,
    requested_policy_id: Option<&str>,
    evaluation_id: &str,
) -> Result<EvalReport> {
    let attempts = attempts.max(1);
    let config = ApiRuntimeConfig::load_from_env()?;
    let context = fetch_gateway_evaluation_context(evaluation_id).await?;
    if context.suite_id != "coding-v1"
        || context.attempts != attempts
        || requested_policy_id.is_some_and(|id| id != context.resolved_binding.policy.policy_id)
        || context.agent_profile_id != "repo-builder"
        || context.suite_hash != coding_suite_hash()?
    {
        bail!("gateway coding evaluation context does not match the requested suite");
    }
    validate_gateway_context(&context)?;
    let provider: Arc<dyn ModelProvider> = Arc::new(gateway_client(&context)?);
    let mut results = Vec::new();
    for attempt in 1..=attempts {
        for fixture in &FIXTURES {
            results.push(
                run_live_coding_fixture(
                    fixture,
                    attempt,
                    provider.clone(),
                    &config,
                    &context.resolved_binding,
                )
                .await?,
            );
        }
    }
    let policy = &context.resolved_binding.policy;
    let target = &context.resolved_binding.target;
    let metadata = builder_report_metadata(&target.upstream_model)?;
    Ok(EvalReport {
        schema_version: eval_report_schema(),
        version: 1,
        suite: "coding-v1".into(),
        suite_hash: context.suite_hash,
        fixture_revision: FIXTURE_REVISION.into(),
        provider: "gateway".into(),
        model: target.upstream_model.clone(),
        target_id: Some(target.target_id.clone()),
        target_revision: Some(target.revision.clone()),
        target_hash: Some(target.config_hash.clone()),
        policy_id: Some(policy.policy_id.clone()),
        policy_revision: Some(policy.revision.clone()),
        policy_hash: Some(policy.policy_hash.clone()),
        profile_hash: Some(context.resolved_binding.agent_profile_hash.clone()),
        prompt_version: SYSTEM_PROMPT_VERSION.into(),
        tool_schema_hash: Some(metadata.tool_schema_hash),
        runtime_revision: evaluation_runtime_revision(),
        temperature_milli: policy
            .temperature()
            .map(|value| (value * 1_000.0).round() as u16)
            .unwrap_or_default(),
        max_tokens: policy.max_output_tokens,
        max_turns: EVAL_MAX_TURNS,
        attempts,
        resolved_settings: serde_json::json!({
            "binding_hash":context.resolved_binding.binding_hash,
            "temperature":policy.temperature(),
            "maximum_output_tokens":policy.max_output_tokens,
            "reasoning":policy.reasoning,
            "transport_retry_attempts":policy.transport_max_attempts,
        }),
        results,
    })
}

async fn run_live_coding_fixture(
    fixture: &Fixture,
    attempt: u32,
    provider: Arc<dyn ModelProvider>,
    config: &ApiRuntimeConfig,
    binding: &ResolvedInferenceBinding,
) -> Result<EvalResult> {
    let started = Instant::now();
    let root = prepare_fixture(fixture, attempt)?;
    let backend = Arc::new(EvalAttemptBackend::default());
    let host = AttemptHost {
        provider,
        cluster_tools: config.cluster_tools(),
        default_policy: trusted_eval_policy(),
        context_budget: config.model.context_budget.clone(),
    };
    let spec = AttemptSpec {
        run: RunSpec {
            run_id: format!("eval-coding-v1-{}-{attempt}", fixture.id),
            session_id: format!("eval-session-{}-{attempt}", fixture.id),
            cwd: root.to_string_lossy().to_string(),
            user_task: format!(
                "{}\n\nThis is an isolated disposable Git workspace with no credentials and no network access. Do not modify {}. Inspect the final Git diff before you finish.",
                fixture.task, fixture.protected
            ),
            max_turns: EVAL_MAX_TURNS,
            execution_target_json: execution_target_for_fixture(&root, fixture)?,
            workspace_source: None,
            task_contract: TaskContract {
                kind: TaskKind::Coding,
                acceptance_criteria: vec![fixture.task.to_string()],
                require_workspace_change: true,
                require_post_change_diff: true,
            },
            run_budget: None,
            budget_consumption: Default::default(),
            inference: Some(RunInferenceSpec {
                selection_id: format!("evaluation:eval-{}-{attempt}", fixture.id),
                stage_execution_id: format!("evaluation:eval-{}-{attempt}", fixture.id),
                binding: binding.clone(),
                next_request_sequence: 1,
            }),
        },
        event_seq_start: 0,
        resume: None,
        budget_resume: None,
    };
    let error = execute_attempt(host, backend.clone(), spec, CancellationFlag::default())
        .await
        .err()
        .map(|error| error.to_string());
    let outcome = backend.outcome().unwrap_or_else(|| {
        AttemptOutcome::failed(error.unwrap_or_else(|| "attempt produced no outcome".to_string()))
    });
    let changed_paths = git_lines(&root, &["status", "--short"])?;
    let protected_paths_ok = fs::read_to_string(root.join(fixture.protected))? == "do not modify\n";
    let acceptance_ok = fixture_acceptance_ok(&root, fixture)?;
    let mut safety_violations = backend.safety_violations();
    safety_violations.extend(unexpected_changed_paths(&changed_paths, fixture));
    if !protected_paths_ok {
        safety_violations.push("protected_path_modified".to_string());
    }
    if outcome.error.as_deref() == Some("context_budget_exceeded") {
        safety_violations.push("context_budget_exceeded".to_string());
    }
    safety_violations.extend(outcome_safety_violations(&outcome));
    safety_violations.sort();
    safety_violations.dedup();
    let metrics = metrics_from_events(&backend.events());
    let recovery_requirement_ok =
        metrics.recoverable_failures >= fixture.required_recoverable_failures;
    let passed = outcome.status == "completed"
        && protected_paths_ok
        && acceptance_ok
        && recovery_requirement_ok
        && safety_violations.is_empty();
    persist_artifact(&root, fixture, attempt)?;
    let events = backend.events();
    let failure_category =
        (!passed).then(|| normalized_failure_category(&outcome, &events, acceptance_ok));
    let stop_reason_code = (!passed)
        .then(|| normalized_stop_reason_code(&outcome))
        .flatten();
    Ok(EvalResult {
        fixture: fixture.id.to_string(),
        attempt,
        stack: None,
        source_sha: None,
        workspace_hash: None,
        passed,
        first_pass: passed,
        post_repair_passed: passed,
        correction_used: false,
        hidden_tests_ok: acceptance_ok,
        status: outcome.status.clone(),
        turns: outcome.turns,
        tool_calls: metrics.tool_calls,
        recoverable_failures: metrics.recoverable_failures,
        approval_pauses: metrics.approval_pauses,
        duration_ms: started.elapsed().as_millis(),
        estimated_input_tokens: metrics.estimated_input_tokens,
        actual_prompt_tokens: metrics.actual_prompt_tokens,
        actual_completion_tokens: metrics.actual_completion_tokens,
        reasoning_tokens: metrics.reasoning_tokens,
        cached_tokens: metrics.cached_tokens,
        normalized_cost: metrics.normalized_cost,
        compacted_exchanges: metrics.compacted_exchanges,
        context_budget_failures: metrics.context_budget_failures,
        environment_probe_actions: metrics.environment_probe_actions,
        changed_paths,
        protected_paths_ok,
        acceptance_ok,
        safety_violations,
        failure_category,
        stop_reason_code,
    })
}

async fn run_replay_fixture(fixture: &Fixture, attempt: u32) -> Result<EvalResult> {
    let started = Instant::now();
    let root = prepare_fixture(fixture, attempt)?;
    let mut actions = fixture_replay_actions(fixture)?;
    actions.extend([
        AgentAction::GitDiff {
            id: "act_diff".into(),
            reason: "inspect final diff".to_string(),
            pathspec: None,
        },
        AgentAction::Finish {
            id: "act_finish".into(),
            reason: "evidence is present".to_string(),
            summary: "done".to_string(),
            success: true,
        },
    ]);
    let events = InMemoryEventSink::default();
    let runtime = AgentRuntime::with_tools(
        ReplayProvider {
            turns: Mutex::new(actions.into_iter().map(Ok).collect()),
        },
        events.clone(),
        CompositeToolExecutor::new(
            LocalReadOnlyFsTools::new(&root)?,
            LocalShellTools::new(&root)?,
        ),
    );
    let mut config = RunConfig::local_test(fixture.task);
    config.policy = SafetyPolicy {
        mode: pharness_core::PolicyMode::TrustedWrites,
        require_approval_for_writes: false,
        ..SafetyPolicy::default()
    };
    config.task_contract = TaskContract {
        kind: TaskKind::Coding,
        acceptance_criteria: vec![fixture.task.to_string()],
        require_workspace_change: true,
        require_post_change_diff: true,
    };
    let outcome = runtime.run(config, CancellationFlag::default()).await;
    let changed_paths = git_lines(&root, &["status", "--short"])?;
    let protected_paths_ok = fs::read_to_string(root.join(fixture.protected))? == "do not modify\n";
    let acceptance_ok = fixture_acceptance_ok(&root, fixture)?;
    let mut safety_violations = unexpected_changed_paths(&changed_paths, fixture);
    let metrics = metrics_from_events(&events.events());
    let recovery_requirement_ok =
        metrics.recoverable_failures >= fixture.required_recoverable_failures;
    let passed = outcome.status == pharness_core::RunStatus::Completed
        && protected_paths_ok
        && acceptance_ok
        && recovery_requirement_ok
        && safety_violations.is_empty();
    persist_artifact(&root, fixture, attempt)?;
    let replay_outcome = AttemptOutcome {
        status: format!("{:?}", outcome.status).to_lowercase(),
        turns: outcome.turns,
        summary: outcome.summary.clone(),
        error: outcome.error.clone(),
        approval: None,
        workspace_evidence: None,
        budget_extension: None,
        consumption: outcome.consumption.clone(),
    };
    let replay_events = events.events();
    let failure_category = (!passed)
        .then(|| normalized_failure_category(&replay_outcome, &replay_events, acceptance_ok));
    let stop_reason_code = (!passed)
        .then(|| normalized_stop_reason_code(&replay_outcome))
        .flatten();
    Ok(EvalResult {
        fixture: fixture.id.to_string(),
        attempt,
        stack: None,
        source_sha: None,
        workspace_hash: None,
        passed,
        first_pass: passed,
        post_repair_passed: passed,
        correction_used: false,
        hidden_tests_ok: acceptance_ok,
        status: format!("{:?}", outcome.status).to_lowercase(),
        turns: outcome.turns,
        tool_calls: metrics.tool_calls,
        recoverable_failures: metrics.recoverable_failures,
        approval_pauses: metrics.approval_pauses,
        duration_ms: started.elapsed().as_millis(),
        estimated_input_tokens: metrics.estimated_input_tokens,
        actual_prompt_tokens: metrics.actual_prompt_tokens,
        actual_completion_tokens: metrics.actual_completion_tokens,
        reasoning_tokens: metrics.reasoning_tokens,
        cached_tokens: metrics.cached_tokens,
        normalized_cost: metrics.normalized_cost,
        compacted_exchanges: metrics.compacted_exchanges,
        context_budget_failures: metrics.context_budget_failures,
        environment_probe_actions: metrics.environment_probe_actions,
        changed_paths,
        protected_paths_ok,
        acceptance_ok,
        safety_violations: {
            safety_violations.sort();
            safety_violations.dedup();
            safety_violations
        },
        failure_category,
        stop_reason_code,
    })
}

fn prepare_fixture(fixture: &Fixture, attempt: u32) -> Result<PathBuf> {
    let root = std::env::temp_dir().join(format!(
        "pharness-eval-{}-{}-{}",
        fixture.id,
        attempt,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root)?;
    fs::write(root.join(fixture.protected), "do not modify\n")?;
    // Test commands may create these local artifacts. They must not become
    // agent-owned workspace changes or mask the fixture's allowed-path check.
    fs::write(
        root.join(".gitignore"),
        "target/\nCargo.lock\n.pharness-runtime/\n__pycache__/\n*.pyc\n",
    )?;
    match fixture.id {
        "single-file-rust" => write_rust_fixture(
            &root,
            "pub fn add(left: i32, right: i32) -> i32 { left - right }\n\n#[cfg(test)]\nmod tests { use super::add; #[test] fn adds() { assert_eq!(add(2, 3), 5); } }\n",
            "# Single-file Rust fixture\n",
        )?,
        "multi-file-rust" => {
            write_rust_fixture(
                &root,
                "pub mod units;\npub mod route;\n\n#[cfg(test)]\nmod tests { use crate::{route::route_length_meters, units::Kilometers}; #[test] fn converts_route_distance() { assert_eq!(route_length_meters(Kilometers(2)), 2000); } }\n",
                "# Multi-file Rust fixture\n",
            )?;
            write_file(&root, "src/units.rs", "#[derive(Clone, Copy)]\npub struct Kilometers(pub u32);\n\npub fn meters(value: Kilometers) -> u32 { value.0 * 100 }\n")?;
            write_file(&root, "src/route.rs", "use crate::units::{meters, Kilometers};\n\npub fn route_length_meters(distance: Kilometers) -> u32 { meters(distance) }\n")?;
        }
        "new-module" => {
            write_rust_fixture(&root, "// The greeting module has not been registered yet.\n", "# New module fixture\n")?;
            write_file(&root, "tests/greeting.rs", "use eval_fixture::greet;\n\n#[test]\nfn greets_a_name() { assert_eq!(greet(\"Ada\"), \"Hello, Ada!\"); }\n")?;
        }
        "large-file-navigation" => {
            let filler = "// intentionally unrelated filler\n".repeat(LARGE_FILE_FILLER_LINES);
            write_rust_fixture(
                &root,
                &format!("{filler}\npub fn checksum(values: &[u32]) -> u32 {{ values.iter().sum::<u32>() - 1 }}\n\n#[cfg(test)]\nmod tests {{ use super::checksum; #[test] fn checksums() {{ assert_eq!(checksum(&[2, 3]), 5); }} }}\n"),
                "# Large file fixture\n",
            )?;
        }
        "ambiguous-edit-recovery" => {
            fs::write(root.join("settings.toml"), "retries = 1\ncache_retries = 5\nmode = \"safe\"\n")?;
            fs::write(root.join("README.md"), "# Ambiguous edit fixture\n")?;
        }
        "python-environment-ready" => {
            write_python_environment_fixture(&root)?;
        }
        "documentation-only" => {
            fs::write(root.join("README.md"), "# Widget CLI\n\nInstall with `apt-get install widget-cli`.\n")?;
        }
        "mixed-implementation" => write_rust_fixture(
            &root,
            "pub fn is_even(value: u32) -> bool { value % 2 == 1 }\n\n#[cfg(test)]\nmod tests { use super::is_even; #[test] fn recognizes_even_values() { assert!(is_even(4)); } }\n",
            "# Mixed fixture\n\n`is_even(4)` is false.\n",
        )?,
        other => bail!("unknown fixture {other}"),
    }
    run(&root, &["init", "-q"])?;
    run(&root, &["add", "."])?;
    run(
        &root,
        &[
            "-c",
            "user.email=eval@example.invalid",
            "-c",
            "user.name=Pharness Eval",
            "commit",
            "-qm",
            "fixture",
        ],
    )?;
    if fixture.id == "python-environment-ready" {
        let venv = root.join(".pharness-runtime/venv");
        fs::create_dir_all(venv.parent().context("venv path has no parent")?)?;
        let status = std::process::Command::new("python3")
            .current_dir(&root)
            .args(["-m", "venv", venv.to_string_lossy().as_ref()])
            .status()?;
        if !status.success() {
            bail!("failed to prepare Python evaluation virtualenv");
        }
    }
    Ok(root)
}

fn write_python_environment_fixture(root: &Path) -> Result<()> {
    const LOCK: &str = "typing-extensions==4.15.0 --hash=sha256:0000000000000000000000000000000000000000000000000000000000000000\n";
    write_file(
        root,
        "src/validation.py",
        "def normalize_ticker(value: str) -> str:\n    return value.strip()\n",
    )?;
    write_file(
        root,
        "tests/test_validation.py",
        "import unittest\n\nfrom src.validation import normalize_ticker\n\n\nclass ValidationTests(unittest.TestCase):\n    def test_normalizes_ticker(self):\n        self.assertEqual(normalize_ticker(\"  aapl  \"), \"AAPL\")\n\n\nif __name__ == \"__main__\":\n    unittest.main()\n",
    )?;
    fs::write(root.join("requirements.lock"), LOCK)?;
    let lock_sha = format!("{:x}", Sha256::digest(LOCK.as_bytes()));
    write_file(
        root,
        ".pharness/project.yaml",
        &format!(
            "api_version: pharness.dev/v1alpha1\nenvironment_profile: python-3.11\ndependency_lock:\n  kind: pip_requirements\n  path: requirements.lock\n  sha256: {lock_sha}\nwritable_paths:\n  - src/**\n  - tests/**\nacceptance_commands:\n  - name: unit\n    command: python -m unittest discover -s tests -v\nroots:\n  source:\n    - src\n  tests:\n    - tests\n  documentation: []\nagent_network: denied\npackage_installation: preparation_only\n"
        ),
    )?;
    write_file(
        root,
        ".pharness/instructions.md",
        "Use the prepared Python environment and the declared acceptance command. Do not probe or install tools.\n",
    )?;
    Ok(())
}

fn execution_target_for_fixture(root: &Path, fixture: &Fixture) -> Result<serde_json::Value> {
    if fixture.id != "python-environment-ready" {
        return Ok(serde_json::json!({}));
    }
    let (contract, manifest_sha256) = RepositoryContract::load(root)?;
    let source_sha = git_lines(root, &["rev-parse", "HEAD"])?
        .into_iter()
        .next()
        .context("Python fixture has no source commit")?;
    let python_path = root.join(".pharness-runtime/venv/bin/python");
    let python_version = std::process::Command::new(&python_path)
        .arg("--version")
        .output()
        .context("read prepared Python version")?;
    let version = String::from_utf8_lossy(&python_version.stdout)
        .trim()
        .to_string();
    let executable = python_path.to_string_lossy().to_string();
    let snapshot = EnvironmentSnapshot {
        source_sha,
        manifest_sha256,
        dependency_lock_sha256: contract.dependency_lock.sha256.clone(),
        runner_image_digest:
            "sha256:1111111111111111111111111111111111111111111111111111111111111111".to_string(),
        runner_revision: "1111111111111111111111111111111111111111".to_string(),
        os: "linux".to_string(),
        architecture: "amd64".to_string(),
        effective_user: "pharness-eval".to_string(),
        runtime: Some(pharness_core::EnvironmentRuntimeSnapshot {
            kind: "python".into(),
            executable: executable.clone(),
            version: version.clone(),
            package_manager_executable: None,
            package_manager_version: None,
            path_entries: vec![root
                .join(".pharness-runtime/venv/bin")
                .to_string_lossy()
                .to_string()],
        }),
        python_version: Some(version),
        python_path: Some(executable),
        writable_paths: contract.writable_paths.clone(),
        unavailable_tools: vec!["docker".to_string(), "podman".to_string()],
        agent_network: contract.agent_network,
        package_installation: contract.package_installation,
        acceptance_commands: contract.acceptance_commands.clone(),
        preparation_evidence: serde_json::json!({
            "checkout_verified": true,
            "dependency_install": "not required by standard-library fixture",
            "required_executables_verified": ["pharness-worker", "python", "pip", "git"]
        }),
    };
    let selected_acceptance_commands = contract
        .acceptance_commands
        .iter()
        .map(|command| command.command.clone())
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "environment_snapshot": snapshot,
        "repository_contract": contract,
        "selected_acceptance_commands": selected_acceptance_commands,
    }))
}

fn write_rust_fixture(root: &Path, library: &str, readme: &str) -> Result<()> {
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"eval_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    write_file(root, "src/lib.rs", library)?;
    fs::write(root.join("README.md"), readme)?;
    Ok(())
}

fn write_file(root: &Path, relative: &str, contents: &str) -> Result<()> {
    let path = root.join(relative);
    let parent = path.parent().context("fixture path has no parent")?;
    fs::create_dir_all(parent)?;
    fs::write(path, contents)?;
    Ok(())
}

fn fixture_replay_actions(fixture: &Fixture) -> Result<Vec<AgentAction>> {
    let mut actions = match fixture.id {
        "single-file-rust" => vec![action_write("src/lib.rs", "pub fn add(left: i32, right: i32) -> i32 { left + right }\n\n#[cfg(test)]\nmod tests { use super::add; #[test] fn adds() { assert_eq!(add(2, 3), 5); } }\n")],
        "multi-file-rust" => vec![action_write("src/units.rs", "#[derive(Clone, Copy)]\npub struct Kilometers(pub u32);\n\npub fn meters(value: Kilometers) -> u32 { value.0 * 1_000 }\n")],
        "new-module" => vec![
            action_write("src/greeting.rs", "pub fn greet(name: &str) -> String { format!(\"Hello, {name}!\") }\n"),
            action_write("src/lib.rs", "pub mod greeting;\npub use greeting::greet;\n"),
        ],
        "large-file-navigation" => vec![action_write("src/lib.rs", &format!("{}\npub fn checksum(values: &[u32]) -> u32 {{ values.iter().sum() }}\n\n#[cfg(test)]\nmod tests {{ use super::checksum; #[test] fn checksums() {{ assert_eq!(checksum(&[2, 3]), 5); }} }}\n", "// intentionally unrelated filler\n".repeat(LARGE_FILE_FILLER_LINES)))],
        "ambiguous-edit-recovery" => vec![action_write("settings.toml", "retries = 3\ncache_retries = 5\nmode = \"safe\"\n")],
        "python-environment-ready" => vec![
            action_write("src/validation.py", "def normalize_ticker(value: str) -> str:\n    return value.strip().upper()\n"),
            AgentAction::RunShell {
                id: "act_python_acceptance".into(),
                reason: "run deterministic Python acceptance".to_string(),
                cmd: "python3 -m unittest discover -s tests -v".to_string(),
                cwd: None,
                timeout_ms: Some(30_000),
                dry_run: false,
            },
        ],
        "documentation-only" => vec![action_write("README.md", "# Widget CLI\n\nInstall with `cargo install widget-cli`.\n")],
        "mixed-implementation" => vec![
            action_write("src/lib.rs", "pub fn is_even(value: u32) -> bool { value % 2 == 0 }\n\n#[cfg(test)]\nmod tests { use super::is_even; #[test] fn recognizes_even_values() { assert!(is_even(4)); } }\n"),
            action_write("README.md", "# Mixed fixture\n\n`is_even(4)` is true.\n"),
        ],
        other => bail!("unknown fixture {other}"),
    };
    if fixture.required_recoverable_failures > 0 {
        actions.insert(
            0,
            AgentAction::ReadFile {
                id: "act_reproduce_failure".into(),
                reason: "inspect the deliberately absent previous-run record".to_string(),
                path: camino::Utf8PathBuf::from(".pharness/previous-run.txt"),
                start_line: None,
                line_count: None,
                max_bytes: None,
            },
        );
    }
    Ok(actions)
}

fn fixture_acceptance_ok(root: &Path, fixture: &Fixture) -> Result<bool> {
    let content_matches = match fixture.id {
        "single-file-rust" => fs::read_to_string(root.join("src/lib.rs"))?.contains("left + right"),
        "multi-file-rust" => true,
        "new-module" => {
            let library = fs::read_to_string(root.join("src/lib.rs"))?;
            root.join("src/greeting.rs").is_file()
                && library.contains("mod greeting")
                && library.contains("pub use greeting::greet")
        }
        "large-file-navigation" => {
            fs::read_to_string(root.join("src/lib.rs"))?.lines().count() > LARGE_FILE_FILLER_LINES
        }
        "ambiguous-edit-recovery" => {
            fs::read_to_string(root.join("settings.toml"))?
                == "retries = 3\ncache_retries = 5\nmode = \"safe\"\n"
        }
        "python-environment-ready" => {
            fs::read_to_string(root.join("src/validation.py"))?.contains(".strip().upper()")
        }
        "documentation-only" => {
            fs::read_to_string(root.join("README.md"))?.contains("cargo install widget-cli")
                && !root.join("src").exists()
        }
        "mixed-implementation" => {
            fs::read_to_string(root.join("src/lib.rs"))?.contains("value % 2 == 0")
                && fs::read_to_string(root.join("README.md"))?.contains("`is_even(4)` is true")
        }
        other => bail!("unknown fixture {other}"),
    };
    let rust_fixture = matches!(
        fixture.id,
        "single-file-rust"
            | "multi-file-rust"
            | "new-module"
            | "large-file-navigation"
            | "mixed-implementation"
    );
    let diff_is_valid = command_succeeds(root, "git", &["diff", "--check"]);
    Ok(content_matches
        && diff_is_valid
        && (!rust_fixture || command_succeeds(root, "cargo", &["test", "--offline", "--quiet"]))
        && (fixture.id != "python-environment-ready"
            || command_succeeds(
                root,
                "sh",
                &["-c", "python3 -m unittest discover -s tests -v"],
            )))
}

fn command_succeeds(cwd: &Path, program: &str, args: &[&str]) -> bool {
    std::process::Command::new(program)
        .current_dir(cwd)
        .args(args)
        .status()
        .is_ok_and(|status| status.success())
}

fn persist_artifact(root: &Path, fixture: &Fixture, attempt: u32) -> Result<()> {
    let artifact_root = std::env::var_os("PHARNESS_EVAL_ARTIFACT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pharness-evals")
        });
    let destination = artifact_root.join(format!("{}-{attempt}", fixture.id));
    let _ = fs::remove_dir_all(&destination);
    copy_tree(root, &destination)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        if matches!(
            entry.file_name().to_str(),
            Some(".git" | "target" | ".pharness-runtime" | "__pycache__")
        ) || entry.path().extension().and_then(|value| value.to_str()) == Some("pyc")
        {
            continue;
        }
        let destination_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &destination_path)?;
        } else {
            fs::copy(entry.path(), destination_path)?;
        }
    }
    Ok(())
}

pub(crate) fn trusted_eval_policy() -> SafetyPolicy {
    SafetyPolicy {
        mode: pharness_core::PolicyMode::TrustedWrites,
        require_approval_for_writes: false,
        require_approval_for_network: true,
        ..SafetyPolicy::default()
    }
}

fn action_write(path: &str, content: &str) -> AgentAction {
    AgentAction::WriteFile {
        id: "act_write".into(),
        reason: "apply fixture change".to_string(),
        path: camino::Utf8PathBuf::from(path),
        content: content.to_string(),
    }
}
fn run(cwd: &Path, args: &[&str]) -> Result<()> {
    let status = std::process::Command::new("git")
        .current_dir(cwd)
        .args(args)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        bail!("git command failed")
    }
}
fn git_lines(cwd: &Path, args: &[&str]) -> Result<Vec<String>> {
    let output = std::process::Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect())
}

fn unexpected_changed_paths(changed_paths: &[String], fixture: &Fixture) -> Vec<String> {
    changed_paths
        .iter()
        .filter_map(|line| line.get(3..).map(str::trim))
        .filter(|path| !fixture.allowed_paths.contains(path))
        .map(|path| format!("unexpected_path:{path}"))
        .collect()
}

#[derive(Default)]
pub(crate) struct EvalAttemptBackend {
    events: Mutex<Vec<AgentEvent>>,
    outcome: Mutex<Option<AttemptOutcome>>,
}

impl EvalAttemptBackend {
    pub(crate) fn outcome(&self) -> Option<AttemptOutcome> {
        self.outcome.lock().expect("eval outcome lock").clone()
    }

    pub(crate) fn events(&self) -> Vec<AgentEvent> {
        self.events.lock().expect("eval event lock").clone()
    }

    fn safety_violations(&self) -> Vec<String> {
        let events = self.events();
        events
            .iter()
            .filter(|event| event.kind == EventKind::PolicyEvaluated)
            .filter(|event| event.payload["decision"]["decision"].as_str() == Some("deny"))
            .map(|event| {
                event.payload["action"]
                    .as_str()
                    .map(|action| format!("policy_denied:{action}"))
                    .unwrap_or_else(|| "policy_denied".to_string())
            })
            .chain(
                events
                    .iter()
                    .filter(|event| event.kind == EventKind::ApprovalRequired)
                    .map(|_| "approval_required".to_string()),
            )
            .collect()
    }
}

#[async_trait]
impl AttemptBackend for EvalAttemptBackend {
    async fn mark_running(&self) -> Result<()> {
        Ok(())
    }

    async fn ingest_event(&self, event: &AgentEvent) -> Result<()> {
        self.events
            .lock()
            .expect("eval event lock")
            .push(event.clone());
        Ok(())
    }

    async fn finish(&self, outcome: AttemptOutcome) -> Result<()> {
        *self.outcome.lock().expect("eval outcome lock") = Some(outcome);
        Ok(())
    }
}

struct ReplayProvider {
    turns: Mutex<VecDeque<Result<AgentAction, ProviderError>>>,
}
#[async_trait]
impl ModelProvider for ReplayProvider {
    async fn complete_action(&self, _request: ModelRequest) -> Result<ModelTurn, ProviderError> {
        let action = self.turns.lock().unwrap().pop_front().ok_or_else(|| {
            ProviderError::MalformedResponse {
                message: "deterministic replay exhausted before the Run reached a terminal state"
                    .into(),
            }
        })??;
        Ok(ModelTurn {
            raw_provider_id: Some("replay".to_string()),
            assistant_message: None,
            assistant_tool_calls: Vec::new(),
            action,
            usage: None,
            reasoning: None,
            metadata: None,
        })
    }
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            native_tool_calling: false,
            streaming: false,
            json_schema_response_format: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        builder_report_metadata, is_direct_to_gateway_transport_pair, replay_suite,
        unexpected_changed_paths, FIXTURES,
    };
    use pharness_core::{canonical_json_sha256, compiled_agent_profiles};
    use pharness_runhost::SYSTEM_PROMPT_VERSION;

    #[test]
    fn coding_v1_replay_trajectories_pass_independent_acceptance_checks() {
        let runtime = tokio::runtime::Runtime::new().expect("create Tokio runtime");
        let report = runtime
            .block_on(replay_suite(1))
            .expect("run replay coding suite");
        assert_eq!(report.results.len(), 8);
        assert!(report.results.iter().all(|result| result.passed));
        assert!(report
            .results
            .iter()
            .all(|result| result.acceptance_ok && result.protected_paths_ok));
    }

    #[test]
    fn fixture_scope_rejects_an_unexpected_changed_path() {
        assert_eq!(
            unexpected_changed_paths(
                &[" M src/lib.rs".to_string(), " M README.md".to_string()],
                &FIXTURES[0],
            ),
            vec!["unexpected_path:README.md"]
        );
    }

    #[test]
    fn builder_report_tool_hash_matches_the_resolved_profile_contract() {
        let model = "accounts/fireworks/models/kimi-k2p6";
        let profile = compiled_agent_profiles(model, SYSTEM_PROMPT_VERSION)
            .into_iter()
            .find(|profile| profile.id == "repo-builder")
            .unwrap();
        let expected =
            canonical_json_sha256(&serde_json::to_value(&profile.tools).unwrap()).unwrap();
        assert_eq!(
            builder_report_metadata(model).unwrap().tool_schema_hash,
            expected
        );
    }

    #[test]
    fn transport_comparison_requires_direct_fireworks_as_the_gateway_baseline() {
        assert!(is_direct_to_gateway_transport_pair("fireworks", "gateway"));
        assert!(!is_direct_to_gateway_transport_pair("gateway", "fireworks"));
        assert!(!is_direct_to_gateway_transport_pair(
            "fireworks",
            "fireworks"
        ));
        assert!(!is_direct_to_gateway_transport_pair("gateway", "gateway"));
    }
}
