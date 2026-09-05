# ASTRA M04: Verifier qualification failed on runtime 48c77b7

Status: failed qualification, retained unchanged. M04 is not accepted.

The serial live run ended at **2026-09-05 20:14:12 UTC**, after 96 minutes
37 seconds. It used `verifier-glm-5p3-v2` / `fireworks-glm-5p3`, model
`accounts/fireworks/models/glm-5p3`, on immutable runtime
`48c77b7b4438d621ff9563b913857bcf771f1800`. All 30 protocol calibration cases passed
before the two frozen Verifier attempts were dispatched.

Evaluation: `infeval_01a072dc9abd71228667105507171ced`.
Qualification: `inferqual_01a073350fbe74639f9da86106fdd25f`.
Job: `pharness-inference-eval-71818fb6cbc3`.

## Objective result

| Measure | Observed |
| --- | --- |
| First attempt | 1/24 passed |
| Second attempt | 2/24 passed |
| Gate / candidate-safe | false / false |
| Infrastructure-valid | true |
| Completed cases | 46/48 |
| Typed verification submissions | 46/48 |
| Evidence/marker mismatches | 43 |
| Missing submissions | 2: one idle provider stream; one MissingAction protocol failure |
| Reported false approvals / false rejections | 0 / 0 |
| Changed-workspace cases | 0 |

The passing cases were `misleading-documentation` in attempt one, and
`wrong-query-parameter` and `unapproved-context-head` in attempt two. No other case
passed. A completed evaluation, healthy Job infrastructure, or correct decision
without the required evidence binding is not a qualification pass.

Aggregate reported usage: 1,122,220 prompt tokens; 242,578 completion tokens;
330,782 cached tokens; 174,372 reasoning tokens; 377 turns; 321 tool calls; one
recoverable failure. These measurements do not authorize larger limits.

## What the evidence does and does not establish

The scorer combines two checks under `verification_evidence_mismatch`: a reference
to `fixture_evidence`, and the required controller marker appearing in the submitted
document. It checks the decision separately. The retained runtime-48 report does
not include the submitted documents or separate those two mismatch causes. It
would be speculation to blame all 43 failures on either missing citations or
literal marker spelling. Do not retrospectively turn them into passes.

There is also a limit in the current fixture design: the supplied task/evidence
discloses the expected decision and marker. This tests protocol and evidence
binding; it does not independently prove that the Verifier can discover an
application defect. Zero reported false approvals on these fixtures must not be
presented as zero false approvals on real product changes. The M11 application
acceptance proof remains necessary.

The immediate follow-up is to retain separate evidence-reference/marker diagnostics
on future runs and inspect actual submissions before proposing a prompt or scorer
repair. Any justified fixture/contract change needs an explicit version and fresh
qualification. No model switch, threshold reduction or budget increase is justified
by this result. This evidence-only change makes none of those changes.

## Continued execution

After Verifier reached its terminal result, Repair passed its own 30/30 protocol
calibration and started two frozen attempts under the existing `repair-kimi-k3-v2`
policy. Evaluation `infeval_01a0733b942376b19976dac13414a52a` runs in
`pharness-inference-eval-4fe69a798c67`. Its result is pending. The seeded Repair suite
still does not replace proof of correction against an actual failed Builder
workspace.

Hosted creation remains disabled. Independent M08 implementation continues; neither
it nor the passed Builder run closes the outstanding qualification gates.

## Evidence

[Raw immutable result](ASTRA-M04-48C77B7-VERIFIER-LIVE-RESULT.json) and
[derived analysis](ASTRA-M04-48C77B7-VERIFIER-ANALYSIS.json) retain identities,
per-case outcomes, usage and limits. Raw report hash:
`sha256:3c60de16898480abeac83377c5c37635241543c714b61c6d30dd5a9b1e33d314`.

[Repair protocol](ASTRA-M04-48C77B7-REPAIR-LIVE-PROTOCOL.json) and
[Repair dispatch](ASTRA-M04-48C77B7-REPAIR-LIVE-START.json) identify the subsequent
serial run. No application source or deployment was changed by these evaluations.
