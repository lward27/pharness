# Decisions

- Add durable `DeploymentIntent` records as the reviewable bridge from an approved `PipelineIntent` to future Argo deployment execution.
- Keep V1 DeploymentIntents non-executing. The default intent JSON includes `execution.enabled = false` and records that DeploymentIntent is review state only in V1.
- Allow one current DeploymentIntent per PipelineIntent in V1. Repeated create requests for the same PipelineIntent return the existing intent instead of creating duplicates.
- Require the parent PipelineIntent to be `approved` before creating a DeploymentIntent. Build/test/package intent approval is the prerequisite for deployment intent review.
- Default DeploymentIntent kind is `argo_sync_deploy`.
- Store deployment target fields directly: target environment, target namespace, and Argo CD application. These are policy inputs, not just descriptive JSON.
- Use the status graph `proposed -> approved` or `proposed -> rejected`. Approved intents may be rejected later.
- A material ChangeSet revision that stales the current PipelineIntent also marks the derived DeploymentIntent `stale`.
- Creating a DeploymentIntent for an approved PipelineIntent that already has a stale DeploymentIntent re-proposes that same row in place.
- Expose DeploymentIntents through `POST /api/deployment-intents/from-pipeline-intent`, `GET /api/deployment-intents`, `GET /api/deployment-intents/:deployment_intent_id`, and `POST /api/deployment-intents/:deployment_intent_id/transition`, with matching CLI commands.
- Record `deployment_intent.proposed`, `deployment_intent.approved`, `deployment_intent.rejected`, `deployment_intent.stale`, and `deployment_intent.reproposed` audit events.
- ChangeSet readiness reports DeploymentIntent state after the PipelineIntent is approved. A missing, stale, or non-approved DeploymentIntent is a warning today, not a blocker, because V1 cluster mutation is still disabled.
- An approved DeploymentIntent can now produce one Release for review. Release creation is still non-executing in V1.
- When a material ChangeSet revision marks a DeploymentIntent stale, the derived Release is marked stale as well. Re-proposing the approved DeploymentIntent can then re-propose the same Release row.
- DeploymentIntent creation now carries the parent PipelineIntent evidence into `intent_json.pipeline_evidence`.
  - Missing evidence is explicit: `status = missing`, `deploy_ready = false`, `review_required = true`.
  - Attached evidence keeps the observation id, artifact id, summarized Tekton/Argo/image-alignment fields, and the raw evidence snapshot.
  - `attention_required`, `running`, `failed`, and `unknown` evidence do not block V1 DeploymentIntent proposal, but they are machine-readable review signals and audit context.
- DeploymentIntent audit events include upstream pipeline evidence status and deploy-readiness state.
- Approved DeploymentIntents can now attach Argo CD Application observations through `POST /api/deployment-intents/:deployment_intent_id/evidence` and `pharness-cli deployment-intents attach-evidence`.
  - Only `argocd` Application observations are accepted in V1.
  - Attached evidence is stored in `intent_json.deployment_evidence`.
  - `Synced` and `Healthy` evidence records `status = satisfied`, `deploy_ready = true`, and `review_required = false`.
  - Out-of-sync or unhealthy evidence records `attention_required`; missing fields record `unknown`.
  - Lifecycle status remains separate from evidence status.
- A WorkItem-backed PipelineIntent can now create a DeploymentIntent without
  fabricating remediation or incident records. The durable source lineage is
  its PipelineIntent, ChangeSet, and WorkPlan; the legacy incident fields are
  nullable and remain populated for incident-backed delivery.
- An approved development DeploymentIntent can create a deployment-scoped
  supervised-autonomy envelope through
  `POST /api/deployment-intents/:deployment_intent_id/trusted-envelope` or
  `pharness-cli deployment-intents create-trusted-envelope`.
  - The envelope is limited to a WorkItem-backed, non-production `dev` target.
  - Its exact scope contains the WorkPlan, ChangeSet, PipelineIntent,
    DeploymentIntent, target namespace, Argo Application, and only
    `argo_sync` / `argocd_sync` authority for `agent:argo-runner`.
  - The target must exactly equal the WorkItem target; it cannot be reused for
    another namespace or Application.
- `POST /api/deployment-intents/:deployment_intent_id/preflight` and
  `pharness-cli deployment-intents preflight` now return a durable, structured
  readiness result. They require approved delivery records, satisfied matching
  PipelineRun evidence, one active exact DeploymentContract, a satisfied or
  waived scoped `cluster_mutation` WorkItem gate, and an active matching
  envelope. When the WorkItem declares `gitops_repo` and `gitops_ref`, they
  also require a current approved GitOpsChangeSet and an observed
  `gitops_delivery_merge` with a valid immutable merge SHA. A branch or open
  PR is deliberately insufficient. The selected merge artifact id and SHA are
  bound into the Argo execution receipt, so a later GitOps revision cannot
  reuse an earlier sync request. Every preflight writes a
  `deployment_intent.preflighted` audit event.
- A successful preflight reports `ready_for_argo_runner = true`. Its separate
  `dispatch_ready` field is true only when the deployed API is in Kubernetes
  worker mode and the disabled-by-default executor is enabled for the exact
  Argo Application. The chart contains an application-name-scoped
  `pharness-argo-runner` ServiceAccount and exact RBAC; Helm rejects an
  enabled empty allowlist rather than emitting a broad Argo Application Role.
- `POST /api/deployment-intents/:deployment_intent_id/execute` and
  `pharness-cli deployment-intents execute` are dry-run by default. `--apply`
  requires a reason and re-runs the full contract, evidence, WorkItem gate,
  and permission-grant preflight immediately before dispatching a dedicated
  worker Job.
  - The Job has only the internal worker token and `get`/`patch` access to the
    exact configured Application. It receives no Fireworks, Git, registry,
    database, or secret credentials.
  - It requests an Argo operation sync with `prune=false` and no force option,
    then reports compact `submitted`, `completed`, `failed`, or `cancelled`
    evidence through worker-token-protected internal routes.
  - The outcome artifact is idempotent by execution id and state. A cancelled
    WorkItem stops the worker at its next control poll; Pharness never attempts
    an implicit reverse sync or rollback.
  - `completed` means Argo reported `Synced` with an operation phase of
    `Succeeded`. It is not a rollout-health, metrics, log, trace, or release
    verification assertion.
- WorkItem reconciliation is a read-only controller view over the same
  evidence. After an immutable GitOps merge it returns the exact next
  deployment action: intent review, preflight authorization, runner
  availability, explicit sync dispatch, sync wait/failure, Release creation
  and approval, post-sync verification, or final WorkItem completion. It does
  not call the Argo executor, create a Release, complete a Release, or change
  WorkItem state as a side effect. Its final `complete_work_item` action is
  apply-only and revalidates completed post-sync Release evidence and current
  lineage before recording terminal WorkItem state.

# Backlog

- Exercise the short-lived `pharness-argo-runner` Job against one disposable
  dev Application before enabling it for any other target. Do not hide Argo
  mutation behind shell execution.
- Add observed workload rollout, Prometheus/Loki/trace verification, and
  Release completion as a separate post-sync stage. The first read-only stage
  is implemented through Release verification: it requires a completed Argo
  outcome, reads the exact Application and declared Deployment, and can mark a
  dev Release complete only when both are healthy. LGTM criteria remain the
  next extension of that verifier.
- Add production policy gates for blast radius, sync windows, protected namespaces, and rollback evidence before any production-impacting DeploymentIntent can execute.
- Add Argo preview/diff evidence before approving deploy intent.
- Promote pipeline evidence warnings to blockers once real deployment execution exists, especially for production-impacting DeploymentIntents.
- Promote deployment evidence warnings to blockers once real release or deployment execution exists, especially for production-impacting Release records.
- Extend Release, RegistryEvidence, and ApprovalGate records with the same
  WorkItem-compatible lineage before enabling WorkItem GitOps or Argo actions.
