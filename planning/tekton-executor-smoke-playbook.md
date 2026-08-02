# Tekton Executor Smoke Playbook

## CLI

### Decisions

- The execution smoke is intentionally separate from the broad cluster-runtime
  smoke. It performs one bounded mutation only after a successful preflight.
- The test fixture is GitOps-managed. Do not use `kubectl apply` to create it
  during a smoke run; a missing fixture is a deployment failure to resolve.
- An applied intent records its submission, then a terminal `succeeded` or
  `failed` execution receipt. Submission alone is not a successful mutation.
- A terminal receipt is not by itself a deployment approval. The executor also
  persists a matching bounded PipelineRunAnalysis; its satisfied evidence is
  required before a DeploymentIntent can approve.

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
PipelineIntent identifier, the reused or created PipelineContract, the actual
PipelineRun identity, and typed analysis artifact and observation identifiers.

## Console

1. Open the Pharness console and select **Delivery Test** in the navigation.
2. Review the fixture scope. It must show
   `tekton-pipelines/pharness-e2e-noop`, no parameters or workspaces, and
   application impact `None`.
3. Select the acknowledgement checkbox and choose **Prepare preflight**.
4. Confirm the durable-record panel reports `Preflight Passed`. At this point
   Pharness has not created a PipelineRun.
5. Choose **Dispatch inert PipelineRun**.
6. Wait for the console status to become `Completed`. Its terminal receipt
   reports `succeeded`; then choose **Open delivery flow** to inspect the
   WorkPlan, ChangeSet, PipelineIntent, approval gates, audit events, and
   PipelineRun receipt.

The console path is intentionally equivalent to the CLI smoke. It creates
durable test records but does not change a finance application, read a secret,
or initiate a deployment.

The PipelineIntent returns to `approved` after a successful run; its execution
receipt reports `succeeded`. Those are separate authorization and execution
states.

## Synthetic Build-Output Fixture

`pharness-e2e-build-output` is a separate GitOps-managed, inert Pipeline that
proves the `pipeline_build_output` handoff. It has no inputs, workspaces,
secrets, registry credentials, network calls, source checkout, image build, or
application references. Its sole task writes these fixed synthetic result
markers to Tekton result files:

- `IMAGE_URL = example.invalid/pharness/e2e-build-output:synthetic`
- `IMAGE_DIGEST = sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef`

After the chart revision containing this fixture is merged and Argo reports it
as `Synced` and `Healthy`, run:

```sh
export PHARNESS_API_TOKEN='your operator token'
PHARNESS_TEKTON_SMOKE_PIPELINE=pharness-e2e-build-output \
PHARNESS_TEKTON_SMOKE_EXPECT_BUILD_OUTPUT=1 \
scripts/pharness-tekton-execution-smoke.sh --apply
```

The script requires a terminal `Succeeded=True` PipelineRun plus a verified
digest-pinned `pipeline_build_output` artifact. It records only synthetic
image identity; it does not establish that the image exists, is accessible,
signed, scanned, or deployable.

## Backlog

- Add a console link from terminal evidence directly to the typed
  PipelineRunAnalysis artifact and observation.
- Add an operator-reviewed artifact retention workflow before deleting any
  completed fixture PipelineRuns.
- Add a controlled failure variant that proves executor-loss reconciliation and
  late-callback rejection.
