# ASTRA M04: Builder qualification on 48c77b7

Status: Builder qualification passed; M04 remains open. Observed 2026-09-05.

Both independent frozen runs passed **24/24 on the first pass**. Rust, Python and Node each passed 8/8 in both runs. All 48 cases passed their visible acceptance checks, hidden checks and protected-path checks. No policy violations, hidden-test false passes or provider failures were reported. The registry, suite and runtime bindings were retained; thresholds and execution limits were not lowered.

| Run | First pass | Passing after at most one correction | Rust / Python / Node | Corrections used |
| --- | --- | --- | --- | --- |
| 1 | 24/24 | 24/24 | 8/8 each | 0 |
| 2 | 24/24 | 24/24 | 8/8 each | 0 |

The result satisfies the program's numeric Builder thresholds without using correction. It does not claim that an actual failed Builder workspace was repaired. The separate `repair-v2` qualification and the stage-specific qualifications remain required.

## Reproducible identity

- Runtime: `48c77b7b4438d621ff9563b913857bcf771f1800`.
- Evaluation: `infeval_01a0725f855b7c038234cd6af3830594`.
- Qualification: `inferqual_01a0727ed8c87f439779e994d72ad105`, verdict `passed`.
- Policy: `builder-kimi-k2p7-code-v2@v1`; target: `fireworks-kimi-k2p7-code@v1`.
- Model: `accounts/fireworks/models/kimi-k2p7-code`, through the existing gateway.
- Prompt bundle: `2026-09-05.1`; frozen fixtures: `coding-reliability-v2.1`.
- Suite hash: `sha256:4bf3fce21f86369794ac6e57816436ff331e7dd607eb303baaf720c885583767`.
- Report hash: `sha256:adcb5593be29eaa209008efcf63aca28fbcd2eeeebeb375f7f718bb54146c814`.

The [stored qualification and full binding](ASTRA-M04-48C77B7-BUILDER-PROFILE-PIN.json), [raw report](ASTRA-M04-48C77B7-CODING-QUALIFICATION-RESULT.json), [independent threshold analysis](ASTRA-M04-48C77B7-BUILDER-ANALYSIS.json), and [30/30 protocol calibration](ASTRA-M04-48C77B7-GATEWAY-PREFLIGHT.json) retain the evidence.

## Usage and interpretation

The API evaluation ran from 16:20:58 to 16:55:10 UTC, including queue/startup time. Reported active case duration totals 1,700,269 ms. Usage totals 3,773,503 prompt tokens, 116,514 completion tokens, 785,214 cached tokens, 61,968 reasoning tokens, 624 turns and 51 recoverable failures. These fields retain the provider's accounting categories; they are not all additive billable categories and no monetary estimate is asserted.

The result is a substantial improvement over the [earlier failed candidate](ASTRA-M04-FD740-QUALIFICATION-FAILURE-ANALYSIS.md). It is still a finite maintenance benchmark, not proof of unattended Finance delivery. Two successful runs do not erase the earlier failures or establish reliability outside this tested scope.

## Remaining gates and release constraint

Qualification Jobs continue serially for Onboarding, Planner, Test diagnosis, Verifier and Repair. M04 is not accepted until its required suites pass. Hosted creation and Coding Reliability V2 remain disabled in the live release.

The current creation check binds qualification to the exact API revision. A subsequent PHarness runtime must acquire its own matching qualification before new hosted work can be enabled. This result remains evidence for `48c77b7`; it is not silently transferred to a later source/build revision. M11 still requires two real autonomous application changes and human production approvals.
