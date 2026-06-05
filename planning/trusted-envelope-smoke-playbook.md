# Decisions

- Add this as the update smoke for trusted WorkPlan and ChangeSet envelopes.
- Run it after the main smoke playbook has produced at least one remediation plan, or after any live run that created a draft remediation plan. This slice intentionally does not add fixture-only API state.
- Use `change-sets create-trusted-envelope` for the actual no-approval write smoke because it is narrower than a WorkPlan-only envelope.
- Keep the negative mismatch smoke manual-safe: it should pause at approval instead of writing, then the pending approval is denied.

# Backlog

- Add a fixture-backed control-plane seed command when we want this smoke to run from an empty database without a live cluster or model-backed remediation plan.
- Add automatic stale-envelope checks after WorkPlan or ChangeSet material revisions.
- Require approved WorkPlan or ChangeSet status before trusted-envelope creation once status gates become policy, not just review metadata.

# Trusted Envelope Smoke Playbook

Run every command from the repository root. The API should already be running with `PHARNESS_API_URL=http://127.0.0.1:4777`, as in `planning/pharness-smoke-playbook.md`.

## Common Environment

```sh
export PHARNESS_API_URL=http://127.0.0.1:4777
export CARGO_TARGET_DIR=target
mkdir -p target
```

## Find A Remediation Plan

```sh
cargo run -p pharness-cli -- remediation-plans list \
  --limit 1 \
  --offset 0 | tee target/pharness-envelope-remediation-plans.json
```

```sh
PLAN_ID="$(jq -r '.remediation_plans[0].id // empty' target/pharness-envelope-remediation-plans.json)"
test -n "$PLAN_ID"
```

Expected signal:

- `PLAN_ID` is non-empty.
- If this fails, run the Tekton/incident/remediation section of `planning/pharness-smoke-playbook.md` first.

## Create Or Fetch A WorkPlan

```sh
cargo run -p pharness-cli -- work-plans create-from-remediation-plan \
  --remediation-plan-id "$PLAN_ID" | tee target/pharness-envelope-work-plan.json
```

```sh
WORK_PLAN_ID="$(jq -r '.work_plan.id' target/pharness-envelope-work-plan.json)"
test -n "$WORK_PLAN_ID"
cargo run -p pharness-cli -- work-plans get \
  --work-plan-id "$WORK_PLAN_ID" | tee target/pharness-envelope-work-plan-detail.json | jq '{id, status, revision, remediation_plan_id, risk_level}'
```

Expected signal:

- `WORK_PLAN_ID` is non-empty.
- The WorkPlan is returned with a lifecycle `status` and `revision`.

## Smoke The WorkPlan Envelope Factory

```sh
cargo run -p pharness-cli -- work-plans create-trusted-envelope \
  --work-plan-id "$WORK_PLAN_ID" \
  --created-by lucas \
  --reason "trusted envelope smoke WorkPlan review" \
  --namespace apps-dev \
  --repo git@example.test/team/pharness.git \
  --branch smoke/workplan-envelope | tee target/pharness-work-plan-envelope-grant.json
```

```sh
WORK_PLAN_GRANT_ID="$(jq -r '.grant.id' target/pharness-work-plan-envelope-grant.json)"
test -n "$WORK_PLAN_GRANT_ID"
jq '{id: .grant.id, status: .grant.status, scope: .grant.scope, policy: .grant.policy}' target/pharness-work-plan-envelope-grant.json
cargo run -p pharness-cli -- audit-events \
  --resource-kind work_plan \
  --resource-id "$WORK_PLAN_ID" | tee target/pharness-work-plan-envelope-audit.json | jq
```

Expected signal:

- The grant status is `active`.
- `grant.scope.work_plan_ids[0]` equals `WORK_PLAN_ID`.
- `grant.scope.change_set_ids` is absent or `null`.
- WorkPlan audit events include `work_plan.trusted_envelope_created`.

## Create And Approve A ChangeSet

```sh
cargo run -p pharness-cli -- change-sets create \
  --work-plan-id "$WORK_PLAN_ID" \
  --title "Trusted envelope smoke ChangeSet" \
  --summary "Create a bounded local smoke-test file" \
  --risk-level medium \
  --actor lucas \
  --reason "trusted envelope smoke source proposal" \
  --change-set-json '{"changes":[{"path":"pharness-envelope-write-smoke.txt","operation":"create","summary":"Create smoke-test marker file"}],"rollback":"rm -f pharness-envelope-write-smoke.txt"}' \
  | tee target/pharness-envelope-change-set.json
```

```sh
CHANGE_SET_ID="$(jq -r '.change_set.id' target/pharness-envelope-change-set.json)"
test -n "$CHANGE_SET_ID"
cargo run -p pharness-cli -- change-sets revise \
  --change-set-id "$CHANGE_SET_ID" \
  --actor lucas \
  --reason "trusted envelope smoke normalized payload" \
  --material-change true \
  --change-set-json '{"changes":[{"path":"pharness-envelope-write-smoke.txt","operation":"create","summary":"Create smoke-test marker file"}],"rollback":"rm -f pharness-envelope-write-smoke.txt"}' \
  | tee target/pharness-envelope-change-set-revised.json
cargo run -p pharness-cli -- change-sets transition \
  --change-set-id "$CHANGE_SET_ID" \
  --target-status proposed \
  --actor lucas \
  --reason "trusted envelope smoke ready for review" | tee target/pharness-envelope-change-set-proposed.json
cargo run -p pharness-cli -- change-sets transition \
  --change-set-id "$CHANGE_SET_ID" \
  --target-status approved \
  --actor lucas \
  --reason "trusted envelope smoke approved" | tee target/pharness-envelope-change-set-approved.json
```

Expected signal:

- `CHANGE_SET_ID` is non-empty.
- The final ChangeSet status is `approved`.

## Create A ChangeSet Trusted Envelope

```sh
cargo run -p pharness-cli -- change-sets create-trusted-envelope \
  --change-set-id "$CHANGE_SET_ID" \
  --created-by lucas \
  --reason "trusted envelope smoke ChangeSet review" \
  --namespace apps-dev \
  --repo git@example.test/team/pharness.git \
  --branch smoke/trusted-envelope | tee target/pharness-change-set-envelope-grant.json
```

```sh
GRANT_ID="$(jq -r '.grant.id' target/pharness-change-set-envelope-grant.json)"
test -n "$GRANT_ID"
jq --arg work_plan_id "$WORK_PLAN_ID" --arg change_set_id "$CHANGE_SET_ID" \
  -e '.grant.status == "active" and .grant.scope.work_plan_ids[0] == $work_plan_id and .grant.scope.change_set_ids[0] == $change_set_id and .grant.scope.actions == ["write_file","patch_file"] and .grant.scope.max_risk == "medium"' \
  target/pharness-change-set-envelope-grant.json
cargo run -p pharness-cli -- audit-events \
  --resource-kind change_set \
  --resource-id "$CHANGE_SET_ID" | tee target/pharness-change-set-envelope-audit.json | jq
```

Expected signal:

- The grant status is `active`.
- The grant scope includes both `WORK_PLAN_ID` and `CHANGE_SET_ID`.
- ChangeSet audit events include `change_set.trusted_envelope_created`.

## Run A Matching Scoped Write

```sh
rm -f pharness-envelope-write-smoke.txt
cargo run -p pharness-cli -- run \
  --follow-events \
  --task "Create a file named pharness-envelope-write-smoke.txt in the workspace containing exactly: pharness trusted envelope smoke test. Then finish with a short summary." \
  --cwd "$PWD" \
  --namespace apps-dev \
  --repo git@example.test/team/pharness.git \
  --branch smoke/trusted-envelope \
  --work-plan-id "$WORK_PLAN_ID" \
  --change-set-id "$CHANGE_SET_ID" \
  --timeout-ms 180000 | tee target/pharness-envelope-write.json
```

```sh
jq '{wait_status, run_status: .run.status, scope: .run.scope, result: .run.result}' target/pharness-envelope-write.json
jq '[.events[] | select(.type == "approval.required")] | length' target/pharness-envelope-write.json
jq --arg grant_id "$GRANT_ID" '[.events[] | select(.type == "policy.evaluated" and .payload.decision.grant_id == $grant_id)] | length' target/pharness-envelope-write.json
cat pharness-envelope-write-smoke.txt
RUN_ID="$(jq -r '.run.id' target/pharness-envelope-write.json)"
cargo run -p pharness-cli -- audit-events \
  --run-id "$RUN_ID" | tee target/pharness-envelope-write-audit.json | jq
```

Expected signal:

- Final run status is `completed`.
- There are zero `approval.required` events.
- At least one `policy.evaluated` event has `decision.grant_id == GRANT_ID`.
- The file content is exactly `pharness trusted envelope smoke test`.
- Run audit events include `permission_grant.used`.

## Run A Mismatched ChangeSet Scope

```sh
rm -f pharness-envelope-mismatch-smoke.txt
cargo run -p pharness-cli -- run \
  --follow-events \
  --task "Create a file named pharness-envelope-mismatch-smoke.txt in the workspace containing exactly: pharness mismatched envelope smoke test. Then finish with a short summary." \
  --cwd "$PWD" \
  --namespace apps-dev \
  --repo git@example.test/team/pharness.git \
  --branch smoke/trusted-envelope \
  --work-plan-id "$WORK_PLAN_ID" \
  --change-set-id cset_wrong \
  --timeout-ms 180000 | tee target/pharness-envelope-mismatch.json
```

```sh
jq '{wait_status, run_status: .run.status, scope: .run.scope, result: .run.result}' target/pharness-envelope-mismatch.json
jq '[.events[] | select(.type == "approval.required")] | length' target/pharness-envelope-mismatch.json
MISMATCH_APPROVAL_ID="$(jq -r '.run.result.approval_id // empty' target/pharness-envelope-mismatch.json)"
if [ -n "$MISMATCH_APPROVAL_ID" ]; then
  cargo run -p pharness-cli -- approvals deny \
    --approval-id "$MISMATCH_APPROVAL_ID" \
    --decided-by lucas \
    --reason "trusted envelope mismatch smoke cleanup" \
    --wait \
    --timeout-ms 180000 | tee target/pharness-envelope-mismatch-denied.json
fi
test ! -f pharness-envelope-mismatch-smoke.txt
```

Expected signal:

- The mismatched run stops at `approval_required`.
- There is at least one `approval.required` event.
- The mismatch file is not created after denying the approval.

## Revoke Grants And Inspect Audit

```sh
cargo run -p pharness-cli -- permission-grants revoke \
  --grant-id "$GRANT_ID" \
  --revoked-by lucas \
  --reason "trusted envelope smoke complete" | tee target/pharness-change-set-envelope-revoked.json
cargo run -p pharness-cli -- permission-grants revoke \
  --grant-id "$WORK_PLAN_GRANT_ID" \
  --revoked-by lucas \
  --reason "trusted envelope smoke complete" | tee target/pharness-work-plan-envelope-revoked.json
cargo run -p pharness-cli -- audit-events \
  --resource-kind permission_grant \
  --resource-id "$GRANT_ID" | tee target/pharness-change-set-envelope-grant-audit.json | jq
```

Expected signal:

- Both grants return `status = revoked`.
- PermissionGrant audit includes `permission_grant.created`, `permission_grant.used`, and `permission_grant.revoked` for the ChangeSet grant.

## Cleanup

```sh
rm -f pharness-envelope-write-smoke.txt pharness-envelope-mismatch-smoke.txt
```
