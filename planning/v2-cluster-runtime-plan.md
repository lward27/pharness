# V2 Cluster Runtime Plan (Phase 10 / V2 Alpha)

## Why This Is The Next Phase

- Phases 0 through 7 are complete and verified by `scripts/pharness-e2e-smoke.sh` (deterministic, model, and cluster modes).
- Phase 8 is substantially complete: the sibling UI is a live API-backed operations console for Flow, Queue, Run Detail with SSE, Tool Approvals, Approval Gates, Audit, and WorkPlans.
- The SDLC control plane (WorkPlan through RegistryEvidence, readiness, staleness, gates, evidence attachment) is complete as durable review-state. Nothing executes yet, by design.
- The master plan gates all mutation capabilities behind V2 isolation: worker pods, scoped service accounts, namespace-scoped RBAC. The missing prerequisite for executing approved intents is cluster-native identity and isolation, not more control-plane modeling.
- The read-only capability half of Phase 10 (Kubernetes, Argo, Tekton, Prometheus, Loki reads) already shipped in V1, so the remaining scope is bounded: containerize, split the worker out of the API process, orchestrate worker Jobs, and deploy through the existing homelab GitOps flow.
- Deploying Pharness itself through Tekton and Argo CD makes its own build and rollout observable through its own read-only capabilities, which is the strongest available dogfood and produces real data for the UI surfaces that are currently empty.

## Current Baseline

- `ExecutionTarget` already models `LocalProcess` and `KubernetesJob`; only `LocalProcess` is wired.
- `LocalWorker` in `pharness-api` owns run execution in-process via `spawn_run`, `resume_run`, and `cancel`; approval pauses persist `resume_messages_json`, so a run attempt can end at `approval_required` and a later attempt can rehydrate.
- The API is the sole SQLite writer. Queued runs already persist when no worker can execute them.
- No Dockerfiles, no deploy manifests, no auth on the API (loopback assumption).
- The pharness repo has substantial uncommitted work; the UI directory is not a git repository.

## Decisions

- Treat repo hygiene as stage zero, not housekeeping.
  - Commit the current working tree (code, migrations 0017 through 0020, planning docs) before any V2 work.
  - Move `pharness-ui` into the pharness repo as `ui/`, matching the repo structure proposed in the master plan. The UI has no git history to lose, and image builds plus GitOps want one versioned root.
  - Re-run the deterministic and cluster smokes, then tag the result as the V1 baseline.

- Extract a `pharness-worker` binary that executes one run attempt, and keep the API as the only store writer.
  - The worker receives `PHARNESS_API_URL`, `PHARNESS_RUN_ID`, `PHARNESS_WORKER_TOKEN`, and attempt kind (initial or resume) through env.
  - The worker fetches the run, policy snapshot, and resume state from the API, executes the agent loop, and reports events, diffs, artifacts, approvals, and terminal state through authenticated internal ingest endpoints.
  - Reuse the existing agent runtime crate; the worker is a thin host, not a rewrite.
  - SQLite plus a PVC stays the store for alpha with a single API replica. Postgres remains the documented scale-up path, not an alpha dependency.

- Map worker lifecycle to run attempts, not run lifetimes.
  - One Kubernetes Job per attempt. The Job exits at terminal state or at `approval_required`.
  - Approval decisions trigger a fresh attempt Job that rehydrates from persisted `resume_messages_json`, mirroring the existing `resume_run` seam.
  - Cancellation deletes the Job and marks the run cancelled. Pod death without a reported terminal state marks the run failed with a visible event.

- Dispatch on `ExecutionTarget` inside the API.
  - `LocalProcess` keeps today's in-process behavior for laptop use.
  - `KubernetesJob` creates, lists, and deletes Jobs by shelling `kubectl` with the pod service account, matching how the typed read-only cluster capabilities already execute instead of pulling in `kube-rs`. Revisit a typed client when watch semantics or manifest complexity demand it.
  - Worker orchestration is control-plane infrastructure, not an agent capability, so the V1 non-goal of agent-facing Kubernetes mutation tools still holds.

- Ship one runtime image with two entrypoints plus one UI image.
  - `deploy/docker/Dockerfile.runtime` builds a multi-stage image exposing `pharness-api` and `pharness-worker`.
  - `deploy/docker/Dockerfile.ui` builds the Vite bundle behind a small static server.
  - Build and push through a Tekton pipeline to the homelab registry; deploy through an Argo CD Application registered in the existing app-of-apps repo.

- Configure the cluster runtime with in-cluster endpoints and registry aliases.
  - Point Prometheus and Loki config at in-cluster service DNS and retire the port-forward workflow for deployed mode.
  - Set `PHARNESS_REGISTRY_ALIASES` for the internal and external registry hostnames so image alignment reports `registry_alias_match` instead of the known `registry_mismatch` backlog state.

- Pull minimal API auth forward from Phase 11.
  - The moment the API leaves loopback it needs auth: one operator bearer token for UI and CLI, one worker token valid only for internal ingest routes, both sourced from Secrets.
  - Loopback local mode stays auth-free.
  - Replace the UI's hard-coded `decided_by` operator with the authenticated identity.

- Enforce the V2 sandbox posture from the plan's security checklist.
  - Non-root containers, read-only root filesystem where possible, default seccomp, resource requests and limits, NetworkPolicy.
  - Worker service account carries only the existing read-only observation permissions and explicitly no Secret reads.
  - Per-run workspace is an emptyDir; repo provisioning for alpha is clone-by-URL with an optional deploy key Secret. Local repo sync stays future work.
  - Reserve distinct, initially unbound service accounts for future mutation capabilities so V3 becomes an RBAC grant plus a typed tool, not a re-architecture.

## Workstreams

1. Baseline: commit, restructure `ui/`, re-run smokes, tag V1 baseline.
2. Worker extraction: `crates/pharness-worker`, internal ingest endpoints with token auth, `ExecutionTarget` dispatch, Job orchestration via `kube-rs`, cancellation and pod-death handling.
3. Packaging and deploy: Dockerfiles, Helm chart under `deploy/helm/pharness/` (api, ui, RBAC, PVC, Secrets, NetworkPolicy, Ingress with cluster TLS pattern), Tekton build pipeline, Argo CD Application in the app-of-apps repo.
4. Auth and identity: bearer tokens, worker token scoping, UI operator identity.
5. Verification: cluster-runtime smoke mode and playbook.

## Acceptance Criteria (V2 Alpha cut line)

- Helm chart deploys API and UI into a homelab namespace through Argo CD.
- A run created through the deployed API executes in an isolated worker Job with resource limits and a non-root security context.
- Events persist and stream through the deployed API to the deployed UI, including the `after_seq` SSE cursor contract.
- An approval pause ends the attempt Job; approving through UI or CLI spawns a resume attempt that completes the run.
- Cancelling a run terminates the worker Job.
- Artifacts and diffs survive worker pod exit.
- Worker service account is namespace-scoped, grants no Secret reads, and secret-shaped capability reads still deny.
- Cluster observation capabilities work from in-cluster config without port-forwards, and image alignment reports `registry_alias_match` for the homelab registry pair.
- Non-loopback API requests without a valid token are rejected.

## Verification

- Add a cluster-runtime mode to the smoke tooling (extend `scripts/pharness-e2e-smoke.sh` or add `scripts/pharness-cluster-runtime-smoke.sh`) covering the acceptance list above: deterministic control-plane checks against the deployed API, one model-backed run in a worker Job, approval pause and resume across two Jobs, cancellation, artifact persistence, and denied unauthenticated access.
- Record results in `planning/v2-cluster-smoke-playbook.md` following the existing playbook convention.
- Keep `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` green throughout.

## Explicit Non-Goals For This Phase

- No mutation capabilities: no Tekton triggering, no Argo sync, no registry writes, no database operators. Approved intents remain review-state.
- No Postgres migration; single API replica with SQLite on a PVC.
- No CRD controller for SDLC resources.
- No multi-user auth beyond static bearer tokens.
- No UI feature expansion beyond operator identity; the Observations, Incidents, RemediationPlans, and Capabilities surfaces stay a separate parallel track.

## Follow-On After This Phase

- First V3 mutation slice: a typed `tekton_trigger_pipeline` capability for approved PipelineIntents behind approval gates and trusted envelopes, then typed Argo sync for approved DeploymentIntents, each with its own service account and production-impacting policy gates.

## Backlog

- Postgres store option and multi-replica API once run volume justifies it.
- Local repo sync into cluster workspaces as an alternative to clone-by-URL.
- Structured worker metrics (run duration, model latency, approval wait) once runs execute in-cluster.
- Session export bundle for debugging cluster runs.
