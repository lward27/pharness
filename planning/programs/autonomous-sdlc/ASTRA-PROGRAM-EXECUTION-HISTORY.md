# ASTRA: Program execution history

This file preserves superseded status descriptions and source references. They are dated implementation history, not current operating instructions. Use [the master program](ASTRA-00-PROGRAM.md) for current milestones, authority, dependencies and next work. Individual evidence files remain the authoritative records for their checks.

## Status record through 2026-09-05, before the schema 53 release was verified

Next eligible: address the measured M04 coding failures and qualify the resulting
immutable runtime while continuing M06, M07 and M10 preparation. Source `48c77b7`
is being built as one immutable controller/console release; live PHarness remains
on `2249950`. The isolated schema-53 migration passed against the Finance snapshot;
live migration and its compatible rollback floor remain pending.
The fd74092 live evaluation completed at 12:34 UTC and failed: 22/24 and 20/24,
two hidden-test false passes, and one blocked write outside permitted paths.
There were no provider or infrastructure failures in this run. See the
[failure analysis](../../evidence/autonomous-sdlc/ASTRA-M04-FD740-QUALIFICATION-FAILURE-ANALYSIS.md).
All 216 packaged offline checks passed; those fixtures do not qualify the model.
The original backend artifact is restored; both staging deployments are Healthy;
all 13 isolation checks passed. Cert-manager 1.20.3 preserves 31 ready certificates
and 78 retained requests. The owner-authorized Mac serves Tekton's existing
BuildKit endpoint; uncached AMD64 execution, a 112 MiB private TLS push, and
exact-digest pull/run passed. Worker capability checks passed after the GitOps
writer credential was rotated. Runtime contract declarations remain M05 and the
real frontend pipeline is deployed through GitOps `491f081`; sequential backend
and frontend Tekton builds passed with independently verified registry identities.
Automatic source delivery and dispatch remain M07, with the repository-protection
decision pending owner input. The tested M04 scratch cleanup is merged at
`fd740927110366a983de6bb0d3bc6c576577708b`; its [release evidence](../../evidence/autonomous-sdlc/ASTRA-M04-CODING-RELIABILITY-QUALIFICATION.md)
does not replace live model qualification. M05 now enforces saved stage profiles, gateway choices and limits in
merged code; [live acceptance remains open](../../evidence/autonomous-sdlc/ASTRA-M05-UNIFIED-SDLC-CONTRACT.md).
Fresh gateway calibration passed for Builder and Planner. MiniMax's malformed
history rejection exposed a protocol compatibility defect; that repair and complete
stage-report guards merged in PR 330 at `db84b797f1bbc833ba86844874d1d041bc33ab72`.
The completed coding evaluation used fd74092. New runtime qualification remains
required; keep qualification jobs serial. M05 compatible-reader source merged in
PR 328 at `2249950d225a4632b24235c2b6f2d8469a774243`. Its complete seven-image
AMD64 release and native bundle were verified and deployed through PR 333, merge
`8ca88f32e3d50f8430cf5a486912ebe6d00a392d`. Argo and all five long-running
Deployment image identities matched; hosted creation and Coding Reliability V2
remain disabled. See [reader release and rollback floor](../../evidence/autonomous-sdlc/ASTRA-M05-COMPATIBLE-READER-RELEASE.md).
M04 contract clarification merged through [PR 331](https://github.com/lward27/pharness/pull/331)
at `4c40b10c0b2f71ab92d464528145e178222a3368`. Its 28 runhost tests and Clippy pass. Its live qualification follows a new immutable release; the current reader
release does not include that clarification. Neither source merge nor provider
diagnostics close M04.

M06 engineering progression is integrated at `e1709a2` on
`codex/astra-autonomous-controller`, following persistence `9d52c9e`, dispatch
recovery `bded5a1`, and controls/admission `51485ba`. Atomic preparation recovery follows at `f755915`, with 304 distinct API/admin/store
tests passing. Terminal normalization recovery follows at `1dc8c97`, with 306
distinct passing API/admin/store tests. Source delivery dispatch ordering follows at `deb7ebf`, with 308
distinct passing tests. These controller changes merged through [PR 332](https://github.com/lward27/pharness/pull/332)
at `ba8ce03e4dfd3df5815c897a69276858b53aacb2`. Full combined validation passed
641 workspace tests, Clippy, formatting and architecture checks; see the
[combined validation record](../../evidence/autonomous-sdlc/ASTRA-M06-COMBINED-WORKSPACE-VALIDATION.json).
Controller delivery integration, terminal cancellation and live acceptance remain open. These independent preparations do
not waive M05 or M04 gates. See
[controller evidence](../../evidence/autonomous-sdlc/ASTRA-M06-DURABLE-AUTONOMOUS-CONTROLLER.md).
M10 initial console corrections merged through [PR 334](https://github.com/lward27/pharness/pull/334)
at `ca98fa7c7474902d206e130ca14eddddec8d82a7`. All 79 UI unit checks, the production build,
and the real API journey with both console flags passed against combined M04/M06 source.
See the [console evidence and subjective review](../../evidence/autonomous-sdlc/ASTRA-M10-CONSOLE-CONVERGENCE-AND-POLISH.md).
The documented remaining concerns and delivery dependencies keep M10 open.

The next console slice is [PR 337](https://github.com/lward27/pharness/pull/337),
source `1b52f2520c38dd185f02c82760939d5c037b9642`, based on `48c77b7`.
It preserves focused searches, repairs disappearing pagination, independently pages
legacy records, and makes failures readable. The UI build, 94 unit checks and
116 distinct browser checks passed; this source is not yet deployed.
See [list evidence and visual judgment](../../evidence/autonomous-sdlc/ASTRA-M10-LIST-CONSISTENCY.md).

Schema 0052 is applied to the live Finance database; 0053 remains undeployed.
A new 21,213,184-byte pre-0053 snapshot preserves the same 14 WorkItems and 82 Runs;
its [verified manifest](../../evidence/autonomous-sdlc/ASTRA-M06-DATABASE-ARCHIVE-VERIFIED.json)
is retained. Clone migration and a compatible immutable schema-53 rollback floor
are required before the controller release. Neither snapshot is an independent
disaster-recovery backup.
A verified 21,204,992-byte pre-0052 snapshot is retained on its existing PVC.
The immutable 2249950 reader successfully migrated an isolated copy to 0052 while
preserving its 14 WorkItems, 82 Runs, evidence, audit records and four holds; see
[isolated migration proof](../../evidence/autonomous-sdlc/ASTRA-M05-CLONE-MIGRATION-VERIFIED.json).
The subsequent live read-only comparison verified all original WorkItems, Runs,
stage outcomes, audit records and holds unchanged; see [live preservation evidence](../../evidence/autonomous-sdlc/ASTRA-M05-LIVE-DATABASE-VERIFIED.json).
See [M02 evidence](../../evidence/autonomous-sdlc/ASTRA-M02-FINANCE-PLATFORM-READINESS.md).
M03 implementation is `b354c2b534fb4f518a439e92bb6770c8287fd4fd`; see [its acceptance evidence](../../evidence/autonomous-sdlc/ASTRA-M03-EVIDENCE-AND-CODE-INTEGRITY.md).
A real external blocker may suspend its dependent work but never waive its gate.
Continue eligible independent work and ask only for missing authority/credentials
or a decision with material downstream consequences.


## Superseded M04 execution sequence before owner-approved reassessment — 2026-09-06

The owner approved the process reassessment on 2026-09-06. The following prior instructions are retained as history only. The revised M04 document owns the active sequence; no failed result is rescored.

### Previous master status

### Current qualification — 2026-09-06

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

Next eligible: finish serial source-19 qualification, correct proven context/evaluator defects and prepare the next immutable release; continue M08 runtime integration and
M10 refinement. All qualification Jobs on `48c77b7` are terminal. After live history preservation passed at 16:20 UTC, the gateway protocol checks
passed 30/30 and coding evaluation `infeval_01a0725f855b7c038234cd6af3830594` started
with two frozen attempts. Both finished 24/24 on the first pass, with every stack
8/8 and no reported hidden-test false passes or policy violations.
[Builder qualification](../../evidence/autonomous-sdlc/ASTRA-M04-48C77B7-BUILDER-QUALIFICATION.md)
is passed. [Onboarding failed 0/12 in both attempts](../../evidence/autonomous-sdlc/ASTRA-M04-48C77B7-ONBOARDING-FAILURE.md)
despite passing 30 protocol checks. Planner subsequently scored 4/12 and 7/12;
Test Diagnosis scored 0/12 in both attempts. Inspection found a Test Diagnosis
scorer/tool-schema mismatch and an overbroad Planner substring check. These require
contract-aligned regression fixes and fresh qualification, not reclassification of
the recorded results as passes. The [scoring correction](../../evidence/autonomous-sdlc/ASTRA-M04-STAGE-SCORING-CONTRACT-CORRECTION.md)
preserves the twelve-case scenarios and thresholds, changes the two affected
suite revisions to V2.1, and adds bounded contract diagnostics. The subsequent
[onboarding correction](../../evidence/autonomous-sdlc/ASTRA-M04-ONBOARDING-CONTRACT-AND-FIXTURE-CORRECTION.md)
retains blocked proposals without invented contracts, guards approval/source effects,
and uses real discovery for twelve distinct V2.1 fixtures. It requires compatible
API/worker/UI deployment and fresh live qualification. Existing failed results remain unchanged.
[Verifier finished at 20:14 UTC with 1/24 and 2/24 passes](../../evidence/autonomous-sdlc/ASTRA-M04-48C77B7-VERIFIER-FAILURE.md).
Its zero reported false approvals do not override 43 evidence/marker mismatches
and two missing submissions. [Repair completed at 20:54 UTC with 24/24 in both attempts](../../evidence/autonomous-sdlc/ASTRA-M04-48C77B7-REPAIR-QUALIFICATION.md),
8/8 per stack and eight recoverable tool failures within existing limits. Its
seeded repair proof does not establish correction of an actual failed Builder workspace.
The [submission diagnostic correction](../../evidence/autonomous-sdlc/ASTRA-M04-STAGE-SUBMISSION-DIAGNOSTICS.md)
retains bounded accepted documents and separates the three Verifier predicates;
the scorer, fixtures, gates, profiles and limits remain unchanged.
Keep qualification Jobs serial. The frozen coding suite, thresholds, profiles and
execution limits remain in force. Scorer repairs require explicit suite revisions
and fresh evidence. The exact-runtime creation gate also requires
matching qualification on any subsequent release before activation. The earlier fd74092 runs failed; they remain
[recorded evidence](../../evidence/autonomous-sdlc/ASTRA-M04-FD740-QUALIFICATION-FAILURE-ANALYSIS.md),
not superseded passes.

The source-publication controller merged through [PR 338](https://github.com/lward27/pharness/pull/338),
with its normal-callback identity regression fixed in [PR 342](https://github.com/lward27/pharness/pull/342).
The finite build-dispatch restriction merged through [PR 336](https://github.com/lward27/pharness/pull/336),
and the latest list polish merged through [PR 337](https://github.com/lward27/pharness/pull/337).
Guarded source merge merged through [PR 344](https://github.com/lward27/pharness/pull/344)
at `2bbc7a77152d4104651702e84bac3b1893739fc3`; its
[validation](../../evidence/autonomous-sdlc/ASTRA-M07-GUARDED-SOURCE-MERGE.md)
includes persisted merge admission, exact source/base checks and independent provider observation.
The [verified build handoff](../../evidence/autonomous-sdlc/ASTRA-M07-HOSTED-BUILD-HANDOFF.md)
merged through [PR 345](https://github.com/lward27/pharness/pull/345) at
`252030cdd2e457e4658ed7489c7e6a833add2f28`; 464 API/core/worker checks passed.
It binds finite build authority to sealed source and retains declared Tekton outputs
and conflicts. The [durable build controller](../../evidence/autonomous-sdlc/ASTRA-M07-DURABLE-BUILD-CONTROLLER.md)
now records one build admission, original Job and PipelineRun identities, one read-only
recovery observer, bounded grants and duplicate-safe terminal receipts. Its implementation
passed 672 workspace tests and merged in [PR 347](https://github.com/lward27/pharness/pull/347)
at `94e81f89cfa6922224c23193520346a0884ced75`. Deployment and the actual autonomous build/registry chain
remain open.
These foundations are now included in the deployed 19b0c55 artifacts. Source-to-build
progression and live source-merge acceptance remain open. The owner approved the Finance source protections. Both CI pull requests are
merged and both main branches now require strict, app-bound checks and PRs,
including administrators, with zero human reviewers. The
[application and actual writer readback](../../evidence/autonomous-sdlc/ASTRA-M07-APPROVED-SOURCE-PROTECTION.md)
passed; no owner decision remains for this policy.
The owner updated the source writer's Administration read permission; both required
reads are now [verified as authorized](../../evidence/autonomous-sdlc/ASTRA-M07-SOURCE-CREDENTIAL-VERIFIED.json).
Autonomous source-merge acceptance and exact-runtime coding qualification remain open.

The [compatible correction release](../../evidence/autonomous-sdlc/ASTRA-19B0C55-IMMUTABLE-COMPATIBLE-RELEASE.md)
is deployed with all seven verified artifacts and the native bundle. Exact live
identity and schema/history preservation passed before fresh qualification began.
Hosted creation and Coding Reliability V2 remain disabled; no autonomous acceptance
is implied by this release.


### Previous M04 document


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


## M04 boundary corrections through source38 — archived 2026-09-08

The following is the superseded master-program narrative before the source80 Product-validation release. Its instructions and present-tense statuses are historical; follow the current master and M04 document. Original evidence and failed results are retained.

[M04A native submission binding](../../evidence/autonomous-sdlc/ASTRA-M04-TRUSTED-SUBMISSIONS.md)
is locally validated; stored proposals retain original discovery identity.
[M04B context delivery and replay](../../evidence/autonomous-sdlc/ASTRA-M04-CONTEXT-ENVELOPE.md)
also pass local checks; live provider canaries remain pending.
[M04C source-backed stage measurements](../../evidence/autonomous-sdlc/ASTRA-M04-STAGE-MEASUREMENTS.md)
are locally validated; all 48 scenarios are mapped and replayed with actual receipts.
[M04D bounded diagnostic execution and common-input controls](../../evidence/autonomous-sdlc/ASTRA-M04-DIAGNOSTIC-CANARIES.md)
are locally validated on `f0c7afd` (627 passing component tests; one existing live-only
test ignored). Partial results cannot qualify a policy. The current schema-54
[Finance database snapshot](../../evidence/autonomous-sdlc/ASTRA-M04-PRE0055-ARCHIVE-VERIFIED.json)
is verified, and the [real schema-55 clone migration](../../evidence/autonomous-sdlc/ASTRA-M04-DIAGNOSTIC-CLONE-MIGRATION-VERIFIED.json) preserves all original columns in all 81 tables. The [complete diagnostic release](../../evidence/autonomous-sdlc/ASTRA-M04-DIAGNOSTIC-COMPATIBLE-RELEASE.md) has seven verified images, a verified native bundle and validated pins.
The [matching deployment, live data preservation and full internal service window](../../evidence/autonomous-sdlc/ASTRA-M04-DIAGNOSTIC-LIVE-RELEASE.md) now pass.
The [2026-09-07 gateway/local-model preparation and scope analysis](../../evidence/autonomous-sdlc/ASTRA-M04-LOCAL-MODEL-READINESS.md)
identify the Onboarding scope mix-up and prepare a versioned shared prompt correction,
gateway credential wiring and bounded transport checks. The Windows Minisforum guide
is available; no local target is enabled and no local model is qualified.
[Accounting correction and Mac build recovery](../../evidence/autonomous-sdlc/ASTRA-M04-METERED-MODEL-TURNS.md) document why the incomplete first candidate was superseded.
[The complete accounting-corrected release](../../evidence/autonomous-sdlc/ASTRA-M04-METERED-GATEWAY-RELEASE.md) was deployed from `81edd9e`; its live identities matched. The ten-minute service window passed with 21 samples.
The new source-81 diagnostic failed without qualification after a passing 30/30 protocol renewal. It now separates the two permission scopes but submits nonrecursive directory strings. [The shared writable-scope contract gap](../../evidence/autonomous-sdlc/ASTRA-M04-WRITABLE-SCOPE-CONTRACT-GAP.md) records the evidence and exact implementation boundary.
The [shared scope correction passed its first diagnostic](../../evidence/autonomous-sdlc/ASTRA-M04-A2D05F1-ONBOARDING-ACCEPTANCE.md). The [native source-identity reader correction](../../evidence/autonomous-sdlc/ASTRA-M04-DIAGNOSTIC-SOURCE-RELEASE.md) is now released from source `38a4282c0bef27491a8e6e44640dbbe3f33526ca`; exact images, data preservation and the ten-minute service window pass. The [fresh primary/control comparison](../../evidence/autonomous-sdlc/ASTRA-M04-38A4282-ONBOARDING-CONTROL.md) proves identical retained inputs and context. Both models pass 1/2, on different cases. The native submission tool accepts a duplicate Service that the product API correctly rejects later.
**Next eligible: release the validated [shared Product submission checks](../../evidence/autonomous-sdlc/ASTRA-M04-ONBOARDING-PRODUCT-BOUNDARY.md), then run a fresh same-runtime onboarding primary/control pair.** The [Mac build route is restored through GitOps](../../evidence/autonomous-sdlc/ASTRA-M02-MAC-BUILDKIT-RETURN.md) while lucas-desktop is off. Broad
qualification waits for those gates. The source-19 Verifier has completed; retain its failed result without starting
another old-suite batch. M08 and
M09 preparation may continue independently, but their acceptance gates remain open.
The prepared Finance production-baseline change has not been approved; M04 approval
does not authorize its merge or count as M11 production approval.

Historical runtime `19b0c55` was unqualified: Onboarding 0/12 twice, Planner 6/12 and 7/12,
Test Diagnosis 1/12 twice, and [Verifier 1/24 twice](../../evidence/autonomous-sdlc/ASTRA-M04-19B0C55-VERIFIER-ANALYSIS.md). Builder and
seeded Repair on earlier `48c77b7` passed 24/24 twice, 8/8 per stack; actual failed
Builder-to-repair handoff and blind semantic verification remain unproven. Retain
all failed evidence. The context/evidence corrections are now deployed in `92f8f1b` and need fresh
qualification; old success cannot qualify this runtime.


## M04 measurement and protocol correction checkpoint — 2026-09-08

The [operator checkpoint before the combined release](https://github.com/lward27/pharness/blob/d675159cb205d222616d253d311706be1889bb91/planning/evidence/autonomous-sdlc/ASTRA-CLUSTER-OPERATOR-CHECKPOINT.md) preserves the preceding detailed operational narrative. It is historical, not the current resume instruction. Sourcee508 was deployed through pin040caeb with exact identities and a full passing ten-minute R2 window; the first failed window remains retained. Its same-runtime Onboarding primary passed 1/2 and matched control 2/2. Planner native1/2 and Verifier native3/3 were invalid measurements, so their controls were deliberately not dispatched. Diagnosis timed out at its unchanged 300-second first-response bound before a semantic evaluation. No model switch or qualification followed.

PR389 corrected diagnostic completeness and Planner selected commands. PR391 corrected and audited all semantic fixtures, advancing Planner to v2.5 and Verifier to v2.4 while preserving the frozen coding and repair tasks. Source24790cc was only preflighted. Source5a40945 subsequently produced seven verified images and a verified native bundle but was held from deployment after a shared-target protocol-check bug was found. PR392 fixes exact-policy selection, receipt binding and admission, and moves the console action beside each policy. The combined source is d675159. These are separate operations; no artifact or historical diagnostic is relabelled as a later release.

M08 draft363 was refreshed against5a40945 with851 Rust checks, including two explicit Helm tests; M09 draft366 with216 core/integration checks. Both remain drafts, unreleased and unaccepted. Their implementation gates and the real Finance production approval remain separate from M04.


## 2026-09-08: Complete d675159 diagnostics and merge the Planner exception guard

The three primary/control pairs are terminal with exact-policy30/30 prerequisites and independently matched retained inputs: Planner native2/2 versus2/2, Verifier GLM2/3 versus Kimi3/3, and Diagnosis Nemotron2/2 versus Kimi2/2. The previous Diagnosis timeout did not recur. These are diagnostics, not qualification; the Planner control's recommendation to weaken an existing regression exposes a real approval gap despite its native score.

PR395 merges the explicit saved-Run Planner readiness contract, blocked-outcome preservation, and shared exact-plan revalidation at automatic plan approval, implementation and ChangeSet approval. Its final implementation head is `4807e6dd69c73b70cc3ef07e943ebcd46037bf2d`, merged as `7b2c06daf4b549fa8acd62a363bd4aedb758fc2f`, with590 passing component tests across recorded full/focused checks and unchanged limits/schema/defaults. PR394 merges the separately validated evaluator layer reuse as `fda1174b57dfcf29ccc5ec2f6011bfe30c49e20e`. Both await one combined immutable release; serving images still contain d675159. The program must not count these local guards as live or connected-loop acceptance.


## Planner readiness and evaluator-layer release — 2026-09-08

Source `291e007cedcb2440a328f5e24710fb3423cb0ded` combines Planner readiness PR395, evaluator layer reuse PR394 and the retained d675159 diagnostics. Release PR398 merged as `90ab524aeef4fa6692057a6f89526674de3a8fa2`. [All seven images and the native bundle, exact serving identities and full ten-minute service observation](../../evidence/autonomous-sdlc/ASTRA-M04-PLANNER-READINESS-RELEASE.md) pass. The build took 2,065.052 seconds, reused all twelve stable evaluator toolchain steps, and recorded no TLS-handshake timeout warning; this does not establish a permanent fix for intermittent registry transport. Database generation and 82 ordinary Runs are preserved, with no SQL migration or gateway model/default/limit change. Hosted creation and Coding Reliability V2 remain disabled.

The separately validated repair authorization PR397 is not in this release. It closes a missing recheck before automatic repair using the same authority validator as the other stages; 301 API/admin checks plus the final regression, Clippy and architecture checks pass locally. Neither this correction nor the service release proves the connected M04E handoff. The new Planner v2.6 diagnostics must retain their own native outcomes; the previous v2.5 results stay unchanged.
