# Kubernetes Coding Alpha Smoke

## Decisions

- This smoke uses the public disposable repository
  `https://github.com/lward27/yfinance_wrapper.git`. It is a source read plus
  an ephemeral in-Job write only: Pharness must not push, commit, merge,
  deploy, or delete any repository content.
- The remote repository allowlist is disabled by default and must be changed
  through the existing Pharness GitOps owner. Do not patch the deployed API or
  mutate a live Helm release directly for this smoke.
- Run one worker at a time on `ubuntu-lucas-engineering`, retain the durable
  evidence, then remove the allowlist entry unless a follow-on review keeps
  this alpha capability active.

## Preconditions

- The Pharness API/UI revision containing the typed remote-workspace changes
  is built and Argo CD has synced it.
- `pharness-api`, `pharness-worker`, and its runtime image have `git`; the
  current runtime image already includes it.
- The Pharness namespace can resolve and reach `github.com:443` from a worker
  Job. This is a bounded source-read requirement, not permission for the
  model to use network tools.
- A real Fireworks key is configured in the existing `pharness-fireworks`
  Secret. The operator and worker tokens remain separate.

## GitOps Change

In the Pharness Helm values owned by the cluster GitOps repository, set only:

```yaml
worker:
  workspaceAllowedRemoteRepos:
    - https://github.com/lward27/yfinance_wrapper.git
```

Commit that source change through the normal GitOps review path. Wait for the
Pharness Argo CD Application to report `Synced` and `Healthy`; do not begin
the smoke against an older API image.

## Run The Smoke

From the Pharness repository, type the following in one terminal to expose
the deployed API and obtain the existing operator token without printing it:

```bash
kubectl -n pharness port-forward svc/pharness-api 4777:4777
```

In a second terminal, type:

```bash
cd "$(git rev-parse --show-toplevel)"
export PHARNESS_API_URL="http://127.0.0.1:4777"
read -r -s "PHARNESS_API_TOKEN?Pharness operator token: "
export PHARNESS_API_TOKEN
echo
export CARGO_HOME="$PWD/target/cargo-home"
export CARGO_TARGET_DIR="$PWD/target"
```

First verify the worker is cluster-backed and the remote allowlist is visible
without exposing any credential:

```bash
cargo run -q -p pharness-cli -- config | jq '{worker, policy}'
```

Create a deliberately small WorkItem. It asks only for a new markdown note in
the worker clone:

```bash
cargo run -q -p pharness-cli -- work-items create \
  --title "Kubernetes coding alpha note" \
  --intent "Create pharness-kubernetes-coding-alpha.md at the repository root containing one short sentence stating that the change was produced in an isolated Pharness Kubernetes workspace. Do not modify existing source files." \
  --acceptance-criterion "pharness-kubernetes-coding-alpha.md exists" \
  --acceptance-criterion "No existing source file is modified" \
  --source-repo "https://github.com/lward27/yfinance_wrapper.git" \
  --source-ref main \
  --target-environment dev \
  --max-attempts 1 \
  --actor lucas | tee target/kubernetes-coding-alpha-work-item.json

export WORK_ITEM_ID="$(jq -r '.id' target/kubernetes-coding-alpha-work-item.json)"
```

Preview the controller's first action, then let it declare the source-only
WorkPlan and ephemeral workspace. The preview is non-mutating; `--apply` stops
at the WorkPlan review boundary:

```bash
cargo run -q -p pharness-cli -- work-items reconcile \
  --work-item-id "$WORK_ITEM_ID" \
  --actor lucas | jq '{action,applied,work_item:(.work_item | {id,status}),message}'

cargo run -q -p pharness-cli -- work-items reconcile \
  --work-item-id "$WORK_ITEM_ID" \
  --apply \
  --actor lucas \
  --reason "declare Kubernetes coding alpha review boundary" \
  | tee target/kubernetes-coding-alpha-plan.json

export WORK_PLAN_ID="$(jq -r '.work_plan.id' target/kubernetes-coding-alpha-plan.json)"

cargo run -q -p pharness-cli -- work-plans transition \
  --work-plan-id "$WORK_PLAN_ID" \
  --target-status proposed \
  --actor lucas \
  --reason "review source-only Kubernetes attempt"

cargo run -q -p pharness-cli -- work-plans transition \
  --work-plan-id "$WORK_PLAN_ID" \
  --target-status approved \
  --actor lucas \
  --reason "approve bounded development coding attempt"
```

After WorkPlan approval, the controller's next applied action starts the one
bounded coding attempt and retains its JSON result:

```bash
cargo run -q -p pharness-cli -- work-items reconcile \
  --work-item-id "$WORK_ITEM_ID" \
  --apply \
  --actor lucas \
  --reason "run one bounded Kubernetes coding attempt" \
  --max-turns 16 | tee target/kubernetes-coding-alpha-execution.json

export RUN_ID="$(jq -r '.run.id' target/kubernetes-coding-alpha-execution.json)"
```

Watch its durable events in a third terminal:

```bash
cd "$(git rev-parse --show-toplevel)"
export PHARNESS_API_URL="http://127.0.0.1:4777"
read -r -s "PHARNESS_API_TOKEN?Pharness operator token: "
export PHARNESS_API_TOKEN
echo
export CARGO_HOME="$PWD/target/cargo-home"
export CARGO_TARGET_DIR="$PWD/target"
cargo run -q -p pharness-cli -- runs events --run-id "$RUN_ID" --stream --timeout-ms 300000
```

If the normal write policy pauses the run, approve the concrete isolated
workspace write and wait for it to finish:

```bash
cargo run -q -p pharness-cli -- approvals approve \
  --run-id "$RUN_ID" \
  --decided-by lucas \
  --reason "approve one isolated Kubernetes workspace write" \
  --wait \
  --follow-events \
  --timeout-ms 300000 | tee target/kubernetes-coding-alpha-approved.json
```

Ask the controller to capture and inspect the resulting ChangeSet:

```bash
cargo run -q -p pharness-cli -- work-items reconcile \
  --work-item-id "$WORK_ITEM_ID" \
  --apply \
  --actor lucas \
  --reason "capture Kubernetes coding alpha evidence" | tee target/kubernetes-coding-alpha-change-set.json

export CHANGE_SET_ID="$(jq -r '.change_set.id' target/kubernetes-coding-alpha-change-set.json)"

cargo run -q -p pharness-cli -- workspaces list --work-item-id "$WORK_ITEM_ID" | jq '.workspaces[] | {id,status,source_repo,source_ref,resolved_commit,branch,run_id}'
cargo run -q -p pharness-cli -- artifacts list --run-id "$RUN_ID" | jq '.artifacts[] | {id,kind,label}'
cargo run -q -p pharness-cli -- change-sets get --change-set-id "$CHANGE_SET_ID" | jq '.change_set | {id,status,work_item_id,run_id,change_set_json}'
cargo run -q -p pharness-cli -- audit-events --run-id "$RUN_ID" | jq '.events[] | select(.kind == "workspace.provisioned" or .kind == "workspace.evidence_recorded")'
```

Expected result: `workspace.provisioned` appears before the model action,
the Workspace reaches `captured`, its `resolved_commit` is a full Git object
ID, the run owns `workspace_git_diff` and `workspace_git_status` artifacts,
and the ChangeSet is `proposed`.

Review the ChangeSet, then ask the controller to prepare and preflight the
non-mutating Git delivery plan. This does not push, commit, create a remote
branch, or open a pull request:

```bash
cargo run -q -p pharness-cli -- change-sets transition \
  --change-set-id "$CHANGE_SET_ID" \
  --target-status approved \
  --actor lucas \
  --reason "approve the captured source diff for Git delivery planning"

cargo run -q -p pharness-cli -- work-items reconcile \
  --work-item-id "$WORK_ITEM_ID" \
  --apply \
  --actor lucas \
  --reason "prepare and preflight immutable Git delivery plan" \
  | tee target/kubernetes-coding-alpha-git-delivery-preflight-blocked.json

jq '{action, applied, message, preflight: (.git_delivery_preflight | {status,authorization_ready,dispatch_ready,artifact_id: .artifact.id,plan_id: .plan.id})}' \
  target/kubernetes-coding-alpha-git-delivery-preflight-blocked.json

cargo run -q -p pharness-cli -- change-sets authorize-git-delivery \
  --change-set-id "$CHANGE_SET_ID" \
  --created-by lucas \
  --reason "authorize this exact reviewed dev source delivery" \
  | tee target/kubernetes-coding-alpha-git-delivery-authorization.json

jq '{created, grant: (.grant | {id,subject,status,scope,policy}), plan_id: .plan.id}' \
  target/kubernetes-coding-alpha-git-delivery-authorization.json

cargo run -q -p pharness-cli -- work-items reconcile \
  --work-item-id "$WORK_ITEM_ID" \
  --apply \
  --actor lucas \
  --reason "record authorized Git delivery readiness before writer dispatch" \
  | tee target/kubernetes-coding-alpha-git-delivery-preflight.json

jq '{action, applied, preflight: (.git_delivery_preflight | {status,authorization_ready,dispatch_ready,plan_id: .plan.id,grant_id: (.permission_grant.id // null),checks: [.checks[] | {code,passed,summary}]})}' \
  target/kubernetes-coding-alpha-git-delivery-preflight.json
```

Expected result: one `git_delivery_plan` artifact with the source repository,
base commit, issued head branch, diff digest, and `authorization.state` of
`not_authorized`. The first controller preflight is `blocked` because no Git
writer grant exists. The subsequent `agent:git-writer` grant is scoped to that
plan; after it is created, the second controller preflight returns
`status: ready_for_writer` and `authorization_ready: true`, but
`dispatch_ready: false` and a failed `git_writer_executor_available` check.
The plan remains immutable and the grant is the authorization record. This is
still the correct stopping point: the remote repository has no branch, commit,
or pull request from this smoke.

## Disable Again

Remove the `workspaceAllowedRemoteRepos` entry through the same GitOps path
and wait for Argo CD to reconcile. Retain the `target/` evidence locally for
review; the Kubernetes Job's `emptyDir` is intentionally ephemeral.

## Backlog

- Add a private-repository read-only identity only after this public-source
  smoke has passed and an egress policy can scope it to the required Git host.
- Automate this smoke only after it can use a disposable repository created
  specifically for Pharness; no existing finance-app repository should become
  an unattended CI mutation target.
