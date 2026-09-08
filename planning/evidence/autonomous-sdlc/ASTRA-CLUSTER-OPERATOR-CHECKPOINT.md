# ASTRA: Cluster operator checkpoint

Updated 2026-09-08. This is the current resume point; the [master program](../../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md) owns acceptance and authority. M04 remains open. No Finance production approval or local-model activation has occurred.

## Current state

- **Deployed and observed:** source `d675159cb205d222616d253d311706be1889bb91`, release pin `ce98a26065cd48323b58b8905fcbbfc6c76452cd`. [Seven images, native bundle, exact live identities and the full ten-minute window](ASTRA-M04-POLICY-PROTOCOL-RELEASE.md) pass. The release includes the audited semantic fixtures and exact-policy protocol gate. Sourcee508 is the preceding verified deployment.
- **Next:** release the merged [Planner decision boundary PR395](https://github.com/lward27/pharness/pull/395) and [evaluator layer-reuse PR394](https://github.com/lward27/pharness/pull/394), freeze one source, release immutably and rerun the affected ready/decision-required Planner checks. All six declared d675159 semantic evaluations are terminal; do not repeat them on the old runtime to obtain different scores.
- **Data and activation:** schema55, Finance generation `dbgen_finance_20260827`, 82 Runs. Hosted creation and Coding Reliability V2 stay disabled. The compatible-reader floor remains source `92f8f1b` or a later compatible reader. Older protocol implementations also lack the new exact-policy gate, so a rollback must suspend qualification/activation until that protection is restored.
- **Configuration:** inference registry `sha256:e05f943fcb3a870c4a3a145ab3b06849a36e8a3c13d262cdd28e49394719bd79`; existing models, defaults and execution limits are unchanged. Windows Minisforum setup is pending an exact endpoint/model; no local target is enabled.
- **Builder:** the [Mac/Rancher Desktop Tekton route](ASTRA-M02-MAC-BUILDKIT-RETURN.md) is selected while lucas-desktop is off. Keep its active forwarding process running. Use the explicit `pharness-mac` builder and verify uncached AMD64 execution.

## Next eligible work

1. Preserve the three completed diagnostic pairs below. No declared d675159 semantic operation remains active. The last evaluation was Diagnosis control `infeval_01a082a6774176c1a9d61344c1b84aa0`.
2. Planner PR395 is merged as `7b2c06daf4b549fa8acd62a363bd4aedb758fc2f`; layer-reuse PR394 is merged as `fda1174b57dfcf29ccc5ec2f6011bfe30c49e20e`. The [Planner correction and 590 component checks](ASTRA-M04-PLANNER-DECISION-BOUNDARY.md) are committed. Merge this evidence checkpoint, then freeze that main source before building. Keep model defaults, limits, database schema and hosted activation unchanged. Release all seven images plus the native bundle and complete the exact identity/service-window checks.
3. On the new runtime, recheck the exact Planner policies and run the affected ready/failing-baseline pair on v2.6. A typed readiness claim needs live and connected-loop proof; local tests do not close that gate.
4. M04E's connected coding/repair handoff and M04F's two complete frozen qualifying runs remain open. Keep [M08/M09 drafts](ASTRA-M08-5A40945-INTEGRATION.md) independent; neither is accepted delivery.

The prepared requests and exclusive operation records are under `/Users/wardl/Personal/apps/pharness-release-artifacts/d675159cb205d222616d253d311706be1889bb91`. Reconcile an existing operation ID after uncertainty; never repeat a POST to discover whether it worked. Current source/runtime and suite identities must match before dispatch.

## Evidence that constrains the next decision

| Boundary | Retained result | Consequence |
| --- | --- | --- |
| Planner on d675159 | [Native2/2 versus2/2, identical valid inputs](ASTRA-M04-D675159-PLANNER-COMPARISON.md) | Same-model repeatability only. Control recommends weakening a regression; explicit readiness and connected-loop protection are still required. |
| Verifier on d675159 | [GLM2/3 versus Kimi3/3](ASTRA-M04-D675159-VERIFIER-COMPARISON.md) | Malformed GLM terminal submission; Kimi is a stronger next qualification candidate in this small sample, not an activated default. |
| Diagnosis on d675159 | [Nemotron2/2 versus Kimi2/2](ASTRA-M04-D675159-DIAGNOSIS-COMPARISON.md) | Both correctly classify failure and refrain from repair on a pass. Prior protocol timeout did not recur. |
| Onboarding on e508 | [Primary1/2; same-input control2/2](ASTRA-M04-E508F43-ONBOARDING-COMPARISON.md) | Diagnostic model difference, not new-runtime qualification or permission to switch defaults. |
| Planner on e508 | [Native1/2, mismatched request/selection](ASTRA-M04-E508F43-PLANNER-ANALYSIS.md) | Original result retained; old-suite control undispatched. Use revised v2.5. |
| Verifier on e508 | [Native3/3, invalid semantic/control fixtures](ASTRA-M04-E508F43-VERIFIER-ANALYSIS.md) | Original result retained; old-suite control undispatched. Use revised v2.4. |
| Diagnosis on e508 | [Protocol response timeout](ASTRA-M04-E508F43-DIAGNOSIS-PREFLIGHT.md) | Historical timeout preserved. Fresh d675159 protocol and semantic checks pass within unchanged limits. |
| Shared-target protocol | [Wrong-policy and stale/cross-policy admission](ASTRA-M04-POLICY-BOUND-PROTOCOL.md) | New gate requires current-runtime exact-policy receipts; historical unbound passes stay history. |
| Independent delivery | [M08 refresh](ASTRA-M08-5A40945-INTEGRATION.md), [M09 refresh](ASTRA-M09-5A40945-INTEGRATION.md) | Drafts363/366 are validated locally, not merged, deployed or accepted. |

Older source38/source80/source92 failures and repairs are indexed in the [execution history](../../programs/autonomous-sdlc/ASTRA-PROGRAM-EXECUTION-HISTORY.md). Their original native reports remain unchanged.

## Maintained operator interface

Use `~/.local/bin/lucas-ops` with the [Lucas Engineering operator guide](https://github.com/lward27/lucas_engineering/blob/main/docs/operations/ASTRA-CLUSTER-DEVELOPMENT.md). Resolve the `lucas_engineering` profile/context before cluster actions. Keep tokens in the named cluster Secrets; do not copy them into commands, logs or evidence.

Installed content hash: `f71aa9220801f723be0a6e8f21e19fc8d6146d65a81d604ea8f0d6f8e47ea8d5`. [PR58](https://github.com/lward27/lucas_engineering/pull/58) introduced the maintained CLI/skills; [PR62](https://github.com/lward27/lucas_engineering/pull/62) updated Mac builder inspection; [PR64](https://github.com/lward27/lucas_engineering/pull/64) added final evaluation Job/Pod image/exit receipts. Thirty-five deterministic checks and the [live receipt acceptance](ASTRA-M04-EVALUATION-RECEIPT-ACCEPTANCE.md) pass. An expired Job produces missing evidence, never a replacement model run.

The 91 retired temporary helpers remain privately archived at `/Users/wardl/.local/state/lucas-ops/legacy/20260906T203053Z`; use their [inventory](https://github.com/lward27/lucas_engineering/blob/main/docs/operations/ASTRA-LEGACY-HELPER-ARCHIVE.json) only for historical forensics. No evidence or credential files were erased.

## Separate infrastructure scope

The [GitOps retirement record](https://github.com/lward27/lucas_engineering/blob/main/docs/operations/ASTRA-APPLICATION-RETIREMENT.md) owns the separately authorized application/data cleanup. PHarness, Finance, Tekton, LGTM, registry and shared databases remain outside it. LEA retirement has not been authorized. Failed diagnostic Pod [evidence is preserved](ASTRA-FAILED-DIAGNOSTIC-POD-PRESERVATION.json); this implementation continuation performs no broad Pod, Job or PVC cleanup.
