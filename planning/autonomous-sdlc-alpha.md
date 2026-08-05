# Autonomous SDLC Alpha

## Decisions

- Treat a WorkItem as the durable root for a requested feature, bug fix, or
  operational outcome. It records the target source repository and ref,
  optional GitOps repository and ref, intended environment, acceptance
  criteria, retry budget, and lifecycle independent of incident response.
- Keep incident-derived WorkPlans valid, but allow a WorkPlan to be rooted in
  exactly one of a WorkItem or a RemediationPlan. A feature request must not
  be represented as a synthetic incident merely to satisfy legacy storage.
- Add Workspace as a durable provenance record, not a persistent source tree.
  It records the immutable source revision, requested branch, assigned run,
  and retention state while artifacts retain the meaningful diff and output.
- Keep this source pass non-mutating outside the repository. No GitHub token,
  Git writer, Argo runner, database identity, or deployment RBAC is added.
  Those capabilities require separately named external targets and approval.
- The first autonomous delivery target remains an isolated development
  workload. Production promotion, database mutation, and autonomous merge
  stay blocked behind later typed policy and verification work.
- The WorkPlan lineage migration was exercised against a SQLite database at
  the prior migration level with an existing remediation-backed WorkPlan. It
  preserves that legacy lineage while enabling WorkItem-backed plans.
- Real coding alpha is local-worker-only and accepts only canonical local Git
  repositories named in `PHARNESS_WORKSPACE_ALLOWED_REPOS`. Each attempt
  clones with `--no-local`, pins the requested ref to an immutable commit, and
  works on an attempt branch under the configured workspace root. It does not
  commit, push, open a pull request, or interact with a cluster.
- A completed coding attempt moves its WorkItem and Workspace to `verifying`.
  Capturing a ChangeSet stores a bounded Git diff and compact test-event
  summaries as artifacts, then returns the WorkItem to `awaiting_approval` for
  human source review. Secret-shaped paths are rejected before diff capture.
- The first in-cluster coding workspace will be a per-run `emptyDir`, separate
  from the `pharness-api-data` SQLite PVC. Start with one concurrent coding
  worker, a `4Gi` `emptyDir.sizeLimit`, `2Gi` ephemeral-storage request, `4Gi`
  ephemeral-storage limit, and node affinity to the roomier
  `ubuntu-lucas-engineering` node. The Job TTL reclaims the clone; Pharness
  keeps only bounded diff, test, and run artifacts durably.
- Do not expand or share the API's durable `2Gi` PVC for source workspaces.
  The cluster's `local-path` storage class does not advertise PVC expansion and
  does not provide a cluster-wide free-space reservation. Workspace capacity is
  therefore governed by the selected node's actual free disk, not PVC request
  totals.
- The Kubernetes worker manifest now enforces the initial workspace envelope:
  `4Gi` `emptyDir.sizeLimit`, `2Gi` ephemeral-storage request, `4Gi` limit,
  node affinity to `ubuntu-lucas-engineering`, and one active model worker Job.
  The admission check is intentionally API-local because the deployed API is
  single-replica with SQLite; it is not presented as a distributed scheduler.
  It is safe to prevent a second run, but it does not make a remote source
  checkout available yet.
- Remote source checkout instructions are now a typed, API-issued attempt
  contract (`WorkspaceSourceSpec`), not worker environment variables. The
  worker accepts HTTPS-only repositories and safe Git refs, rejects
  credential-shaped URLs, and the API owns an exact configured remote-repo
  allowlist. The allowlist is deliberately empty by default, so no in-cluster
  source clone is enabled by this foundation alone.
- Kubernetes WorkItem execution now consumes that source contract. The worker
  clones only into its bounded `emptyDir`, resolves the requested ref to a
  full immutable Git object ID, creates the issued attempt branch, and reports
  the pin before the model can act. The API accepts that report only when its
  workspace, repository, source ref, branch, and WorkItem scope exactly match
  the issued run contract.
- A completed Kubernetes coding attempt carries bounded Git status/diff/test
  evidence back to the API. The API rejects missing, cross-workspace,
  cross-branch, cross-repository, oversized, or secret-shaped evidence before
  marking the run complete. It persists `workspace_git_diff` and
  `workspace_git_status` artifacts; ChangeSet capture reuses those artifacts
  rather than attempting to inspect an expired Job `emptyDir`.
- The initial remote coding target is the public disposable
  `https://github.com/lward27/yfinance_wrapper.git` repository. It remains
  disabled in deployed values until a GitOps-reviewed activation and smoke
  confirm worker DNS/egress and the bounded Job lifecycle in the real cluster.
- Git delivery now begins with an idempotent `git_delivery_plan` artifact,
  prepared only from an approved WorkItem-backed ChangeSet with matching real
  workspace artifacts. It records the exact repository, immutable base commit,
  issued branch, diff digest, commit metadata, and pull-request metadata. It
  performs no Git operation and explicitly records `not_authorized` until a
  separate Git writer identity and typed grant exist.
- An operator can now authorize exactly one current delivery plan. This creates
  an idempotent `supervised_autonomy` PermissionGrant for `agent:git-writer`,
  constrained to Git branch/commit/push/PR actions, the dev-only WorkItem
  repository, issued branch, WorkPlan, ChangeSet, and plan artifact. Creating
  the grant does not introduce Git credentials or perform a remote operation.
- Git delivery preflight now turns that immutable plan and exact grant into a
  durable `git_delivery_preflight` artifact. It rechecks approved parent
  state, development-only targeting, source pinning, and the matching writer
  grant. `ready_for_writer` means the authorization contract is complete;
  `dispatch_ready=false` remains explicit until a separate isolated writer is
  deployed, so preflight cannot be mistaken for a remote Git mutation.
- A dedicated Git writer executor is now implemented but off by default. Its
  branch/commit/push/PR path starts only after an explicit API execute call
  revalidates the current immutable plan and exact grant. The job has no model
  credential, uses its own ServiceAccount without Kubernetes RBAC, and mounts
  the GitHub token only from a writer-specific Secret. It reports bounded
  completed/failed provenance back as immutable result artifacts; a PR alone
  never advances deployment state.
- WorkItem reconciliation now exposes the controller's next action as a
  preview and applies it only when explicitly requested. It can deterministically
  declare a WorkPlan/workspace, start an already-approved bounded coding
  attempt, capture its durable ChangeSet evidence, and prepare/preflight an
  approved ChangeSet for Git delivery. From durable Git delivery artifacts it
  now reports the exact next handoff: authorization, writer availability,
  explicit writer execution, result wait, PR observation, merge wait, or
  PipelineIntent definition after immutable merge evidence. After that intent
  exists, reconcile returns its review state and a read-only execution
  preflight where appropriate, then distinguishes scoped Tekton authorization,
  explicit executor dispatch, external wait, terminal analysis review,
  build-output review, DeploymentIntent declaration, and GitOps-update
  planning. Once that separate GitOps ChangeSet exists, it reports its review,
  base-ref observation, immutable delivery plan, scoped writer authorization,
  writer availability, explicit execution, PR observation, merge provenance,
  and DeploymentIntent-review handoffs from durable artifacts. After an
  immutable GitOps merge, it similarly reports deployment preflight,
  authorization, Argo runner availability, execution, terminal state, Release
  review, post-sync verification, and final WorkItem completion. Applying the
  final action is allowed only after current approved lineage and completed
  post-sync verification evidence are revalidated; it writes an audit event
  but never initiates an external operation. It stops at every external
  mutation/review boundary rather than inferring success from a branch, PR,
  or successful build alone.
- Applying any reconciliation action that waits on an external coding,
  branch-and-PR, Tekton, GitOps, or Argo outcome now persists one active
  `controller_wait` for that WorkItem. The record has a 15-second first
  observation target, a WorkItem/controller-budget-capped deadline, a bounded
  check count, and only non-secret source/target provenance. Re-applying the
  same action is idempotent; a later controller action supersedes the active
  wait and emits `controller_wait.scheduled` or
  `controller_wait.superseded` audit evidence. The API and CLI can list waits,
  but this slice intentionally adds no background poller, automatic retry,
  rollback, or external operation.
  Operators can inspect one WorkItem with
  `pharness work-items waits --work-item-id <id> --status active`.
- The first due-wait controller tick is `POST
  /api/controller-waits/reconcile-due` or `pharness work-items
  reconcile-due-waits`. For a `pipeline_execution` wait it first performs one
  exact, policy-checked `TektonAnalyzePipelineRun` read against the namespace
  and name already persisted in that PipelineIntent's execution state. A
  terminal `succeeded`, `failed`, or `cancelled` result is stored through the
  existing terminal-evidence boundary; `running` and `unknown` results receive
  a compact idempotent Observation. A `deployment_execution` wait likewise
  reads only the Application recorded by its immutable Argo execution receipt;
  only `Succeeded` plus `Synced` becomes a completed sync result, while
  `Failed`, `Error`, and `Terminated` remain terminal non-success evidence.
  All other Argo states are compact observations. The tick then compares each expected action
  with the current action derived from durable evidence. A matching action
  increments the check count and reschedules the wait; a different action
  resolves it and reports the next boundary; an
  expired/depleted wait blocks its non-terminal WorkItem with audit evidence.
  It deliberately does not perform read-only remote observation yet, because
  observer Job dispatch is a separate, explicitly governed capability.
- Applying reconciliation to a known downstream failure now writes a typed
  `work_item.delivery_blocked` audit event, a linked `delivery_failure`
  Observation, a candidate Incident, and a deterministic read-only draft
  RemediationPlan anchored to the WorkPlan's workload target and run
  provenance. The plan has pending file, Git, pipeline, cluster, and
  production gates, and cannot execute or approve itself. The WorkItem stops
  without retries, rollback, remediation execution, secret collection, or any
  external mutation; those are separate reviewed recovery decisions.
- A remediation-derived WorkPlan now requires the parent RemediationPlan to
  progress through `draft -> proposed -> approved`. Approval requires an
  explicit actor and reason when the plan declares `requires_approval=true`.
  Creating the resulting WorkPlan is audited and leaves it in `draft` with
  execution disabled; this is a recovery handoff, not permission to retry.
- A WorkItem-specific PipelineIntent endpoint now resolves that approved
  ChangeSet internally and requires its current immutable Git merge artifact
  before proposing an intent. The caller supplies the active PipelineContract
  id and an enabled concrete Tekton definition; Pharness pins the contract
  id/version/target into the intent and rejects a retired or drifted binding at
  execution preflight. The result is review-only; this bridge cannot approve
  or execute a PipelineIntent.
- The paired read-only WorkItem pipeline-intent context returns the exact
  merged source provenance, any already-proposed intent, and active
  PipelineContracts filtered by explicit namespace/ref. It gives a controller
  the facts needed to choose a reviewed contract without making that policy
  choice implicitly inside the API.
- A blocked or failed development WorkItem can be explicitly replanned only
  when its approved WorkPlan has no captured ChangeSet and it has remaining
  attempt budget. Replan clears any stale run link, retains the WorkPlan, and
  returns the WorkItem to `awaiting_approval`; a subsequent controller step
  starts the fresh isolated workspace. It is deliberately not an automatic
  retry mechanism.
- Terminal coding attempts now produce one `work_item.attempt_finished` audit
  event with a bounded, evidence-backed classification and a recommended next
  action. The initial classifier recognizes policy denials, turn-budget
  exhaustion, tool failures, absent model responses, completion, cancellation,
  and unknown failure. It does not inspect or persist raw error text in the
  audit payload and cannot dispatch a retry.

## Backlog

- Run a controlled disposable-repository GitHub smoke with a fine-grained
  token, exact repository allowlist, branch protections, and egress review.
  Then add PR status/merge observation before a controller can schedule a
  build from Git provenance.
- Add an API-level fake-provider fixture for the full coding workflow. The
  workspace provisioner is tested against a real disposable Git repository,
  but the Fireworks-backed HTTP smoke remains an operator-run playbook so it
  does not consume credentials or model quota in CI.
- Before allowing concurrent coding workers or workspaces above `4Gi`, add a
  real multi-node storage backend or an explicit node-local capacity controller.
  Do not rely on `local-path` PVC request sizes as admission control; observed
  free disk is materially lower than the aggregate PVC requests in the current
  homelab.
- Replace the API-local worker concurrency check with durable queued WorkItem
  admission before running multiple API replicas or multiple coding workers.
- Before enabling a private repository, provide a distinct read-only Git
  identity through a narrowly mounted credential mechanism and a matching
  egress policy. Do not reuse the worker token, operator token, or model key
  as a Git credential.
- Add a controlled Kubernetes coding-alpha smoke for the public disposable
  repository, with the exact Helm allowlist change reviewed through the
  cluster's GitOps owner. It must prove `workspace.provisioned`, bounded diff
  artifacts, ChangeSet capture, and cleanup before the allowlist is kept on.
- Add DeploymentIntent preflight and a separate Argo runner only after GitOps
  revision provenance is available.
- Extend the terminal coding classifier to typed external-system outcomes
  (Tekton, Git, Argo, registry, and database), then define a policy that can
  propose only a bounded replan, an external wait, or a blocked remediation
  path. It must never silently retry on an unknown classification.
- Add a due-wait controller worker only after each wait kind has a typed,
  read-only observation adapter, explicit terminal/timeout semantics, and
  durable backoff/check-count handling. It must resolve or block a WorkItem;
  it must not dispatch a retry, merge, build, sync, or rollback by itself.
