# RegistryEvidence Smoke Playbook

## Decisions

- Use this as the update smoke for the durable RegistryEvidence slice.
- Run the Release smoke first or otherwise start with an approved `Release`.
- RegistryEvidence can be manual or inspection-backed. This smoke uses manual verified evidence so readiness can clear the verification warning.
- A material ChangeSet revision should stale PipelineIntent, DeploymentIntent, Release, and RegistryEvidence.

## Backlog

- Use `planning/registry-inspection-evidence-smoke-playbook.md` for the one-step read-only registry inspection path.
- Add production policy smoke coverage once registry evidence becomes a blocker for production Release execution.

# RegistryEvidence Smoke Playbook

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
CHANGE_SET_ID="$(jq -r '.release.change_set_id' target/pharness-release-approved.json)"
```

If your approved Release output was saved under a different filename, replace `target/pharness-release-approved.json` with that file.

## Confirm Readiness Warns Before Evidence

```bash
cargo run -p pharness-cli -- change-sets readiness \
  --change-set-id "$CHANGE_SET_ID" \
  | tee target/pharness-registry-readiness-before.json
```

```bash
jq -e '[.warnings[].code] | index("missing_registry_evidence") != null' \
  target/pharness-registry-readiness-before.json
```

Expected result:

- The `jq` command exits successfully.
- Readiness includes `missing_registry_evidence`.

## Inspect Image Identity

This command is anonymous and read-only. The deterministic smoke below omits a registry host so it parses image identity without making a network call.

```bash
cargo run -p pharness-cli -- capabilities registry-inspect-image \
  --image-ref team/checkout-api:v0.1.0-smoke \
  --timeout-ms 30000 \
  | tee target/pharness-registry-inspect-image.json
```

```bash
jq -e '.action == "registry_inspect_image" and .executed == true and .result.content.source == "registry"' \
  target/pharness-registry-inspect-image.json
```

Expected result:

- The command executes through the direct capability endpoint.
- The result includes parsed image identity under `.result.content.image`.
- `.result.content.verification_status` is `unknown`.

Optional live registry probe:

```bash
cargo run -p pharness-cli -- capabilities registry-inspect-image \
  --image-ref registry.example.test/checkout-api:v0.1.0-smoke \
  --registry-base-url https://registry.example.test \
  --timeout-ms 30000
```

Expected optional result:

- Public or unauthenticated registries may return `verification_status = "verified"` or `"mismatch"`.
- Inaccessible or authenticated registries may return a tool error or `verification_status = "unknown"`.
- Pharness should not read registry credentials.

## Create Or Fetch RegistryEvidence

```bash
cargo run -p pharness-cli -- registry-evidence create-from-release \
  --release-id "$RELEASE_ID" \
  --registry registry.example.test \
  --repository checkout-api \
  --image-ref registry.example.test/checkout-api:v0.1.0-smoke \
  --image-digest sha256:deadbeef \
  --tag v0.1.0-smoke \
  --source manual \
  --verification-status verified \
  --actor lucas \
  --reason "registry evidence smoke" \
  | tee target/pharness-registry-evidence-created.json
```

```bash
REGISTRY_EVIDENCE_ID="$(jq -r '.registry_evidence.id' target/pharness-registry-evidence-created.json)"
jq -e '.created == true and .registry_evidence.status == "proposed" and .registry_evidence.verification_status == "verified"' \
  target/pharness-registry-evidence-created.json
```

Expected result:

- `REGISTRY_EVIDENCE_ID` is non-empty.
- Evidence status is `proposed`.
- Verification status is `verified`.

## Verify List And Get

```bash
cargo run -p pharness-cli -- registry-evidence list \
  --release-id "$RELEASE_ID" \
  --status proposed \
  --verification-status verified \
  --limit 10 \
  | tee target/pharness-registry-evidence-list.json
```

```bash
jq -e --arg id "$REGISTRY_EVIDENCE_ID" \
  '.count == 1 and .registry_evidence[0].id == $id' \
  target/pharness-registry-evidence-list.json
```

```bash
cargo run -p pharness-cli -- registry-evidence get \
  --evidence-id "$REGISTRY_EVIDENCE_ID" \
  | tee target/pharness-registry-evidence-get.json
```

```bash
jq -e --arg id "$REGISTRY_EVIDENCE_ID" '.id == $id' \
  target/pharness-registry-evidence-get.json
```

Expected result:

- List returns exactly one proposed RegistryEvidence row for the Release.
- Get returns the same row.

## Readiness Before Evidence Verification

```bash
cargo run -p pharness-cli -- change-sets readiness \
  --change-set-id "$CHANGE_SET_ID" \
  | tee target/pharness-registry-readiness-proposed.json
```

```bash
jq -e '[.warnings[].code] | index("registry_evidence_not_verified") != null' \
  target/pharness-registry-readiness-proposed.json
```

Expected result:

- Readiness warns that RegistryEvidence is proposed, not lifecycle-verified.

## Verify The Evidence

```bash
cargo run -p pharness-cli -- registry-evidence transition \
  --evidence-id "$REGISTRY_EVIDENCE_ID" \
  --target-status verified \
  --actor lucas \
  --reason "registry evidence verified" \
  | tee target/pharness-registry-evidence-verified.json
```

```bash
jq -e '.registry_evidence.status == "verified" and .registry_evidence.verification_status == "verified"' \
  target/pharness-registry-evidence-verified.json
```

Expected result:

- RegistryEvidence status is `verified`.
- Verification status remains `verified`.

## Readiness After Evidence Verification

```bash
cargo run -p pharness-cli -- change-sets readiness \
  --change-set-id "$CHANGE_SET_ID" \
  | tee target/pharness-registry-readiness-verified.json
```

```bash
jq -e '[.warnings[].code] | index("missing_registry_evidence") == null and index("registry_evidence_not_verified") == null and index("registry_evidence_verification_not_verified") == null' \
  target/pharness-registry-readiness-verified.json
```

```bash
jq -e '[.warnings[].code] | index("registry_evidence_supply_chain_not_verified") == null' \
  target/pharness-registry-readiness-verified.json
```

Expected result:

- RegistryEvidence appears in `.registry_evidence`.
- Registry evidence lifecycle, verification-status, and supply-chain warning codes are absent for this manual verified V1 smoke.

## Audit Events

```bash
cargo run -p pharness-cli -- audit-events \
  --resource-kind registry_evidence \
  --resource-id "$REGISTRY_EVIDENCE_ID" \
  | tee target/pharness-registry-evidence-audit.json
```

```bash
jq -e '[.events[].kind] | index("registry_evidence.proposed") != null and index("registry_evidence.verified") != null' \
  target/pharness-registry-evidence-audit.json
```

Expected result:

- Audit events include `registry_evidence.proposed` and `registry_evidence.verified`.

## Optional Stale Propagation Check

Only run this section if you are ready to revise the ChangeSet. It intentionally stales the current PipelineIntent, DeploymentIntent, Release, and RegistryEvidence.

```bash
cargo run -p pharness-cli -- change-sets revise \
  --change-set-id "$CHANGE_SET_ID" \
  --change-set-json '{"changes":[{"path":"registry/evidence-smoke.yaml","operation":"update","summary":"Force RegistryEvidence invalidation smoke"}],"rollback":"restore prior registry evidence smoke state"}' \
  --summary "Force registry evidence invalidation smoke" \
  --actor lucas \
  --reason "registry evidence stale smoke" \
  | tee target/pharness-registry-stale-revision.json
```

```bash
jq -e '.invalidated_registry_evidence.status == "stale"' \
  target/pharness-registry-stale-revision.json
```

Expected result:

- `invalidated_registry_evidence.status` is `stale`.
