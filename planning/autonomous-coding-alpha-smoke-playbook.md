# Autonomous Coding Alpha Smoke Playbook

> Historical local-path smoke. Prefer
> [`kubernetes-coding-alpha-smoke-playbook.md`](kubernetes-coding-alpha-smoke-playbook.md)
> and `work-items reconcile` for the active controller-driven Kubernetes path.

This exercises the local development-only coding loop against the disposable
`yfinance_wrapper` repository. Pharness creates an independent clone under its
workspace root. It never pushes, commits, merges, deploys, or changes the
original repository.

## Terminal 1: Start The API

From the Pharness repository, type the following exactly:

```bash
source "$HOME/.zshrc"
cd "$(git rev-parse --show-toplevel)"
export REPO="$(cd ../yfinance_wrapper && pwd)"
export PHARNESS_DB_PATH="$PWD/target/coding-alpha.db"
export PHARNESS_WORKSPACE_ROOT="$PWD/target/coding-alpha-workspaces"
export PHARNESS_WORKSPACE_ALLOWED_REPOS="$REPO"
export CARGO_HOME="$PWD/target/cargo-home"
export CARGO_TARGET_DIR="$PWD/target"
cargo run -p pharness-api
```

Leave that terminal running. The explicit repository allowlist is the safety
boundary for this alpha. An empty allowlist disables source execution.

## Terminal 2: Submit And Approve The Work

Open a second terminal in the Pharness repository and type:

```bash
cd "$(git rev-parse --show-toplevel)"
export CARGO_HOME="$PWD/target/cargo-home"
export CARGO_TARGET_DIR="$PWD/target"
export REPO="$(cd ../yfinance_wrapper && pwd)"
```

Create a small, disposable feature request:

```bash
cargo run -p pharness-cli -- work-items create \
  --title "Add coding alpha note" \
  --intent "Create a Markdown file named pharness-coding-alpha.md at the repository root containing a short note that this change was produced by the Pharness coding alpha. Do not modify existing source files." \
  --acceptance-criterion "pharness-coding-alpha.md exists" \
  --acceptance-criterion "No existing source file is modified" \
  --source-repo "$REPO" \
  --source-ref HEAD \
  --target-environment dev \
  --max-attempts 1 \
  --actor lucas | tee target/coding-alpha-work-item.json

export WORK_ITEM_ID="$(jq -r '.id' target/coding-alpha-work-item.json)"
echo "$WORK_ITEM_ID"
```

Move the request into planning, create its plan and workspace, then review and
approve the WorkPlan:

```bash
cargo run -p pharness-cli -- work-items transition \
  --work-item-id "$WORK_ITEM_ID" \
  --target-status planning \
  --actor lucas \
  --reason "coding alpha planning" >/dev/null

cargo run -p pharness-cli -- work-items create-work-plan \
  --work-item-id "$WORK_ITEM_ID" | tee target/coding-alpha-plan.json

export WORK_PLAN_ID="$(jq -r '.work_plan.id' target/coding-alpha-plan.json)"

cargo run -p pharness-cli -- work-plans transition \
  --work-plan-id "$WORK_PLAN_ID" \
  --target-status proposed \
  --actor lucas \
  --reason "reviewable local coding plan" >/dev/null

cargo run -p pharness-cli -- work-plans transition \
  --work-plan-id "$WORK_PLAN_ID" \
  --target-status approved \
  --actor lucas \
  --reason "development-only coding alpha approved" >/dev/null
```

Provision the isolated workspace and start the bounded model attempt:

```bash
cargo run -p pharness-cli -- work-items execute \
  --work-item-id "$WORK_ITEM_ID" \
  --actor lucas \
  --reason "run one development-only coding attempt" \
  --max-turns 16 | tee target/coding-alpha-execution.json

export RUN_ID="$(jq -r '.run.id' target/coding-alpha-execution.json)"
echo "$RUN_ID"
```

Watch the event stream in a third terminal, or wait for it to close:

```bash
cd "$(git rev-parse --show-toplevel)"
export CARGO_HOME="$PWD/target/cargo-home"
export CARGO_TARGET_DIR="$PWD/target"
cargo run -p pharness-cli -- runs events --run-id "$RUN_ID" --stream --timeout-ms 300000
```

The normal default policy may pause on the file write. When the run reports
`approval_required`, approve the exact pending action and wait for the resumed
attempt:

```bash
cargo run -p pharness-cli -- approvals approve \
  --run-id "$RUN_ID" \
  --decided-by lucas \
  --reason "approve the isolated coding alpha file write" \
  --wait \
  --follow-events \
  --timeout-ms 300000 | tee target/coding-alpha-approved-run.json
```

If the original attempt completed without an approval, inspect it instead:

```bash
cargo run -p pharness-cli -- runs get --run-id "$RUN_ID" --with-events | tee target/coding-alpha-run.json
```

Capture the real Git evidence as a proposed ChangeSet and inspect the durable
result:

```bash
cargo run -p pharness-cli -- work-items capture-change-set \
  --work-item-id "$WORK_ITEM_ID" \
  --actor lucas \
  --reason "capture local coding alpha evidence" | tee target/coding-alpha-change-set.json

export CHANGE_SET_ID="$(jq -r '.change_set.id' target/coding-alpha-change-set.json)"

cargo run -p pharness-cli -- work-items get --work-item-id "$WORK_ITEM_ID" | jq '{id,status,attempt_count,current_run_id}'
cargo run -p pharness-cli -- workspaces list --work-item-id "$WORK_ITEM_ID" | jq '.workspaces[] | {id,status,resolved_commit,branch,run_id}'
cargo run -p pharness-cli -- change-sets get --change-set-id "$CHANGE_SET_ID" | jq '.change_set | {id,status,work_item_id,work_plan_id,run_id,change_set_json}'
cargo run -p pharness-cli -- artifacts list --run-id "$RUN_ID" | jq '.artifacts[] | {id,kind,label}'
```

Expected result: the WorkItem returns to `awaiting_approval`, the workspace is
`captured`, and the ChangeSet is `proposed` with `workspace_git_diff` and
`workspace_git_status` artifacts. The source repository remains untouched;
only its clone under `target/coding-alpha-workspaces` contains the change.

## Cleanup

Stop the API with `Ctrl-C` in Terminal 1. The clone directory is disposable.
Only remove it after reviewing the diff artifacts:

```bash
rm -rf target/coding-alpha-workspaces target/coding-alpha.db
```
