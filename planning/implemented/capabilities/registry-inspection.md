# Registry Inspection

## Decisions

- Add `registry_inspect_image` as a direct read-only capability before exposing it as a model-loop tool.
- Keep the capability anonymous. Pharness does not read registry credentials, image pull secrets, docker config files, or Kubernetes secrets in this slice.
- Accept an `image_ref` and optional `registry_base_url`. If no registry can be inferred, the tool still returns parsed image identity with `verification_status = unknown`.
- Probe OCI/Docker manifests with `HEAD`, falling back to `GET` only when the registry reports `405 Method Not Allowed`. The response body is not persisted in tool results or audit summaries.
- Treat authenticated or inaccessible registries as `verification_status = unknown` rather than attempting credential discovery.
- Store only compact direct-capability audit fields for registry inspection: image identity, verification status, probe status, probe accessibility, and probe digest.
- Add `POST /api/registry-evidence/from-registry-inspection` and `registry-evidence create-from-inspection` as the one-step bridge from successful anonymous registry inspection to durable `RegistryEvidence`.
- Keep inspection-backed evidence on the same Release-gated lifecycle as manual RegistryEvidence. The Release must be approved, duplicate creates return the existing row, and stale rows are re-proposed.
- Default inspection-backed evidence to the registry tool's verification status. Identity-only inspections remain `unknown`; they do not masquerade as verified supply-chain checks.
- Audit both sides of the bridge: direct capability execution remains a `direct_capability.*` audit fact, and RegistryEvidence creation remains a `registry_evidence.*` audit fact.

## Backlog

- Add optional allowlisted registry auth from explicit Pharness-managed credentials, not ambient kubeconfigs, docker configs, or secrets.
- Add signature, SBOM, provenance, and vulnerability inspection after the manifest read path is stable.
- Expose registry inspection as a model-loop tool once the auto-recording policy for RegistryEvidence is defined.
- Add production policy checks that require digest matching and signed images before Release execution.
