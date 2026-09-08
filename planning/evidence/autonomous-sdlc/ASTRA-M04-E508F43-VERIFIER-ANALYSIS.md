# ASTRA M04: Verifier canary result and measurement limits

Status: native primary completed **3/3**, but not a valid complete semantic-qualification result. The matched control remains undispatched. No result was rescored, model selected or profile activated.

Source `e508f430e9f006c5f3e9f74fdd120d275605d85c`; Verifier v2.3 hash `sha256:32bbf81acc16d65a96608bd9c27a3df9b915ed0430a460cdd30e64b265d17b61`. [Protocol](ASTRA-M04-E508F43-VERIFY-PRIMARY-PROTOCOL.json) passed 30/30 before primary `infeval_01a08218541878c3b1e43d120c62c474`. [Native output](ASTRA-M04-E508F43-VERIFY-PRIMARY-RESULT.json) and [final execution](ASTRA-M04-E508F43-VERIFY-PRIMARY-EXECUTION.json) remain unchanged, with qualification null.

| Case | Native verdict | What was actually demonstrated |
| --- | --- | --- |
| wrong-endpoint-path | Correct rejection | The model identifies `/api/quotes` versus required `/api/quote` despite passing public tests, and explains the coupled test/documentation mismatch. This is useful semantic evidence. |
| frontend-semantic-mismatch | Correct rejection | The candidate has a JavaScript syntax error in `Number(quote.price) * 100.toFixed(2)`, and its public unit check fails. The model also explains the intended wrong-units defect, but this case does not require semantic inspection to reject. |
| valid-implementation-a | Expected approval with risks | The model accepts the coherent route rename but notes incomplete error documentation and the gap between an actual unavailable upstream and this in-memory handler. Those ambiguities belong to the fixture contract. |

Measured work totals 412.522 seconds, 144,442 input and 23,007 completion tokens; monetary cost is unavailable. Case times were 110.071, 176.853 and 125.598 seconds respectively. The old diagnostic wrapper reports `infrastructure_valid: false` because of the separately corrected full-suite count bug; the native case outcomes above are unchanged.

## Decision

Do not spend a matched control on the invalid frontend measurement or count the complete 3/3 as blind semantic proof. The [full fixture audit and correction](ASTRA-M04-SEMANTIC-FIXTURE-VALIDITY.md) also found an incorrect public response expectation in the wrong-query case, a corrected baseline control that deleted its failing test, and underspecified valid-control documentation. All 24 cases now have explicit local audit coverage; new live measurements require the revised suite and immutable runtime.

The next build was stopped before artifact publication. Source24790cc has a passing [uncached Mac AMD64 preflight](ASTRA-M04-24790CC-AMD64-PREFLIGHT.json), but [zero image pushes and no deployment change](ASTRA-M04-24790CC-BUILD-HOLD.json). No control run was created or cancelled; it simply remains undispatched.

The useful result is that GLM can identify a real source/test disagreement. The bad result is that our fixture review did not consistently require the claimed kind of evidence. A new orchestration backend or larger model budget would not repair that. M04C–F, the Diagnosis provider prerequisite, and both Finance acceptance WorkItems remain open.
