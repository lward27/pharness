# ASTRA: Cluster operator checkpoint

Updated 2026-09-08. This is the current resume point; the [master program](../../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md) owns acceptance and authority. M04 remains open. No Finance production approval or local-model activation has occurred.

## Current state

- **Deployed and observed:** source `291e007cedcb2440a328f5e24710fb3423cb0ded`, release pin `90ab524aeef4fa6692057a6f89526674de3a8fa2`. [Seven images, native bundle, exact live identities and the full ten-minute window](ASTRA-M04-PLANNER-READINESS-RELEASE.md) pass. The release includes Planner readiness and stable evaluator toolchain layers. Source d675159 is the preceding verified deployment.
- **Next:** finish source291e007 Planner v2.6 protocol and two-case primary/control diagnostics. Then integrate the locally validated [repair authorization PR397](https://github.com/lward27/pharness/pull/397) and implement the connected loop before freezing a qualification candidate. All six declared d675159 semantic evaluations are terminal; their original scores remain unchanged.
- **Data and activation:** schema55, Finance generation `dbgen_finance_20260827`, 82 Runs. Hosted creation and Coding Reliability V2 stay disabled. The compatible-reader floor remains source `92f8f1b` or a later compatible reader. Older releases lack the new Planner readiness guard, so pause new Planner/development work and qualification/activation before rollback. Data compatibility alone does not establish compatible execution of the new contract.
- **Configuration:** inference registry `sha256:e05f943fcb3a870c4a3a145ab3b06849a36e8a3c13d262cdd28e49394719bd79`; existing models, defaults and execution limits are unchanged. Windows Minisforum setup is pending an exact endpoint/model; no local target is enabled.
- **Builder:** the [Mac/Rancher Desktop Tekton route](ASTRA-M02-MAC-BUILDKIT-RETURN.md) is selected while lucas-desktop is off. Keep its active forwarding process running. Use the explicit `pharness-mac` builder and verify uncached AMD64 execution.

## Next eligible work

1. Preserve the six terminal d675159 evaluations and their complete evidence. The new source does not rescore their native reports.
2. Source291e007 includes Planner PR395 and layer-reuse PR394. Release PR398 merged at90ab524; all seven images, the native bundle, exact live identities, the five-minute baseline and the full ten-minute service observation pass. No hosted work, model default or execution limit was activated.
3. The new Planner primary protocol operation is recorded as `ASTRA-M04-291E007-PLAN-PRIMARY-PROTOCOL.json`. Finish its exact-policy current-runtime prerequisite, then the declared `acceptance-boundary` / `failing-baseline` pair; inspect retained public inputs and submitted blockers before a matched control. Reconcile every uncertain dispatch by its saved operation/evaluation ID.
4. PR397 is locally validated and ready for integration, but not included in source291e007. M04E's actual coding/failure/diagnosis/repair/Test/Verifier handoff and M04F's two complete frozen qualifying runs remain open. [M08/M09 drafts](ASTRA-M08-5A40945-INTEGRATION.md) remain independent and unaccepted.

Prepared requests and exclusive operation records for the new release are under `/Users/wardl/Personal/apps/pharness-release-artifacts/291e007cedcb2440a328f5e24710fb3423cb0ded`. The preceding source's records stay in its separate d675159 directory. Current source/runtime and suite identities must match before dispatch.

## Evidence that constrains the next decision

| Boundary | Retained result | Consequence |
| --- | --- | --- |
| Planner on d675159 | [Native2/2 versus2/2, identical valid inputs](ASTRA-M04-D675159-PLANNER-COMPARISON.md) | Same-model repeatability only. Control recommends weakening a regression; explicit readiness and connected-loop protection are still required. |
| Verifier on d675159 | [GLM2/3 versus Kimi3/3](ASTRA-M04-D675159-VERIFIER-COMPARISON.md) | Malformed GLM terminal submission; Kimi is a stronger next qualification candidate in this small sample, not an activated default. |
| Diagnosis on d675159 | [Nemotron2/2 versus Kimi2/2](ASTRA-M04-D675159-DIAGNOSIS-COMPARISON.md) | Both correctly classify failure and refrain from repair on a pass. Prior protocol timeout did not recur. |
| Onboarding on e508 | [Primary1/2; same-input control2/2](ASTRA-M04-E508F43-ONBOARDING-COMPARISON.md) | Diagnostic model difference, not new-runtime qualification or permission to switch defaults. |
| Planner on e508 | [Native1/2, mismatched request/selection](ASTRA-M04-E508F43-PLANNER-ANALYSIS.md) | Original result retained; old-suite control undispatched. Use current v2.6; v2.5 remains historical. |
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
