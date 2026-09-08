# ASTRA M04: Release the Planner readiness boundary

Status: the immutable release, exact live identities and full ten-minute service observation are verified. Both fresh Planner diagnostics are terminal and pass 2/2 on identical valid inputs. This is not M04 qualification or Finance acceptance.

## Source and implementation

Compiled source: `291e007cedcb2440a328f5e24710fb3423cb0ded`. [Release PR #398](https://github.com/lward27/pharness/pull/398) merged as `90ab524aeef4fa6692057a6f89526674de3a8fa2` on 2026-09-08 at 21:00:53 UTC. The source includes [Planner decision PR #395](https://github.com/lward27/pharness/pull/395) and [evaluator layer reuse PR #394](https://github.com/lward27/pharness/pull/394).

Planner submissions now distinguish a settled plan from a plan requiring a decision. Native sealing preserves unresolved blockers and automatic approval checks the exact sealed plan. The [implementation evidence](ASTRA-M04-PLANNER-DECISION-BOUNDARY.md) records 590 distinct component checks and the saved-contract compatibility boundary. Planner qualification advances to v2.6; the twelve scenario inputs, commands and acceptance requirements are preserved. Frozen coding/repair tasks, model defaults and budgets are unchanged.

The separately validated [repair authorization PR #397](https://github.com/lward27/pharness/pull/397) is not included in this release. It is merged as `67f5303`; include it when the connected-loop candidate is frozen. A passing release observation cannot close that handoff proof or the full qualification gate.

## Objective release evidence

- [Uncached AMD64 preflight](ASTRA-PLANNER-GUARD-AMD64-PREFLIGHT.json) uses Rancher Desktop and the explicitly selected `pharness-mac` builder; lucas-desktop remains off.
- [One full build](ASTRA-PLANNER-GUARD-BUILD-OPERATION.json) produced [all seven images and the native bundle](ASTRA-PLANNER-GUARD-RELEASE-BUILD.json) from the same merged source in 2,065.052 seconds. All thirteen native-bundle checksum entries, its source revision and absence of macOS metadata files were checked.
- [Registry verification](ASTRA-PLANNER-GUARD-ARTIFACT-VERIFICATION.json) confirms all manifest/config digests, Linux AMD64 platforms and source labels. The native archive digest is `sha256:46f76b17cb8a4d6ff261fca8bc873c858cdec230fdf71d8feb1451d1135531f7`. This is not an SBOM, signature or verified provenance attestation.
- [The reviewed pin diff](ASTRA-PLANNER-GUARD-PIN-DIFF-REVIEW.json) changes only image/revision pins and derived native-execution policy hashes. [Strict Helm and server dry-runs](ASTRA-PLANNER-GUARD-RELEASE-PREFLIGHT.json) pass for all 53 resources from the actual Argo overlay: 41 in pharness, two cluster-scoped, two apps-prod, four argocd and four tekton-pipelines.
- [The five-minute baseline](ASTRA-PLANNER-GUARD-BASELINE-SERVICE-WINDOW-R2.json) passes with eleven samples. [Live identities](ASTRA-PLANNER-GUARD-RELEASE-OBSERVED.json) verify the exact release commit, all five serving deployments, three Service endpoint sets, API/UI revisions and configured worker/evaluator/language-runner images. Argo auto-sync performed the rollout; no manual sync or restart was issued.
- [Post-readiness metrics](ASTRA-PLANNER-GUARD-WINDOW-START-READINESS.json) are fresh before the full observation clock begins. [The full ten-minute observation](ASTRA-PLANNER-GUARD-RELEASED-SERVICE-WINDOW-R2.json) passes with 21 samples, fresh availability metrics, zero restarts, delivered API/UI logs and no recorded severe-log signal or current-Pod warnings. Its checks are internal PHarness service checks, not Finance acceptance.

## Build performance and judgment

[Real release measurements](ASTRA-PLANNER-GUARD-BUILD-PERFORMANCE.json) show all twelve non-base evaluator toolchain steps were cache hits. The seven completed push phases total 1,184.6 seconds; the log contains no TLS-handshake timeout warning. Stable toolchain reuse is demonstrated. This one build cannot isolate its timing benefit from network variance and does not establish that the preceding intermittent registry transport issue is permanently fixed.

## Compatibility and authority

[Compatibility evidence](ASTRA-PLANNER-GUARD-COMPATIBILITY.json) records no changed SQL migrations, preserved Finance generation `dbgen_finance_20260827`, and 82 ordinary Runs. Gateway registry hash remains `sha256:e05f943fcb3a870c4a3a145ab3b06849a36e8a3c13d262cdd28e49394719bd79`. Hosted creation and Coding Reliability V2 remain disabled. No local-model endpoint or policy default was activated; no Finance production approval or deployment was performed.

Preferred rollback runtime is `d675159cb205d222616d253d311706be1889bb91`; the schema-compatible reader floor remains `92f8f1b8e98dd45d0a01e030aeb99ef9bcf95267` or a later compatible reader. Pause new Planner work/activation before rollback because the preceding runtime lacks this new readiness guard. Preserve original saved contracts and in-flight history; data compatibility alone does not grant safe execution of a new contract under older code.

## Remaining acceptance

The full service window and both exact-policy 30/30 prerequisites are complete. [The declared Planner pair](ASTRA-M04-291E007-PLANNER-COMPARISON.md) passes 2/2 for both runs: the ready case remains ready and the unresolved baseline remains blocked. The record includes independent inspection of the model prose and its remaining caveats. [Closeout](ASTRA-M04-291E007-CANARY-CLOSEOUT.json) confirms no active evaluations, unchanged source/configuration and 82 ordinary Runs. These two cases are diagnostics, never qualification. M04E still needs the connected failure/repair handoff and independently scoped verification; M04F still needs the full frozen acceptance matrix on its final runtime. M11 Finance and M12 operational acceptance remain open.
