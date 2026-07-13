# Tekton Execution

## Decisions

- Add `POST /api/pipeline-intents/:pipeline_intent_id/execute` with a dry-run
  default. The matching CLI command is `pharness-cli pipeline-intents execute`;
  `--apply` is the explicit mutation switch.
- Require an approved PipelineIntent and approved parent ChangeSet and WorkPlan.
  Every gate on the associated RemediationPlan must be satisfied or waived;
  `pipeline_mutation` and `cluster_mutation` gates are mandatory, with
  `production_impact` also mandatory for production-impacting execution.
- Add a PipelineIntent trusted-envelope factory. It creates an active
  `supervised_autonomy` PermissionGrant bound to one SDLC chain and one Tekton
  namespace, not a broad trusted mode.
- Limit the first manifest builder to an enabled PipelineRef, scalar or array
  params, existing PVC workspaces, or ReadWriteOnce claim templates. It cannot
  set service accounts, pod templates, secret references, arbitrary resources,
  or Tekton task specifications.
- Route mutation through a short-lived `pharness-worker` executor mode. It
  submits exactly one prebuilt manifest, reports submission, then observes that
  exact PipelineRun until Tekton reports `Succeeded=True` or `Succeeded=False`.
  It reports final `completed` or `failed` outcome through the token-protected
  internal API and does not load a model key.
- Keep PipelineIntent authorization status distinct from execution state. A
  successful callback returns the intent to `approved` and records
  `pipeline_run_succeeded`; a failed callback marks it `failed`. This keeps
  downstream approval semantics stable without losing execution history.
- Add `pharness-tekton-runner` with namespaced PipelineRun rights only. The API
  keeps permission only to create executor Jobs in its own namespace; the
  normal agent worker remains read-only.
- Reconcile executor Jobs every 30 seconds. A failed Job, a successful Job
  without a callback, or a deleted Job for an execution already dispatched
  changes that exact PipelineIntent to `failed` and appends
  `pipeline_intent.execution_executor_lost`. A late worker callback cannot
  restore a terminal intent to `executing`.
- Preserve the dispatch-owned execution identity (`executor_job_name` and
  PermissionGrant) when executor callbacks update PipelineRun state. This lets
  the reaper identify a worker that disappears while observing an otherwise
  running PipelineRun.
- Use a configurable five-second default observation cadence
  (`PHARNESS_TEKTON_EXECUTOR_POLL_SECONDS`). The executor Job active deadline
  is the hard maximum observation window; hitting it is reconciled as a failed,
  ambiguous execution rather than retried automatically.
- Expose `execution_state` directly on PipelineIntent API responses in addition
  to the durable `intent_json`, so machine clients can inspect the executor Job,
  PipelineRun identity, and reported failure without decoding the whole plan.
- Persist one compact, idempotent `tekton_pipeline_run_execution` artifact and
  `pipeline_run_execution` observation for each terminal executor callback.
  They contain only execution identity, terminal status, PipelineRun identity,
  and bounded error text; no Pod logs or full Kubernetes objects are retained.
- A DeploymentIntent may be proposed without evidence, but its approval needs
  satisfied `PipelineRunAnalysis` evidence. If Pharness executed the PipelineRun,
  that analysis must match the recorded namespace and name. The terminal receipt
  alone cannot authorize deployment.
- Require one active PipelineContract before a PipelineIntent becomes ready for
  execution. The contract is operator-managed durable policy data and validates
  parameter shapes and workspace bindings before a manifest is dispatched.
- Retired contracts remain visible and auditable but make matching execution
  previews blocked. This prevents an older contract from silently authorizing
  a PipelineRun after an operator withdraws it.
- Replace an active contract atomically when a Pipeline schema changes. The
  outgoing contract is retired and the replacement becomes the only active
  contract before subsequent preflight evaluation.
- Extend the deterministic smoke with a dry-run PipelineIntent preview and a
  synthetic successful PipelineRunAnalysis fixture before DeploymentIntent
  approval. The latest verified artifact is generated under `target/e2e-smoke/`.

## Backlog

- Add a retry/reconciliation state machine. Reaping marks an ambiguous executor
  attempt failed; it does not retry a build automatically.
- Add a reviewed Tekton import flow. The current contract is explicit policy
  data; it is not yet reconciled from the live Pipeline resource.
- Add a disposable in-cluster PipelineRun apply smoke. Do not use a production
  pipeline for the first mutation verification.
- Surface execution preflight, manifest, executor Job, PipelineRun, and callback
  audit state in the console flow instead of showing them only in JSON.
- Consider a separate watcher service only after real PipelineRun durations
  show that holding one bounded executor Job for terminal observation is an
  operational problem. The first implementation favors one auditable execution
  identity over a second controller.
