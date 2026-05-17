# Decisions

- Add CLI artifact inspection as a V1 operator surface: `pharness-cli artifacts list` and `pharness-cli artifacts get`.
- Add CLI run inspection as a V1 operator surface: `pharness-cli runs get` and `pharness-cli runs diff`.
- Add `--wait` to approval decisions so write-approval smokes can approve and block until the resumed run reaches a terminal state.
- Keep artifact inspection read-only and API-backed. The CLI does not interpret artifact contents; it prints the persisted JSON so Codex and shell scripts can parse the same contract.
- Use this playbook as the current smoke path for pharness. It exercises the API, worker config, direct capabilities, policy denials, model-backed runs, approvals, artifacts, and Tekton SDLC analysis.
- Start the API with request logging visible in the terminal. That is the current live-log path for local dogfooding.
- Live write-approval smoke passed through the new CLI-only path: initial run paused at `approval_required`, `approvals approve --wait` returned a completed run, `runs get --with-events` returned 15 events, and `runs diff` returned one file change.

# Backlog

- Add a fixture-backed smoke mode later so CI can exercise the same contracts without a live cluster or Fireworks key.
- Add registry image identity normalization so internal and external registry hostnames can be compared without noisy `image_alignment` mismatches.

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

## Terminal 2: Start The API

Leave this running. This terminal is also your live API/request log.

```sh
rm -f target/pharness-playbook.db
PHARNESS_BIND=127.0.0.1:4777 \
PHARNESS_DB_PATH=target/pharness-playbook.db \
PHARNESS_FIREWORKS_MODEL=accounts/fireworks/models/kimi-k2p5 \
PHARNESS_PROMETHEUS_URL=http://127.0.0.1:19090 \
RUST_LOG=pharness_api=info,tower_http=info \
cargo run -p pharness-api
```

## Terminal 3: Common Environment

Run this once before the smoke commands below.

```sh
export PHARNESS_API_URL=http://127.0.0.1:4777
export CARGO_TARGET_DIR=target
mkdir -p target
```

## API Config

```sh
cargo run -p pharness-cli -- config | jq
```

Expected signal:

- Config shows worker enabled when `FIREWORKS_API_KEY` is set.
- Config model is `accounts/fireworks/models/kimi-k2p5` unless you override it.

## Direct Kubernetes Read

```sh
cargo run -p pharness-cli -- capabilities kubernetes-get \
  --resource pods \
  --namespace argocd | jq '{status, executed, item_count: .result.content.output.item_count}'
```

Expected signal:

- `status` is `ok`.
- `executed` is `true`.
- `item_count` is a number.

## Direct Secret Denial

```sh
cargo run -p pharness-cli -- capabilities kubernetes-get \
  --resource secrets \
  --namespace argocd | jq '{status, executed, error, decision}'
```

Expected signal:

- `status` is `denied`.
- `executed` is `false`.

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

## Model-Backed Tekton Run And Artifact Inspection

```sh
cargo run -p pharness-cli -- run \
  --follow-events \
  --task "Use tekton_get_pipeline_runs and tekton_get_task_runs to inspect Tekton runs across all namespaces. If a PipelineRun exists, use tekton_analyze_pipeline_run for one concrete PipelineRun. Finish with one sentence summarizing status, deployment health, and Argo sync. Do not mutate anything and do not read secrets." \
  --cwd "$PWD" \
  --timeout-ms 180000 | tee target/pharness-tekton-run.json
```

```sh
RUN_ID="$(jq -r '.run.id' target/pharness-tekton-run.json)"
cargo run -p pharness-cli -- artifacts list \
  --run-id "$RUN_ID" | tee target/pharness-tekton-artifacts.json
```

```sh
ARTIFACT_ID="$(jq -r '.artifacts[0].id // empty' target/pharness-tekton-artifacts.json)"
test -n "$ARTIFACT_ID"
cargo run -p pharness-cli -- artifacts get \
  --artifact-id "$ARTIFACT_ID" | jq '{id, kind, label, source: .content_json.source, resource: .content_json.resource}'
```

Expected signal:

- The model-backed run completes.
- Artifact list contains at least one cluster/Tekton artifact.
- `artifacts get` returns persisted JSON for the artifact.

## Write Approval Smoke

```sh
rm -f pharness-write-smoke.txt
cargo run -p pharness-cli -- run \
  --follow-events \
  --task "Create a file named pharness-write-smoke.txt in the workspace containing exactly: pharness approval smoke test. Then finish with a short summary." \
  --cwd "$PWD" \
  --timeout-ms 180000 | tee target/pharness-write-approval.json
```

Expected signal:

- The run stops at `approval_required`.
- Events include `approval.required`.

Approve and wait for the resumed run:

```sh
RUN_ID="$(jq -r '.run.id' target/pharness-write-approval.json)"
cargo run -p pharness-cli -- approvals list | jq
cargo run -p pharness-cli -- approvals approve \
  --run-id "$RUN_ID" \
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
  --with-events | jq '{run_status: .run.status, event_count: (.events | length)}'
cat pharness-write-smoke.txt
```

Expected signal:

- Final run status is `completed`.
- The file content is exactly `pharness approval smoke test`.

Inspect the diff:

```sh
cargo run -p pharness-cli -- runs diff \
  --run-id "$RUN_ID" | jq
```

## Cleanup

```sh
rm -f pharness-write-smoke.txt
```
