# ASTRA M04: Coding reliability qualification

Status: active.
Authority: [approved program](ASTRA-00-PROGRAM.md).
Dependencies: M03. An external qualification blocker does not stop independent M02/M05 preparation.

## Objective and scope

Qualify and pin the existing gateway/Coding Reliability V2 path before autonomous source delivery.

The program's locked authority, budget, source identity, compatibility, and evidence
rules apply. Work on current verified main in an isolated `codex/` worktree.
Do not mark this milestone accepted because a dependency appears healthy.

## Implementation

1. Reuse current gateway, deterministic Test, checkpoint/context machinery, and one bounded correction. Do not expand the native Codex-host backend.

2. Recheck the historical provider-account failure against current behavior using bounded, authorized qualification. Never print credentials or silently change provider/model/budget.

3. Freeze the existing 24-task Rust/Python/Node suite and existing protocol/stage suites. Pin prompts, models, policy/profile revisions, environment images, task revision, and all execution limits.

4. Use the registered candidate order and existing qualification procedure. Enable the selected V2 path only after all existing gates pass.

## Interfaces and compatibility

Pin existing execution/profile selections and feature configuration; preserve existing tool contracts and limits.

Preserve immutable historical evidence, existing Finance data generation, and additive migration compatibility. Record any minimum compatible rollback version before enabling new writes.

## Tests and acceptance

- [ ] Two independent qualifying runs each achieve >=21/24 first-pass and >=23/24 after one correction.
- [ ] Each language stack achieves >=6/8 first-pass and >=7/8 after correction in both runs.
- [ ] Existing protocol and stage-specific gates pass, including the 10-case protocol suite repeated three times (30/30). No hidden-test false passes or policy violations.
- [ ] Usage, wall time, intervention, failure class, and exact configuration are recorded; no budget increases or softened gates are hidden.
- [ ] Provider/credential failures remain explicit blockers. A replay pass does not substitute for live gateway qualification.

Use the existing qualification milestone and scripts as procedure references, subordinate to these scope and acceptance decisions. Deterministic suites first; live model runs stay explicitly gated and out of ordinary CI.

## Deployment and recovery

Record exact changed resources, source revisions, validation, before/after identities,
and external effects. Keep managed cluster changes in authoritative GitOps sources.
Production runtime acceptance still requires a concrete human approval.

Keep or restore the previously accepted disabled/legacy configuration if qualification fails. Do not replace a known profile with an unqualified fallback.

## Evidence and closeout

Write ASTRA-M04-CODING-RELIABILITY-QUALIFICATION.md and immutable raw evaluation results with configuration hashes.
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

