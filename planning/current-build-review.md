# Current Build Review

Date: 2026-05-16

## What Exists

- Rust workspace with five crates:
  - `pharness-core`
  - `pharness-fireworks`
  - `pharness-store`
  - `pharness-api`
  - `pharness-cli`
- Fireworks-first model provider with streaming chat-completions support.
- Fireworks native tool-call mode is now the default worker protocol.
- Core agent runtime with one-action-per-turn loop, policy evaluation, cancellation, and event emission.
- Durable SQLite store for sessions, runs, events, approvals, tool calls, artifacts, file changes, resource refs, and context items.
- Machine-facing API:
  - `POST /api/runs`
  - `GET /api/runs/:run_id`
  - `GET /api/runs/:run_id/events`
  - `GET /api/runs/:run_id/events/stream`
  - `GET /api/runs/:run_id/diff`
  - `GET /api/runs/:run_id/artifacts`
  - `POST /api/runs/:run_id/cancel`
  - `POST /api/runs/:run_id/approvals`
  - `GET /api/artifacts/:artifact_id`
  - `GET /api/approvals`
  - `POST /api/capabilities/execute`
- CLI run submission:
  - `pharness-cli run --task ...`
  - JSON output suitable for Codex parsing.
- CLI run inspection:
  - `pharness-cli runs get --run-id ...`
  - `pharness-cli runs get --run-id ... --with-events`
  - `pharness-cli runs diff --run-id ...`
- CLI artifact inspection:
  - `pharness-cli artifacts list --run-id ...`
  - `pharness-cli artifacts get --artifact-id ...`
- Typed read-only capabilities:
  - `kubernetes_get`, backed by `kubectl get -o json`
  - `argo_get_app`, backed by the Argo CD Application CRD through `kubectl`
  - `prometheus_query`
  - `tekton_get_pipeline_runs`, backed by the Tekton PipelineRun CRD through `kubectl`
  - `tekton_get_task_runs`, backed by the Tekton TaskRun CRD through `kubectl`
  - `tekton_analyze_pipeline_run`, backed by one PipelineRun read plus related TaskRuns by label
- Local filesystem tools:
  - `read_file`
  - `write_file`
  - `patch_file`
  - `list_dir`
  - `search_files`
- Local shell/git tools:
  - `run_shell`
  - `git_status`
  - `git_diff`
- Policy posture:
  - Read-only actions allowed.
  - File writes ask by default.
  - Destructive/network commands ask by default.
  - Privileged and secret-accessing commands denied by default.
  - Typed cluster reads are allowed unless they look secret-accessing.
- Future CRD vocabulary is now captured in the main implementation plan and ADR.

## Verified Fireworks Smoke Test

Date: 2026-05-15

Command shape:

```sh
CARGO_TARGET_DIR=target \
cargo run -p pharness-cli -- run \
  --task "List the top-level files in the workspace, then finish with a short summary." \
  --cwd "$PWD" \
  --timeout-ms 180000
```

Observed result:

- `wait_status` was `completed`.
- `run.status` was `completed`.
- Worker used Fireworks model `accounts/fireworks/models/kimi-k2p5`.
- The run took 2 turns:
  - turn 0 proposed `list_dir` for path `.` with depth `1`
  - policy allowed the read-only action
  - tool returned 23 entries
  - turn 1 proposed `finish`
- Final result JSON contained:
  - `status: completed`
  - `error: null`
  - a concise summary of the Rust workspace and five crates

Decision:

- Treat this as the first validated machine-facing run contract: `POST /api/runs` -> durable events -> native tool call -> policy decision -> tool result -> typed finish result.
- Keep the default worker on native Fireworks tool calls with required tool choice. JSON action mode remains useful as a fallback path, not the default smoke path.
- `write_file` and `patch_file` are now exposed through the default worker schema after approval decision, resume behavior, and diff persistence landed.

## Approval Resume Path

Implemented:

- Runtime captures `PendingApproval` with:
  - approval kind
  - risk level
  - summary
  - exact reviewed action payload
  - resumable message transcript
  - completed turn count
- Store persists pending approvals with the reviewed action and transcript.
- API exposes `POST /api/runs/:run_id/approvals`.
- Approval denial marks the approval `denied`, emits `approval.decided`, and completes the run as failed without executing the action.
- Approval grant marks the approval `approved`, emits `approval.decided`, resumes the local worker, executes the exact reviewed action payload, appends the tool result, and continues the model loop.
- Runtime emits `run.resumed` when continuing from approval.
- `request_approval` is not exposed in the default native worker tool schema. The model should call the concrete policy-gated action, such as `write_file`; policy creates the approval gate.
- CLI approval decisions now support `--wait` and `--follow-events` so write approval smokes can approve and wait for the resumed run without raw API polling.
- Live write-approval smoke passed with the CLI-only path: `approvals approve --wait`, `runs get --with-events`, and `runs diff`.

Current limitation:

- Approval resume is implemented for approvals that include a concrete reviewed action. A model-authored `request_approval` without an action is still a pause, not an executable resume, so it is intentionally hidden from the default worker schema.

## Failed Write Smoke Learning

The first write smoke attempt paused correctly but did not resume:

- The model called `request_approval` directly.
- That produced an approval with no reviewed `write_file` payload.
- The API correctly rejected approval with `pending approval has no reviewed action to resume`.

Decision:

- Remove `request_approval` from the default Fireworks native tool schema.
- Keep `request_approval` in the core `AgentAction` enum for fallback/future use.
- Instruct the model to call concrete available tools, and let policy create approval-required state.

## Patch File Path

Implemented:

- `patch_file` accepts a structured exact text patch: `find`, `replace`, and optional `replace_all`.
- The executor requires an existing UTF-8 file inside the workspace.
- Default replacement requires exactly one match. Multiple matches require `replace_all=true`.
- Patch writes are atomic through the same temp-file replacement path as `write_file`.
- Patch tool results include path, replacement count, byte count, and diff.
- File-change persistence captures patch diffs through the existing `tool.finished` diff extraction path.

Live smoke:

- A Fireworks-backed run proposed `patch_file`.
- Policy returned `ask` with approval kind `file_write`.
- Approval resumed the exact reviewed patch action.
- The run completed in two turns.
- `GET /api/runs/:id/diff` returned one change and the expected patched content.

## Intentional Gaps

- No TypeScript UI yet.
- No Kubernetes worker pod or CRD controller yet.
- No Git commit/push automation.
- No registry, database operator, Argo sync, release, or incident/remediation mutating capabilities yet.
- No Tekton mutation yet. PipelineRun and TaskRun visibility are read-only.
- PipelineRun analysis now includes build outputs, deployment target, Deployment rollout correlation, image alignment, and Argo sync/health. Bounded logs, test report parsing, and Prometheus correlation are not included yet.
- Smoke-test playbook now lives at `planning/pharness-smoke-playbook.md`.

## Cluster Dogfood

Date: 2026-05-16

Local prerequisites observed:

- Local `kubectl` is available.
- Current Kubernetes context is `lucas_engineering`.
- The cluster is reachable from the local machine.
- The `argocd` CLI is not installed locally, so Argo reads should use Kubernetes Application CRDs for V1.
- `PHARNESS_PROMETHEUS_URL` was not set, so Prometheus live dogfood remains pending.

Kubernetes read smoke:

- Task asked the worker to use `kubernetes_get` for pods in the `argocd` namespace.
- Events showed `action.proposed`, `policy.evaluated` with `allow`, `tool.started`, `tool.finished`, and `run.finished`.
- The run completed in two turns.
- Result summary: 5 running pods were found, and the data was read through a read-only `kubernetes_get` operation.

Dogfood finding:

- The first pod read produced a very large text payload because redaction ran before JSON parsing.
- Fix: cluster stdout is now parsed first, structurally redacted, and compacted into resource summaries before being persisted or passed back to the model.
- Command summaries now use executable names instead of local absolute executable paths.

Argo read smoke:

- `argo_get_app` was changed to read `applications.argoproj.io` through `kubectl` in the configured Argo namespace.
- Task asked the worker to inspect the `ghost` Argo CD Application.
- The run completed in two turns.
- Result summary: the `ghost` Application is `Synced` and `Healthy`.

Direct capability smoke:

- `pharness-cli capabilities kubernetes-get --resource pods --namespace argocd` returned `status: ok`, policy `allow`, `executed: true`, and compact pod summaries with `item_count: 5`.
- `pharness-cli capabilities argo-get-app --app ghost` returned `status: ok`, policy `allow`, `executed: true`, and compact Argo Application status showing `Synced` and `Healthy`.
- `pharness-cli capabilities kubernetes-get --resource secrets --namespace argocd` returned `status: denied`, policy `deny`, and `executed: false`.
- `pharness-cli capabilities prometheus-query --query up` returned `status: tool_error` because `PHARNESS_PROMETHEUS_URL` is not configured.
- `pipelineruns.tekton.dev` exists in the live cluster. A read-only inventory first proved empty-list handling, then two user-triggered PipelineRuns gave pharness real SDLC evidence to analyze.

Prometheus success smoke:

- `PHARNESS_PROMETHEUS_URL` was created locally by port-forwarding `service/prometheus-server` in `monitoring` to a loopback port.
- `pharness-cli capabilities prometheus-query --query up` returned `status: ok`, policy `allow`, `executed: true`, and `result_count: 33`.
- Prometheus responses are compacted to `result_count`, `results_truncated`, and at most 20 sample results before they enter event/model context.
- `pharness-cli capabilities prometheus-query --query kube_secret_info` returned `status: denied`, policy `deny`, and `executed: false`.
- A model-backed run used `prometheus_query` and completed with the summary that the `up` query returned 33 series.
- The model-backed Prometheus run created one `prometheus_tool_result` artifact. `GET /api/runs/:id/artifacts` and `GET /api/artifacts/:id` returned the persisted artifact with `result_count: 33`.

Tekton read slice:

- Added `tekton_get_pipeline_runs` as a typed read-only action.
- Added `tekton_get_task_runs` as a typed read-only action.
- Added `tekton_analyze_pipeline_run` as a typed read-only action that returns a normalized `PipelineRunAnalysis` shape.
- Direct API execution exposes it through `POST /api/capabilities/execute`.
- CLI exposes it as `pharness-cli capabilities tekton-get-pipeline-runs`.
- CLI exposes TaskRuns as `pharness-cli capabilities tekton-get-task-runs`.
- CLI exposes PipelineRun analysis as `pharness-cli capabilities tekton-analyze-pipeline-run`.
- Worker native tool schema exposes it to Fireworks-backed runs.
- Policy allows ordinary Tekton reads as low-risk typed cluster reads and denies secret-shaped namespaces, names, and label selectors before execution.
- Tekton tool results are persisted as `tekton_tool_result` artifacts.
- PipelineRun analysis results are persisted as `pipeline_run_analysis` artifacts.
- CLI exposes persisted artifacts as `pharness-cli artifacts list` and `pharness-cli artifacts get`.
- CLI exposes run reads and diffs as `pharness-cli runs get` and `pharness-cli runs diff`.
- Direct TaskRun smoke returned `status: ok`, `executed: true`, and `item_count: 0` against the live cluster.
- Secret-shaped TaskRun smoke returned `status: denied` and `executed: false`.
- Direct PipelineRun analysis smoke returned a structured `tool_error` for a missing PipelineRun after policy allowed the read-only capability.
- Secret-shaped PipelineRun analysis smoke returned `status: denied` and `executed: false`.
- Live PipelineRun analysis succeeded against two real PipelineRuns:
  - `finance-frontend-run-6mwcl` was `succeeded`, with 3 succeeded TaskRuns, commit, image URL, image digest, deployment target, healthy Deployment rollout, Argo app, and Argo sync/health captured.
  - `finance-app-db-service-run-jkx6k` moved from `running` during first analysis to `succeeded` after completion, with 3 succeeded TaskRuns, commit, image URL, image digest, deployment target, healthy Deployment rollout, Argo app, and Argo sync/health captured.
- Both live analyses surfaced `image_alignment.status: mismatch` because Tekton produced image URLs with the in-cluster registry hostname while the Deployments reference the external registry hostname. That is a useful rollout-evidence signal for later registry canonicalization work.

## Local Smoke Test Without Fireworks

This verifies API/store/CLI wiring and durable event logging. The run will remain `queued` because no local worker is configured without `FIREWORKS_API_KEY`.

```sh
PHARNESS_BIND=127.0.0.1:4777 \
PHARNESS_DB_PATH=target/pharness-smoke.db \
CARGO_TARGET_DIR=target \
cargo run -p pharness-api
```

In another terminal:

```sh
CARGO_TARGET_DIR=target \
cargo run -p pharness-cli -- run \
  --task "smoke test queued control plane" \
  --no-wait \
  --timeout-ms 1000
```

Expected result:

- CLI prints JSON.
- `run.status` is `queued`.
- `events[0].type` is `run.queued`.
- `events[0].payload.worker` is `disabled`.

## Fireworks Worker Smoke Test

This verifies the local worker path, provider call, event persistence, tool loop, and structured run result.

First list the serverless models visible to your Fireworks key:

```sh
CARGO_TARGET_DIR=target \
cargo run -p pharness-cli -- fireworks models
```

Choose one returned `models[].name` value for `PHARNESS_FIREWORKS_MODEL`. The API process reads this environment variable at startup, so restart the API after changing it.

```sh
PHARNESS_BIND=127.0.0.1:4777 \
PHARNESS_DB_PATH=target/pharness-worker-smoke.db \
FIREWORKS_API_KEY="$FIREWORKS_API_KEY" \
PHARNESS_FIREWORKS_MODEL="accounts/fireworks/models/kimi-k2p5" \
CARGO_TARGET_DIR=target \
cargo run -p pharness-api
```

Confirm the already-running API is using the intended model:

```sh
CARGO_TARGET_DIR=target \
cargo run -p pharness-cli -- config
```

In another terminal:

```sh
CARGO_TARGET_DIR=target \
cargo run -p pharness-cli -- run \
  --task "List the top-level files in the workspace, then finish with a short summary." \
  --cwd "$PWD" \
  --timeout-ms 180000
```

Expected result:

- CLI prints JSON.
- `run.status` becomes `completed`.
- Events include `run.started`, `model.request_started`, `model.response_finished`, `action.proposed`, `policy.evaluated`, `tool.started`, `tool.finished`, and `run.finished`.
- For the repo-listing task, expected actions are `list_dir` followed by `finish`.
- Secret-shaped reads should be denied or redacted.

If Fireworks returns `Model not found, inaccessible, and/or not deployed`, either the API server is still running with an old `PHARNESS_FIREWORKS_MODEL`, or the selected model is not visible to that API key. Stop and restart `pharness-api`, then re-run `pharness-cli config`. The current tested default is `accounts/fireworks/models/kimi-k2p5`.

```sh
cargo run -p pharness-cli -- config
cargo run -p pharness-cli -- fireworks models
```

## Optional Cluster Read Smoke Test

Use only when local `kubectl` and/or `PHARNESS_PROMETHEUS_URL` are configured safely.

```sh
cargo run -p pharness-cli -- run \
  --task "Use kubernetes_get to list pods in the argocd namespace, then finish with a short health summary." \
  --cwd "$PWD" \
  --timeout-ms 180000
```

```sh
cargo run -p pharness-cli -- run \
  --task "Use argo_get_app to inspect the Argo CD Application named ghost, then finish with one sentence summarizing its sync and health status." \
  --cwd "$PWD" \
  --timeout-ms 180000
```

Model-free direct capability checks:

```sh
cargo run -p pharness-cli -- capabilities kubernetes-get \
  --resource pods \
  --namespace argocd
```

```sh
cargo run -p pharness-cli -- capabilities argo-get-app \
  --app ghost
```

```sh
cargo run -p pharness-cli -- capabilities kubernetes-get \
  --resource secrets \
  --namespace argocd
```

Expected result:

- `kubernetes_get` is allowed by policy for non-secret resources.
- Attempts to read Secrets or secret-shaped names are denied before execution.

## Live Logging Check

Verified on 2026-05-15:

- `pharness-api` emits startup and access logs through `tracing`.
- `tower-http` logs request method, URI, status, and latency.
- `pharness-cli run --follow-events --no-wait` prints durable run events to stderr while preserving final JSON on stdout.
- Worker-disabled smoke run produced live `[1] run.queued` output and API access logs for `POST /api/runs` and event fetches.
- After switching the worker default model to `accounts/fireworks/models/kimi-k2p5`, a sourced-shell live run with no explicit `PHARNESS_FIREWORKS_MODEL` completed successfully in 2 turns.
- `pharness-cli approvals list` and `pharness-cli approvals approve --run-id ...` were live-tested against a write approval; the run resumed and completed after approval.
- `GET /api/runs/:id/events/stream` now streams durable run events as SSE. A worker-disabled smoke run replayed `run.queued` and `run.cancelled`, and `Last-Event-ID` resumed from the next event.
- `GET /api/runs/:id/diff` now returns file-change rows and combined diff text. A live Fireworks write approval produced one stored file change and a retrievable diff containing the written content.

Operator usage:

```sh
RUST_LOG=pharness_api=debug,tower_http=info cargo run -p pharness-api
```

```sh
cargo run -p pharness-cli -- run \
  --follow-events \
  --task "List the top-level files, then finish with one sentence." \
  --cwd "$PWD"
```

## Next Implementation Steps

1. Add CLI artifact commands only if operator workflow needs them.
   - The API retrieval path exists.
   - Codex can already call the API directly.

2. Start read-only Tekton/LGTM inventory planning.
   - Keep it typed and read-only.
   - Do not add mutation paths until approval gates, resource refs, and audit events are tighter.

3. Add patch preview if review ergonomics need it.
   - The current approval summary is enough for machine flow, but a UI/operator path should show generated diff before approval.
