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
- Use `POST /api/work-items/:id/replan` as the explicit, auditable recovery
  boundary for a blocked or failed dev WorkItem. It preserves the approved
  WorkPlan but rejects exhausted budgets and any WorkItem with a captured
  ChangeSet, so a retry cannot bypass source review.
- Persist one bounded `work_item.attempt_finished` audit event for every
  terminal coding attempt. Its classification is evidence-backed and advisory:
  it supports later controller policy but does not authorize a retry.
- Git delivery uses a distinct Kubernetes ServiceAccount with no Kubernetes
  RoleBinding and a writer-only GitHub token Secret mounted only into its
  short-lived Job. The API holds the Secret name, never its value; model and
  Tekton Jobs never receive that token.
- Treat `git_delivery_preflight.status=ready_for_writer` as immutable-plan,
  scope-matching WorkItem gate, and exact writer-grant readiness;
  `dispatch_ready` remains executor availability. A remote mutation still
  requires the explicit `execute-git-delivery` operation, which re-runs
  preflight immediately before dispatch.
- A WorkItem PipelineIntent must carry observed immutable GitHub merge
  provenance. The branch commit and PR are evidence of proposed source work;
  only the separate read-only observer's `git_delivery_merge` artifact can
  supply a build revision.
- A WorkItem PipelineContract must explicitly bind that merge SHA through a
  required scalar `source_revision_param`; the execution preflight rejects a
  missing or mismatched value before a Tekton Job can be dispatched.
- Preserve the deployment boundary while WorkItem lineage reaches downstream
  delivery records. A WorkItem PipelineIntent can now create non-executing
  DeploymentIntent, Release, and RegistryEvidence records through its durable
  ChangeSet and WorkPlan. WorkItem delivery gates are now durable, invalidate
  on material source-plan changes, and participate in Tekton preflight.
- WorkItem approval gates will be a first-class scoped authorization record,
  not a null-incident variant of remediation gates. The required ownership,
  invalidation, and preflight rules are recorded in
  `planning/work-item-approval-gates.md` before that shared safety surface is
  migrated.

## Phase Status

| Phase | Outcome | Status |
|---|---|---|
| 0 | Plan convergence and truthful status model | In progress: this document is authoritative; historical plans are marked below. |
| 1 | Intent and workspace ownership | Implemented alpha: durable WorkItem, lifecycle, audit events, workspace declaration, WorkItem-backed WorkPlan, CLI/API. |
| 2 | Real coding changes | Code complete; local alpha verified and Kubernetes source provisioning/evidence path is tested but operator-disabled pending a controlled cluster smoke. |
| 3 | Git and PR delivery | Implemented source path, disabled by default: approved ChangeSets produce immutable plans; exact dev-only writer grants and scope-matching WorkItem gates authorize a dedicated GitHub branch/commit/push/PR Job; a separate read-only observer records exact PR state and immutable merge SHA. A disposable GitHub credential smoke remains. |
| 4 | Dev build and GitOps | In progress: WorkItem PipelineIntent accepts native lineage, requires immutable merge evidence, and binds its observed merge SHA to a declared Tekton contract parameter. A completed terminal Tekton analysis automatically records digest-pinned `pipeline_build_output` evidence and rejects its use when the reported commit disagrees with the observed source merge. A completed, evidenced dev pipeline can use that output to prepare a digest-pinned Kustomize GitOps update plan against its declared GitOps repo/ref, then materialize a separate `GitOpsChangeSet` with immutable source/pipeline/deployment lineage and its own `proposed -> approved/rejected` review lifecycle. Releases and RegistryEvidence now preserve that same build artifact and exact image identity, while keeping registry and supply-chain verification separate. A dedicated read-only observer Job resolves the exact GitOps base ref to durable SHA evidence; an approved ChangeSet can then produce an immutable delivery plan binding that SHA, target image operation, and deterministic branch. A separate plan-scoped GitOps writer grant and preflight enforce the exact `gitops_mutation` gate. The separately configured, disabled-by-default GitOps writer identity has its own ServiceAccount, token Secret, repo allowlist, and author metadata. The explicit dev-only execution route revalidates the plan/gate/grant, runs a fail-closed exact Kustomization transformer, and creates a branch/commit/PR with durable receipts. A separate read-only observer then records only the matching PR state and immutable merge SHA; it cannot merge, sync Argo, or patch Kubernetes. Remaining: disposable GitOps writer/observer smoke and build-output collection against a real disposable PipelineRun. |
| 5 | Dev deployment and verification | In progress: WorkItem-backed delivery records and durable delivery gates retain real source lineage; Git and Tekton preflights enforce scope matching. DeploymentIntents now have a dev-only scoped Argo envelope, preflight, dry-run/execute API, and an isolated exact-Application sync worker with durable idempotent outcomes and cancellation polling. For any WorkItem that declares a GitOps target, preflight requires the exact current observed GitOps merge and binds its artifact id/SHA into the Argo execution receipt. A typed post-sync Release verifier requires that durable completion, then reads the exact Application and Deployment and may mark the dev Release complete only on healthy evidence. An immutable DeploymentContract id from that sync receipt can require the bounded Prometheus inventory gate; missing or unhealthy evidence prevents completion. The chart remains disabled by default. Remaining: disposable dev sync smoke and target-scoped Loki/trace criteria. |
| 6 | Autonomous recovery | In progress: explicit bounded WorkItem replan and terminal coding-attempt classification are implemented; external waits, incident/remediation linkage, and rollback planning remain. |
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
- `POST /api/work-items/:id/replan`
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

- Extend coding-attempt classification to Tekton, Git, Argo, registry, and
  database outcomes, then define a policy for when a controller may propose
  (but not silently perform) an eligible bounded replan or external wait.
- Add workspace status transitions (`provisioning`, `ready`, `retained`, `cleaned`) with a cleanup worker only after the real workspace executor exists.
- Run the disposable GitHub credential smoke and require branch protection
  before enabling any non-disposable repository. Keep the writer identity
  separate from model, Tekton, and API identities.
- Preserve `git_delivery_preflight` as the machine-facing handoff between
  authorization and dispatch. It is evidence that a plan is ready for an
  isolated writer, not evidence that a branch, commit, or pull request exists.
- Add GitHub pull-request status/merge observation before allowing a WorkItem
  controller to advance from a source PR to PipelineIntent. Do not infer merge
  state from an executor result; obtain immutable merged commit provenance.
- Replace SQLite/PVC coordination with Postgres and retained object artifacts before multi-worker controller coordination.
- Project the stable durable model into CRDs only after the real development delivery loop proves state transitions and reconciliation semantics.
