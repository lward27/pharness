# ASTRA M01: Current baseline and authoritative documentation

Status: accepted (2026-09-04).
Authority: [approved program](ASTRA-00-PROGRAM.md).
Dependencies: None. This is the first milestone.

## Objective and scope

Establish one truthful account of the implementation, deployed release, approved direction, and remaining proof.

The program's locked authority, budget, source identity, compatibility, and evidence
rules apply. Work on current verified main in an isolated `codex/` worktree.
Do not mark this milestone accepted because a dependency appears healthy.

## Implementation

1. Compare the original review at 12d36e9 with current remote main and live Argo/image identities. Preserve the dated review; add an evidence-backed current-baseline addendum.

2. Create this master and all twelve milestone documents. Reconcile every F01–F16 finding with current code, tests, UI, and release evidence.

3. Correct README, planning/active indexes, product vision/model, stage handoffs, and ui/AGENTS.md. Mark source-only contracts and earlier campaigns as historical or subordinate, with replacement links.

4. Record that Lamina and both Finance market features already exist. Keep coding qualification incomplete until its gates pass.

## Interfaces and compatibility

Documentation authority and links only; no API, schema, feature-flag, or runtime behavior change.

Preserve dated documents as history while correcting current entry points.

## Tests and acceptance

- [x] All twelve documents contain scope, dependencies, implementation instructions, tests, recovery, evidence, and an execution prompt.
- [x] Local Markdown links resolve; code and runtime claims identify exact revisions and distinguish observation from historical reports.
- [x] Every F01–F16 finding has a disposition, responsible milestone, and closure criterion. No existing implementation is counted as new work.
- [x] Documentation accurately says hosted automation is approved but not yet accepted. Original checkout and untracked user files remain intact.

Validate links and diff whitespace. Refresh bounded cluster metadata and source identities; do not invoke models or mutate a deployment to prove this documentation milestone.

## Deployment and recovery

Record exact changed resources, source revisions, validation, before/after identities,
and external effects. Keep managed cluster changes in authoritative GitOps sources.
Production runtime acceptance still requires a concrete human approval.

Revert the documentation commit if necessary. Preserve dated review and evidence records; do not rewrite historical results.

## Evidence and closeout

Write ASTRA-M01-BASELINE-AND-DOCUMENTATION.md plus ASTRA-CURRENT-BASELINE-ADDENDUM.md in the assessment directory.
Use `planning/evidence/autonomous-sdlc/` for milestone execution evidence unless an
existing assessment location is explicitly named. Include date, revisions, commands
without secrets, observed results, failures, limitations, and commit/release identities.
A test result and a deployed result are separate claims.

Review coverage: F08; initial classification and ownership for F01–F16.
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

