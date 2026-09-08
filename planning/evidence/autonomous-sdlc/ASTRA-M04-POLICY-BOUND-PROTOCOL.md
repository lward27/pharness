# ASTRA M04: Protocol checks must qualify the requested policy

Status: implemented and locally validated on main base `5a409453d28d68d22b12899a312df763c51e48dd`; merged/deployed identity remains a separate release gate. M04 remains unqualified.

## Objective finding and correction

The old target preflight selected the first policy pointing to a model target. For `fireworks-kimi-k3@v1`, that was the onboarding profile with medium reasoning, even when an external operator intended to check the Planner profile with high reasoning. The four common-control profiles also share one target revision. Qualification admission accepted any fresh 30/30 target receipt without checking the actual policy, registry or runtime. A newer failed check could be bypassed by an older passing check.

The existing preflight request now accepts an exact `policy` reference. An ambiguous target without one returns a conflict before a verification record or model request is created. A target with exactly one policy retains its previous request compatibility. Unknown references or references bound to another target/revision are rejected.

Each success or failure records the actual policy ID, revision and hash plus the API runtime revision in its existing capabilities JSON. Qualification and diagnostic admission require the latest receipt for the exact policy, target, registry and runtime, all 30 cases passing, complete capability flags, no contradictory failure, and the unchanged 900-second expiry. A newer failure for that binding overrides an older pass. Historical receipts stay readable; missing provenance cannot satisfy the new gate. The deterministic protocol-contract hash in the execution binding remains distinct from the observed calibration report hash.

The policy API exposes its matching receipt and readiness. Settings places **Check protocol** beside the policy, shows its protocol state separately from semantic qualification, and disables a new qualification until the exact check is ready. Operator/reason fields sit with those actions. Opening or refreshing settings is read-only. Checking protocol still makes bounded paid model calls and persists evidence; it does not activate a profile.

## Validation and subjective assessment

[Validation](ASTRA-M04-POLICY-PROTOCOL-VALIDATION.json) records 297 API Rust tests, including four policy-boundary and two HTTP/store integration tests; three UI unit tests; 16 browser scenarios; API Clippy with warnings denied; formatting; five architecture regressions; and a production UI build. HTTP tests use a disabled gateway and worker and prove rejected admission creates no evaluation. No fixture receipt represents a live model pass.

The browser matrix covers unverified, stale, failed and passing policy evidence in both themes at desktop and phone widths. A passing shared target never unlocks an unchecked policy. Keyboard activation emits one request for the selected policy; opening settings emits no mutations. The first browser fixture incorrectly intercepted source-module URLs; that failed report remains retained, and the corrected full matrix passes.

Subjectively, the action is now attached to the settings it tests, and the separate protocol and semantic states are understandable. The settings page remains dense and technical, with considerable scrolling on a phone and unused grid space when only one policy is shown. This targeted correction preserves Lamina; it does not close M10's wider presentation or end-to-end acceptance gates. Screenshots use explicit presentation fixtures:

- [Dark desktop](screenshots/ASTRA-M04-policy-dark-desktop.png) and [light desktop](screenshots/ASTRA-M04-policy-light-desktop.png).
- [Dark phone](screenshots/ASTRA-M04-policy-dark-mobile.png) and [light phone](screenshots/ASTRA-M04-policy-light-mobile.png).

## Release, compatibility and remaining work

No schema migration, model/default switch, new backend, limit increase or frozen-suite change is needed. All Planner/Verifier fixture corrections already merged at source5a40945 remain intact. The [intermediate release hold](ASTRA-M04-5A40945-RELEASE-HOLD.json) preserves its build receipts; release the combined correction from one later merged source with all seven images and its native bundle, then verify exact deployment identities and the complete service window. Do not mix artifacts from the two source revisions.

Use the [operator procedure](../../operations/ASTRA-M04-GATEWAY-PREFLIGHT.md) after that compatible release. Fresh affected Planner/Verifier primary and control diagnostics still need to run on valid current-suite inputs, with exact-policy protocol receipts first. The earlier Diagnosis timeout remains a prerequisite failure; changing profile selection does not establish that its provider is healthy. Existing native results are not rescored. No Finance source change, production approval, local-model activation or autonomous acceptance occurred here.

Readers tolerate the additive JSON fields. Older runtimes lack this qualification guard; an operational rollback must suspend new qualification/activation until this enforcement is restored. The hosted-data reader minimum remains separately recorded; no old runtime is newly declared safe for hosted writes.
