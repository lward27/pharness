# Pharness

Pharness is a lightweight, Fireworks-first agent harness for local coding workflows that is designed to grow into a Kubernetes-native autonomous delivery runtime.

V1 runs locally from a project root. It focuses on the agent loop, explicit tool actions, file and shell tooling, policy checks, approvals, event streaming, and durable sessions. V2 moves execution into a homelab Kubernetes cluster with isolated worker pods. V3 adds first-class typed capabilities for registry, Tekton, Argo CD, database operators, LGTM observability, and RAG-backed long-lived context.

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
- No MCP.
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

## Development Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Fireworks Model

When `FIREWORKS_API_KEY` is set, the local worker uses Fireworks with this default model:

```sh
accounts/fireworks/models/kimi-k2p5
```

Override it explicitly when trying another visible model:

```sh
PHARNESS_FIREWORKS_MODEL=accounts/fireworks/models/kimi-k2p5 cargo run -p pharness-api
```

The API reads provider configuration at startup. Restart `pharness-api` after changing `FIREWORKS_API_KEY` or `PHARNESS_FIREWORKS_MODEL`, then confirm with:

```sh
cargo run -p pharness-cli -- config
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

The API also exposes a Server-Sent Events stream for machine consumers:

```sh
curl -N "http://127.0.0.1:4777/api/runs/$RUN_ID/events/stream"
```

Resume after a known event with `Last-Event-ID`:

```sh
curl -N \
  -H "Last-Event-ID: evt_${RUN_ID}_1" \
  "http://127.0.0.1:4777/api/runs/$RUN_ID/events/stream"
```

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

Approval decisions can also wait for the resumed run to finish:

```sh
cargo run -p pharness-cli -- approvals approve \
  --run-id "$RUN_ID" \
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

The analysis response includes PipelineRun status, TaskRun status counts, task identities, pod names, repo URL, image reference, deployment target, commit SHA, image digest/image URL, Deployment rollout status, image alignment, and Argo sync/health when those related resources can be read safely.

Secret-shaped reads should be denied before tool execution:

```sh
cargo run -p pharness-cli -- capabilities kubernetes-get \
  --resource secrets \
  --namespace argocd
```

For local Prometheus smoke tests, create a temporary loopback URL with port-forwarding:

```sh
kubectl -n monitoring port-forward svc/prometheus-server 19090:80
```

Start the API with:

```sh
PHARNESS_PROMETHEUS_URL=http://127.0.0.1:19090 cargo run -p pharness-api
```

Then run:

```sh
cargo run -p pharness-cli -- capabilities prometheus-query \
  --query up
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

## Current Status

The local control-plane slice is running: API, CLI, Fireworks worker, durable events, approvals, SSE, file diffs, artifacts, and typed read-only Kubernetes/Argo/Prometheus/Tekton paths. See [planning/current-build-review.md](planning/current-build-review.md) for the current reviewed state and [planning/agent-harness-implementation-plan.md](planning/agent-harness-implementation-plan.md) for the full phased plan.
