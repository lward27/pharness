# WorkItem and Workspace Smoke Playbook

> Historical manual-path smoke. Prefer
> [`kubernetes-coding-alpha-smoke-playbook.md`](kubernetes-coding-alpha-smoke-playbook.md)
> and `work-items reconcile` for the current controller-driven flow. The
> commands below remain useful for isolating one lifecycle transition during
> diagnostics.

This validates the autonomous-SDLC alpha boundary without invoking a model,
cloning a repository, accessing a secret, or changing external Git/cluster
state. It uses a disposable local SQLite database under `target/`.

## Terminal 1: Start the API

From the repository root, run exactly:

```sh
rm -f target/pharness-work-item-smoke.db
CARGO_HOME="$PWD/target/cargo-home" \
CARGO_TARGET_DIR="$PWD/target" \
PHARNESS_BIND=127.0.0.1:4790 \
PHARNESS_DB_PATH="$PWD/target/pharness-work-item-smoke.db" \
FIREWORKS_API_KEY='' \
cargo run -p pharness-api
```

Wait until the API reports that it is listening. Keep this terminal running.

## Terminal 2: Submit and Plan a WorkItem

From the same repository root, run exactly:

```sh
export PHARNESS_API_URL=http://127.0.0.1:4790

cargo run -p pharness-cli -- work-items create \
  --title "Finance health endpoint" \
  --intent "Add a tested read-only health endpoint." \
  --acceptance-criterion "Endpoint returns a stable response." \
  --source-repo team/finance-api \
  --source-ref main \
  --gitops-repo team/finance-gitops \
  --gitops-ref main \
  --target-environment dev \
  --target-namespace apps-dev \
  --argo-application finance-api \
  --max-attempts 2 \
  --max-elapsed-seconds 900 \
  --actor smoke | tee target/work-item-create.json

WORK_ITEM_ID="$(jq -r '.id' target/work-item-create.json)"
echo "$WORK_ITEM_ID"

cargo run -p pharness-cli -- work-items transition \
  --work-item-id "$WORK_ITEM_ID" \
  --target-status planning \
  --actor smoke \
  --reason "start alpha planning" | jq

cargo run -p pharness-cli -- work-items create-work-plan \
  --work-item-id "$WORK_ITEM_ID" | tee target/work-item-plan.json | jq

cargo run -p pharness-cli -- workspaces list \
  --work-item-id "$WORK_ITEM_ID" | jq

cargo run -p pharness-cli -- work-items events \
  --work-item-id "$WORK_ITEM_ID" | jq
```

Expected result: the WorkItem is `planning`; the new WorkPlan has a
`work_item_id`, no remediation or incident lineage, and an execution block
with `enabled: false`; one Workspace has `status: declared` and
`retention_status: ephemeral`.

## Verify the Safety Boundary

This call intentionally fails because a WorkItem ChangeSet requires a real
workspace Git diff, which is the next implementation slice:

```sh
WORK_PLAN_ID="$(jq -r '.work_plan.id' target/work-item-plan.json)"

curl -sS -o target/work-item-change-set-error.json -w '%{http_code}\n' \
  -X POST "${PHARNESS_API_URL}/api/change-sets" \
  -H 'content-type: application/json' \
  -d "{\"work_plan_id\":\"${WORK_PLAN_ID}\",\"change_set_json\":{\"files\":[]}}"

jq . target/work-item-change-set-error.json
```

Expected result: HTTP `409` and an error explaining that captured workspace
Git diff provenance is required.

## Cancel and Inspect

Run exactly:

```sh
cargo run -p pharness-cli -- work-items cancel \
  --work-item-id "$WORK_ITEM_ID" \
  --actor smoke \
  --reason "smoke complete" | jq

cargo run -p pharness-cli -- work-items get \
  --work-item-id "$WORK_ITEM_ID" | jq

cargo run -p pharness-cli -- work-items events \
  --work-item-id "$WORK_ITEM_ID" | jq
```

Expected result: the WorkItem is `cancelled` and its audit event stream
contains `work_item.submitted`, `work_item.planning`, `work_item.work_plan_created`,
and `work_item.cancelled`.

## Cleanup

Press `Ctrl-C` in Terminal 1, then run:

```sh
rm -f target/pharness-work-item-smoke.db
```
