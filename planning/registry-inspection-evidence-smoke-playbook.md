# Registry Inspection Evidence Smoke Playbook

## Decisions

- Use this smoke to verify the one-step inspection-backed RegistryEvidence path.
- Keep this separate from the verified RegistryEvidence smoke because anonymous image identity inspection usually produces `verification_status = unknown`.
- A successful run proves the Release gate, direct registry capability audit, RegistryEvidence creation, and lifecycle audit are wired together.

## Backlog

- Add a live authenticated registry smoke only after Pharness has explicit registry credential management.
- Add signature, SBOM, provenance, and vulnerability checks before treating registry evidence as production-ready supply-chain verification.

## Start API

In one terminal:

```bash
CARGO_HOME="$PWD/target/cargo-home" \
CARGO_TARGET_DIR="$PWD/target" \
cargo run -p pharness-api
```

In a second terminal:

```bash
cd "$(git rev-parse --show-toplevel)"
```

## Prepare An Approved Release

Run `planning/release-smoke-playbook.md` through the first approved Release, then set:

```bash
RELEASE_ID="$(jq -r '.release.id' target/pharness-release-approved.json)"
```

If your approved Release output was saved under a different filename, replace `target/pharness-release-approved.json` with that file.

## Create RegistryEvidence From Anonymous Inspection

This deterministic smoke omits a registry host, so Pharness parses image identity without making a network call.

```bash
cargo run -p pharness-cli -- registry-evidence create-from-inspection \
  --release-id "$RELEASE_ID" \
  --image-ref team/checkout-api:v0.1.0-smoke \
  --actor lucas \
  --reason "registry inspection evidence smoke" \
  --timeout-ms 30000 \
  | tee target/pharness-registry-evidence-from-inspection.json
```

```bash
jq -e '.created == true and .inspection.status == "ok" and .inspection.executed == true and .registry_evidence.status == "proposed" and .registry_evidence.source == "registry_inspect_image" and .registry_evidence.verification_status == "unknown"' \
  target/pharness-registry-evidence-from-inspection.json
```

Expected result:

- The command exits successfully.
- `.inspection.action` is `registry_inspect_image`.
- `.inspection.result.content.image.repository` is `team/checkout-api`.
- `.registry_evidence.source` is `registry_inspect_image`.
- `.registry_evidence.verification_status` is `unknown`.
- This identity-only evidence is useful audit context, but it is not treated as supply-chain verification.

## Capture Evidence ID

```bash
REGISTRY_EVIDENCE_ID="$(jq -r '.registry_evidence.id' target/pharness-registry-evidence-from-inspection.json)"
```

```bash
test -n "$REGISTRY_EVIDENCE_ID" && test "$REGISTRY_EVIDENCE_ID" != "null"
```

Expected result:

- `REGISTRY_EVIDENCE_ID` is non-empty.

## Verify RegistryEvidence Audit

```bash
cargo run -p pharness-cli -- audit-events \
  --resource-kind registry_evidence \
  --resource-id "$REGISTRY_EVIDENCE_ID" \
  | tee target/pharness-registry-evidence-from-inspection-audit.json
```

```bash
jq -e '[.events[].kind] | index("registry_evidence.proposed") != null' \
  target/pharness-registry-evidence-from-inspection-audit.json
```

```bash
jq -e '.events[] | select(.kind == "registry_evidence.proposed") | .payload.extra.source == "registry_inspection" and .payload.extra.execution_enabled == true' \
  target/pharness-registry-evidence-from-inspection-audit.json
```

Expected result:

- Audit events include `registry_evidence.proposed`.
- The proposed event records `extra.source = registry_inspection`.
- The proposed event records `extra.execution_enabled = true`.

## Verify Direct Capability Audit

```bash
cargo run -p pharness-cli -- audit-events \
  --resource-kind capability \
  --resource-id registry_inspect_image \
  | tee target/pharness-registry-inspection-capability-audit.json
```

```bash
jq -e '.events[] | select(.kind == "direct_capability.executed") | .payload.executed == true and .payload.result.image.repository == "team/checkout-api"' \
  target/pharness-registry-inspection-capability-audit.json
```

Expected result:

- Audit events include `direct_capability.executed`.
- The capability audit summary includes the image repository.
- The capability audit summary does not contain a manifest body.

## Duplicate Create Is Idempotent

```bash
cargo run -p pharness-cli -- registry-evidence create-from-inspection \
  --release-id "$RELEASE_ID" \
  --image-ref team/checkout-api:v0.1.0-smoke \
  --actor lucas \
  --reason "registry inspection evidence duplicate smoke" \
  --timeout-ms 30000 \
  | tee target/pharness-registry-evidence-from-inspection-duplicate.json
```

```bash
jq -e --arg id "$REGISTRY_EVIDENCE_ID" '.created == false and .registry_evidence.id == $id' \
  target/pharness-registry-evidence-from-inspection-duplicate.json
```

Expected result:

- Duplicate create returns the existing RegistryEvidence row.
- The duplicate call still runs the read-only inspection capability and writes direct capability audit.

## Optional Live Registry Probe

Only run this if the registry is public or intentionally supports anonymous manifest reads.

```bash
cargo run -p pharness-cli -- registry-evidence create-from-inspection \
  --release-id "$RELEASE_ID" \
  --image-ref registry.example.test/checkout-api:v0.1.0-smoke \
  --registry-base-url https://registry.example.test \
  --actor lucas \
  --reason "live registry inspection evidence smoke" \
  --timeout-ms 30000
```

Expected optional result:

- Public or unauthenticated registries may return `verification_status = "verified"` or `"mismatch"`.
- Inaccessible or authenticated registries may return a tool error or `verification_status = "unknown"`.
- Pharness should not read registry credentials, Kubernetes secrets, docker configs, or image pull secrets.
- Even if a live registry probe returns `verification_status = "verified"`, readiness should still require richer signature, SBOM, provenance, attestation, or vulnerability evidence before treating the row as supply-chain verified.
