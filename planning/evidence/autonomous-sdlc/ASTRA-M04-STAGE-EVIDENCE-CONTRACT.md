# ASTRA M04: Align stage submissions and qualification evidence

Status: implemented and locally validated; **not deployed or live-qualified**.
Source: `cb5142ff0e58bbb7c86e167cf50eda8a83343b70`.
Base main: `812e063e7decc19726f1540329b59c6a37ecd484`.

## Corrected behavior

V2 Test Diagnosis and Verifier tools now expose the exact controller evidence IDs
in their submission schema. The runtime checks every reference against that catalog
before accepting the document. Unknown IDs, payload hashes, filenames, duplicates,
empty lists and ambiguous duplicate catalog entries are rejected with a bounded
correctable error. V1 submission behavior and stored history are preserved. A
reference proves identity only; the controller and evaluator still assess the claim.

Planner scoring distinguishes direct prohibitions from proposals to run a forbidden
operation. Explicit warnings such as “Do not run curl” no longer fail solely because
they mention the excluded command. Affirmative, conditional, double-negated or
ambiguous clauses remain rejected. This is deliberately a narrow rule, not a general
natural-language permission parser. Structured writable-path and acceptance checks
remain enforced. Complex warnings may still need clearer wording; the scorer does
not authorize actual execution.

All twelve Test Diagnosis scenarios remain. Their snapshots and synthetic Test
receipts now agree about assertion, compilation, lint, semantic, environment,
contract, timeout and successful outcomes. Preexisting/localized/coherent failures
carry an explicit underlying assertion type, rather than falling through to unknown
because history or repair scope was confused with a failure type. Six snapshots
reproduce their declared assertion, syntax-error or passing receipt using the actual
Python commands. Lint, timeout, missing-tool and controller-boundary receipts remain
explicit synthetic fixtures, not claims of live application execution.

The diagnosis prompt separates failure type, causal confidence and repair scope.
It requires evidence IDs and prohibits inventing causes from filenames, changing
budgets/contracts/environment, or recommending repair for passing evidence.

## Compatibility and qualification

Planner and Test Diagnosis suites advance to **stage-qualification-v2.2**. The diagnosis
stage prompt is **2026-09-06.1**. Their hashes and per-run tool-schema hashes change
explicitly. Onboarding, Verifier, Builder and Repair fixtures retain their existing
revisions; the frozen 24-task coding data is unchanged. No model, policy registry,
execution/token/time/repair limit or acceptance threshold changes. Stored failed
runs are not rescored. Compatible historical reads remain intact; no migration or
production action is introduced.

**520 distinct tests passed** across the affected core, runner, evaluator and API
packages. Clippy with warnings denied, formatting, architecture and five parser
checks passed. Tests cover reference correction and legacy behavior; positive and
negative Planner clauses; exact suite revisions; fixture/source consistency; all
stage replays; and existing controller, policy and evidence behavior.
[Validation and hashes](ASTRA-M04-STAGE-EVIDENCE-CONTRACT-VALIDATION.json) retain the
per-binary counts. Nested expected failures in benchmark logs are not counted again.

The Planner fixtures remain a limited test of bounded planning rather than a complete
simulation of product maintenance. The live Finance acceptance and frozen coding
runs remain necessary. The current source-19 Verifier result must be collected before
changing its runtime; do not repeat old-source Builder/Repair runs after these defects
have made a newer release necessary. Their full required checks run on the next exact
release. Keep all qualification Jobs serial.

## Release and remaining gates

Merge with the reviewed staging implementation, then build one final source revision
using the immutable seven-image/native-bundle procedure. Preserve schema 54 and
Finance history. Keep hosted creation and Coding Reliability V2 disabled. Run fresh
protocol and stage qualification, plus both required frozen coding runs, on that
release before activation. The earlier failures remain inspectable evidence. The
MiniMax transport fix is included in the base source; its live qualification is still
required. M04 and autonomous Finance acceptance remain open.
