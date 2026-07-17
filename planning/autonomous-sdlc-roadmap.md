# Autonomous SDLC Roadmap

This is the active implementation roadmap. The older V1 and V2 plans remain useful design history, but their status claims are superseded here.

## Current Position

Pharness is a deployed V2 control plane with authenticated API/UI, isolated Fireworks worker Jobs, durable SQLite state, cancellation, approvals, artifacts, and typed read-only Kubernetes, Argo CD, Tekton, Prometheus, Loki, and registry capabilities. It also has durable delivery-review records from `WorkPlan` through `RegistryEvidence`, PipelineContracts, DeploymentContracts, typed Tekton execution, terminal analysis, and declared deployment handoff.

The coding alpha now provisions an allowlisted local repository into an
independent workspace, pins its base commit, executes a bounded Fireworks
attempt, and captures a real Git ChangeSet with test-event evidence. It does
not commit, push, open a PR, or mutate GitOps/Argo state.

Kubernetes worker Jobs have bounded `emptyDir` workspaces and a single-worker
admission limit. Remote source checkout is represented as a typed,
API-validated HTTPS contract; a worker must report a full immutable commit
pin before model execution, and completed attempts must return bounded Git
evidence for durable artifact capture. The exact repository allowlist remains
empty by default, so deployed Kubernetes coding stays operator-disabled until
a controlled disposable-repository smoke is reviewed and run.

## Decisions

- Treat autonomous SDLC as a durable WorkItem controller loop, rather than as a chat session. It must pause, resume, wait for external systems, respect budgets, and finish with a durable result.
- Keep development as the first autonomous environment. Production is explicitly gated until the development loop has measured success.
- Reuse PermissionGrant/trusted-envelope semantics as the sole authorization model for Git, Tekton, GitOps, deployment, and future database actions.
- Keep GitOps as source of truth. Argo reconciliation comes after immutable source provenance, not as a substitute for it.
- Use branch-and-PR delivery first. GitHub is a small native adapter; governed MCP adapters are later for Jira, Slack, and similar systems.
- Workspaces are ephemeral execution locations. Durable evidence is the source/base revision, diff, commit, build output, and verification artifacts.
- A WorkItem-backed ChangeSet must be derived from a captured workspace Git diff. The API rejects synthetic ChangeSet creation for this lineage.
- Use `POST /api/work-items/:id/reconcile` as the current single-step WorkItem
  controller surface. It previews its next action by default and requires
  `apply=true` to make a durable transition, preserving approval boundaries
  while allowing a scheduler to drive the proven portions of the dev loop.

## Phase Status

| Phase | Outcome | Status |
|---|---|---|
| 0 | Plan convergence and truthful status model | In progress: this document is authoritative; historical plans are marked below. |
| 1 | Intent and workspace ownership | Implemented alpha: durable WorkItem, lifecycle, audit events, workspace declaration, WorkItem-backed WorkPlan, CLI/API. |
| 2 | Real coding changes | Code complete; local alpha verified and Kubernetes source provisioning/evidence path is tested but operator-disabled pending a controlled cluster smoke. |
| 3 | Git and PR delivery | In progress: approved ChangeSets produce immutable delivery plans and dev-only, plan-bound Git writer grants; typed commit/push/PR execution still needs a scoped Git identity. |
| 4 | Dev build and GitOps | Pending: bind immutable commit to PipelineContract, collect image/provenance, prepare reviewed GitOps ChangeSet/PR. |
| 5 | Dev deployment and verification | Pending: DeploymentIntent preflight, scoped Argo sync runner, rollout and LGTM verification. |
| 6 | Autonomous recovery | Pending: classified failures, bounded retry/wait/reconcile, incident/remediation linkage, explicit rollback planning. |
| 7 | Production readiness | Pending: promotion, protected namespaces, windows, blast-radius checks, release gates, backup-aware database flow, rollback authority. |
| 8 | Platform completion | Pending: Postgres/object artifacts, CRD projections/controllers, RAG with citations, database operator, governed MCP ToolServers. |

## Public Contract

Current WorkItem endpoints:

- `POST /api/work-items`
- `GET /api/work-items`
- `GET /api/work-items/:id`
- `GET /api/work-items/:id/events`
- `POST /api/work-items/:id/transition`
- `POST /api/work-items/:id/cancel`
- `POST /api/work-items/:id/reconcile`
- `POST /api/work-items/:id/work-plan`
- `GET /api/workspaces`
- `GET /api/workspaces/:id`

WorkItem statuses are `submitted`, `planning`, `awaiting_approval`, `executing`, `verifying`, `blocked`, `completed`, `failed`, and `cancelled`. The current transition guard is deliberately conservative; a terminal WorkItem cannot be reopened.

## Next Cut Line: Kubernetes Autonomous Coding Alpha

Enable and observe the bounded dev-only code worker against the explicitly
permitted disposable `yfinance_wrapper` repository:

1. GitOps-review an exact HTTPS allowlist entry for the disposable repository.
2. Create a dev WorkItem from the API, then verify a worker Job pins an immutable base SHA before model execution.
3. Capture actual Git diff and test artifacts; create a ChangeSet only from that evidence.
4. Prove cancellation and failure leave the WorkItem bounded and the ephemeral workspace reclaimable.
5. Remove the allowlist entry after the smoke unless a reviewed follow-on keeps it enabled.

Success proves a real, reviewable in-cluster source change without credentials
leaking or remote state changing. Failure must leave a retained evidence
summary and a durable `blocked` or `failed` WorkItem, never an unbounded loop.

## Backlog

- Add an API-level WorkItem `replan` endpoint once the WorkPlan revision/invalidation behavior is defined for WorkItem lineage.
- Add workspace status transitions (`provisioning`, `ready`, `retained`, `cleaned`) with a cleanup worker only after the real workspace executor exists.
- Design Git credentials and GitHub installation/token delivery with separate identities; do not use worker service-account credentials as a Git write credential.
- Require the future Git writer to consume a current `git_delivery_plan`
  artifact and matching `agent:git-writer` grant, then revalidate its ChangeSet
  revision, material hash, source base commit, branch, and diff digest
  immediately before every remote mutation.
- Preserve `git_delivery_preflight` as the machine-facing handoff between
  authorization and dispatch. It is evidence that a plan is ready for an
  isolated writer, not evidence that a branch, commit, or pull request exists.
- Replace SQLite/PVC coordination with Postgres and retained object artifacts before multi-worker controller coordination.
- Project the stable durable model into CRDs only after the real development delivery loop proves state transitions and reconciliation semantics.
