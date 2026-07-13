# Tekton End-To-End Smoke

## Decisions

- The live execution smoke targets only the GitOps-managed `pharness-e2e-noop`
  Pipeline in `tekton-pipelines`. The Pipeline has one inline step that prints
  a marker. It has no parameters, workspaces, secrets, registry credentials,
  network calls, or application references.
- Finance experiments are valid observation targets for model-backed tests, but
  they are not execution targets for this smoke. The test records that no
  application resources changed.
- The smoke creates a normal audited chain: Observation, Incident,
  RemediationPlan, approval gates, WorkPlan, ChangeSet, PipelineIntent,
  trusted envelopes, PipelineContract, preflight, executor Job, and
  PipelineRun. There is no hidden test-only API.
- An existing active contract is reused only when it declares exactly the
  fixture's empty parameters and workspaces. A mismatched or duplicate active
  contract fails the smoke rather than being silently replaced.
- GitOps owns the fixture and executor RBAC. The smoke validates their
  presence; it does not apply, edit, or delete cluster configuration.
- The operator console uses the same public APIs as the script. It first
  creates the chain and validates preflight, then requires a second explicit
  operator action to dispatch the fixture.

## Backlog

- Persist a typed `PipelineRunAnalysis` after the executor callback and attach
  it to the PipelineIntent automatically once analysis semantics are finalized.
- Add a retention policy for completed disposable PipelineRuns once durable
  evidence export has an operator-approved archival path.
