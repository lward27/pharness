# ASTRA M04: First passing onboarding diagnostic

Recorded 2026-09-08. Compiled source `a2d05f1b41c15cb34a4543327ba468b9da3b5094`; release pin `0f14597e11c437e007757365953f7b411e322252`. The [release](ASTRA-M04-WRITABLE-SCOPE-RELEASE.md) and one live `python-contract` diagnostic pass. **M04 remains open and the runtime remains unqualified.**

## Objective evidence

The [single dispatched evaluation](ASTRA-M04-A2D05F1-ONBOARDING-START.json), `infeval_01a07e7783717723aeb38a529472d1cf`, completed with [native diagnostic.passed true](ASTRA-M04-A2D05F1-ONBOARDING-RESULT.json). MiniMax submitted `src/**`, `tests/**`, and `README.md` in one tool call and one attempt, without correction, policy violations or context-budget failure. Model execution took 21.497 seconds and reported 4,721 input and 3,520 completion tokens. The [analysis](ASTRA-M04-A2D05F1-ONBOARDING-ANALYSIS.json) links the actual source and workspace identities.

The [renewed protocol calibration](ASTRA-M04-A2D05F1-PROTOCOL-VERIFICATION.json) passed 30/30 before dispatch; its prior expiry was recorded [before renewal](ASTRA-M04-A2D05F1-VERIFICATION-FRESHNESS.json). The policy, target, suite, execution budget and case remain the same as source 81. The new recorded prompt/tool hashes match the [compiled production constructors](ASTRA-M04-WRITABLE-SCOPE-CONTRACT.json). Historical source-92 and source-81 failures remain failed.

The native `candidate_safe`, `gate_passed` and `infrastructure_valid` fields remain false, and qualification is null. Only one of the full stage's twelve fixtures ran. These fields are preserved: the diagnostic pass cannot satisfy the full-suite gate or activate a policy.

The [initial Job receipt](ASTRA-M04-A2D05F1-EVALUATION-JOB.json) captures requested image and Job identity. At the [completion follow-up](ASTRA-M04-A2D05F1-EVALUATION-COMPLETION-OBSERVATION.json), Kubernetes no longer retained that Job or Pod. The native terminal report is durable; a final container-exit/image-ID receipt was not saved before expiry and is not reconstructed or claimed. Capture that receipt promptly for subsequent evaluations.

## Judgment and next boundary

The passing submission supports the shared scope correction. One case cannot establish general onboarding quality, compare models, or qualify the connected engineering workflow.

Inspection before the planned common-input control found a reader mismatch: native evaluator results serialize fixture identity as `source_sha`, while API `retained_inputs` requires `base_sha`. The saved result contains the former and no latter. The API's synthetic test fixture uses the reader's expected name, hiding the mismatch. No paid control was dispatched into this known rejection.

Next, repair the reference reader to consume the native report's source identity and test the boundary against this actual retained report, keeping hashes, source checks, immutable history and budget equality intact. Release the correction under the existing immutable procedure. Its new runtime requires a fresh primary diagnostic before the exact-runtime control; the existing source-a2 result remains historical evidence. Then continue the remaining bounded M04D cases, M04E connected coding/repair, and M04F frozen qualification. No Finance production approval or local-model activation is implied.
