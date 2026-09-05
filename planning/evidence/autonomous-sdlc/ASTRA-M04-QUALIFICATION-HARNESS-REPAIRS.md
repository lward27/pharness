# ASTRA M04: Protocol history and qualification gate repairs

Status: implementation tested; new runtime publication and live qualification pending.
Observed 2026-09-05. Source base:
`548b978c33f8f32fb23d91120ef65a3502188d1c`. This is part of
[M04](../../programs/autonomous-sdlc/ASTRA-04-CODING-RELIABILITY-QUALIFICATION.md),
not a new coding backend or a relaxed acceptance bar.

## Protocol rejection

The primary MiniMax M3 target failed gateway calibration on the deployed fd74092
runtime. Direct, bounded diagnostics accepted the first six protocol-history
inputs and rejected `malformed_arguments_correction` with HTTP 400. The provider
requires historical function arguments to be a JSON object. Its response
identified invalid JSON in that historical call. See the
[diagnostic record](ASTRA-M04-FD740-MINIMAX-HISTORY-DIAGNOSTIC.json).
The configured model path and function-calling support are listed by
[Fireworks](https://fireworks.ai/models/fireworks/minimax-m3); advertised support
does not establish compatibility with PHarness's actual request history.

Outgoing history now wraps invalid or non-object arguments in an explicitly
rejected error object that retains the exact original argument bytes. The
original ModelMessage and persisted failure remain unchanged. Valid argument
objects retain their original bytes. Tool-call identity and the controller's
error result remain paired. This conversion applies only when constructing
outgoing history: malformed current actions, including the error envelope itself,
still fail action parsing and cannot execute.

The proposed envelope received HTTP 200 in a separate, bounded MiniMax diagnostic.
[That record](ASTRA-M04-MINIMAX-RECOVERY-ENVELOPE-DIAGNOSTIC.json) is diagnostic
evidence only: it is not a 30/30 gateway result or a model qualification. Calibration
errors also identify their case and attempt so future diagnosis does not require
guessing which input failed.

All ten protocol cases and their three attempts remain unchanged, including the
malformed-history recovery challenge. The 24 coding/repair tasks, models, prompts,
policy limits, correction allowance and qualification thresholds are unchanged.
No deployed configuration or running evaluation was altered by this code change.

## Incomplete qualification reports

Code inspection found that non-coding V2 gates did not require complete results.
For example, zero successes out of zero rows met the old Onboarding comparison,
and a two-attempt Planner report used an aggregate 11-success floor. These are
acceptance-logic defects, not observed successful live qualifications.

V2 stage gates now require the expected unique fixture count in every requested
attempt, reject missing/duplicate/misnumbered results and infrastructure aborts,
and apply the Planner's existing 11/12 floor to each attempt. A 12/12 run followed
by 10/12 fails; 11/12 in each satisfies that floor. Existing zero false-approval
and quality constraints still apply. Coding/Repair's existing per-stack and
per-attempt gates are unchanged. Reports expose completeness separately from
model quality; neither missing data nor an aborted attempt can become a pass.

## Validation

The shared transport, gateway, run host, evaluator and API/admin suites passed
288 tests: 13 transport, 3 gateway, 28 run host, 20 evaluator and 224 API/admin.
The two subsequent stage-gate regressions passed alongside seven existing
qualification tests; those repeated seven are not counted twice. Five core V2
protocol/correction tests also passed. **295 distinct tests passed.**

Tests retain raw rejected history, enforce valid history byte preservation, reject
malformed current actions and replay envelopes as executable actions, preserve
output limits, retain bounded correction behavior, reject incomplete reports and
enforce each Planner attempt. The evaluator's intentional negative fixture
subprocesses print failed test output; the enclosing evaluator suite passed.
Those nested failures are not hidden or counted as model qualification results.

Clippy passed for all five affected consumer crates/targets with warnings denied.
Formatting and architecture checks passed, including five dependency-parser tests
(reported separately from the 295 Rust tests). Full published-image replay and
live qualification must run again on the next exact source revision.

## Deployment and recovery

The existing coding evaluation on fd74092 continues unchanged. Retain its eventual
result as evidence for that runtime. Build the required immutable release set from
one merged source, pin its images, verify Argo/live identities, then run fresh
protocol and stage qualification serially. Never reuse the older runtime's result
as qualification for this repair.

This change has no database migration and does not enable V2 or hosted creation.
If combined with the separately reviewed M05 reader release, its migration-0052
rollback floor applies; record that compatible release before deployment. M04,
M05 and F13 acceptance remain open until their respective gates are evidenced.
