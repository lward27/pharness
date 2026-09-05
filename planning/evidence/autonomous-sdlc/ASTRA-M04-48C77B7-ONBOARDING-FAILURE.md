# ASTRA M04: Onboarding qualification failed

Status: **not qualified**. The immutable `48c77b7` runtime completed the evaluation
on 2026-09-05 at 18:03:24 UTC. Job completion is not qualification success.

Evaluation: `infeval_01a07286ae577961a520ab17797fd2a6`.
Qualification: `inferqual_01a072bd50bf73e2880bf1d38d3e82a1`.
Target: `accounts/fireworks/models/minimax-m3`, policy
`onboarding-minimax-m3-v2@v1`, through the existing gateway.

## Objective results

| Check | Result |
| --- | --- |
| Protocol calibration | 30/30 passed |
| First twelve-case stage attempt | 0/12 passed |
| Second twelve-case stage attempt | 0/12 passed |
| Accepted typed onboarding submissions | 0/24 |
| Terminal reasons | 13 missing actions; 6 exhausted tool recovery; 5 soft budget boundaries |
| Workspace mutations | None reported |
| Qualification gate | Failed |

The model attempted `submit_onboarding_proposal` in multiple cases, but no typed
submission was accepted. Six cases stopped at the tool-recovery ceiling after
`invalid_arguments` failures. Others ended without a usable provider action or
at the existing soft budget boundary. The raw report labels the complete fixture
execution `infrastructure_valid: true`; that does not establish an accepted
proposal or validate the representativeness of every fixture.

Recorded usage: 1,357,101 prompt tokens, 223,429 completion tokens, 1,098,140 cached
tokens and 147,634 reasoning tokens across 318 turns. Cached and reasoning counts
are overlapping categories, not extra totals to add. There were 53 recoverable
failures and 3,545,275 milliseconds of recorded fixture duration. No limits were
increased and no fallback model was selected.

## What the evidence can and cannot explain

The protocol check proved a small transport/tool-call boundary. It did not prove
the model could complete the actual onboarding contract. Its green result did
not authorize activation; the separate stage gate correctly rejected this run.

The deployed report retained action names and terminal categories but discarded
the concrete recoverable validation messages. It cannot now establish which
proposal fields were rejected. Declaring the model alone responsible for all
24 failures would go beyond that evidence.

Source inspection also found an evaluation concern: `prepare_workspace` builds
the same Python scaffold for every stage case, including cases described as
Node repositories and missing-lock situations. Expected candidate contracts are
in fixture evidence, while this onboarding profile exposes filesystem reads and
submission, not `get_evidence`. This warrants a review of the fixture/context
boundary before interpreting the result as a broad onboarding capability score.
It is not proof of the exact rejected field and does not turn this failure into
a pass. No fixture, expected result, threshold, profile or prompt was changed.

## Implemented diagnostic correction

The evaluator now retains the last concrete recoverable tool failure alongside
the terminal reason, using the existing bounded, redacted `failure_detail` field.
It labels that message as historical context; it does not replace a later
provider error or claim causation. Budget-boundary failures retain the same
context even when no `RunFailed` event exists. Missing historical evidence stays
missing. The live failed result above is immutable and was not rewritten.

All **23 evaluator tests** passed, including the frozen coding/stage replays and
new cases for recovery exhaustion, subsequent provider failure, budget exhaustion,
missing evidence and sensitive-line redaction. Evaluator Clippy with warnings
denied and formatting/diff checks passed. This correction is not deployed and
does not itself qualify onboarding.

See the [raw result](ASTRA-M04-48C77B7-ONBOARDING-LIVE-RESULT.json) and
[analysis and source/log hashes](ASTRA-M04-48C77B7-ONBOARDING-ANALYSIS.json).
Planner and Test Diagnosis subsequently failed; their
[scorer contract correction](ASTRA-M04-STAGE-SCORING-CONTRACT-CORRECTION.md)
does not change this onboarding result. Verifier and Repair remain serial.
Builder's two 24/24 passes remain valid for `48c77b7`.
M04, hosted activation and end-to-end acceptance remain open. A later runtime
must be qualified against its own exact revision.
