# Pharness

Pharness is a lightweight, Fireworks-first agent harness for local coding workflows that is designed to grow into a Kubernetes-native autonomous delivery runtime.

V1 runs locally from a project root. It focuses on the agent loop, explicit tool actions, file and shell tooling, policy checks, approvals, event streaming, and durable sessions. V2 moves execution into a homelab Kubernetes cluster with isolated worker pods. V3 adds first-class typed capabilities for registry, Tekton, Argo CD, database operators, LGTM observability, and RAG-backed long-lived context.

MCP is a future adapter option for obvious external workflow integrations such as Jira or Slack, but it is not a V1 dependency and should not become a plugin marketplace by accident.

For current architecture, design, implementation history, active work, and
operational evidence, start with the
[`planning/` documentation map](planning/README.md). Archived plans are retained
there but are explicitly separated from current direction.

## V1 Scope

- Rust core runtime and state machine.
- Fireworks AI provider client.
- Local filesystem tools.
- Shell command execution with timeout, truncation, redaction, and approval policy.
- Git status and diff awareness.
- SQLite session and event persistence.
- Local CLI and minimal web UI.

## V1 Non-Goals

- No plugin marketplace.
- No third-party integration ecosystem.
- No MCP dependency in the core V1 loop.
- No remote execution.
- No direct Kubernetes mutation tools.
- No assumption that `kubectl`, `argocd`, `tkn`, `helm`, registry clients, or database CLIs are safe just because they are installed.
- No autonomous production deployment.

## Cluster-Native Direction

The early code keeps these future concepts explicit without implementing them prematurely:

- `ExecutionTarget`: local process today, Kubernetes worker job later.
- `ResourceRef`: local files today, Kubernetes objects, OCI images, Tekton runs, Argo CD apps, LGTM queries, database backups, and RAG memories later.
- `CapabilityKind`: policy can evaluate filesystem, shell, git, Kubernetes, registry, Tekton, Argo, database, observability, and RAG actions through one vocabulary.

Production app changes should eventually flow through GitOps: edit Git, build with Tekton, publish immutable image digests, reconcile with Argo CD, verify with LGTM signals, and require explicit approval for production-impacting actions.

For a Pharness release, build the API runtime, UI, Python runner, Node runner,
model gateway, and evaluation runner from the exact merged `origin/main`
revision. Capture all six registry digests, then prepare a separate reviewed
Helm release commit:

```bash
scripts/pharness-build.sh all --revision <40-character-origin-main-sha>
scripts/pharness-release-pin.sh <same-sha> <runtime-sha256-digest> <ui-sha256-digest> <python-runner-sha256-digest> <node-runner-sha256-digest> <model-gateway-sha256-digest> <eval-runner-sha256-digest>
```

The pinning command requires a clean worktree and an exact current `origin/main`
revision. It updates only the API/UI, runner, gateway, and evaluator
digest/revision fields, validates the chart, and proves the rendered release
uses the supplied immutable references. It never commits, merges, restarts a
Deployment, or syncs Argo.

## Development Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Fireworks Model

When `FIREWORKS_API_KEY` is set, the local worker uses Fireworks with this default model:

```sh
accounts/fireworks/models/kimi-k2p6
```

Override it explicitly when trying another visible model:

```sh
PHARNESS_FIREWORKS_MODEL=accounts/fireworks/models/kimi-k2p6 cargo run -p pharness-api
```

The API reads provider configuration at startup. Restart `pharness-api` after changing `FIREWORKS_API_KEY` or `PHARNESS_FIREWORKS_MODEL`, then confirm with:

```sh
cargo run -p pharness-cli -- config
```

## Coding Reliability Evaluation

`pharness-eval` exercises the real runtime, policy, tools, and Fireworks
provider against eight disposable Git fixtures. Replay trajectories are the
deterministic CI check; live Fireworks evaluation is deliberately manual and
must never expose credentials or run in CI.

```sh
CARGO_HOME=/private/tmp/pharness-cargo-home CARGO_TARGET_DIR=$PWD/target \
  cargo run -p pharness-eval -- list

CARGO_HOME=/private/tmp/pharness-cargo-home CARGO_TARGET_DIR=$PWD/target \
  cargo run -p pharness-eval -- run --provider replay --attempts 2 \
  --output target/pharness-evals/replay.json
```

For the release gate, use an approved pre-change Fireworks result as the
baseline, pin the model and configuration, then run the candidate twice over
all eight fixtures. The evaluator refuses comparisons that differ in suite,
fixture revision, provider, model, prompt version, temperature, token cap, or
turn cap. Create the matched pre-change control with the historical runner: it
executes the old runtime in a disposable worktree but supplies the candidate
prompt so those comparison fields are identical.

```sh
FIREWORKS_API_KEY='...' \
  CARGO_HOME=/private/tmp/pharness-cargo-home CARGO_TARGET_DIR=$PWD/target \
  cargo run -p pharness-eval -- run --provider fireworks --attempts 2 \
  --output target/pharness-evals/candidate-v1.7.json

FIREWORKS_API_KEY='...' \
  ./scripts/run-historical-baseline.sh --revision <deployed-source-sha> \
  --output target/pharness-evals/baseline-v1.7.json --attempts 2

cargo run -p pharness-eval -- compare \
  --baseline target/pharness-evals/baseline-v1.7.json \
  --candidate target/pharness-evals/candidate-v1.7.json
```

Do not begin the `app.rs` extraction unless the comparison reports the
required improvement and zero safety regressions. Fixture artifacts are kept
under `target/pharness-evals/` and remain untracked.

## Runtime Config

By default the API uses local defaults and env overrides. If `config/pharness.toml` exists, it is loaded automatically; set `PHARNESS_CONFIG` to point at a different TOML file.

```sh
cp config/pharness.example.toml config/pharness.toml
cargo run -p pharness-cli -- config validate --file config/pharness.toml
PHARNESS_CONFIG=config/pharness.toml cargo run -p pharness-api
```

Env overrides still win over TOML for the runtime-critical fields: `PHARNESS_BIND`, `PHARNESS_DB_PATH`, `PHARNESS_FIREWORKS_MODEL`, `PHARNESS_FIREWORKS_BASE_URL`, `PHARNESS_CONTEXT_MAX_INPUT_TOKENS`, `PHARNESS_CONTEXT_RECENT_MESSAGE_TOKENS`, `PHARNESS_CONTEXT_MAX_TOOL_RESULT_TOKENS`, `PHARNESS_KUBECTL_BIN`, `PHARNESS_ARGOCD_NAMESPACE`, `PHARNESS_PROMETHEUS_URL`, `PHARNESS_LOKI_URL`, `PHARNESS_REGISTRY_ALIASES`, `PHARNESS_CLUSTER_TOOL_TIMEOUT_MS`, `PHARNESS_CLUSTER_TOOL_MAX_OUTPUT_BYTES`, `PHARNESS_POLICY_SUBJECT`, `PHARNESS_POLICY_ENVIRONMENT`, `PHARNESS_POLICY_MODE`, `PHARNESS_ALLOW_READ_ONLY_SHELL`, `PHARNESS_REQUIRE_APPROVAL_FOR_WRITES`, `PHARNESS_REQUIRE_APPROVAL_FOR_NETWORK`, `PHARNESS_REQUIRE_APPROVAL_FOR_DESTRUCTIVE`, `PHARNESS_DENY_PRIVILEGED`, and `PHARNESS_DENY_SECRET_ACCESS`.

Policy defaults are intentionally conservative. `default` asks for local file writes, asks for destructive and network shell commands, and denies privileged or secret-accessing commands. `trusted_writes` can be selected in config or per run for local write autonomy:

```sh
cargo run -p pharness-cli -- run \
  --policy-mode trusted_writes \
  --task "Create a small local scratch file, then finish with a summary." \
  --cwd "$PWD"
```

The selected policy is persisted on the run execution target and reused when a run resumes after approval.

Runs can carry SDLC metadata for later audit and policy work. These fields are metadata-only in V1; they do not grant production mutation or change policy decisions by themselves:

```sh
cargo run -p pharness-cli -- run \
  --task "Inspect this repo and finish with one sentence." \
  --cwd "$PWD" \
  --namespace apps-dev \
  --repo git@example.test/team/app.git \
  --branch feature/pharness
```

## Live Logs

The API emits request logs through `tracing`. By default it enables `pharness_api=info,tower_http=info`, so running the API directly shows startup and access logs:

```sh
cargo run -p pharness-api
```

Override verbosity with `RUST_LOG` when needed:

```sh
RUST_LOG=pharness_api=debug,tower_http=info cargo run -p pharness-api
```

For run-level logs, use durable events. The CLI can follow those events while preserving final machine JSON on stdout:

```sh
cargo run -p pharness-cli -- run \
  --follow-events \
  --task "List the top-level files, then finish with one sentence." \
  --cwd "$PWD"
```

The live event lines are written to stderr; the final structured run result is still written to stdout.

Fetch an existing run through the CLI:

```sh
cargo run -p pharness-cli -- runs get --run-id "$RUN_ID"
cargo run -p pharness-cli -- runs get --run-id "$RUN_ID" --with-events
```

Fetch just the durable event log through the CLI:

```sh
cargo run -p pharness-cli -- runs events --run-id "$RUN_ID"
```

Stream new events after a known durable sequence:

```sh
cargo run -p pharness-cli -- runs events \
  --run-id "$RUN_ID" \
  --after-seq 1 \
  --stream
```

The CLI stream prints newline-delimited event JSON. Underneath, the API exposes `GET /api/runs/:run_id/events/stream?after_seq=N` as Server-Sent Events for browser and machine consumers.

Fetch stored file diffs for a run:

```sh
curl "http://127.0.0.1:4777/api/runs/$RUN_ID/diff"
```

The same diff read is available through the CLI:

```sh
cargo run -p pharness-cli -- runs diff --run-id "$RUN_ID"
```

Fetch artifacts for a run:

```sh
curl "http://127.0.0.1:4777/api/runs/$RUN_ID/artifacts"
curl "http://127.0.0.1:4777/api/artifacts/$ARTIFACT_ID"
```

The same artifact reads are available through the CLI:

```sh
cargo run -p pharness-cli -- artifacts list --run-id "$RUN_ID"
cargo run -p pharness-cli -- artifacts get --artifact-id "$ARTIFACT_ID"
```

Manage durable permission grants:

```sh
cargo run -p pharness-cli -- permission-grants create \
  --subject agent:local-worker \
  --created-by "$USER" \
  --reason "trusted local write smoke" \
  --policy-mode trusted_writes \
  --scope-json '{"environment":"local","capability_kinds":["filesystem"],"actions":["write_file","patch_file"],"max_risk":"medium"}'

cargo run -p pharness-cli -- permission-grants list
cargo run -p pharness-cli -- permission-grants get --grant-id "$GRANT_ID"
cargo run -p pharness-cli -- permission-grants revoke \
  --grant-id "$GRANT_ID" \
  --revoked-by lucas \
  --reason "smoke complete"
```

Active permission grants are snapshotted onto new runs and can convert matching local file-write approval requests into allows when the grant subject, environment, capability kind, action, and risk ceiling match the run policy. Matching policy events include `decision.grant_id`. Grants do not override denials and do not grant shell, network, privileged, secret, destructive, or production-impacting actions.

Approval decisions can also wait for the resumed run to finish:

```sh
cargo run -p pharness-cli -- approvals approve \
  --run-id "$RUN_ID" \
  --decided-by lucas \
  --reason "approved" \
  --wait \
  --follow-events
```

Approvals can also be fetched and decided directly by approval id:

```sh
cargo run -p pharness-cli -- approvals get --approval-id "$APPROVAL_ID"
cargo run -p pharness-cli -- approvals approve \
  --approval-id "$APPROVAL_ID" \
  --decided-by lucas \
  --reason "approved" \
  --wait \
  --follow-events
```

Execute a typed read-only capability without invoking the model:

```sh
curl -X POST "http://127.0.0.1:4777/api/capabilities/execute" \
  -H "content-type: application/json" \
  -d '{
    "action": {
      "action": "kubernetes_get",
      "id": "manual.kubernetes_get",
      "reason": "manual smoke",
      "resource": "pods",
      "namespace": "argocd",
      "name": null,
      "all_namespaces": false,
      "label_selector": null
    }
  }'
```

## Cluster Read Smoke

The local worker can dogfood read-only cluster capabilities through typed tools. Kubernetes and Argo reads use `kubectl`; Argo reads target the Application CRD in `PHARNESS_ARGOCD_NAMESPACE`, defaulting to `argocd`.

```sh
cargo run -p pharness-cli -- run \
  --follow-events \
  --task "Use kubernetes_get to list pods in the argocd namespace, then finish with one sentence summarizing the pod count and whether the data was read-only. Do not read secrets." \
  --cwd "$PWD" \
  --timeout-ms 180000
```

Model-free direct checks:

```sh
cargo run -p pharness-cli -- capabilities kubernetes-get \
  --resource pods \
  --namespace argocd
```

```sh
cargo run -p pharness-cli -- capabilities argo-get-app \
  --app ghost
```

Read Tekton PipelineRuns without invoking the model:

```sh
cargo run -p pharness-cli -- capabilities tekton-get-pipeline-runs \
  --all-namespaces
```

Read Tekton TaskRuns without invoking the model:

```sh
cargo run -p pharness-cli -- capabilities tekton-get-task-runs \
  --all-namespaces
```

Analyze one Tekton PipelineRun and related TaskRuns:

```sh
cargo run -p pharness-cli -- capabilities tekton-analyze-pipeline-run \
  --namespace ci \
  --name build-app
```

The analysis response includes PipelineRun status, TaskRun status counts, task identities, pod names, repo URL, image reference, deployment target, commit SHA, image digest/image URL, Deployment rollout status, registry-aware image alignment, and Argo sync/health when those related resources can be read safely.

If Tekton and Deployments refer to the same registry through different hostnames, configure aliases on the API process or in `[cluster].registry_aliases`:

```sh
PHARNESS_REGISTRY_ALIASES=docker-registry.registry.svc.cluster.local:5000=registry.lucas.engineering
```

Configured host-equivalent image matches report `image_alignment.status` as `registry_alias_match`; unconfigured registry differences remain visible as `registry_mismatch`.

Secret-shaped reads should be denied before tool execution:

```sh
cargo run -p pharness-cli -- capabilities kubernetes-get \
  --resource secrets \
  --namespace argocd
```

For local Prometheus and Loki smoke tests, create temporary loopback URLs with port-forwarding:

```sh
kubectl -n monitoring port-forward svc/prometheus-server 19090:80
```

```sh
kubectl -n monitoring port-forward svc/loki 13100:3100
```

Start the API with:

```sh
PHARNESS_PROMETHEUS_URL=http://127.0.0.1:19090 \
PHARNESS_LOKI_URL=http://127.0.0.1:13100 \
cargo run -p pharness-api
```

Then run:

```sh
cargo run -p pharness-cli -- capabilities prometheus-query \
  --query up
```

Read bounded Prometheus target, rule, and alert inventory:

```sh
cargo run -p pharness-cli -- capabilities prometheus-inventory
```

Read bounded Loki log lines:

```sh
cargo run -p pharness-cli -- capabilities loki-log-summary \
  --query '{namespace="argocd"}' \
  --since-seconds 900 \
  --limit 25
```

Model-backed Tekton read:

```sh
cargo run -p pharness-cli -- run \
  --follow-events \
  --task "Use tekton_get_pipeline_runs and tekton_get_task_runs to inspect Tekton runs across all namespaces, then finish with one sentence summarizing whether any runs exist. If a concrete PipelineRun exists, use tekton_analyze_pipeline_run for that run. Do not mutate anything and do not read secrets." \
  --cwd "$PWD" \
  --timeout-ms 180000
```

## Patch File Tool

`patch_file` is exposed to the worker behind the same file-write approval policy as `write_file`. It accepts an exact UTF-8 replacement payload:

```json
{
  "find": "old text",
  "replace": "new text",
  "replace_all": false
}
```

By default the find text must match exactly once. Set `replace_all=true` only when replacing every match is intended.

```sh
cargo run -p pharness-cli -- run \
  --follow-events \
  --task "Use argo_get_app to inspect the Argo CD Application named ghost, then finish with one sentence summarizing its sync and health status. Do not mutate anything and do not read secrets." \
  --cwd "$PWD" \
  --timeout-ms 180000
```

## Approval CLI

List pending approvals:

```sh
cargo run -p pharness-cli -- approvals list
```

Filter approval queues by run scope:

```sh
cargo run -p pharness-cli -- approvals list \
  --namespace apps-dev \
  --repo git@example.test/team/pharness.git \
  --branch smoke/write-approval \
  --limit 25 \
  --offset 0
```

Fetch one approval:

```sh
cargo run -p pharness-cli -- approvals get --approval-id "$APPROVAL_ID"
```

Approve or deny the pending approval for a run:

```sh
cargo run -p pharness-cli -- approvals approve \
  --run-id "$RUN_ID" \
  --decided-by "$USER" \
  --reason "approved local write"
```

```sh
cargo run -p pharness-cli -- approvals deny \
  --run-id "$RUN_ID" \
  --decided-by "$USER" \
  --reason "not approved"
```

Approve or deny a specific pending approval:

```sh
cargo run -p pharness-cli -- approvals approve \
  --approval-id "$APPROVAL_ID" \
  --decided-by "$USER" \
  --reason "approved local write"
```

```sh
cargo run -p pharness-cli -- approvals deny \
  --approval-id "$APPROVAL_ID" \
  --decided-by "$USER" \
  --reason "not approved"
```

## Cluster Runtime (V2)

The runtime also deploys into the homelab Kubernetes cluster with one worker
Job per run attempt. The API stays the sole SQLite writer; worker attempts
report events and outcomes through token-gated `/api/internal/*` ingest
routes.

Worker dispatch is selected by `PHARNESS_WORKER_MODE`:

- `local` (default): the in-process worker, exactly as before.
- `kubernetes_job`: each run attempt executes in an isolated Job with a
  non-root security context, read-only root filesystem, an emptyDir
  workspace, and the read-only observer service account. Attempts exit at
  terminal states or approval pauses; approvals launch a resume Job that
  rehydrates from the persisted approval transcript.

### Kubernetes Coding Alpha

The Kubernetes coding path is intentionally disabled unless
`PHARNESS_WORKSPACE_ALLOWED_REMOTE_REPOS` contains an exact HTTPS repository
URL. A permitted WorkItem creates one bounded Job workspace, checks out the
requested ref, reports the full resolved Git object ID to the API, and only
then starts the model attempt. The worker cannot push, commit, open a pull
request, or deploy through a typed capability; it has no Git write identity,
and secret reads remain denied by policy.

On successful completion, the worker returns bounded Git status and binary
diff evidence. The API validates it against the issued workspace/repository/
branch contract and stores `workspace_git_diff` and `workspace_git_status`
artifacts. ChangeSet capture uses those artifacts, so it never depends on a
Job workspace surviving after the Job exits. See
[`planning/archive/runbooks/kubernetes-coding-alpha-smoke-playbook.md`](planning/archive/runbooks/kubernetes-coding-alpha-smoke-playbook.md)
for the reviewed activation path.

`POST /api/work-items/:id/reconcile` is the machine-facing controller step for
the coding alpha. It previews its exact next action by default and uses
`apply: true` to advance one safe state transition: declaring a WorkPlan and
workspace, starting an approved coding attempt, capturing a completed
ChangeSet, or preparing Git-delivery evidence. It stops at each review and
authorization boundary rather than treating a WorkItem as an unbounded chat.

A failed or blocked development WorkItem does not retry itself. Use
`POST /api/work-items/:id/replan` or `pharness-cli work-items replan` with a
non-empty reason to schedule one remaining attempt. Replan requires a still
approved WorkPlan, available attempt budget, and no captured ChangeSet; it
returns the WorkItem to `awaiting_approval`, clears any stale run link, and
records `work_item.replanned`. A subsequent controller reconcile is still the
operation that starts a fresh isolated workspace attempt.

Every terminal coding attempt also emits a `work_item.attempt_finished` audit
event. Its bounded classification distinguishes completed work, cancellation,
policy denial, turn-budget exhaustion, tool failure, missing model responses,
and unknown failures; it recommends a next operator/controller action without
starting another attempt.

An approved source ChangeSet can also produce a durable `git_delivery_plan`
artifact. It binds a future branch-and-PR operation to the repository, pinned
base commit, issued branch, and captured diff digest, but it does not perform
a Git write. An operator can then create a separate, idempotent
`supervised_autonomy` grant for the exact plan using
`change-sets authorize-git-delivery`. That grant is constrained to the single
repository, issued branch, WorkPlan, ChangeSet, and delivery-plan artifact.
`change-sets preflight-git-delivery` records the current authorization and
source checks as a durable `git_delivery_preflight` artifact. A preflight can
be `ready_for_writer` while `dispatch_ready` is `false`: that means the plan
and grant are correct, while the separate Git writer capability remains
intentionally absent until it has a scoped identity and revalidates the grant
immediately before each remote Git operation.

When explicitly enabled, `change-sets execute-git-delivery` dispatches a
short-lived, purpose-built Kubernetes Job. It receives only the current plan,
the captured diff, the API worker token, and a GitHub fine-grained token
mounted from a writer-only Secret. It verifies the pinned base commit, applies
the reviewed diff, creates the issued branch and commit, pushes without force,
and opens one GitHub pull request. The API records separate immutable
`git_delivery_execution` and `git_delivery_result` artifacts; opening a PR
does not mark a ChangeSet applied or bypass later merge/build/deployment gates.
The feature is disabled by default and supports GitHub HTTPS repositories
only. The historical direct smoke is retained at
[`planning/archive/runbooks/git-writer-pr-executor-smoke.md`](planning/archive/runbooks/git-writer-pr-executor-smoke.md).

### Tekton PipelineIntents

The first cluster mutation is intentionally narrow. An approved PipelineIntent
can create a bounded `supervised_autonomy` envelope, then return a structured
PipelineRun preview:

```sh
cargo run -p pharness-cli -- pipeline-intents execute \
  --pipeline-intent-id "$PIPELINE_INTENT_ID"
```

The preview is non-mutating. It reports parent-state, approval-gate, and
envelope checks plus the exact `tekton.dev/v1` manifest. `--apply` dispatches a
separate executor Job only when every check passes. That Job has no model
credentials and uses a dedicated service account with PipelineRun access only
in the namespaces configured by the Helm chart. It acknowledges submission,
then observes that exact PipelineRun until Tekton reports `Succeeded=True` or
`Succeeded=False`. A successful execution returns the PipelineIntent to its
approved authorization state and records terminal execution evidence; a failed
execution changes the intent to `failed`.

Applied intents expose `execution_state` on their API response. If the executor
does not report a durable terminal outcome, the API reaper marks the intent
failed and records the reconciliation in the audit log rather than leaving it
executing. `PHARNESS_TEKTON_EXECUTOR_POLL_SECONDS` controls the bounded worker's
observation interval; the executor Job deadline remains the hard upper bound.

Terminal execution evidence includes a compact executor receipt plus a bounded
`PipelineRunAnalysis` for the exact PipelineRun and its TaskRuns. The executor
does not read deployment, Argo CD, registry, logs, or secrets while collecting
that analysis. A DeploymentIntent can be drafted before analysis exists, but
approval requires satisfied analysis evidence. When the executor ran a
concrete PipelineRun, the attached analysis must match its namespace and name.

When the PipelineIntent includes a declared `deployment_handoff`, a successful
matching analysis can create one **proposed** DeploymentIntent for the exact
environment, namespace, and Argo CD Application. This is a durable planning
handoff, not a sync request: it does not approve or mutate the Application.
Deployment execution will require its own reviewed contract and executor
identity.

DeploymentContracts are operator-managed durable policy for one exact Argo CD
Application target. The initial schema admits only a non-pruning, non-forced
`sync` operation and has no mutation endpoint or additional Kubernetes RBAC:

```sh
pharness-cli deployment-contracts create \
  --target-environment homelab \
  --target-namespace pharness \
  --argo-application pharness \
  --contract-json '{"operation":"sync","prune":false,"force":false}' \
  --reason 'reviewed future sync boundary'
```

Each executable intent must also match exactly one active PipelineContract for
its namespace and PipelineRef. Contracts are created by an operator through
the API or CLI and enumerate the permitted parameter shapes and workspace
bindings; an unknown or missing input blocks execution before a Job is created.
For WorkItem delivery, the contract must additionally declare
`source_revision_param` as a required scalar parameter. Its execution value is
checked against Pharness's observed GitHub merge SHA, so a mutable branch
cannot be substituted as a build input.
Contracts can be retired but are never deleted; a retired-only target remains
blocked until one replacement contract is active.
Use the replacement command when changing a reviewed Pipeline schema so the
prior version retires and the next version activates atomically.

Deployment lives in `deploy/`:

- `deploy/docker/Dockerfile.runtime` builds `pharness-api` + `pharness-worker`.
- `deploy/docker/Dockerfile.ui` builds the operator console behind nginx with
  same-origin `/api` proxying and server-side operator identity injection.
- `deploy/helm/pharness` is deployed by the Argo CD Application registered in
  the `lucas_engineering` app-of-apps; images build through the shared Tekton
  `clone-build-push` pipeline.

Secrets are created out-of-band in the `pharness` namespace: see the header
comment in `deploy/helm/pharness/values.yaml`.

### Auth

When `PHARNESS_OPERATOR_TOKENS` (comma-separated `name=token` pairs) is set,
every operator route requires `Authorization: Bearer <token>`; `/health`
stays open and worker ingest uses its own `PHARNESS_WORKER_TOKEN`. The CLI
sends `PHARNESS_API_TOKEN` automatically:

```sh
PHARNESS_API_URL=http://127.0.0.1:14777 PHARNESS_API_TOKEN=<operator token> cargo run -p pharness-cli -- runs summary
```

Loopback local mode without configured tokens keeps the previous
auth-free behavior.

### Cluster Smoke

```sh
scripts/pharness-cluster-runtime-smoke.sh
```

Validates the deployed control plane end to end; see
[planning/archive/v2/v2-cluster-smoke-playbook.md](planning/archive/v2/v2-cluster-smoke-playbook.md).

### Controlled Tekton Execution Smoke

The bounded delivery smoke exercises the first real execution path without
touching an application. It creates ordinary audited SDLC records, preflights
an approved PipelineIntent, and, only with `--apply`, runs the GitOps-managed
`pharness-e2e-noop` Pipeline in `tekton-pipelines`. The fixture has no inputs
or application references and only writes a completion marker to its own task
log.

```sh
export PHARNESS_API_TOKEN='your operator token'
scripts/pharness-tekton-execution-smoke.sh --apply
```

The equivalent operator-console walkthrough, including the distinct preflight
and dispatch actions, is in
[planning/operations/runbooks/tekton-executor-smoke-playbook.md](planning/operations/runbooks/tekton-executor-smoke-playbook.md).

The same script can prove the digest-pinned build-output handoff after the
GitOps-managed synthetic fixture is synced:

```sh
PHARNESS_TEKTON_SMOKE_PIPELINE=pharness-e2e-build-output \
PHARNESS_TEKTON_SMOKE_EXPECT_BUILD_OUTPUT=1 \
scripts/pharness-tekton-execution-smoke.sh --apply
```

It writes only fixed synthetic `IMAGE_URL` and `IMAGE_DIGEST` result values;
it does not build, push, pull, inspect, or verify an image.

## Current Status

Pharness is a deployed supervised V3 SDLC control plane. It supports immutable
source preparation, isolated Fireworks coding Jobs, resumable budgets,
attempt-scoped workspace grants, source PR delivery, immutable Tekton builds,
digest-only GitOps PRs, explicitly approved Argo synchronization, deployment
verification, and prepared-but-never-automatic rollback. External effects and
PR merges remain explicitly supervised.

The stable operator-console baseline is the annotated tag
`v3-operator-cockpit`. The behavior-preserving `app.rs` decomposition is
complete and tagged `v3-decomposed-stable`; no implementation milestone is
currently active. See the [`planning/` documentation map](planning/README.md)
before starting the next milestone.
