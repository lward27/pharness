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
- Applying a reconcile action that is waiting on an external coding, Git,
  pull-request, Tekton, GitOps, or Argo outcome now records one bounded,
  durable `controller_wait`. It stores the next observation time, deadline,
  check budget, and non-secret provenance; repeat reconciliation retains the
  same active wait. Advancing to another action supersedes it with an audit
  record. This is scheduling only: no background poller, automatic retry,
  rollback, or external mutation has been introduced. Narrow explicit
  `apply=true` controller actions are the exception: a source Git delivery
  action may dispatch the separately configured branch-and-PR writer only
  after it revalidates the immutable plan, satisfied `git_mutation` gate,
  matching plan-scoped grant, and repository allowlist; source and GitOps
  pull-request-observation actions may dispatch the separately configured
  read-only observer for an already completed immutable result. Each records a
  bounded wait only after dispatch/reuse succeeds. Observer dispatch failure is
  persisted as a non-secret failure artifact so the next explicit reconcile can
  retry rather than waiting for a Job that never existed. Due-wait
  reconciliation itself never dispatches a writer or observer Job.
- `POST /api/controller-waits/reconcile-due` is the first bounded controller
  tick. It examines only due wait records and already-persisted delivery
  evidence, except that a `pipeline_execution` wait may perform the exact
  typed read-only `TektonAnalyzePipelineRun` recorded in durable execution
  state and a `deployment_execution` wait may read its exact declared Argo CD
  Application. Terminal Tekton evidence and only an Argo `Succeeded + Synced`
  result are persisted before the normal action comparison; nonterminal state
  is an idempotent compact Observation. It
  records a checked wait when nothing changed, resolves a wait when durable
  evidence changes the next controller action, and blocks a
  non-terminal WorkItem when its deadline or check budget expires. It never
  starts an observer Job, retries, merges, builds, syncs, rolls back, or calls
  a provider.
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
- WorkItem reconciliation derives its Git-delivery next action exclusively
  from durable plan, preflight, writer-result, PR-observation, and merge
  artifacts. It surfaces authorization, writer availability, execution,
  observation, merge, failed-delivery, and immutable-source-pipeline
  boundaries without silently dispatching a remote mutation.
- After immutable source merge, the same reconcile response carries the
  current PipelineIntent and, when execution is pending, its read-only Tekton
  preflight. It distinguishes PipelineIntent review, missing scoped Tekton
  authorization, explicit executor dispatch, external execution wait, failed
  execution, unsatisfied terminal analysis, untrusted or absent build output,
  required DeploymentIntent declaration, and ready-for-GitOps-plan states.
  The controller previews by default; an explicit `apply=true` may dispatch
  one isolated Tekton executor only when that current preflight is ready,
  then records an audit receipt and bounded read-only wait. It does not create
  a DeploymentIntent or GitOps change as a side effect.
- Once a verified build and declared DeploymentIntent exist, reconciliation
  also carries the separate GitOps ChangeSet and its delivery state. It
  distinguishes GitOps review, exact base-ref observation, immutable delivery
  planning, scoped writer authorization, writer availability, explicit
  branch-and-PR execution, PR observation, merge provenance, and the return to
  DeploymentIntent review. The controller previews by default; an explicit
  `apply=true` may dispatch one isolated GitOps writer only when its current
  immutable-plan preflight is ready, then records an audit receipt and bounded
  read-only wait. It never merges a PR or syncs Argo as a side effect.
- After immutable GitOps merge provenance is present, the same controller also
  carries DeploymentIntent preflight and durable Argo execution/Release flow.
  It distinguishes intent review, missing exact contract/gate/grant
  authorization, runner availability, explicit sync dispatch, external wait,
  failed execution, Release proposal and approval, post-sync verification, and
  final WorkItem completion. The controller previews by default; an explicit
  `apply=true` may dispatch one exact-Application Argo runner only when its
  current preflight is ready, then records an audit receipt and bounded
  read-only wait. It never force-syncs, prunes, merges, retries, rolls back,
  or alters a second target. The final `complete_work_item` action remains
  evidence-only and revalidates lineage plus post-sync verification first.
- Delivery failures are symmetric and terminal: applying reconciliation for a
  failed Git/Tekton/GitOps/Argo action, or a stale/rejected downstream intent,
  records `work_item.delivery_blocked` with a typed failure code and sets the
  WorkItem to `blocked`. The controller neither retries nor rolls back as a
  side effect; recovery remains an explicit reviewed replan or remediation
  decision.
- `POST /api/work-items/:id/pipeline-intent` is the machine-facing bridge from
  a merged WorkItem source change to a proposed PipelineIntent. It derives the
  WorkItem's approved ChangeSet, rejects missing or mutable source provenance,
  records the observed merge in the intent and WorkItem audit trail, and
  requires both a concrete active PipelineContract id and a concrete enabled
  pipeline definition. It pins the contract id/version/target into the intent;
  execution preflight rejects a missing, retired, or drifted binding. It neither
  approves nor executes Tekton.
- `GET /api/work-items/:id/pipeline-intent-context` returns only the merged
  WorkItem's durable lineage, current PipelineIntent, and filtered active
  PipelineContracts. It is a read-only selection aid for an orchestrator; it
  does not guess a contract, synthesize parameters, or alter delivery state.
- A WorkItem PipelineIntent must carry observed immutable GitHub merge
  provenance. The branch commit and PR are evidence of proposed source work;
  only the separate read-only observer's `git_delivery_merge` artifact can
  supply a build revision.
- A WorkItem PipelineIntent pins the active PipelineContract id, version,
  namespace, and pipeline reference selected at proposal time. That contract
  must explicitly bind the observed merge SHA through a required scalar
  `source_revision_param`; execution preflight rejects a missing, retired,
  drifted, or mismatched binding before a Tekton Job can be dispatched.
- Preserve the deployment boundary while WorkItem lineage reaches downstream
  delivery records. A WorkItem PipelineIntent can now create non-executing
  DeploymentIntent, Release, and RegistryEvidence records through its durable
  ChangeSet and WorkPlan. WorkItem delivery gates are now durable, invalidate
  on material source-plan changes, and participate in Tekton preflight.
- WorkItem approval gates will be a first-class scoped authorization record,
  not a null-incident variant of remediation gates. The required ownership,
  invalidation, and preflight rules are recorded in
  `planning/implemented/capabilities/work-item-approval-gates.md` before that
  shared safety surface is
  migrated.

## Phase Status

| Phase | Outcome | Status |
|---|---|---|
| 0 | Plan convergence and truthful status model | In progress: this document is authoritative; historical plans are marked below. |
| 1 | Intent and workspace ownership | Implemented alpha: durable WorkItem, lifecycle, audit events, workspace declaration, WorkItem-backed WorkPlan, CLI/API. |
| 2 | Real coding changes | Code complete; local alpha verified and Kubernetes source provisioning/evidence path is tested but operator-disabled pending a controlled cluster smoke. |
| 3 | Git and PR delivery | Implemented source path, disabled by default: approved ChangeSets produce immutable plans; exact dev-only writer grants and scope-matching WorkItem gates authorize a dedicated GitHub branch/commit/push/PR Job; a separate read-only observer records exact PR state and immutable merge SHA. A disposable GitHub credential smoke remains. |
| 4 | Dev build and GitOps | In progress: WorkItem PipelineIntent accepts native lineage, requires immutable merge evidence, and binds its observed merge SHA to a declared Tekton contract parameter. WorkItem reconciliation previews the delivery action and, on explicit `apply=true`, can dispatch one isolated Tekton executor only after the current preflight confirms the pinned contract, immutable source revision, exact target, pipeline/cluster gates, and plan-scoped envelope; it records an audit receipt and schedules a bounded read-only wait. Future GitOps gates do not block that separate build boundary. A successful executor leaves the PipelineIntent `approved` with its durable analysis/evidence; GitOps planning correctly accepts that eligible status only when evidence is satisfied, rather than requiring an unreachable `completed` status. A completed terminal Tekton analysis automatically records digest-pinned `pipeline_build_output` evidence and rejects its use when the reported commit disagrees with the observed source merge. A completed, evidenced dev pipeline with an exact DeploymentIntent can use that output to prepare a digest-pinned Kustomize GitOps update plan against its declared GitOps repo/ref, then materialize a separate `GitOpsChangeSet` with immutable source/pipeline/deployment lineage and its own `proposed -> approved/rejected` review lifecycle. Reconciliation follows that ChangeSet through exact base-ref observation, immutable delivery-plan preparation, scoped `gitops_mutation` authorization, writer availability, explicit branch/PR execution, observation, merge provenance, and back to DeploymentIntent review. The controller now uses that same explicit `apply=true` boundary to dispatch one preflighted isolated GitOps writer, then persists a bounded read-only result wait; it does not merge the PR or sync Argo. Releases and RegistryEvidence preserve that same build artifact and exact image identity, while keeping registry and supply-chain verification separate. A dedicated read-only observer Job resolves the exact GitOps base ref to durable SHA evidence; an approved ChangeSet can then produce an immutable delivery plan binding that SHA, target image operation, and deterministic branch. A separate plan-scoped GitOps writer grant and preflight enforce the exact `gitops_mutation` gate. The separately configured, disabled-by-default GitOps writer identity has its own ServiceAccount, token Secret, repo allowlist, and author metadata. The explicit dev-only execution route revalidates the plan/gate/grant, runs a fail-closed exact Kustomization transformer, and creates a branch/commit/PR with durable receipts. A separate read-only observer then records only the matching PR state and immutable merge SHA; it cannot merge, sync Argo, or patch Kubernetes. Remaining: disposable GitOps writer/observer smoke and build-output collection against a real disposable PipelineRun. |
| 5 | Dev deployment and verification | In progress: WorkItem-backed delivery records and durable delivery gates retain real source lineage; Git and Tekton preflights enforce scope matching. DeploymentIntents now have a dev-only scoped Argo envelope, preflight, dry-run/execute API, and an isolated exact-Application sync worker with durable idempotent outcomes and cancellation polling. WorkItem reconciliation uses the same explicit `apply=true` boundary to dispatch one preflighted runner, record its receipt, and move to a bounded read-only deployment wait. An in-process full-chain test proves the controller dispatches exactly one runner only after current source merge, successful build evidence, immutable GitOps base-revision-bound merge, exact active DeploymentContract, satisfied `cluster_mutation` gate, and scoped Argo envelope; repeat reconciliation only reuses the durable wait. For any WorkItem that declares a GitOps target, preflight requires the exact current observed GitOps merge and binds its artifact id/SHA into the Argo execution receipt. A typed post-sync Release verifier requires that durable completion, then reads the exact Application and Deployment and may mark the dev Release complete only on healthy evidence. An immutable DeploymentContract id from that sync receipt can require the bounded Prometheus inventory gate; missing or unhealthy evidence prevents completion. The chart remains disabled by default. Remaining: live disposable dev-Application controller smoke and target-scoped Loki/trace criteria. |
| 6 | Autonomous recovery | In progress: explicit bounded WorkItem replan, terminal coding-attempt classification, durable external controller waits, and apply-only terminal blocking for known Git/Tekton/GitOps/Argo delivery failures are implemented. Each terminal delivery block records a linked, non-mutating `delivery_failure` Observation, candidate Incident, and deterministic read-only draft RemediationPlan with pending mutation gates. A plan now follows `draft -> proposed -> approved` under an actor/reason audit before it can create an execution-disabled remediation WorkPlan. A manual/cron-callable due-wait tick resolves observed progress and blocks expiry. Its typed adapters can read only the exact persisted Tekton PipelineRun for a `pipeline_execution` wait or the exact declared Argo Application for a `deployment_execution` wait, recording terminal or nonterminal evidence without dispatching, retrying, rolling back, remediating, merging, or otherwise mutating an external system. |
| 7 | Production readiness | Pending: promotion, protected namespaces, windows, blast-radius checks, release gates, backup-aware database flow, rollback authority. |
| 8 | Platform completion | Pending: Postgres/object artifacts, CRD projections/controllers, RAG with citations, database operator, governed MCP ToolServers. |

## Public Contract

Current WorkItem endpoints:

- `POST /api/work-items`
- `GET /api/work-items`
- `GET /api/work-items/:id`
- `GET /api/work-items/:id/events`
- `GET /api/work-items/:id/controller-waits`
- `POST /api/controller-waits/reconcile-due`
- `POST /api/work-items/:id/transition`
- `POST /api/work-items/:id/cancel`
- `POST /api/work-items/:id/reconcile`
- `POST /api/work-items/:id/replan`
- `POST /api/work-items/:id/work-plan`
- `POST /api/work-items/:id/pipeline-intent`
- `GET /api/work-items/:id/pipeline-intent-context`
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
- Keep PipelineIntent proposal explicit until a reviewed policy can select an
  exact PipelineContract and input values from WorkItem intent. Do not infer
  a pipeline definition from a source PR; the observed immutable merge only
  supplies the build revision.
- Replace SQLite/PVC coordination with Postgres and retained object artifacts before multi-worker controller coordination.
- Project the stable durable model into CRDs only after the real development delivery loop proves state transitions and reconciliation semantics.
