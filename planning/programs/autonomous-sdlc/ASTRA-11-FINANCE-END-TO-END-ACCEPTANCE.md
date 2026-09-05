# ASTRA M11: Finance end-to-end acceptance

Status: planned.
Authority: [approved program](ASTRA-00-PROGRAM.md).
Dependencies: M09 and M10, with all earlier gates satisfied.

## Objective and scope

Prove two meaningful maintenance changes through PHarness itself from request to verified production.

The program's locked authority, budget, source identity, compatibility, and evidence
rules apply. Work on current verified main in an isolated `codex/` worktree.
Do not mark this milestone accepted because a dependency appears healthy.

## Implementation

1. First submit a yfinance WorkItem: prevent synchronous upstream operations from blocking health-check responsiveness while preserving routes, validation order, response shapes, and error behavior; require deterministic concurrency regression coverage.

2. Then submit a frontend WorkItem pinned to accepted backend context: load non-secret /runtime-config.json before initialization, validate configuration, visibly fail when production configuration is missing, and test configuration failures plus existing market behavior.

3. Use the existing repository acceptance commands. Keep one mutable application repo per WorkItem; use the authorized GitOps delivery boundary.

4. Do not manually provide generated application patches, merge routine source steps, or tick the workflow. Already-done/noop work does not count.

5. Obtain genuine human approval on each concrete production promotion. Exercise restart, duplicate notification, stale source, failed build, missing telemetry, and rollback in tests or staging.

## Interfaces and compatibility

Exercise the accepted APIs and runtime, not a special proof-only orchestrator. Frontend config is a non-secret deployment artifact; staging uses staging yfinance and isolated browser verification.

Preserve immutable historical evidence, existing Finance data generation, and additive migration compatibility. Record any minimum compatible rollback version before enabling new writes.

## Tests and acceptance

- [ ] Both requests produce meaningful changes and complete discover through observe with only the agreed production approval intervention.
- [ ] Evidence links acceptance -> tested source -> merge -> Tekton -> digest -> staging GitOps/verification -> human approval -> production GitOps/imageID/runtime verification.
- [ ] All failures and interventions are recorded. A manually rescued run is reported honestly and cannot be called a clean autonomous pass.
- [ ] No deliberately induced production failure is used for acceptance.
- [ ] Both actual application releases satisfy their acceptance and verification windows.

Real user-style requests, existing unit/lint/build commands, actual external effects, full runtime evidence, and bounded staging failure scenarios.

## Deployment and recovery

Record exact changed resources, source revisions, validation, before/after identities,
and external effects. Keep managed cluster changes in authoritative GitOps sources.
Production runtime acceptance still requires a concrete human approval.

Use only M09's approved safe rollback semantics. If either request fails, retain evidence and repair the platform before a fresh bounded acceptance attempt.

## Evidence and closeout

Write ASTRA-M11-FINANCE-END-TO-END-ACCEPTANCE.md with per-WorkItem intervention and identity ledgers.
Use `planning/evidence/autonomous-sdlc/` for milestone execution evidence unless an
existing assessment location is explicitly named. Include date, revisions, commands
without secrets, observed results, failures, limitations, and commit/release identities.
A test result and a deployed result are separate claims.

Review coverage: F13 complete-journey proof.
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

