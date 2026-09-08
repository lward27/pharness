# ASTRA M04: Make semantic measurements match their claims

Status: implemented and locally validated on base `24790ccc0aea037d59cdaed0605d03c5c816145d`. The deployed runtime remains sourcee508; no new build or deployment has started.

## Corrections

The [live Verifier inspection](ASTRA-M04-E508F43-VERIFIER-ANALYSIS.md) prompted a complete audit, not another paid comparison of a known-invalid fixture. Preserve all 12 Planner and 24 Verifier scenarios, five approved/19 rejected Verifier expectations, tool schemas, prompts, models, limits and frozen coding/repair tasks.

- The frontend wrong-units candidate now uses `(Number(quote.price) * 100).toFixed(2)`: valid syntax, passing weak public tests, and an actual currency-units defect detected by the private behavioral probe.
- The wrong-query candidate still accepts `ticker` instead of required `symbol`. Its public test now expects the actual flat response key `symbol`; the test no longer fails accidentally because of an unrelated wrong expected envelope. The private correct-query probe still fails.
- The private corrected baseline control preserves the exact failing `test_existing_contract` assertion and fixes the corresponding `/legacy` behavior. It cannot manufacture a green result by deleting the test. This is a counterfactual fixture control, not an approved application patch or a waiver of production scope review.
- Python requests explicitly describe the existing in-memory handler: configuration comes from `environ`, price comes from the query stub, and missing/empty `MARKET_API_URL` returns 503. No network availability is implied. Valid and corrected README examples document the required status/body/configuration behavior; the two deliberately bad documentation candidates remain unchanged.
- Before model dispatch, a compiled semantic-defect fixture must have completed, passing public checks. A syntax or other public-check failure stops the measurement. Tests separately prove that its private probe fails on the defect and passes after correction, and check all valid/corrected controls against the complete handler/view contract.

The [change map](ASTRA-M04-SEMANTIC-FIXTURE-CHANGE-MAP.json) identifies every changed field and preserves case identities and expectations. Planner becomes **v2.5** because its public contract is clarified after the committed v2.4 selection correction; Verifier becomes **v2.4**. Onboarding, Diagnosis, frozen Coding and Repair revisions remain unchanged. No old report is edited or treated as a control for a new suite.

## Evidence and practical limits

The [independent native audit](ASTRA-M04-SEMANTIC-FIXTURE-NATIVE-AUDIT.json) runs the declared commands over every baseline, candidate and corrected workspace. All 11 semantic-defect candidates have green public checks and fail their private behavior probe; every corrected control passes. One of these scenarios deliberately exposes a semantic failure receipt as evidence; this audit does not claim all eleven are blind tests. The five approved examples and every corrected example also receive complete functional-contract checks in Rust tests. Existing source, stale-receipt, protected-path and documentation cases retain their distinct acceptance checks.

Three regressions failed before correction: frontend public checks were red, the corrected baseline deleted its assertion, and corrected documentation omitted required error behavior. The stricter audit then found the wrong-query expected-response defect. Local command evidence uses Node 26.0.0 and Python 3.14.4 on this Mac; it is not qualification on the pinned Linux runner environments. New live checks and final exact-runtime qualification remain mandatory.

[Validation receipt](ASTRA-M04-SEMANTIC-FIXTURE-VALIDATION.json): 235 core/evaluator tests and 13 API inference tests pass (248 total; one existing live-only test ignored). Scoped Clippy with warnings denied, formatting, and five architecture regressions pass. The original frozen coding and seeded repair source files remain byte-identical.

## Release and next execution

Commit the validated change and evidence, then build the full immutable artifact set from one merged revision using the selected Mac builder. Reconcile the exact release and full observation window before new calls. Repeat the affected Planner and Verifier primary/control measurements with new suite hashes and same-runtime references. The [Diagnosis timeout](ASTRA-M04-E508F43-DIAGNOSIS-PREFLIGHT.md) remains a separate prerequisite; no provider or deadline is silently replaced.

Preserve schema 55, the source-92-compatible reader floor, all Finance records and existing execution limits. Roll back only to a compatible complete release. Hosted activation and Finance production remain gated. M04D–F and M11/M12 acceptance are not closed by this correction.
