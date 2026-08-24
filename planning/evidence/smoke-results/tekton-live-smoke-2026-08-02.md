# Tekton Live Smoke: 2026-08-02

## Result

The deployed Pharness control plane completed the bounded Tekton executor
smoke successfully against the `lucas_engineering` cluster. The smoke ran one
GitOps-managed inert PipelineRun and did not change any finance experiment or
other application resource.

## Exact Evidence

- Cluster context: `lucas_engineering`
- Pharness API Deployment: healthy and available in namespace `pharness`
- Fixture Pipeline: `tekton-pipelines/pharness-e2e-noop`
- PipelineIntent: `pint_1785681316878757224`
- PipelineContract: `pcontract_1783950712019126146`
- Executor-created PipelineRun:
  `tekton-pipelines/pharness-pint-1785681316878757224`
- Terminal condition: `Succeeded=True`, reason `Succeeded`
- TaskRuns: one `report` TaskRun, zero failed task runs
- Terminal execution artifact: `art_pipeline_execution_pexec_1785681317130134698`
- Typed analysis artifact: `art_pipeline_analysis_pexec_1785681317130134698`
- Typed analysis observation: `obs_pipeline_analysis_pexec_1785681317130134698`
- Proposed deployment handoff:
  `dint_1785681338117688251`
- Durable artifact directory:
  `target/tekton-execution-smoke/20260802T143514Z`

## What Ran

The script used the authenticated public API and normal control-plane routes;
it did not use a test-only backdoor. It created and audited an Observation,
Incident, RemediationPlan, approval gates, WorkPlan, ChangeSet, trusted
envelope, PipelineIntent, and PipelineContract. It then performed a dry-run
preflight before an explicit `--apply` dispatched the dedicated Tekton runner
Job.

That runner created and observed exactly one PipelineRun in the allowlisted
`tekton-pipelines` namespace. On `Succeeded=True`, Pharness persisted an
executor receipt and matching bounded `PipelineRunAnalysis`, attached the
analysis as satisfied PipelineIntent evidence, and created one proposed
DeploymentIntent handoff. The DeploymentIntent was not approved and no Argo
sync, Git write, registry action, secret read, source checkout, image build, or
application deployment occurred.

## Build-Output Status

The original `pharness-e2e-noop` run intentionally had no image outputs, so it
correctly persisted `build_output = null`. The separate GitOps-managed
`pharness-e2e-build-output` fixture was subsequently published and run live.
It emits fixed synthetic `IMAGE_URL` and `IMAGE_DIGEST` results only; it does
not build or push an image, authenticate to a registry, read a secret, or
change an application resource.

The successful build-output run produced the following durable evidence:

- API image: `registry.lucas.engineering/pharness-runtime@sha256:833fcdd93b7ae56d42a406af5cf2ce368c35db83889e598a29d15cd518235620`
- Git revision: `adce687403b0369749dc6ca2bbb330b09ba3713c`
- PipelineIntent: `pint_1785686655786860403`
- PipelineRun: `tekton-pipelines/pharness-pint-1785686655786860403`
- Terminal condition: `Succeeded=True`, one successful `report` TaskRun
- PipelineRunAnalysis artifact:
  `art_pipeline_analysis_pexec_1785686656033883908`
- PipelineRunAnalysis observation:
  `obs_pipeline_analysis_pexec_1785686656033883908`
- Verified build-output artifact:
  `art_pipeline_build_output_pexec_1785686656033883908`
- Recorded image reference:
  `example.invalid/pharness/e2e-build-output:synthetic@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef`
- Proposed-only DeploymentIntent handoff: `dint_1785686676567739621`
- Durable local evidence bundle:
  `target/tekton-execution-smoke/20260802T160413Z`

The smoke script waits for the asynchronous terminal analysis and optional
build-output record before it reports success. This closes a race where a
terminal PipelineRun receipt was visible before its follow-on durable evidence
had been attached.

The initial published runtime exposed a SQLite migration behavior specific to
SQLx: SQLite migrations run in transactions, so the historical table-rebuild
migrations could not toggle `foreign_keys` inside their SQL bodies. The runtime
now performs migrations through one dedicated connection with foreign-key
enforcement disabled, then opens the normal runtime pool with enforcement on.
The complete migration chain was tested on a WAL-safe backup of the live PVC,
followed by a clean `foreign_key_check`; the deployed API started healthy with
zero restarts after the corrective rollout.

To repeat the bounded build-output smoke:

```sh
export PHARNESS_API_TOKEN='your operator token'
PHARNESS_TEKTON_SMOKE_PIPELINE=pharness-e2e-build-output \
PHARNESS_TEKTON_SMOKE_EXPECT_BUILD_OUTPUT=1 \
scripts/pharness-tekton-execution-smoke.sh --apply
```

Success requires a terminal successful PipelineRun and a verified,
digest-pinned `pipeline_build_output` artifact. That proves data capture and
lineage only; it does not claim image availability, registry authentication,
signature/SBOM/provenance attestation, vulnerability posture, or deployment
readiness.
