# RegistryEvidence

## Decisions

- Add durable `RegistryEvidence` records as the reviewable evidence handoff after an approved `Release`.
- Keep V1 registry evidence non-mutating. Evidence can be manual, API-fed, or informed by the anonymous `registry_inspect_image` direct capability. Pharness does not yet authenticate to an OCI registry or run signature/SBOM/vulnerability verification.
- Allow one current `RegistryEvidence` row per `Release` in V1. Repeated create requests return the existing row unless it is stale, in which case the same row is re-proposed.
- Require the parent `Release` to be `approved` before registry evidence can be proposed.
- Use the status graph `proposed -> verified` or `proposed -> rejected`. Verified evidence may be rejected later. Material upstream changes mark evidence `stale`.
- Track both lifecycle status and evidence verification status. A row can be lifecycle `verified` only after explicit transition; its `verification_status` records the evidence claim (`verified`, `unverified`, `mismatch`, or `unknown`).
- Expose RegistryEvidence through `POST /api/registry-evidence/from-release`, `POST /api/registry-evidence/from-registry-inspection`, `GET /api/registry-evidence`, `GET /api/registry-evidence/:evidence_id`, and `POST /api/registry-evidence/:evidence_id/transition`, with matching CLI commands.
- `create-from-inspection` runs the read-only `registry_inspect_image` capability and records successful output as proposed RegistryEvidence for an approved Release.
- ChangeSet readiness reports RegistryEvidence state after the Release is approved. Missing, stale, non-verified, or verification-not-verified evidence is a warning in V1.
- Inspection-backed evidence is treated as image identity/probe evidence unless it also includes signature, SBOM, provenance, attestation, or vulnerability-check data. Lifecycle-verified inspection evidence without those richer checks emits `registry_evidence_supply_chain_not_verified`.
- The e2e SDLC chain now uses `create-from-inspection` instead of manual RegistryEvidence.
  - The portable smoke uses `team/checkout-api:v0.1.0-smoke`, which parses image identity without probing a remote registry.
  - The resulting evidence records `source = registry_inspect_image` and `verification_status = unknown`.
  - Operators can still transition the RegistryEvidence lifecycle to `verified`; readiness remains unblocked but warns that registry verification is not verified.

## Backlog

- Add explicit registry credential management before live private registry verification.
- Extend registry inspection to signatures, SBOMs, and vulnerability metadata without exposing registry credentials in logs.
- Support multiple image artifacts per Release once PipelineIntent execution produces structured build outputs.
- Add provenance checks that link Tekton build output, registry digest, Git commit, and Argo image inputs into one evidence bundle.
- Add policy thresholds for registry evidence, such as blocking production Release execution on unsigned images, digest mismatch, or critical vulnerabilities.
- Add UI panels for image metadata, verification status, and evidence history.
