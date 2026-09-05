# ASTRA M04: Correct the stage qualification contracts

Status: implemented and locally validated; **M04 remains unqualified**.
Source baseline: `3388a6ad32045db43daedc09c8cff9b59ab19203` (PR 349), isolated
branch `codex/astra-stage-contract-integrity`, 2026-09-05.
The running qualification release remains `48c77b7b4438d621ff9563b913857bcf771f1800`.

## Recorded results

| Stage | First attempt | Second attempt | Recorded disposition |
| --- | --- | --- | --- |
| Builder | 24/24 first pass | 24/24 first pass | Passed on 48c77b7 only |
| Onboarding | 0/12 | 0/12 | Failed; rejected submissions and fixture/context concerns remain under investigation |
| Planner | 4/12 | 7/12 | Failed; all thirteen mismatches carry the scorer's `undeclared_command_or_path` label |
| Test Diagnosis | 0/12 | 0/12 | Failed; all twenty-four mismatches carry `test_failure_misclassified`; ten also lack the expected evidence reference |
| Verifier | Running | Pending completion | No qualification claim |
| Repair | Pending | Pending | No qualification claim |

All four non-coding stage protocol calibrations passed 30/30 before their stage
runs started. Protocol success did not establish stage capability. Planner and
Test Diagnosis report no workspace changes; their scorer violation labels are
not evidence that application source was modified or a forbidden command executed.

Planner evaluation `infeval_01a072bf2c737ae2bc7cbeab02c5524b` ran from 18:05:26
to 18:29:16 UTC. Test Diagnosis evaluation
`infeval_01a072d57b3e71508aa856dfa6d328a4` ran from 18:29:48 to 18:32:32 UTC.
The original reports, hashes, policy identities, usage and stage gate results are
preserved in the [Planner analysis](ASTRA-M04-48C77B7-PLANNER-ANALYSIS.json),
[Planner raw result](ASTRA-M04-48C77B7-PLANNER-LIVE-RESULT.json),
[Test Diagnosis analysis](ASTRA-M04-48C77B7-TEST-DIAGNOSIS-ANALYSIS.json), and
[Test Diagnosis raw result](ASTRA-M04-48C77B7-TEST-DIAGNOSIS-LIVE-RESULT.json).
The original failed qualifications are not edited, rerated, or presented as passes.

## Objective defects

**Test Diagnosis could not satisfy the published tool contract and the scorer at
the same time.** The advertised tool requires `failure_kind` and
`repair_recommendations` and forbids additional properties. The scorer instead
required `classification`. Its canned replay supplied that prohibited field,
plus `repairable` and `recommended_scope`, and bypassed the advertised schema.
Consequently, its replay green result concealed a real harness defect. A
schema-compliant live submission necessarily failed the classification check.

**Planner treated mention as proposed execution.** Its scorer searched the entire
serialized plan for strings such as `curl`, `npm install` and `deploy/`. A risk
warning saying not to use them failed the same check as an instruction to use
them. Conversely, mentioning acceptance names somewhere in prose could satisfy
coverage without declaring them in the plan's steps. Regression tests reproduce
both defects against the former checks.

The old reports do not retain the actual successful submissions, so they cannot
prove that all thirteen Planner mismatches were innocent warnings. Ten Test
Diagnosis cases also failed the separate evidence-reference check. Neither fact
may be dismissed to manufacture a successful qualification.

## Implemented correction

The V2 Planner scorer now validates the WorkPlan shape, declared acceptance names
and actual proposed paths. Every required acceptance name must appear in a
step's `acceptance_names`; unknown names and paths outside the fixture's writable
boundary fail. Free-form command fields fail. Explicit execution steps remain
conservative about forbidden operation text. Warnings in `risks`, `assumptions`
and summary text do not authorize those operations. No natural-language negation
classifier was added; a warning embedded in an executable step may still be
rejected conservatively. Historical V1 Planner scoring is unchanged.

The Test Diagnosis scorer and replay now use the actual `failure_kind` enum and
`repair_recommendations` array. The fine-grained controller category must remain
in the summary; its normal space-separated spelling is accepted. The original
fixture evidence reference remains mandatory, invented references fail, and the
passing-control case cannot propose a repair. The twelve fixture identities,
tasks and supplied evidence remain unchanged.

Failed stage reports now preserve bounded, redacted contract diagnostics even
when the model successfully submitted a document. These diagnostics retain the
relevant received fields and expected constraint; they do not invent a missing
submission or replace a later provider failure. Existing tool-recovery diagnostics
from PR 348 remain in place.

## Versioning and acceptance

Planner V2 and Test Diagnosis V2 move from `stage-qualification-v2.0` to
`stage-qualification-v2.1`, producing new suite hashes. Reports and evidence
catalogs now obtain their revision from the same definition as that hash; they
no longer incorrectly label V2 fixtures as V1. The frozen coding and Repair suites,
all acceptance thresholds, model/profile selections, prompts, tool schemas and
execution limits are unchanged. Existing qualifications cannot satisfy a new
suite hash or the separately enforced exact-runtime gate.

This is a correction to defective scoring, not a waiver of its intended safety
and quality boundaries. Regression checks reject invented commands/paths,
acceptance names supplied only in prose, wrong or missing evidence, wrong
classifications, malformed documents and repairs to a passing control. A replay
test now checks the Test Diagnosis submission against the advertised tool's
required properties and enum, addressing the specific bypass that hid the defect.

All **466 API/core/evaluator tests** passed, including 27 evaluator tests and the
unchanged frozen coding/stage replays. The four new contract regression tests
were rerun after the final assertion was tightened. Clippy with warnings denied,
architecture checks, formatting and diff checks passed.

The [validation record](ASTRA-M04-STAGE-SCORING-VALIDATION.json) records source
hashes, test results, suite revisions and unchanged frozen inputs. These corrections
are not deployed and do not qualify the live model profiles. Preserve active
48c77b7 operations; release a compatible immutable image set only after they are
terminal, then obtain fresh qualification against that exact release.

Onboarding's workspace/context representativeness issue remains open and must
be resolved before another full qualification attempt is treated as meaningful.
This slice does not change those fixtures or claim to explain their exact rejected
fields. Hosted creation, autonomous source acceptance, M04 and M11 remain blocked
on their actual gates.
