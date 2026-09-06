# ASTRA M04 diagnostic release: historical progress snapshot

Superseded by [the compatible diagnostic release](ASTRA-M04-DIAGNOSTIC-COMPATIBLE-RELEASE.md). The observations and remaining work below describe this earlier snapshot.

Updated: 2026-09-06T13:11:34.434026+00:00. Source: `92f8f1b8e98dd45d0a01e030aeb99ef9bcf95267` (PR #371).

The approved M04A–C changes and bounded M04D diagnostic path are merged. M04D implementation
passed 627 component tests, Clippy, formatting and five architecture checks. Its
[local validation](ASTRA-M04-DIAGNOSTIC-CANARIES-VALIDATION.json) does not qualify models.

The runtime image is [built and verified](ASTRA-M04-DIAGNOSTIC-RELEASE-PHARNESS-RUNTIME.json).
The existing private TLS registry route verified the manifest, image configuration,
source revision and Linux AMD64 platform after the public Cloudflare endpoint
returned 403. No rebuild, credential change or disabled TLS verification was used.
[Registry evidence](ASTRA-M04-DIAGNOSTIC-REGISTRY-READ-RECOVERY.json) retains that distinction.

The [real API migration on the current database copy](ASTRA-M04-DIAGNOSTIC-CLONE-MIGRATION-VERIFIED.json)
passed: schema 55, integrity/foreign-key checks passed, and every original column
in all 81 tables is identical. The source snapshot remains schema 54. The observer's
first console summary accidentally printed the old literal 54; the native result,
persisted evidence and acceptance assertion all say 55. Its display code is corrected.
No live database migration or new model evaluation has occurred.

Argo tracks the PHarness chart in this repository. It has [reconciled the additive
control registry](ASTRA-M04-DIAGNOSTIC-SOURCE-CHART-OBSERVATION.json) from the source merge,
while image pins still identify the old source-19 release. The four original policies
and defaults remain unchanged. The new API diagnostic contract is not live yet.

The UI image is also [built and verified](ASTRA-M04-DIAGNOSTIC-RELEASE-PHARNESS-UI.json).

Remaining: build and verify the other five images and native bundle; prepare/review
immutable pins; observe Argo, API/UI/worker identity and live data preservation;
then run the small M04D canaries with recorded common-input controls. The connected
loop and full frozen qualification remain later gates. Hosted creation and Coding
Reliability V2 stay disabled. No Finance production approval is inferred.
