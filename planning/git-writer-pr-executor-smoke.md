# Git Writer PR Executor Smoke

## Decisions

- This is a development-only, externally mutating smoke. Use a disposable
  GitHub repository or a temporary branch in an approved finance experiment.
  Do not run it against production repositories or GitOps repositories.
- The GitHub token must be fine-grained, scoped to one repository, and allow
  contents read/write plus pull-request read/write. It is created out of band
  and is never printed, committed, or mounted into the API/model/Tekton jobs.
- The writer executes one immutable plan once. A Job failure after a remote
  push is a review boundary, not an automatic retry; inspect the issued branch
  and result/audit artifacts before making another plan.

## Preconditions

1. A WorkItem-backed ChangeSet has actual workspace Git diff evidence, is
   approved, and has an approved WorkPlan.
2. The repository is HTTPS GitHub form `https://github.com/OWNER/REPO.git`.
3. The Helm value change is reviewed through the existing GitOps owner:

```yaml
worker:
  gitWriter:
    enabled: true
    tokenSecretName: pharness-git-writer-token
    allowedRepos:
      - https://github.com/OWNER/REPO.git
    authorName: Pharness
    authorEmail: pharness@lucas.engineering
```

4. Create the writer-only secret without echoing its value:

```bash
kubectl -n pharness create secret generic pharness-git-writer-token \
  --from-literal=token="$(security find-generic-password -a pharness-git-writer -s github-token -w)"
```

Use a safer local secret manager command if this token is not stored in the
macOS keychain. Do not place the literal token in shell history.

5. After Argo sync, confirm the API sees the setting without asking it for a
secret value:

```bash
kubectl -n pharness exec deploy/pharness-api -- \
  printenv PHARNESS_GIT_WRITER_ENABLED PHARNESS_GIT_WRITER_ALLOWED_REPOS
```

Expected: `true` and the exact reviewed repository URL. Never run `printenv`
for the writer Job or Secret.

## Execute

Set the reviewed ChangeSet and WorkItem IDs. Satisfy the scoped Git delivery
gate first, then create the exact writer grant if one does not exist:

```bash
export PHARNESS_API_URL=http://127.0.0.1:4777
export CHANGE_SET_ID="replace-with-approved-change-set-id"
export WORK_ITEM_ID="replace-with-owning-work-item-id"

export GIT_GATE_ID="$(cargo run -q -p pharness-cli -- approval-gates list \
  --work-item-id "$WORK_ITEM_ID" \
  --gate-kind git_mutation \
  --status pending | jq -r '.approval_gates[0].id')"

cargo run -q -p pharness-cli -- approval-gates satisfy \
  --gate-id "$GIT_GATE_ID" \
  --decided-by lucas \
  --reason "approve the bounded disposable Git branch and pull-request delivery" | jq

cargo run -q -p pharness-cli -- change-sets authorize-git-delivery \
  --change-set-id "$CHANGE_SET_ID" \
  --created-by lucas \
  --reason "approve one reviewed disposable development source delivery" | jq

cargo run -q -p pharness-cli -- change-sets preflight-git-delivery \
  --change-set-id "$CHANGE_SET_ID" \
  --actor lucas \
  --reason "verify immutable plan, grant, and writer availability" | jq
```

Expected: `status: ready_for_writer`, `approval_gate_ready: true`,
`authorization_ready: true`, and `dispatch_ready: true`.

Dispatch exactly once:

```bash
cargo run -q -p pharness-cli -- change-sets execute-git-delivery \
  --change-set-id "$CHANGE_SET_ID" \
  --actor lucas \
  --reason "execute one approved disposable development pull request" \
  | tee target/git-writer-execute.json | jq
```

Record the returned `job_name`, then observe only its bounded status/log
metadata. Do not print its environment:

```bash
export GIT_WRITER_JOB="$(jq -r '.job_name' target/git-writer-execute.json)"
kubectl -n pharness get job "$GIT_WRITER_JOB" -o json | \
  jq '{name: .metadata.name, active: .status.active, succeeded: .status.succeeded, failed: .status.failed}'
kubectl -n pharness logs "job/$GIT_WRITER_JOB" --tail=100
```

Inspect durable evidence through Pharness, then inspect the actual PR in
GitHub. A completed result must contain the exact issued branch, 40-character
commit SHA, PR URL, and PR number:

```bash
curl -sS "$PHARNESS_API_URL/api/change-sets/$CHANGE_SET_ID/flow" | \
  jq '.git_delivery | {plan, latest_preflight, latest_execution, latest_result}'
```

## Cleanup

Disable `worker.gitWriter.enabled` or remove the repository from its exact
allowlist through GitOps. Delete the writer token Secret only after the
evidence review is complete. Retain the Pharness execution/result artifacts
and audit events; remove only the disposable branch/PR after review.

## Merge Observation

After manually merging the disposable PR, enable the independent observer
through GitOps. It needs a different fine-grained GitHub token with repository
metadata and pull-request **read** access only; do not reuse the writer token.

```yaml
worker:
  gitObserver:
    enabled: true
    tokenSecretName: pharness-git-observer-token
    allowedRepos:
      - https://github.com/OWNER/REPO.git
```

Create the separate Secret through your local secret manager, then dispatch
the observer from the port-forwarded API:

```bash
cargo run -q -p pharness-cli -- change-sets observe-git-delivery \
  --change-set-id "$CHANGE_SET_ID" \
  --actor lucas \
  --reason "record immutable merge provenance for the reviewed disposable PR" \
  | tee target/git-observer-execute.json | jq
```

Expected: the response dispatches a `pharness-git-observer` Job. Once it
finishes, the ChangeSet flow has `latest_observation` and, only for a merged
PR, `latest_merge` with a 40-character `merge_commit_sha`. An unmerged PR
records observation but intentionally cannot create a WorkItem PipelineIntent.

## Backlog

- Add an optional Cilium FQDN egress policy for GitHub before enabling this in
  a shared cluster; generic NetworkPolicy cannot safely express the required
  GitHub hostname scope.
- Add GitHub App installation-token support only after the fine-grained token
  smoke proves the stable branch/PR contract.
