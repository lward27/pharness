# ASTRA M04: Native source identity for diagnostic controls

2026-09-08. Implementation base: `8f1308fa980b8c63c45b8b7b1fc8f2e0e15a4bb1`; deployed source at diagnosis: `a2d05f1b41c15cb34a4543327ba468b9da3b5094`. Status: narrow reader correction and local validation complete; immutable release and live control remain pending. M04 remains open.

## Problem and correction

The [actual passing onboarding report](ASTRA-M04-A2D05F1-ONBOARDING-RESULT.json) contains `source_sha` and no `base_sha`. The API common-input reader required the latter, rejecting a valid reference before dispatch. A new regression using that unchanged native report reproduced HTTP 409. The earlier synthetic API fixture used `base_sha`, so it tested an internally consistent example that the real evaluator did not produce.

The reader now requires native `source_sha` and passes that exact value as `base_sha` in the evaluator's existing input-reference contract. No alias fallback fills missing evidence; a missing source field produces a specific error. This translates two existing boundaries without changing either stored reports or evaluator output. Report/input hashes, exact-runtime/suite/prompt/tool/context/budget equality, complete retained inputs and reconstructed source/workspace checks remain mandatory.

The regression reads the real stored report, checks exact identity and public-input transfer, and rejects a missing native source even when a base_sha alias is present. The evaluator's existing canary replay now reconstructs source from serialized results for every declared Onboarding, Planner, Test Diagnosis and Verifier case. Existing changed-source/context/input rejection tests remain.

No model, gateway configuration, budget, suite identity, scorer, frozen coding task, database schema, production authority or historical verdict changes. This fixes comparison machinery; it supplies no evidence of model quality by itself.

## Validation, release and recovery

The [validation record](ASTRA-M04-DIAGNOSTIC-SOURCE-VALIDATION.json) owns red/green regression, inference/evaluator tests, architecture checks, formatting and Clippy results. All 12 API inference checks, 48 evaluator checks, five architecture checks, formatting and Clippy with warnings denied passed. The three diagnostic tests are included in the twelve API checks. Local replay is not a live comparison. The existing source-a2 diagnostic remains a pass on source a2 only.

Build the complete seven-image/native-bundle release from one merged revision using the explicitly selected Rancher Desktop `pharness-mac` builder. Preserve the uncached AMD64 execution and actual-runtime checks. Validate the real Argo values and immutable registry identities before merging PHarness pins. Observe five-minute baseline and ten-minute released service windows, exact running images, source alignment and unchanged database generation. Source a2 is the preceding compatible release; schema 55 and the compatible-reader floor remain unchanged.

After release, renew the required protocol checks and run the bounded valid/blocked Onboarding primary diagnostic and registered Kimi control with the primary evaluation as the input reference. Both must use the same new runtime and original limits. Capture requested and running Job identities plus terminal container status promptly. Then continue grounded Plan, observed Test Diagnosis and blind Verifier canaries and controls, stopping to inspect a first boundary failure. M04E connected coding/repair and M04F frozen qualification remain separate gates. No Finance production action or local Windows target is enabled.
