# V2 Continuation

## Decisions

- Accept `planning/v2-handoff.md` as the current baseline after verifying the
  monorepo layout, worker/API split, Kubernetes dispatcher, Helm packaging,
  auth middleware, and recent Git history in the repository.
- Complete the first console P2 slice before cluster mutation. It uses existing
  machine-facing contracts and improves operation of the deployed control
  plane without changing execution authority.
- Add typed audit filters in the store and API. Free-text search covers event
  kind, actor, resource identity, run id, and JSON payload; exact filters cover
  kind, actor, resource kind/id, run id, and run scope.
- Resolve audit namespace from the referenced SDLC resource when the audit
  payload does not embed scope. Durable resource state is authoritative for
  WorkPlan, ChangeSet, PipelineIntent, DeploymentIntent, Release, ApprovalGate,
  Observation, Incident, RemediationPlan, and PermissionGrant events.
- Keep the first Tekton mutation behind a separate execution identity. The API
  may dispatch a short-lived executor Job, but neither the API service account
  nor the read-only agent worker should receive general PipelineRun create
  rights.
- The Tekton mutation contract must start from an approved PipelineIntent,
  validate parent WorkPlan/ChangeSet state, satisfied or waived gates, and an
  active bounded envelope, then report the concrete PipelineRun identity and
  outcome back through an authenticated internal API.
- Implement the first typed mutation as `pipeline-intents execute`. It is a
  dry-run preview unless `--apply` is explicit. A preview returns the exact
  constrained `tekton.dev/v1` PipelineRun manifest and structured preflight
  checks without changing the PipelineIntent.
- Use `supervised_autonomy` only for scoped, audited control-plane envelopes;
  it does not relax the ordinary agent file or shell policy. The first scope
  is pinned to one PipelineIntent, ChangeSet, WorkPlan, namespace, environment,
  production flag, `tekton_start_run`, and `tekton_trigger_pipeline`.
- Dispatch an applied PipelineIntent through a separate `pharness-tekton-runner`
  Kubernetes service account. It receives neither Fireworks credentials nor
  general cluster rights, and has only `PipelineRun` create/get/list/watch
  rights in Helm-configured namespaces.
- Materialize `approval_gates` for every user-created RemediationPlan and audit
  their creation. Previously only the release-observability helper did this,
  which left normal plan gate definitions inert.
- Reconcile failed, callback-less, or disappeared executor Jobs to a durable
  PipelineIntent failure. The reaper uses the executor Job annotations to bind
  one Job to one execution and refuses late callbacks after terminal state.
- Require a durable active PipelineContract for the target namespace and
  PipelineRef. Contracts validate parameter types and workspace bindings before
  execution; a missing or ambiguous contract blocks both preview readiness and
  apply.
- Support an explicit `active -> retired` PipelineContract lifecycle transition
  with audit history. Retired-only targets now report a distinct blocked
  preflight instead of looking like an unconfigured target.
- Replace PipelineContracts atomically so a target never has both ambiguous
  active versions or an authorization gap between reviewed versions.
- Keep the dedicated Tekton executor alive after manifest submission until the
  exact PipelineRun reaches a terminal Tekton condition. It records submitted,
  completed, or failed state through the internal callback, preserving the
  original executor Job identity so the reaper can safely fail an interrupted
  observation.
- Persist terminal executor receipts as compact Tekton artifacts and
  observations. Keep them separate from PipelineRunAnalysis: deployment
  approval requires satisfied analysis evidence, and an executed PipelineRun
  requires that analysis to match its namespace and name.

## Backlog

- Deploy the P2 UI/API image changes and run the cluster-runtime smoke against
  `pharness.lucas.engineering`.
- Add a cluster-only smoke that uses `--apply` against a disposable Pipeline
  and confirms the terminal executor callback, PipelineRun identity, receipt,
  matching analysis, and audit.
- Add a reviewed import from live Tekton Pipeline specs before treating
  contracts as continuously reconciled cluster state.
- Decide whether long-running production Pipelines need a dedicated watcher
  controller after measuring executor Job occupancy and deadline pressure.
- Add the GitHub webhook to the existing Tekton EventListener.
- Garbage-collect the two mistagged image repositories and address node 1 disk
  headroom.
- Finish console P2 worker-Job affordances, then split `ui/src/App.jsx` by view
  without changing behavior.
