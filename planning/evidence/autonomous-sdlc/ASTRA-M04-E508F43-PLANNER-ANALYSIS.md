# ASTRA M04: Planner acceptance-input mismatch

Status: native diagnostic completed 1/2; the failing case is not a valid model comparison. The matched Planner control is deliberately undispatched. M04C is reopened for this bounded input correction; M04D–F remain open.

## Observed result

Source `e508f430e9f006c5f3e9f74fdd120d275605d85c`, Planner suite `stage-qualification-v2.3`, hash `sha256:01f45441afaa41c3e9be862575953434fc6ef19271522ab273c75667e2baba00`. The [30/30 protocol receipt](ASTRA-M04-E508F43-PLAN-PRIMARY-PROTOCOL.json) preceded evaluation `infeval_01a0820b8a4c7a329e029bf2d2916a24`. The [native result](ASTRA-M04-E508F43-PLAN-PRIMARY-RESULT.json) and [final execution receipt](ASTRA-M04-E508F43-PLAN-PRIMARY-EXECUTION.json) are retained unchanged; qualification is null.

- `acceptance-boundary` passes with concrete implementation, test and documentation steps covering selected `unit` and `compile`. Five tool calls; 53.836 seconds; 18,411 input and 2,522 completion tokens.
- `failing-baseline` fails `undeclared_command_or_path`. Its plan covers both commands and retrieves the actual pre-existing failure receipt. However, the request and RepositoryContract require/declare both `unit` and `compile`, while its selected acceptance list contains only `unit`. Both the production worker and evaluator reject `compile` under that narrower selection. Six tool calls; 53.987 seconds; 23,850 input and 2,626 completion tokens.

Total measured model work: 107.823 seconds, 42,261 input and 5,148 completion tokens. Dollar cost is unavailable. No execution or repair limit changed. The wrapper also carries the separately identified diagnostic completeness-label defect; its full-suite `infrastructure_valid: false` is not evidence of a hardware abort.

## Cause and bounded correction

The [complete twelve-case audit](ASTRA-M04-E508F43-PLANNER-CONTRACT-AUDIT.json) finds eight inherited single-command selections despite requests requiring both commands: cross-repository context, failing baseline, ambiguous intent, immutable source, undeclared path, documentation boundary, stale context and path intersection. The other four already select both. All twelve declare and execute both commands in their repository baseline.

Correct those eight public selections to match the unchanged requests and executable contracts. Preserve all twelve scenarios, source files, baseline receipts, path boundaries, prompts, model settings and execution limits. Bump only Planner to `stage-qualification-v2.4`; leave the other suites and frozen coding/repair tasks unchanged. The worker's rejection of unselected commands is appropriate and must remain. Prove the correction against the original submission in both evaluator and worker tests, along with full fixture and adversarial checks. A passing local structured check does not rewrite the historical run or establish that its prose is an acceptable autonomous plan.

No Planner control should be dispatched against this known-invalid case. After a compatible immutable release, use a new primary and same-runtime control reference under the new suite hash. Independently grounded Diagnosis and Verifier canaries can continue on sourcee508 while their native contracts remain valid.

## Judgment

This is another measurement defect, and it belongs to PHarness. Spending more model calls on it would not tell us which model is better. The lesson is to validate the full public request, selected acceptance and executable contract together rather than testing a scorer with answers constructed from its own expected fields.

The plan's proposed separate `/legacy` fix also deserves scrutiny: it records the unresolved scope choice but recommends a change beyond the requested endpoint behavior. That is a qualitative concern, not a second proven native failure or a reason to invent a language-based grader. The connected-loop gate must demonstrate how unresolved scope reaches intervention before source changes. No broad prompt expansion, extra backend or budget increase is justified here.

See [M04](../../programs/autonomous-sdlc/ASTRA-04-CODING-RELIABILITY-QUALIFICATION.md) and the [master program](../../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md). The two Finance acceptance changes, production approvals and operational closeout remain pending.
