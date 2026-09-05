# ASTRA M09: Production approval and bounded rollback

Status: planned.
Authority: [approved program](ASTRA-00-PROGRAM.md).
Dependencies: M08.

## Objective and scope

Make production authority precise and recover only when the approved release and evidence allow it.

The program's locked authority, budget, source identity, compatibility, and evidence
rules apply. Work on current verified main in an isolated `codex/` worktree.
Do not mark this milestone accepted because a dependency appears healthy.

## Implementation

Baseline correction: yfinance is currently manual-sync while frontend auto-syncs.
Enable yfinance auto-sync in GitOps only with the approval-before-merge contract in
place, before accepting hosted production promotion. Do not treat a manual sync as
a substitute for that contract.

1. Prepare approval over exact image digest, GitOps diff, staging evidence, target, and preceding healthy deployment. Human approval must precede production GitOps merge because Argo auto-sync is enabled.

2. Revalidate the bound state immediately before merging. Changed source/config/digest/target or invalid staging evidence invalidates approval.

3. Observe actual Argo reconciliation and runtime identity; do not demand a fabricated manual-sync receipt.

4. Allow one GitOps rollback to the recorded healthy baseline for a confirmed release regression. Telemetry loss alone is not rollback proof.

5. Stop on incompatible migrations/configuration, conflicting deployments, unsafe baseline, or a failed rollback. A recovered service leaves the requested WorkItem failed.

## Interfaces and compatibility

Existing approval material/state hashes, GitOps writer/observer, release and remediation records; no broad autonomous cluster mutation.

Preserve immutable historical evidence, existing Finance data generation, and additive migration compatibility. Record any minimum compatible rollback version before enabling new writes.

## Tests and acceptance

- [ ] No production GitOps merge occurs without a valid, current human approval.
- [ ] Approval and rollback tests cover changed digests, stale evidence, target drift, concurrent deployment, duplicate requests, and lost acknowledgments.
- [ ] Same digest reaches staging and production; no rebuild.
- [ ] Recovery is demonstrated in staging without deliberate production failure injection.
- [ ] Production M11 approval is a genuine human action, never the implementation agent approving its own proof.

Focused authority and idempotence tests, staging rollback exercise, and production observation only after a concrete human approval in M11.

## Deployment and recovery

Record exact changed resources, source revisions, validation, before/after identities,
and external effects. Keep managed cluster changes in authoritative GitOps sources.
Production runtime acceptance still requires a concrete human approval.

One preauthorized safe rollback; otherwise stop with complete evidence and a concrete operator action. Never revert application data or shared resources automatically.

## Evidence and closeout

Write ASTRA-M09-PRODUCTION-PROMOTION-AND-ROLLBACK.md separating tested recovery from actual production events.
Use `planning/evidence/autonomous-sdlc/` for milestone execution evidence unless an
existing assessment location is explicitly named. Include date, revisions, commands
without secrets, observed results, failures, limitations, and commit/release identities.
A test result and a deployed result are separate claims.

Review coverage: F13.
Update the master ledger and this document only after its checks are evidenced.
Unmet criteria remain unchecked with a concrete reason and next action.

## Goal-mode execution prompt

Read ASTRA-00-PROGRAM.md and this milestone. Verify dependencies against current
evidence, inspect the affected implementation, execute the bounded changes above,
and run the specified meaningful checks. Preserve user work and all safety/identity
boundaries. Record results, commit the implementation and evidence, update the master
and finding ledger, then continue the next eligible milestone. If an external input is
missing, explain the exact blocker and continue independent work. Do not weaken a gate,
silently switch provider/budget, or claim unexecuted deployment or autonomous acceptance.

