# V2 Cluster Runtime Smoke Playbook

## Purpose

Prove the deployed cluster runtime meets the V2 Alpha cut line: runs execute
in isolated worker Jobs, durable state flows only through the API, operator
auth gates the control plane, and the operator console proxies with an
authenticated identity.

## Prerequisites

- kubectl context pointing at the homelab cluster with access to the
  `pharness` namespace.
- The pharness Argo CD Application synced and healthy.
- Secrets present in the `pharness` namespace:
  - `pharness-fireworks` (`api-key`): real key enables model-backed checks;
    the placeholder still validates worker failure ingest.
  - `pharness-worker-token` (`token`).
  - `pharness-operator-token` (`tokens`, `console-token`).
  - `pharness-console-basic-auth` (`auth`) when the ingress is enabled.
- Local Rust toolchain for the machine-facing CLI used by the deterministic
  contract checks.

## Run

```sh
scripts/pharness-cluster-runtime-smoke.sh
```

The script port-forwards `svc/pharness-api` and `svc/pharness-ui`, requires an
operator-supplied `PHARNESS_API_TOKEN`, and writes artifacts to
`target/cluster-smoke/<timestamp>`. Set `PHARNESS_SMOKE_MODEL_CHECKS=enabled`
only when an operator intends to use the configured Fireworks integration.

## Checks

1. `deployment/pharness-api` and `deployment/pharness-ui` roll out.
2. `/health` responds without auth; `/api/*` returns 401 without or with a
   wrong bearer; a valid operator token resolves `operator.name`.
3. `/api/config/effective` reports `worker.mode == kubernetes_job`.
4. The deterministic control-plane contract passes against the deployed API
   through `scripts/pharness-e2e-smoke.sh --external-api --no-model` with
   `PHARNESS_E2E_ALLOW_EXISTING_RUNS=1` (SDLC roots, downstream chain,
   readiness, flow aggregation, event stream cursor).
5. A submitted run produces a worker Job labelled
   `pharness.lucas.engineering/run-id=<run>`; the run reaches a durable
   terminal state reported by the worker through token-gated ingest.
   With a placeholder Fireworks key the expected terminal state is `failed`
   with a provider error, which still proves the ingest path end to end.
6. Cancelling a fresh run marks it `cancelled` and deletes its worker Job.
7. The console serves the app shell and its proxy authenticates as the
   console operator without a browser-held token.

## Verification

- `scripts/pharness-cluster-runtime-smoke.sh` passed with `model_checks: enabled` on 2026-07-07 after the real Fireworks key was set.
  - The model-backed lifecycle run completed in a worker Job (`kimi-k2p6`; the provider now returns 500 for `kimi-k2p5`, so the default model moved to `kimi-k2p6`).
  - Approval pause/resume verified end to end against the finance app: a run inspected the `finance-frontend` Argo CD Application through the worker's read-only service account, paused at `approval.required` for `write_file finance-observations.md` (initial Job exited), and an operator approval launched a resume Job that executed the write and finished. Durable events show `approval.required -> approval.decided -> run.resumed -> tool.finished -> run.finished`, and the persisted run diff contains the written markdown summary (finance-frontend Synced/Healthy).
  - A parallel read-only model run listed `apps-prod` pods and inspected `finance-frontend`, completing with persisted observations.
  - Artifact directory: `target/cluster-smoke/20260707T180157Z`.

- `scripts/pharness-cluster-runtime-smoke.sh` passed against the deployed stack on 2026-07-07.
  - All eight checks passed: rollout health, operator auth gating, kubernetes_job dispatcher config, the deterministic control-plane contract (all fourteen e2e checks through the deployed API), worker Job lifecycle, worker outcome ingest, cancellation deleting the worker Job, and console shell plus proxy identity.
  - The worker Job executed with the placeholder Fireworks key and reported `run.queued -> run.started -> model.request_started -> run.failed` with the provider's 401 through token-gated ingest, proving the attempt path end to end without a live model.
  - Cancellation marked the run cancelled and removed the worker Job within the poll window.
  - The console proxy authenticated as operator `lucas` without a browser-held token.
  - Artifact directory: `target/cluster-smoke/20260707T135513Z`.

- Environment note: fresh worker pods initially saw `connection refused` against the API service until kube-router's policy state included the new pod; the worker's startup context fetch now retries through that window. Confirmed by probing from a fresh pod (first attempt refused, second succeeded).

- Known follow-ups after this verification:
  - Add the `pharness.lucas.engineering` public hostname route in Cloudflare so the console ingress certificate can issue.
  - Add a GitHub webhook on the pharness repo pointing at the Tekton EventListener for push-triggered image builds; manual PipelineRuns work today.
