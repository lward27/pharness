# Autonomous SDLC Roadmap

This is the active implementation roadmap. The older V1 and V2 plans remain useful design history, but their status claims are superseded here.

## Current Position

Pharness is a deployed V2 control plane with authenticated API/UI, isolated Fireworks worker Jobs, durable SQLite state, cancellation, approvals, artifacts, and typed read-only Kubernetes, Argo CD, Tekton, Prometheus, Loki, and registry capabilities. It also has durable delivery-review records from `WorkPlan` through `RegistryEvidence`, PipelineContracts, DeploymentContracts, typed Tekton execution, terminal analysis, and declared deployment handoff.

The new alpha foundation adds durable `WorkItem` and `Workspace` records. A WorkItem now owns the feature or bug intent, source and GitOps targets, acceptance criteria, budget, target environment, status history, and cancellation. It can create a WorkItem-backed WorkPlan and an ephemeral workspace declaration. This does **not** clone a repository, run a model, create a Git diff, or write GitHub/Argo state yet.

## Decisions

- Treat autonomous SDLC as a durable WorkItem controller loop, rather than as a chat session. It must pause, resume, wait for external systems, respect budgets, and finish with a durable result.
- Keep development as the first autonomous environment. Production is explicitly gated until the development loop has measured success.
- Reuse PermissionGrant/trusted-envelope semantics as the sole authorization model for Git, Tekton, GitOps, deployment, and future database actions.
- Keep GitOps as source of truth. Argo reconciliation comes after immutable source provenance, not as a substitute for it.
- Use branch-and-PR delivery first. GitHub is a small native adapter; governed MCP adapters are later for Jira, Slack, and similar systems.
- Workspaces are ephemeral execution locations. Durable evidence is the source/base revision, diff, commit, build output, and verification artifacts.
- A WorkItem-backed ChangeSet must be derived from a captured workspace Git diff. The API rejects synthetic ChangeSet creation for this lineage.

## Phase Status

| Phase | Outcome | Status |
|---|---|---|
| 0 | Plan convergence and truthful status model | In progress: this document is authoritative; historical plans are marked below. |
| 1 | Intent and workspace ownership | Implemented alpha: durable WorkItem, lifecycle, audit events, workspace declaration, WorkItem-backed WorkPlan, CLI/API. |
| 2 | Real coding changes | Next: provision isolated workspace, pin base SHA, run bounded model attempts, record diffs/tests, derive ChangeSet. |
| 3 | Git and PR delivery | Pending: typed branch, commit, push, PR, status, and merge-eligibility actions using a scoped Git identity. |
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
- `POST /api/work-items/:id/work-plan`
- `GET /api/workspaces`
- `GET /api/workspaces/:id`

WorkItem statuses are `submitted`, `planning`, `awaiting_approval`, `executing`, `verifying`, `blocked`, `completed`, `failed`, and `cancelled`. The current transition guard is deliberately conservative; a terminal WorkItem cannot be reopened.

## Next Cut Line: Autonomous Coding Alpha

Implement a bounded dev-only code worker for an explicitly permitted disposable finance app:

1. Create workspace from an approved source repo/ref and resolve an immutable base SHA.
2. Start a bounded Fireworks attempt in that workspace with no secret access.
3. Capture actual Git diff and test artifacts; create a ChangeSet only from that evidence.
4. Retry test failures within the WorkItem attempt and elapsed-time budgets.
5. Stop at a WorkPlan/PermissionGrant gate before any branch push, PR, Tekton, GitOps, or deployment mutation.

Success proves a real, reviewable local source change without credentials leaking or remote state changing. Failure must leave a retained evidence summary and a durable `blocked` or `failed` WorkItem, never an unbounded loop.

## Backlog

- Add an API-level WorkItem `replan` endpoint once the WorkPlan revision/invalidation behavior is defined for WorkItem lineage.
- Add workspace status transitions (`provisioning`, `ready`, `retained`, `cleaned`) with a cleanup worker only after the real workspace executor exists.
- Design Git credentials and GitHub installation/token delivery with separate identities; do not use worker service-account credentials as a Git write credential.
- Replace SQLite/PVC coordination with Postgres and retained object artifacts before multi-worker controller coordination.
- Project the stable durable model into CRDs only after the real development delivery loop proves state transitions and reconciliation semantics.
