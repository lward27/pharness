# E2E Smoke

## Decisions

- Add `scripts/pharness-e2e-smoke.sh` as the first repeatable smoke runner for the machine-facing runtime.
  - It starts a fresh local API instance with an isolated SQLite database.
  - It verifies health, effective config, empty run summary, direct capability execution, secret-read denial, and audit events.
  - It runs a model-backed agent loop only when `FIREWORKS_API_KEY` is present, because local backend checks should still work without live provider access.

- Keep this smoke test focused on the public API and CLI contract.
  - The script does not seed SQLite directly.
  - Direct database seeding would hide gaps in the machine-facing API that Codex and future workers need.

- Treat the current smoke as a control-plane smoke, not a full autonomous SDLC smoke.
  - It now creates deterministic SDLC roots through public API and CLI surfaces.
  - It now drives WorkPlan through RegistryEvidence and ChangeSet readiness.
  - It now verifies negative readiness paths for missing trusted envelopes and material ChangeSet revisions.
  - Live cluster evidence is opt-in through `scripts/pharness-e2e-smoke.sh --cluster`.

- Model material revisions as approval invalidation events.
  - A revised ChangeSet returns to `draft`.
  - Dependent PipelineIntent, DeploymentIntent, Release, and RegistryEvidence records become `stale`.
  - Prior trusted envelopes remain auditable, but no longer satisfy readiness for the revised ChangeSet.

- Keep the deterministic smoke as the default and make live checks explicit.
  - `--no-model` forces provider-independent execution.
  - `--model` requires `FIREWORKS_API_KEY` and fails fast if it is missing.
  - `--cluster` runs read-only Tekton inventory and analyzes one concrete PipelineRun when one exists.
  - Argo, Prometheus, and Loki checks stay environment-gated by `PHARNESS_E2E_ARGO_APP`, `PHARNESS_PROMETHEUS_URL`, `PHARNESS_LOKI_URL`, and `PHARNESS_E2E_LOKI_QUERY`.

- Persist successful direct cluster capability results as durable evidence.
  - Direct read-only Kubernetes, Argo, Tekton, Prometheus, and Loki capability calls now return optional `artifact_id` and `observation_id`.
  - The persisted artifact keeps the compact tool result JSON.
  - The persisted observation indexes source, kind, subject, normalized resource identity, artifact id, and compact data.
  - Direct capability evidence is runless and owned by a control-plane session rather than a fake run.

- Attach live PipelineRunAnalysis observations to approved PipelineIntents.
  - Cluster smoke attaches the persisted Tekton observation to the approved PipelineIntent before DeploymentIntent creation.
  - Attached evidence updates `intent_json.evidence` and records `pipeline_intent.evidence_attached`.
  - Lifecycle status remains `approved`; evidence status is separate and can be `attention_required`.
  - DeploymentIntent creation inherits that evidence into `intent_json.pipeline_evidence`, including `deploy_ready` and `review_required`, so downstream state does not treat lifecycle approval as proof of deploy readiness.

- Attach live Argo Application observations to approved DeploymentIntents when `PHARNESS_E2E_ARGO_APP` is set.
  - Cluster smoke reads the configured Argo app, persists an artifact and observation, and attaches the observation to the approved DeploymentIntent before Release creation.
  - Attached evidence updates `intent_json.deployment_evidence` and records `deployment_intent.evidence_attached`.
  - Release creation inherits that evidence into `release_json.deployment_evidence`, including `release_ready` and `review_required`.

- Attach Release observability evidence in every smoke run.
  - Cluster smoke reuses a persisted Prometheus inventory or Loki log summary observation when one exists.
  - Deterministic smoke creates a synthetic Prometheus inventory observation so the Release evidence API and CLI path are still exercised without live LGTM endpoints.
  - Attached evidence updates `release_json.observability_evidence` and records `release.evidence_attached`.
  - Deterministic smoke also attaches a synthetic attention-required Prometheus inventory observation to prove Release observability can create a candidate Incident, draft RemediationPlan, approval gates, and audit event.
  - Final ChangeSet readiness asserts that `missing_release_observability_evidence` is absent while keeping registry supply-chain and Release observability warnings visible.

- Fetch the ChangeSet SDLC flow in every smoke run.
  - The flow response must include the WorkPlan, ChangeSet, PipelineIntent, DeploymentIntent, Release, RegistryEvidence, readiness, release-observability Incident, RemediationPlan, ApprovalGates, and audit event.
  - This keeps the machine-facing aggregate endpoint under the same public CLI/API contract as the rest of the smoke.

- Fetch the WorkPlan SDLC flow before creating a ChangeSet.
  - The response must be rooted at the WorkPlan, keep `change_set` and downstream intent fields null, include the root RemediationPlan, and surface `missing_change_set` as a readiness warning.
  - This gives the UI a stable read model for agent planning state before source changes exist.

- Prove the run event stream cursor in the deterministic smoke.
  - The smoke creates a harmless run, cancels it through `pharness-cli runs cancel`, then calls `pharness-cli runs events --stream --after-seq N`.
  - The streamed SSE payload must contain only events after `N`; it must not replay the cursor event.
  - This keeps the browser Run Detail live-event contract covered without requiring Fireworks or cluster access.

## Backlog

- Add public, policy-aware creation for standalone `ApprovalGate` only if operators need gates outside WorkPlan or ChangeSet lifecycle flows.

- Add an assertion helper that records expected and actual JSON fragments for failed checks, so smoke failures are easier to review without replaying the command.

- Add documented registry aliases for homelab image identity.
  - Latest cluster smoke showed Tekton/Argo analysis healthy, but image alignment reported `registry_mismatch` when comparing `docker-registry.registry.svc.cluster.local:5000` and `registry.lucas.engineering`.
  - The current smoke should expose that state rather than marking it as failure until registry equivalence is configured.

## Verification

- `scripts/pharness-e2e-smoke.sh --no-model` passed against a fresh local API instance.
  - Verified the deterministic run event stream cursor contract through `pharness-cli runs events --stream`.
  - With `after_seq` set to the queued event sequence, the SSE replay returned only the later `run.cancelled` event.
  - Manifest includes `event_stream_cursor`.
  - Model and cluster checks were explicitly skipped.
  - Latest artifact directory: `target/e2e-smoke/20260702T172921Z`.

- `scripts/pharness-e2e-smoke.sh --no-model` passed against a fresh local API instance.
  - Verified deterministic Release observability attachment for both clean evidence and attention-required Prometheus inventory evidence.
  - Verified attention-required Release observability creates a candidate Incident, draft RemediationPlan, four pending approval gates, and a `remediation_plan.created` audit event.
  - Verified final ChangeSet readiness is still unblocked while preserving `release_observability_attention_required`.
  - Verified ChangeSet flow aggregates the SDLC chain, Release observability remediation artifacts, gates, and audit event.
  - Manifest reported `release_observability = attention_required` and `release_observability_remediation = created`.
  - Latest artifact directory: `target/e2e-smoke/20260626T131743Z`.

- `scripts/pharness-e2e-smoke.sh --no-model` passed against a fresh local API instance.
  - Verified deterministic Release observability evidence creation and attachment through `pharness-cli observations create` and `pharness-cli releases attach-evidence`.
  - Verified final ChangeSet readiness is unblocked after Release observability and RegistryEvidence are present.
  - Verified `missing_release_observability_evidence` is absent while `registry_evidence_verification_not_verified` remains visible.
  - Manifest reported `release_observability = observed`.
  - Latest artifact directory: `target/e2e-smoke/20260615T190215Z`.

- `scripts/pharness-e2e-smoke.sh --no-model` passed against a fresh local API instance.
  - Verified the deterministic control-plane path without Fireworks or live cluster reads.
  - Verified DeploymentIntent creation records missing upstream pipeline evidence as `intent_json.pipeline_evidence.status = missing` and `deploy_ready = false`.
  - Verified Release creation records missing upstream deployment evidence as `release_json.deployment_evidence.status = missing` and `release_ready = false`.
  - Verified RegistryEvidence is created from `registry_inspect_image` using `team/checkout-api:v0.1.0-smoke`.
  - Verified lifecycle-verified inspection evidence keeps `registry_evidence_verification_not_verified` as a warning when no signature/SBOM/provenance data is present.
  - Latest artifact directory: `target/e2e-smoke/20260615T182202Z`.

- `PHARNESS_E2E_ARGO_APP=ghost scripts/pharness-e2e-smoke.sh --cluster --no-model` passed against the current Kubernetes context.
  - Verified 15 PipelineRuns and 31 TaskRuns through typed read-only capabilities.
  - Analyzed PipelineRun `tekton-pipelines/escape-backend-manual-9l9zj`.
  - Verified direct Tekton analysis persisted artifact and observation.
  - Verified the observation index can find the PipelineRunAnalysis by `source=tekton`, `kind=pipeline_run_analysis`, `resource_namespace=tekton-pipelines`, `resource_kind=PipelineRun`, and `resource_name=escape-backend-manual-9l9zj`.
  - Verified the persisted Tekton observation attaches to the approved PipelineIntent.
  - Verified DeploymentIntent creation inherits the attached evidence as `intent_json.pipeline_evidence.status = attention_required` and `deploy_ready = false`.
  - Verified direct Argo app read for `ghost` persists observation `obs_direct_argo_get_app_1781521948426738000`.
  - Verified the persisted Argo observation attaches to the approved DeploymentIntent as `intent_json.deployment_evidence.status = satisfied` and `deploy_ready = true`.
  - Verified Release creation inherits the attached deployment evidence as `release_json.deployment_evidence.status = satisfied` and `release_ready = true`.
  - Verified RegistryEvidence is created from `registry_inspect_image`, lifecycle-verified, and still carries `verification_status = unknown`.
  - Verified registry mismatch produces `intent_json.evidence.status = attention_required` instead of hiding the risk as satisfied.
  - Prometheus and Loki checks were skipped because their opt-in env vars were not set.
  - Latest artifact directory: `target/e2e-smoke/20260615T182237Z`.

- `scripts/pharness-e2e-smoke.sh` passed against a fresh local API instance.
  - Verified health, effective config, empty run summary, secret-read denial, registry inspection, capability audit events, deterministic SDLC root creation, downstream SDLC resource creation, trusted envelope creation, registry evidence verification, ChangeSet readiness, missing-envelope blocking, and material-revision invalidation.
  - Model-backed run was skipped because `FIREWORKS_API_KEY` was not exported in the process environment.
  - Latest artifact directory: `target/e2e-smoke/20260611T223022Z`.

- `bash -n scripts/pharness-e2e-smoke.sh` passed.

- `cargo fmt --all` passed.

- `git diff --check` passed.

- `cargo clippy --workspace --all-targets -- -D warnings` passed.

- `cargo test --workspace` passed.
