# Tekton Executor Smoke Playbook

## CLI

### Decisions

- The execution smoke is intentionally separate from the broad cluster-runtime
  smoke. It performs one bounded mutation only after a successful preflight.
- The test fixture is GitOps-managed. Do not use `kubectl apply` to create it
  during a smoke run; a missing fixture is a deployment failure to resolve.
- An applied intent records its submission, then a terminal `succeeded` or
  `failed` execution receipt. Submission alone is not a successful mutation.
- A terminal receipt is not a deployment approval. A matching satisfied
  PipelineRunAnalysis remains required before a DeploymentIntent can approve.

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
6. Wait for the status to become `Succeeded`, then choose **Open delivery
   flow** to inspect the WorkPlan, ChangeSet, PipelineIntent, approval gates,
   audit events, and terminal PipelineRun receipt.

The console path is intentionally equivalent to the CLI smoke. It creates
durable test records but does not change a finance application, read a secret,
or initiate a deployment.

The PipelineIntent returns to `approved` after a successful run; its execution
receipt reports `succeeded`. Those are separate authorization and execution
states.

## Backlog

- Add a console link from terminal evidence directly to its typed
  PipelineRunAnalysis after that evidence is persisted automatically.
- Add an operator-reviewed artifact retention workflow before deleting any
  completed fixture PipelineRuns.
- Add a controlled failure variant that proves executor-loss reconciliation and
  late-callback rejection.
