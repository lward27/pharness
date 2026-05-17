#![forbid(unsafe_code)]

use anyhow::{bail, Context};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Parser)]
#[command(name = "pharness")]
#[command(about = "Machine-facing control CLI for pharness runs")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Submit a run to the local pharness API and wait for a structured result.
    Run(RunArgs),
    /// Inspect existing runs.
    Runs {
        #[command(subcommand)]
        command: RunCommand,
    },
    /// Print effective API/worker configuration.
    Config(ApiArgs),
    /// Execute typed read-only capabilities without invoking the model.
    Capabilities {
        #[command(subcommand)]
        command: CapabilityCommand,
    },
    /// Inspect and decide run approvals.
    Approvals {
        #[command(subcommand)]
        command: ApprovalCommand,
    },
    /// Inspect persisted run artifacts.
    Artifacts {
        #[command(subcommand)]
        command: ArtifactCommand,
    },
    /// Fireworks utility commands.
    Fireworks {
        #[command(subcommand)]
        command: FireworksCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ApprovalCommand {
    /// List approvals.
    List(ApprovalListArgs),
    /// Approve the pending approval for a run.
    Approve(ApprovalDecisionArgs),
    /// Deny the pending approval for a run.
    Deny(ApprovalDecisionArgs),
}

#[derive(Debug, Subcommand)]
enum RunCommand {
    /// Fetch one run by id.
    Get(RunGetArgs),
    /// Fetch stored file diffs for one run.
    Diff(RunDiffArgs),
}

#[derive(Debug, Subcommand)]
enum ArtifactCommand {
    /// List artifacts produced by one run.
    List(ArtifactListArgs),
    /// Fetch one artifact by id.
    Get(ArtifactGetArgs),
}

#[derive(Debug, Subcommand)]
enum CapabilityCommand {
    /// Read Kubernetes resources through the typed kubernetes_get capability.
    KubernetesGet(KubernetesGetArgs),
    /// Read an Argo CD Application through the typed argo_get_app capability.
    ArgoGetApp(ArgoGetAppArgs),
    /// Run a read-only Prometheus instant query.
    PrometheusQuery(PrometheusQueryArgs),
    /// Read Tekton PipelineRuns through the typed tekton_get_pipeline_runs capability.
    TektonGetPipelineRuns(TektonGetRunsArgs),
    /// Read Tekton TaskRuns through the typed tekton_get_task_runs capability.
    TektonGetTaskRuns(TektonGetRunsArgs),
    /// Analyze one Tekton PipelineRun and related TaskRuns.
    TektonAnalyzePipelineRun(TektonAnalyzePipelineRunArgs),
}

#[derive(Debug, Subcommand)]
enum FireworksCommand {
    /// List serverless models visible to FIREWORKS_API_KEY.
    Models(FireworksModelsArgs),
}

#[derive(Debug, Parser)]
struct RunArgs {
    #[arg(long)]
    task: String,
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    cwd: Option<String>,
    #[arg(long, default_value_t = 40)]
    max_turns: u32,
    #[arg(long)]
    no_wait: bool,
    /// Print run events to stderr while waiting. Final machine JSON stays on stdout.
    #[arg(long)]
    follow_events: bool,
    #[arg(long, default_value_t = 500)]
    poll_interval_ms: u64,
    #[arg(long, default_value_t = 300_000)]
    timeout_ms: u64,
}

#[derive(Debug, Parser)]
struct RunGetArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    run_id: String,
    #[arg(long)]
    with_events: bool,
}

#[derive(Debug, Parser)]
struct RunDiffArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    run_id: String,
}

#[derive(Debug, Parser)]
struct ApiArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
}

#[derive(Debug, Parser)]
struct KubernetesGetArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    resource: String,
    #[arg(long)]
    namespace: Option<String>,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    all_namespaces: bool,
    #[arg(long)]
    label_selector: Option<String>,
}

#[derive(Debug, Parser)]
struct ArgoGetAppArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    app: String,
}

#[derive(Debug, Parser)]
struct PrometheusQueryArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    query: String,
}

#[derive(Debug, Parser)]
struct TektonGetRunsArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    namespace: Option<String>,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    all_namespaces: bool,
    #[arg(long)]
    label_selector: Option<String>,
}

#[derive(Debug, Parser)]
struct TektonAnalyzePipelineRunArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    namespace: String,
    #[arg(long)]
    name: String,
}

#[derive(Debug, Parser)]
struct ApprovalListArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long, default_value = "pending")]
    status: String,
    #[arg(long, default_value_t = 50)]
    limit: u32,
}

#[derive(Debug, Parser)]
struct ApprovalDecisionArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    run_id: String,
    #[arg(long)]
    decided_by: Option<String>,
    #[arg(long)]
    reason: Option<String>,
    /// Wait for the run to reach a terminal status after deciding.
    #[arg(long)]
    wait: bool,
    /// Print new run events to stderr while waiting. Final machine JSON stays on stdout.
    #[arg(long)]
    follow_events: bool,
    #[arg(long, default_value_t = 500)]
    poll_interval_ms: u64,
    #[arg(long, default_value_t = 300_000)]
    timeout_ms: u64,
}

#[derive(Debug, Parser)]
struct ArtifactListArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    run_id: String,
}

#[derive(Debug, Parser)]
struct ArtifactGetArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    artifact_id: String,
}

#[derive(Debug, Parser)]
struct FireworksModelsArgs {
    #[arg(long, env = "FIREWORKS_API_KEY")]
    api_key: String,
    #[arg(long, default_value = "fireworks")]
    account: String,
    #[arg(long, default_value = "supports_serverless=true")]
    filter: String,
    #[arg(long, default_value_t = 50)]
    page_size: u32,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Run(args) => run(args).await?,
        Command::Runs { command } => match command {
            RunCommand::Get(args) => get_run(args).await?,
            RunCommand::Diff(args) => get_run_diff(args).await?,
        },
        Command::Config(args) => config(args).await?,
        Command::Capabilities { command } => match command {
            CapabilityCommand::KubernetesGet(args) => kubernetes_get(args).await?,
            CapabilityCommand::ArgoGetApp(args) => argo_get_app(args).await?,
            CapabilityCommand::PrometheusQuery(args) => prometheus_query(args).await?,
            CapabilityCommand::TektonGetPipelineRuns(args) => {
                tekton_get_pipeline_runs(args).await?
            }
            CapabilityCommand::TektonGetTaskRuns(args) => tekton_get_task_runs(args).await?,
            CapabilityCommand::TektonAnalyzePipelineRun(args) => {
                tekton_analyze_pipeline_run(args).await?
            }
        },
        Command::Approvals { command } => match command {
            ApprovalCommand::List(args) => list_approvals(args).await?,
            ApprovalCommand::Approve(args) => decide_approval(args, "approve").await?,
            ApprovalCommand::Deny(args) => decide_approval(args, "deny").await?,
        },
        Command::Artifacts { command } => match command {
            ArtifactCommand::List(args) => list_artifacts(args).await?,
            ArtifactCommand::Get(args) => get_artifact(args).await?,
        },
        Command::Fireworks { command } => match command {
            FireworksCommand::Models(args) => fireworks_models(args).await?,
        },
    }

    Ok(())
}

async fn kubernetes_get(args: KubernetesGetArgs) -> anyhow::Result<()> {
    execute_capability(
        &args.api_url,
        serde_json::json!({
            "action": "kubernetes_get",
            "id": "cli.kubernetes_get",
            "reason": "direct CLI capability execution",
            "resource": args.resource,
            "namespace": args.namespace,
            "name": args.name,
            "all_namespaces": args.all_namespaces,
            "label_selector": args.label_selector,
        }),
    )
    .await
}

async fn argo_get_app(args: ArgoGetAppArgs) -> anyhow::Result<()> {
    execute_capability(
        &args.api_url,
        serde_json::json!({
            "action": "argo_get_app",
            "id": "cli.argo_get_app",
            "reason": "direct CLI capability execution",
            "app": args.app,
        }),
    )
    .await
}

async fn prometheus_query(args: PrometheusQueryArgs) -> anyhow::Result<()> {
    execute_capability(
        &args.api_url,
        serde_json::json!({
            "action": "prometheus_query",
            "id": "cli.prometheus_query",
            "reason": "direct CLI capability execution",
            "query": args.query,
        }),
    )
    .await
}

async fn tekton_get_pipeline_runs(args: TektonGetRunsArgs) -> anyhow::Result<()> {
    let api_url = args.api_url.clone();
    tekton_get_runs(
        &api_url,
        "tekton_get_pipeline_runs",
        "cli.tekton_get_pipeline_runs",
        args,
    )
    .await
}

async fn tekton_get_task_runs(args: TektonGetRunsArgs) -> anyhow::Result<()> {
    let api_url = args.api_url.clone();
    tekton_get_runs(
        &api_url,
        "tekton_get_task_runs",
        "cli.tekton_get_task_runs",
        args,
    )
    .await
}

async fn tekton_get_runs(
    api_url: &str,
    action: &str,
    id: &str,
    args: TektonGetRunsArgs,
) -> anyhow::Result<()> {
    execute_capability(
        api_url,
        serde_json::json!({
            "action": action,
            "id": id,
            "reason": "direct CLI capability execution",
            "namespace": args.namespace,
            "name": args.name,
            "all_namespaces": args.all_namespaces,
            "label_selector": args.label_selector,
        }),
    )
    .await
}

async fn tekton_analyze_pipeline_run(args: TektonAnalyzePipelineRunArgs) -> anyhow::Result<()> {
    execute_capability(
        &args.api_url,
        serde_json::json!({
            "action": "tekton_analyze_pipeline_run",
            "id": "cli.tekton_analyze_pipeline_run",
            "reason": "direct CLI capability execution",
            "namespace": args.namespace,
            "name": args.name,
        }),
    )
    .await
}

async fn execute_capability(api_url_base: &str, action: serde_json::Value) -> anyhow::Result<()> {
    let http = reqwest::Client::new();
    let response = http
        .post(api_url(api_url_base, "/api/capabilities/execute"))
        .json(&serde_json::json!({ "action": action }))
        .send()
        .await
        .context("failed to execute capability")?
        .error_for_status()
        .context("pharness API rejected capability execution")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode capability execution response")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn config(args: ApiArgs) -> anyhow::Result<()> {
    let http = reqwest::Client::new();
    let config = http
        .get(api_url(&args.api_url, "/api/config/effective"))
        .send()
        .await
        .context("failed to fetch effective config")?
        .error_for_status()
        .context("pharness API rejected config request")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode effective config")?;

    println!("{}", serde_json::to_string_pretty(&config)?);
    Ok(())
}

async fn fireworks_models(args: FireworksModelsArgs) -> anyhow::Result<()> {
    let http = reqwest::Client::new();
    let url = format!(
        "https://api.fireworks.ai/v1/accounts/{}/models",
        args.account
    );
    let body = http
        .get(url)
        .bearer_auth(args.api_key)
        .query(&[
            ("filter", args.filter.as_str()),
            ("pageSize", &args.page_size.to_string()),
        ])
        .send()
        .await
        .context("failed to list Fireworks models")?
        .error_for_status()
        .context("Fireworks rejected model list request")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode Fireworks model list")?;

    let models = extract_model_summaries(&body);
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "account": args.account,
            "filter": args.filter,
            "count": models.len(),
            "models": models,
        }))?
    );

    Ok(())
}

async fn list_approvals(args: ApprovalListArgs) -> anyhow::Result<()> {
    let http = reqwest::Client::new();
    let response = http
        .get(api_url(&args.api_url, "/api/approvals"))
        .query(&[
            ("status", args.status.as_str()),
            ("limit", &args.limit.to_string()),
        ])
        .send()
        .await
        .context("failed to fetch approvals")?
        .error_for_status()
        .context("pharness API rejected approval list")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode approval list")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn decide_approval(args: ApprovalDecisionArgs, decision: &str) -> anyhow::Result<()> {
    let http = reqwest::Client::new();
    let api_url_base = args.api_url.clone();
    let run_id = args.run_id.clone();
    let wait = args.wait;
    let follow_events = args.follow_events;
    let poll_interval_ms = args.poll_interval_ms;
    let timeout_ms = args.timeout_ms;
    let response = http
        .post(api_url(
            &api_url_base,
            &format!("/api/runs/{run_id}/approvals"),
        ))
        .json(&ApprovalDecisionRequest {
            decision,
            decided_by: args.decided_by,
            reason: args.reason,
        })
        .send()
        .await
        .context("failed to submit approval decision")?
        .error_for_status()
        .context("pharness API rejected approval decision")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode approval decision")?;

    if !wait {
        println!("{}", serde_json::to_string_pretty(&response)?);
        return Ok(());
    }

    let seen_events = if follow_events {
        emit_new_events(&api_url_base, &http, &run_id, 0).await?
    } else {
        0
    };
    let (run, events) = wait_for_run(
        &api_url_base,
        &http,
        &run_id,
        follow_events,
        seen_events,
        poll_interval_ms,
        timeout_ms,
    )
    .await?;

    println!(
        "{}",
        serde_json::to_string_pretty(&ApprovalDecisionWaitOutput {
            decision: response,
            wait_status: "completed",
            run,
            events,
        })?
    );
    Ok(())
}

async fn get_run(args: RunGetArgs) -> anyhow::Result<()> {
    let http = reqwest::Client::new();
    let run = fetch_run(&args.api_url, &http, &args.run_id).await?;
    if args.with_events {
        let events = fetch_events(&args.api_url, &http, &args.run_id).await?;
        println!(
            "{}",
            serde_json::to_string_pretty(&RunGetOutput {
                run,
                events: Some(events),
            })?
        );
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&RunGetOutput { run, events: None })?
        );
    }

    Ok(())
}

async fn get_run_diff(args: RunDiffArgs) -> anyhow::Result<()> {
    let http = reqwest::Client::new();
    let response = http
        .get(api_url(
            &args.api_url,
            &format!("/api/runs/{}/diff", args.run_id),
        ))
        .send()
        .await
        .context("failed to fetch run diff")?
        .error_for_status()
        .context("pharness API rejected run diff")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode run diff")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn list_artifacts(args: ArtifactListArgs) -> anyhow::Result<()> {
    let http = reqwest::Client::new();
    let response = http
        .get(api_url(
            &args.api_url,
            &format!("/api/runs/{}/artifacts", args.run_id),
        ))
        .send()
        .await
        .context("failed to fetch run artifacts")?
        .error_for_status()
        .context("pharness API rejected artifact list")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode artifact list")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn get_artifact(args: ArtifactGetArgs) -> anyhow::Result<()> {
    let http = reqwest::Client::new();
    let response = http
        .get(api_url(
            &args.api_url,
            &format!("/api/artifacts/{}", args.artifact_id),
        ))
        .send()
        .await
        .context("failed to fetch artifact")?
        .error_for_status()
        .context("pharness API rejected artifact fetch")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode artifact")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn run(args: RunArgs) -> anyhow::Result<()> {
    let http = reqwest::Client::new();
    let create_url = api_url(&args.api_url, "/api/runs");
    let run = http
        .post(create_url)
        .json(&CreateRunRequest {
            task: args.task,
            cwd: args.cwd,
            max_turns: Some(args.max_turns),
        })
        .send()
        .await
        .context("failed to submit run")?
        .error_for_status()
        .context("pharness API rejected run")?
        .json::<RunResponse>()
        .await
        .context("failed to decode create run response")?;

    let mut seen_events = 0;
    if args.follow_events {
        seen_events = emit_new_events(&args.api_url, &http, &run.id, seen_events).await?;
    }

    let (run, wait_status) = if args.no_wait {
        (run, "not_waited")
    } else {
        let run_id = run.id.clone();
        let (run, _) = wait_for_run(
            &args.api_url,
            &http,
            &run_id,
            args.follow_events,
            seen_events,
            args.poll_interval_ms,
            args.timeout_ms,
        )
        .await?;
        (run, "completed")
    };

    let output = output(&args.api_url, &http, run, wait_status).await?;
    print_json(&output)?;
    Ok(())
}

async fn wait_for_run(
    api_url_base: &str,
    http: &reqwest::Client,
    run_id: &str,
    follow_events: bool,
    mut seen_events: usize,
    poll_interval_ms: u64,
    timeout_ms: u64,
) -> anyhow::Result<(RunResponse, Vec<serde_json::Value>)> {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let run = fetch_run(api_url_base, http, run_id).await?;
        if follow_events {
            seen_events = emit_new_events(api_url_base, http, run_id, seen_events).await?;
        }

        if is_terminal(&run.status) {
            let events = fetch_events(api_url_base, http, run_id).await?;
            return Ok((run, events));
        }

        if tokio::time::Instant::now() >= deadline {
            let events = fetch_events(api_url_base, http, run_id).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&CliRunOutput {
                    wait_status: "timeout".to_string(),
                    run,
                    events,
                })?
            );
            bail!("timed out waiting for pharness run");
        }

        tokio::time::sleep(Duration::from_millis(poll_interval_ms)).await;
    }
}

async fn fetch_run(
    api_url_base: &str,
    http: &reqwest::Client,
    run_id: &str,
) -> anyhow::Result<RunResponse> {
    http.get(api_url(api_url_base, &format!("/api/runs/{run_id}")))
        .send()
        .await
        .context("failed to fetch run")?
        .error_for_status()
        .context("pharness API rejected run fetch")?
        .json::<RunResponse>()
        .await
        .context("failed to decode run response")
}

async fn output(
    api_url_base: &str,
    http: &reqwest::Client,
    run: RunResponse,
    wait_status: &str,
) -> anyhow::Result<CliRunOutput> {
    let events = fetch_events(api_url_base, http, &run.id).await?;

    Ok(CliRunOutput {
        wait_status: wait_status.to_string(),
        run,
        events,
    })
}

async fn emit_new_events(
    api_url_base: &str,
    http: &reqwest::Client,
    run_id: &str,
    seen_events: usize,
) -> anyhow::Result<usize> {
    let events = fetch_events(api_url_base, http, run_id).await?;
    for event in events.iter().skip(seen_events) {
        eprintln!("{}", event_log_line(event));
    }
    Ok(events.len())
}

async fn fetch_events(
    api_url_base: &str,
    http: &reqwest::Client,
    run_id: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    Ok(http
        .get(api_url(api_url_base, &format!("/api/runs/{run_id}/events")))
        .send()
        .await
        .context("failed to fetch run events")?
        .error_for_status()
        .context("pharness API rejected event fetch")?
        .json::<EventsResponse>()
        .await
        .context("failed to decode event response")?
        .events)
}

fn print_json(output: &CliRunOutput) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(output)?);
    Ok(())
}

fn event_log_line(event: &serde_json::Value) -> String {
    let seq = event
        .get("seq")
        .and_then(serde_json::Value::as_u64)
        .map_or_else(|| "?".to_string(), |seq| seq.to_string());
    let kind = event
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let payload = event.get("payload").unwrap_or(&serde_json::Value::Null);

    let mut fields = Vec::new();
    push_str_field(&mut fields, "action", payload.get("action"));
    push_str_field(&mut fields, "summary", payload.get("summary"));
    push_str_field(&mut fields, "error", payload.get("error"));
    push_decision_field(&mut fields, payload.get("decision"));

    if fields.is_empty() {
        format!("[{seq}] {kind}")
    } else {
        format!("[{seq}] {kind} {}", fields.join(" "))
    }
}

fn push_str_field(fields: &mut Vec<String>, name: &str, value: Option<&serde_json::Value>) {
    if let Some(value) = value.and_then(serde_json::Value::as_str) {
        fields.push(format!("{name}={value:?}"));
    }
}

fn push_decision_field(fields: &mut Vec<String>, value: Option<&serde_json::Value>) {
    match value {
        Some(serde_json::Value::String(decision)) => {
            fields.push(format!("decision={decision:?}"));
        }
        Some(serde_json::Value::Object(decision)) => {
            if let Some(decision) = decision.get("decision").and_then(serde_json::Value::as_str) {
                fields.push(format!("decision={decision:?}"));
            }
        }
        _ => {}
    }
}

fn is_terminal(status: &str) -> bool {
    matches!(
        status,
        "completed" | "failed" | "cancelled" | "approval_required"
    )
}

fn api_url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn extract_model_summaries(body: &serde_json::Value) -> Vec<ModelSummary> {
    let Some(models) = body
        .get("models")
        .or_else(|| body.get("items"))
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };

    models
        .iter()
        .filter_map(|model| {
            let name = model.get("name")?.as_str()?.to_string();
            Some(ModelSummary {
                name,
                display_name: model
                    .get("displayName")
                    .or_else(|| model.get("display_name"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect()
}

#[derive(Debug, Serialize)]
struct CreateRunRequest {
    task: String,
    cwd: Option<String>,
    max_turns: Option<u32>,
}

#[derive(Debug, Serialize)]
struct ApprovalDecisionRequest<'a> {
    decision: &'a str,
    decided_by: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunResponse {
    id: String,
    status: String,
    task: String,
    max_turns: u32,
    result: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct EventsResponse {
    events: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct CliRunOutput {
    wait_status: String,
    run: RunResponse,
    events: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct RunGetOutput {
    run: RunResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    events: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Serialize)]
struct ApprovalDecisionWaitOutput {
    decision: serde_json::Value,
    wait_status: &'static str,
    run: RunResponse,
    events: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ModelSummary {
    name: String,
    display_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{api_url, event_log_line, extract_model_summaries, is_terminal};

    #[test]
    fn builds_api_urls_without_double_slashes() {
        assert_eq!(
            api_url("http://127.0.0.1:4777/", "/api/runs"),
            "http://127.0.0.1:4777/api/runs"
        );
    }

    #[test]
    fn detects_terminal_statuses() {
        assert!(is_terminal("completed"));
        assert!(is_terminal("approval_required"));
        assert!(!is_terminal("queued"));
        assert!(!is_terminal("running"));
    }

    #[test]
    fn extracts_fireworks_model_summaries() {
        let models = extract_model_summaries(&serde_json::json!({
            "models": [
                {
                    "name": "accounts/fireworks/models/kimi-k2p6",
                    "displayName": "Kimi K2.6"
                }
            ]
        }));

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "accounts/fireworks/models/kimi-k2p6");
        assert_eq!(models[0].display_name.as_deref(), Some("Kimi K2.6"));
    }

    #[test]
    fn formats_event_log_lines_for_follow_output() {
        let line = event_log_line(&serde_json::json!({
            "seq": 6,
            "type": "policy.evaluated",
            "payload": {
                "action": "write_file",
                "decision": {
                    "decision": "ask",
                    "risk": "medium"
                }
            }
        }));

        assert_eq!(
            line,
            "[6] policy.evaluated action=\"write_file\" decision=\"ask\""
        );
    }
}
