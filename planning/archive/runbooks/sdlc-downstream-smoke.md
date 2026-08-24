# SDLC Downstream Smoke

## Decisions

- Extend `scripts/pharness-e2e-smoke.sh` beyond root creation into the durable delivery chain:
  - `WorkPlan`
  - `ChangeSet`
  - trusted ChangeSet envelope
  - `PipelineIntent`
  - `DeploymentIntent`
  - `Release`
  - `RegistryEvidence`
  - ChangeSet readiness

- Keep the smoke deterministic and provider-independent.
  - The model-backed run remains optional and only executes when `FIREWORKS_API_KEY` is exported.
  - The downstream chain uses public CLI/API commands only.

- Require the same lifecycle transitions as the backend.
  - WorkPlan and ChangeSet move through `draft -> proposed -> approved`.
  - PipelineIntent, DeploymentIntent, and Release are approved before downstream resources are created.
  - RegistryEvidence is created and then transitioned to `verified`.

- Use a ChangeSet trusted envelope for readiness.
  - The readiness check should prove that an approved ChangeSet with an active envelope and verified registry evidence has no blockers.
  - Inspection-backed registry evidence may still leave warnings when it lacks stronger supply-chain verification.

- Add negative readiness checks around approval invalidation.
  - An approved ChangeSet without a trusted envelope is blocked by `missing_active_trusted_envelope`.
  - A material ChangeSet revision resets the ChangeSet to `draft`, stales dependent delivery records, and stales the prior trusted envelope.

- Add a live read-only cluster path without replacing deterministic SDLC data yet.
  - `scripts/pharness-e2e-smoke.sh --cluster` reads Tekton PipelineRuns and TaskRuns.
  - If at least one PipelineRun exists, it runs `tekton_analyze_pipeline_run` for a concrete namespace/name.
  - Direct Tekton analysis now persists a runless `pipeline_run_analysis` artifact and a `tekton/pipeline_run_analysis` observation.
  - The persisted Tekton observation is attached to the approved PipelineIntent before DeploymentIntent creation.
  - Registry mismatch in the attached evidence produces `intent_json.evidence.status = attention_required`.
  - DeploymentIntent creation inherits the attached PipelineIntent evidence into `intent_json.pipeline_evidence` and records `deploy_ready = false` when evidence requires review.
  - When `PHARNESS_E2E_ARGO_APP` is set, the smoke reads that Argo app, attaches the persisted observation to the approved DeploymentIntent, and verifies Release creation inherits `release_json.deployment_evidence`.
  - RegistryEvidence is created from the typed `registry_inspect_image` capability instead of manual JSON evidence.
  - Registry inspection currently proves image identity only in the portable path, so ChangeSet readiness retains `registry_evidence_verification_not_verified`.
  - Argo, Prometheus, and Loki checks are opt-in so the default smoke remains portable.

## Backlog

- Replace remaining synthetic SDLC data with durable live evidence from Tekton, Argo, registry, and LGTM.
  - Add richer registry verification for signature, SBOM, provenance, attestation, and vulnerability evidence.
  - Add LGTM/Prometheus/Loki evidence attachment to Release readiness.

- Add optional JSON summary extraction so the script can print IDs for WorkPlan, ChangeSet, PipelineIntent, DeploymentIntent, Release, and RegistryEvidence without requiring users to inspect artifacts.

## Verification

- `scripts/pharness-e2e-smoke.sh` passed with deterministic downstream checks.
  - Positive readiness passed after WorkPlan, ChangeSet, trusted envelope, PipelineIntent, DeploymentIntent, Release, and RegistryEvidence were all in approved or verified states.
  - Missing-envelope readiness failed before the trusted envelope was created.
  - Revision readiness failed after a material ChangeSet change and exposed stale downstream evidence.
  - Latest artifact directory: `target/e2e-smoke/20260611T223022Z`.

- `PHARNESS_E2E_ARGO_APP=ghost scripts/pharness-e2e-smoke.sh --cluster --no-model` passed against the current Kubernetes context.
  - Read 15 PipelineRuns and 31 TaskRuns through the typed read-only Tekton capabilities.
  - Analyzed PipelineRun `tekton-pipelines/escape-backend-manual-9l9zj`.
  - Verified the resulting PipelineRunAnalysis artifact and Observation can be fetched by ID and found through the Observation index.
  - Verified the Observation attaches to an approved PipelineIntent and records `attention_required` because image alignment is `registry_mismatch`.
  - Verified the downstream DeploymentIntent inherits that evidence with `deploy_ready = false`.
  - Verified the Argo `ghost` observation attaches to an approved DeploymentIntent and records `satisfied`.
  - Verified the downstream Release inherits that evidence with `release_ready = true`.
  - Verified RegistryEvidence is created from `registry_inspect_image`, then lifecycle-verified, while readiness keeps `registry_evidence_verification_not_verified`.
  - Latest artifact directory: `target/e2e-smoke/20260615T182237Z`.
