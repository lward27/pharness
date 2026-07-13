# Tekton Executor Smoke

## Decisions

- Verify `pipeline-intents execute` in two stages: a deterministic preview
  first, then one `--apply` against a purpose-built disposable Pipeline in an
  allowlisted non-production namespace.
- An applied intent must first record `pipeline_run_created`, then reach either
  `pipeline_run_succeeded` with PipelineIntent returned to `approved` or
  `pipeline_run_failed` with PipelineIntent `failed`. Submission alone is not
  a successful mutation result.
- Expect exactly one terminal execution artifact and observation per execution
  identity. They are a bounded receipt, not an approval to deploy: attach a
  successful PipelineRunAnalysis for that exact PipelineRun before approving a
  DeploymentIntent.
- Keep the executor Job alive while it polls the exact PipelineRun at the
  configured interval. The Job active deadline is the maximum observation
  period, and the API reaper owns ambiguous worker-loss handling.
- Treat a failed executor Job as a control-plane outcome, not a reason to retry
  automatically. The expected result is a failed PipelineIntent with
  `execution_state.state = executor_job_lost` and a durable audit event.

## Backlog

- Add this apply check to the cluster smoke only after the disposable Pipeline
  is installed and its inputs are represented by a Pipeline contract record.
- Add a controlled failure variant that deletes the executor Job after dispatch
  and proves the reaper transition and late-callback rejection.
- Add a disposable successful PipelineRun variant that asserts the executor
  terminal callback, approved intent, PipelineRun identity, artifact, and audit
  trail.
# Tekton Executor Smoke Playbook

## CLI

### Decisions

- The execution smoke is intentionally separate from the broad cluster-runtime
  smoke. It performs one bounded mutation only after a successful preflight.
- The test fixture is GitOps-managed. Do not use `kubectl apply` to create it
  during a smoke run; a missing fixture is a deployment failure to resolve.

### Run

From the repository root, export an operator token, then run the preflight:

```sh
export PHARNESS_API_TOKEN='your operator token'
scripts/pharness-tekton-execution-smoke.sh
```

The preflight creates the audited control-plane records and stops before any
PipelineRun exists. Inspect the latest artifacts under
`target/tekton-execution-smoke/`.

To dispatch the single inert PipelineRun after reviewing the preflight:

```sh
export PHARNESS_API_TOKEN='your operator token'
scripts/pharness-tekton-execution-smoke.sh --apply
```

The successful manifest reports `application_resources_changed: false`, the
PipelineIntent identifier, the reused or created PipelineContract, and the
actual PipelineRun identity.

## Console

1. Open the Pharness console and select **Delivery Test** in the navigation.
2. Review the fixture scope. It must show
   `tekton-pipelines/pharness-e2e-noop`, no parameters or workspaces, and
   application impact `None`.
3. Select the acknowledgement checkbox and choose **Prepare preflight**.
4. Confirm the durable-record panel reports `Preflight Passed`. At this point
   Pharness has not created a PipelineRun.
5. Choose **Dispatch inert PipelineRun**.
6. Wait for the status to become `Completed`, then choose **Open delivery
   flow** to inspect the WorkPlan, ChangeSet, PipelineIntent, approval gates,
   audit events, and terminal PipelineRun receipt.

The console path is intentionally equivalent to the CLI smoke. It creates
durable test records but does not change a finance application, read a secret,
or initiate a deployment.

## Backlog

- Add a console link from terminal evidence directly to its typed
  PipelineRunAnalysis after that evidence is persisted automatically.
- Add an operator-reviewed artifact retention workflow before deleting any
  completed fixture PipelineRuns.
