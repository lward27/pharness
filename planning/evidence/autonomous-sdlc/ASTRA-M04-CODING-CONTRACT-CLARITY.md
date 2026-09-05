# ASTRA M04: Coding contract clarification

Status: implemented and locally checked; publication and live qualification open.
Base: main `2249950d225a4632b24235c2b6f2d8469a774243`. This responds to the
failed fd74092 evaluation without altering or regrading that result.

Builder and Repair instructions now explicitly require checking established
invalid-input return values, exceptions, and response shapes. They name the
runner's existing inline-program restriction and direct focused checks into
repository test modules or declared scripts. Tests remain subject to the original
writable paths; useful regression coverage should remain in the patch instead of
being implemented as disposable validation/cleanup helpers.

The command tool description now states those existing restrictions. The actual
command parser, policy, executable list, tool access, budgets, frozen tasks, hidden
tests, model, provider, and acceptance thresholds are unchanged. No fixture-specific
answer or function name was added. The prompt bundle advances to `2026-09-05.1`,
so existing prompt/profile evidence cannot qualify the changed instructions.

All 28 existing runhost tests passed. Runhost Clippy passed for all targets with
warnings denied; formatting and diff checks passed. The evaluator has no diff.
[Validation record](ASTRA-M04-CODING-CONTRACT-CLARITY.json) pins source and log hashes.
These checks validate the unchanged tool boundary, not improved model performance.

The compatible-reader release at 2249950 is being completed independently from
this patch. Hold this patch's source merge until that build finishes its exact-main
revision checks, then publish one complete immutable image set and native bundle
for the new merged source. Refresh each required target's calibration and run the
unchanged qualification gates serially. Keep hosted creation and V2 disabled until
their respective acceptance requirements are satisfied.
