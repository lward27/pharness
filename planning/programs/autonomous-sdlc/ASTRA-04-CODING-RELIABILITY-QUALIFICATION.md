# ASTRA M04: Coding reliability qualification

## Current qualification — 2026-09-06

Runtime `19b0c55` remains unqualified. Onboarding scored 0/12 in both attempts;
Planner scored 6/12 and 7/12; Test Diagnosis scored 1/12 in both. All results are
retained. The [MiniMax context correction](../../evidence/autonomous-sdlc/ASTRA-M04-SYSTEM-CONTEXT-DELIVERY.md)
passes 86 local regressions but requires deployment and fresh qualification.
[Planner diagnosis](../../evidence/autonomous-sdlc/ASTRA-M04-19B0C55-PLANNER-FAILURE.md)
and [Test Diagnosis findings](../../evidence/autonomous-sdlc/ASTRA-M04-19B0C55-TEST-DIAGNOSIS-FAILURE.md)
separate remaining fixture/scorer defects from unsupported model claims. Fixing a
harness defect does not retroactively qualify any run. Verifier qualification is
in progress on source 19; all live evaluations remain serial. Hosted activation
and autonomous Finance acceptance remain disabled/open.

## Historical qualification on source 48

Status: active and not qualified. On runtime `48c77b7`, Builder passed both frozen
runs 24/24, while onboarding failed both twelve-case attempts at 18:03 UTC on
2026-09-05. Planner then scored 4/12 and 7/12; Test Diagnosis scored 0/12 in both
attempts. [Verifier failed with 1/24 and 2/24 passes at 20:14 UTC](../../evidence/autonomous-sdlc/ASTRA-M04-48C77B7-VERIFIER-FAILURE.md).
[Repair passed both 24-case attempts at 20:54 UTC](../../evidence/autonomous-sdlc/ASTRA-M04-48C77B7-REPAIR-QUALIFICATION.md),
including 8/8 per stack in each attempt after 30/30 protocol checks. These are
seeded repairs; correction of an actual failed Builder workspace remains unproven.
All stage attempts on this runtime are terminal. The
[stage scoring correction](../../evidence/autonomous-sdlc/ASTRA-M04-STAGE-SCORING-CONTRACT-CORRECTION.md)
documents proven harness defects, their bounded fixes and the required fresh
qualification. The [onboarding contract and fixture correction](../../evidence/autonomous-sdlc/ASTRA-M04-ONBOARDING-CONTRACT-AND-FIXTURE-CORRECTION.md)
adds explicit blocked proposals, prevents their approval, and replaces contradictory
V2 fixture workspaces with actual discovery. Its release and fresh live qualification
remain open; no historical failed result is rescored.
The [submission diagnostic correction](../../evidence/autonomous-sdlc/ASTRA-M04-STAGE-SUBMISSION-DIAGNOSTICS.md)
retains bounded accepted documents and separates the Verifier's three existing
predicates without changing its scorer, fixtures or gates. It also requires a
compatible release and fresh qualification; it cannot recover missing historical submissions.
See the [Builder evidence](../../evidence/autonomous-sdlc/ASTRA-M04-48C77B7-BUILDER-QUALIFICATION.md)
and [onboarding failure](../../evidence/autonomous-sdlc/ASTRA-M04-48C77B7-ONBOARDING-FAILURE.md).
Earlier `fd740927` results scored 22/24 and 20/24 and remain retained as
[failed historical evidence](../../evidence/autonomous-sdlc/ASTRA-M04-FD740-QUALIFICATION-FAILURE-ANALYSIS.md).
No failed result has been converted to a pass; a newer runtime requires matching qualification.
Evidence: [release, offline results and outstanding gates](../../evidence/autonomous-sdlc/ASTRA-M04-CODING-RELIABILITY-QUALIFICATION.md).
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

3. Freeze the existing 24-task Rust/Python/Node suite and the protocol/stage acceptance requirements. Pin prompts, models, policy/profile revisions, environment images, task revision, and all execution limits. A demonstrated harness defect requires documented regression evidence, an explicit new affected suite hash, and fresh qualification; it cannot justify editing a historical failed result or weakening a gate.

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

## Current Builder result — 2026-09-05

The exact `48c77b7` Builder passed both frozen runs 24/24 on the first pass, with every stack 8/8 and no reported hidden-test false passes or policy violations. [Pinned qualification and remaining gates](../../evidence/autonomous-sdlc/ASTRA-M04-48C77B7-BUILDER-QUALIFICATION.md) distinguish the successful Builder result from the still-open M04 milestone. Other qualification Jobs remain serial. A later runtime requires matching qualification before hosted creation.

The [stage evidence-contract correction](../../evidence/autonomous-sdlc/ASTRA-M04-STAGE-EVIDENCE-CONTRACT.md)
binds V2 references to controller IDs and gives Planner/Test Diagnosis explicit V2.2
suites with unchanged thresholds. It is locally validated and unreleased. Collect the
currently running source-19 Verifier result; the full required qualification then runs
on the next exact release, without repeating superseded old-source runs.

## Process reassessment requested by the owner — 2026-09-06

[ASTRA-M04-PROCESS-REASSESSMENT](../../evidence/autonomous-sdlc/ASTRA-M04-PROCESS-REASSESSMENT.md)
recommends a bounded redesign of agent-facing contracts and qualification before
more broad runs. It separates confirmed transport/evaluator defects from unisolated
model-quality concerns. This is a review recommendation; the existing acceptance
thresholds, runtime activation gate, models, budgets and production authority have
not changed. The in-flight source-19 Verifier run remains recorded separately.
