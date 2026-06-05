# Current Build Review

Date: 2026-05-20

## What Exists

- Rust workspace with six crates:
  - `pharness-core`
  - `pharness-fireworks`
  - `pharness-store`
  - `pharness-api`
  - `pharness-cli`
  - `pharness-config`
- Fireworks-first model provider with streaming chat-completions support.
- Fireworks native tool-call mode is now the default worker protocol.
- Fireworks requests disable parallel tool calls; if a model still returns parallel calls, pharness keeps the first call and continues the one-action loop.
- Core agent runtime with one-action-per-turn loop, policy evaluation, cancellation, and event emission.
- Durable SQLite store for sessions, runs, events, approvals, tool calls, artifacts, file changes, resource refs, and context items.
- Pending write approvals persist preview JSON for `write_file` and `patch_file`, including generated diffs when the target is safe to preview.
- Machine-facing API:
  - `POST /api/runs`
  - `GET /api/runs`
  - `GET /api/runs/summary`
  - `GET /api/runs/:run_id`
  - `GET /api/runs/:run_id/events`
  - `GET /api/runs/:run_id/events/stream`
  - `GET /api/runs/:run_id/diff`
  - `GET /api/runs/:run_id/artifacts`
  - `GET /api/runs/:run_id/observations`
  - `POST /api/runs/:run_id/cancel`
  - `POST /api/runs/:run_id/approvals`
  - `GET /api/artifacts/:artifact_id`
  - `GET /api/observations`
  - `GET /api/observations/:observation_id`
  - `GET /api/incidents`
  - `GET /api/incidents/:incident_id`
  - `GET /api/remediation-plans`
  - `GET /api/remediation-plans/:plan_id`
  - `POST /api/work-plans/from-remediation-plan`
  - `GET /api/work-plans`
  - `GET /api/work-plans/:work_plan_id`
  - `GET /api/approval-gates`
  - `GET /api/approval-gates/summary`
  - `GET /api/approval-gates/:gate_id`
  - `POST /api/approval-gates/:gate_id/satisfy`
  - `POST /api/approval-gates/:gate_id/waive`
  - `POST /api/approval-gates/:gate_id/reject`
  - `GET /api/approvals`
  - `GET /api/approvals/:approval_id`
  - `POST /api/approvals/:approval_id/approve`
  - `POST /api/approvals/:approval_id/deny`
  - `GET /api/approvals` supports status, run-scope, requested-time, limit, and offset filters.
  - `GET /api/observations` supports run, source, kind, subject, normalized resource identity, observed-time, limit, and offset filters.
  - `GET /api/incidents` supports run, status, severity, normalized resource identity, created-time, limit, and offset filters.
  - `GET /api/remediation-plans` supports incident, run, status, risk, normalized resource identity, created-time, limit, and offset filters.
  - `GET /api/work-plans` supports remediation plan, incident, run, status, risk, normalized resource identity, created-time, limit, and offset filters.
  - `GET /api/approval-gates` supports remediation plan, incident, run, status, gate kind, risk, normalized resource identity, created-time, limit, and offset filters.
  - `GET /api/approval-gates/summary` includes status, gate kind, risk, age, resource identity, incident, and remediation plan buckets with matching non-pagination filters.
  - `GET /api/approvals/summary` includes status, kind, risk, age, and run-scope buckets, with matching requested-time filters.
  - `GET /api/permission-grants`
  - `POST /api/permission-grants`
  - `GET /api/permission-grants/:grant_id`
  - `POST /api/permission-grants/:grant_id/revoke`
  - `GET /api/audit-events`
  - `POST /api/capabilities/execute`
- CLI run submission:
  - `pharness-cli run --task ...`
  - `pharness-cli run --policy-mode trusted_writes --task ...`
  - `pharness-cli run --namespace ... --repo ... --branch ...`
  - JSON output suitable for Codex parsing.
- CLI run inspection:
  - `pharness-cli runs list`
  - `pharness-cli runs summary`
  - `pharness-cli runs cancel --run-id ...`
  - `pharness-cli runs cancel --run-id ... --with-events`
  - `pharness-cli runs get --run-id ...`
  - `pharness-cli runs get --run-id ... --with-events`
  - `pharness-cli runs diff --run-id ...`
- CLI artifact inspection:
  - `pharness-cli artifacts list --run-id ...`
  - `pharness-cli artifacts get --artifact-id ...`
- CLI observation inspection:
  - `pharness-cli observations list [--run-id ...] [--source ...] [--kind ...] [--subject ...]`
  - `pharness-cli observations list [--resource-namespace ...] [--resource-kind ...] [--resource-name ...]`
  - `pharness-cli observations get --observation-id ...`
- CLI incident inspection:
  - `pharness-cli incidents list [--status ...] [--severity ...] [--resource-namespace ...] [--resource-kind ...] [--resource-name ...]`
  - `pharness-cli incidents get --incident-id ...`
- CLI remediation plan inspection:
  - `pharness-cli remediation-plans list [--incident-id ...] [--status ...] [--risk-level ...] [--resource-namespace ...] [--resource-kind ...] [--resource-name ...]`
  - `pharness-cli remediation-plans get --plan-id ...`
- CLI work plan inspection:
  - `pharness-cli work-plans create-from-remediation-plan --remediation-plan-id ...`
  - `pharness-cli work-plans list [--remediation-plan-id ...] [--incident-id ...] [--status ...] [--risk-level ...]`
  - `pharness-cli work-plans get --work-plan-id ...`
- CLI approval gate inspection:
  - `pharness-cli approval-gates list [--remediation-plan-id ...] [--incident-id ...] [--status ...] [--gate-kind ...] [--risk-level ...]`
  - `pharness-cli approval-gates summary [--incident-id ...] [--status ...] [--gate-kind ...] [--risk-level ...]`
  - `pharness-cli approval-gates get --gate-id ...`
  - `pharness-cli approval-gates satisfy|waive|reject --gate-id ... --decided-by ... --reason ...`
- CLI permission grant inspection:
  - `pharness-cli permission-grants create ...`
  - `pharness-cli permission-grants list`
  - `pharness-cli permission-grants get --grant-id ...`
  - `pharness-cli permission-grants revoke --grant-id ...`
- CLI audit inspection:
  - `pharness-cli audit-events ...`
- CLI approval inspection and decisions:
  - `pharness-cli approvals list`
  - `pharness-cli approvals get --approval-id ...`
  - `pharness-cli approvals approve --run-id ...`
  - `pharness-cli approvals approve --approval-id ...`
  - `pharness-cli approvals deny --run-id ...`
  - `pharness-cli approvals deny --approval-id ...`
  - `pharness-cli approvals list --namespace ... --repo ... --branch ... --production-impacting false --limit ... --offset ...`
  - `pharness-cli approvals summary --namespace ... --repo ... --branch ... --production-impacting false`
- Typed read-only capabilities:
  - `kubernetes_get`, backed by `kubectl get -o json`
  - `argo_get_app`, backed by the Argo CD Application CRD through `kubectl`
  - `prometheus_query`
  - `prometheus_inventory`, backed by Prometheus targets, rules, and alerts APIs
  - `loki_log_summary`, backed by bounded Loki `query_range` reads
  - `tekton_get_pipeline_runs`, backed by the Tekton PipelineRun CRD through `kubectl`
  - `tekton_get_task_runs`, backed by the Tekton TaskRun CRD through `kubectl`
  - `tekton_analyze_pipeline_run`, backed by one PipelineRun read plus related TaskRuns by label
- Durable observations:
  - successful Kubernetes, Argo, Prometheus, Loki, and Tekton tool results create run-scoped `Observation` records
  - observations include source, kind, subject, summary, normalized resource identity, optional resource ref JSON, optional artifact id, and compact data JSON
- Durable incident candidates:
  - Tekton PipelineRun analysis observations create read-only `Incident` candidates when PipelineRun, Deployment, Argo, or image alignment signals indicate risk
  - incidents are queryable by status, severity, run id, and normalized resource identity
- Durable remediation plan drafts:
  - candidate incidents create conservative `RemediationPlan` drafts linked to incident, run, session, risk, and normalized resource identity
  - plans contain read-only evidence-gathering steps and explicit approval gates before any file, pipeline, cluster, or production-impacting mutation
- Durable work plans:
  - remediation plans can create idempotent non-executable `WorkPlan` records for the next SDLC control-plane handoff
  - WorkPlans preserve source plan steps, gates, risk, and resource identity, but `work_plan_json.execution.enabled` remains false
- Durable approval gates:
  - remediation plan draft gates are persisted as first-class `ApprovalGate` records for queueable, filterable review
  - gates can transition from pending to satisfied, waived, or rejected with durable audit events
  - gate decisions are governance/review state only; tool approval decisions still flow through the existing approval records
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
  - `trusted_writes` can be selected as a config default or per-run override to allow local file-write actions without an approval gate.
  - Policy identity includes `subject` and `environment`; the default environment is `local`.
  - Destructive/network commands ask by default.
  - Privileged and secret-accessing commands denied by default.
  - Typed cluster reads are allowed unless they look secret-accessing.
  - The selected policy is persisted in the run execution target and reused when the worker resumes an approved run.
- Run scope metadata:
  - Run creation accepts optional namespace, repo, branch, and production-impacting metadata.
  - The run scope is persisted in the execution target, included in run responses, and emitted on `run.queued`.
  - Empty run scope is normalized as absent: run responses omit or return `null`, and event/result payloads use `run_scope: null`.
  - Run scope does not authorize actions by itself; it only constrains approvals, audit context, and permission-grant matching.
- Durable permission grants:
  - SQLite persists `permission_grants` with subject, status, reason, scope JSON, policy JSON, expiry, and revocation metadata.
  - API and CLI can create, list, fetch, and revoke grants.
  - Active, unexpired grants are snapshotted onto new runs.
  - Matching grants can convert local `write_file` and `patch_file` approvals from `ask` to `allow` only when subject, environment, capability kind, action, risk ceiling, and any configured run-scope restrictions match.
  - `policy.evaluated` serializes the matching grant as `decision.grant_id`.
  - Grant creation, revocation, and use now emit durable audit events. Grant creation accepts `created_by` for audit actor attribution.
  - Approval approve/deny decisions and direct capability outcomes now emit durable audit events.
  - Direct capability audit outcomes are `direct_capability.executed`, `direct_capability.failed`, and `direct_capability.denied`.
  - Direct capability timeout cancellations emit `direct_capability.cancelled`.
  - Direct capability audit payloads include `executed` and policy decision. Successful rows include compact result summaries, not full tool payloads.
  - Grants do not override denials and do not grant shell, network, privileged, secret, destructive, registry, deployment, or production-mutation behavior.
- Runtime config:
  - `pharness-config` loads defaults, optional TOML config, and env overrides.
  - `/api/config/effective` exposes non-secret worker, cluster, and policy configuration.
  - `pharness-cli config validate --file ...` validates TOML locally and reports secret presence without exposing secret values.
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
- API exposes run-scoped and approval-id decision routes.
- Approval denial marks the approval `denied`, emits `approval.decided`, and completes the run as failed without executing the action.
- Approval grant marks the approval `approved`, emits `approval.decided`, resumes the local worker, executes the exact reviewed action payload, appends the tool result, and continues the model loop.
- Runtime emits `run.resumed` when continuing from approval.
- `request_approval` is not exposed in the default native worker tool schema. The model should call the concrete policy-gated action, such as `write_file`; policy creates the approval gate.
- CLI approval decisions now support `--wait` and `--follow-events` so write approval smokes can approve and wait for the resumed run without raw API polling.
- Live write-approval smoke passed with the CLI-only path: `approvals approve --wait`, `runs get --with-events`, and `runs diff`.
- Approval records now persist run scope metadata when present. `approval.required`, `approval.decided`, and `run.resumed` events include the same scope, so approval reviews remain tied to namespace, repo, branch, and production-impacting context.
- Approval decisions now also create durable audit events without storing full reviewed action payloads.
- Approvals are first-class API resources. Operators can fetch and decide a specific approval id, and stale approval ids are rejected if they are not the current pending approval for the run.
- Approvals now expose persisted preview JSON. Write and patch approvals include a pre-execution diff when possible; secret-shaped paths intentionally skip preview content and record an error preview instead.

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
- Revoking a permission grant currently affects future runs only because run policies snapshot active grants at creation time.
- No Git commit/push automation.
- No registry, database operator, Argo sync, release, or incident/remediation mutating capabilities yet.
- Remediation plans are durable drafts only. They are not executable and do not imply approval.
- No Tekton mutation yet. PipelineRun and TaskRun visibility are read-only.
- PipelineRun analysis now includes build outputs, deployment target, Deployment rollout correlation, registry-aware image alignment, and Argo sync/health. Bounded logs, test report parsing, and Prometheus correlation are not included yet.
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
- Prometheus inventory is now a separate bounded read-only action. It summarizes targets, rules, and active alerts without exposing rule query bodies or alert annotations.
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
- Both live analyses originally surfaced a registry-host mismatch because Tekton produced image URLs with the in-cluster registry hostname while the Deployments reference the external registry hostname.
- Registry alias normalization is now available through `PHARNESS_REGISTRY_ALIASES`. Exact string matches remain `exact_match`; configured host-equivalent matches are `registry_alias_match`; unconfigured host differences remain `registry_mismatch`.
- Live registry-alias smoke passed against `finance-frontend-run-6mwcl`: the homelab internal/external registry hosts normalized to `registry_alias_match`.

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
- `pharness-cli approvals list`, `pharness-cli approvals get`, and `pharness-cli approvals approve --run-id ...` were live-tested against a write approval; the run resumed and completed after approval.
- `GET /api/runs/:id/events/stream` now streams durable run events as SSE. A worker-disabled smoke run replayed `run.queued` and `run.cancelled`, and `Last-Event-ID` resumed from the next event.
- `GET /api/runs/:id/diff` now returns file-change rows and combined diff text. A live Fireworks write approval produced one stored file change and a retrievable diff containing the written content.
- Runtime config is now parsed once by `pharness-api`. The API auto-loads `config/pharness.toml` when present, honors `PHARNESS_CONFIG`, keeps env overrides authoritative, resolves Fireworks keys from env only, and injects configured cluster tools into direct capabilities and worker runs.
- Config-path smoke passed against a temporary TOML file: `/api/config/effective` reported `cluster.registry_alias_count = 1`, `prometheus_configured = false`, worker enabled, and no secret material.
- Runtime config now lives in the shared `pharness-config` crate so the API and CLI validate the same TOML/env semantics.
- `pharness-cli config validate --file ...` validates local TOML offline and reports only secret presence, not secret values.
- Direct capability execution now accepts a bounded `timeout_ms`. Timeout cancellation returns `status = cancelled`, marks the response and audit row as cancelled, and drops the underlying tool future.
- Approval summaries now return compact grouped queue counts without reviewed action payloads or preview diffs.
- `pharness-cli runs cancel` covers the run cancellation API without curl and can return the event log in the same JSON response.

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

1. Run the updated smoke playbook, including direct capability cancellation and persisted write approval previews.
   - Confirm `direct_capability.cancelled` appears when using a deliberately tiny direct capability timeout.
   - Confirm `approvals summary` returns pending `file_write` counts before approval.
   - Confirm `approvals get` returns `preview.status = ok` and a generated diff before approval.
   - Confirm the generated preview survives after the file is written.

2. Add stale-run duration buckets only if run summaries are not enough to spot stuck running or approval-required runs.
