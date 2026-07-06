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

The script port-forwards `svc/pharness-api` and `svc/pharness-ui`, resolves
the operator token from the cluster secret unless `PHARNESS_API_TOKEN` is
already exported, and writes artifacts to `target/cluster-smoke/<timestamp>`.

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

- Pending first full run against the deployed stack.
