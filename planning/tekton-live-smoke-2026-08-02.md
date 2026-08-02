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

This completed run validates the live typed Tekton execution path but does not
validate `pipeline_build_output`: the existing `pharness-e2e-noop` fixture has
no Tekton image-result outputs, and the persisted PipelineIntent correctly
shows `build_output = null`.

The repository now defines a separate GitOps-managed
`pharness-e2e-build-output` fixture. It emits fixed synthetic `IMAGE_URL` and
`IMAGE_DIGEST` result markers only and performs no OCI registry operation. The
fixture must be merged and allowed to sync through Argo before its live smoke
can run. This preserves the rule that disposable cluster fixtures are
GitOps-managed, rather than installed with an ad hoc `kubectl apply`.

After Argo is `Synced` and `Healthy` for the revision containing the fixture,
run:

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
