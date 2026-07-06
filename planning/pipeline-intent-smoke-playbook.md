# Decisions

- Add this as the update smoke for the durable PipelineIntent slice.
- Reuse the approved WorkPlan and ChangeSet produced by `planning/trusted-envelope-smoke-playbook.md`.
- Keep the smoke non-mutating. It creates and approves control-plane intent records only; it does not create Tekton PipelineRuns.
- Treat a non-approved PipelineIntent as a ChangeSet readiness warning in V1.
- A material ChangeSet revision should mark the current PipelineIntent stale. Re-running create-from-ChangeSet should re-propose that same intent row with the current material hash.

# Backlog

- Add an empty-database seed path once fixture-backed SDLC resource creation exists.
- Extend this playbook when PipelineIntent execution becomes a separate approved capability.

# PipelineIntent Smoke Playbook

Run every command from the repository root. The API should already be running with `PHARNESS_API_URL=http://127.0.0.1:4777`.

## Common Environment

```sh
export PHARNESS_API_URL=http://127.0.0.1:4777
export CARGO_TARGET_DIR=target
mkdir -p target
```

## Load Prior Smoke IDs

```sh
WORK_PLAN_ID="$(jq -r '.work_plan.id // .id // empty' target/pharness-envelope-work-plan-approved.json)"
CHANGE_SET_ID="$(jq -r '.change_set.id // .id // empty' target/pharness-envelope-change-set-approved.json)"
test -n "$WORK_PLAN_ID"
test -n "$CHANGE_SET_ID"
```

Expected signal:

- `WORK_PLAN_ID` is non-empty.
- `CHANGE_SET_ID` is non-empty.
- If either check fails, run `planning/trusted-envelope-smoke-playbook.md` through the ChangeSet approval step first.

## Create Or Fetch The PipelineIntent

```sh
cargo run -p pharness-cli -- pipeline-intents create-from-change-set \
  --change-set-id "$CHANGE_SET_ID" \
  --actor lucas \
  --reason "pipeline intent smoke" \
  | tee target/pharness-pipeline-intent.json
```

```sh
PIPELINE_INTENT_ID="$(jq -r '.pipeline_intent.id' target/pharness-pipeline-intent.json)"
test -n "$PIPELINE_INTENT_ID"
jq '{created, id: .pipeline_intent.id, status: .pipeline_intent.status, intent_kind: .pipeline_intent.intent_kind, execution_enabled: .pipeline_intent.intent_json.execution.enabled}' target/pharness-pipeline-intent.json
```

Expected signal:

- `PIPELINE_INTENT_ID` is non-empty.
- `status` is `proposed`.
- `intent_kind` is `tekton_build_test_package`.
- `execution_enabled` is `false`.

## Verify Idempotent Create

```sh
cargo run -p pharness-cli -- pipeline-intents create-from-change-set \
  --change-set-id "$CHANGE_SET_ID" \
  --title "ignored duplicate title" \
  --actor lucas \
  --reason "pipeline intent duplicate smoke" \
  | tee target/pharness-pipeline-intent-existing.json
```

```sh
jq --arg pipeline_intent_id "$PIPELINE_INTENT_ID" \
  -e '.created == false and .pipeline_intent.id == $pipeline_intent_id' \
  target/pharness-pipeline-intent-existing.json
```

Expected signal:

- The duplicate create returns `created = false`.
- The returned id matches `PIPELINE_INTENT_ID`.

## List And Fetch

```sh
cargo run -p pharness-cli -- pipeline-intents list \
  --change-set-id "$CHANGE_SET_ID" \
  --work-plan-id "$WORK_PLAN_ID" \
  --status proposed \
  --intent-kind tekton_build_test_package \
  --limit 10 \
  | tee target/pharness-pipeline-intents-list.json
```

```sh
jq --arg pipeline_intent_id "$PIPELINE_INTENT_ID" \
  -e '.count == 1 and .pipeline_intents[0].id == $pipeline_intent_id' \
  target/pharness-pipeline-intents-list.json
```

```sh
cargo run -p pharness-cli -- pipeline-intents get \
  --pipeline-intent-id "$PIPELINE_INTENT_ID" \
  | tee target/pharness-pipeline-intent-detail.json
```

```sh
jq --arg pipeline_intent_id "$PIPELINE_INTENT_ID" \
  -e '.id == $pipeline_intent_id and .status == "proposed"' \
  target/pharness-pipeline-intent-detail.json
```

Expected signal:

- List returns exactly one proposed intent for the ChangeSet.
- Get returns the same proposed intent.

## Readiness Before Approval

```sh
cargo run -p pharness-cli -- change-sets readiness \
  --change-set-id "$CHANGE_SET_ID" \
  | tee target/pharness-pipeline-intent-readiness-before.json
```

```sh
jq -e '[.warnings[].code] | index("pipeline_intent_not_approved") != null' \
  target/pharness-pipeline-intent-readiness-before.json
```

Expected signal:

- Readiness includes warning code `pipeline_intent_not_approved`.

## Approve The PipelineIntent

```sh
cargo run -p pharness-cli -- pipeline-intents transition \
  --pipeline-intent-id "$PIPELINE_INTENT_ID" \
  --target-status approved \
  --actor lucas \
  --reason "pipeline intent smoke approved" \
  | tee target/pharness-pipeline-intent-approved.json
```

```sh
jq -e '.pipeline_intent.status == "approved"' target/pharness-pipeline-intent-approved.json
```

Expected signal:

- The PipelineIntent status is `approved`.

## Readiness After Approval

```sh
cargo run -p pharness-cli -- change-sets readiness \
  --change-set-id "$CHANGE_SET_ID" \
  | tee target/pharness-pipeline-intent-readiness-after.json
```

```sh
jq -e '[.warnings[].code] | index("pipeline_intent_not_approved") == null and index("missing_pipeline_intent") == null' \
  target/pharness-pipeline-intent-readiness-after.json
```

Expected signal:

- PipelineIntent warning codes are absent.
- Other readiness blockers may still exist if prior smoke gates or trusted envelopes were not satisfied.

## Stale And Re-Propose After ChangeSet Revision

```sh
cargo run -p pharness-cli -- change-sets revise \
  --change-set-id "$CHANGE_SET_ID" \
  --actor lucas \
  --reason "pipeline intent stale smoke source changed" \
  --material-change true \
  --change-set-json '{"changes":[{"path":"pharness-envelope-write-smoke.txt","operation":"create","summary":"Create smoke-test marker file"},{"path":"tekton/pipeline.yaml","operation":"update","summary":"Force PipelineIntent invalidation smoke"}],"rollback":"rm -f pharness-envelope-write-smoke.txt"}' \
  | tee target/pharness-pipeline-intent-change-set-revised.json
```

```sh
jq --arg pipeline_intent_id "$PIPELINE_INTENT_ID" \
  -e '.invalidated_pipeline_intent.id == $pipeline_intent_id and .invalidated_pipeline_intent.status == "stale"' \
  target/pharness-pipeline-intent-change-set-revised.json
```

```sh
cargo run -p pharness-cli -- change-sets readiness \
  --change-set-id "$CHANGE_SET_ID" \
  | tee target/pharness-pipeline-intent-readiness-stale.json
```

```sh
jq -e '[.warnings[].code] | index("stale_pipeline_intent") != null' \
  target/pharness-pipeline-intent-readiness-stale.json
```

Expected signal:

- The ChangeSet revision response includes `invalidated_pipeline_intent.status = "stale"`.
- Readiness includes warning code `stale_pipeline_intent`.

Re-approve the revised ChangeSet and re-propose the PipelineIntent:

```sh
cargo run -p pharness-cli -- change-sets transition \
  --change-set-id "$CHANGE_SET_ID" \
  --target-status proposed \
  --actor lucas \
  --reason "pipeline intent stale smoke revised ChangeSet ready" \
  | tee target/pharness-pipeline-intent-change-set-proposed-again.json
```

```sh
cargo run -p pharness-cli -- change-sets transition \
  --change-set-id "$CHANGE_SET_ID" \
  --target-status approved \
  --actor lucas \
  --reason "pipeline intent stale smoke revised ChangeSet approved" \
  | tee target/pharness-pipeline-intent-change-set-approved-again.json
```

```sh
cargo run -p pharness-cli -- pipeline-intents create-from-change-set \
  --change-set-id "$CHANGE_SET_ID" \
  --actor lucas \
  --reason "pipeline intent stale smoke reproposed" \
  | tee target/pharness-pipeline-intent-reproposed.json
```

```sh
CURRENT_CHANGE_SET_HASH="$(jq -r '.change_set.material_hash' target/pharness-pipeline-intent-change-set-approved-again.json)"
jq --arg pipeline_intent_id "$PIPELINE_INTENT_ID" --arg material_hash "$CURRENT_CHANGE_SET_HASH" \
  -e '.created == false and .pipeline_intent.id == $pipeline_intent_id and .pipeline_intent.status == "proposed" and .pipeline_intent.intent_json.source.material_hash == $material_hash' \
  target/pharness-pipeline-intent-reproposed.json
```

Expected signal:

- Re-propose returns `created = false`.
- The PipelineIntent id is unchanged.
- The PipelineIntent status is `proposed`.
- The default intent JSON now references the revised ChangeSet material hash.

## Audit Evidence

```sh
cargo run -p pharness-cli -- audit-events \
  --resource-kind pipeline_intent \
  --resource-id "$PIPELINE_INTENT_ID" \
  | tee target/pharness-pipeline-intent-audit.json
```

```sh
jq -e '[.events[].kind] | index("pipeline_intent.proposed") != null and index("pipeline_intent.approved") != null and index("pipeline_intent.stale") != null and index("pipeline_intent.reproposed") != null' \
  target/pharness-pipeline-intent-audit.json
```

Expected signal:

- Audit events include `pipeline_intent.proposed`.
- Audit events include `pipeline_intent.approved`.
- Audit events include `pipeline_intent.stale`.
- Audit events include `pipeline_intent.reproposed`.
