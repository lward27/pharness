# Decisions

- Add CLI artifact inspection as a V1 operator surface: `pharness-cli artifacts list` and `pharness-cli artifacts get`.
- Add CLI run inspection as a V1 operator surface: `pharness-cli runs get`, `pharness-cli runs diff`, and `pharness-cli runs cancel`.
- Add CLI observation inspection as a V1 operator surface: `pharness-cli observations list` and `pharness-cli observations get`.
- Add `--wait` to approval decisions so write-approval smokes can approve and block until the resumed run reaches a terminal state.
- Add registry image identity normalization through `PHARNESS_REGISTRY_ALIASES`, so Tekton output images and Deployment images can match across known internal/external registry hostnames.
- Add parsed API runtime config. `config/pharness.toml` is auto-loaded when present, `PHARNESS_CONFIG` can point elsewhere, and env overrides still win for operator-critical runtime fields.
- Add offline config validation through `pharness-cli config validate --file ...`.
- Keep artifact inspection read-only and API-backed. The CLI does not interpret artifact contents; it prints the persisted JSON so Codex and shell scripts can parse the same contract.
- Use this playbook as the current smoke path for pharness. It exercises the API, worker config, direct capabilities, policy denials, model-backed runs, approvals, artifacts, and Tekton SDLC analysis.
- Start the API with request logging visible in the terminal. That is the current live-log path for local dogfooding.
- Live write-approval smoke passed through the new CLI-only path: initial run paused at `approval_required`, `approvals approve --wait` returned a completed run, `runs get --with-events` returned 15 events, and `runs diff` returned one file change.
- Prefer approval-id smoke commands now that approvals are first-class resources. Keep run-id approval commands available for compatibility.
- Add `prometheus_inventory` to the smoke path as the first bounded LGTM inventory check.
- Add `loki_log_summary` to the smoke path as the first bounded log-read check. It depends on `PHARNESS_LOKI_URL` and should return compacted, redacted log lines.
- Add direct capability audit checks for executed and denied outcomes.
- Use `--created-by` in permission-grant smoke tests so creation actor attribution is explicit.
- Add approval preview inspection to the write smoke. Pending write approvals should include persisted preview JSON with a generated diff before approval is decided.
- Add direct capability cancellation smoke with a deliberately tiny timeout. This validates API-level cancellation and `direct_capability.cancelled` audit rows.
- Add approval summary smoke beside approval list smoke so queues can be reviewed without fetching full approval payloads.
- Add remediation plan inspection beside incident inspection. Failed or degraded Tekton analysis should create conservative draft plans with approval gates, but zero plans is acceptable when the observed run is healthy.
- Add approval gate inspection beside remediation plan inspection. Draft plans should expose gate records without making them executable.
- Add `planning/trusted-envelope-smoke-playbook.md` as the focused update smoke for WorkPlan/ChangeSet trusted envelopes and scoped no-approval writes.

# Backlog

- Add a fixture-backed smoke mode later so CI can exercise the same contracts without a live cluster or Fireworks key.
- Add a config validation CLI command later so bad local TOML can be checked without starting the API.

# Playbook

Run every command from the repository root.

## Prerequisite Check

```sh
cargo --version
kubectl version --client
jq --version
```

If your Fireworks key is loaded from your shell profile:

```sh
source ~/.zshrc
test -n "$FIREWORKS_API_KEY"
```

## Terminal 1: Optional Prometheus Port-Forward

Leave this running if you want the Prometheus smoke to pass.

```sh
kubectl -n monitoring port-forward svc/prometheus-server 19090:80
```

## Terminal 2: Optional Loki Port-Forward

Leave this running if you want the Loki smoke to pass.

```sh
kubectl -n monitoring port-forward svc/loki 13100:3100
```

## Terminal 3: Start The API

Leave this running. This terminal is also your live API/request log.

```sh
rm -f target/pharness-playbook.db
PHARNESS_BIND=127.0.0.1:4777 \
PHARNESS_DB_PATH=target/pharness-playbook.db \
PHARNESS_FIREWORKS_MODEL=accounts/fireworks/models/kimi-k2p5 \
PHARNESS_PROMETHEUS_URL=http://127.0.0.1:19090 \
PHARNESS_LOKI_URL=http://127.0.0.1:13100 \
PHARNESS_REGISTRY_ALIASES=docker-registry.registry.svc.cluster.local:5000=registry.lucas.engineering \
RUST_LOG=pharness_api=info,tower_http=info \
cargo run -p pharness-api
```

## Terminal 4: Common Environment

Run this once before the smoke commands below.

```sh
export PHARNESS_API_URL=http://127.0.0.1:4777
export CARGO_TARGET_DIR=target
mkdir -p target
```

## Optional Config File Smoke

This verifies the parsed config path without changing your normal local config.

```sh
cat > target/pharness-smoke.toml <<'EOF'
[api]
bind = "127.0.0.1:4777"

[storage]
path = "target/pharness-playbook.db"

[model]
provider = "fireworks"
model = "accounts/fireworks/models/kimi-k2p5"
api_key_env = "FIREWORKS_API_KEY"
base_url = "https://api.fireworks.ai/inference/v1"

[cluster]
kubectl_bin = "kubectl"
argocd_namespace = "argocd"
prometheus_url = "http://127.0.0.1:19090"
loki_url = "http://127.0.0.1:13100"
registry_aliases = ["docker-registry.registry.svc.cluster.local:5000=registry.lucas.engineering"]
tool_timeout_ms = 15000
tool_max_output_bytes = 524288

[policy]
subject = "agent:local-worker"
environment = "local"
EOF
```

Validate it before starting the API:

```sh
cargo run -p pharness-cli -- config validate --file target/pharness-smoke.toml | jq
```

Expected signal:

- `status` is `ok`.
- `model.api_key_configured` is `true` when `FIREWORKS_API_KEY` is set.
- No API key value is printed.

To use that file instead of the env-heavy API start command, run Terminal 3 with:

```sh
rm -f target/pharness-playbook.db
PHARNESS_CONFIG=target/pharness-smoke.toml \
RUST_LOG=pharness_api=info,tower_http=info \
cargo run -p pharness-api
```

## API Config

```sh
cargo run -p pharness-cli -- config | jq
```

Expected signal:

- Config shows worker enabled when `FIREWORKS_API_KEY` is set.
- Config model is `accounts/fireworks/models/kimi-k2p5` unless you override it.
- Config shows `cluster.registry_alias_count` as `1` when using the aliases above.
- Config shows `policy.environment` as `local`.

## Direct Kubernetes Read

```sh
cargo run -p pharness-cli -- capabilities kubernetes-get \
  --resource pods \
  --namespace argocd | jq '{status, executed, item_count: .result.content.output.item_count}'
```

```sh
cargo run -p pharness-cli -- audit-events \
  --resource-kind capability \
  --resource-id kubernetes_get \
  | jq '.events[] | select(.kind == "direct_capability.executed") | {kind, executed: .payload.executed, source: .payload.result.source, item_count: .payload.result.output.item_count}'
```

Expected signal:

- `status` is `ok`.
- `executed` is `true`.
- `item_count` is a number.
- The audit event query returns at least one `direct_capability.executed` row with `executed = true`.
- The successful audit result is compact and does not include the full Kubernetes resource payload.

## Direct Capability Cancellation

This intentionally uses a tiny timeout against a real Kubernetes read. It should cancel before `kubectl` finishes.

```sh
cargo run -p pharness-cli -- capabilities kubernetes-get \
  --resource pods \
  --all-namespaces \
  --timeout-ms 1 | tee target/pharness-capability-cancelled.json
```

```sh
jq '{status, executed, cancelled, timeout_ms, error}' target/pharness-capability-cancelled.json
cargo run -p pharness-cli -- audit-events \
  --resource-kind capability \
  --resource-id kubernetes_get \
  | jq '.events[] | select(.kind == "direct_capability.cancelled") | {kind, executed: .payload.executed, cancelled: .payload.cancelled, timeout_ms: .payload.timeout_ms, error: .payload.error}'
```

Expected signal:

- The direct capability response shows `status` as `cancelled`.
- The direct capability response shows `executed` as `true`.
- The direct capability response shows `cancelled` as `true`.
- The audit event query returns at least one `direct_capability.cancelled` row.
- The cancelled audit row has `executed = true` and `cancelled = true`.

## Direct Secret Denial

```sh
cargo run -p pharness-cli -- capabilities kubernetes-get \
  --resource secrets \
  --namespace argocd | jq '{status, executed, error, decision}'
```

```sh
cargo run -p pharness-cli -- audit-events \
  --resource-kind capability \
  --resource-id kubernetes_get \
  | jq '.events[] | {kind, resource_kind, resource_id, executed: .payload.executed, decision: .payload.decision.decision, risk: .payload.decision.risk}'
```

Expected signal:

- The direct capability response shows `status` as `denied`.
- The direct capability response shows `executed` as `false`.
- The audit event query returns `kind` as `direct_capability.denied`.
- The audit event query returns `executed` as `false`.
- The audit event query returns `decision` as `deny`.

## Direct Argo Read

```sh
cargo run -p pharness-cli -- capabilities argo-get-app \
  --app ghost | jq '{status, executed, app: .result.content.output.metadata.name, health: .result.content.output.status.health.status, sync: .result.content.output.status.sync.status}'
```

Expected signal:

- `status` is `ok`.
- `health` and `sync` are populated.

## Direct Prometheus Read

Only run this if Terminal 1 is active.

```sh
cargo run -p pharness-cli -- capabilities prometheus-query \
  --query up | jq '{status, executed, result_count: .result.content.response.data.result_count}'
```

Expected signal:

- `status` is `ok`.
- `executed` is `true`.
- `result_count` is a number.

## Direct Prometheus Inventory

Only run this if Terminal 1 is active.

```sh
cargo run -p pharness-cli -- capabilities prometheus-inventory \
  | jq '{status, executed, targets: .result.content.inventory.targets.active_count, rules: .result.content.inventory.rules.rule_count, alerts: .result.content.inventory.alerts.alert_count}'
```

Expected signal:

- `status` is `ok`.
- `executed` is `true`.
- Target, rule, and alert counts are numbers.
- Inventory output omits rule query bodies and alert annotations.

## Direct Loki Log Summary

Only run this if Terminal 2 is active.

```sh
cargo run -p pharness-cli -- capabilities loki-log-summary \
  --query '{namespace="argocd"}' \
  --since-seconds 900 \
  --limit 25 | jq '{status, executed, streams: .result.content.response.data.stream_count, entries: .result.content.response.data.entry_count}'
```

Expected signal:

- `status` is `ok`.
- `executed` is `true`.
- Stream and entry counts are numbers.
- Log lines are bounded and secret-shaped lines are redacted.

## Prometheus Secret Denial

```sh
cargo run -p pharness-cli -- capabilities prometheus-query \
  --query kube_secret_info | jq '{status, executed, error, decision}'
```

Expected signal:

- `status` is `denied`.
- `executed` is `false`.

## Direct Tekton Inventory

```sh
cargo run -p pharness-cli -- capabilities tekton-get-pipeline-runs \
  --all-namespaces | tee target/pharness-pipelineruns.json | jq '{status, executed, item_count: .result.content.output.item_count}'
```

```sh
cargo run -p pharness-cli -- capabilities tekton-get-task-runs \
  --all-namespaces | jq '{status, executed, item_count: .result.content.output.item_count}'
```

Expected signal:

- Both commands return `status: ok`.
- PipelineRun and TaskRun item counts reflect the live cluster.

## Direct Tekton Analysis

Use the first PipelineRun from the inventory output.

```sh
PIPELINE_RUN="$(jq -r '.result.content.output.items[0].metadata.name' target/pharness-pipelineruns.json)"
PIPELINE_NAMESPACE="$(jq -r '.result.content.output.items[0].metadata.namespace' target/pharness-pipelineruns.json)"
echo "$PIPELINE_NAMESPACE/$PIPELINE_RUN"
```

```sh
cargo run -p pharness-cli -- capabilities tekton-analyze-pipeline-run \
  --namespace "$PIPELINE_NAMESPACE" \
  --name "$PIPELINE_RUN" | tee target/pharness-pipelinerun-analysis.json | jq '{status, executed, summary: .result.content.analysis.summary, outputs: .result.content.analysis.outputs, deployment: .result.content.analysis.deployment, argo_application: .result.content.analysis.argo_application}'
```

Expected signal:

- `status` is `ok`.
- `summary.status` is populated.
- `summary.task_run_count` is populated.
- `deployment.status` is `healthy`, `progressing`, `skipped`, or `error`.
- `argo_application.sync_status` is populated when the Deployment has Argo tracking.
- `summary.image_alignment.status` is `exact_match`, `registry_alias_match`, `registry_mismatch`, `mismatch`, or `unknown`. For the current homelab registry setup, Finance PipelineRuns should report `registry_alias_match` when `PHARNESS_REGISTRY_ALIASES` is set as shown above.

## Model-Backed Read Run

```sh
cargo run -p pharness-cli -- run \
  --follow-events \
  --task "List the top-level files in the workspace, then finish with one sentence." \
  --cwd "$PWD" \
  --timeout-ms 180000 | tee target/pharness-read-run.json
```

```sh
jq '{wait_status, run_status: .run.status, result: .run.result}' target/pharness-read-run.json
```

Expected signal:

- Events stream to stderr while the final JSON stays on stdout.
- `run_status` is `completed`.
- `result.status` is `completed`.

## Run Scope Metadata Smoke

```sh
cargo run -p pharness-cli -- run \
  --follow-events \
  --task "List the top-level files in the workspace, then finish with one sentence." \
  --cwd "$PWD" \
  --namespace apps-dev \
  --repo git@example.test/team/pharness.git \
  --branch smoke/run-scope \
  --timeout-ms 180000 | tee target/pharness-run-scope.json
```

```sh
jq '{run_status: .run.status, scope: .run.scope}' target/pharness-run-scope.json
jq '.events[0].payload.run_scope' target/pharness-run-scope.json
cargo run -p pharness-cli -- runs list \
  --status completed \
  --namespace apps-dev \
  --repo git@example.test/team/pharness.git \
  --branch smoke/run-scope \
  --production-impacting false \
  --limit 10 \
  --offset 0 | jq '{count, first: .runs[0] | {id, status, scope, started_at}}'
cargo run -p pharness-cli -- runs summary \
  --status completed \
  --namespace apps-dev \
  --repo git@example.test/team/pharness.git \
  --branch smoke/run-scope \
  --production-impacting false | jq '{total: .summary.total, status: .summary.by_status, age: .summary.by_age_bucket}'
```

Expected signal:

- Final run status is `completed`.
- `run.scope.namespace` is `apps-dev`.
- The first event payload includes the same `run_scope` metadata.
- `runs list` returns the completed scoped run with `started_at`.
- `runs summary` returns at least one completed scoped run and includes status and age buckets.
- Scope metadata does not imply any approval bypass or cluster mutation.

## Approval-Gated Run Cancellation Smoke

```sh
cargo run -p pharness-cli -- run \
  --follow-events \
  --task "Create a file named pharness-cancel-smoke.txt in the workspace containing exactly: pharness cancellation smoke test. Do not finish until the file is written." \
  --cwd "$PWD" \
  --namespace apps-dev \
  --repo git@example.test/team/pharness.git \
  --branch smoke/cancel \
  --timeout-ms 180000 | tee target/pharness-cancel-approval.json
```

```sh
RUN_ID="$(jq -r '.run.id' target/pharness-cancel-approval.json)"
jq '{run_status: .run.status, approval_id: .run.result.approval_id}' target/pharness-cancel-approval.json
cargo run -p pharness-cli -- runs cancel \
  --run-id "$RUN_ID" \
  --with-events | tee target/pharness-cancelled-run.json
jq '{run_status: .run.status, cancel_requested_at: .run.cancel_requested_at, event_types: [.events[].type]}' target/pharness-cancelled-run.json
```

Expected signal:

- The first command pauses at `approval_required`.
- `runs cancel --with-events` returns `run.status = cancelled`.
- `cancel_requested_at` is populated.
- Events include `approval.required` followed by `run.cancelled`.
- `pharness-cancel-smoke.txt` is not created.

## Model-Backed Tekton Run And Artifact Inspection

```sh
cargo run -p pharness-cli -- run \
  --follow-events \
  --task "Inspect Tekton runs across all namespaces using exactly one tool per turn. First call tekton_get_pipeline_runs. Then call tekton_get_task_runs. If a PipelineRun exists, call tekton_analyze_pipeline_run for one concrete PipelineRun. Finish with one sentence summarizing status, deployment health, and Argo sync. Do not mutate anything and do not read secrets." \
  --cwd "$PWD" \
  --timeout-ms 180000 | tee target/pharness-tekton-run.json
```

```sh
RUN_ID="$(jq -r '.run.id' target/pharness-tekton-run.json)"
cargo run -p pharness-cli -- artifacts list \
  --run-id "$RUN_ID" | tee target/pharness-tekton-artifacts.json
cargo run -p pharness-cli -- observations list \
  --run-id "$RUN_ID" | tee target/pharness-tekton-observations.json
cargo run -p pharness-cli -- observations list \
  --source tekton \
  --kind pipeline_run_analysis \
  --limit 10 \
  --offset 0 | tee target/pharness-tekton-observation-index.json
RESOURCE_NAMESPACE="$(jq -r '.observations[0].resource_namespace // empty' target/pharness-tekton-observation-index.json)"
RESOURCE_NAME="$(jq -r '.observations[0].resource_name // empty' target/pharness-tekton-observation-index.json)"
test -n "$RESOURCE_NAMESPACE"
test -n "$RESOURCE_NAME"
cargo run -p pharness-cli -- observations list \
  --source tekton \
  --resource-namespace "$RESOURCE_NAMESPACE" \
  --resource-kind PipelineRun \
  --resource-name "$RESOURCE_NAME" \
  --limit 10 \
  --offset 0 | tee target/pharness-tekton-observation-resource-index.json
cargo run -p pharness-cli -- incidents list \
  --resource-kind PipelineRun \
  --limit 10 \
  --offset 0 | tee target/pharness-incident-candidates.json
cargo run -p pharness-cli -- remediation-plans list \
  --resource-kind PipelineRun \
  --limit 10 \
  --offset 0 | tee target/pharness-remediation-plans.json
cargo run -p pharness-cli -- approval-gates list \
  --resource-kind PipelineRun \
  --limit 10 \
  --offset 0 | tee target/pharness-approval-gates.json
cargo run -p pharness-cli -- approval-gates summary \
  --resource-kind PipelineRun \
  --status pending | tee target/pharness-approval-gate-summary.json
```

```sh
ARTIFACT_ID="$(jq -r '.artifacts[0].id // empty' target/pharness-tekton-artifacts.json)"
test -n "$ARTIFACT_ID"
cargo run -p pharness-cli -- artifacts get \
  --artifact-id "$ARTIFACT_ID" | jq '{id, kind, label, source: .content_json.source, resource: .content_json.resource}'
OBSERVATION_ID="$(jq -r '.observations[0].id // empty' target/pharness-tekton-observations.json)"
test -n "$OBSERVATION_ID"
cargo run -p pharness-cli -- observations get \
  --observation-id "$OBSERVATION_ID" | jq '{id, source, kind, subject, resource_namespace, resource_kind, resource_name, artifact_id, observed_at}'
jq '{count, first: (.observations[0] | {id, run_id, source, kind, subject, resource_namespace, resource_kind, resource_name, artifact_id})}' target/pharness-tekton-observation-index.json
jq '{count, first: (.observations[0] | {id, run_id, source, kind, subject, resource_namespace, resource_kind, resource_name, artifact_id})}' target/pharness-tekton-observation-resource-index.json
jq '{count, first: (.incidents[0] // null)}' target/pharness-incident-candidates.json
jq '{count, first: (.remediation_plans[0] // null)}' target/pharness-remediation-plans.json
jq '{count, first: (.approval_gates[0] // null)}' target/pharness-approval-gates.json
jq '{total: .summary.total, status: .summary.by_status, gate_kind: .summary.by_gate_kind, age: .summary.by_age_bucket}' target/pharness-approval-gate-summary.json
INCIDENT_ID="$(jq -r '.incidents[0].id // empty' target/pharness-incident-candidates.json)"
if [ -n "$INCIDENT_ID" ]; then
  cargo run -p pharness-cli -- incidents get \
    --incident-id "$INCIDENT_ID" | jq '{id, observation_id, status, severity, title, resource_namespace, resource_kind, resource_name}'
fi
PLAN_ID="$(jq -r '.remediation_plans[0].id // empty' target/pharness-remediation-plans.json)"
if [ -n "$PLAN_ID" ]; then
  cargo run -p pharness-cli -- remediation-plans get \
    --plan-id "$PLAN_ID" | jq '{id, incident_id, status, risk_level, requires_approval, title, gates: (.plan_json.approval_gates | length)}'
  cargo run -p pharness-cli -- work-plans create-from-remediation-plan \
    --remediation-plan-id "$PLAN_ID" | tee target/pharness-work-plan.json
  WORK_PLAN_ID="$(jq -r '.work_plan.id' target/pharness-work-plan.json)"
  cargo run -p pharness-cli -- work-plans create-from-remediation-plan \
    --remediation-plan-id "$PLAN_ID" | tee target/pharness-work-plan-idempotent.json
  cargo run -p pharness-cli -- work-plans list \
    --remediation-plan-id "$PLAN_ID" \
    --limit 10 \
    --offset 0 | tee target/pharness-work-plans.json
  cargo run -p pharness-cli -- work-plans get \
    --work-plan-id "$WORK_PLAN_ID" | jq '{id, remediation_plan_id, incident_id, status, risk_level, execution: .work_plan_json.execution}'
  cargo run -p pharness-cli -- approval-gates list \
    --remediation-plan-id "$PLAN_ID" \
    --limit 10 \
    --offset 0 | tee target/pharness-plan-approval-gates.json
  GATE_ID="$(jq -r '.approval_gates[0].id // empty' target/pharness-plan-approval-gates.json)"
  if [ -n "$GATE_ID" ]; then
    cargo run -p pharness-cli -- approval-gates get \
      --gate-id "$GATE_ID" | jq '{id, remediation_plan_id, incident_id, status, gate_kind, gate_order, risk_level, title}'
    cargo run -p pharness-cli -- approval-gates satisfy \
      --gate-id "$GATE_ID" \
      --decided-by lucas \
      --reason "smoke lifecycle review" | tee target/pharness-approval-gate-satisfied.json
    cargo run -p pharness-cli -- audit-events \
      --resource-kind approval_gate \
      --resource-id "$GATE_ID" | tee target/pharness-approval-gate-audit.json
  fi
fi
```

Expected signal:

- The model-backed run completes.
- Artifact list contains at least one cluster/Tekton artifact.
- `artifacts get` returns persisted JSON for the artifact.
- Observation list contains at least one cluster/Tekton observation.
- Cross-run observation index contains the Tekton `pipeline_run_analysis` observation without needing the run id.
- Resource-filtered observation index returns the same Tekton `PipelineRun` analysis observation by `resource_namespace`, `resource_kind`, and `resource_name`.
- `observations get` returns source, kind, subject, normalized resource identity, optional artifact id, and observed time.
- Incident candidate list returns zero or more read-only candidates. Failed PipelineRun, unhealthy Deployment, degraded Argo, or image alignment mismatch observations should produce `status = candidate` incidents linked to their source observation.
- Remediation plan list returns zero or more draft plans. Candidate incidents should produce `status = draft`, `requires_approval = true` plans with approval gates; healthy observations may produce none.
- WorkPlan creation from a remediation plan returns `created = true` on first call, `created = false` on the second call, and `work_plan_json.execution.enabled = false`.
- Approval gate list returns zero or more pending gates. Draft plans should produce `status = pending` gate records; healthy observations may produce none.
- Approval gate summary returns compact grouped counts for the pending PipelineRun gate queue.
- If a gate exists, `approval-gates satisfy` returns `approval_gate.status = satisfied` and the audit query returns `approval_gate.satisfied`.

## Write Approval Smoke

```sh
rm -f pharness-write-smoke.txt
cargo run -p pharness-cli -- run \
  --follow-events \
  --task "Create a file named pharness-write-smoke.txt in the workspace containing exactly: pharness approval smoke test. Then finish with a short summary." \
  --cwd "$PWD" \
  --namespace apps-dev \
  --repo git@example.test/team/pharness.git \
  --branch smoke/write-approval \
  --timeout-ms 180000 | tee target/pharness-write-approval.json
```

Expected signal:

- The run stops at `approval_required`.
- Events include `approval.required`.
- `run.scope.namespace` and the `approval.required` event scope are both `apps-dev`.
- Scoped `approvals list` returns the pending approval with pagination metadata.
- Scoped `approvals summary` returns one pending approval grouped by status, kind, risk, age, and namespace.
- `approvals get` returns a persisted `preview` object for the pending file-write approval.

```sh
jq '{run_status: .run.status, scope: .run.scope}' target/pharness-write-approval.json
jq '.events[] | select(.type == "approval.required") | .payload.run_scope' target/pharness-write-approval.json
```

Approve and wait for the resumed run:

```sh
RUN_ID="$(jq -r '.run.id' target/pharness-write-approval.json)"
APPROVAL_ID="$(jq -r '.run.result.approval_id' target/pharness-write-approval.json)"
cargo run -p pharness-cli -- approvals list | jq
cargo run -p pharness-cli -- approvals list \
  --namespace apps-dev \
  --repo git@example.test/team/pharness.git \
  --branch smoke/write-approval \
  --production-impacting false \
  --limit 10 \
  --offset 0 | jq
cargo run -p pharness-cli -- approvals summary \
  --namespace apps-dev \
  --repo git@example.test/team/pharness.git \
  --branch smoke/write-approval \
  --production-impacting false | tee target/pharness-write-approval-summary.json | jq
jq -e '.summary.total >= 1 and (.summary.by_status[] | select(.value == "pending" and .count >= 1)) and (.summary.by_kind[] | select(.value == "file_write" and .count >= 1)) and (.summary.by_age_bucket[] | select(.value == "lt_5m" and .count >= 1))' target/pharness-write-approval-summary.json
NOW_MS="$(python3 - <<'PY'
import time
print(int(time.time() * 1000))
PY
)"
FIVE_MIN_AGO_MS="$((NOW_MS - 300000))"
cargo run -p pharness-cli -- approvals summary \
  --namespace apps-dev \
  --repo git@example.test/team/pharness.git \
  --branch smoke/write-approval \
  --production-impacting false \
  --requested-after-ms "$FIVE_MIN_AGO_MS" | jq
cargo run -p pharness-cli -- approvals get \
  --approval-id "$APPROVAL_ID" | tee target/pharness-write-approval-detail.json | jq
jq '{status, action: .preview.action, preview_status: .preview.status, path: .preview.path, diff: .preview.diff}' target/pharness-write-approval-detail.json
jq -e '.preview.status == "ok" and .preview.action == "write_file" and (.preview.diff | contains("pharness approval smoke test"))' target/pharness-write-approval-detail.json
cargo run -p pharness-cli -- approvals approve \
  --approval-id "$APPROVAL_ID" \
  --decided-by lucas \
  --reason "write smoke test approved" \
  --wait \
  --follow-events \
  --timeout-ms 180000 | tee target/pharness-write-approved.json
```

```sh
jq '{wait_status, run_status: .run.status, result: .run.result}' target/pharness-write-approved.json
cargo run -p pharness-cli -- runs get \
  --run-id "$RUN_ID" \
  --with-events | tee target/pharness-write-approved-run.json | jq '{run_status: .run.status, scope: .run.scope, event_count: (.events | length)}'
jq '.events[] | select(.type == "approval.decided" or .type == "run.resumed") | {type, run_scope: .payload.run_scope}' target/pharness-write-approved-run.json
cargo run -p pharness-cli -- audit-events \
  --run-id "$RUN_ID" | jq
cat pharness-write-smoke.txt
```

Expected signal:

- Final run status is `completed`.
- The file content is exactly `pharness approval smoke test`.
- Approval summary includes at least one pending `file_write` approval before approval.
- Approval summary includes `by_age_bucket` with the fresh approval in `lt_5m`.
- Approval summary accepts `--requested-after-ms` and can narrow the queue to fresh gates.
- Approval detail includes a persisted `preview` with `status = ok`, `action = write_file`, and a diff containing the requested file content.
- `approval.decided` and `run.resumed` include the run scope.
- Audit events include `approval.approved`.

Inspect the diff:

```sh
cargo run -p pharness-cli -- runs diff \
  --run-id "$RUN_ID" | jq
```

## Trusted Local Write Smoke

```sh
rm -f pharness-trusted-write-smoke.txt
cargo run -p pharness-cli -- run \
  --follow-events \
  --policy-mode trusted_writes \
  --task "Create a file named pharness-trusted-write-smoke.txt in the workspace containing exactly: pharness trusted write smoke test. Then finish with a short summary." \
  --cwd "$PWD" \
  --timeout-ms 180000 | tee target/pharness-trusted-write.json
```

Expected signal:

- Final run status is `completed`.
- Events include `policy.evaluated` with `decision="allow"` for `write_file`.
- Events do not include `approval.required`.
- `run.scope` and `run.result.run_scope` are `null` because no run-scope flags were provided.
- The file content is exactly `pharness trusted write smoke test`.

```sh
jq '{wait_status, run_status: .run.status, result: .run.result}' target/pharness-trusted-write.json
jq -e '.run.scope == null and .run.result.run_scope == null' target/pharness-trusted-write.json
jq '[.events[] | select(.type == "approval.required")] | length' target/pharness-trusted-write.json
cat pharness-trusted-write-smoke.txt
```

## Permission Grant Smoke

```sh
cargo run -p pharness-cli -- permission-grants create \
  --subject agent:local-worker \
  --created-by lucas \
  --reason "scoped trusted local write smoke" \
  --policy-mode trusted_writes \
  --scope-json '{"environment":"local","capability_kinds":["filesystem"],"actions":["write_file","patch_file"],"max_risk":"medium","namespaces":["apps-dev"],"repos":["git@example.test/team/pharness.git"],"branches":["smoke/permission-grant"],"production_impacting":false}' \
  | tee target/pharness-permission-grant.json
```

```sh
GRANT_ID="$(jq -r '.id' target/pharness-permission-grant.json)"
cargo run -p pharness-cli -- permission-grants list | jq
cargo run -p pharness-cli -- permission-grants get --grant-id "$GRANT_ID" | jq
cargo run -p pharness-cli -- audit-events \
  --resource-kind permission_grant \
  --resource-id "$GRANT_ID" | jq
rm -f pharness-granted-write-smoke.txt
cargo run -p pharness-cli -- run \
  --follow-events \
  --task "Create a file named pharness-granted-write-smoke.txt in the workspace containing exactly: pharness permission grant smoke test. Then finish with a short summary." \
  --cwd "$PWD" \
  --namespace apps-dev \
  --repo git@example.test/team/pharness.git \
  --branch smoke/permission-grant \
  --timeout-ms 180000 | tee target/pharness-granted-write.json
jq '{wait_status, run_status: .run.status, result: .run.result}' target/pharness-granted-write.json
jq '[.events[] | select(.type == "approval.required")] | length' target/pharness-granted-write.json
jq --arg grant_id "$GRANT_ID" '[.events[] | select(.type == "policy.evaluated" and .payload.decision.grant_id == $grant_id)] | length' target/pharness-granted-write.json
RUN_ID="$(jq -r '.run.id' target/pharness-granted-write.json)"
cargo run -p pharness-cli -- audit-events \
  --run-id "$RUN_ID" | jq
cat pharness-granted-write-smoke.txt
cargo run -p pharness-cli -- permission-grants revoke \
  --grant-id "$GRANT_ID" \
  --revoked-by lucas \
  --reason "smoke complete" \
  | tee target/pharness-permission-grant-revoked.json
cargo run -p pharness-cli -- audit-events \
  --resource-kind permission_grant \
  --resource-id "$GRANT_ID" | jq
```

Expected signal:

- Create returns `status = active`.
- List includes the new grant before revoke.
- The initial permission-grant audit query shows `permission_grant.created` with `actor = lucas`.
- The granted write run completes under the default policy mode and default `local` policy environment when its run scope matches the grant scope.
- The granted write run has zero `approval.required` events.
- The granted write run has at least one `policy.evaluated` event with `decision.grant_id` equal to the created grant id.
- Audit events include `permission_grant.created`, `permission_grant.used`, and `permission_grant.revoked`.
- `permission_grant.used` includes the matching run id and run scope.
- `permission_grant.revoked` appears later than create/use and applies to future runs only.
- The file content is exactly `pharness permission grant smoke test`.
- Revoke returns `status = revoked` and `revoked_by = lucas`.
- Revoke affects future runs. The just-created run remains reproducible because active grants are snapshotted when the run is created.

## Cleanup

```sh
rm -f pharness-write-smoke.txt pharness-trusted-write-smoke.txt pharness-granted-write-smoke.txt
```
