# Pharness Agent Harness Implementation Plan

Date: 2026-05-03

Working name: `pharness`

## Executive Summary

Build a local-first coding agent harness with a Rust core runtime, Rust API/CLI, minimal TypeScript UI, and Fireworks AI as the primary model provider. V1 runs on a developer machine against a local project directory, but the core abstractions should assume the primary long-term operating context is a Kubernetes cluster. V2 moves the same runtime into a homelab Kubernetes deployment with per-run workspace sandboxes. V3 makes Kubernetes-native app delivery, observability, build, deploy, database, and long-lived context capabilities first-class.

The product surface is intentionally small:

- Agent loop over explicit actions.
- File, shell, search, patch, and git tools.
- Human approval for writes, destructive commands, network access, and sensitive operations.
- Durable sessions and event replay.
- Minimal UI for prompt entry, live events, approvals, diffs, and status.
- No plugin marketplace, no integration ecosystem, no MCP in V1.

## Current Verified Baseline

Last updated: 2026-05-15

The current implementation has crossed the first useful smoke-test boundary:

- `POST /api/runs` queues a durable run.
- The local worker picks up the run when `FIREWORKS_API_KEY` is configured.
- Fireworks native tool calling is the default worker protocol.
- A real Fireworks run using `accounts/fireworks/models/kimi-k2p5` completed in two turns:
  - model proposed `list_dir`
  - policy allowed the read-only action
  - local filesystem tool returned structured entries
  - model proposed `finish`
  - final run result was structured JSON with `status: completed`
- Durable events recorded the full sequence: queued, started, model request/response, action proposed, policy evaluated, tool started/finished, final action, run finished.

This validates the machine-facing control-plane contract. The next implementation priority is not a chat UI. The next priority is approval decision/resume semantics so mutation tools can be safely exposed.

Current phase status:

- Phase 0: complete enough for current work.
- Phase 1: Fireworks baseline complete; retry/error polish remains.
- Phase 2: core loop baseline plus approval resume are implemented.
- Phase 3: read/list/search/write shell/git baseline exists; patch executor and durable diff artifacts remain.
- Phase 4: policy baseline, approval decision API, and reviewed-action replay are implemented.
- Phase 5: durable run/event/result storage exists; file-change diff persistence and retrieval are implemented. General artifact/resource retrieval remains.
- Phase 6: machine-facing `run`, config, Fireworks model-listing, event-following, and approval decision CLI exists; session CLI and interactive approvals remain.
- Phase 7: core run API, approval listing, run-scoped approval decision route, SSE event streaming, and run diff retrieval exist; general artifact routes remain.
- Phase 8 and beyond: not started.

## Long-Term Cluster-Native North Star

The end goal is an autonomous coding workflow for production apps where the cluster already contains the systems needed for safe delivery:

- Docker/OCI registry for build outputs.
- Tekton build pipeline for repeatable builds/tests/image publication.
- Database operator for migrations, backups, restores, and database lifecycle actions.
- Argo CD as the GitOps deployment and reconciliation control plane.
- LGTM stack for logs, metrics, traces, dashboards, alerts, and run verification.
- Kubernetes API access with tightly scoped service accounts, NetworkPolicies, resource limits, and per-run workspaces.
- RAG store for longer-lived operational, codebase, incident, and architecture memory.

V1 should not implement these integrations directly. It should, however, avoid choices that would make them awkward later. Concretely:

- Treat "where a run executes" as an `ExecutionTarget`, not as an assumption that all tools run in the local OS process.
- Treat external systems as typed capabilities with policy and audit metadata, not as arbitrary shell commands.
- Treat every artifact as a durable reference that may be a local file, OCI image, Tekton run, Argo CD application, Kubernetes object, metric query, trace, log stream, backup, or RAG memory item.
- Treat GitOps as the default path for production changes: edit manifests/config in Git, let Argo reconcile, then verify cluster state.
- Make read-only cluster and observability inspection cheap, but require explicit approval for production-impacting writes, deploy syncs, database changes, registry mutations, secret access, and network exposure changes.
- Keep the event model rich enough to replay not just local tool calls, but a full build-deploy-observe loop.

## Kubernetes SDLC CRD Model

V3 should express the autonomous SDLC as Kubernetes-native custom resources. These CRDs are not V1 scope, but V1/V2 storage, events, resource references, approvals, and result JSON should map cleanly to them.

Core identity and execution CRDs:

- `Agent`: configured agent persona/runtime binding, provider defaults, policy profile, and allowed capability classes.
- `Skill`: bounded operating knowledge and procedure bundle, such as `lucas_engineering-gitops-diagnosis` or `rails-upgrade`.
- `ToolServer`: cluster-local service that exposes typed capabilities such as Kubernetes read, Argo read/sync, Tekton start/read, registry publish, database migration, LGTM query, or RAG search.
- `PermissionGrant`: time-scoped, audience-scoped authorization for an agent, skill, tool, namespace, environment, or action class.
- `Workspace`: per-run source checkout, mounted volume, object-store prefix, or ephemeral worktree with provenance and cleanup policy.

Work planning CRDs:

- `WorkItem`: user/request-level unit of work with priority, repo/app target, environment, and desired outcome.
- `WorkPlan`: agent-authored ordered plan with steps, expected tools, risk notes, rollback assumptions, and approval checkpoints.
- `ChangeSet`: proposed or applied source changes, diffs, generated artifacts, image tags/digests, and Git commit refs.

Build and delivery CRDs:

- `PipelineIntent`: desired build/test/package operation derived from a `WorkPlan` or `ChangeSet`.
- `PipelineRunAnalysis`: normalized Tekton/build result summary, failed steps, logs, test results, artifacts, and suggested next action.
- `DeploymentIntent`: desired GitOps/deployment action, target Argo app, environment, sync window, and blast-radius metadata.
- `Release`: promoted change with commit, image digest, deployment state, rollback pointer, and verification evidence.

Observe, incident, and remediation CRDs:

- `Observation`: structured read-only fact from Kubernetes, Argo CD, LGTM, database operator, registry, Tekton, or RAG.
- `Incident`: correlated failure or degradation with affected app/environment, signals, severity, and current owner.
- `RemediationPlan`: agent-authored plan to mitigate an `Incident`, including verification and rollback.

Governance CRDs:

- `ApprovalGate`: explicit pause point tied to a proposed action, risk level, environment, required approver, and decision.
- `AuditEvent`: append-only normalized record of model decisions, policy decisions, approvals, tool executions, resource reads/writes, and release events.

High-level ownership flow:

```text
WorkItem -> WorkPlan -> Workspace -> ChangeSet
ChangeSet -> PipelineIntent -> PipelineRunAnalysis
PipelineRunAnalysis -> DeploymentIntent -> Release
Observation -> Incident -> RemediationPlan -> WorkPlan
PermissionGrant + ApprovalGate + AuditEvent wrap every risky transition
Agent + Skill + ToolServer define who can act and how
```

Early design implications:

- Keep `Run`, `AgentEvent`, `ToolResult`, `ResourceRef`, and `ArtifactRef` generic enough to project into these CRDs later.
- Prefer typed capabilities over raw shell for anything that would become a `ToolServer` operation.
- Make policy decisions durable and machine-readable so they can become `ApprovalGate` and `AuditEvent` resources.
- Keep every delivery action connected to source provenance, target environment, and verification evidence.
- Treat direct production mutation as a future CRD-controlled workflow, not as a V1 shell shortcut.

## Research Notes

### Claude Code Public Workflow Patterns

Public docs describe Claude Code as an agentic coding tool that reads codebases, edits files, runs commands, and works from a terminal/project context. The key pattern to copy is not product shape, but the loop:

1. Gather context.
2. Take action with tools.
3. Verify results.
4. Repeat until done or interrupted.

Relevant public docs:

- [Claude Code overview](https://code.claude.com/docs): reads codebase, edits files, runs commands, works from terminal, supports git workflows.
- [How Claude Code works](https://code.claude.com/docs/en/how-claude-code-works): documents the gather context, take action, verify results agent loop.
- [Common workflows](https://code.claude.com/docs/en/common-workflows): shows project-root operation, codebase overview, relevant-file discovery, bug fixing, refactoring, tests, PR/git workflow, session resume, worktrees, and plan-before-editing.
- [Permissions](https://code.claude.com/docs/en/permissions): documents tiered permissions, read-only tool access, prompts for shell/file modification, permission modes, command matching, compound command handling, and read-only command classification.

Design takeaways:

- Start in the project root and treat the filesystem and shell as the agent's main environment.
- Prefer a small set of composable local tools over broad integrations.
- Make tool execution observable and interruptible.
- Keep read-only discovery cheap and low-friction.
- Require approval for mutation and riskier shell commands.
- Preserve/replay sessions so long-running tasks can continue.
- Make git status/diff first-class so users can inspect exactly what changed.

### Cursor SDK Comparison

Cursor's recent SDK and cloud-agent material is useful as a comparison point, not a dependency.

Relevant public docs/posts:

- [Cursor SDK changelog](https://cursor.com/changelog/sdk-release): SDK can run agents locally or on Cursor cloud, exposes run streaming, durable agents, per-prompt runs, run-scoped follow-ups/cancellation, lifecycle controls, SSE with reconnect via `Last-Event-ID`, and standardized response/error shapes.
- [Self-hosted cloud agents](https://cursor.com/blog/self-hosted-cloud-agents): self-hosted agents keep code/tool execution in the user's environment, use isolated development environments, and offer Kubernetes/operator-style scaling for larger deployments.
- [Cursor background agents](https://docs.cursor.com/en/background-agents): background agents run asynchronously in remote Ubuntu environments, clone repos, edit/run code, and allow status/follow-up/takeover.

Why not use Cursor SDK:

- This harness is Fireworks-first and local-first.
- Depending on Cursor's harness defeats the goal of owning the core runtime, tool execution, policy model, and provider abstraction.
- Cursor's SDK optimizes for their models/runtime and cloud/self-hosted worker model, while this project needs a small Rust runtime that can later run in a homelab cluster.

What to borrow:

- Durable `session` plus per-prompt `run` model.
- SSE event streaming with replay/reconnect.
- Run-scoped cancellation.
- Clear terminal run states.
- Self-hosted worker idea for V2, but implemented as our own Kubernetes worker pods.

### Fireworks AI API

Relevant public docs:

- [Create Chat Completion](https://docs.fireworks.ai/api-reference/post-chatcompletions): OpenAI-compatible `POST /v1/chat/completions` under `https://api.fireworks.ai/inference/v1/chat/completions`, messages, tools, `tool_choice`, `stream`, `response_format`, `max_tokens`, `reasoning_effort`, and prompt truncation controls.
- [Tool Calling](https://docs.fireworks.ai/guides/function-calling): supports OpenAI-compatible function/tool specs using JSON Schema, `tool_choice`, streaming tool call arguments, and troubleshooting advice for malformed arguments.
- [Rate Limits and Quotas](https://docs.fireworks.ai/guides/quotas_usage/rate-limits): serverless rate limits, adaptive token throughput, quota checks, and 429 behavior.
- [Completions API](https://docs.fireworks.ai/guides/completions-api): raw completions exist, but docs recommend chat completions for instruct/chat models.

Design takeaways:

- Use chat completions for V1.
- Prefer native Fireworks/OpenAI-compatible tool calling where the selected model supports tools.
- Stream model output and tool calls to the event bus.
- Use low temperature for action selection.
- Treat 429/5xx as retryable with exponential backoff and visible events.
- Make model capabilities explicit in config because tool-calling support is model-dependent.

### Model Interaction Recommendation

Use a dual-mode interaction layer:

1. Preferred: native Fireworks tool calling.
   - Register each supported agent action as an OpenAI-compatible function tool with JSON Schema.
   - Use required tool choice for the worker path once `respond` and `finish` are registered as tools. This keeps every model turn as one typed action and avoids prose or malformed JSON drifting into the loop.
   - Use temperature `0.0` to `0.2` for deterministic action selection.
   - Accumulate streamed tool-call argument deltas before validation.
   - Validate tool args with Rust `serde` plus JSON Schema before execution.
   - Preserve assistant tool-call history and attach matching `tool_call_id` values to tool-result messages before the next model turn.

2. Fallback: structured JSON action protocol.
   - Use Fireworks `response_format` with `json_schema` where supported.
   - Ask for exactly one action object per turn.
   - Validate against the same `AgentAction` enum schema as native tools.
   - If parsing fails, do one repair round: send the invalid payload and validation error back to the model with a strict "return only valid JSON" repair prompt.
   - If repair fails, emit `run.failed` or ask the user to continue in safe mode. Never execute malformed actions.

The 2026-05-15 smoke test confirmed why native tools should be the default: JSON action mode reached Fireworks but failed when the model returned a JSON object missing the top-level `action` field. Native required tool calls fixed the run path and produced a durable `list_dir` -> `finish` event sequence.

Do not build different business logic for these modes. Both should produce the same internal `AgentAction` type.

## Explicit Non-Goals

- No plugin marketplace.
- No third-party integration ecosystem.
- No MCP in V1.
- No remote execution in V1.
- No direct Kubernetes mutation tools in V1, except through explicit shell commands governed by policy.
- No V1 assumption that `kubectl`, `argocd`, `tkn`, `helm`, registry clients, or database CLIs are safe just because they are installed.
- No autonomous git push.
- No auto-committing without explicit user request and approval.
- No browser automation in V1.
- No multi-agent orchestration in V1.
- No cloud tenancy or hosted SaaS control plane.
- No arbitrary secret manager integration in V1.
- No model-specific prompt maze beyond a Fireworks-first provider and a stable provider trait.
- No production deployment autonomy until V3 has typed capabilities, scoped credentials, policy checks, audit events, rollback plans, and observability verification.
- No Kubernetes CRD controllers in V1. The CRD vocabulary is a future contract that shapes current schemas, not an early implementation target.

## Architecture Decisions

### Language Split

- Rust core runtime for predictable performance, low memory overhead, strong typed interfaces, and safe filesystem/process handling.
- Rust API server using `axum` and `tokio`.
- TypeScript UI using Vite, React, and a small component set.
- SQLite for V1 using `sqlx` with checked migrations.
- Optional Postgres for V2 behind the same store trait.

### Cluster-Native Design Guardrails

These are early design constraints, not V1 feature scope:

- Add an `ExecutionTarget` abstraction early, even if V1 only implements `LocalProcess`.
- Add stable `ResourceRef` and `ArtifactRef` types early, so future events can point at Kubernetes objects, OCI images, Tekton runs, Argo CD apps, dashboards, traces, logs, backups, and RAG memories without changing the event contract.
- Keep the tool registry closed and static in V1, but define tools with capability metadata: `filesystem`, `shell`, `git`, `kubernetes_read`, `kubernetes_write`, `registry`, `tekton`, `argocd`, `database`, `observability`, `rag`.
- Add policy dimensions for environment and blast radius: `local`, `dev`, `staging`, `production`, `cluster_admin`, `data_plane`, `control_plane`.
- Prefer typed future tools over shell wrappers for cluster operations. Shell remains useful, but `kubectl apply`, `argocd app sync`, `tkn pipeline start`, registry push, and database migration should eventually be first-class actions with structured args, preflight checks, and audit trails.
- Design the context packer as a set of context sources. V1 sources are local files/git/session history. V3 sources can include RAG, Kubernetes state, Argo CD app health, Tekton history, image metadata, database migration state, and LGTM queries.
- Treat Git as the source of truth for production app changes. Direct cluster mutation is a break-glass or read-only inspection path unless a typed capability explicitly marks the operation as safe.

### Crate Boundaries

Keep crate count low enough to reason about:

- `pharness-core`: provider traits, agent loop, actions, tool registry, tools, policy, context, events, redaction.
- `pharness-fireworks`: Fireworks chat client, streaming parser, tool-call conversion, retry/error mapping.
- `pharness-store`: SQLite/Postgres-neutral persistence trait plus SQLite implementation and migrations.
- `pharness-api`: HTTP/SSE API using core and store.
- `pharness-cli`: CLI entrypoint for local runs, serve, approvals, session inspection.

Future crates, introduced only when the phase needs them:

- `pharness-kube`: Kubernetes API client, worker/job control, typed Kubernetes resource refs, RBAC-aware preflight.
- `pharness-delivery`: registry, Tekton, Argo CD, and deployment workflow capabilities.
- `pharness-observe`: LGTM query clients and verification summaries.
- `pharness-rag`: long-lived context retrieval/writeback interface.

The UI is not a Rust crate.

### Runtime Shape

The agent runtime is a state machine, not a free-form framework.

Core states:

- `Created`
- `ContextBuilding`
- `ModelThinking`
- `ActionProposed`
- `ApprovalRequired`
- `ToolRunning`
- `ToolObserved`
- `Finishing`
- `Completed`
- `Failed`
- `Cancelled`

Every state transition emits an event and persists it before moving to the next state.

## Proposed Repo Structure

```text
pharness/
  Cargo.toml
  README.md
  prompt.md
  planning/
    agent-harness-implementation-plan.md
  crates/
    pharness-core/
      Cargo.toml
      src/
        lib.rs
        agent/
          mod.rs
          loop.rs
          state.rs
          turn.rs
          limits.rs
        execution/
          mod.rs
          target.rs
          environment.rs
        context/
          mod.rs
          packer.rs
          repo_scan.rs
          sources.rs
          token_budget.rs
        events/
          mod.rs
          bus.rs
          schema.rs
        model/
          mod.rs
          provider.rs
          request.rs
          response.rs
          action_protocol.rs
        policy/
          mod.rs
          command_classifier.rs
          path_policy.rs
          approval.rs
          redaction.rs
        tools/
          mod.rs
          registry.rs
          capability.rs
          fs.rs
          patch.rs
          shell.rs
          git.rs
          search.rs
        resources/
          mod.rs
          artifact_ref.rs
          resource_ref.rs
        workspace/
          mod.rs
          diff.rs
          ignore.rs
          paths.rs
        error.rs
    pharness-fireworks/
      Cargo.toml
      src/
        lib.rs
        client.rs
        types.rs
        stream.rs
        tools.rs
        errors.rs
    pharness-store/
      Cargo.toml
      migrations/
        0001_initial.sql
      src/
        lib.rs
        sqlite.rs
        postgres.rs
        models.rs
    pharness-api/
      Cargo.toml
      src/
        main.rs
        app.rs
        routes/
          mod.rs
          sessions.rs
          messages.rs
          runs.rs
          events.rs
          approvals.rs
          artifacts.rs
        sse.rs
        dto.rs
    pharness-cli/
      Cargo.toml
      src/
        main.rs
        commands/
          mod.rs
          init.rs
          run.rs
          serve.rs
          sessions.rs
          approvals.rs
    # Future V2/V3 crates. Do not scaffold in Phase 0 unless actively implementing them.
    pharness-kube/
    pharness-delivery/
    pharness-observe/
    pharness-rag/
  ui/
    package.json
    vite.config.ts
    tsconfig.json
    src/
      main.tsx
      api/client.ts
      api/events.ts
      app/App.tsx
      components/
        SessionList.tsx
        PromptBox.tsx
        RunStatus.tsx
        EventLog.tsx
        ApprovalCard.tsx
        DiffViewer.tsx
        ShellOutput.tsx
        # Future V3 components:
        ClusterResourcePanel.tsx
        DeliveryTimeline.tsx
        ObservabilityPanel.tsx
      styles.css
  config/
    pharness.example.toml
  deploy/
    docker/
      Dockerfile.runtime
      Dockerfile.ui
    k8s/
      base/
      overlays/
        homelab/
    helm/
      pharness/
```

## Core Interfaces

### Agent Action Schema

All model outputs normalize into this internal enum. Native tool calls and JSON action mode both produce this shape.

```json
{
  "id": "act_01hx...",
  "action": "run_shell",
  "reason": "Run the package test command after editing the parser.",
  "args": {
    "cmd": "cargo test -p pharness-core",
    "cwd": ".",
    "timeout_ms": 120000,
    "dry_run": false
  }
}
```

Supported actions:

```text
respond
read_file
write_file
patch_file
list_dir
search_files
run_shell
git_diff
git_status
request_approval
finish
```

Planned V3 action families are intentionally not V1 tools, but the internal schemas should leave room for them:

```text
kubernetes_get
kubernetes_describe
kubernetes_diff
kubernetes_apply_gitops_change
registry_build_image
registry_publish_image
tekton_start_pipeline
tekton_get_pipeline_runs
argocd_get_app
argocd_sync_app
argocd_rollback_app
database_plan_migration
database_apply_migration
database_backup
observability_query_logs
observability_query_metrics
observability_query_traces
rag_search
rag_write_memory
```

Those future actions should be typed tools with scoped credentials and policy checks, not opaque shell commands.

Rust shape:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AgentAction {
    Respond { id: ActionId, reason: String, message: String },
    ReadFile { id: ActionId, reason: String, path: Utf8PathBuf, max_bytes: Option<u64> },
    WriteFile { id: ActionId, reason: String, path: Utf8PathBuf, content: String },
    PatchFile { id: ActionId, reason: String, path: Utf8PathBuf, patch: String },
    ListDir { id: ActionId, reason: String, path: Utf8PathBuf, depth: u8 },
    SearchFiles { id: ActionId, reason: String, query: String, path: Option<Utf8PathBuf>, glob: Option<String> },
    RunShell { id: ActionId, reason: String, cmd: String, cwd: Option<Utf8PathBuf>, timeout_ms: Option<u64>, dry_run: bool },
    GitDiff { id: ActionId, reason: String, pathspec: Option<String> },
    GitStatus { id: ActionId, reason: String },
    RequestApproval { id: ActionId, reason: String, approval_kind: ApprovalKind, summary: String },
    Finish { id: ActionId, reason: String, summary: String, success: bool },
}
```

### Execution Target Schema

V1 implements only `local_process`, but the run model should carry a target from the start.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionTarget {
    LocalProcess {
        cwd: Utf8PathBuf,
        shell: String,
    },
    KubernetesJob {
        cluster: String,
        namespace: String,
        service_account: String,
        workspace: WorkspaceMount,
        network_profile: String,
        resource_profile: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentRef {
    pub id: String,
    pub name: String,
    pub tier: EnvironmentTier,
    pub cluster: Option<String>,
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentTier {
    Local,
    Dev,
    Staging,
    Production,
}
```

### Resource and Artifact References

Use generic references rather than path-only artifacts. This keeps local V1 artifacts and V3 cluster artifacts in the same event/session model.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRef {
    pub provider: String,       // local, git, kubernetes, tekton, argocd, registry, lgtm, database, rag
    pub kind: String,           // file, commit, deployment, pipeline_run, application, image, log_query, backup, memory
    pub name: String,
    pub namespace: Option<String>,
    pub uri: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub artifact_id: ArtifactId,
    pub kind: String,
    pub label: String,
    pub uri: Option<String>,
    pub resource_ref: Option<ResourceRef>,
}
```

### Tool Result Schema

```json
{
  "tool_call_id": "tc_01hx...",
  "action_id": "act_01hx...",
  "status": "ok",
  "summary": "3 tests passed",
  "content": {
    "stdout": "...truncated...",
    "stderr": "",
    "exit_code": 0,
    "truncated": false
  },
  "artifacts": [
    {
      "artifact_id": "art_01hx...",
      "kind": "shell_output",
      "path": null
    }
  ],
  "resource_refs": [
    {
      "provider": "local",
      "kind": "file",
      "name": "crates/pharness-core/src/agent/loop.rs",
      "namespace": null,
      "uri": "workspace://crates/pharness-core/src/agent/loop.rs",
      "metadata": {}
    }
  ]
}
```

### Provider Interface

```rust
#[async_trait::async_trait]
pub trait ModelProvider: Send + Sync {
    async fn complete_action(
        &self,
        request: ModelRequest,
        events: EventSink,
        cancellation: CancellationToken,
    ) -> Result<ModelTurn, ProviderError>;

    fn capabilities(&self) -> ModelCapabilities;
}

pub struct ModelRequest {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub messages: Vec<ModelMessage>,
    pub tools: Vec<ToolSpec>,
    pub mode: ToolProtocolMode,
    pub temperature: f32,
    pub max_tokens: u32,
}

pub enum ToolProtocolMode {
    NativeTools,
    JsonAction,
}

pub struct ModelTurn {
    pub raw_provider_id: Option<String>,
    pub assistant_message: Option<String>,
    pub action: AgentAction,
    pub usage: Option<TokenUsage>,
}
```

### Fireworks Client Interface

```rust
pub struct FireworksClient {
    http: reqwest::Client,
    api_key: SecretString,
    base_url: Url,
    model: String,
    retry: RetryPolicy,
}

impl FireworksClient {
    pub async fn chat_stream(
        &self,
        request: FireworksChatRequest,
    ) -> Result<impl Stream<Item = Result<FireworksStreamEvent, FireworksError>>, FireworksError>;
}
```

Default endpoint:

```text
https://api.fireworks.ai/inference/v1/chat/completions
```

### Event Schema

Persist every event and stream the same shape over SSE.

```json
{
  "event_id": "evt_01hx...",
  "session_id": "ses_01hx...",
  "run_id": "run_01hx...",
  "seq": 42,
  "ts": "2026-05-03T11:15:30.123Z",
  "type": "tool.finished",
  "payload": {
    "tool_call_id": "tc_01hx...",
    "status": "ok",
    "summary": "cargo test passed",
    "duration_ms": 8321
  }
}
```

Event types:

```text
session.created
run.started
run.cancel_requested
run.cancelled
run.failed
run.finished
context.scan_started
context.scan_finished
context.packed
model.request_started
model.delta
model.tool_call_delta
model.response_finished
action.proposed
policy.evaluated
approval.required
approval.decided
tool.started
tool.output
tool.finished
artifact.created
diff.created
redaction.applied
resource.observed
resource.changed
delivery.build_started
delivery.build_finished
delivery.deploy_started
delivery.deploy_finished
observability.query_started
observability.query_finished
rag.context_loaded
rag.memory_written
```

SSE format:

```text
id: evt_01hx...
event: tool.finished
data: {"event_id":"evt_01hx...","session_id":"ses_...","run_id":"run_...","seq":42,"ts":"...","type":"tool.finished","payload":{...}}
```

### API Endpoint Sketch

V1 local API:

```text
GET    /health
GET    /api/config/effective
POST   /api/capabilities/execute

POST   /api/sessions
GET    /api/sessions
GET    /api/sessions/:session_id
PATCH  /api/sessions/:session_id
DELETE /api/sessions/:session_id

POST   /api/sessions/:session_id/messages
GET    /api/sessions/:session_id/messages

POST   /api/sessions/:session_id/runs
GET    /api/sessions/:session_id/runs
GET    /api/runs/:run_id
POST   /api/runs/:run_id/cancel

GET    /api/runs/:run_id/events
GET    /api/runs/:run_id/events/stream

POST   /api/runs/:run_id/approvals

GET    /api/approvals
GET    /api/approvals/:approval_id
POST   /api/approvals/:approval_id/approve
POST   /api/approvals/:approval_id/deny

GET    /api/artifacts/:artifact_id
GET    /api/runs/:run_id/artifacts
GET    /api/runs/:run_id/diff
```

V2/V3 API additions should extend this shape instead of replacing it:

```text
GET    /api/environments
GET    /api/environments/:environment_id
GET    /api/runs/:run_id/resources
GET    /api/resources/:resource_id

GET    /api/capabilities
GET    /api/capabilities/:capability/status

POST   /api/runs/:run_id/context/rag/search
POST   /api/runs/:run_id/context/rag/memories

GET    /api/delivery/builds/:build_id
GET    /api/delivery/deployments/:deployment_id
GET    /api/observability/queries/:query_id
```

Keep these future endpoints resource-oriented. Do not expose raw "run arbitrary cluster command" APIs as the main interface.

SSE replay:

- Client sends `Last-Event-ID`.
- Server resumes after that event if available.
- If event is too old or unknown, server sends a snapshot event followed by live events.

### Session Database Schema

SQLite V1. Use WAL mode. Store JSON payloads as text with typed columns for queryable state.

```sql
CREATE TABLE sessions (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  cwd TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE runs (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  environment_id TEXT,
  status TEXT NOT NULL,
  user_task TEXT NOT NULL,
  max_turns INTEGER NOT NULL,
  started_at TEXT NOT NULL,
  finished_at TEXT,
  cancel_requested_at TEXT,
  error TEXT,
  execution_target_json TEXT NOT NULL DEFAULT '{"kind":"local_process"}',
  metadata_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE messages (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT REFERENCES runs(id),
  role TEXT NOT NULL,
  content TEXT NOT NULL,
  created_at TEXT NOT NULL,
  token_estimate INTEGER,
  metadata_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE events (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT REFERENCES runs(id),
  seq INTEGER NOT NULL,
  type TEXT NOT NULL,
  ts TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  UNIQUE(run_id, seq)
);

CREATE TABLE tool_calls (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT NOT NULL REFERENCES runs(id),
  action_id TEXT NOT NULL,
  action_type TEXT NOT NULL,
  status TEXT NOT NULL,
  approval_id TEXT,
  proposed_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  args_json TEXT NOT NULL,
  result_json TEXT,
  policy_json TEXT NOT NULL
);

CREATE TABLE approvals (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT NOT NULL REFERENCES runs(id),
  tool_call_id TEXT REFERENCES tool_calls(id),
  status TEXT NOT NULL,
  kind TEXT NOT NULL,
  summary TEXT NOT NULL,
  risk_level TEXT NOT NULL,
  requested_at TEXT NOT NULL,
  decided_at TEXT,
  decided_by TEXT,
  decision_reason TEXT,
  action_json TEXT,
  resume_messages_json TEXT,
  turns_completed INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE artifacts (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT REFERENCES runs(id),
  kind TEXT NOT NULL,
  label TEXT NOT NULL,
  mime_type TEXT,
  path TEXT,
  content_text TEXT,
  content_json TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE file_changes (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT NOT NULL REFERENCES runs(id),
  tool_call_id TEXT REFERENCES tool_calls(id),
  path TEXT NOT NULL,
  before_hash TEXT,
  after_hash TEXT,
  diff TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE environments (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  tier TEXT NOT NULL,
  cluster TEXT,
  namespace TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE resource_refs (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT REFERENCES runs(id),
  provider TEXT NOT NULL,
  kind TEXT NOT NULL,
  name TEXT NOT NULL,
  namespace TEXT,
  uri TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  observed_at TEXT NOT NULL
);

CREATE TABLE context_items (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  run_id TEXT REFERENCES runs(id),
  source TEXT NOT NULL,
  kind TEXT NOT NULL,
  title TEXT NOT NULL,
  content TEXT NOT NULL,
  resource_ref_id TEXT REFERENCES resource_refs(id),
  token_estimate INTEGER,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);
```

Indexes:

```sql
CREATE INDEX idx_runs_session_started ON runs(session_id, started_at DESC);
CREATE INDEX idx_events_run_seq ON events(run_id, seq);
CREATE INDEX idx_approvals_status ON approvals(status, requested_at DESC);
CREATE INDEX idx_artifacts_run ON artifacts(run_id, created_at DESC);
CREATE INDEX idx_resource_refs_run ON resource_refs(run_id, provider, kind);
CREATE INDEX idx_context_items_session ON context_items(session_id, created_at DESC);
```

## Context Packer

V1 context packing should be deliberately simple:

1. Always include:
   - System prompt.
   - Current user task.
   - Effective config summary.
   - Working directory.
   - Git status summary if repo exists.
   - Repo file tree summary using ignore rules.
   - Recent messages/events from current run.
   - Recent tool results, summarized.
2. Include file contents only after the agent has explicitly read them.
3. Include diffs after writes/patches.
4. Truncate shell output with head/tail preservation.
5. Estimate tokens with a fast approximation first. Add real tokenizer later only if needed.

Represent each context contributor as a `ContextSource`:

```rust
#[async_trait::async_trait]
pub trait ContextSource: Send + Sync {
    fn name(&self) -> &'static str;
    async fn collect(&self, request: ContextRequest) -> Result<Vec<ContextItem>, ContextError>;
}
```

V1 context sources:

- `SessionSummarySource`
- `RepoTreeSource`
- `GitStatusSource`
- `ReadFilesSource`
- `RecentToolResultsSource`
- `DiffSource`

V3 context sources:

- `RagMemorySource`
- `KubernetesStateSource`
- `ArgoAppHealthSource`
- `TektonRunHistorySource`
- `RegistryImageSource`
- `DatabaseStateSource`
- `LgtmSignalSource`

Suggested budgets:

```text
system prompt: 2k tokens
task and session summary: 2k
repo overview: 4k
recent messages: 8k
tool results: 8k
read files: configurable, default 48k
diffs: 12k
reserve for model output: 4k to 8k
```

When over budget:

1. Drop older shell output first.
2. Summarize older tool results.
3. Keep file paths and short summaries.
4. Keep current diffs and pending approvals.
5. Never drop system prompt, current task, pending approval, or current error.

For cluster-native runs, also never drop:

- Target environment and namespace.
- Pending approval details.
- Current GitOps diff.
- Current build/deploy identifier.
- Current Argo CD application health.
- Current database migration/backup status.
- Current production-impacting risk assessment.

## Safety Model

### Command Classes

```rust
pub enum CommandClass {
    SafeReadOnly,
    WriteLocalProject,
    DestructiveLocal,
    Network,
    Privileged,
    SecretAccessing,
    Unknown,
}
```

Classifier inputs:

- Parsed command words.
- Shell operators.
- Current working directory.
- Environment variables requested.
- Path arguments.
- Known risky binaries and flags.

Default policy:

```text
SafeReadOnly: allow
WriteLocalProject: ask unless trusted mode
DestructiveLocal: ask always
Network: ask always
Privileged: deny by default
SecretAccessing: deny by default or ask with high-risk warning
Unknown: ask
```

Read-only examples:

```text
ls
pwd
cat non-sensitive-file
head
tail
rg
grep
find without -delete/-exec
git status
git diff
git log
cargo test --no-run? ask, because it writes build artifacts
```

Privileged or denied examples:

```text
sudo
su
chmod -R 777 /
rm -rf /
rm -rf ~
security find-generic-password
op read
pass show
kubectl get secret -o yaml
cat ~/.ssh/id_rsa
cat .env
```

Network examples:

```text
curl
wget
git fetch
git pull
git push
npm install
cargo install
kubectl apply
kubectl delete
docker pull
docker push
crane push
oras push
argocd app sync
tkn pipeline start
helm upgrade
```

### Future Cluster Policy Dimensions

V1 classifies shell commands. V3 should classify typed capability calls with more dimensions:

```rust
pub struct PolicySubject {
    pub actor: String,
    pub session_id: SessionId,
    pub run_id: RunId,
}

pub struct PolicyResource {
    pub environment: EnvironmentRef,
    pub resource: Option<ResourceRef>,
    pub capability: CapabilityKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    Filesystem,
    Shell,
    Git,
    KubernetesRead,
    KubernetesWrite,
    RegistryRead,
    RegistryWrite,
    TektonRead,
    TektonStartRun,
    ArgoRead,
    ArgoSync,
    DatabaseRead,
    DatabaseBackup,
    DatabaseMigration,
    ObservabilityRead,
    RagRead,
    RagWrite,
}
```

Default V3 posture:

```text
Cluster read in dev/staging: allow if service account permits
Cluster read in production: allow for non-secret resources, log audit event
Registry read: allow
Registry write: ask
Tekton start run: ask unless explicitly trusted for dev/staging
Argo app health read: allow
Argo sync: ask always, require Git revision and rollback note
Kubernetes direct write: ask always, deny in production unless break-glass enabled
Database backup: ask
Database migration: ask always, require backup/preflight/rollback plan
Secret read: deny by default
Production traffic exposure change: ask always, high risk
Namespace/RBAC/CRD changes: ask always, high or critical risk
```

Typed cluster tools should produce preflight summaries before approval:

- Target cluster, namespace, app, and environment tier.
- Git commit or manifest diff being applied.
- Resources that will be created, updated, or deleted.
- Expected Argo CD sync result.
- Build image digest, not just mutable tag.
- Database migration plan and backup reference.
- Observability queries that will verify success.

### Secret Detection

Sensitive path patterns:

```text
.env
.env.*
*.pem
*.key
id_rsa
id_ed25519
known_hosts
*.kube/config
kubeconfig*
.npmrc
.pypirc
.netrc
credentials
token
secret
```

Redaction:

- Redact provider API keys, bearer tokens, private key blocks, kubeconfig tokens/certs, AWS/GCP/Azure keys, GitHub tokens, npm tokens.
- Redact before storing logs/events.
- Store a redaction event with counts and patterns, not raw secret values.

Path safety:

- Canonicalize paths.
- Require all write paths to stay inside workspace root unless explicitly added as allowed directories.
- Check symlink target and symlink path.
- Deny writes to `.git` by default unless the command is a known git command and approved.

## Example System Prompt

```text
You are Pharness, a local-first coding agent running inside a developer's project.

You operate by choosing one explicit action at a time. You can inspect files, search the project,
run approved shell commands, propose patches, and finish with a concise summary.

Rules:
- Work from the configured workspace root.
- Prefer reading and searching before editing.
- Use the smallest action that advances the task.
- Do not invent file contents. Read files before modifying them unless creating a new file.
- Do not access secrets, credentials, .env files, kubeconfigs, SSH keys, or token files.
- Do not run network, destructive, privileged, or secret-accessing commands unless the policy layer grants approval.
- When a command may modify files or external state, explain why it is needed.
- After edits, inspect the diff and run the most relevant local verification command when allowed.
- For Kubernetes-backed production apps, prefer GitOps changes over direct cluster mutation.
- Before any production-impacting action, identify the target environment, expected blast radius, rollback path, and verification signal.
- Treat registry writes, deploy syncs, database migrations, secret access, and network exposure changes as approval-required operations.
- If blocked by approval, request approval with a clear risk summary.
- Finish only when the task is complete, blocked, or further action would be unsafe.

Return exactly one action per turn.
```

## Example Config File

`config/pharness.example.toml`:

```toml
[workspace]
root = "."
respect_gitignore = true
additional_read_dirs = []
additional_write_dirs = []

[model]
provider = "fireworks"
model = "accounts/fireworks/models/kimi-k2p5"
api_key_env = "FIREWORKS_API_KEY"
base_url = "https://api.fireworks.ai/inference/v1"
temperature = 0.1
max_tokens = 4096
tool_protocol = "native_tools" # native_tools | json_action
reasoning_effort = "medium"

[agent]
max_turns = 40
max_context_tokens = 96000
shell_output_max_bytes = 65536
file_read_max_bytes = 262144

[execution]
target = "local_process" # local_process | kubernetes_job
environment = "local"

[cluster]
# Future V2/V3 fields. Ignored by V1 unless explicitly enabled.
name = "homelab"
namespace = "pharness"
service_account = "pharness-worker"
gitops_source_of_truth = true
default_delivery_mode = "gitops" # gitops | direct_apply_break_glass

[rag]
enabled = false
endpoint_env = "PHARNESS_RAG_ENDPOINT"
collection = "pharness-memory"

[policy]
mode = "default" # default | trusted_writes | plan | deny_all_writes
allow_read_only_shell = true
require_approval_for_writes = true
require_approval_for_network = true
require_approval_for_destructive = true
deny_privileged = true
deny_secret_access = true
command_timeout_ms = 120000

[policy.cluster]
allow_read_only_cluster_inspection = false
require_approval_for_registry_write = true
require_approval_for_tekton_run = true
require_approval_for_argocd_sync = true
require_approval_for_database_change = true
deny_secret_reads = true
deny_direct_production_apply = true

[storage]
kind = "sqlite"
path = "~/.local/share/pharness/pharness.db"

[api]
bind = "127.0.0.1:4777"
cors_origin = "http://127.0.0.1:5173"
```

## Example User Flow

```text
cd ./project
pharness init
export FIREWORKS_API_KEY=...
pharness run "Add tests for the config parser and fix any failures"
```

Flow:

1. CLI creates a session and run.
2. Runtime scans repo tree and git status.
3. Model requests `search_files` for config parser.
4. Runtime allows read-only search and emits events.
5. Model requests `read_file` for parser and tests.
6. Runtime reads files and redacts sensitive content if needed.
7. Model proposes `patch_file`.
8. Policy pauses for approval and UI/CLI shows diff preview.
9. User approves.
10. Runtime applies patch and stores file diff.
11. Model requests `run_shell` with test command.
12. Policy classifies as write local project or unknown because tests can write build artifacts, asks approval.
13. User approves once.
14. Runtime runs command with timeout and truncation.
15. Model iterates on failures or finishes.
16. CLI shows final summary and points to session id.

Future V3 cluster-native flow:

1. User asks: "Fix the checkout bug in the production app and ship it if verification passes."
2. Agent inspects repo, recent incidents/RAG notes, Argo CD app health, and LGTM signals.
3. Agent edits code and tests locally/in workspace.
4. Agent starts a Tekton pipeline for build/test/image publication after approval.
5. Tekton produces an immutable image digest and build provenance artifact.
6. Agent updates GitOps manifests to the new digest and opens or stages the change.
7. Agent requests approval for production sync with diff, digest, target app, rollback plan, and verification queries.
8. Argo CD sync applies the Git-backed change.
9. Agent watches rollout, pods, events, logs, metrics, and traces.
10. Agent finishes with commit/digest/deployment status, verification evidence, and any RAG memory worth retaining.

## Multi-Phase Implementation Plan

### Phase 0: Architecture Decisions and Repo Setup

Goal:

Create the Rust workspace, basic project docs, config example, and development commands.

Implementation steps:

1. Add root `Cargo.toml` workspace with the five crates.
2. Add `rust-toolchain.toml` pinned to stable.
3. Add `README.md` with V1 scope, safety defaults, and quickstart placeholder.
4. Add `config/pharness.example.toml`.
5. Add basic CI-friendly commands to README:
   - `cargo fmt --all -- --check`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace`
6. Add crate skeletons with `lib.rs`/`main.rs`.
7. Add core type skeletons for future cluster-native operation without implementing cluster tools:
   - `ExecutionTarget`
   - `EnvironmentRef`
   - `EnvironmentTier`
   - `ResourceRef`
   - `ArtifactRef`
   - `CapabilityKind`
8. Add an architecture decision record: "V1 local runtime, V3 cluster-native workflow."
9. Decide crate dependencies:
   - async runtime: `tokio`
   - HTTP: `reqwest`, `axum`
   - serialization: `serde`, `serde_json`
   - errors: `thiserror`, `anyhow` only in binaries
   - paths: `camino`
   - CLI: `clap`
   - SQLite: `sqlx`
   - events: `tokio::sync::broadcast`

Suggested files/modules:

- `Cargo.toml`
- `rust-toolchain.toml`
- `README.md`
- `config/pharness.example.toml`
- `crates/*/Cargo.toml`
- `crates/pharness-core/src/execution/target.rs`
- `crates/pharness-core/src/resources/resource_ref.rs`
- `crates/pharness-core/src/tools/capability.rs`
- `docs/adr/0001-local-first-cluster-native.md`

Acceptance criteria:

- `cargo metadata` succeeds.
- `cargo fmt --all -- --check` succeeds.
- Empty crates compile with `cargo test --workspace`.
- README states V1 non-goals clearly.
- Core resource/execution/capability types serialize and deserialize in unit tests.
- ADR explains that V1 is local but should preserve V3 cluster-native seams.

Risks/tradeoffs:

- Too many crates early can slow iteration. Keep boundaries stable but APIs small and keep V2/V3 crates unscaffolded until needed.
- SQLite and API crates may pull compile time upward. Start with minimal feature flags.
- Over-modeling future systems can slow V1. Add shared nouns and policy dimensions, not fake Kubernetes implementations.

### Phase 1: Fireworks Provider Client

Goal:

Implement a tested Fireworks chat client that supports streaming, native tool calling, JSON action fallback, retries, and error mapping.

Implementation steps:

1. Define provider trait in `pharness-core::model`.
2. Implement Fireworks request/response DTOs.
3. Add chat completion request builder:
   - `model`
   - `messages`
   - `tools`
   - `tool_choice`
   - `stream`
   - `temperature`
   - `max_tokens`
   - optional `response_format`
   - optional `reasoning_effort`
4. Implement SSE streaming parser for Fireworks chunks.
5. Accumulate native streamed tool-call arguments by tool-call index.
6. Convert provider tool calls to `AgentAction`.
7. Implement JSON action fallback parser and validation path.
8. Add retry policy for 429, 408, 5xx, connection reset:
   - exponential backoff with jitter
   - max attempts configurable
   - emit retry events
9. Map provider errors to stable internal error codes.
10. Add tests using recorded JSON fixtures, not live calls.
11. Add optional ignored live test behind `FIREWORKS_API_KEY`.

Suggested files/modules:

- `crates/pharness-core/src/model/provider.rs`
- `crates/pharness-core/src/model/action_protocol.rs`
- `crates/pharness-fireworks/src/client.rs`
- `crates/pharness-fireworks/src/types.rs`
- `crates/pharness-fireworks/src/stream.rs`
- `crates/pharness-fireworks/src/tools.rs`
- `crates/pharness-fireworks/src/errors.rs`

Acceptance criteria:

- Can parse a non-streamed assistant response.
- Can parse a streamed content response.
- Can parse a streamed tool-call response with incremental arguments.
- Invalid action JSON is rejected before execution.
- 429 fixture maps to retryable provider error.
- Live ignored test can call Fireworks and get one `respond` or tool action.

Risks/tradeoffs:

- Tool-calling quality varies by model. Keep model capabilities configurable.
- Streaming tool-call assembly is easy to get subtly wrong. Use fixtures for partial argument chunks.
- Fireworks is OpenAI-compatible but not identical. Keep provider DTOs separate from internal types.

### Phase 2: Rust Core Agent Loop

Goal:

Implement the state machine that accepts a user task, asks the model for one action per turn, executes/persists/streams results, and stops on finish, failure, cancellation, approval pause, or max turns.

Implementation steps:

1. Define `AgentRuntime`, `RunConfig`, `RunState`, `RunLimits`, and `ExecutionTarget`.
2. Define `AgentAction` and `ToolResult`.
3. Implement turn loop:
   - build context
   - call provider
   - emit `action.proposed`
   - evaluate policy
   - execute or pause
   - append result to context
   - continue
4. Add cancellation token checked:
   - before provider call
   - after provider call
   - before tool execution
   - during long shell execution
5. Add max-turn enforcement.
6. Add deterministic fake provider for tests.
7. Add in-memory event sink for tests.
8. Add run terminal states and error codes.
9. Ensure the loop does not know whether tools are local or cluster-backed; it only sees actions, policy decisions, events, and tool results.

Suggested files/modules:

- `crates/pharness-core/src/agent/loop.rs`
- `crates/pharness-core/src/agent/state.rs`
- `crates/pharness-core/src/agent/turn.rs`
- `crates/pharness-core/src/agent/limits.rs`
- `crates/pharness-core/src/execution/target.rs`
- `crates/pharness-core/src/execution/environment.rs`
- `crates/pharness-core/src/events/bus.rs`
- `crates/pharness-core/src/events/schema.rs`

Acceptance criteria:

- Fake provider can drive a complete run: search/read/respond/finish.
- Run stops at `max_turns`.
- Run pauses on approval-required action.
- Run resumes after approval.
- Cancellation transitions to `Cancelled`.
- Every transition emits a persisted event through the event sink interface.
- A run stores and emits its execution target, even when it is only `local_process`.

Current status:

- One-action-per-turn loop, max-turn stop, cancellation, approval-required pause, policy evaluation, tool execution, and durable event emission exist.
- Native tool-call history is preserved across turns, including matching tool result IDs.
- Approval resume is implemented. Runtime persists a pending approval payload with the exact reviewed action and transcript, then resumes by executing that reviewed payload before returning to the model.

Risks/tradeoffs:

- A too-clever loop will be hard to debug. Keep one action per turn in V1, and keep execution-target dispatch below the tool interface.
- Parallel tool execution is tempting, but it complicates safety and ordering. Defer.

### Phase 3: Local Filesystem and Shell Tools

Goal:

Implement the actual V1 tool set for local repo work.

Implementation steps:

1. Implement `Tool` trait:
   - name
   - schema
   - capability kind
   - supported execution targets
   - policy precheck inputs
   - async execute
2. Implement `list_dir`:
   - respects root path
   - depth limit
   - respects `.gitignore` by default
3. Implement `search_files`:
   - use `ignore`/`walkdir` and `grep`-style Rust search, or shell out to `rg` only through shell tool
   - return paths, line numbers, snippets
4. Implement `read_file`:
   - max bytes
   - UTF-8 text first
   - binary detection
   - sensitive path denial
5. Implement `write_file`:
   - create parent dirs only with approval
   - write via temp file plus atomic rename where possible
   - store before/after hashes and diff
6. Implement `patch_file`:
   - support unified diff or a constrained patch format
   - reject fuzzy patch failures
   - store diff artifact
7. Implement `run_shell`:
   - run via configured shell
   - controlled cwd
   - timeout
   - stdout/stderr streaming
   - max output bytes with truncation
   - environment allowlist
8. Implement `git_status` and `git_diff` as direct command wrappers with stable parsing/output truncation.
9. Add tests in temp directories.

Suggested files/modules:

- `crates/pharness-core/src/tools/registry.rs`
- `crates/pharness-core/src/tools/capability.rs`
- `crates/pharness-core/src/tools/fs.rs`
- `crates/pharness-core/src/tools/search.rs`
- `crates/pharness-core/src/tools/patch.rs`
- `crates/pharness-core/src/tools/shell.rs`
- `crates/pharness-core/src/tools/git.rs`
- `crates/pharness-core/src/workspace/paths.rs`
- `crates/pharness-core/src/workspace/diff.rs`

Acceptance criteria:

- Read/list/search tools cannot escape workspace via `..` or symlink.
- Write/patch generates a unified diff artifact.
- Shell command timeout kills process group.
- Shell output streams as events and stores a redacted/truncated final result.
- Git status/diff work in a repo and fail gracefully outside a repo.
- Each tool declares capability metadata so future cluster policy can evaluate typed tools the same way it evaluates V1 shell/file tools.

Risks/tradeoffs:

- Implementing patch application is riskier than direct writes. Consider using a proven patch crate or applying full-file edits first, then adding unified patches.
- Shell process group handling differs by OS. V1 can target macOS/Linux and document Windows as future.

### Phase 4: Safety and Approval System

Goal:

Add explicit policy evaluation, approval persistence, CLI/API approval flow, command classification, and secret redaction.

Implementation steps:

1. Define `PolicyDecision`:
   - `Allow`
   - `Ask`
   - `Deny`
2. Define `RiskLevel`: `low`, `medium`, `high`, `critical`.
3. Implement path policy:
   - canonical root checks
   - symlink checks
   - sensitive path detection
   - protected directories
4. Implement command classifier:
   - tokenize with a shell parser crate where possible
   - split compound commands
   - classify each subcommand
   - highest risk wins
5. Implement approval model:
   - approval request from policy
   - persisted approval row
   - run pauses
   - user approves/denies
   - run resumes or fails/finishes blocked
6. Add trusted mode:
   - writes inside workspace can be auto-approved
   - destructive/network still ask
   - privileged and secret-accessing remain denied
7. Implement redaction:
   - output redaction
   - event payload redaction
   - artifact redaction for shell output
8. Add policy tests with a table of commands and paths.
9. Add policy data structures for environment tier and capability kind, even though V1 mostly evaluates filesystem/shell/git.
10. Add explicit tests that cluster mutation-shaped shell commands are not accidentally treated as read-only:
   - `kubectl apply`
   - `kubectl delete`
   - `helm upgrade`
   - `argocd app sync`
   - `tkn pipeline start`
   - `docker push`
   - `crane push`

Suggested files/modules:

- `crates/pharness-core/src/policy/command_classifier.rs`
- `crates/pharness-core/src/policy/path_policy.rs`
- `crates/pharness-core/src/policy/approval.rs`
- `crates/pharness-core/src/policy/redaction.rs`
- `crates/pharness-core/src/tools/capability.rs`
- `crates/pharness-core/src/execution/environment.rs`

Acceptance criteria:

- Read-only commands are allowed by default.
- `write_file` and `patch_file` ask in default mode.
- `rm -rf`, network commands, and package installs ask.
- `sudo` is denied by default.
- `.env`, SSH keys, kubeconfigs, and private keys are denied/redacted.
- Approval denial does not execute the tool.
- Approval grant executes only the originally reviewed action payload.
- Cluster mutation shell commands require approval or denial by default.
- Production environment metadata always increases risk for mutating capabilities.

Current status:

- Policy decisions are durable events and use explicit `allow`, `ask`, and `deny` results.
- Read-only filesystem, git, and typed cluster reads are allowed.
- File writes ask by default; destructive/network shell commands ask; privileged and secret-accessing commands deny.
- The default worker schema exposes `write_file` and `patch_file` behind policy approval.
- The default worker schema does not expose `request_approval`. Models must call the concrete policy-gated tool; policy creates approval-required state. This avoids approvals that have no reviewed action payload to resume.

Next slice:

1. Smoke-test approval resume with a harmless real Fireworks write task.
2. Add CLI approval commands.
3. Store and retrieve reviewed diffs/artifacts for write and future patch actions.
4. Add richer patch preview/review if the approval UI needs generated diffs before approval.

Risks/tradeoffs:

- Command classification can never be perfect. The policy should be conservative and transparent.
- Secret detection can false-positive. Better to block and let the user override through explicit config later.
- Future typed cluster tools reduce reliance on fragile shell parsing, but they still need policy gates.

### Phase 5: Session and Event Persistence

Goal:

Persist sessions, runs, messages, events, tool calls, approvals, artifacts, diffs, and shell summaries locally in SQLite.

Implementation steps:

1. Add `Store` trait in `pharness-store`.
2. Implement SQLite migrations.
3. Implement repository methods:
   - create/list/get sessions
   - create/get/update runs
   - append messages
   - append events with seq
   - create/update tool calls
   - create/decide approvals
   - create artifacts/file changes
   - create/list resource refs
   - create/list context items
4. Enable SQLite WAL mode.
5. Add event replay query by run and `Last-Event-ID`.
6. Add transaction boundaries:
   - persist event and state update together where possible
   - persist approval before emitting approval-required event
7. Add export command later: session as JSON.

Suggested files/modules:

- `crates/pharness-store/migrations/0001_initial.sql`
- `crates/pharness-store/src/lib.rs`
- `crates/pharness-store/src/models.rs`
- `crates/pharness-store/src/sqlite.rs`

Acceptance criteria:

- A complete fake run can be persisted and reloaded.
- Event seq is monotonically increasing per run.
- SSE replay can query events after a given event id.
- Approvals survive process restart.
- Store tests run against temp SQLite DB.
- Resource refs can represent local files now and Kubernetes/Tekton/Argo/registry/LGTM/RAG objects later.
- Context items can store local summaries now and RAG retrieval results later.

Risks/tradeoffs:

- Storing large outputs in SQLite can bloat DB. V1 can store truncated text in DB and later move large artifacts to files/object storage.
- Full reproducibility requires provider raw responses and config snapshots. Store those as metadata but redact secrets.
- Future cluster artifacts may belong in object storage, but their metadata should still be visible through the same store interface.

### Phase 6: CLI

Goal:

Provide a fast local developer workflow without requiring the web UI.

Implementation steps:

1. Implement `pharness init`:
   - write `.pharness.toml` if missing
   - never overwrite without prompt
2. Implement `pharness run "<task>"`:
   - create session/run
   - stream events to terminal
   - prompt for approvals interactively
3. Implement `pharness serve`:
   - starts API server
   - optionally serves UI static build later
4. Implement `pharness sessions list/show`.
5. Implement `pharness approvals list/approve/deny`.
6. Implement `--cwd`, `--config`, `--model`, `--max-turns`, `--permission-mode`.
7. Add terminal rendering:
   - compact event log
   - approval prompt with reason/classification/diff
   - final summary

Suggested files/modules:

- `crates/pharness-cli/src/main.rs`
- `crates/pharness-cli/src/commands/init.rs`
- `crates/pharness-cli/src/commands/run.rs`
- `crates/pharness-cli/src/commands/serve.rs`
- `crates/pharness-cli/src/commands/sessions.rs`
- `crates/pharness-cli/src/commands/approvals.rs`

Acceptance criteria:

- `pharness run "summarize this repo"` works with fake provider.
- CLI asks before a write action.
- CLI approval executes reviewed action.
- `Ctrl-C` requests cancellation and stores cancelled run state.
- Session can be resumed/listed after process restart.

Risks/tradeoffs:

- Fancy terminal UI can eat time. Keep V1 output plain and reliable.
- Approval UX must show enough context without dumping huge content.

### Phase 7: Minimal API

Goal:

Expose sessions, messages, runs, event streaming, approvals, artifacts, and diffs to the UI.

Implementation steps:

1. Add `axum` app with typed routes.
2. Add request/response DTOs separate from DB models.
3. Add session routes.
4. Add message/run creation route.
5. Add run cancellation route.
6. Add SSE route with replay support.
7. Add approval routes.
8. Add artifact and diff routes.
9. Add local-only auth posture:
   - bind to `127.0.0.1` by default
   - no auth in V1 local mode
   - explicit warning if binding non-loopback
10. Add OpenAPI later only if needed.

Suggested files/modules:

- `crates/pharness-api/src/app.rs`
- `crates/pharness-api/src/routes/*.rs`
- `crates/pharness-api/src/sse.rs`
- `crates/pharness-api/src/dto.rs`

Acceptance criteria:

- UI can create a session and run.
- `GET /api/runs/:id/events/stream` streams live events.
- SSE reconnect with `Last-Event-ID` replays missed events.
- Approval approve/deny changes run behavior.
- Cancel endpoint stops a running fake-provider run.

Current status:

- Implemented: `POST /api/runs`, `GET /api/runs/:id`, `GET /api/runs/:id/events`, `GET /api/runs/:id/events/stream`, `GET /api/runs/:id/diff`, `GET /api/runs/:id/artifacts`, `GET /api/artifacts/:id`, `POST /api/runs/:id/cancel`, `GET /api/approvals`, `POST /api/runs/:id/approvals`, `POST /api/capabilities/execute`, and effective worker config.
- Verified with a real Fireworks-backed run that persisted structured events and final result JSON.
- Tested approval denial, approval listing, and runtime approved-action resume.
- Live-tested `pharness-cli approvals list` and `pharness-cli approvals approve --run-id ...` against a real Fireworks write approval.
- Live-tested direct model-free capability execution for Kubernetes pod reads, Argo Application reads, secret-shaped Kubernetes denial, and missing Prometheus URL error handling.
- Live-tested Prometheus success through a loopback port-forward, including direct capability execution, model-backed run execution, secret-shaped query denial, response compaction, and artifact retrieval.
- Implemented typed read-only Tekton PipelineRun inventory through `tekton_get_pipeline_runs`, TaskRun inventory through `tekton_get_task_runs`, and normalized PipelineRun analysis through `tekton_analyze_pipeline_run`.
- Live-tested PipelineRun analysis against real Finance PipelineRuns; pharness now correlates build outputs to Deployment rollout status, image alignment, and Argo sync/health without mutating the cluster.
- Not implemented: approval-by-id routes.

Next API order:

1. Approval-by-id routes if external operators need global approval workflows.
2. CLI artifact commands if operator workflow needs them.
3. Broader read-only cluster inventory capabilities, starting with LGTM endpoints.

Risks/tradeoffs:

- API and CLI may duplicate approval logic. Keep approval decisions in core/store and make CLI/API thin.
- Local no-auth is acceptable only when loopback-bound. V2 needs auth or network isolation.

### Phase 8: Minimal TypeScript UI

Goal:

Build a small local UI for interactive runs, event visibility, approvals, diffs, and session history.

Implementation steps:

1. Create Vite React app.
2. Build API client.
3. Build SSE event client with reconnect.
4. Build main layout:
   - left session list
   - center event log
   - right run details/diff/approvals, or responsive stacked layout
5. Components:
   - `PromptBox`
   - `RunStatus`
   - `EventLog`
   - `ApprovalCard`
   - `DiffViewer`
   - `ShellOutput`
   - `SessionList`
6. Add diff viewer with syntax-light styling. Avoid heavy editor integrations in V1.
7. Add approval buttons:
   - approve once
   - deny
   - optional trust writes for this session
8. Add run cancellation button.
9. Add basic empty/error/loading states.

Suggested files/modules:

- `ui/src/api/client.ts`
- `ui/src/api/events.ts`
- `ui/src/app/App.tsx`
- `ui/src/components/*.tsx`
- `ui/src/styles.css`

Acceptance criteria:

- User can start a run from browser.
- Live events appear without page refresh.
- Approval card pauses/resumes a run.
- Diff viewer shows file changes from artifacts.
- User can cancel a run.
- Session list loads previous sessions.

Risks/tradeoffs:

- Avoid building an IDE. UI is operational visibility and approvals only.
- Keep frontend dependencies minimal to avoid maintenance drag.

### Phase 9: Local Dogfooding on Real Tasks

Goal:

Use the harness against small real repos and tighten the loop, context packing, policy, and UX.

Implementation steps:

1. Dogfood task class A: repo summary only.
2. Dogfood task class B: add a small markdown/doc file.
3. Dogfood task class C: make a one-file code change with tests.
4. Dogfood task class D: fix a failing test in a temp fixture repo.
5. Dogfood task class E: make a GitOps-style manifest change in a local fixture repo without applying it to a cluster.
6. Dogfood task class F: inspect captured Kubernetes/Argo/Tekton/LGTM fixture JSON as read-only context and produce a deployment diagnosis.
7. Record:
   - turn count
   - tokens
   - approvals requested
   - false-positive approvals
   - command failures
   - context overflows
   - bad tool args
   - production-risk classifications
   - cluster-shaped command classifications
8. Add regression fixtures for failures found.
9. Tune prompts/tool descriptions/context packer.
10. Write a "known limitations" doc.

Suggested files/modules:

- `tests/fixtures/`
- `tests/fixtures/gitops-app/`
- `tests/fixtures/cluster-observations/`
- `docs/dogfooding.md`
- `docs/known-limitations.md`

Acceptance criteria:

- Harness can complete at least three small real local tasks.
- Every write is visible in diff before/after execution.
- Failed commands feed back into the model and cause a useful next action.
- Sessions can be replayed from persisted events.
- No secrets appear in event logs during test fixtures.
- GitOps-style changes produce clear diffs and do not imply direct cluster mutation.
- Read-only cluster fixture context can be packed and summarized without adding real cluster access.

Current first dogfood result:

- Real Fireworks repo-listing task completed against this workspace with durable events and a structured result.
- Treat this as dogfood task class A baseline.
- Next dogfood tasks should deliberately target approval behavior and read-only cluster observations before UI work.

Risks/tradeoffs:

- Model quality may be the biggest variable. Keep action protocol and tool results crisp before adding features.
- Dogfooding can tempt scope creep. Fix loop/policy/context issues first.

### Phase 10: V2 Kubernetes/Homelab Deployment

Goal:

Move the runtime into Kubernetes with isolated per-run workspaces while preserving the V1 API/session semantics and preparing for typed cluster capabilities.

Implementation steps:

1. Containerize runtime and UI.
2. Split services:
   - API service
   - UI service/static assets
   - worker pod/job for agent runs
3. Add queue table or lightweight internal queue:
   - V2 can start with Postgres table polling
   - avoid Kafka/Redis unless needed
4. Add per-run workspace:
   - init container clones or copies repo
   - emptyDir or PVC
   - artifact volume
5. Add sandbox controls:
   - non-root containers
   - read-only root filesystem where possible
   - seccomp/runtime default
   - resource requests/limits
   - network policy
6. Add secrets:
   - Fireworks API key as Kubernetes Secret
   - never inject user repo secrets by default
7. Add optional Postgres store.
8. Add homelab manifests:
   - Helm chart or Kustomize
   - Ingress
   - TLS via existing cluster pattern
   - PVC for artifacts if needed
9. Add worker lifecycle:
   - create run job
   - stream events through DB/API
   - cancel run by deleting job and marking cancelled
10. Add artifact retention settings.
11. Add read-only Kubernetes observation capability:
   - list/get/describe non-secret objects in allowed namespaces
   - pod logs with redaction and truncation
   - events
   - deployment/rollout status
12. Add read-only Argo CD observation capability:
   - app health
   - sync status
   - target revision
   - resource tree
13. Add read-only Tekton observation capability:
   - PipelineRun/TaskRun status
   - logs
   - artifact/image digest references when present
   - V1 has PipelineRun and TaskRun inventory plus PipelineRun analysis with build outputs, Deployment rollout, image alignment, and Argo sync/health; bounded logs and Prometheus correlation remain future work.
14. Add read-only LGTM observation capability through configured query endpoints:
   - logs
   - metrics
   - traces
   - alert state
15. Keep all mutation capabilities disabled or approval-only until V3.

Suggested files/modules:

- `deploy/docker/Dockerfile.runtime`
- `deploy/docker/Dockerfile.ui`
- `deploy/helm/pharness/templates/api-deployment.yaml`
- `deploy/helm/pharness/templates/ui-deployment.yaml`
- `deploy/helm/pharness/templates/worker-rbac.yaml`
- `deploy/helm/pharness/templates/postgres-secret.yaml`
- `deploy/helm/pharness/values.yaml`
- `crates/pharness-kube/src/read.rs`
- `crates/pharness-delivery/src/argocd_read.rs`
- `crates/pharness-delivery/src/tekton_read.rs`
- `crates/pharness-observe/src/lgtm.rs`

Acceptance criteria:

- Helm/Kustomize deploys API and UI in homelab namespace.
- A run creates an isolated worker pod/job.
- Worker has resource limits and non-root security context.
- Events persist and stream through API.
- Cancelling a run terminates worker.
- Artifacts survive worker pod exit.
- Network access can be disabled or constrained per run.
- Read-only cluster observations are stored as resource refs/context items.
- Service account permissions are namespace-scoped and do not grant secret reads by default.
- Argo/Tekton/LGTM read failures degrade into visible events, not hidden agent confusion.

Risks/tradeoffs:

- Cloning local/private repos into cluster introduces auth complexity. V2 should support a simple Git URL plus deploy key first, then local sync later.
- Kubernetes sandboxing is not a perfect security boundary. Treat it as defense in depth, not permission replacement.
- Postgres adds operational overhead. Keep SQLite for single-node local mode.
- Even read-only cluster context can leak sensitive data through logs. Redaction and namespace scoping matter before broadening access.

### Phase 11: Hardening, Observability, and Future Extensions

Goal:

Make the harness reliable enough for regular use and identify future expansion points without building them prematurely.

Implementation steps:

1. Add structured logs with request/run/session IDs.
2. Add metrics:
   - run duration
   - model latency
   - tool duration
   - approval wait time
   - token usage
   - retries/errors
3. Add trace export later if needed.
4. Add session export/import.
5. Add compaction/summarization for long sessions.
6. Add model-provider tests for another OpenAI-compatible provider only after Fireworks is solid.
7. Add filesystem snapshot/rollback support.
8. Add configurable command allow/deny rules.
9. Add optional auth for non-loopback API.
10. Add signed audit logs for V2 if needed.
11. Add RAG abstraction behind `ContextSource`:
   - search memories
   - cite retrieved context in events
   - write durable run learnings only after policy approval or explicit config
12. Add delivery verification summaries from Argo/Tekton/LGTM resource refs.

Suggested files/modules:

- `crates/pharness-core/src/observability/`
- `crates/pharness-core/src/session_export.rs`
- `docs/security.md`
- `docs/operations.md`
- `crates/pharness-rag/src/lib.rs`
- `crates/pharness-observe/src/verification.rs`

Acceptance criteria:

- Every run can be diagnosed from logs and events.
- Long sessions can compact without losing pending approval/current task.
- User can export a session bundle for debugging.
- Policy config supports exact command allow/deny entries.
- V2 deployment has basic dashboards or documented log queries.
- RAG context is visible as explicit context items, never hidden prompt magic.
- Delivery verification can tie a code change to build, image digest, deployed revision, and observability signals.

Risks/tradeoffs:

- Observability libraries can get heavy. Start with structured logs and internal event schema.
- Future extension points should not become a plugin system by accident.
- RAG writeback can pollute future context if too eager. Start read-mostly and make memory writes explicit.

### V3 North Star: Cluster-Native Autonomous Production-App Workflow

Goal:

Give the agent first-class, policy-governed access to the cluster systems needed to code, build, release, verify, and roll back production apps.

Implementation steps:

1. Add OCI registry capability:
   - inspect repositories/tags/digests
   - publish image metadata from Tekton outputs
   - verify deployed digest matches built digest
2. Add Tekton mutation capability:
   - start pipeline from approved repo revision
   - stream PipelineRun/TaskRun logs
   - collect image digest, test results, SBOM/provenance if available
3. Add Argo CD mutation capability:
   - inspect app health/sync/resource tree
   - preview GitOps diff
   - sync approved app/revision
   - watch reconciliation
   - initiate rollback to prior known-good revision
4. Add database operator capability:
   - inspect database/custom resource health
   - request backup
   - plan migration
   - apply migration only with backup reference and approval
   - verify post-migration health
5. Add LGTM verification capability:
   - query logs for new errors
   - query metrics for SLO and rollout health
   - query traces for latency/error changes
   - capture dashboards or query summaries as artifacts
6. Add RAG-backed long-lived context:
   - retrieve architecture notes, runbooks, past incident summaries, app ownership notes, and prior deployment lessons
   - write memory only for stable, reviewed facts
7. Add autonomous workflow controller:
   - task intake
   - repo edit
   - tests
   - build
   - image publish
   - GitOps update
   - Argo sync
   - rollout watch
   - LGTM verification
   - rollback or finish
8. Add production approval gates:
   - before build publish if registry write is mutable
   - before GitOps merge/commit
   - before Argo sync
   - before database migration
   - before rollback
   - before memory writeback that affects future production behavior

Suggested files/modules:

- `crates/pharness-delivery/src/registry.rs`
- `crates/pharness-delivery/src/tekton.rs`
- `crates/pharness-delivery/src/argocd.rs`
- `crates/pharness-delivery/src/workflow.rs`
- `crates/pharness-kube/src/resources.rs`
- `crates/pharness-observe/src/logs.rs`
- `crates/pharness-observe/src/metrics.rs`
- `crates/pharness-observe/src/traces.rs`
- `crates/pharness-rag/src/search.rs`
- `crates/pharness-rag/src/writeback.rs`

Acceptance criteria:

- Agent can take a code task from prompt to verified deployed revision in a non-production namespace with only configured approval gates.
- Production run requires explicit approval for deploy, database, rollback, and registry mutation gates.
- Every external action has a resource ref, event trail, policy decision, and artifact where applicable.
- Agent can explain exactly what changed: commit, image digest, GitOps diff, Argo app, Kubernetes resources, Tekton run, database migration, and LGTM verification.
- Failed verification triggers a safe stop or approved rollback path.
- RAG memories are cited when used and reviewed before durable writeback.

Risks/tradeoffs:

- This is where the system can become dangerous. Typed tools, scoped service accounts, approval gates, and audit events are non-negotiable.
- Cluster APIs create version drift. Keep adapters thin and test against fixture payloads plus one real homelab conformance suite.
- Autonomous production workflows need rollback discipline as much as deploy capability.

## Milestone Cut Lines

### V1 Alpha

Includes:

- Fireworks client.
- Core loop.
- File read/list/search.
- Shell execution with policy.
- SQLite sessions/events.
- CLI only.

Acceptance:

- Complete read-only repo summary.
- Complete one approved file edit.
- Run one approved verification command.

### V1 Beta

Includes:

- Patch/diff workflow.
- API.
- Minimal UI.
- Approval cards.
- Session replay.
- Dogfooding fixes.

Acceptance:

- Complete small real coding task through UI.
- Approval and diff UX is usable.
- No known secret leakage in logs/events.

### V1 Stable

Includes:

- Hardened policy tests.
- Better context packing.
- Session export.
- Documentation.
- Installable local binary.

Acceptance:

- Regular local use on small/medium repos.
- Clear recovery on model/tool failures.
- Reproducible session log.

### V2 Alpha

Includes:

- Container images.
- Homelab Kubernetes deployment.
- Worker pod per run.
- Optional Postgres.
- Artifact persistence.
- Read-only Kubernetes/Argo/Tekton/LGTM observation capabilities.

Acceptance:

- Run executes in cluster worker and streams events to UI.
- Worker isolation and limits are documented and enforced.
- Cluster observations are scoped, redacted, persisted, and visible in the event log.

### V3 Alpha

Includes:

- OCI registry capability.
- Tekton build/run capability.
- Argo CD sync/rollback capability.
- Database operator backup/migration capability.
- LGTM verification capability.
- RAG retrieval and reviewed memory writeback.
- End-to-end delivery workflow controller.

Acceptance:

- Agent can complete a non-production code-build-deploy-verify loop.
- Production deploy path pauses at explicit approval gates.
- Every cluster/build/deploy/database/observability action is typed, audited, and replayable.

## Testing Strategy

### Unit Tests

- `AgentAction` schema parse/validation.
- Fireworks stream chunk parser.
- Tool-call argument accumulator.
- Command classifier table tests.
- Path canonicalization and symlink blocking.
- Secret redaction patterns.
- Context budget pruning.
- Event seq assignment.
- `ExecutionTarget`, `ResourceRef`, and `ArtifactRef` serialization.
- Cluster capability policy matrix tests.

### Integration Tests

- Fake provider drives complete run.
- Fake provider requests approval, run pauses, approval resumes.
- Temp repo write creates correct diff.
- Shell timeout kills long command.
- Shell output truncates and redacts.
- SQLite persists/reloads run.
- API SSE replay from `Last-Event-ID`.
- GitOps fixture change creates diff without direct cluster mutation.
- Captured Kubernetes/Argo/Tekton/LGTM fixture payloads become context items/resource refs.

### Live Provider Tests

Ignored by default:

```text
FIREWORKS_API_KEY=... cargo test -p pharness-fireworks --test live -- --ignored
```

Live tests should:

- Send a simple respond request.
- Send a tool-calling request.
- Send a JSON action fallback request if supported.
- Verify retry mapping manually with mocked 429, not by forcing real rate limits.

### UI Tests

- Component tests for event rendering and approval cards.
- Playwright smoke:
  - start API with fake provider
  - start UI
  - create run
  - approve patch
  - see finished state

### V2/V3 Cluster Conformance Tests

Run only against an explicitly configured test namespace:

- Worker pod starts with expected service account, limits, and network policy.
- Kubernetes read capability cannot read Secrets by default.
- Argo CD read capability can observe only allowed apps.
- Tekton read capability can observe only allowed PipelineRuns.
- LGTM queries redact sensitive log patterns.
- Registry write requires approval and records image digest.
- Database migration requires backup ref and approval.

## Security Checklist

V1 must satisfy:

- Workspace path canonicalization for every file tool.
- Symlink target checks.
- Sensitive path denylist.
- Secret redaction before event/artifact persistence.
- Command classification before shell execution.
- Compound command splitting.
- Timeout for every shell command.
- Output truncation for every shell command.
- Default-deny privileged commands.
- Approval required for writes in default mode.
- Approval required for destructive/network commands.
- Reviewed action payload is immutable after approval request.
- No provider API key in logs, events, DB, or UI.
- API binds to loopback by default.
- Warning when API binds to non-loopback.
- Tests for common secret patterns and dangerous commands.

V2 must add:

- Non-root worker containers.
- Resource limits.
- NetworkPolicy.
- Minimal RBAC.
- Secret mounted only where required.
- Artifact retention policy.
- Per-run workspace cleanup.
- Auth or network isolation for UI/API.
- Read-only cluster access must exclude Secrets by default.
- Worker service accounts must be namespace-scoped unless a run explicitly needs more.
- Cluster observation logs must pass through the same redaction pipeline as shell output.

V3 must add:

- Separate service accounts per capability or environment tier where practical.
- Registry write approval and immutable digest recording.
- Tekton run approval and provenance/test artifact capture.
- Argo CD sync approval with Git revision, diff, target app, and rollback reference.
- Database migration approval with backup reference, preflight result, and rollback plan.
- Production direct `kubectl apply/delete` denied unless break-glass is explicitly enabled.
- LGTM verification queries recorded as artifacts.
- RAG retrieval citations visible in the run log.
- RAG memory writeback reviewed or policy-approved.
- Audit log for every production-impacting action.

## First Concrete Execution Sequence

This is the order Codex should implement in this repo:

1. Create Rust workspace and crate skeletons.
2. Add shared IDs, errors, event schema, action schema, execution target, capability kind, and resource/artifact ref types in `pharness-core`.
3. Add fake provider and in-memory event sink.
4. Implement core loop against fake provider.
5. Add file list/read/search tools with path policy and capability metadata.
6. Add SQLite store and persist fake runs, resource refs, and context items.
7. Add Fireworks client behind provider trait.
8. Add CLI `run` with fake provider, then Fireworks provider.
9. Add write/patch tools and approvals.
10. Add shell tool and command classifier, including cluster-shaped command classifications.
11. Add API routes and SSE.
12. Add minimal UI.
13. Dogfood local repo tasks and GitOps fixture tasks.
14. Harden before adding real cluster access.

## Practical Defaults

- One action per model turn.
- Native Fireworks tool calling first.
- JSON action fallback available by config.
- Read-only filesystem and git inspection allowed.
- Writes ask.
- Shell tests ask in default mode because they can write build artifacts.
- Network asks.
- Privileged denied.
- Sensitive files denied.
- SQLite local DB.
- SSE over WebSockets for V1 because replay is simpler.
- No plugin loading.
- No MCP until after V1 stable, and only if it solves a concrete local workflow.
- V1 cluster-shaped commands are treated as risky shell commands, not privileged built-ins.
- V2 adds read-only cluster observation before cluster mutation.
- V3 production changes flow through GitOps, typed capabilities, approval gates, and observability verification.
- RAG is an explicit context source with visible citations, not hidden memory magic.
