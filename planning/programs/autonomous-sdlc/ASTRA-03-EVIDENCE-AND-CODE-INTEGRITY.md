# ASTRA M03: Evidence and code integrity

Status: accepted (committed implementation and tests; deployment remains pending the program release).
Authority: [approved program](ASTRA-00-PROGRAM.md).
Dependencies: M01. May proceed independently of M02.

## Objective and scope

Make normalized evidence truthful and restore existing engineering guardrails without a broad rewrite.

The program's locked authority, budget, source identity, compatibility, and evidence
rules apply. Work on current verified main in an isolated `codex/` worktree.
Do not mark this milestone accepted because a dependency appears healthy.

## Implementation

1. Revalidate the landed source-closure fix and retain useful idempotence regression coverage; do not reimplement it.

2. In verifier sealing, preserve submitted risks and contradictions in normalized outcomes. Keep agent claims distinct from independently verified facts. An approved submission with unresolved contradictions cannot become unconditional success.

3. Repair scripts/app-module-dependencies.py so prose inside Rust strings/comments is not parsed as a use tree. Cover nested comments, raw/escaped strings, use groups, and real imports.

4. Split oversized products.rs and repo_mode.rs along current responsibilities using the established architecture allowlist. Preserve route behavior and external-effect boundaries; do not relax size or dependency rules.

5. Untrack only generated evaluation workspace outputs, retain intentional result/evidence artifacts, and add appropriately scoped ignore rules.

## Interfaces and compatibility

Existing StageOutcome normalization and module ownership; no new workflow resource or service split.

Preserve immutable historical evidence, existing Finance data generation, and additive migration compatibility. Record any minimum compatible rollback version before enabling new writes.

## Tests and acceptance

- [x] Approved-with-risk retains visible caveats; approved-with-contradiction cannot normalize to succeeded; raw submissions remain immutable.
- [x] Normal prior-state closure and repeated closure do not create duplicate stage executions or rewrite history.
- [x] Both architecture boundary and dependency checks pass under their existing limits.
- [x] Focused regression tests and the required workspace checks pass. No public API behavior changes beyond correcting contradictory outcome semantics.
- [x] Tracked-file inventory distinguishes intentional evidence from disposable target/workspace output.

Inspect current regression coverage first; add tests only for substantive gaps. Run parser/boundary checks, focused Rust tests, fmt, clippy, and workspace tests as appropriate to the change.

## Deployment and recovery

Record exact changed resources, source revisions, validation, before/after identities,
and external effects. Keep managed cluster changes in authoritative GitOps sources.
Production runtime acceptance still requires a concrete human approval.

Revert code commits without mutating sealed historical outcomes. Existing historical records remain evidence; do not bulk rewrite old caveat fields.

## Evidence and closeout

Write ASTRA-M03-EVIDENCE-AND-CODE-INTEGRITY.md with test results, parser checks, module boundary results, and removed generated-file inventory.
Use `planning/evidence/autonomous-sdlc/` for milestone execution evidence unless an
existing assessment location is explicitly named. Include date, revisions, commands
without secrets, observed results, failures, limitations, and commit/release identities.
A test result and a deployed result are separate claims.

Review coverage: F01, F02, F11, F12.
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


Acceptance evidence: [M03 report](../../evidence/autonomous-sdlc/ASTRA-M03-EVIDENCE-AND-CODE-INTEGRITY.md).
