# V2 Hand-Off (2026-07-06 through 2026-07-08)

Follow-up (2026-07-10): the first P2 console slice now exists locally. Scope
selectors drive server-side filters, audit search/filtering is live, and audit
payloads expand in place. See [v2-continuation.md](v2-continuation.md). Cluster
rollout is still pending.

One-page summary of the work that took Pharness from a laptop-only V1 to the
deployed, verified V2 cluster runtime plus the first two console improvement
passes. Deeper detail lives in the linked planning docs.

## What Shipped

- **Repo baseline.** The operator UI moved into this repo as `ui/`
  (monorepo), and the verified V1 state was tagged `v1-baseline`. The
  generated `outputs/` tree was untracked after Argo CD rejected the repo
  over an out-of-bounds symlink inside it.

- **Worker extraction (`crates/pharness-runhost`, `crates/pharness-worker`).**
  One run attempt is hosted generically over an `AttemptBackend`: the
  in-process local worker and the new worker binary share one loop. The API
  stays the sole SQLite writer; workers report events and outcomes through
  token-gated `/api/internal/runs/:id/*` routes. Approval previews are
  computed worker-side. Attempts exit at terminal states or approval pauses;
  approvals launch a resume Job that rehydrates from the persisted
  transcript.

- **Kubernetes dispatch (`crates/pharness-api/src/dispatch.rs`).**
  `RunDispatcher` selects `local` or `kubernetes_job` mode
  (`PHARNESS_WORKER_MODE`, `[worker]` config). Job-per-attempt manifests are
  applied by shelling kubectl (deliberate: matches the repo's typed-shell
  pattern; kube-rs avoided). Non-root, read-only rootfs, emptyDir workspace,
  secret-sourced env. A 30s reaper fails runs whose Jobs die silently; late
  outcomes cannot overwrite terminal runs. Cancellation deletes the Job.

- **Packaging and GitOps (`deploy/`).** One runtime image
  (`pharness-api` + `pharness-worker` entrypoints, kubectl + git included)
  and one console image (unprivileged nginx, same-origin `/api` proxy).
  The Helm chart carries the API with SQLite PVC (Recreate strategy), Job
  RBAC, the read-only worker observer ClusterRole (no secrets),
  NetworkPolicies, and the basic-auth-gated console ingress. Registered in
  `lucas_engineering`: root-app Application, tekton-ci service entries,
  trigger-build.sh entries. Namespace `pharness`; secrets created
  out-of-band (see `deploy/helm/pharness/values.yaml` header).

- **Auth.** `PHARNESS_OPERATOR_TOKENS` (name=token pairs) gates all operator
  routes; `/health` stays open; worker ingest has its own token. The CLI
  sends `PHARNESS_API_TOKEN`; the console injects the operator bearer
  server-side through an nginx template so EventSource works without
  browser-held tokens. `decided_by` resolves from the authenticated
  identity. Console + operator credentials: `.pharness/console-credentials.txt`
  (gitignored, local only).

- **Verification.** `scripts/pharness-cluster-runtime-smoke.sh` wraps the
  e2e smoke in `--external-api` mode (all fourteen control-plane checks ran
  against the deployed API) and adds Job lifecycle, outcome ingest,
  cancellation, auth gating, and console identity checks. Passed with model
  checks enabled; approval pause/resume proven end to end against the
  finance app (write approved by the operator resumed in a second Job).
  Results in [v2-cluster-smoke-playbook.md](v2-cluster-smoke-playbook.md).
  Tagged `v2-alpha`.

- **Console P0 + P1 passes.** P0 fixed the trust-breakers found in live
  operation (stat-card field mismatch, phantom risk column, misleading
  pills, empty gate panel, hardcoded rail count, stale copy; the approvals
  API gained requested/decided metadata). P1 added hash deep links to every
  surface, the Flow root picker, live Incidents / Remediation Plans /
  Observations views, gates grouped by remediation plan, and clickable
  resource ids across audit, timeline, and evidence. Plan and verification:
  [ui-v2-improvements.md](ui-v2-improvements.md). P2 (scope filters, audit
  search) and P3 (App.jsx split) remain.

## Incidents Hit and Resolved

- Rust image build evicted on node 1 (chronic disk pressure); builds pin to
  `ubuntu-lucas-engineering-2` (`scripts/pharness-build.sh --node`).
- kube-router policy-sync race gave fresh worker pods a transient
  `connection refused` against the API service; the worker startup context
  fetch retries through it.
- Fireworks began returning 500 for `kimi-k2p5`; default model moved to
  `kimi-k2p6` (chart value + config default).
- An ad-hoc zsh heredoc expanded `$IMG:latest` as `${IMG:l}` + `atest` and
  silently pushed images to `pharness-uiatest` / `pharness-runtimeatest`.
  Recovered via in-cluster skopeo copy; `scripts/pharness-build.sh` (bash,
  literal refs) is now the only sanctioned manual build path. The junk
  repositories await a registry GC pass.

## Operating It

- Build images: `scripts/pharness-build.sh <runtime|ui|all>` (includes the
  rollout restart). Local checks: `cargo fmt/clippy/test`, then
  `scripts/pharness-e2e-smoke.sh --no-model`; deployed checks:
  `scripts/pharness-cluster-runtime-smoke.sh`.
- CLI against the deployed API: port-forward `svc/pharness-api` and export
  `PHARNESS_API_URL` + `PHARNESS_API_TOKEN`.
- Console: https://pharness.lucas.engineering (ingress basic auth).

## Open Items

- GitHub webhook on this repo → Tekton EventListener for push-triggered
  builds (TriggerTemplates already live; manual builds work today).
- Registry GC for the two mistagged repositories; node 1 disk headroom.
- Finish the remaining console P2 cluster-mode affordances and P3 module split
  per [ui-v2-improvements.md](ui-v2-improvements.md); deploy the completed P2
  scope/search slice.
- Next phase: first typed mutation slice (`tekton_trigger_pipeline` for
  approved PipelineIntents behind gates and envelopes), per the follow-on in
  [v2-cluster-runtime-plan.md](v2-cluster-runtime-plan.md). The worker RBAC
  design already reserves the seam.
