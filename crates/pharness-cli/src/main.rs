#![forbid(unsafe_code)]

use anyhow::{bail, Context};
use clap::{Parser, Subcommand};
use futures::StreamExt;
use pharness_config::ApiRuntimeConfig;
use pharness_core::{PolicyMode, RunScope};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

/// Build the API client, attaching `Authorization: Bearer $PHARNESS_API_TOKEN`
/// when the operator token env is set. Deployed APIs require it; local
/// loopback APIs ignore it.
fn api_client() -> reqwest::Client {
    api_client_builder(reqwest::Client::builder())
}

fn api_client_with_timeout(timeout: Duration) -> reqwest::Client {
    api_client_builder(reqwest::Client::builder().timeout(timeout))
}

fn api_client_builder(builder: reqwest::ClientBuilder) -> reqwest::Client {
    let builder = match operator_token_from_env() {
        Some(token) => {
            let mut headers = reqwest::header::HeaderMap::new();
            match reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")) {
                Ok(mut value) => {
                    value.set_sensitive(true);
                    headers.insert(reqwest::header::AUTHORIZATION, value);
                }
                Err(_) => eprintln!("warning: PHARNESS_API_TOKEN is not a valid header value"),
            }
            builder.default_headers(headers)
        }
        None => builder,
    };

    builder
        .build()
        .expect("reqwest client construction must succeed")
}

fn operator_token_from_env() -> Option<String> {
    std::env::var("PHARNESS_API_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

const DEFAULT_CAPABILITY_TIMEOUT_MS: u64 = 60_000;

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
    /// Inspect or validate configuration.
    Config(ConfigArgs),
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
    /// Inspect normalized run observations.
    Observations {
        #[command(subcommand)]
        command: ObservationCommand,
    },
    /// Inspect durable incident candidates.
    Incidents {
        #[command(subcommand)]
        command: IncidentCommand,
    },
    /// Inspect durable remediation plan drafts.
    RemediationPlans {
        #[command(subcommand)]
        command: RemediationPlanCommand,
    },
    /// Inspect durable work plans.
    WorkPlans {
        #[command(subcommand)]
        command: WorkPlanCommand,
    },
    /// Inspect durable change sets.
    ChangeSets {
        #[command(subcommand)]
        command: ChangeSetCommand,
    },
    /// Inspect durable pipeline intents.
    PipelineIntents {
        #[command(subcommand)]
        command: PipelineIntentCommand,
    },
    /// Inspect operator-managed Tekton Pipeline contracts.
    PipelineContracts {
        #[command(subcommand)]
        command: PipelineContractCommand,
    },
    /// Inspect operator-managed Argo CD deployment contracts.
    DeploymentContracts {
        #[command(subcommand)]
        command: DeploymentContractCommand,
    },
    /// Inspect durable deployment intents.
    DeploymentIntents {
        #[command(subcommand)]
        command: DeploymentIntentCommand,
    },
    /// Inspect durable releases.
    Releases {
        #[command(subcommand)]
        command: ReleaseCommand,
    },
    /// Inspect durable registry evidence.
    RegistryEvidence {
        #[command(subcommand)]
        command: RegistryEvidenceCommand,
    },
    /// Inspect durable approval gates.
    ApprovalGates {
        #[command(subcommand)]
        command: ApprovalGateCommand,
    },
    /// Inspect and manage durable permission grants.
    PermissionGrants {
        #[command(subcommand)]
        command: PermissionGrantCommand,
    },
    /// Inspect durable audit events.
    AuditEvents(AuditEventListArgs),
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
    /// Summarize approval counts by status, kind, risk, and run scope.
    Summary(ApprovalSummaryArgs),
    /// Fetch one approval by id.
    Get(ApprovalGetArgs),
    /// Approve a pending approval by run id or approval id.
    Approve(ApprovalDecisionArgs),
    /// Deny a pending approval by run id or approval id.
    Deny(ApprovalDecisionArgs),
}

#[derive(Debug, Subcommand)]
enum RunCommand {
    /// List persisted runs.
    List(RunListArgs),
    /// Summarize run counts by status, scope, and age.
    Summary(RunSummaryArgs),
    /// Cancel one run by id.
    Cancel(RunCancelArgs),
    /// Fetch one run by id.
    Get(RunGetArgs),
    /// Fetch or stream durable events for one run.
    Events(RunEventsArgs),
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
enum ObservationCommand {
    /// List observations across runs, optionally filtered by run or observation metadata.
    List(ObservationListArgs),
    /// Fetch one observation by id.
    Get(ObservationGetArgs),
    /// Create an observation record.
    Create(ObservationCreateArgs),
}

#[derive(Debug, Subcommand)]
enum IncidentCommand {
    /// List incident candidates.
    List(IncidentListArgs),
    /// Fetch one incident candidate by id.
    Get(IncidentGetArgs),
    /// Create an incident candidate from an observation.
    Create(IncidentCreateArgs),
}

#[derive(Debug, Subcommand)]
enum RemediationPlanCommand {
    /// List remediation plan drafts.
    List(RemediationPlanListArgs),
    /// Fetch one remediation plan draft by id.
    Get(RemediationPlanGetArgs),
    /// Create a remediation plan draft from an incident.
    Create(RemediationPlanCreateArgs),
}

#[derive(Debug, Subcommand)]
enum WorkPlanCommand {
    /// List durable work plans.
    List(Box<WorkPlanListArgs>),
    /// Fetch one work plan by id.
    Get(WorkPlanGetArgs),
    /// Summarize whether one WorkPlan is ready for trusted-envelope execution.
    Readiness(WorkPlanReadinessArgs),
    /// Fetch one compact SDLC flow rooted at a WorkPlan.
    Flow(WorkPlanFlowArgs),
    /// Create or fetch a WorkPlan from one remediation plan.
    CreateFromRemediationPlan(WorkPlanCreateFromRemediationPlanArgs),
    /// Revise a WorkPlan and stale prior satisfied or waived gates on material changes.
    Revise(WorkPlanReviseArgs),
    /// Move a WorkPlan through its lifecycle state machine.
    Transition(WorkPlanTransitionArgs),
    /// Create a bounded trusted write envelope for one WorkPlan.
    CreateTrustedEnvelope(WorkPlanCreateTrustedEnvelopeArgs),
}

#[derive(Debug, Subcommand)]
enum ChangeSetCommand {
    /// List durable change sets.
    List(Box<ChangeSetListArgs>),
    /// Fetch one change set by id.
    Get(ChangeSetGetArgs),
    /// Summarize whether one ChangeSet is ready for trusted-envelope execution.
    Readiness(ChangeSetReadinessArgs),
    /// Fetch one compact SDLC flow rooted at a ChangeSet.
    Flow(ChangeSetFlowArgs),
    /// Create or fetch a ChangeSet for one WorkPlan.
    Create(ChangeSetCreateArgs),
    /// Revise a ChangeSet and stale prior satisfied or waived gates on material changes.
    Revise(ChangeSetReviseArgs),
    /// Move a ChangeSet through its lifecycle state machine.
    Transition(ChangeSetTransitionArgs),
    /// Create a bounded trusted write envelope for one ChangeSet.
    CreateTrustedEnvelope(ChangeSetCreateTrustedEnvelopeArgs),
}

#[derive(Debug, Subcommand)]
enum PipelineIntentCommand {
    /// List durable pipeline intents.
    List(Box<PipelineIntentListArgs>),
    /// Fetch one pipeline intent by id.
    Get(PipelineIntentGetArgs),
    /// Create or fetch a proposed PipelineIntent for one approved ChangeSet.
    CreateFromChangeSet(PipelineIntentCreateFromChangeSetArgs),
    /// Move a PipelineIntent through its lifecycle state machine.
    Transition(PipelineIntentTransitionArgs),
    /// Attach a Tekton PipelineRunAnalysis observation as PipelineIntent evidence.
    AttachEvidence(PipelineIntentAttachEvidenceArgs),
    /// Create a bounded supervised-autonomy envelope for one approved PipelineIntent.
    CreateTrustedEnvelope(PipelineIntentCreateTrustedEnvelopeArgs),
    /// Preview a PipelineRun by default; pass --apply to dispatch the dedicated executor Job.
    Execute(PipelineIntentExecuteArgs),
}

#[derive(Debug, Subcommand)]
enum PipelineContractCommand {
    /// List durable Pipeline contracts.
    List(Box<PipelineContractListArgs>),
    /// Fetch one Pipeline contract by id.
    Get(PipelineContractGetArgs),
    /// Create an active Pipeline contract for one namespace and PipelineRef.
    Create(PipelineContractCreateArgs),
    /// Atomically replace one active Pipeline contract with a new version.
    Replace(PipelineContractReplaceArgs),
    /// Retire an active Pipeline contract. This never deletes audit history.
    Retire(PipelineContractRetireArgs),
}

#[derive(Debug, Subcommand)]
enum DeploymentIntentCommand {
    /// List durable deployment intents.
    List(Box<DeploymentIntentListArgs>),
    /// Fetch one deployment intent by id.
    Get(DeploymentIntentGetArgs),
    /// Create or fetch a proposed DeploymentIntent for one approved PipelineIntent.
    CreateFromPipelineIntent(DeploymentIntentCreateFromPipelineIntentArgs),
    /// Move a DeploymentIntent through its lifecycle state machine.
    Transition(DeploymentIntentTransitionArgs),
    /// Attach an Argo CD Application observation as DeploymentIntent evidence.
    AttachEvidence(DeploymentIntentAttachEvidenceArgs),
}

#[derive(Debug, Subcommand)]
enum DeploymentContractCommand {
    /// List durable deployment contracts.
    List(Box<DeploymentContractListArgs>),
    /// Fetch one deployment contract by id.
    Get(DeploymentContractGetArgs),
    /// Create an active deployment contract for one exact Argo CD Application target.
    Create(DeploymentContractCreateArgs),
    /// Retire an active deployment contract without deleting its audit history.
    Retire(DeploymentContractRetireArgs),
}

#[derive(Debug, Subcommand)]
enum ReleaseCommand {
    /// List durable releases.
    List(Box<ReleaseListArgs>),
    /// Fetch one release by id.
    Get(ReleaseGetArgs),
    /// Create or fetch a proposed Release for one approved DeploymentIntent.
    CreateFromDeploymentIntent(ReleaseCreateFromDeploymentIntentArgs),
    /// Move a Release through its lifecycle state machine.
    Transition(ReleaseTransitionArgs),
    /// Attach a Prometheus or Loki observation as Release observability evidence.
    AttachEvidence(ReleaseAttachEvidenceArgs),
}

#[derive(Debug, Subcommand)]
enum RegistryEvidenceCommand {
    /// List durable registry evidence records.
    List(Box<RegistryEvidenceListArgs>),
    /// Fetch one registry evidence record by id.
    Get(RegistryEvidenceGetArgs),
    /// Create or fetch proposed RegistryEvidence for one approved Release.
    CreateFromRelease(Box<RegistryEvidenceCreateFromReleaseArgs>),
    /// Inspect an image and record the result as RegistryEvidence for one approved Release.
    CreateFromInspection(Box<RegistryEvidenceCreateFromInspectionArgs>),
    /// Move RegistryEvidence through its lifecycle state machine.
    Transition(RegistryEvidenceTransitionArgs),
}

#[derive(Debug, Subcommand)]
enum ApprovalGateCommand {
    /// List approval gates.
    List(Box<ApprovalGateListArgs>),
    /// Summarize approval gate counts by status, kind, risk, age, and resource.
    Summary(Box<ApprovalGateSummaryArgs>),
    /// Fetch one approval gate by id.
    Get(ApprovalGateGetArgs),
    /// Mark one pending gate as satisfied.
    Satisfy(ApprovalGateDecisionArgs),
    /// Mark one pending gate as waived.
    Waive(ApprovalGateDecisionArgs),
    /// Mark one pending gate as rejected.
    Reject(ApprovalGateDecisionArgs),
}

#[derive(Debug, Subcommand)]
enum PermissionGrantCommand {
    /// List permission grants.
    List(PermissionGrantListArgs),
    /// Create a permission grant record.
    Create(PermissionGrantCreateArgs),
    /// Fetch one permission grant by id.
    Get(PermissionGrantGetArgs),
    /// Revoke one permission grant by id.
    Revoke(PermissionGrantRevokeArgs),
}

#[derive(Debug, Subcommand)]
enum CapabilityCommand {
    /// Read Kubernetes resources through the typed kubernetes_get capability.
    KubernetesGet(KubernetesGetArgs),
    /// Read an Argo CD Application through the typed argo_get_app capability.
    ArgoGetApp(ArgoGetAppArgs),
    /// Run a read-only Prometheus instant query.
    PrometheusQuery(PrometheusQueryArgs),
    /// Read bounded Prometheus target, rule, and alert inventory.
    PrometheusInventory(PrometheusInventoryArgs),
    /// Read bounded Loki log lines through the typed loki_log_summary capability.
    LokiLogSummary(LokiLogSummaryArgs),
    /// Read Tekton PipelineRuns through the typed tekton_get_pipeline_runs capability.
    TektonGetPipelineRuns(TektonGetRunsArgs),
    /// Read Tekton TaskRuns through the typed tekton_get_task_runs capability.
    TektonGetTaskRuns(TektonGetRunsArgs),
    /// Analyze one Tekton PipelineRun and related TaskRuns.
    TektonAnalyzePipelineRun(TektonAnalyzePipelineRunArgs),
    /// Inspect an OCI/Docker registry image manifest anonymously.
    RegistryInspectImage(RegistryInspectImageArgs),
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
    /// Override the API's default policy mode for this run.
    #[arg(long)]
    policy_mode: Option<PolicyMode>,
    /// Optional SDLC namespace metadata for the run. Metadata only in V1.
    #[arg(long)]
    namespace: Option<String>,
    /// Optional repository metadata for the run. Metadata only in V1.
    #[arg(long)]
    repo: Option<String>,
    /// Optional branch metadata for the run. Metadata only in V1.
    #[arg(long)]
    branch: Option<String>,
    /// Optional WorkPlan envelope metadata for the run.
    #[arg(long)]
    work_plan_id: Option<String>,
    /// Optional ChangeSet envelope metadata for the run.
    #[arg(long)]
    change_set_id: Option<String>,
    /// Mark the run scope as production-impacting metadata. Does not grant production mutation.
    #[arg(long)]
    production_impacting: bool,
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
struct RunEventsArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    run_id: String,
    /// Return only events with seq greater than this cursor.
    #[arg(long)]
    after_seq: Option<u64>,
    /// Use the Server-Sent Events endpoint and print newline-delimited event JSON.
    #[arg(long)]
    stream: bool,
    /// Maximum time to wait for the next streamed event before failing.
    #[arg(long, default_value_t = 300_000)]
    timeout_ms: u64,
}

#[derive(Debug, Parser)]
struct RunListArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    namespace: Option<String>,
    #[arg(long)]
    repo: Option<String>,
    #[arg(long)]
    branch: Option<String>,
    #[arg(long)]
    production_impacting: Option<bool>,
    /// Include runs started at or after this Unix epoch millisecond.
    #[arg(long)]
    started_after_ms: Option<i64>,
    /// Include runs started at or before this Unix epoch millisecond.
    #[arg(long)]
    started_before_ms: Option<i64>,
    #[arg(long, default_value_t = 50)]
    limit: u32,
    #[arg(long, default_value_t = 0)]
    offset: u32,
}

#[derive(Debug, Parser)]
struct RunSummaryArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    namespace: Option<String>,
    #[arg(long)]
    repo: Option<String>,
    #[arg(long)]
    branch: Option<String>,
    #[arg(long)]
    production_impacting: Option<bool>,
    /// Include runs started at or after this Unix epoch millisecond.
    #[arg(long)]
    started_after_ms: Option<i64>,
    /// Include runs started at or before this Unix epoch millisecond.
    #[arg(long)]
    started_before_ms: Option<i64>,
}

#[derive(Debug, Parser)]
struct RunCancelArgs {
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
struct ConfigArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[command(subcommand)]
    command: Option<ConfigCommand>,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Validate a local pharness TOML config file without starting the API.
    Validate(ConfigValidateArgs),
}

#[derive(Debug, Parser)]
struct ConfigValidateArgs {
    #[arg(long)]
    file: PathBuf,
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
    /// Cancel direct capability execution if the API does not finish in this many milliseconds.
    #[arg(long, default_value_t = DEFAULT_CAPABILITY_TIMEOUT_MS)]
    timeout_ms: u64,
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
    /// Cancel direct capability execution if the API does not finish in this many milliseconds.
    #[arg(long, default_value_t = DEFAULT_CAPABILITY_TIMEOUT_MS)]
    timeout_ms: u64,
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
    /// Cancel direct capability execution if the API does not finish in this many milliseconds.
    #[arg(long, default_value_t = DEFAULT_CAPABILITY_TIMEOUT_MS)]
    timeout_ms: u64,
}

#[derive(Debug, Parser)]
struct PrometheusInventoryArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    /// Cancel direct capability execution if the API does not finish in this many milliseconds.
    #[arg(long, default_value_t = DEFAULT_CAPABILITY_TIMEOUT_MS)]
    timeout_ms: u64,
}

#[derive(Debug, Parser)]
struct LokiLogSummaryArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    query: String,
    #[arg(long)]
    since_seconds: Option<u64>,
    #[arg(long)]
    limit: Option<u32>,
    /// Cancel direct capability execution if the API does not finish in this many milliseconds.
    #[arg(long, default_value_t = DEFAULT_CAPABILITY_TIMEOUT_MS)]
    timeout_ms: u64,
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
    /// Cancel direct capability execution if the API does not finish in this many milliseconds.
    #[arg(long, default_value_t = DEFAULT_CAPABILITY_TIMEOUT_MS)]
    timeout_ms: u64,
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
    /// Cancel direct capability execution if the API does not finish in this many milliseconds.
    #[arg(long, default_value_t = DEFAULT_CAPABILITY_TIMEOUT_MS)]
    timeout_ms: u64,
}

#[derive(Debug, Parser)]
struct RegistryInspectImageArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    image_ref: String,
    /// Optional registry base URL. Must not include credentials.
    #[arg(long)]
    registry_base_url: Option<String>,
    /// Cancel direct capability execution if the API does not finish in this many milliseconds.
    #[arg(long, default_value_t = DEFAULT_CAPABILITY_TIMEOUT_MS)]
    timeout_ms: u64,
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
    #[arg(long)]
    namespace: Option<String>,
    #[arg(long)]
    repo: Option<String>,
    #[arg(long)]
    branch: Option<String>,
    #[arg(long)]
    production_impacting: Option<bool>,
    /// Include approvals requested at or after this Unix epoch millisecond.
    #[arg(long)]
    requested_after_ms: Option<i64>,
    /// Include approvals requested at or before this Unix epoch millisecond.
    #[arg(long)]
    requested_before_ms: Option<i64>,
    #[arg(long, default_value_t = 50)]
    limit: u32,
    #[arg(long, default_value_t = 0)]
    offset: u32,
}

#[derive(Debug, Parser)]
struct ApprovalSummaryArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long, default_value = "pending")]
    status: String,
    #[arg(long)]
    namespace: Option<String>,
    #[arg(long)]
    repo: Option<String>,
    #[arg(long)]
    branch: Option<String>,
    #[arg(long)]
    production_impacting: Option<bool>,
    /// Include approvals requested at or after this Unix epoch millisecond.
    #[arg(long)]
    requested_after_ms: Option<i64>,
    /// Include approvals requested at or before this Unix epoch millisecond.
    #[arg(long)]
    requested_before_ms: Option<i64>,
}

#[derive(Debug, Parser)]
struct ApprovalGetArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    approval_id: String,
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
    run_id: Option<String>,
    #[arg(long)]
    approval_id: Option<String>,
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
struct ObservationListArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    run_id: Option<String>,
    #[arg(long)]
    source: Option<String>,
    #[arg(long)]
    kind: Option<String>,
    #[arg(long)]
    subject: Option<String>,
    #[arg(long)]
    resource_namespace: Option<String>,
    #[arg(long)]
    resource_kind: Option<String>,
    #[arg(long)]
    resource_name: Option<String>,
    /// Include observations recorded at or after this Unix epoch millisecond.
    #[arg(long)]
    observed_after_ms: Option<i64>,
    /// Include observations recorded at or before this Unix epoch millisecond.
    #[arg(long)]
    observed_before_ms: Option<i64>,
    #[arg(long, default_value_t = 50)]
    limit: u32,
    #[arg(long, default_value_t = 0)]
    offset: u32,
}

#[derive(Debug, Parser)]
struct ObservationGetArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    observation_id: String,
}

#[derive(Debug, Parser)]
struct ObservationCreateArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    id: Option<String>,
    #[arg(long)]
    session_id: Option<String>,
    #[arg(long)]
    run_id: Option<String>,
    #[arg(long)]
    source: String,
    #[arg(long)]
    kind: String,
    #[arg(long)]
    subject: String,
    #[arg(long)]
    summary: String,
    #[arg(long)]
    resource_namespace: Option<String>,
    #[arg(long)]
    resource_kind: Option<String>,
    #[arg(long)]
    resource_name: Option<String>,
    #[arg(long)]
    resource_ref_json: Option<String>,
    #[arg(long)]
    artifact_id: Option<String>,
    #[arg(long)]
    data_json: Option<String>,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct IncidentListArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    run_id: Option<String>,
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    severity: Option<String>,
    #[arg(long)]
    resource_namespace: Option<String>,
    #[arg(long)]
    resource_kind: Option<String>,
    #[arg(long)]
    resource_name: Option<String>,
    /// Include incidents created at or after this Unix epoch millisecond.
    #[arg(long)]
    created_after_ms: Option<i64>,
    /// Include incidents created at or before this Unix epoch millisecond.
    #[arg(long)]
    created_before_ms: Option<i64>,
    #[arg(long, default_value_t = 50)]
    limit: u32,
    #[arg(long, default_value_t = 0)]
    offset: u32,
}

#[derive(Debug, Parser)]
struct IncidentGetArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    incident_id: String,
}

#[derive(Debug, Parser)]
struct IncidentCreateArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    id: Option<String>,
    #[arg(long)]
    observation_id: String,
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    severity: String,
    #[arg(long)]
    title: String,
    #[arg(long)]
    summary: String,
    #[arg(long)]
    resource_namespace: Option<String>,
    #[arg(long)]
    resource_kind: Option<String>,
    #[arg(long)]
    resource_name: Option<String>,
    #[arg(long)]
    data_json: Option<String>,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct RemediationPlanListArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    incident_id: Option<String>,
    #[arg(long)]
    run_id: Option<String>,
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    risk_level: Option<String>,
    #[arg(long)]
    resource_namespace: Option<String>,
    #[arg(long)]
    resource_kind: Option<String>,
    #[arg(long)]
    resource_name: Option<String>,
    /// Include remediation plans created at or after this Unix epoch millisecond.
    #[arg(long)]
    created_after_ms: Option<i64>,
    /// Include remediation plans created at or before this Unix epoch millisecond.
    #[arg(long)]
    created_before_ms: Option<i64>,
    #[arg(long, default_value_t = 50)]
    limit: u32,
    #[arg(long, default_value_t = 0)]
    offset: u32,
}

#[derive(Debug, Parser)]
struct RemediationPlanGetArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    plan_id: String,
}

#[derive(Debug, Parser)]
struct RemediationPlanCreateArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    id: Option<String>,
    #[arg(long)]
    incident_id: String,
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    title: String,
    #[arg(long)]
    summary: String,
    #[arg(long)]
    risk_level: String,
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    requires_approval: bool,
    #[arg(long)]
    resource_namespace: Option<String>,
    #[arg(long)]
    resource_kind: Option<String>,
    #[arg(long)]
    resource_name: Option<String>,
    #[arg(long)]
    plan_json: Option<String>,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct WorkPlanListArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    remediation_plan_id: Option<String>,
    #[arg(long)]
    incident_id: Option<String>,
    #[arg(long)]
    run_id: Option<String>,
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    risk_level: Option<String>,
    #[arg(long)]
    resource_namespace: Option<String>,
    #[arg(long)]
    resource_kind: Option<String>,
    #[arg(long)]
    resource_name: Option<String>,
    /// Include work plans created at or after this Unix epoch millisecond.
    #[arg(long)]
    created_after_ms: Option<i64>,
    /// Include work plans created at or before this Unix epoch millisecond.
    #[arg(long)]
    created_before_ms: Option<i64>,
    #[arg(long, default_value_t = 50)]
    limit: u32,
    #[arg(long, default_value_t = 0)]
    offset: u32,
}

#[derive(Debug, Parser)]
struct WorkPlanGetArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    work_plan_id: String,
}

#[derive(Debug, Parser)]
struct WorkPlanReadinessArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    work_plan_id: String,
}

#[derive(Debug, Parser)]
struct WorkPlanFlowArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    work_plan_id: String,
}

#[derive(Debug, Parser)]
struct WorkPlanCreateFromRemediationPlanArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    remediation_plan_id: String,
}

#[derive(Debug, Parser)]
struct WorkPlanReviseArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    work_plan_id: String,
    #[arg(long)]
    work_plan_json: String,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    summary: Option<String>,
    #[arg(long)]
    risk_level: Option<String>,
    #[arg(long)]
    requires_approval: Option<bool>,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    material_change: bool,
}

#[derive(Debug, Parser)]
struct WorkPlanTransitionArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    work_plan_id: String,
    #[arg(long)]
    target_status: String,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct WorkPlanCreateTrustedEnvelopeArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    work_plan_id: String,
    #[arg(long)]
    subject: Option<String>,
    #[arg(long)]
    created_by: Option<String>,
    #[arg(long)]
    reason: String,
    #[arg(long, default_value = "local")]
    environment: String,
    #[arg(long)]
    namespace: Option<String>,
    #[arg(long)]
    repo: Option<String>,
    #[arg(long)]
    branch: Option<String>,
    #[arg(long)]
    production_impacting: bool,
    #[arg(long)]
    expires_at: Option<String>,
}

#[derive(Debug, Parser)]
struct ChangeSetListArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    work_plan_id: Option<String>,
    #[arg(long)]
    remediation_plan_id: Option<String>,
    #[arg(long)]
    incident_id: Option<String>,
    #[arg(long)]
    run_id: Option<String>,
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    risk_level: Option<String>,
    #[arg(long)]
    resource_namespace: Option<String>,
    #[arg(long)]
    resource_kind: Option<String>,
    #[arg(long)]
    resource_name: Option<String>,
    /// Include change sets created at or after this Unix epoch millisecond.
    #[arg(long)]
    created_after_ms: Option<i64>,
    /// Include change sets created at or before this Unix epoch millisecond.
    #[arg(long)]
    created_before_ms: Option<i64>,
    #[arg(long, default_value_t = 50)]
    limit: u32,
    #[arg(long, default_value_t = 0)]
    offset: u32,
}

#[derive(Debug, Parser)]
struct ChangeSetGetArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    change_set_id: String,
}

#[derive(Debug, Parser)]
struct ChangeSetReadinessArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    change_set_id: String,
}

#[derive(Debug, Parser)]
struct ChangeSetFlowArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    change_set_id: String,
}

#[derive(Debug, Parser)]
struct ChangeSetCreateArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    work_plan_id: String,
    #[arg(long)]
    change_set_json: String,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    summary: Option<String>,
    #[arg(long)]
    risk_level: Option<String>,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct ChangeSetReviseArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    change_set_id: String,
    #[arg(long)]
    change_set_json: String,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    summary: Option<String>,
    #[arg(long)]
    risk_level: Option<String>,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    material_change: bool,
}

#[derive(Debug, Parser)]
struct ChangeSetTransitionArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    change_set_id: String,
    #[arg(long)]
    target_status: String,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct ChangeSetCreateTrustedEnvelopeArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    change_set_id: String,
    #[arg(long)]
    subject: Option<String>,
    #[arg(long)]
    created_by: Option<String>,
    #[arg(long)]
    reason: String,
    #[arg(long, default_value = "local")]
    environment: String,
    #[arg(long)]
    namespace: Option<String>,
    #[arg(long)]
    repo: Option<String>,
    #[arg(long)]
    branch: Option<String>,
    #[arg(long)]
    production_impacting: bool,
    #[arg(long)]
    expires_at: Option<String>,
}

#[derive(Debug, Parser)]
struct PipelineIntentListArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    change_set_id: Option<String>,
    #[arg(long)]
    work_plan_id: Option<String>,
    #[arg(long)]
    remediation_plan_id: Option<String>,
    #[arg(long)]
    incident_id: Option<String>,
    #[arg(long)]
    run_id: Option<String>,
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    intent_kind: Option<String>,
    #[arg(long)]
    risk_level: Option<String>,
    #[arg(long)]
    resource_namespace: Option<String>,
    #[arg(long)]
    resource_kind: Option<String>,
    #[arg(long)]
    resource_name: Option<String>,
    /// Include pipeline intents created at or after this Unix epoch millisecond.
    #[arg(long)]
    created_after_ms: Option<i64>,
    /// Include pipeline intents created at or before this Unix epoch millisecond.
    #[arg(long)]
    created_before_ms: Option<i64>,
    #[arg(long, default_value_t = 50)]
    limit: u32,
    #[arg(long, default_value_t = 0)]
    offset: u32,
}

#[derive(Debug, Parser)]
struct PipelineIntentGetArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    pipeline_intent_id: String,
}

#[derive(Debug, Parser)]
struct PipelineIntentCreateFromChangeSetArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    change_set_id: String,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    summary: Option<String>,
    #[arg(long)]
    risk_level: Option<String>,
    #[arg(long)]
    intent_kind: Option<String>,
    #[arg(long)]
    intent_json: Option<String>,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct PipelineIntentTransitionArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    pipeline_intent_id: String,
    #[arg(long)]
    target_status: String,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct PipelineIntentAttachEvidenceArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    pipeline_intent_id: String,
    #[arg(long)]
    observation_id: String,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct PipelineIntentCreateTrustedEnvelopeArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    pipeline_intent_id: String,
    #[arg(long)]
    subject: Option<String>,
    #[arg(long)]
    created_by: Option<String>,
    #[arg(long)]
    reason: String,
    #[arg(long)]
    expires_at: Option<String>,
}

#[derive(Debug, Parser)]
struct PipelineIntentExecuteArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    pipeline_intent_id: String,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
    /// Dispatch the executor Job. Without this flag Pharness only returns a preflight preview.
    #[arg(long)]
    apply: bool,
}

#[derive(Debug, Parser)]
struct PipelineContractListArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    namespace: Option<String>,
    #[arg(long)]
    pipeline_ref: Option<String>,
    #[arg(long)]
    status: Option<String>,
    #[arg(long, default_value_t = 50)]
    limit: u32,
    #[arg(long, default_value_t = 0)]
    offset: u32,
}

#[derive(Debug, Parser)]
struct PipelineContractGetArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    pipeline_contract_id: String,
}

#[derive(Debug, Parser)]
struct PipelineContractCreateArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    namespace: String,
    #[arg(long)]
    pipeline_ref: String,
    #[arg(long, default_value = "v1")]
    version: String,
    #[arg(long)]
    contract_json: String,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct PipelineContractRetireArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    pipeline_contract_id: String,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct PipelineContractReplaceArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    pipeline_contract_id: String,
    #[arg(long)]
    version: String,
    #[arg(long)]
    contract_json: String,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct DeploymentContractListArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    target_environment: Option<String>,
    #[arg(long)]
    target_namespace: Option<String>,
    #[arg(long)]
    argo_application: Option<String>,
    #[arg(long)]
    status: Option<String>,
    #[arg(long, default_value_t = 50)]
    limit: u32,
    #[arg(long, default_value_t = 0)]
    offset: u32,
}

#[derive(Debug, Parser)]
struct DeploymentContractGetArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    deployment_contract_id: String,
}

#[derive(Debug, Parser)]
struct DeploymentContractCreateArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    target_environment: String,
    #[arg(long)]
    target_namespace: String,
    #[arg(long)]
    argo_application: String,
    #[arg(long, default_value = "v1")]
    version: String,
    #[arg(long)]
    contract_json: String,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct DeploymentContractRetireArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    deployment_contract_id: String,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct DeploymentIntentListArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    pipeline_intent_id: Option<String>,
    #[arg(long)]
    change_set_id: Option<String>,
    #[arg(long)]
    work_plan_id: Option<String>,
    #[arg(long)]
    remediation_plan_id: Option<String>,
    #[arg(long)]
    incident_id: Option<String>,
    #[arg(long)]
    run_id: Option<String>,
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    intent_kind: Option<String>,
    #[arg(long)]
    risk_level: Option<String>,
    #[arg(long)]
    target_environment: Option<String>,
    #[arg(long)]
    target_namespace: Option<String>,
    #[arg(long)]
    argo_application: Option<String>,
    #[arg(long)]
    resource_namespace: Option<String>,
    #[arg(long)]
    resource_kind: Option<String>,
    #[arg(long)]
    resource_name: Option<String>,
    /// Include deployment intents created at or after this Unix epoch millisecond.
    #[arg(long)]
    created_after_ms: Option<i64>,
    /// Include deployment intents created at or before this Unix epoch millisecond.
    #[arg(long)]
    created_before_ms: Option<i64>,
    #[arg(long, default_value_t = 50)]
    limit: u32,
    #[arg(long, default_value_t = 0)]
    offset: u32,
}

#[derive(Debug, Parser)]
struct DeploymentIntentGetArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    deployment_intent_id: String,
}

#[derive(Debug, Parser)]
struct DeploymentIntentCreateFromPipelineIntentArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    pipeline_intent_id: String,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    summary: Option<String>,
    #[arg(long)]
    risk_level: Option<String>,
    #[arg(long)]
    intent_kind: Option<String>,
    #[arg(long)]
    target_environment: Option<String>,
    #[arg(long)]
    target_namespace: Option<String>,
    #[arg(long)]
    argo_application: Option<String>,
    #[arg(long)]
    intent_json: Option<String>,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct DeploymentIntentTransitionArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    deployment_intent_id: String,
    #[arg(long)]
    target_status: String,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct DeploymentIntentAttachEvidenceArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    deployment_intent_id: String,
    #[arg(long)]
    observation_id: String,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct ReleaseListArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    deployment_intent_id: Option<String>,
    #[arg(long)]
    pipeline_intent_id: Option<String>,
    #[arg(long)]
    change_set_id: Option<String>,
    #[arg(long)]
    work_plan_id: Option<String>,
    #[arg(long)]
    remediation_plan_id: Option<String>,
    #[arg(long)]
    incident_id: Option<String>,
    #[arg(long)]
    run_id: Option<String>,
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    release_kind: Option<String>,
    #[arg(long)]
    risk_level: Option<String>,
    #[arg(long)]
    target_environment: Option<String>,
    #[arg(long)]
    target_namespace: Option<String>,
    #[arg(long)]
    argo_application: Option<String>,
    #[arg(long)]
    version: Option<String>,
    #[arg(long)]
    commit_sha: Option<String>,
    #[arg(long)]
    image_digest: Option<String>,
    /// Include releases created at or after this Unix epoch millisecond.
    #[arg(long)]
    created_after_ms: Option<i64>,
    /// Include releases created at or before this Unix epoch millisecond.
    #[arg(long)]
    created_before_ms: Option<i64>,
    #[arg(long, default_value_t = 50)]
    limit: u32,
    #[arg(long, default_value_t = 0)]
    offset: u32,
}

#[derive(Debug, Parser)]
struct ReleaseGetArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    release_id: String,
}

#[derive(Debug, Parser)]
struct ReleaseCreateFromDeploymentIntentArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    deployment_intent_id: String,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    summary: Option<String>,
    #[arg(long)]
    risk_level: Option<String>,
    #[arg(long)]
    release_kind: Option<String>,
    #[arg(long)]
    version: Option<String>,
    #[arg(long)]
    commit_sha: Option<String>,
    #[arg(long)]
    image_digest: Option<String>,
    #[arg(long)]
    rollback_ref: Option<String>,
    #[arg(long)]
    release_json: Option<String>,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct ReleaseTransitionArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    release_id: String,
    #[arg(long)]
    target_status: String,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct ReleaseAttachEvidenceArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    release_id: String,
    #[arg(long)]
    observation_id: String,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct RegistryEvidenceListArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    release_id: Option<String>,
    #[arg(long)]
    deployment_intent_id: Option<String>,
    #[arg(long)]
    pipeline_intent_id: Option<String>,
    #[arg(long)]
    change_set_id: Option<String>,
    #[arg(long)]
    work_plan_id: Option<String>,
    #[arg(long)]
    remediation_plan_id: Option<String>,
    #[arg(long)]
    incident_id: Option<String>,
    #[arg(long)]
    run_id: Option<String>,
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    risk_level: Option<String>,
    #[arg(long)]
    registry: Option<String>,
    #[arg(long)]
    repository: Option<String>,
    #[arg(long)]
    image_ref: Option<String>,
    #[arg(long)]
    image_digest: Option<String>,
    #[arg(long)]
    tag: Option<String>,
    #[arg(long)]
    source: Option<String>,
    #[arg(long)]
    verification_status: Option<String>,
    /// Include evidence created at or after this Unix epoch millisecond.
    #[arg(long)]
    created_after_ms: Option<i64>,
    /// Include evidence created at or before this Unix epoch millisecond.
    #[arg(long)]
    created_before_ms: Option<i64>,
    #[arg(long, default_value_t = 50)]
    limit: u32,
    #[arg(long, default_value_t = 0)]
    offset: u32,
}

#[derive(Debug, Parser)]
struct RegistryEvidenceGetArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    evidence_id: String,
}

#[derive(Debug, Parser)]
struct RegistryEvidenceCreateFromReleaseArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    release_id: String,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    summary: Option<String>,
    #[arg(long)]
    risk_level: Option<String>,
    #[arg(long)]
    registry: Option<String>,
    #[arg(long)]
    repository: Option<String>,
    #[arg(long)]
    image_ref: Option<String>,
    #[arg(long)]
    image_digest: Option<String>,
    #[arg(long)]
    tag: Option<String>,
    #[arg(long)]
    source: Option<String>,
    #[arg(long)]
    verification_status: Option<String>,
    #[arg(long)]
    evidence_json: Option<String>,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct RegistryEvidenceCreateFromInspectionArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    release_id: String,
    #[arg(long)]
    image_ref: String,
    #[arg(long)]
    registry_base_url: Option<String>,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    summary: Option<String>,
    #[arg(long)]
    risk_level: Option<String>,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
    #[arg(long)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Parser)]
struct RegistryEvidenceTransitionArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    evidence_id: String,
    #[arg(long)]
    target_status: String,
    #[arg(long)]
    actor: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct ApprovalGateListArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    remediation_plan_id: Option<String>,
    #[arg(long)]
    incident_id: Option<String>,
    #[arg(long)]
    run_id: Option<String>,
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    gate_kind: Option<String>,
    #[arg(long)]
    risk_level: Option<String>,
    #[arg(long)]
    resource_namespace: Option<String>,
    #[arg(long)]
    resource_kind: Option<String>,
    #[arg(long)]
    resource_name: Option<String>,
    /// Include approval gates created at or after this Unix epoch millisecond.
    #[arg(long)]
    created_after_ms: Option<i64>,
    /// Include approval gates created at or before this Unix epoch millisecond.
    #[arg(long)]
    created_before_ms: Option<i64>,
    #[arg(long, default_value_t = 50)]
    limit: u32,
    #[arg(long, default_value_t = 0)]
    offset: u32,
}

#[derive(Debug, Parser)]
struct ApprovalGateSummaryArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    remediation_plan_id: Option<String>,
    #[arg(long)]
    incident_id: Option<String>,
    #[arg(long)]
    run_id: Option<String>,
    #[arg(long, default_value = "pending")]
    status: String,
    #[arg(long)]
    gate_kind: Option<String>,
    #[arg(long)]
    risk_level: Option<String>,
    #[arg(long)]
    resource_namespace: Option<String>,
    #[arg(long)]
    resource_kind: Option<String>,
    #[arg(long)]
    resource_name: Option<String>,
    /// Include approval gates created at or after this Unix epoch millisecond.
    #[arg(long)]
    created_after_ms: Option<i64>,
    /// Include approval gates created at or before this Unix epoch millisecond.
    #[arg(long)]
    created_before_ms: Option<i64>,
}

#[derive(Debug, Parser)]
struct ApprovalGateGetArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    gate_id: String,
}

#[derive(Debug, Parser)]
struct ApprovalGateDecisionArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    gate_id: String,
    #[arg(long)]
    decided_by: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct PermissionGrantListArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long, default_value = "active")]
    status: String,
    #[arg(long, default_value_t = 50)]
    limit: u32,
}

#[derive(Debug, Parser)]
struct PermissionGrantCreateArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    subject: String,
    #[arg(long)]
    created_by: Option<String>,
    #[arg(long)]
    reason: String,
    #[arg(long)]
    policy_mode: PolicyMode,
    #[arg(long)]
    scope_json: String,
    #[arg(long)]
    expires_at: Option<String>,
}

#[derive(Debug, Parser)]
struct PermissionGrantGetArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    grant_id: String,
}

#[derive(Debug, Parser)]
struct PermissionGrantRevokeArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    grant_id: String,
    #[arg(long)]
    revoked_by: Option<String>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Debug, Parser)]
struct AuditEventListArgs {
    #[arg(
        long,
        env = "PHARNESS_API_URL",
        default_value = "http://127.0.0.1:4777"
    )]
    api_url: String,
    #[arg(long)]
    resource_kind: Option<String>,
    #[arg(long)]
    resource_id: Option<String>,
    #[arg(long)]
    run_id: Option<String>,
    #[arg(long, default_value_t = 50)]
    limit: u32,
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
            RunCommand::List(args) => list_runs(args).await?,
            RunCommand::Summary(args) => summarize_runs(args).await?,
            RunCommand::Cancel(args) => cancel_run(args).await?,
            RunCommand::Get(args) => get_run(args).await?,
            RunCommand::Events(args) => get_run_events_command(args).await?,
            RunCommand::Diff(args) => get_run_diff(args).await?,
        },
        Command::Config(args) => config(args).await?,
        Command::Capabilities { command } => match command {
            CapabilityCommand::KubernetesGet(args) => kubernetes_get(args).await?,
            CapabilityCommand::ArgoGetApp(args) => argo_get_app(args).await?,
            CapabilityCommand::PrometheusQuery(args) => prometheus_query(args).await?,
            CapabilityCommand::PrometheusInventory(args) => prometheus_inventory(args).await?,
            CapabilityCommand::LokiLogSummary(args) => loki_log_summary(args).await?,
            CapabilityCommand::TektonGetPipelineRuns(args) => {
                tekton_get_pipeline_runs(args).await?
            }
            CapabilityCommand::TektonGetTaskRuns(args) => tekton_get_task_runs(args).await?,
            CapabilityCommand::TektonAnalyzePipelineRun(args) => {
                tekton_analyze_pipeline_run(args).await?
            }
            CapabilityCommand::RegistryInspectImage(args) => registry_inspect_image(args).await?,
        },
        Command::Approvals { command } => match command {
            ApprovalCommand::List(args) => list_approvals(args).await?,
            ApprovalCommand::Summary(args) => summarize_approvals(args).await?,
            ApprovalCommand::Get(args) => get_approval(args).await?,
            ApprovalCommand::Approve(args) => decide_approval(args, "approve").await?,
            ApprovalCommand::Deny(args) => decide_approval(args, "deny").await?,
        },
        Command::Artifacts { command } => match command {
            ArtifactCommand::List(args) => list_artifacts(args).await?,
            ArtifactCommand::Get(args) => get_artifact(args).await?,
        },
        Command::Observations { command } => match command {
            ObservationCommand::List(args) => list_observations(args).await?,
            ObservationCommand::Get(args) => get_observation(args).await?,
            ObservationCommand::Create(args) => create_observation(args).await?,
        },
        Command::Incidents { command } => match command {
            IncidentCommand::List(args) => list_incidents(args).await?,
            IncidentCommand::Get(args) => get_incident(args).await?,
            IncidentCommand::Create(args) => create_incident(args).await?,
        },
        Command::RemediationPlans { command } => match command {
            RemediationPlanCommand::List(args) => list_remediation_plans(args).await?,
            RemediationPlanCommand::Get(args) => get_remediation_plan(args).await?,
            RemediationPlanCommand::Create(args) => create_remediation_plan(args).await?,
        },
        Command::WorkPlans { command } => match command {
            WorkPlanCommand::List(args) => list_work_plans(*args).await?,
            WorkPlanCommand::Get(args) => get_work_plan(args).await?,
            WorkPlanCommand::Readiness(args) => get_work_plan_readiness(args).await?,
            WorkPlanCommand::Flow(args) => get_work_plan_flow(args).await?,
            WorkPlanCommand::CreateFromRemediationPlan(args) => {
                create_work_plan_from_remediation_plan(args).await?
            }
            WorkPlanCommand::Revise(args) => revise_work_plan(args).await?,
            WorkPlanCommand::Transition(args) => transition_work_plan(args).await?,
            WorkPlanCommand::CreateTrustedEnvelope(args) => {
                create_work_plan_trusted_envelope(args).await?
            }
        },
        Command::ChangeSets { command } => match command {
            ChangeSetCommand::List(args) => list_change_sets(*args).await?,
            ChangeSetCommand::Get(args) => get_change_set(args).await?,
            ChangeSetCommand::Readiness(args) => get_change_set_readiness(args).await?,
            ChangeSetCommand::Flow(args) => get_change_set_flow(args).await?,
            ChangeSetCommand::Create(args) => create_change_set(args).await?,
            ChangeSetCommand::Revise(args) => revise_change_set(args).await?,
            ChangeSetCommand::Transition(args) => transition_change_set(args).await?,
            ChangeSetCommand::CreateTrustedEnvelope(args) => {
                create_change_set_trusted_envelope(args).await?
            }
        },
        Command::PipelineIntents { command } => match command {
            PipelineIntentCommand::List(args) => list_pipeline_intents(*args).await?,
            PipelineIntentCommand::Get(args) => get_pipeline_intent(args).await?,
            PipelineIntentCommand::CreateFromChangeSet(args) => {
                create_pipeline_intent_from_change_set(args).await?
            }
            PipelineIntentCommand::Transition(args) => transition_pipeline_intent(args).await?,
            PipelineIntentCommand::AttachEvidence(args) => {
                attach_pipeline_intent_evidence(args).await?
            }
            PipelineIntentCommand::CreateTrustedEnvelope(args) => {
                create_pipeline_intent_trusted_envelope(args).await?
            }
            PipelineIntentCommand::Execute(args) => execute_pipeline_intent(args).await?,
        },
        Command::PipelineContracts { command } => match command {
            PipelineContractCommand::List(args) => list_pipeline_contracts(*args).await?,
            PipelineContractCommand::Get(args) => get_pipeline_contract(args).await?,
            PipelineContractCommand::Create(args) => create_pipeline_contract(args).await?,
            PipelineContractCommand::Replace(args) => replace_pipeline_contract(args).await?,
            PipelineContractCommand::Retire(args) => retire_pipeline_contract(args).await?,
        },
        Command::DeploymentContracts { command } => match command {
            DeploymentContractCommand::List(args) => list_deployment_contracts(*args).await?,
            DeploymentContractCommand::Get(args) => get_deployment_contract(args).await?,
            DeploymentContractCommand::Create(args) => create_deployment_contract(args).await?,
            DeploymentContractCommand::Retire(args) => retire_deployment_contract(args).await?,
        },
        Command::DeploymentIntents { command } => match command {
            DeploymentIntentCommand::List(args) => list_deployment_intents(*args).await?,
            DeploymentIntentCommand::Get(args) => get_deployment_intent(args).await?,
            DeploymentIntentCommand::CreateFromPipelineIntent(args) => {
                create_deployment_intent_from_pipeline_intent(args).await?
            }
            DeploymentIntentCommand::Transition(args) => transition_deployment_intent(args).await?,
            DeploymentIntentCommand::AttachEvidence(args) => {
                attach_deployment_intent_evidence(args).await?
            }
        },
        Command::Releases { command } => match command {
            ReleaseCommand::List(args) => list_releases(*args).await?,
            ReleaseCommand::Get(args) => get_release(args).await?,
            ReleaseCommand::CreateFromDeploymentIntent(args) => {
                create_release_from_deployment_intent(args).await?
            }
            ReleaseCommand::Transition(args) => transition_release(args).await?,
            ReleaseCommand::AttachEvidence(args) => attach_release_evidence(args).await?,
        },
        Command::RegistryEvidence { command } => match command {
            RegistryEvidenceCommand::List(args) => list_registry_evidence(*args).await?,
            RegistryEvidenceCommand::Get(args) => get_registry_evidence(args).await?,
            RegistryEvidenceCommand::CreateFromRelease(args) => {
                create_registry_evidence_from_release(*args).await?
            }
            RegistryEvidenceCommand::CreateFromInspection(args) => {
                create_registry_evidence_from_inspection(*args).await?
            }
            RegistryEvidenceCommand::Transition(args) => transition_registry_evidence(args).await?,
        },
        Command::ApprovalGates { command } => match command {
            ApprovalGateCommand::List(args) => list_approval_gates(*args).await?,
            ApprovalGateCommand::Summary(args) => summarize_approval_gates(*args).await?,
            ApprovalGateCommand::Get(args) => get_approval_gate(args).await?,
            ApprovalGateCommand::Satisfy(args) => decide_approval_gate(args, "satisfy").await?,
            ApprovalGateCommand::Waive(args) => decide_approval_gate(args, "waive").await?,
            ApprovalGateCommand::Reject(args) => decide_approval_gate(args, "reject").await?,
        },
        Command::PermissionGrants { command } => match command {
            PermissionGrantCommand::List(args) => list_permission_grants(args).await?,
            PermissionGrantCommand::Create(args) => create_permission_grant(args).await?,
            PermissionGrantCommand::Get(args) => get_permission_grant(args).await?,
            PermissionGrantCommand::Revoke(args) => revoke_permission_grant(args).await?,
        },
        Command::AuditEvents(args) => list_audit_events(args).await?,
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
        args.timeout_ms,
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
        args.timeout_ms,
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
        args.timeout_ms,
    )
    .await
}

async fn prometheus_inventory(args: PrometheusInventoryArgs) -> anyhow::Result<()> {
    execute_capability(
        &args.api_url,
        serde_json::json!({
            "action": "prometheus_inventory",
            "id": "cli.prometheus_inventory",
            "reason": "direct CLI capability execution",
        }),
        args.timeout_ms,
    )
    .await
}

async fn loki_log_summary(args: LokiLogSummaryArgs) -> anyhow::Result<()> {
    execute_capability(
        &args.api_url,
        serde_json::json!({
            "action": "loki_log_summary",
            "id": "cli.loki_log_summary",
            "reason": "direct CLI capability execution",
            "query": args.query,
            "since_seconds": args.since_seconds,
            "limit": args.limit,
        }),
        args.timeout_ms,
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
        args.timeout_ms,
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
        args.timeout_ms,
    )
    .await
}

async fn registry_inspect_image(args: RegistryInspectImageArgs) -> anyhow::Result<()> {
    execute_capability(
        &args.api_url,
        serde_json::json!({
            "action": "registry_inspect_image",
            "id": "cli.registry_inspect_image",
            "reason": "direct CLI capability execution",
            "image_ref": args.image_ref,
            "registry_base_url": args.registry_base_url,
        }),
        args.timeout_ms,
    )
    .await
}

async fn execute_capability(
    api_url_base: &str,
    action: serde_json::Value,
    timeout_ms: u64,
) -> anyhow::Result<()> {
    let http = api_client_with_timeout(Duration::from_millis(timeout_ms.saturating_add(5_000)));
    let response = http
        .post(api_url(api_url_base, "/api/capabilities/execute"))
        .json(&serde_json::json!({
            "action": action,
            "timeout_ms": timeout_ms,
        }))
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

async fn config(args: ConfigArgs) -> anyhow::Result<()> {
    if let Some(command) = args.command {
        match command {
            ConfigCommand::Validate(args) => validate_config(args)?,
        }
        return Ok(());
    }

    let http = api_client();
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

fn validate_config(args: ConfigValidateArgs) -> anyhow::Result<()> {
    let config = ApiRuntimeConfig::load_path_with_env(&args.file)
        .with_context(|| format!("failed to validate {}", args.file.display()))?;
    let output = ConfigValidationOutput::from_config(args.file, &config);

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

async fn fireworks_models(args: FireworksModelsArgs) -> anyhow::Result<()> {
    let http = api_client();
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
    let http = api_client();
    let mut query = vec![
        ("status".to_string(), args.status),
        ("limit".to_string(), args.limit.to_string()),
        ("offset".to_string(), args.offset.to_string()),
    ];
    if let Some(namespace) = args.namespace {
        query.push(("namespace".to_string(), namespace));
    }
    if let Some(repo) = args.repo {
        query.push(("repo".to_string(), repo));
    }
    if let Some(branch) = args.branch {
        query.push(("branch".to_string(), branch));
    }
    if let Some(production_impacting) = args.production_impacting {
        query.push((
            "production_impacting".to_string(),
            production_impacting.to_string(),
        ));
    }
    if let Some(requested_after_ms) = args.requested_after_ms {
        query.push((
            "requested_after_ms".to_string(),
            requested_after_ms.to_string(),
        ));
    }
    if let Some(requested_before_ms) = args.requested_before_ms {
        query.push((
            "requested_before_ms".to_string(),
            requested_before_ms.to_string(),
        ));
    }
    let response = http
        .get(api_url(&args.api_url, "/api/approvals"))
        .query(&query)
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

async fn summarize_approvals(args: ApprovalSummaryArgs) -> anyhow::Result<()> {
    let http = api_client();
    let mut query = vec![("status".to_string(), args.status)];
    if let Some(namespace) = args.namespace {
        query.push(("namespace".to_string(), namespace));
    }
    if let Some(repo) = args.repo {
        query.push(("repo".to_string(), repo));
    }
    if let Some(branch) = args.branch {
        query.push(("branch".to_string(), branch));
    }
    if let Some(production_impacting) = args.production_impacting {
        query.push((
            "production_impacting".to_string(),
            production_impacting.to_string(),
        ));
    }
    if let Some(requested_after_ms) = args.requested_after_ms {
        query.push((
            "requested_after_ms".to_string(),
            requested_after_ms.to_string(),
        ));
    }
    if let Some(requested_before_ms) = args.requested_before_ms {
        query.push((
            "requested_before_ms".to_string(),
            requested_before_ms.to_string(),
        ));
    }
    let response = http
        .get(api_url(&args.api_url, "/api/approvals/summary"))
        .query(&query)
        .send()
        .await
        .context("failed to fetch approval summary")?
        .error_for_status()
        .context("pharness API rejected approval summary")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode approval summary")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn get_approval(args: ApprovalGetArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .get(api_url(
            &args.api_url,
            &format!("/api/approvals/{}", args.approval_id),
        ))
        .send()
        .await
        .context("failed to fetch approval")?
        .error_for_status()
        .context("pharness API rejected approval fetch")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode approval")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn decide_approval(args: ApprovalDecisionArgs, decision: &str) -> anyhow::Result<()> {
    let http = api_client();
    let api_url_base = args.api_url.clone();
    let endpoint = approval_decision_endpoint(&args, decision)?;
    let wait = args.wait;
    let follow_events = args.follow_events;
    let poll_interval_ms = args.poll_interval_ms;
    let timeout_ms = args.timeout_ms;
    let response = http
        .post(api_url(&api_url_base, &endpoint.path))
        .json(&ApprovalReviewRequest {
            decision: endpoint.includes_decision.then_some(decision),
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

    let run_id = response
        .get("run")
        .and_then(|run| run.get("id"))
        .and_then(serde_json::Value::as_str)
        .context("approval decision response did not include run.id")?
        .to_string();
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
    let http = api_client();
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

async fn list_runs(args: RunListArgs) -> anyhow::Result<()> {
    let http = api_client();
    let mut query = vec![
        ("limit".to_string(), args.limit.to_string()),
        ("offset".to_string(), args.offset.to_string()),
    ];
    if let Some(status) = args.status {
        query.push(("status".to_string(), status));
    }
    if let Some(namespace) = args.namespace {
        query.push(("namespace".to_string(), namespace));
    }
    if let Some(repo) = args.repo {
        query.push(("repo".to_string(), repo));
    }
    if let Some(branch) = args.branch {
        query.push(("branch".to_string(), branch));
    }
    if let Some(production_impacting) = args.production_impacting {
        query.push((
            "production_impacting".to_string(),
            production_impacting.to_string(),
        ));
    }
    if let Some(started_after_ms) = args.started_after_ms {
        query.push(("started_after_ms".to_string(), started_after_ms.to_string()));
    }
    if let Some(started_before_ms) = args.started_before_ms {
        query.push((
            "started_before_ms".to_string(),
            started_before_ms.to_string(),
        ));
    }
    let response = http
        .get(api_url(&args.api_url, "/api/runs"))
        .query(&query)
        .send()
        .await
        .context("failed to fetch runs")?
        .error_for_status()
        .context("pharness API rejected run list")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode run list")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn summarize_runs(args: RunSummaryArgs) -> anyhow::Result<()> {
    let http = api_client();
    let mut query = Vec::new();
    if let Some(status) = args.status {
        query.push(("status".to_string(), status));
    }
    if let Some(namespace) = args.namespace {
        query.push(("namespace".to_string(), namespace));
    }
    if let Some(repo) = args.repo {
        query.push(("repo".to_string(), repo));
    }
    if let Some(branch) = args.branch {
        query.push(("branch".to_string(), branch));
    }
    if let Some(production_impacting) = args.production_impacting {
        query.push((
            "production_impacting".to_string(),
            production_impacting.to_string(),
        ));
    }
    if let Some(started_after_ms) = args.started_after_ms {
        query.push(("started_after_ms".to_string(), started_after_ms.to_string()));
    }
    if let Some(started_before_ms) = args.started_before_ms {
        query.push((
            "started_before_ms".to_string(),
            started_before_ms.to_string(),
        ));
    }
    let response = http
        .get(api_url(&args.api_url, "/api/runs/summary"))
        .query(&query)
        .send()
        .await
        .context("failed to fetch run summary")?
        .error_for_status()
        .context("pharness API rejected run summary")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode run summary")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn cancel_run(args: RunCancelArgs) -> anyhow::Result<()> {
    let http = api_client();
    let run = http
        .post(api_url(
            &args.api_url,
            &format!("/api/runs/{}/cancel", args.run_id),
        ))
        .send()
        .await
        .context("failed to cancel run")?
        .error_for_status()
        .context("pharness API rejected run cancellation")?
        .json::<RunResponse>()
        .await
        .context("failed to decode cancelled run")?;

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
        println!("{}", serde_json::to_string_pretty(&run)?);
    }

    Ok(())
}

async fn get_run_events_command(args: RunEventsArgs) -> anyhow::Result<()> {
    if args.stream {
        stream_run_events(args).await
    } else {
        let http = api_client();
        let mut events = fetch_events(&args.api_url, &http, &args.run_id).await?;
        if let Some(after_seq) = args.after_seq {
            events.retain(|event| event_seq(event).is_some_and(|seq| seq > after_seq));
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&EventsResponse { events })?
        );
        Ok(())
    }
}

async fn stream_run_events(args: RunEventsArgs) -> anyhow::Result<()> {
    let http = api_client();
    let mut request = http.get(api_url(
        &args.api_url,
        &format!("/api/runs/{}/events/stream", args.run_id),
    ));
    if let Some(after_seq) = args.after_seq {
        request = request.query(&[("after_seq", after_seq)]);
    }

    let response = request
        .send()
        .await
        .context("failed to stream run events")?
        .error_for_status()
        .context("pharness API rejected run event stream")?;
    let mut chunks = response.bytes_stream();
    let deadline = tokio::time::Instant::now() + Duration::from_millis(args.timeout_ms);
    let mut buffer = String::new();

    loop {
        let remaining = deadline
            .checked_duration_since(tokio::time::Instant::now())
            .unwrap_or_default();
        let next = tokio::time::timeout(remaining, chunks.next())
            .await
            .with_context(|| {
                format!(
                    "timed out waiting for streamed run events after {} ms",
                    args.timeout_ms
                )
            })?;

        match next {
            Some(Ok(chunk)) => {
                let text = std::str::from_utf8(&chunk)
                    .context("run event stream returned non-UTF-8 data")?;
                buffer.push_str(&text.replace("\r\n", "\n"));
                emit_sse_event_frames(&mut buffer)?;
            }
            Some(Err(error)) => return Err(error).context("run event stream failed"),
            None => break,
        }
    }

    Ok(())
}

async fn get_run_diff(args: RunDiffArgs) -> anyhow::Result<()> {
    let http = api_client();
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
    let http = api_client();
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
    let http = api_client();
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

async fn list_permission_grants(args: PermissionGrantListArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .get(api_url(&args.api_url, "/api/permission-grants"))
        .query(&[
            ("status", args.status.as_str()),
            ("limit", &args.limit.to_string()),
        ])
        .send()
        .await
        .context("failed to fetch permission grants")?
        .error_for_status()
        .context("pharness API rejected permission grant list")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode permission grant list")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn list_observations(args: ObservationListArgs) -> anyhow::Result<()> {
    let http = api_client();
    let mut query = vec![
        ("limit".to_string(), args.limit.to_string()),
        ("offset".to_string(), args.offset.to_string()),
    ];
    if let Some(run_id) = args.run_id {
        query.push(("run_id".to_string(), run_id));
    }
    if let Some(source) = args.source {
        query.push(("source".to_string(), source));
    }
    if let Some(kind) = args.kind {
        query.push(("kind".to_string(), kind));
    }
    if let Some(subject) = args.subject {
        query.push(("subject".to_string(), subject));
    }
    if let Some(resource_namespace) = args.resource_namespace {
        query.push(("resource_namespace".to_string(), resource_namespace));
    }
    if let Some(resource_kind) = args.resource_kind {
        query.push(("resource_kind".to_string(), resource_kind));
    }
    if let Some(resource_name) = args.resource_name {
        query.push(("resource_name".to_string(), resource_name));
    }
    if let Some(observed_after_ms) = args.observed_after_ms {
        query.push((
            "observed_after_ms".to_string(),
            observed_after_ms.to_string(),
        ));
    }
    if let Some(observed_before_ms) = args.observed_before_ms {
        query.push((
            "observed_before_ms".to_string(),
            observed_before_ms.to_string(),
        ));
    }

    let response = http
        .get(api_url(&args.api_url, "/api/observations"))
        .query(&query)
        .send()
        .await
        .context("failed to fetch observations")?
        .error_for_status()
        .context("pharness API rejected observation list")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode observation list")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn get_observation(args: ObservationGetArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .get(api_url(
            &args.api_url,
            &format!("/api/observations/{}", args.observation_id),
        ))
        .send()
        .await
        .context("failed to fetch observation")?
        .error_for_status()
        .context("pharness API rejected observation fetch")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode observation")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn create_observation(args: ObservationCreateArgs) -> anyhow::Result<()> {
    let resource_ref = parse_optional_json_object(args.resource_ref_json, "--resource-ref-json")?;
    let data_json = parse_optional_json_object(args.data_json, "--data-json")?;
    let http = api_client();
    let response = http
        .post(api_url(&args.api_url, "/api/observations"))
        .json(&serde_json::json!({
            "id": args.id,
            "session_id": args.session_id,
            "run_id": args.run_id,
            "source": args.source,
            "kind": args.kind,
            "subject": args.subject,
            "summary": args.summary,
            "resource_namespace": args.resource_namespace,
            "resource_kind": args.resource_kind,
            "resource_name": args.resource_name,
            "resource_ref": resource_ref,
            "artifact_id": args.artifact_id,
            "data_json": data_json,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to create observation")?
        .error_for_status()
        .context("pharness API rejected observation creation")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode observation creation")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn list_incidents(args: IncidentListArgs) -> anyhow::Result<()> {
    let http = api_client();
    let mut query = vec![
        ("limit".to_string(), args.limit.to_string()),
        ("offset".to_string(), args.offset.to_string()),
    ];
    if let Some(run_id) = args.run_id {
        query.push(("run_id".to_string(), run_id));
    }
    if let Some(status) = args.status {
        query.push(("status".to_string(), status));
    }
    if let Some(severity) = args.severity {
        query.push(("severity".to_string(), severity));
    }
    if let Some(resource_namespace) = args.resource_namespace {
        query.push(("resource_namespace".to_string(), resource_namespace));
    }
    if let Some(resource_kind) = args.resource_kind {
        query.push(("resource_kind".to_string(), resource_kind));
    }
    if let Some(resource_name) = args.resource_name {
        query.push(("resource_name".to_string(), resource_name));
    }
    if let Some(created_after_ms) = args.created_after_ms {
        query.push(("created_after_ms".to_string(), created_after_ms.to_string()));
    }
    if let Some(created_before_ms) = args.created_before_ms {
        query.push((
            "created_before_ms".to_string(),
            created_before_ms.to_string(),
        ));
    }

    let response = http
        .get(api_url(&args.api_url, "/api/incidents"))
        .query(&query)
        .send()
        .await
        .context("failed to fetch incidents")?
        .error_for_status()
        .context("pharness API rejected incident list")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode incident list")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn get_incident(args: IncidentGetArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .get(api_url(
            &args.api_url,
            &format!("/api/incidents/{}", args.incident_id),
        ))
        .send()
        .await
        .context("failed to fetch incident")?
        .error_for_status()
        .context("pharness API rejected incident fetch")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode incident")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn create_incident(args: IncidentCreateArgs) -> anyhow::Result<()> {
    let data_json = parse_optional_json_object(args.data_json, "--data-json")?;
    let http = api_client();
    let response = http
        .post(api_url(&args.api_url, "/api/incidents"))
        .json(&serde_json::json!({
            "id": args.id,
            "observation_id": args.observation_id,
            "status": args.status,
            "severity": args.severity,
            "title": args.title,
            "summary": args.summary,
            "resource_namespace": args.resource_namespace,
            "resource_kind": args.resource_kind,
            "resource_name": args.resource_name,
            "data_json": data_json,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to create incident")?
        .error_for_status()
        .context("pharness API rejected incident creation")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode incident creation")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn list_remediation_plans(args: RemediationPlanListArgs) -> anyhow::Result<()> {
    let http = api_client();
    let mut query = vec![
        ("limit".to_string(), args.limit.to_string()),
        ("offset".to_string(), args.offset.to_string()),
    ];
    if let Some(incident_id) = args.incident_id {
        query.push(("incident_id".to_string(), incident_id));
    }
    if let Some(run_id) = args.run_id {
        query.push(("run_id".to_string(), run_id));
    }
    if let Some(status) = args.status {
        query.push(("status".to_string(), status));
    }
    if let Some(risk_level) = args.risk_level {
        query.push(("risk_level".to_string(), risk_level));
    }
    if let Some(resource_namespace) = args.resource_namespace {
        query.push(("resource_namespace".to_string(), resource_namespace));
    }
    if let Some(resource_kind) = args.resource_kind {
        query.push(("resource_kind".to_string(), resource_kind));
    }
    if let Some(resource_name) = args.resource_name {
        query.push(("resource_name".to_string(), resource_name));
    }
    if let Some(created_after_ms) = args.created_after_ms {
        query.push(("created_after_ms".to_string(), created_after_ms.to_string()));
    }
    if let Some(created_before_ms) = args.created_before_ms {
        query.push((
            "created_before_ms".to_string(),
            created_before_ms.to_string(),
        ));
    }

    let response = http
        .get(api_url(&args.api_url, "/api/remediation-plans"))
        .query(&query)
        .send()
        .await
        .context("failed to fetch remediation plans")?
        .error_for_status()
        .context("pharness API rejected remediation plan list")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode remediation plan list")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn get_remediation_plan(args: RemediationPlanGetArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .get(api_url(
            &args.api_url,
            &format!("/api/remediation-plans/{}", args.plan_id),
        ))
        .send()
        .await
        .context("failed to fetch remediation plan")?
        .error_for_status()
        .context("pharness API rejected remediation plan fetch")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode remediation plan")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn create_remediation_plan(args: RemediationPlanCreateArgs) -> anyhow::Result<()> {
    let plan_json = parse_optional_json_object(args.plan_json, "--plan-json")?;
    let http = api_client();
    let response = http
        .post(api_url(&args.api_url, "/api/remediation-plans"))
        .json(&serde_json::json!({
            "id": args.id,
            "incident_id": args.incident_id,
            "status": args.status,
            "title": args.title,
            "summary": args.summary,
            "risk_level": args.risk_level,
            "requires_approval": args.requires_approval,
            "resource_namespace": args.resource_namespace,
            "resource_kind": args.resource_kind,
            "resource_name": args.resource_name,
            "plan_json": plan_json,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to create remediation plan")?
        .error_for_status()
        .context("pharness API rejected remediation plan creation")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode remediation plan creation")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn list_work_plans(args: WorkPlanListArgs) -> anyhow::Result<()> {
    let http = api_client();
    let mut query = vec![
        ("limit".to_string(), args.limit.to_string()),
        ("offset".to_string(), args.offset.to_string()),
    ];
    if let Some(remediation_plan_id) = args.remediation_plan_id {
        query.push(("remediation_plan_id".to_string(), remediation_plan_id));
    }
    if let Some(incident_id) = args.incident_id {
        query.push(("incident_id".to_string(), incident_id));
    }
    if let Some(run_id) = args.run_id {
        query.push(("run_id".to_string(), run_id));
    }
    if let Some(status) = args.status {
        query.push(("status".to_string(), status));
    }
    if let Some(risk_level) = args.risk_level {
        query.push(("risk_level".to_string(), risk_level));
    }
    if let Some(resource_namespace) = args.resource_namespace {
        query.push(("resource_namespace".to_string(), resource_namespace));
    }
    if let Some(resource_kind) = args.resource_kind {
        query.push(("resource_kind".to_string(), resource_kind));
    }
    if let Some(resource_name) = args.resource_name {
        query.push(("resource_name".to_string(), resource_name));
    }
    if let Some(created_after_ms) = args.created_after_ms {
        query.push(("created_after_ms".to_string(), created_after_ms.to_string()));
    }
    if let Some(created_before_ms) = args.created_before_ms {
        query.push((
            "created_before_ms".to_string(),
            created_before_ms.to_string(),
        ));
    }

    let response = http
        .get(api_url(&args.api_url, "/api/work-plans"))
        .query(&query)
        .send()
        .await
        .context("failed to fetch work plans")?
        .error_for_status()
        .context("pharness API rejected work plan list")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode work plan list")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn get_work_plan(args: WorkPlanGetArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .get(api_url(
            &args.api_url,
            &format!("/api/work-plans/{}", args.work_plan_id),
        ))
        .send()
        .await
        .context("failed to fetch work plan")?
        .error_for_status()
        .context("pharness API rejected work plan fetch")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode work plan")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn get_work_plan_readiness(args: WorkPlanReadinessArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .get(api_url(
            &args.api_url,
            &format!("/api/work-plans/{}/readiness", args.work_plan_id),
        ))
        .send()
        .await
        .context("failed to fetch work plan readiness")?
        .error_for_status()
        .context("pharness API rejected work plan readiness fetch")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode work plan readiness")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn get_work_plan_flow(args: WorkPlanFlowArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .get(api_url(
            &args.api_url,
            &format!("/api/work-plans/{}/flow", args.work_plan_id),
        ))
        .send()
        .await
        .context("failed to fetch work plan flow")?
        .error_for_status()
        .context("pharness API rejected work plan flow fetch")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode work plan flow")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn create_work_plan_from_remediation_plan(
    args: WorkPlanCreateFromRemediationPlanArgs,
) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            "/api/work-plans/from-remediation-plan",
        ))
        .json(&serde_json::json!({
            "remediation_plan_id": args.remediation_plan_id,
        }))
        .send()
        .await
        .context("failed to create work plan from remediation plan")?
        .error_for_status()
        .context("pharness API rejected work plan creation")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode work plan creation")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn revise_work_plan(args: WorkPlanReviseArgs) -> anyhow::Result<()> {
    let work_plan_json = parse_json_object(&args.work_plan_json, "--work-plan-json")
        .context("failed to parse --work-plan-json as a JSON object")?;
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            &format!("/api/work-plans/{}/revise", args.work_plan_id),
        ))
        .json(&serde_json::json!({
            "title": args.title,
            "summary": args.summary,
            "risk_level": args.risk_level,
            "requires_approval": args.requires_approval,
            "work_plan_json": work_plan_json,
            "actor": args.actor,
            "reason": args.reason,
            "material_change": args.material_change,
        }))
        .send()
        .await
        .context("failed to revise work plan")?
        .error_for_status()
        .context("pharness API rejected work plan revision")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode work plan revision")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn transition_work_plan(args: WorkPlanTransitionArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            &format!("/api/work-plans/{}/transition", args.work_plan_id),
        ))
        .json(&serde_json::json!({
            "target_status": args.target_status,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to transition work plan")?
        .error_for_status()
        .context("pharness API rejected work plan transition")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode work plan transition")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn create_work_plan_trusted_envelope(
    args: WorkPlanCreateTrustedEnvelopeArgs,
) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            &format!("/api/work-plans/{}/trusted-envelope", args.work_plan_id),
        ))
        .json(&CreateTrustedEnvelopeRequest {
            subject: args.subject,
            created_by: args.created_by,
            reason: args.reason,
            environment: Some(args.environment),
            namespace: args.namespace,
            repo: args.repo,
            branch: args.branch,
            production_impacting: Some(args.production_impacting),
            expires_at: args.expires_at,
        })
        .send()
        .await
        .context("failed to create WorkPlan trusted envelope")?
        .error_for_status()
        .context("pharness API rejected WorkPlan trusted envelope creation")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode WorkPlan trusted envelope creation")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn list_change_sets(args: ChangeSetListArgs) -> anyhow::Result<()> {
    let http = api_client();
    let mut query = Vec::new();
    if let Some(value) = args.work_plan_id {
        query.push(("work_plan_id", value));
    }
    if let Some(value) = args.remediation_plan_id {
        query.push(("remediation_plan_id", value));
    }
    if let Some(value) = args.incident_id {
        query.push(("incident_id", value));
    }
    if let Some(value) = args.run_id {
        query.push(("run_id", value));
    }
    if let Some(value) = args.status {
        query.push(("status", value));
    }
    if let Some(value) = args.risk_level {
        query.push(("risk_level", value));
    }
    if let Some(value) = args.resource_namespace {
        query.push(("resource_namespace", value));
    }
    if let Some(value) = args.resource_kind {
        query.push(("resource_kind", value));
    }
    if let Some(value) = args.resource_name {
        query.push(("resource_name", value));
    }
    if let Some(value) = args.created_after_ms {
        query.push(("created_after_ms", value.to_string()));
    }
    if let Some(value) = args.created_before_ms {
        query.push(("created_before_ms", value.to_string()));
    }
    query.push(("limit", args.limit.to_string()));
    query.push(("offset", args.offset.to_string()));

    let response = http
        .get(api_url(&args.api_url, "/api/change-sets"))
        .query(&query)
        .send()
        .await
        .context("failed to fetch change sets")?
        .error_for_status()
        .context("pharness API rejected change set list")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode change set list")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn get_change_set(args: ChangeSetGetArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .get(api_url(
            &args.api_url,
            &format!("/api/change-sets/{}", args.change_set_id),
        ))
        .send()
        .await
        .context("failed to fetch change set")?
        .error_for_status()
        .context("pharness API rejected change set fetch")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode change set")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn get_change_set_readiness(args: ChangeSetReadinessArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .get(api_url(
            &args.api_url,
            &format!("/api/change-sets/{}/readiness", args.change_set_id),
        ))
        .send()
        .await
        .context("failed to fetch change set readiness")?
        .error_for_status()
        .context("pharness API rejected change set readiness fetch")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode change set readiness")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn get_change_set_flow(args: ChangeSetFlowArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .get(api_url(
            &args.api_url,
            &format!("/api/change-sets/{}/flow", args.change_set_id),
        ))
        .send()
        .await
        .context("failed to fetch change set flow")?
        .error_for_status()
        .context("pharness API rejected change set flow fetch")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode change set flow")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn create_change_set(args: ChangeSetCreateArgs) -> anyhow::Result<()> {
    let change_set_json = parse_json_object(&args.change_set_json, "--change-set-json")
        .context("failed to parse --change-set-json as a JSON object")?;
    let http = api_client();
    let response = http
        .post(api_url(&args.api_url, "/api/change-sets"))
        .json(&serde_json::json!({
            "work_plan_id": args.work_plan_id,
            "title": args.title,
            "summary": args.summary,
            "risk_level": args.risk_level,
            "change_set_json": change_set_json,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to create change set")?
        .error_for_status()
        .context("pharness API rejected change set creation")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode change set creation")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn revise_change_set(args: ChangeSetReviseArgs) -> anyhow::Result<()> {
    let change_set_json = parse_json_object(&args.change_set_json, "--change-set-json")
        .context("failed to parse --change-set-json as a JSON object")?;
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            &format!("/api/change-sets/{}/revise", args.change_set_id),
        ))
        .json(&serde_json::json!({
            "title": args.title,
            "summary": args.summary,
            "risk_level": args.risk_level,
            "change_set_json": change_set_json,
            "actor": args.actor,
            "reason": args.reason,
            "material_change": args.material_change,
        }))
        .send()
        .await
        .context("failed to revise change set")?
        .error_for_status()
        .context("pharness API rejected change set revision")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode change set revision")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn transition_change_set(args: ChangeSetTransitionArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            &format!("/api/change-sets/{}/transition", args.change_set_id),
        ))
        .json(&serde_json::json!({
            "target_status": args.target_status,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to transition change set")?
        .error_for_status()
        .context("pharness API rejected change set transition")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode change set transition")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn create_change_set_trusted_envelope(
    args: ChangeSetCreateTrustedEnvelopeArgs,
) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            &format!("/api/change-sets/{}/trusted-envelope", args.change_set_id),
        ))
        .json(&CreateTrustedEnvelopeRequest {
            subject: args.subject,
            created_by: args.created_by,
            reason: args.reason,
            environment: Some(args.environment),
            namespace: args.namespace,
            repo: args.repo,
            branch: args.branch,
            production_impacting: Some(args.production_impacting),
            expires_at: args.expires_at,
        })
        .send()
        .await
        .context("failed to create ChangeSet trusted envelope")?
        .error_for_status()
        .context("pharness API rejected ChangeSet trusted envelope creation")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode ChangeSet trusted envelope creation")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn list_pipeline_intents(args: PipelineIntentListArgs) -> anyhow::Result<()> {
    let http = api_client();
    let mut query = Vec::new();
    if let Some(value) = args.change_set_id {
        query.push(("change_set_id", value));
    }
    if let Some(value) = args.work_plan_id {
        query.push(("work_plan_id", value));
    }
    if let Some(value) = args.remediation_plan_id {
        query.push(("remediation_plan_id", value));
    }
    if let Some(value) = args.incident_id {
        query.push(("incident_id", value));
    }
    if let Some(value) = args.run_id {
        query.push(("run_id", value));
    }
    if let Some(value) = args.status {
        query.push(("status", value));
    }
    if let Some(value) = args.intent_kind {
        query.push(("intent_kind", value));
    }
    if let Some(value) = args.risk_level {
        query.push(("risk_level", value));
    }
    if let Some(value) = args.resource_namespace {
        query.push(("resource_namespace", value));
    }
    if let Some(value) = args.resource_kind {
        query.push(("resource_kind", value));
    }
    if let Some(value) = args.resource_name {
        query.push(("resource_name", value));
    }
    if let Some(value) = args.created_after_ms {
        query.push(("created_after_ms", value.to_string()));
    }
    if let Some(value) = args.created_before_ms {
        query.push(("created_before_ms", value.to_string()));
    }
    query.push(("limit", args.limit.to_string()));
    query.push(("offset", args.offset.to_string()));

    let response = http
        .get(api_url(&args.api_url, "/api/pipeline-intents"))
        .query(&query)
        .send()
        .await
        .context("failed to fetch pipeline intents")?
        .error_for_status()
        .context("pharness API rejected pipeline intent list")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode pipeline intent list")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn get_pipeline_intent(args: PipelineIntentGetArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .get(api_url(
            &args.api_url,
            &format!("/api/pipeline-intents/{}", args.pipeline_intent_id),
        ))
        .send()
        .await
        .context("failed to fetch pipeline intent")?
        .error_for_status()
        .context("pharness API rejected pipeline intent fetch")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode pipeline intent")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn create_pipeline_intent_from_change_set(
    args: PipelineIntentCreateFromChangeSetArgs,
) -> anyhow::Result<()> {
    let intent_json = args
        .intent_json
        .as_deref()
        .map(|value| parse_json_object(value, "--intent-json"))
        .transpose()
        .context("failed to parse --intent-json as a JSON object")?;
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            "/api/pipeline-intents/from-change-set",
        ))
        .json(&serde_json::json!({
            "change_set_id": args.change_set_id,
            "title": args.title,
            "summary": args.summary,
            "risk_level": args.risk_level,
            "intent_kind": args.intent_kind,
            "intent_json": intent_json,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to create pipeline intent")?
        .error_for_status()
        .context("pharness API rejected pipeline intent creation")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode pipeline intent creation")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn transition_pipeline_intent(args: PipelineIntentTransitionArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            &format!(
                "/api/pipeline-intents/{}/transition",
                args.pipeline_intent_id
            ),
        ))
        .json(&serde_json::json!({
            "target_status": args.target_status,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to transition pipeline intent")?
        .error_for_status()
        .context("pharness API rejected pipeline intent transition")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode pipeline intent transition")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn attach_pipeline_intent_evidence(
    args: PipelineIntentAttachEvidenceArgs,
) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            &format!("/api/pipeline-intents/{}/evidence", args.pipeline_intent_id),
        ))
        .json(&serde_json::json!({
            "observation_id": args.observation_id,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to attach pipeline intent evidence")?
        .error_for_status()
        .context("pharness API rejected pipeline intent evidence attachment")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode pipeline intent evidence attachment")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn create_pipeline_intent_trusted_envelope(
    args: PipelineIntentCreateTrustedEnvelopeArgs,
) -> anyhow::Result<()> {
    let response = api_client()
        .post(api_url(
            &args.api_url,
            &format!(
                "/api/pipeline-intents/{}/trusted-envelope",
                args.pipeline_intent_id
            ),
        ))
        .json(&serde_json::json!({
            "subject": args.subject,
            "created_by": args.created_by,
            "reason": args.reason,
            "expires_at": args.expires_at,
        }))
        .send()
        .await
        .context("failed to create PipelineIntent trusted envelope")?
        .error_for_status()
        .context("pharness API rejected PipelineIntent trusted envelope")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode PipelineIntent trusted envelope")?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn execute_pipeline_intent(args: PipelineIntentExecuteArgs) -> anyhow::Result<()> {
    let response = api_client()
        .post(api_url(
            &args.api_url,
            &format!("/api/pipeline-intents/{}/execute", args.pipeline_intent_id),
        ))
        .json(&serde_json::json!({
            "dry_run": !args.apply,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to execute PipelineIntent")?
        .error_for_status()
        .context("pharness API rejected PipelineIntent execution")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode PipelineIntent execution")?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn list_pipeline_contracts(args: PipelineContractListArgs) -> anyhow::Result<()> {
    let mut query = vec![
        ("limit", args.limit.to_string()),
        ("offset", args.offset.to_string()),
    ];
    if let Some(value) = args.namespace {
        query.push(("namespace", value));
    }
    if let Some(value) = args.pipeline_ref {
        query.push(("pipeline_ref", value));
    }
    if let Some(value) = args.status {
        query.push(("status", value));
    }
    let response = api_client()
        .get(api_url(&args.api_url, "/api/pipeline-contracts"))
        .query(&query)
        .send()
        .await
        .context("failed to fetch Pipeline contracts")?
        .error_for_status()
        .context("pharness API rejected Pipeline contract list")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode Pipeline contract list")?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn get_pipeline_contract(args: PipelineContractGetArgs) -> anyhow::Result<()> {
    let response = api_client()
        .get(api_url(
            &args.api_url,
            &format!("/api/pipeline-contracts/{}", args.pipeline_contract_id),
        ))
        .send()
        .await
        .context("failed to fetch Pipeline contract")?
        .error_for_status()
        .context("pharness API rejected Pipeline contract fetch")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode Pipeline contract")?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn create_pipeline_contract(args: PipelineContractCreateArgs) -> anyhow::Result<()> {
    let contract_json = parse_json_object(&args.contract_json, "--contract-json")
        .context("failed to parse --contract-json as a JSON object")?;
    let response = api_client()
        .post(api_url(&args.api_url, "/api/pipeline-contracts"))
        .json(&serde_json::json!({
            "namespace": args.namespace,
            "pipeline_ref": args.pipeline_ref,
            "version": args.version,
            "contract_json": contract_json,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to create Pipeline contract")?
        .error_for_status()
        .context("pharness API rejected Pipeline contract creation")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode Pipeline contract creation")?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn retire_pipeline_contract(args: PipelineContractRetireArgs) -> anyhow::Result<()> {
    let response = api_client()
        .post(api_url(
            &args.api_url,
            &format!(
                "/api/pipeline-contracts/{}/transition",
                args.pipeline_contract_id
            ),
        ))
        .json(&serde_json::json!({
            "target_status": "retired",
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to retire Pipeline contract")?
        .error_for_status()
        .context("pharness API rejected Pipeline contract retirement")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode Pipeline contract retirement")?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn replace_pipeline_contract(args: PipelineContractReplaceArgs) -> anyhow::Result<()> {
    let contract_json = parse_json_object(&args.contract_json, "--contract-json")
        .context("failed to parse --contract-json as a JSON object")?;
    let response = api_client()
        .post(api_url(
            &args.api_url,
            &format!(
                "/api/pipeline-contracts/{}/replace",
                args.pipeline_contract_id
            ),
        ))
        .json(&serde_json::json!({
            "version": args.version,
            "contract_json": contract_json,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to replace Pipeline contract")?
        .error_for_status()
        .context("pharness API rejected Pipeline contract replacement")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode Pipeline contract replacement")?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn list_deployment_contracts(args: DeploymentContractListArgs) -> anyhow::Result<()> {
    let mut query = vec![
        ("limit", args.limit.to_string()),
        ("offset", args.offset.to_string()),
    ];
    if let Some(value) = args.target_environment {
        query.push(("target_environment", value));
    }
    if let Some(value) = args.target_namespace {
        query.push(("target_namespace", value));
    }
    if let Some(value) = args.argo_application {
        query.push(("argo_application", value));
    }
    if let Some(value) = args.status {
        query.push(("status", value));
    }
    let response = api_client()
        .get(api_url(&args.api_url, "/api/deployment-contracts"))
        .query(&query)
        .send()
        .await
        .context("failed to fetch Deployment contracts")?
        .error_for_status()
        .context("pharness API rejected Deployment contract list")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode Deployment contract list")?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn get_deployment_contract(args: DeploymentContractGetArgs) -> anyhow::Result<()> {
    let response = api_client()
        .get(api_url(
            &args.api_url,
            &format!("/api/deployment-contracts/{}", args.deployment_contract_id),
        ))
        .send()
        .await
        .context("failed to fetch Deployment contract")?
        .error_for_status()
        .context("pharness API rejected Deployment contract fetch")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode Deployment contract")?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn create_deployment_contract(args: DeploymentContractCreateArgs) -> anyhow::Result<()> {
    let contract_json = parse_json_object(&args.contract_json, "--contract-json")
        .context("failed to parse --contract-json as a JSON object")?;
    let response = api_client()
        .post(api_url(&args.api_url, "/api/deployment-contracts"))
        .json(&serde_json::json!({
            "target_environment": args.target_environment,
            "target_namespace": args.target_namespace,
            "argo_application": args.argo_application,
            "version": args.version,
            "contract_json": contract_json,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to create Deployment contract")?
        .error_for_status()
        .context("pharness API rejected Deployment contract creation")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode Deployment contract creation")?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn retire_deployment_contract(args: DeploymentContractRetireArgs) -> anyhow::Result<()> {
    let response = api_client()
        .post(api_url(&args.api_url, &format!("/api/deployment-contracts/{}/transition", args.deployment_contract_id)))
        .json(&serde_json::json!({ "target_status": "retired", "actor": args.actor, "reason": args.reason }))
        .send().await.context("failed to retire Deployment contract")?
        .error_for_status().context("pharness API rejected Deployment contract retirement")?
        .json::<serde_json::Value>().await.context("failed to decode Deployment contract retirement")?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn list_deployment_intents(args: DeploymentIntentListArgs) -> anyhow::Result<()> {
    let http = api_client();
    let mut query = Vec::new();
    if let Some(value) = args.pipeline_intent_id {
        query.push(("pipeline_intent_id", value));
    }
    if let Some(value) = args.change_set_id {
        query.push(("change_set_id", value));
    }
    if let Some(value) = args.work_plan_id {
        query.push(("work_plan_id", value));
    }
    if let Some(value) = args.remediation_plan_id {
        query.push(("remediation_plan_id", value));
    }
    if let Some(value) = args.incident_id {
        query.push(("incident_id", value));
    }
    if let Some(value) = args.run_id {
        query.push(("run_id", value));
    }
    if let Some(value) = args.status {
        query.push(("status", value));
    }
    if let Some(value) = args.intent_kind {
        query.push(("intent_kind", value));
    }
    if let Some(value) = args.risk_level {
        query.push(("risk_level", value));
    }
    if let Some(value) = args.target_environment {
        query.push(("target_environment", value));
    }
    if let Some(value) = args.target_namespace {
        query.push(("target_namespace", value));
    }
    if let Some(value) = args.argo_application {
        query.push(("argo_application", value));
    }
    if let Some(value) = args.resource_namespace {
        query.push(("resource_namespace", value));
    }
    if let Some(value) = args.resource_kind {
        query.push(("resource_kind", value));
    }
    if let Some(value) = args.resource_name {
        query.push(("resource_name", value));
    }
    if let Some(value) = args.created_after_ms {
        query.push(("created_after_ms", value.to_string()));
    }
    if let Some(value) = args.created_before_ms {
        query.push(("created_before_ms", value.to_string()));
    }
    query.push(("limit", args.limit.to_string()));
    query.push(("offset", args.offset.to_string()));

    let response = http
        .get(api_url(&args.api_url, "/api/deployment-intents"))
        .query(&query)
        .send()
        .await
        .context("failed to fetch deployment intents")?
        .error_for_status()
        .context("pharness API rejected deployment intent list")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode deployment intent list")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn get_deployment_intent(args: DeploymentIntentGetArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .get(api_url(
            &args.api_url,
            &format!("/api/deployment-intents/{}", args.deployment_intent_id),
        ))
        .send()
        .await
        .context("failed to fetch deployment intent")?
        .error_for_status()
        .context("pharness API rejected deployment intent fetch")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode deployment intent")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn create_deployment_intent_from_pipeline_intent(
    args: DeploymentIntentCreateFromPipelineIntentArgs,
) -> anyhow::Result<()> {
    let intent_json = args
        .intent_json
        .as_deref()
        .map(|value| parse_json_object(value, "--intent-json"))
        .transpose()
        .context("failed to parse --intent-json as a JSON object")?;
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            "/api/deployment-intents/from-pipeline-intent",
        ))
        .json(&serde_json::json!({
            "pipeline_intent_id": args.pipeline_intent_id,
            "title": args.title,
            "summary": args.summary,
            "risk_level": args.risk_level,
            "intent_kind": args.intent_kind,
            "target_environment": args.target_environment,
            "target_namespace": args.target_namespace,
            "argo_application": args.argo_application,
            "intent_json": intent_json,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to create deployment intent")?
        .error_for_status()
        .context("pharness API rejected deployment intent creation")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode deployment intent creation")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn transition_deployment_intent(args: DeploymentIntentTransitionArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            &format!(
                "/api/deployment-intents/{}/transition",
                args.deployment_intent_id
            ),
        ))
        .json(&serde_json::json!({
            "target_status": args.target_status,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to transition deployment intent")?
        .error_for_status()
        .context("pharness API rejected deployment intent transition")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode deployment intent transition")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn attach_deployment_intent_evidence(
    args: DeploymentIntentAttachEvidenceArgs,
) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            &format!(
                "/api/deployment-intents/{}/evidence",
                args.deployment_intent_id
            ),
        ))
        .json(&serde_json::json!({
            "observation_id": args.observation_id,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to attach deployment intent evidence")?
        .error_for_status()
        .context("pharness API rejected deployment intent evidence attachment")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode deployment intent evidence attachment")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn list_releases(args: ReleaseListArgs) -> anyhow::Result<()> {
    let http = api_client();
    let mut query = Vec::new();
    if let Some(value) = args.deployment_intent_id {
        query.push(("deployment_intent_id", value));
    }
    if let Some(value) = args.pipeline_intent_id {
        query.push(("pipeline_intent_id", value));
    }
    if let Some(value) = args.change_set_id {
        query.push(("change_set_id", value));
    }
    if let Some(value) = args.work_plan_id {
        query.push(("work_plan_id", value));
    }
    if let Some(value) = args.remediation_plan_id {
        query.push(("remediation_plan_id", value));
    }
    if let Some(value) = args.incident_id {
        query.push(("incident_id", value));
    }
    if let Some(value) = args.run_id {
        query.push(("run_id", value));
    }
    if let Some(value) = args.status {
        query.push(("status", value));
    }
    if let Some(value) = args.release_kind {
        query.push(("release_kind", value));
    }
    if let Some(value) = args.risk_level {
        query.push(("risk_level", value));
    }
    if let Some(value) = args.target_environment {
        query.push(("target_environment", value));
    }
    if let Some(value) = args.target_namespace {
        query.push(("target_namespace", value));
    }
    if let Some(value) = args.argo_application {
        query.push(("argo_application", value));
    }
    if let Some(value) = args.version {
        query.push(("version", value));
    }
    if let Some(value) = args.commit_sha {
        query.push(("commit_sha", value));
    }
    if let Some(value) = args.image_digest {
        query.push(("image_digest", value));
    }
    if let Some(value) = args.created_after_ms {
        query.push(("created_after_ms", value.to_string()));
    }
    if let Some(value) = args.created_before_ms {
        query.push(("created_before_ms", value.to_string()));
    }
    query.push(("limit", args.limit.to_string()));
    query.push(("offset", args.offset.to_string()));

    let response = http
        .get(api_url(&args.api_url, "/api/releases"))
        .query(&query)
        .send()
        .await
        .context("failed to fetch releases")?
        .error_for_status()
        .context("pharness API rejected release list")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode release list")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn get_release(args: ReleaseGetArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .get(api_url(
            &args.api_url,
            &format!("/api/releases/{}", args.release_id),
        ))
        .send()
        .await
        .context("failed to fetch release")?
        .error_for_status()
        .context("pharness API rejected release fetch")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode release")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn create_release_from_deployment_intent(
    args: ReleaseCreateFromDeploymentIntentArgs,
) -> anyhow::Result<()> {
    let release_json = args
        .release_json
        .as_deref()
        .map(|value| parse_json_object(value, "--release-json"))
        .transpose()
        .context("failed to parse --release-json as a JSON object")?;
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            "/api/releases/from-deployment-intent",
        ))
        .json(&serde_json::json!({
            "deployment_intent_id": args.deployment_intent_id,
            "title": args.title,
            "summary": args.summary,
            "risk_level": args.risk_level,
            "release_kind": args.release_kind,
            "version": args.version,
            "commit_sha": args.commit_sha,
            "image_digest": args.image_digest,
            "rollback_ref": args.rollback_ref,
            "release_json": release_json,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to create release")?
        .error_for_status()
        .context("pharness API rejected release creation")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode release creation")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn transition_release(args: ReleaseTransitionArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            &format!("/api/releases/{}/transition", args.release_id),
        ))
        .json(&serde_json::json!({
            "target_status": args.target_status,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to transition release")?
        .error_for_status()
        .context("pharness API rejected release transition")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode release transition")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn attach_release_evidence(args: ReleaseAttachEvidenceArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            &format!("/api/releases/{}/evidence", args.release_id),
        ))
        .json(&serde_json::json!({
            "observation_id": args.observation_id,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to attach release evidence")?
        .error_for_status()
        .context("pharness API rejected release evidence attachment")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode release evidence attachment")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn list_registry_evidence(args: RegistryEvidenceListArgs) -> anyhow::Result<()> {
    let http = api_client();
    let mut query = Vec::new();
    if let Some(value) = args.release_id {
        query.push(("release_id", value));
    }
    if let Some(value) = args.deployment_intent_id {
        query.push(("deployment_intent_id", value));
    }
    if let Some(value) = args.pipeline_intent_id {
        query.push(("pipeline_intent_id", value));
    }
    if let Some(value) = args.change_set_id {
        query.push(("change_set_id", value));
    }
    if let Some(value) = args.work_plan_id {
        query.push(("work_plan_id", value));
    }
    if let Some(value) = args.remediation_plan_id {
        query.push(("remediation_plan_id", value));
    }
    if let Some(value) = args.incident_id {
        query.push(("incident_id", value));
    }
    if let Some(value) = args.run_id {
        query.push(("run_id", value));
    }
    if let Some(value) = args.status {
        query.push(("status", value));
    }
    if let Some(value) = args.risk_level {
        query.push(("risk_level", value));
    }
    if let Some(value) = args.registry {
        query.push(("registry", value));
    }
    if let Some(value) = args.repository {
        query.push(("repository", value));
    }
    if let Some(value) = args.image_ref {
        query.push(("image_ref", value));
    }
    if let Some(value) = args.image_digest {
        query.push(("image_digest", value));
    }
    if let Some(value) = args.tag {
        query.push(("tag", value));
    }
    if let Some(value) = args.source {
        query.push(("source", value));
    }
    if let Some(value) = args.verification_status {
        query.push(("verification_status", value));
    }
    if let Some(value) = args.created_after_ms {
        query.push(("created_after_ms", value.to_string()));
    }
    if let Some(value) = args.created_before_ms {
        query.push(("created_before_ms", value.to_string()));
    }
    query.push(("limit", args.limit.to_string()));
    query.push(("offset", args.offset.to_string()));

    let response = http
        .get(api_url(&args.api_url, "/api/registry-evidence"))
        .query(&query)
        .send()
        .await
        .context("failed to fetch registry evidence")?
        .error_for_status()
        .context("pharness API rejected registry evidence list")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode registry evidence list")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn get_registry_evidence(args: RegistryEvidenceGetArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .get(api_url(
            &args.api_url,
            &format!("/api/registry-evidence/{}", args.evidence_id),
        ))
        .send()
        .await
        .context("failed to fetch registry evidence")?
        .error_for_status()
        .context("pharness API rejected registry evidence fetch")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode registry evidence")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn create_registry_evidence_from_release(
    args: RegistryEvidenceCreateFromReleaseArgs,
) -> anyhow::Result<()> {
    let evidence_json = args
        .evidence_json
        .as_deref()
        .map(|value| parse_json_object(value, "--evidence-json"))
        .transpose()
        .context("failed to parse --evidence-json as a JSON object")?;
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            "/api/registry-evidence/from-release",
        ))
        .json(&serde_json::json!({
            "release_id": args.release_id,
            "title": args.title,
            "summary": args.summary,
            "risk_level": args.risk_level,
            "registry": args.registry,
            "repository": args.repository,
            "image_ref": args.image_ref,
            "image_digest": args.image_digest,
            "tag": args.tag,
            "source": args.source,
            "verification_status": args.verification_status,
            "evidence_json": evidence_json,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to create registry evidence")?
        .error_for_status()
        .context("pharness API rejected registry evidence creation")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode registry evidence creation")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn create_registry_evidence_from_inspection(
    args: RegistryEvidenceCreateFromInspectionArgs,
) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            "/api/registry-evidence/from-registry-inspection",
        ))
        .json(&serde_json::json!({
            "release_id": args.release_id,
            "image_ref": args.image_ref,
            "registry_base_url": args.registry_base_url,
            "title": args.title,
            "summary": args.summary,
            "risk_level": args.risk_level,
            "actor": args.actor,
            "reason": args.reason,
            "timeout_ms": args.timeout_ms,
        }))
        .send()
        .await
        .context("failed to create registry evidence from inspection")?
        .error_for_status()
        .context("pharness API rejected registry evidence inspection creation")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode registry evidence inspection creation")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn transition_registry_evidence(args: RegistryEvidenceTransitionArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            &format!("/api/registry-evidence/{}/transition", args.evidence_id),
        ))
        .json(&serde_json::json!({
            "target_status": args.target_status,
            "actor": args.actor,
            "reason": args.reason,
        }))
        .send()
        .await
        .context("failed to transition registry evidence")?
        .error_for_status()
        .context("pharness API rejected registry evidence transition")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode registry evidence transition")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn list_approval_gates(args: ApprovalGateListArgs) -> anyhow::Result<()> {
    let http = api_client();
    let mut query = vec![
        ("limit".to_string(), args.limit.to_string()),
        ("offset".to_string(), args.offset.to_string()),
    ];
    if let Some(remediation_plan_id) = args.remediation_plan_id {
        query.push(("remediation_plan_id".to_string(), remediation_plan_id));
    }
    if let Some(incident_id) = args.incident_id {
        query.push(("incident_id".to_string(), incident_id));
    }
    if let Some(run_id) = args.run_id {
        query.push(("run_id".to_string(), run_id));
    }
    if let Some(status) = args.status {
        query.push(("status".to_string(), status));
    }
    if let Some(gate_kind) = args.gate_kind {
        query.push(("gate_kind".to_string(), gate_kind));
    }
    if let Some(risk_level) = args.risk_level {
        query.push(("risk_level".to_string(), risk_level));
    }
    if let Some(resource_namespace) = args.resource_namespace {
        query.push(("resource_namespace".to_string(), resource_namespace));
    }
    if let Some(resource_kind) = args.resource_kind {
        query.push(("resource_kind".to_string(), resource_kind));
    }
    if let Some(resource_name) = args.resource_name {
        query.push(("resource_name".to_string(), resource_name));
    }
    if let Some(created_after_ms) = args.created_after_ms {
        query.push(("created_after_ms".to_string(), created_after_ms.to_string()));
    }
    if let Some(created_before_ms) = args.created_before_ms {
        query.push((
            "created_before_ms".to_string(),
            created_before_ms.to_string(),
        ));
    }

    let response = http
        .get(api_url(&args.api_url, "/api/approval-gates"))
        .query(&query)
        .send()
        .await
        .context("failed to fetch approval gates")?
        .error_for_status()
        .context("pharness API rejected approval gate list")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode approval gate list")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn summarize_approval_gates(args: ApprovalGateSummaryArgs) -> anyhow::Result<()> {
    let http = api_client();
    let mut query = vec![("status".to_string(), args.status)];
    if let Some(remediation_plan_id) = args.remediation_plan_id {
        query.push(("remediation_plan_id".to_string(), remediation_plan_id));
    }
    if let Some(incident_id) = args.incident_id {
        query.push(("incident_id".to_string(), incident_id));
    }
    if let Some(run_id) = args.run_id {
        query.push(("run_id".to_string(), run_id));
    }
    if let Some(gate_kind) = args.gate_kind {
        query.push(("gate_kind".to_string(), gate_kind));
    }
    if let Some(risk_level) = args.risk_level {
        query.push(("risk_level".to_string(), risk_level));
    }
    if let Some(resource_namespace) = args.resource_namespace {
        query.push(("resource_namespace".to_string(), resource_namespace));
    }
    if let Some(resource_kind) = args.resource_kind {
        query.push(("resource_kind".to_string(), resource_kind));
    }
    if let Some(resource_name) = args.resource_name {
        query.push(("resource_name".to_string(), resource_name));
    }
    if let Some(created_after_ms) = args.created_after_ms {
        query.push(("created_after_ms".to_string(), created_after_ms.to_string()));
    }
    if let Some(created_before_ms) = args.created_before_ms {
        query.push((
            "created_before_ms".to_string(),
            created_before_ms.to_string(),
        ));
    }

    let response = http
        .get(api_url(&args.api_url, "/api/approval-gates/summary"))
        .query(&query)
        .send()
        .await
        .context("failed to fetch approval gate summary")?
        .error_for_status()
        .context("pharness API rejected approval gate summary")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode approval gate summary")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn get_approval_gate(args: ApprovalGateGetArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .get(api_url(
            &args.api_url,
            &format!("/api/approval-gates/{}", args.gate_id),
        ))
        .send()
        .await
        .context("failed to fetch approval gate")?
        .error_for_status()
        .context("pharness API rejected approval gate fetch")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode approval gate")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn decide_approval_gate(
    args: ApprovalGateDecisionArgs,
    decision: &str,
) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            &format!("/api/approval-gates/{}/{}", args.gate_id, decision),
        ))
        .json(&serde_json::json!({
            "decided_by": args.decided_by,
            "reason": args.reason,
        }))
        .send()
        .await
        .with_context(|| format!("failed to {decision} approval gate"))?
        .error_for_status()
        .with_context(|| format!("pharness API rejected approval gate {decision}"))?
        .json::<serde_json::Value>()
        .await
        .with_context(|| format!("failed to decode approval gate {decision}"))?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn create_permission_grant(args: PermissionGrantCreateArgs) -> anyhow::Result<()> {
    let http = api_client();
    let scope = parse_json_object(&args.scope_json, "scope-json")?;
    let response = http
        .post(api_url(&args.api_url, "/api/permission-grants"))
        .json(&CreatePermissionGrantRequest {
            subject: args.subject,
            created_by: args.created_by,
            reason: args.reason,
            scope,
            policy: serde_json::json!({
                "policy_mode": args.policy_mode,
            }),
            expires_at: args.expires_at,
        })
        .send()
        .await
        .context("failed to create permission grant")?
        .error_for_status()
        .context("pharness API rejected permission grant create")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode permission grant")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn get_permission_grant(args: PermissionGrantGetArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .get(api_url(
            &args.api_url,
            &format!("/api/permission-grants/{}", args.grant_id),
        ))
        .send()
        .await
        .context("failed to fetch permission grant")?
        .error_for_status()
        .context("pharness API rejected permission grant fetch")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode permission grant")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn revoke_permission_grant(args: PermissionGrantRevokeArgs) -> anyhow::Result<()> {
    let http = api_client();
    let response = http
        .post(api_url(
            &args.api_url,
            &format!("/api/permission-grants/{}/revoke", args.grant_id),
        ))
        .json(&RevokePermissionGrantRequest {
            revoked_by: args.revoked_by,
            reason: args.reason,
        })
        .send()
        .await
        .context("failed to revoke permission grant")?
        .error_for_status()
        .context("pharness API rejected permission grant revoke")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode permission grant revoke")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn list_audit_events(args: AuditEventListArgs) -> anyhow::Result<()> {
    let http = api_client();
    let mut query = vec![("limit".to_string(), args.limit.to_string())];
    if let Some(resource_kind) = args.resource_kind {
        query.push(("resource_kind".to_string(), resource_kind));
    }
    if let Some(resource_id) = args.resource_id {
        query.push(("resource_id".to_string(), resource_id));
    }
    if let Some(run_id) = args.run_id {
        query.push(("run_id".to_string(), run_id));
    }

    let response = http
        .get(api_url(&args.api_url, "/api/audit-events"))
        .query(&query)
        .send()
        .await
        .context("failed to fetch audit events")?
        .error_for_status()
        .context("pharness API rejected audit event list")?
        .json::<serde_json::Value>()
        .await
        .context("failed to decode audit event list")?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

async fn run(args: RunArgs) -> anyhow::Result<()> {
    let http = api_client();
    let create_url = api_url(&args.api_url, "/api/runs");
    let scope = run_scope_from_args(&args);
    let run = http
        .post(create_url)
        .json(&CreateRunRequest {
            task: args.task,
            cwd: args.cwd,
            max_turns: Some(args.max_turns),
            policy_mode: args.policy_mode,
            scope,
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

fn run_scope_from_args(args: &RunArgs) -> Option<RunScope> {
    let scope = RunScope {
        namespace: args.namespace.clone(),
        repo: args.repo.clone(),
        branch: args.branch.clone(),
        work_plan_id: args.work_plan_id.clone(),
        change_set_id: args.change_set_id.clone(),
        production_impacting: args.production_impacting,
    };
    (!scope.is_empty()).then_some(scope)
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

fn event_seq(event: &serde_json::Value) -> Option<u64> {
    event.get("seq").and_then(serde_json::Value::as_u64)
}

fn emit_sse_event_frames(buffer: &mut String) -> anyhow::Result<()> {
    while let Some(frame_end) = buffer.find("\n\n") {
        let frame = buffer[..frame_end].to_string();
        buffer.drain(..frame_end + 2);
        if let Some(event) = sse_frame_event(&frame)? {
            println!("{}", serde_json::to_string(&event)?);
            std::io::stdout().flush()?;
        }
    }

    Ok(())
}

fn sse_frame_event(frame: &str) -> anyhow::Result<Option<serde_json::Value>> {
    let data = frame
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim_start)
        .collect::<Vec<_>>();
    if data.is_empty() {
        return Ok(None);
    }

    serde_json::from_str(&data.join("\n"))
        .map(Some)
        .context("failed to decode SSE event data")
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
            if let Some(grant_id) = decision.get("grant_id").and_then(serde_json::Value::as_str) {
                fields.push(format!("grant_id={grant_id:?}"));
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

#[derive(Debug)]
struct ApprovalDecisionEndpoint {
    path: String,
    includes_decision: bool,
}

fn approval_decision_endpoint(
    args: &ApprovalDecisionArgs,
    decision: &str,
) -> anyhow::Result<ApprovalDecisionEndpoint> {
    match (&args.run_id, &args.approval_id) {
        (Some(run_id), None) => Ok(ApprovalDecisionEndpoint {
            path: format!("/api/runs/{run_id}/approvals"),
            includes_decision: true,
        }),
        (None, Some(approval_id)) => Ok(ApprovalDecisionEndpoint {
            path: format!("/api/approvals/{approval_id}/{decision}"),
            includes_decision: false,
        }),
        (Some(_), Some(_)) => bail!("pass only one of --run-id or --approval-id"),
        (None, None) => bail!("pass one of --run-id or --approval-id"),
    }
}

fn parse_json_object(value: &str, label: &str) -> anyhow::Result<serde_json::Value> {
    let parsed = serde_json::from_str::<serde_json::Value>(value)
        .with_context(|| format!("{label} must be valid JSON"))?;
    if parsed.is_object() {
        Ok(parsed)
    } else {
        bail!("{label} must be a JSON object")
    }
}

fn parse_optional_json_object(
    value: Option<String>,
    label: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    value
        .as_deref()
        .map(|value| parse_json_object(value, label))
        .transpose()
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
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_mode: Option<PolicyMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<RunScope>,
}

#[derive(Debug, Serialize)]
struct ApprovalReviewRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    decision: Option<&'a str>,
    decided_by: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreatePermissionGrantRequest {
    subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_by: Option<String>,
    reason: String,
    scope: serde_json::Value,
    policy: serde_json::Value,
    expires_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreateTrustedEnvelopeRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_by: Option<String>,
    reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    environment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    production_impacting: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct RevokePermissionGrantRequest {
    revoked_by: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunResponse {
    id: String,
    status: String,
    task: String,
    max_turns: u32,
    started_at: String,
    finished_at: Option<String>,
    cancel_requested_at: Option<String>,
    scope: Option<RunScope>,
    result: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
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

#[derive(Debug, Serialize)]
struct ConfigValidationOutput {
    status: &'static str,
    file: String,
    api: ConfigValidationApi,
    storage: ConfigValidationStorage,
    model: ConfigValidationModel,
    cluster: ConfigValidationCluster,
    policy: ConfigValidationPolicy,
}

impl ConfigValidationOutput {
    fn from_config(file: PathBuf, config: &ApiRuntimeConfig) -> Self {
        Self {
            status: "ok",
            file: file.display().to_string(),
            api: ConfigValidationApi {
                bind: config.api.bind.to_string(),
            },
            storage: ConfigValidationStorage {
                path: config.storage.path.display().to_string(),
            },
            model: ConfigValidationModel {
                provider: config.model.provider.clone(),
                model: config.model.model.clone(),
                base_url: config.model.base_url.clone(),
                api_key_env: config.model.api_key_env.clone(),
                api_key_configured: config.model.api_key.is_some(),
            },
            cluster: ConfigValidationCluster {
                kubectl_bin: config.cluster.kubectl_bin.clone(),
                argocd_namespace: config.cluster.argocd_namespace.clone(),
                prometheus_configured: config.cluster.prometheus_url.is_some(),
                loki_configured: config.cluster.loki_url.is_some(),
                registry_alias_count: config.cluster.registry_aliases.len(),
                timeout_ms: config.cluster.timeout_ms,
                max_output_bytes: config.cluster.max_output_bytes,
            },
            policy: ConfigValidationPolicy {
                subject: config.policy.subject.clone(),
                environment: config.policy.environment.clone(),
                mode: config.policy.mode.to_string(),
                allow_read_only_shell: config.policy.allow_read_only_shell,
                require_approval_for_writes: config.policy.require_approval_for_writes,
                require_approval_for_network: config.policy.require_approval_for_network,
                require_approval_for_destructive: config.policy.require_approval_for_destructive,
                deny_privileged: config.policy.deny_privileged,
                deny_secret_access: config.policy.deny_secret_access,
                permission_grant_count: config.policy.permission_grants.len(),
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct ConfigValidationApi {
    bind: String,
}

#[derive(Debug, Serialize)]
struct ConfigValidationStorage {
    path: String,
}

#[derive(Debug, Serialize)]
struct ConfigValidationModel {
    provider: String,
    model: String,
    base_url: String,
    api_key_env: String,
    api_key_configured: bool,
}

#[derive(Debug, Serialize)]
struct ConfigValidationCluster {
    kubectl_bin: String,
    argocd_namespace: String,
    prometheus_configured: bool,
    loki_configured: bool,
    registry_alias_count: usize,
    timeout_ms: u64,
    max_output_bytes: usize,
}

#[derive(Debug, Serialize)]
struct ConfigValidationPolicy {
    subject: String,
    environment: String,
    mode: String,
    allow_read_only_shell: bool,
    require_approval_for_writes: bool,
    require_approval_for_network: bool,
    require_approval_for_destructive: bool,
    deny_privileged: bool,
    deny_secret_access: bool,
    permission_grant_count: usize,
}

#[cfg(test)]
mod tests {
    use super::{
        api_url, approval_decision_endpoint, event_log_line, extract_model_summaries, is_terminal,
        parse_json_object, run_scope_from_args, ApprovalDecisionArgs, ApprovalGateCommand,
        CapabilityCommand, ChangeSetCommand, ConfigValidationOutput, DeploymentContractCommand,
        DeploymentIntentCommand, IncidentCommand, ObservationCommand, PipelineIntentCommand,
        RegistryEvidenceCommand, ReleaseCommand, RemediationPlanCommand, RunArgs, RunCommand,
        WorkPlanCommand,
    };
    use crate::{Cli, Command};
    use clap::Parser;
    use pharness_config::ApiRuntimeConfig;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn builds_api_urls_without_double_slashes() {
        assert_eq!(
            api_url("http://127.0.0.1:4777/", "/api/runs"),
            "http://127.0.0.1:4777/api/runs"
        );
    }

    #[test]
    fn parses_run_cancel_command() {
        let cli = Cli::try_parse_from([
            "pharness",
            "runs",
            "cancel",
            "--run-id",
            "run_1",
            "--with-events",
        ])
        .unwrap();

        match cli.command {
            Command::Runs {
                command: RunCommand::Cancel(args),
            } => {
                assert_eq!(args.run_id, "run_1");
                assert!(args.with_events);
            }
            _ => panic!("expected runs cancel command"),
        }
    }

    #[test]
    fn parses_run_events_stream_command() {
        let cli = Cli::try_parse_from([
            "pharness",
            "runs",
            "events",
            "--run-id",
            "run_1",
            "--after-seq",
            "4",
            "--stream",
            "--timeout-ms",
            "1000",
        ])
        .unwrap();

        match cli.command {
            Command::Runs {
                command: RunCommand::Events(args),
            } => {
                assert_eq!(args.run_id, "run_1");
                assert_eq!(args.after_seq, Some(4));
                assert!(args.stream);
                assert_eq!(args.timeout_ms, 1_000);
            }
            _ => panic!("expected runs events command"),
        }
    }

    #[test]
    fn parses_registry_inspect_image_capability_command() {
        let cli = Cli::try_parse_from([
            "pharness",
            "capabilities",
            "registry-inspect-image",
            "--image-ref",
            "registry.example.test/team/checkout-api:v1",
            "--registry-base-url",
            "https://registry.example.test",
            "--timeout-ms",
            "10000",
        ])
        .unwrap();

        match cli.command {
            Command::Capabilities {
                command: CapabilityCommand::RegistryInspectImage(args),
            } => {
                assert_eq!(args.image_ref, "registry.example.test/team/checkout-api:v1");
                assert_eq!(
                    args.registry_base_url.as_deref(),
                    Some("https://registry.example.test")
                );
                assert_eq!(args.timeout_ms, 10_000);
            }
            _ => panic!("expected registry-inspect-image capability command"),
        }
    }

    #[test]
    fn parses_observation_list_command() {
        let cli = Cli::try_parse_from([
            "pharness",
            "observations",
            "list",
            "--run-id",
            "run_1",
            "--source",
            "tekton",
            "--kind",
            "pipeline_run_analysis",
            "--resource-namespace",
            "ci",
            "--resource-kind",
            "PipelineRun",
            "--resource-name",
            "finance-build",
            "--limit",
            "5",
        ])
        .unwrap();

        match cli.command {
            Command::Observations {
                command: ObservationCommand::List(args),
            } => {
                assert_eq!(args.run_id.as_deref(), Some("run_1"));
                assert_eq!(args.source.as_deref(), Some("tekton"));
                assert_eq!(args.kind.as_deref(), Some("pipeline_run_analysis"));
                assert_eq!(args.resource_namespace.as_deref(), Some("ci"));
                assert_eq!(args.resource_kind.as_deref(), Some("PipelineRun"));
                assert_eq!(args.resource_name.as_deref(), Some("finance-build"));
                assert_eq!(args.limit, 5);
            }
            _ => panic!("expected observations list command"),
        }
    }

    #[test]
    fn parses_incident_list_command() {
        let cli = Cli::try_parse_from([
            "pharness",
            "incidents",
            "list",
            "--status",
            "candidate",
            "--severity",
            "high",
            "--resource-namespace",
            "ci",
            "--resource-kind",
            "PipelineRun",
            "--resource-name",
            "build-app",
            "--limit",
            "5",
        ])
        .unwrap();

        match cli.command {
            Command::Incidents {
                command: IncidentCommand::List(args),
            } => {
                assert_eq!(args.status.as_deref(), Some("candidate"));
                assert_eq!(args.severity.as_deref(), Some("high"));
                assert_eq!(args.resource_namespace.as_deref(), Some("ci"));
                assert_eq!(args.resource_kind.as_deref(), Some("PipelineRun"));
                assert_eq!(args.resource_name.as_deref(), Some("build-app"));
                assert_eq!(args.limit, 5);
            }
            _ => panic!("expected incidents list command"),
        }
    }

    #[test]
    fn parses_remediation_plan_list_command() {
        let cli = Cli::try_parse_from([
            "pharness",
            "remediation-plans",
            "list",
            "--incident-id",
            "inc_1",
            "--status",
            "draft",
            "--risk-level",
            "high",
            "--resource-namespace",
            "ci",
            "--resource-kind",
            "PipelineRun",
            "--resource-name",
            "build-app",
            "--limit",
            "5",
        ])
        .unwrap();

        match cli.command {
            Command::RemediationPlans {
                command: RemediationPlanCommand::List(args),
            } => {
                assert_eq!(args.incident_id.as_deref(), Some("inc_1"));
                assert_eq!(args.status.as_deref(), Some("draft"));
                assert_eq!(args.risk_level.as_deref(), Some("high"));
                assert_eq!(args.resource_namespace.as_deref(), Some("ci"));
                assert_eq!(args.resource_kind.as_deref(), Some("PipelineRun"));
                assert_eq!(args.resource_name.as_deref(), Some("build-app"));
                assert_eq!(args.limit, 5);
            }
            _ => panic!("expected remediation-plans list command"),
        }
    }

    #[test]
    fn parses_sdlc_root_create_commands() {
        let observation = Cli::try_parse_from([
            "pharness",
            "observations",
            "create",
            "--source",
            "smoke",
            "--kind",
            "pipeline_run_analysis",
            "--subject",
            "checkout-api",
            "--summary",
            "pipeline pending",
            "--resource-namespace",
            "apps-dev",
            "--resource-kind",
            "PipelineRun",
            "--resource-name",
            "pr-smoke",
            "--data-json",
            "{\"status\":\"running\"}",
        ])
        .unwrap();
        match observation.command {
            Command::Observations {
                command: ObservationCommand::Create(args),
            } => {
                assert_eq!(args.source, "smoke");
                assert_eq!(args.kind, "pipeline_run_analysis");
                assert_eq!(args.data_json.as_deref(), Some("{\"status\":\"running\"}"));
            }
            _ => panic!("expected observation create command"),
        }

        let incident = Cli::try_parse_from([
            "pharness",
            "incidents",
            "create",
            "--observation-id",
            "obs_1",
            "--severity",
            "medium",
            "--title",
            "Pipeline needs review",
            "--summary",
            "Pipeline is running",
        ])
        .unwrap();
        match incident.command {
            Command::Incidents {
                command: IncidentCommand::Create(args),
            } => {
                assert_eq!(args.observation_id, "obs_1");
                assert_eq!(args.severity, "medium");
            }
            _ => panic!("expected incident create command"),
        }

        let remediation_plan = Cli::try_parse_from([
            "pharness",
            "remediation-plans",
            "create",
            "--incident-id",
            "inc_1",
            "--title",
            "Review pipeline",
            "--summary",
            "Collect evidence",
            "--risk-level",
            "medium",
            "--plan-json",
            "{\"steps\":[\"inspect pipeline\"]}",
        ])
        .unwrap();
        match remediation_plan.command {
            Command::RemediationPlans {
                command: RemediationPlanCommand::Create(args),
            } => {
                assert_eq!(args.incident_id, "inc_1");
                assert_eq!(args.risk_level, "medium");
                assert!(args.requires_approval);
            }
            _ => panic!("expected remediation plan create command"),
        }
    }

    #[test]
    fn parses_work_plan_commands() {
        let list = Cli::try_parse_from([
            "pharness",
            "work-plans",
            "list",
            "--remediation-plan-id",
            "rplan_1",
            "--incident-id",
            "inc_1",
            "--status",
            "draft",
            "--risk-level",
            "high",
            "--resource-namespace",
            "ci",
            "--resource-kind",
            "PipelineRun",
            "--limit",
            "5",
        ])
        .unwrap();
        let get =
            Cli::try_parse_from(["pharness", "work-plans", "get", "--work-plan-id", "wplan_1"])
                .unwrap();
        let readiness = Cli::try_parse_from([
            "pharness",
            "work-plans",
            "readiness",
            "--work-plan-id",
            "wplan_1",
        ])
        .unwrap();
        let flow = Cli::try_parse_from([
            "pharness",
            "work-plans",
            "flow",
            "--work-plan-id",
            "wplan_1",
        ])
        .unwrap();
        let create = Cli::try_parse_from([
            "pharness",
            "work-plans",
            "create-from-remediation-plan",
            "--remediation-plan-id",
            "rplan_1",
        ])
        .unwrap();
        let envelope = Cli::try_parse_from([
            "pharness",
            "work-plans",
            "create-trusted-envelope",
            "--work-plan-id",
            "wplan_1",
            "--created-by",
            "lucas",
            "--reason",
            "bounded plan approved",
            "--namespace",
            "apps-dev",
            "--repo",
            "git@example.test/team/app.git",
            "--branch",
            "feature/pharness",
        ])
        .unwrap();

        match list.command {
            Command::WorkPlans {
                command: WorkPlanCommand::List(args),
            } => {
                assert_eq!(args.remediation_plan_id.as_deref(), Some("rplan_1"));
                assert_eq!(args.incident_id.as_deref(), Some("inc_1"));
                assert_eq!(args.status.as_deref(), Some("draft"));
                assert_eq!(args.risk_level.as_deref(), Some("high"));
                assert_eq!(args.resource_namespace.as_deref(), Some("ci"));
                assert_eq!(args.resource_kind.as_deref(), Some("PipelineRun"));
                assert_eq!(args.limit, 5);
            }
            _ => panic!("expected work-plans list command"),
        }
        match get.command {
            Command::WorkPlans {
                command: WorkPlanCommand::Get(args),
            } => assert_eq!(args.work_plan_id, "wplan_1"),
            _ => panic!("expected work-plans get command"),
        }
        match readiness.command {
            Command::WorkPlans {
                command: WorkPlanCommand::Readiness(args),
            } => assert_eq!(args.work_plan_id, "wplan_1"),
            _ => panic!("expected work-plans readiness command"),
        }
        match flow.command {
            Command::WorkPlans {
                command: WorkPlanCommand::Flow(args),
            } => assert_eq!(args.work_plan_id, "wplan_1"),
            _ => panic!("expected work-plans flow command"),
        }
        match create.command {
            Command::WorkPlans {
                command: WorkPlanCommand::CreateFromRemediationPlan(args),
            } => assert_eq!(args.remediation_plan_id, "rplan_1"),
            _ => panic!("expected work-plans create-from-remediation-plan command"),
        }
        match envelope.command {
            Command::WorkPlans {
                command: WorkPlanCommand::CreateTrustedEnvelope(args),
            } => {
                assert_eq!(args.work_plan_id, "wplan_1");
                assert_eq!(args.created_by.as_deref(), Some("lucas"));
                assert_eq!(args.namespace.as_deref(), Some("apps-dev"));
            }
            _ => panic!("expected work-plans create-trusted-envelope command"),
        }
    }

    #[test]
    fn parses_change_set_commands() {
        let list = Cli::try_parse_from([
            "pharness",
            "change-sets",
            "list",
            "--work-plan-id",
            "wplan_1",
            "--status",
            "draft",
            "--risk-level",
            "medium",
            "--limit",
            "5",
        ])
        .unwrap();
        let get = Cli::try_parse_from([
            "pharness",
            "change-sets",
            "get",
            "--change-set-id",
            "cset_1",
        ])
        .unwrap();
        let readiness = Cli::try_parse_from([
            "pharness",
            "change-sets",
            "readiness",
            "--change-set-id",
            "cset_1",
        ])
        .unwrap();
        let flow = Cli::try_parse_from([
            "pharness",
            "change-sets",
            "flow",
            "--change-set-id",
            "cset_1",
        ])
        .unwrap();
        let create = Cli::try_parse_from([
            "pharness",
            "change-sets",
            "create",
            "--work-plan-id",
            "wplan_1",
            "--change-set-json",
            r#"{"changes":[]}"#,
        ])
        .unwrap();
        let revise = Cli::try_parse_from([
            "pharness",
            "change-sets",
            "revise",
            "--change-set-id",
            "cset_1",
            "--change-set-json",
            r#"{"changes":[{"path":"README.md"}]}"#,
            "--material-change",
            "true",
        ])
        .unwrap();
        let transition = Cli::try_parse_from([
            "pharness",
            "change-sets",
            "transition",
            "--change-set-id",
            "cset_1",
            "--target-status",
            "proposed",
        ])
        .unwrap();
        let envelope = Cli::try_parse_from([
            "pharness",
            "change-sets",
            "create-trusted-envelope",
            "--change-set-id",
            "cset_1",
            "--created-by",
            "lucas",
            "--reason",
            "bounded source change approved",
            "--namespace",
            "apps-dev",
            "--repo",
            "git@example.test/team/app.git",
            "--branch",
            "feature/pharness",
        ])
        .unwrap();

        match list.command {
            Command::ChangeSets {
                command: ChangeSetCommand::List(args),
            } => {
                assert_eq!(args.work_plan_id.as_deref(), Some("wplan_1"));
                assert_eq!(args.status.as_deref(), Some("draft"));
                assert_eq!(args.risk_level.as_deref(), Some("medium"));
                assert_eq!(args.limit, 5);
            }
            _ => panic!("expected change-sets list command"),
        }
        match get.command {
            Command::ChangeSets {
                command: ChangeSetCommand::Get(args),
            } => assert_eq!(args.change_set_id, "cset_1"),
            _ => panic!("expected change-sets get command"),
        }
        match readiness.command {
            Command::ChangeSets {
                command: ChangeSetCommand::Readiness(args),
            } => assert_eq!(args.change_set_id, "cset_1"),
            _ => panic!("expected change-sets readiness command"),
        }
        match flow.command {
            Command::ChangeSets {
                command: ChangeSetCommand::Flow(args),
            } => assert_eq!(args.change_set_id, "cset_1"),
            _ => panic!("expected change-sets flow command"),
        }
        match create.command {
            Command::ChangeSets {
                command: ChangeSetCommand::Create(args),
            } => {
                assert_eq!(args.work_plan_id, "wplan_1");
                assert_eq!(args.change_set_json, r#"{"changes":[]}"#);
            }
            _ => panic!("expected change-sets create command"),
        }
        match revise.command {
            Command::ChangeSets {
                command: ChangeSetCommand::Revise(args),
            } => {
                assert_eq!(args.change_set_id, "cset_1");
                assert!(args.material_change);
            }
            _ => panic!("expected change-sets revise command"),
        }
        match transition.command {
            Command::ChangeSets {
                command: ChangeSetCommand::Transition(args),
            } => {
                assert_eq!(args.change_set_id, "cset_1");
                assert_eq!(args.target_status, "proposed");
            }
            _ => panic!("expected change-sets transition command"),
        }
        match envelope.command {
            Command::ChangeSets {
                command: ChangeSetCommand::CreateTrustedEnvelope(args),
            } => {
                assert_eq!(args.change_set_id, "cset_1");
                assert_eq!(args.created_by.as_deref(), Some("lucas"));
                assert_eq!(args.branch.as_deref(), Some("feature/pharness"));
            }
            _ => panic!("expected change-sets create-trusted-envelope command"),
        }
    }

    #[test]
    fn parses_pipeline_intent_commands() {
        let list = Cli::try_parse_from([
            "pharness",
            "pipeline-intents",
            "list",
            "--change-set-id",
            "cset_1",
            "--work-plan-id",
            "wplan_1",
            "--status",
            "proposed",
            "--intent-kind",
            "tekton_build_test_package",
            "--limit",
            "5",
        ])
        .unwrap();
        let get = Cli::try_parse_from([
            "pharness",
            "pipeline-intents",
            "get",
            "--pipeline-intent-id",
            "pint_1",
        ])
        .unwrap();
        let create = Cli::try_parse_from([
            "pharness",
            "pipeline-intents",
            "create-from-change-set",
            "--change-set-id",
            "cset_1",
            "--intent-kind",
            "tekton_build_test_package",
            "--intent-json",
            r#"{"execution":{"enabled":false}}"#,
            "--actor",
            "lucas",
        ])
        .unwrap();
        let transition = Cli::try_parse_from([
            "pharness",
            "pipeline-intents",
            "transition",
            "--pipeline-intent-id",
            "pint_1",
            "--target-status",
            "approved",
            "--actor",
            "lucas",
        ])
        .unwrap();
        let attach_evidence = Cli::try_parse_from([
            "pharness",
            "pipeline-intents",
            "attach-evidence",
            "--pipeline-intent-id",
            "pint_1",
            "--observation-id",
            "obs_1",
            "--actor",
            "lucas",
        ])
        .unwrap();
        let envelope = Cli::try_parse_from([
            "pharness",
            "pipeline-intents",
            "create-trusted-envelope",
            "--pipeline-intent-id",
            "pint_1",
            "--created-by",
            "lucas",
            "--reason",
            "bounded Tekton execution approved",
        ])
        .unwrap();
        let preview = Cli::try_parse_from([
            "pharness",
            "pipeline-intents",
            "execute",
            "--pipeline-intent-id",
            "pint_1",
        ])
        .unwrap();
        let apply = Cli::try_parse_from([
            "pharness",
            "pipeline-intents",
            "execute",
            "--pipeline-intent-id",
            "pint_1",
            "--apply",
        ])
        .unwrap();

        match list.command {
            Command::PipelineIntents {
                command: PipelineIntentCommand::List(args),
            } => {
                assert_eq!(args.change_set_id.as_deref(), Some("cset_1"));
                assert_eq!(args.work_plan_id.as_deref(), Some("wplan_1"));
                assert_eq!(args.status.as_deref(), Some("proposed"));
                assert_eq!(
                    args.intent_kind.as_deref(),
                    Some("tekton_build_test_package")
                );
                assert_eq!(args.limit, 5);
            }
            _ => panic!("expected pipeline-intents list command"),
        }
        match get.command {
            Command::PipelineIntents {
                command: PipelineIntentCommand::Get(args),
            } => assert_eq!(args.pipeline_intent_id, "pint_1"),
            _ => panic!("expected pipeline-intents get command"),
        }
        match create.command {
            Command::PipelineIntents {
                command: PipelineIntentCommand::CreateFromChangeSet(args),
            } => {
                assert_eq!(args.change_set_id, "cset_1");
                assert_eq!(
                    args.intent_kind.as_deref(),
                    Some("tekton_build_test_package")
                );
                assert_eq!(args.actor.as_deref(), Some("lucas"));
                assert_eq!(
                    args.intent_json.as_deref(),
                    Some(r#"{"execution":{"enabled":false}}"#)
                );
            }
            _ => panic!("expected pipeline-intents create-from-change-set command"),
        }
        match transition.command {
            Command::PipelineIntents {
                command: PipelineIntentCommand::Transition(args),
            } => {
                assert_eq!(args.pipeline_intent_id, "pint_1");
                assert_eq!(args.target_status, "approved");
                assert_eq!(args.actor.as_deref(), Some("lucas"));
            }
            _ => panic!("expected pipeline-intents transition command"),
        }
        match attach_evidence.command {
            Command::PipelineIntents {
                command: PipelineIntentCommand::AttachEvidence(args),
            } => {
                assert_eq!(args.pipeline_intent_id, "pint_1");
                assert_eq!(args.observation_id, "obs_1");
                assert_eq!(args.actor.as_deref(), Some("lucas"));
            }
            _ => panic!("expected pipeline-intents attach-evidence command"),
        }
        match envelope.command {
            Command::PipelineIntents {
                command: PipelineIntentCommand::CreateTrustedEnvelope(args),
            } => {
                assert_eq!(args.pipeline_intent_id, "pint_1");
                assert_eq!(args.created_by.as_deref(), Some("lucas"));
            }
            _ => panic!("expected pipeline-intents create-trusted-envelope command"),
        }
        match preview.command {
            Command::PipelineIntents {
                command: PipelineIntentCommand::Execute(args),
            } => assert!(!args.apply),
            _ => panic!("expected pipeline-intents execute command"),
        }
        match apply.command {
            Command::PipelineIntents {
                command: PipelineIntentCommand::Execute(args),
            } => assert!(args.apply),
            _ => panic!("expected pipeline-intents execute apply command"),
        }
    }

    #[test]
    fn parses_deployment_intent_commands() {
        let list = Cli::try_parse_from([
            "pharness",
            "deployment-intents",
            "list",
            "--pipeline-intent-id",
            "pint_1",
            "--status",
            "proposed",
            "--intent-kind",
            "argo_sync_deploy",
            "--target-environment",
            "dev",
            "--argo-application",
            "checkout-api",
            "--limit",
            "5",
        ])
        .unwrap();
        let get = Cli::try_parse_from([
            "pharness",
            "deployment-intents",
            "get",
            "--deployment-intent-id",
            "dint_1",
        ])
        .unwrap();
        let create = Cli::try_parse_from([
            "pharness",
            "deployment-intents",
            "create-from-pipeline-intent",
            "--pipeline-intent-id",
            "pint_1",
            "--target-environment",
            "dev",
            "--target-namespace",
            "apps-dev",
            "--argo-application",
            "checkout-api",
            "--actor",
            "lucas",
        ])
        .unwrap();
        let transition = Cli::try_parse_from([
            "pharness",
            "deployment-intents",
            "transition",
            "--deployment-intent-id",
            "dint_1",
            "--target-status",
            "approved",
            "--actor",
            "lucas",
        ])
        .unwrap();
        let attach_evidence = Cli::try_parse_from([
            "pharness",
            "deployment-intents",
            "attach-evidence",
            "--deployment-intent-id",
            "dint_1",
            "--observation-id",
            "obs_1",
            "--actor",
            "lucas",
        ])
        .unwrap();

        match list.command {
            Command::DeploymentIntents {
                command: DeploymentIntentCommand::List(args),
            } => {
                assert_eq!(args.pipeline_intent_id.as_deref(), Some("pint_1"));
                assert_eq!(args.status.as_deref(), Some("proposed"));
                assert_eq!(args.intent_kind.as_deref(), Some("argo_sync_deploy"));
                assert_eq!(args.target_environment.as_deref(), Some("dev"));
                assert_eq!(args.argo_application.as_deref(), Some("checkout-api"));
                assert_eq!(args.limit, 5);
            }
            _ => panic!("expected deployment-intents list command"),
        }
        match get.command {
            Command::DeploymentIntents {
                command: DeploymentIntentCommand::Get(args),
            } => assert_eq!(args.deployment_intent_id, "dint_1"),
            _ => panic!("expected deployment-intents get command"),
        }
        match create.command {
            Command::DeploymentIntents {
                command: DeploymentIntentCommand::CreateFromPipelineIntent(args),
            } => {
                assert_eq!(args.pipeline_intent_id, "pint_1");
                assert_eq!(args.target_environment.as_deref(), Some("dev"));
                assert_eq!(args.target_namespace.as_deref(), Some("apps-dev"));
                assert_eq!(args.argo_application.as_deref(), Some("checkout-api"));
                assert_eq!(args.actor.as_deref(), Some("lucas"));
            }
            _ => panic!("expected deployment-intents create-from-pipeline-intent command"),
        }
        match transition.command {
            Command::DeploymentIntents {
                command: DeploymentIntentCommand::Transition(args),
            } => {
                assert_eq!(args.deployment_intent_id, "dint_1");
                assert_eq!(args.target_status, "approved");
                assert_eq!(args.actor.as_deref(), Some("lucas"));
            }
            _ => panic!("expected deployment-intents transition command"),
        }
        match attach_evidence.command {
            Command::DeploymentIntents {
                command: DeploymentIntentCommand::AttachEvidence(args),
            } => {
                assert_eq!(args.deployment_intent_id, "dint_1");
                assert_eq!(args.observation_id, "obs_1");
                assert_eq!(args.actor.as_deref(), Some("lucas"));
            }
            _ => panic!("expected deployment-intents attach-evidence command"),
        }
    }

    #[test]
    fn parses_deployment_contract_commands() {
        let list = Cli::try_parse_from([
            "pharness",
            "deployment-contracts",
            "list",
            "--target-environment",
            "homelab",
            "--target-namespace",
            "pharness",
            "--argo-application",
            "pharness",
            "--status",
            "active",
        ])
        .unwrap();
        let get = Cli::try_parse_from([
            "pharness",
            "deployment-contracts",
            "get",
            "--deployment-contract-id",
            "dcontract_1",
        ])
        .unwrap();
        let create = Cli::try_parse_from([
            "pharness",
            "deployment-contracts",
            "create",
            "--target-environment",
            "homelab",
            "--target-namespace",
            "pharness",
            "--argo-application",
            "pharness",
            "--version",
            "v1",
            "--contract-json",
            r#"{\"operation\":\"sync\",\"prune\":false,\"force\":false}"#,
            "--actor",
            "lucas",
            "--reason",
            "reviewed target",
        ])
        .unwrap();
        let retire = Cli::try_parse_from([
            "pharness",
            "deployment-contracts",
            "retire",
            "--deployment-contract-id",
            "dcontract_1",
            "--actor",
            "lucas",
        ])
        .unwrap();

        match list.command {
            Command::DeploymentContracts {
                command: DeploymentContractCommand::List(args),
            } => {
                assert_eq!(args.target_environment.as_deref(), Some("homelab"));
                assert_eq!(args.target_namespace.as_deref(), Some("pharness"));
                assert_eq!(args.argo_application.as_deref(), Some("pharness"));
                assert_eq!(args.status.as_deref(), Some("active"));
            }
            _ => panic!("expected deployment-contracts list command"),
        }
        match get.command {
            Command::DeploymentContracts {
                command: DeploymentContractCommand::Get(args),
            } => assert_eq!(args.deployment_contract_id, "dcontract_1"),
            _ => panic!("expected deployment-contracts get command"),
        }
        match create.command {
            Command::DeploymentContracts {
                command: DeploymentContractCommand::Create(args),
            } => {
                assert_eq!(args.target_environment, "homelab");
                assert_eq!(args.target_namespace, "pharness");
                assert_eq!(args.argo_application, "pharness");
                assert_eq!(args.version, "v1");
                assert_eq!(args.actor.as_deref(), Some("lucas"));
                assert_eq!(args.reason.as_deref(), Some("reviewed target"));
            }
            _ => panic!("expected deployment-contracts create command"),
        }
        match retire.command {
            Command::DeploymentContracts {
                command: DeploymentContractCommand::Retire(args),
            } => {
                assert_eq!(args.deployment_contract_id, "dcontract_1");
                assert_eq!(args.actor.as_deref(), Some("lucas"));
            }
            _ => panic!("expected deployment-contracts retire command"),
        }
    }

    #[test]
    fn parses_release_commands() {
        let list = Cli::try_parse_from([
            "pharness",
            "releases",
            "list",
            "--deployment-intent-id",
            "dint_1",
            "--status",
            "proposed",
            "--release-kind",
            "gitops_release",
            "--target-environment",
            "dev",
            "--version",
            "v0.1.0",
            "--commit-sha",
            "abc1234",
            "--image-digest",
            "sha256:deadbeef",
            "--limit",
            "5",
        ])
        .unwrap();
        let get =
            Cli::try_parse_from(["pharness", "releases", "get", "--release-id", "rel_1"]).unwrap();
        let create = Cli::try_parse_from([
            "pharness",
            "releases",
            "create-from-deployment-intent",
            "--deployment-intent-id",
            "dint_1",
            "--version",
            "v0.1.0",
            "--commit-sha",
            "abc1234",
            "--image-digest",
            "sha256:deadbeef",
            "--actor",
            "lucas",
        ])
        .unwrap();
        let transition = Cli::try_parse_from([
            "pharness",
            "releases",
            "transition",
            "--release-id",
            "rel_1",
            "--target-status",
            "approved",
            "--actor",
            "lucas",
        ])
        .unwrap();
        let attach_evidence = Cli::try_parse_from([
            "pharness",
            "releases",
            "attach-evidence",
            "--release-id",
            "rel_1",
            "--observation-id",
            "obs_1",
            "--actor",
            "lucas",
        ])
        .unwrap();

        match list.command {
            Command::Releases {
                command: ReleaseCommand::List(args),
            } => {
                assert_eq!(args.deployment_intent_id.as_deref(), Some("dint_1"));
                assert_eq!(args.status.as_deref(), Some("proposed"));
                assert_eq!(args.release_kind.as_deref(), Some("gitops_release"));
                assert_eq!(args.target_environment.as_deref(), Some("dev"));
                assert_eq!(args.version.as_deref(), Some("v0.1.0"));
                assert_eq!(args.commit_sha.as_deref(), Some("abc1234"));
                assert_eq!(args.image_digest.as_deref(), Some("sha256:deadbeef"));
                assert_eq!(args.limit, 5);
            }
            _ => panic!("expected releases list command"),
        }
        match get.command {
            Command::Releases {
                command: ReleaseCommand::Get(args),
            } => assert_eq!(args.release_id, "rel_1"),
            _ => panic!("expected releases get command"),
        }
        match create.command {
            Command::Releases {
                command: ReleaseCommand::CreateFromDeploymentIntent(args),
            } => {
                assert_eq!(args.deployment_intent_id, "dint_1");
                assert_eq!(args.version.as_deref(), Some("v0.1.0"));
                assert_eq!(args.commit_sha.as_deref(), Some("abc1234"));
                assert_eq!(args.image_digest.as_deref(), Some("sha256:deadbeef"));
                assert_eq!(args.actor.as_deref(), Some("lucas"));
            }
            _ => panic!("expected releases create-from-deployment-intent command"),
        }
        match transition.command {
            Command::Releases {
                command: ReleaseCommand::Transition(args),
            } => {
                assert_eq!(args.release_id, "rel_1");
                assert_eq!(args.target_status, "approved");
                assert_eq!(args.actor.as_deref(), Some("lucas"));
            }
            _ => panic!("expected releases transition command"),
        }
        match attach_evidence.command {
            Command::Releases {
                command: ReleaseCommand::AttachEvidence(args),
            } => {
                assert_eq!(args.release_id, "rel_1");
                assert_eq!(args.observation_id, "obs_1");
                assert_eq!(args.actor.as_deref(), Some("lucas"));
            }
            _ => panic!("expected releases attach-evidence command"),
        }
    }

    #[test]
    fn parses_registry_evidence_commands() {
        let list = Cli::try_parse_from([
            "pharness",
            "registry-evidence",
            "list",
            "--release-id",
            "rel_1",
            "--status",
            "proposed",
            "--verification-status",
            "verified",
            "--registry",
            "registry.example.test",
            "--repository",
            "checkout-api",
            "--image-digest",
            "sha256:deadbeef",
            "--limit",
            "5",
        ])
        .unwrap();
        let get = Cli::try_parse_from([
            "pharness",
            "registry-evidence",
            "get",
            "--evidence-id",
            "regev_1",
        ])
        .unwrap();
        let create = Cli::try_parse_from([
            "pharness",
            "registry-evidence",
            "create-from-release",
            "--release-id",
            "rel_1",
            "--registry",
            "registry.example.test",
            "--repository",
            "checkout-api",
            "--image-digest",
            "sha256:deadbeef",
            "--verification-status",
            "verified",
            "--actor",
            "lucas",
        ])
        .unwrap();
        let create_from_inspection = Cli::try_parse_from([
            "pharness",
            "registry-evidence",
            "create-from-inspection",
            "--release-id",
            "rel_1",
            "--image-ref",
            "registry.example.test/checkout-api:v0.1.0",
            "--registry-base-url",
            "https://registry.example.test",
            "--actor",
            "lucas",
            "--timeout-ms",
            "5000",
        ])
        .unwrap();
        let transition = Cli::try_parse_from([
            "pharness",
            "registry-evidence",
            "transition",
            "--evidence-id",
            "regev_1",
            "--target-status",
            "verified",
            "--actor",
            "lucas",
        ])
        .unwrap();

        match list.command {
            Command::RegistryEvidence {
                command: RegistryEvidenceCommand::List(args),
            } => {
                assert_eq!(args.release_id.as_deref(), Some("rel_1"));
                assert_eq!(args.status.as_deref(), Some("proposed"));
                assert_eq!(args.verification_status.as_deref(), Some("verified"));
                assert_eq!(args.registry.as_deref(), Some("registry.example.test"));
                assert_eq!(args.repository.as_deref(), Some("checkout-api"));
                assert_eq!(args.image_digest.as_deref(), Some("sha256:deadbeef"));
                assert_eq!(args.limit, 5);
            }
            _ => panic!("expected registry-evidence list command"),
        }
        match get.command {
            Command::RegistryEvidence {
                command: RegistryEvidenceCommand::Get(args),
            } => assert_eq!(args.evidence_id, "regev_1"),
            _ => panic!("expected registry-evidence get command"),
        }
        match create.command {
            Command::RegistryEvidence {
                command: RegistryEvidenceCommand::CreateFromRelease(args),
            } => {
                assert_eq!(args.release_id, "rel_1");
                assert_eq!(args.registry.as_deref(), Some("registry.example.test"));
                assert_eq!(args.repository.as_deref(), Some("checkout-api"));
                assert_eq!(args.image_digest.as_deref(), Some("sha256:deadbeef"));
                assert_eq!(args.verification_status.as_deref(), Some("verified"));
                assert_eq!(args.actor.as_deref(), Some("lucas"));
            }
            _ => panic!("expected registry-evidence create-from-release command"),
        }
        match create_from_inspection.command {
            Command::RegistryEvidence {
                command: RegistryEvidenceCommand::CreateFromInspection(args),
            } => {
                assert_eq!(args.release_id, "rel_1");
                assert_eq!(args.image_ref, "registry.example.test/checkout-api:v0.1.0");
                assert_eq!(
                    args.registry_base_url.as_deref(),
                    Some("https://registry.example.test")
                );
                assert_eq!(args.actor.as_deref(), Some("lucas"));
                assert_eq!(args.timeout_ms, Some(5000));
            }
            _ => panic!("expected registry-evidence create-from-inspection command"),
        }
        match transition.command {
            Command::RegistryEvidence {
                command: RegistryEvidenceCommand::Transition(args),
            } => {
                assert_eq!(args.evidence_id, "regev_1");
                assert_eq!(args.target_status, "verified");
                assert_eq!(args.actor.as_deref(), Some("lucas"));
            }
            _ => panic!("expected registry-evidence transition command"),
        }
    }

    #[test]
    fn parses_approval_gate_list_command() {
        let cli = Cli::try_parse_from([
            "pharness",
            "approval-gates",
            "list",
            "--remediation-plan-id",
            "rplan_1",
            "--incident-id",
            "inc_1",
            "--status",
            "pending",
            "--gate-kind",
            "pipeline_mutation",
            "--risk-level",
            "high",
            "--resource-namespace",
            "ci",
            "--resource-kind",
            "PipelineRun",
            "--resource-name",
            "build-app",
            "--limit",
            "5",
        ])
        .unwrap();

        match cli.command {
            Command::ApprovalGates {
                command: ApprovalGateCommand::List(args),
            } => {
                assert_eq!(args.remediation_plan_id.as_deref(), Some("rplan_1"));
                assert_eq!(args.incident_id.as_deref(), Some("inc_1"));
                assert_eq!(args.status.as_deref(), Some("pending"));
                assert_eq!(args.gate_kind.as_deref(), Some("pipeline_mutation"));
                assert_eq!(args.risk_level.as_deref(), Some("high"));
                assert_eq!(args.resource_namespace.as_deref(), Some("ci"));
                assert_eq!(args.resource_kind.as_deref(), Some("PipelineRun"));
                assert_eq!(args.resource_name.as_deref(), Some("build-app"));
                assert_eq!(args.limit, 5);
            }
            _ => panic!("expected approval-gates list command"),
        }
    }

    #[test]
    fn parses_approval_gate_summary_command() {
        let cli = Cli::try_parse_from([
            "pharness",
            "approval-gates",
            "summary",
            "--incident-id",
            "inc_1",
            "--status",
            "pending",
            "--gate-kind",
            "pipeline_mutation",
            "--resource-namespace",
            "ci",
            "--resource-kind",
            "PipelineRun",
        ])
        .unwrap();

        match cli.command {
            Command::ApprovalGates {
                command: ApprovalGateCommand::Summary(args),
            } => {
                assert_eq!(args.incident_id.as_deref(), Some("inc_1"));
                assert_eq!(args.status, "pending");
                assert_eq!(args.gate_kind.as_deref(), Some("pipeline_mutation"));
                assert_eq!(args.resource_namespace.as_deref(), Some("ci"));
                assert_eq!(args.resource_kind.as_deref(), Some("PipelineRun"));
            }
            _ => panic!("expected approval-gates summary command"),
        }
    }

    #[test]
    fn parses_approval_gate_satisfy_command() {
        let cli = Cli::try_parse_from([
            "pharness",
            "approval-gates",
            "satisfy",
            "--gate-id",
            "agate_1",
            "--decided-by",
            "lucas",
            "--reason",
            "reviewed evidence",
        ])
        .unwrap();

        match cli.command {
            Command::ApprovalGates {
                command: ApprovalGateCommand::Satisfy(args),
            } => {
                assert_eq!(args.gate_id, "agate_1");
                assert_eq!(args.decided_by.as_deref(), Some("lucas"));
                assert_eq!(args.reason.as_deref(), Some("reviewed evidence"));
            }
            _ => panic!("expected approval-gates satisfy command"),
        }
    }

    #[test]
    fn builds_approval_decision_endpoints() {
        let by_run =
            approval_decision_endpoint(&approval_decision_args(Some("run_1"), None), "approve")
                .unwrap();
        let by_approval =
            approval_decision_endpoint(&approval_decision_args(None, Some("appr_1")), "deny")
                .unwrap();
        let both = approval_decision_endpoint(
            &approval_decision_args(Some("run_1"), Some("appr_1")),
            "approve",
        )
        .unwrap_err();

        assert_eq!(by_run.path, "/api/runs/run_1/approvals");
        assert!(by_run.includes_decision);
        assert_eq!(by_approval.path, "/api/approvals/appr_1/deny");
        assert!(!by_approval.includes_decision);
        assert!(both.to_string().contains("only one"));
    }

    #[test]
    fn detects_terminal_statuses() {
        assert!(is_terminal("completed"));
        assert!(is_terminal("approval_required"));
        assert!(!is_terminal("queued"));
        assert!(!is_terminal("running"));
    }

    fn approval_decision_args(
        run_id: Option<&str>,
        approval_id: Option<&str>,
    ) -> ApprovalDecisionArgs {
        ApprovalDecisionArgs {
            api_url: "http://127.0.0.1:4777".to_string(),
            run_id: run_id.map(str::to_string),
            approval_id: approval_id.map(str::to_string),
            decided_by: None,
            reason: None,
            wait: false,
            follow_events: false,
            poll_interval_ms: 500,
            timeout_ms: 300_000,
        }
    }

    #[test]
    fn builds_optional_run_scope_from_cli_args() {
        let empty = run_scope_from_args(&RunArgs {
            task: "inspect".to_string(),
            api_url: "http://127.0.0.1:4777".to_string(),
            cwd: None,
            max_turns: 40,
            policy_mode: None,
            namespace: None,
            repo: None,
            branch: None,
            work_plan_id: None,
            change_set_id: None,
            production_impacting: false,
            no_wait: false,
            follow_events: false,
            poll_interval_ms: 500,
            timeout_ms: 300_000,
        });
        let scoped = run_scope_from_args(&RunArgs {
            task: "inspect".to_string(),
            api_url: "http://127.0.0.1:4777".to_string(),
            cwd: None,
            max_turns: 40,
            policy_mode: None,
            namespace: Some("apps-dev".to_string()),
            repo: Some("git@example.test/team/app.git".to_string()),
            branch: Some("feature/pharness".to_string()),
            work_plan_id: Some("wplan_1".to_string()),
            change_set_id: Some("cset_1".to_string()),
            production_impacting: false,
            no_wait: false,
            follow_events: false,
            poll_interval_ms: 500,
            timeout_ms: 300_000,
        })
        .expect("scope should be present");

        assert!(empty.is_none());
        assert_eq!(scoped.namespace.as_deref(), Some("apps-dev"));
        assert_eq!(scoped.work_plan_id.as_deref(), Some("wplan_1"));
        assert_eq!(scoped.change_set_id.as_deref(), Some("cset_1"));
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

        let line = event_log_line(&serde_json::json!({
            "seq": 7,
            "type": "policy.evaluated",
            "payload": {
                "action": "write_file",
                "decision": {
                    "decision": "allow",
                    "risk": "medium",
                    "grant_id": "pgrant_local"
                }
            }
        }));

        assert_eq!(
            line,
            "[7] policy.evaluated action=\"write_file\" decision=\"allow\" grant_id=\"pgrant_local\""
        );
    }

    #[test]
    fn parses_json_objects_for_cli_payloads() {
        let parsed = parse_json_object(r#"{"environment":"local"}"#, "scope-json").unwrap();
        let error = parse_json_object(r#"["not-object"]"#, "scope-json")
            .err()
            .unwrap();

        assert_eq!(parsed["environment"], "local");
        assert!(error.to_string().contains("JSON object"));
    }

    #[test]
    fn config_validation_output_does_not_expose_secret_values() {
        let mut env = BTreeMap::new();
        env.insert("FIREWORKS_API_KEY".to_string(), "super-secret".to_string());
        let config = ApiRuntimeConfig::from_sources(None, &env).unwrap();
        let output =
            ConfigValidationOutput::from_config(PathBuf::from("config/pharness.toml"), &config);
        let json = serde_json::to_value(output).unwrap();

        assert_eq!(json["model"]["api_key_configured"], true);
        assert_eq!(json["model"]["api_key_env"], "FIREWORKS_API_KEY");
        assert_eq!(json["policy"]["mode"], "default");
        assert_eq!(json["policy"]["subject"], "agent:local-worker");
        assert_eq!(json["policy"]["environment"], "local");
        assert_eq!(json["policy"]["permission_grant_count"], 0);
        assert_eq!(json["policy"]["deny_secret_access"], true);
        assert_eq!(json["cluster"]["loki_configured"], false);
        assert!(!json.to_string().contains("super-secret"));
    }
}
