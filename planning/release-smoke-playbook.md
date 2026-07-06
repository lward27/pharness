# Decisions

- Add this as the update smoke for the durable Release slice.
- Reuse the current DeploymentIntent produced by `planning/deployment-intent-smoke-playbook.md`.
- Approve the current DeploymentIntent before creating a Release because release review starts only after deployment intent approval.
- Keep the smoke non-mutating. It creates and approves control-plane release records only; it does not sync Argo CD or promote images.
- Treat a missing, stale, or non-approved Release as a ChangeSet readiness warning in V1.
- A material ChangeSet revision should stale the current PipelineIntent, DeploymentIntent, and Release.
- Re-running create-from-DeploymentIntent after the current DeploymentIntent is approved should re-propose the same stale Release row.

# Backlog

- Extend this playbook when Release execution, promotion, rollback, and verification become separate approved capabilities.
- Add production-impacting Release smoke coverage once production policy gates exist.

# Release Smoke Playbook

Run every command from the repository root. The API should already be running with `PHARNESS_API_URL=http://127.0.0.1:4777`.

## Common Environment

```sh
export PHARNESS_API_URL=http://127.0.0.1:4777
export CARGO_TARGET_DIR=target
mkdir -p target
```

## Load The Current DeploymentIntent

```sh
DEPLOYMENT_INTENT_ID="$(jq -r '.deployment_intent.id // .id // empty' target/pharness-deployment-intent-approved-again.json 2>/dev/null || true)"
if [ -z "$DEPLOYMENT_INTENT_ID" ]; then DEPLOYMENT_INTENT_ID="$(jq -r '.deployment_intent.id // .id // empty' target/pharness-deployment-intent-approved.json)"; fi
test -n "$DEPLOYMENT_INTENT_ID"
```

Expected signal:

- `DEPLOYMENT_INTENT_ID` is non-empty.
- If this check fails, run `planning/deployment-intent-smoke-playbook.md` through DeploymentIntent approval first.

## Approve The Current DeploymentIntent

```sh
cargo run -p pharness-cli -- deployment-intents transition \
  --deployment-intent-id "$DEPLOYMENT_INTENT_ID" \
  --target-status approved \
  --actor lucas \
  --reason "release smoke requires approved deployment intent" \
  | tee target/pharness-release-deployment-intent-approved.json
```

```sh
jq -e '.deployment_intent.status == "approved"' target/pharness-release-deployment-intent-approved.json
```

Expected signal:

- The current DeploymentIntent status is `approved`.

## Create Or Fetch The Release

```sh
cargo run -p pharness-cli -- releases create-from-deployment-intent \
  --deployment-intent-id "$DEPLOYMENT_INTENT_ID" \
  --version v0.1.0-smoke \
  --commit-sha abc1234 \
  --image-digest sha256:deadbeef \
  --rollback-ref previous-release \
  --actor lucas \
  --reason "release smoke" \
  | tee target/pharness-release.json
```

```sh
RELEASE_ID="$(jq -r '.release.id' target/pharness-release.json)"
CHANGE_SET_ID="$(jq -r '.release.change_set_id' target/pharness-release.json)"
test -n "$RELEASE_ID"
test -n "$CHANGE_SET_ID"
jq '{created, id: .release.id, status: .release.status, release_kind: .release.release_kind, target_environment: .release.target_environment, target_namespace: .release.target_namespace, argo_application: .release.argo_application, version: .release.version, commit_sha: .release.commit_sha, image_digest: .release.image_digest, execution_enabled: .release.release_json.execution.enabled}' target/pharness-release.json
```

Expected signal:

- `RELEASE_ID` is non-empty.
- `CHANGE_SET_ID` is non-empty.
- `status` is `proposed`.
- `release_kind` is `gitops_release`.
- `version` is `v0.1.0-smoke`.
- `commit_sha` is `abc1234`.
- `image_digest` is `sha256:deadbeef`.
- `execution_enabled` is `false`.

## Verify Idempotent Create

```sh
cargo run -p pharness-cli -- releases create-from-deployment-intent \
  --deployment-intent-id "$DEPLOYMENT_INTENT_ID" \
  --title "ignored duplicate release title" \
  --actor lucas \
  --reason "release duplicate smoke" \
  | tee target/pharness-release-existing.json
```

```sh
jq --arg release_id "$RELEASE_ID" \
  -e '.created == false and .release.id == $release_id' \
  target/pharness-release-existing.json
```

Expected signal:

- The duplicate create returns `created = false`.
- The returned id matches `RELEASE_ID`.

## List And Fetch

```sh
cargo run -p pharness-cli -- releases list \
  --deployment-intent-id "$DEPLOYMENT_INTENT_ID" \
  --change-set-id "$CHANGE_SET_ID" \
  --status proposed \
  --release-kind gitops_release \
  --version v0.1.0-smoke \
  --commit-sha abc1234 \
  --image-digest sha256:deadbeef \
  --limit 10 \
  | tee target/pharness-releases-list.json
```

```sh
jq --arg release_id "$RELEASE_ID" \
  -e '.count == 1 and .releases[0].id == $release_id' \
  target/pharness-releases-list.json
```

```sh
cargo run -p pharness-cli -- releases get \
  --release-id "$RELEASE_ID" \
  | tee target/pharness-release-detail.json
```

```sh
jq --arg release_id "$RELEASE_ID" \
  -e '.id == $release_id and .status == "proposed"' \
  target/pharness-release-detail.json
```

Expected signal:

- List returns exactly one proposed Release for the DeploymentIntent.
- Get returns the same proposed Release.

## Readiness Before Release Approval

```sh
cargo run -p pharness-cli -- change-sets readiness \
  --change-set-id "$CHANGE_SET_ID" \
  | tee target/pharness-release-readiness-before.json
```

```sh
jq -e '[.warnings[].code] | index("release_not_approved") != null' \
  target/pharness-release-readiness-before.json
```

Expected signal:

- Readiness includes warning code `release_not_approved`.

## Approve The Release

```sh
cargo run -p pharness-cli -- releases transition \
  --release-id "$RELEASE_ID" \
  --target-status approved \
  --actor lucas \
  --reason "release smoke approved" \
  | tee target/pharness-release-approved.json
```

```sh
jq -e '.release.status == "approved"' target/pharness-release-approved.json
```

Expected signal:

- The Release status is `approved`.

## Readiness After Release Approval

```sh
cargo run -p pharness-cli -- change-sets readiness \
  --change-set-id "$CHANGE_SET_ID" \
  | tee target/pharness-release-readiness-after.json
```

```sh
jq -e '[.warnings[].code] | index("release_not_approved") == null and index("missing_release") == null' \
  target/pharness-release-readiness-after.json
```

Expected signal:

- Release warning codes are absent.
- The response includes the approved `release`.

## Audit Evidence

```sh
cargo run -p pharness-cli -- audit-events \
  --resource-kind release \
  --resource-id "$RELEASE_ID" \
  | tee target/pharness-release-audit.json
```

```sh
jq -e '[.events[].kind] | index("release.proposed") != null and index("release.approved") != null' \
  target/pharness-release-audit.json
```

Expected signal:

- Audit events include `release.proposed`.
- Audit events include `release.approved`.

## Stale And Re-Propose After ChangeSet Revision

This section intentionally revises the ChangeSet. Run it only after the Release above is approved.

```sh
PIPELINE_INTENT_ID="$(jq -r '.release.pipeline_intent_id' target/pharness-release.json)"
test -n "$PIPELINE_INTENT_ID"
```

```sh
cargo run -p pharness-cli -- change-sets revise \
  --change-set-id "$CHANGE_SET_ID" \
  --actor lucas \
  --reason "release stale smoke source changed" \
  --material-change true \
  --change-set-json '{"changes":[{"path":"pharness-envelope-write-smoke.txt","operation":"create","summary":"Create smoke-test marker file"},{"path":"release/metadata.yaml","operation":"update","summary":"Force Release invalidation smoke"}],"rollback":"rm -f pharness-envelope-write-smoke.txt"}' \
  | tee target/pharness-release-change-set-revised.json
```

```sh
jq --arg pipeline_intent_id "$PIPELINE_INTENT_ID" --arg deployment_intent_id "$DEPLOYMENT_INTENT_ID" --arg release_id "$RELEASE_ID" \
  -e '.invalidated_pipeline_intent.id == $pipeline_intent_id and .invalidated_pipeline_intent.status == "stale" and .invalidated_deployment_intent.id == $deployment_intent_id and .invalidated_deployment_intent.status == "stale" and .invalidated_release.id == $release_id and .invalidated_release.status == "stale"' \
  target/pharness-release-change-set-revised.json
```

Expected signal:

- The ChangeSet revision response includes `invalidated_pipeline_intent.status = "stale"`.
- The ChangeSet revision response includes `invalidated_deployment_intent.status = "stale"`.
- The ChangeSet revision response includes `invalidated_release.status = "stale"`.

Re-approve the revised ChangeSet, re-propose and approve upstream intents, then verify readiness exposes the stale Release:

```sh
cargo run -p pharness-cli -- change-sets transition \
  --change-set-id "$CHANGE_SET_ID" \
  --target-status proposed \
  --actor lucas \
  --reason "release stale smoke revised ChangeSet ready" \
  | tee target/pharness-release-change-set-proposed-again.json
```

```sh
cargo run -p pharness-cli -- change-sets transition \
  --change-set-id "$CHANGE_SET_ID" \
  --target-status approved \
  --actor lucas \
  --reason "release stale smoke revised ChangeSet approved" \
  | tee target/pharness-release-change-set-approved-again.json
```

```sh
cargo run -p pharness-cli -- pipeline-intents create-from-change-set \
  --change-set-id "$CHANGE_SET_ID" \
  --actor lucas \
  --reason "release stale smoke pipeline reproposed" \
  | tee target/pharness-release-pipeline-intent-reproposed.json
```

```sh
cargo run -p pharness-cli -- pipeline-intents transition \
  --pipeline-intent-id "$PIPELINE_INTENT_ID" \
  --target-status approved \
  --actor lucas \
  --reason "release stale smoke pipeline approved again" \
  | tee target/pharness-release-pipeline-intent-approved-again.json
```

```sh
cargo run -p pharness-cli -- deployment-intents create-from-pipeline-intent \
  --pipeline-intent-id "$PIPELINE_INTENT_ID" \
  --target-environment dev \
  --target-namespace apps-dev \
  --argo-application checkout-api \
  --actor lucas \
  --reason "release stale smoke deployment reproposed" \
  | tee target/pharness-release-deployment-intent-reproposed.json
```

```sh
jq --arg deployment_intent_id "$DEPLOYMENT_INTENT_ID" \
  -e '.created == false and .deployment_intent.id == $deployment_intent_id and .deployment_intent.status == "proposed"' \
  target/pharness-release-deployment-intent-reproposed.json
```

```sh
cargo run -p pharness-cli -- deployment-intents transition \
  --deployment-intent-id "$DEPLOYMENT_INTENT_ID" \
  --target-status approved \
  --actor lucas \
  --reason "release stale smoke deployment approved again" \
  | tee target/pharness-release-deployment-intent-approved-again.json
```

```sh
cargo run -p pharness-cli -- change-sets readiness \
  --change-set-id "$CHANGE_SET_ID" \
  | tee target/pharness-release-readiness-stale.json
```

```sh
jq -e '[.warnings[].code] | index("stale_release") != null' \
  target/pharness-release-readiness-stale.json
```

Expected signal:

- The DeploymentIntent id is unchanged.
- Readiness includes warning code `stale_release`.

Re-propose and approve the Release:

```sh
cargo run -p pharness-cli -- releases create-from-deployment-intent \
  --deployment-intent-id "$DEPLOYMENT_INTENT_ID" \
  --version v0.1.1-smoke \
  --commit-sha def5678 \
  --image-digest sha256:feedface \
  --rollback-ref "$RELEASE_ID" \
  --actor lucas \
  --reason "release stale smoke reproposed" \
  | tee target/pharness-release-reproposed.json
```

```sh
jq --arg release_id "$RELEASE_ID" \
  -e '.created == false and .release.id == $release_id and .release.status == "proposed" and .release.version == "v0.1.1-smoke"' \
  target/pharness-release-reproposed.json
```

```sh
cargo run -p pharness-cli -- releases transition \
  --release-id "$RELEASE_ID" \
  --target-status approved \
  --actor lucas \
  --reason "release stale smoke approved again" \
  | tee target/pharness-release-approved-again.json
```

```sh
jq -e '.release.status == "approved"' target/pharness-release-approved-again.json
```

Expected signal:

- Re-propose returns `created = false`.
- The Release id is unchanged.
- The Release returns to `approved`.

Verify the expanded audit trail:

```sh
cargo run -p pharness-cli -- audit-events \
  --resource-kind release \
  --resource-id "$RELEASE_ID" \
  | tee target/pharness-release-audit-after-stale.json
```

```sh
jq -e '[.events[].kind] | index("release.stale") != null and index("release.reproposed") != null' \
  target/pharness-release-audit-after-stale.json
```

Expected signal:

- Audit events include `release.stale`.
- Audit events include `release.reproposed`.
