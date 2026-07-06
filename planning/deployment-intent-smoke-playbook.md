# Decisions

- Add this as the update smoke for the durable DeploymentIntent slice.
- Reuse the current PipelineIntent produced by `planning/pipeline-intent-smoke-playbook.md`.
- Approve the current PipelineIntent before creating a DeploymentIntent because deployment intent review starts only after build/test/package intent approval.
- Keep the smoke non-mutating. It creates and approves control-plane intent records only; it does not sync Argo CD.
- Treat a missing, stale, or non-approved DeploymentIntent as a ChangeSet readiness warning in V1.
- A material ChangeSet revision should stale both the current PipelineIntent and the derived DeploymentIntent.
- Re-running create-from-PipelineIntent after the current PipelineIntent is approved should re-propose the same stale DeploymentIntent row.

# Backlog

- Extend this playbook when DeploymentIntent execution becomes a separate approved Argo capability.
- Add production-impacting DeploymentIntent smoke coverage once production policy gates exist.

# DeploymentIntent Smoke Playbook

Run every command from the repository root. The API should already be running with `PHARNESS_API_URL=http://127.0.0.1:4777`.

## Common Environment

```sh
export PHARNESS_API_URL=http://127.0.0.1:4777
export CARGO_TARGET_DIR=target
mkdir -p target
```

## Load The Current PipelineIntent

```sh
PIPELINE_INTENT_ID="$(jq -r '.pipeline_intent.id // .id // empty' target/pharness-pipeline-intent-reproposed.json 2>/dev/null || true)"
if [ -z "$PIPELINE_INTENT_ID" ]; then PIPELINE_INTENT_ID="$(jq -r '.pipeline_intent.id // .id // empty' target/pharness-pipeline-intent-approved.json)"; fi
test -n "$PIPELINE_INTENT_ID"
```

Expected signal:

- `PIPELINE_INTENT_ID` is non-empty.
- If this check fails, run `planning/pipeline-intent-smoke-playbook.md` through PipelineIntent creation first.

## Approve The Current PipelineIntent

```sh
cargo run -p pharness-cli -- pipeline-intents transition \
  --pipeline-intent-id "$PIPELINE_INTENT_ID" \
  --target-status approved \
  --actor lucas \
  --reason "deployment intent smoke requires approved pipeline intent" \
  | tee target/pharness-deployment-pipeline-intent-approved.json
```

```sh
jq -e '.pipeline_intent.status == "approved"' target/pharness-deployment-pipeline-intent-approved.json
```

Expected signal:

- The current PipelineIntent status is `approved`.

## Create Or Fetch The DeploymentIntent

```sh
cargo run -p pharness-cli -- deployment-intents create-from-pipeline-intent \
  --pipeline-intent-id "$PIPELINE_INTENT_ID" \
  --target-environment dev \
  --target-namespace apps-dev \
  --argo-application checkout-api \
  --actor lucas \
  --reason "deployment intent smoke" \
  | tee target/pharness-deployment-intent.json
```

```sh
DEPLOYMENT_INTENT_ID="$(jq -r '.deployment_intent.id' target/pharness-deployment-intent.json)"
CHANGE_SET_ID="$(jq -r '.deployment_intent.change_set_id' target/pharness-deployment-intent.json)"
test -n "$DEPLOYMENT_INTENT_ID"
test -n "$CHANGE_SET_ID"
jq '{created, id: .deployment_intent.id, status: .deployment_intent.status, intent_kind: .deployment_intent.intent_kind, target_environment: .deployment_intent.target_environment, target_namespace: .deployment_intent.target_namespace, argo_application: .deployment_intent.argo_application, execution_enabled: .deployment_intent.intent_json.execution.enabled}' target/pharness-deployment-intent.json
```

Expected signal:

- `DEPLOYMENT_INTENT_ID` is non-empty.
- `CHANGE_SET_ID` is non-empty.
- `status` is `proposed`.
- `intent_kind` is `argo_sync_deploy`.
- `target_environment` is `dev`.
- `target_namespace` is `apps-dev`.
- `argo_application` is `checkout-api`.
- `execution_enabled` is `false`.

## Verify Idempotent Create

```sh
cargo run -p pharness-cli -- deployment-intents create-from-pipeline-intent \
  --pipeline-intent-id "$PIPELINE_INTENT_ID" \
  --title "ignored duplicate deployment title" \
  --actor lucas \
  --reason "deployment intent duplicate smoke" \
  | tee target/pharness-deployment-intent-existing.json
```

```sh
jq --arg deployment_intent_id "$DEPLOYMENT_INTENT_ID" \
  -e '.created == false and .deployment_intent.id == $deployment_intent_id' \
  target/pharness-deployment-intent-existing.json
```

Expected signal:

- The duplicate create returns `created = false`.
- The returned id matches `DEPLOYMENT_INTENT_ID`.

## List And Fetch

```sh
cargo run -p pharness-cli -- deployment-intents list \
  --pipeline-intent-id "$PIPELINE_INTENT_ID" \
  --change-set-id "$CHANGE_SET_ID" \
  --status proposed \
  --intent-kind argo_sync_deploy \
  --target-environment dev \
  --target-namespace apps-dev \
  --argo-application checkout-api \
  --limit 10 \
  | tee target/pharness-deployment-intents-list.json
```

```sh
jq --arg deployment_intent_id "$DEPLOYMENT_INTENT_ID" \
  -e '.count == 1 and .deployment_intents[0].id == $deployment_intent_id' \
  target/pharness-deployment-intents-list.json
```

```sh
cargo run -p pharness-cli -- deployment-intents get \
  --deployment-intent-id "$DEPLOYMENT_INTENT_ID" \
  | tee target/pharness-deployment-intent-detail.json
```

```sh
jq --arg deployment_intent_id "$DEPLOYMENT_INTENT_ID" \
  -e '.id == $deployment_intent_id and .status == "proposed"' \
  target/pharness-deployment-intent-detail.json
```

Expected signal:

- List returns exactly one proposed intent for the PipelineIntent.
- Get returns the same proposed intent.

## Readiness Before Deployment Approval

```sh
cargo run -p pharness-cli -- change-sets readiness \
  --change-set-id "$CHANGE_SET_ID" \
  | tee target/pharness-deployment-intent-readiness-before.json
```

```sh
jq -e '[.warnings[].code] | index("deployment_intent_not_approved") != null' \
  target/pharness-deployment-intent-readiness-before.json
```

Expected signal:

- Readiness includes warning code `deployment_intent_not_approved`.

## Approve The DeploymentIntent

```sh
cargo run -p pharness-cli -- deployment-intents transition \
  --deployment-intent-id "$DEPLOYMENT_INTENT_ID" \
  --target-status approved \
  --actor lucas \
  --reason "deployment intent smoke approved" \
  | tee target/pharness-deployment-intent-approved.json
```

```sh
jq -e '.deployment_intent.status == "approved"' target/pharness-deployment-intent-approved.json
```

Expected signal:

- The DeploymentIntent status is `approved`.

## Readiness After Deployment Approval

```sh
cargo run -p pharness-cli -- change-sets readiness \
  --change-set-id "$CHANGE_SET_ID" \
  | tee target/pharness-deployment-intent-readiness-after.json
```

```sh
jq -e '[.warnings[].code] | index("deployment_intent_not_approved") == null and index("missing_deployment_intent") == null' \
  target/pharness-deployment-intent-readiness-after.json
```

Expected signal:

- DeploymentIntent warning codes are absent.
- The response includes the approved `deployment_intent`.

## Audit Evidence

```sh
cargo run -p pharness-cli -- audit-events \
  --resource-kind deployment_intent \
  --resource-id "$DEPLOYMENT_INTENT_ID" \
  | tee target/pharness-deployment-intent-audit.json
```

```sh
jq -e '[.events[].kind] | index("deployment_intent.proposed") != null and index("deployment_intent.approved") != null' \
  target/pharness-deployment-intent-audit.json
```

Expected signal:

- Audit events include `deployment_intent.proposed`.
- Audit events include `deployment_intent.approved`.

## Stale And Re-Propose After ChangeSet Revision

This section intentionally revises the ChangeSet. Run it only after the DeploymentIntent above is approved.

```sh
cargo run -p pharness-cli -- change-sets revise \
  --change-set-id "$CHANGE_SET_ID" \
  --actor lucas \
  --reason "deployment intent stale smoke source changed" \
  --material-change true \
  --change-set-json '{"changes":[{"path":"pharness-envelope-write-smoke.txt","operation":"create","summary":"Create smoke-test marker file"},{"path":"argocd/application.yaml","operation":"update","summary":"Force DeploymentIntent invalidation smoke"}],"rollback":"rm -f pharness-envelope-write-smoke.txt"}' \
  | tee target/pharness-deployment-intent-change-set-revised.json
```

```sh
jq --arg pipeline_intent_id "$PIPELINE_INTENT_ID" --arg deployment_intent_id "$DEPLOYMENT_INTENT_ID" \
  -e '.invalidated_pipeline_intent.id == $pipeline_intent_id and .invalidated_pipeline_intent.status == "stale" and .invalidated_deployment_intent.id == $deployment_intent_id and .invalidated_deployment_intent.status == "stale"' \
  target/pharness-deployment-intent-change-set-revised.json
```

Expected signal:

- The ChangeSet revision response includes `invalidated_pipeline_intent.status = "stale"`.
- The ChangeSet revision response includes `invalidated_deployment_intent.status = "stale"`.

Re-approve the revised ChangeSet, re-propose and approve the PipelineIntent, then verify readiness exposes the stale DeploymentIntent:

```sh
cargo run -p pharness-cli -- change-sets transition \
  --change-set-id "$CHANGE_SET_ID" \
  --target-status proposed \
  --actor lucas \
  --reason "deployment intent stale smoke revised ChangeSet ready" \
  | tee target/pharness-deployment-intent-change-set-proposed-again.json
```

```sh
cargo run -p pharness-cli -- change-sets transition \
  --change-set-id "$CHANGE_SET_ID" \
  --target-status approved \
  --actor lucas \
  --reason "deployment intent stale smoke revised ChangeSet approved" \
  | tee target/pharness-deployment-intent-change-set-approved-again.json
```

```sh
cargo run -p pharness-cli -- pipeline-intents create-from-change-set \
  --change-set-id "$CHANGE_SET_ID" \
  --actor lucas \
  --reason "deployment intent stale smoke pipeline reproposed" \
  | tee target/pharness-deployment-pipeline-intent-reproposed.json
```

```sh
jq --arg pipeline_intent_id "$PIPELINE_INTENT_ID" \
  -e '.created == false and .pipeline_intent.id == $pipeline_intent_id and .pipeline_intent.status == "proposed"' \
  target/pharness-deployment-pipeline-intent-reproposed.json
```

```sh
cargo run -p pharness-cli -- pipeline-intents transition \
  --pipeline-intent-id "$PIPELINE_INTENT_ID" \
  --target-status approved \
  --actor lucas \
  --reason "deployment intent stale smoke pipeline approved again" \
  | tee target/pharness-deployment-pipeline-intent-approved-again.json
```

```sh
cargo run -p pharness-cli -- change-sets readiness \
  --change-set-id "$CHANGE_SET_ID" \
  | tee target/pharness-deployment-intent-readiness-stale.json
```

```sh
jq -e '[.warnings[].code] | index("stale_deployment_intent") != null' \
  target/pharness-deployment-intent-readiness-stale.json
```

Expected signal:

- Re-propose returns `created = false`.
- The PipelineIntent id is unchanged.
- Readiness includes warning code `stale_deployment_intent`.

Re-propose and approve the DeploymentIntent:

```sh
cargo run -p pharness-cli -- deployment-intents create-from-pipeline-intent \
  --pipeline-intent-id "$PIPELINE_INTENT_ID" \
  --target-environment dev \
  --target-namespace apps-dev \
  --argo-application checkout-api \
  --actor lucas \
  --reason "deployment intent stale smoke reproposed" \
  | tee target/pharness-deployment-intent-reproposed.json
```

```sh
jq --arg deployment_intent_id "$DEPLOYMENT_INTENT_ID" \
  -e '.created == false and .deployment_intent.id == $deployment_intent_id and .deployment_intent.status == "proposed"' \
  target/pharness-deployment-intent-reproposed.json
```

```sh
cargo run -p pharness-cli -- deployment-intents transition \
  --deployment-intent-id "$DEPLOYMENT_INTENT_ID" \
  --target-status approved \
  --actor lucas \
  --reason "deployment intent stale smoke approved again" \
  | tee target/pharness-deployment-intent-approved-again.json
```

```sh
jq -e '.deployment_intent.status == "approved"' target/pharness-deployment-intent-approved-again.json
```

Expected signal:

- Re-propose returns `created = false`.
- The DeploymentIntent id is unchanged.
- The DeploymentIntent returns to `approved`.

Verify the expanded audit trail:

```sh
cargo run -p pharness-cli -- audit-events \
  --resource-kind deployment_intent \
  --resource-id "$DEPLOYMENT_INTENT_ID" \
  | tee target/pharness-deployment-intent-audit-after-stale.json
```

```sh
jq -e '[.events[].kind] | index("deployment_intent.stale") != null and index("deployment_intent.reproposed") != null' \
  target/pharness-deployment-intent-audit-after-stale.json
```

Expected signal:

- Audit events include `deployment_intent.stale`.
- Audit events include `deployment_intent.reproposed`.
