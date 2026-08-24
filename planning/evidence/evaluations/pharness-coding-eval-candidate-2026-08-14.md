# Pharness Coding Evaluation Candidate Observation

Date: 2026-08-14

## Preliminary live result

`target/pharness-evals/candidate.json` recorded a two-attempt Fireworks run
using `accounts/fireworks/models/kimi-k2p6`, prompt version `2026-08-14.1`,
temperature `0.1`, 4,096 maximum output tokens, and 24 maximum turns.

- 16 fixture attempts completed.
- 12 were reported as passing.
- No policy, protected-path, secret-access, network, or scope violations were
  recorded.
- No context-budget failures were recorded.

## Evaluation correction

The four reported failures were two repeated fixtures:

- `multi-file-rust` used `value.0 * 1000` rather than the evaluator's
  formatting-specific `value.0 * 1_000` check.
- `large-file-navigation` used `values.iter().sum::<u32>()` rather than the
  evaluator's formatting-specific inference check.

The preserved workspaces contain correct implementations and their independent
`cargo test --offline --quiet` checks pass. The evaluator was corrected to use
the independent Rust test result for those semantic assertions, while retaining
the large-file shape check.

This preliminary JSON is therefore not eligible for the release comparison.
Run the candidate again after the evaluator correction, then compare it only
with an approved pre-change baseline using the same fixture revision and model
configuration. No `app.rs` extraction is authorized from this observation.

## Follow-up correction

The rerun under fixture revision `coding-v1.2` reported 15/16 successes with
the same zero safety and context-budget failures. Its only reported failure,
`new-module` attempt 2, correctly used a private `mod greeting` plus
`pub use greeting::greet`; the integration test imports `greet` from the crate
root successfully. The evaluator had incorrectly required `pub mod greeting`.

The acceptance check now accepts either module visibility compatible with the
required public re-export, and the fixture revision is `coding-v1.3`. The
`coding-v1.2` candidate JSON is not eligible for comparison.

## Superseded candidate observation

`target/pharness-evals/candidate-v1.3.json` records all 16 fixture attempts as
passing with `accounts/fireworks/models/kimi-k2p6`, prompt version
`2026-08-14.1`, temperature `0.1`, 4,096 maximum output tokens, and 24 maximum
turns. It records zero policy, protected-path, secret-access, network, scope,
or context-budget failures.

The completion audit found that the large-file fixture was only roughly 15 KiB,
below the pre-recovery 256 KiB native read limit. It has been strengthened to
9,000 filler lines (more than 256 KiB). The evaluator now also rejects changes
outside each fixture's explicit allowed paths, while ignoring test-generated
`target/` and `Cargo.lock` artifacts. The fixture revision is now `coding-v1.5`.
The v1.3 result is not eligible for the release comparison. Rerun both the
candidate and the historical control under v1.5 before evaluating the gate.

## Matched v1.5 release-gate result

The candidate and the historical control were both run with fixture revision
`coding-v1.5`, the same Fireworks Kimi K2.6 model, prompt version, temperature,
token limit, turn limit, and two attempts per fixture. Both reports contain 16
completed, passing attempts with zero recorded safety violations and zero
context-budget failures.

```
baseline_passes: 16
candidate_passes: 16
additional_passes: 0
baseline_context_failures: 0
candidate_context_failures: 0
candidate_safe: true
gate_passed: false
```

This is a valid, negative result: the strengthened v1.5 suite does not yet
differentiate the recovered runtime from the historical control for this model.
It does **not** establish the required four additional candidate passes, so the
recovery milestone is not released and the planned `app.rs` run-lifecycle
extraction must remain deferred.

The next reliability iteration should add deterministic, capability-targeted
fixtures that make the historical control exercise the intended boundaries:

- a patch failure that requires a bounded retry with a fresh file read;
- a command timeout or non-zero exit that must be surfaced to the model before
  the subsequent corrective action;
- a repository instruction that must be loaded within its bounded budget;
- a mandatory-context overflow that must fail before the provider is called;
- a large-file task whose relevant change lies beyond the legacy read window
  and requires a ranged follow-up read.

Those fixtures need local, deterministic pass/fail assertions and a fresh
matched baseline before making any threshold decision. Do not weaken the
four-pass threshold or retroactively reinterpret this 16/16 baseline as a
release pass.

## v1.6 recovery-targeted rerun

Fixture revision `coding-v1.6` keeps the same eight workspaces, model settings,
scope checks, and two-attempt protocol. The first four Rust fixtures now require
the agent to use `read_file` on a deliberately absent prior-run record before
editing. A run is accepted only when Pharness records that expected recoverable
executor failure and the agent then finishes the repair safely. The historical
runtime terminates on that executor error; the candidate runtime is intended to
return the structured failure to the model so it can continue.

This makes the live comparison test the actual recovery boundary, rather than
assuming an ordinary code-editing success demonstrates recovery. The v1.5
reports are retained as evidence, but are ineligible for the v1.6 gate.

## Matched v1.6 release-gate result

The v1.6 Fireworks reports are matched on suite, fixture revision, provider,
model, prompt version, temperature, output-token cap, turn cap, and two attempts
per fixture. The historical runtime passed 8 of 16 attempts. It terminated on
all eight required missing-file executor failures before making a workspace
change. The candidate passed all 16 attempts; each of the eight targeted runs
recorded one recoverable failure and then completed the requested repair.

```
baseline_passes: 8
candidate_passes: 16
additional_passes: 8
baseline_context_failures: 0
candidate_context_failures: 0
candidate_safe: true
gate_passed: true
```

All candidate changed paths stayed within their fixture allowlists, every
acceptance check passed, and no safety or context-budget violation was recorded.
This exceeds the required four-pass improvement without weakening the threshold.

The passing gate authorized the deferred API refactor. Public run routes,
internal worker run transitions, run creation/list/summary/lookup, event and SSE
delivery, diffs, artifacts, run observations, cancellation, and the run-specific
approval entry point now live in `crates/pharness-api/src/app/runs.rs`. Shared
operator grouping, audit, and approval machinery remains in `app.rs` where it is
also used by non-run resources.
