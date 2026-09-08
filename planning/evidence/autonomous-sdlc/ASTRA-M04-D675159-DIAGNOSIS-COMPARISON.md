# ASTRA M04D: Diagnosis primary and matched control

Status: both complete valid diagnostics pass2/2. The earlier Nemotron protocol timeout did not recur. No model or limit changed; neither run is qualification.

Both exact-policy protocol checks pass30/30 on source `d675159cb205d222616d253d311706be1889bb91`. The [comparison](ASTRA-M04-D675159-DIAGNOSIS-COMPARISON.json) verifies complete unredacted identical public inputs and initial contexts, equal source/workspace identities, native report hashes, and matching prompts/tools/context/budgets under Diagnosis v2.3.

| Run | Native result | Prompt/completion tokens | Case execution time |
| --- | --- | --- | --- |
| [Nemotron primary](ASTRA-M04-D675159-TEST-DIAGNOSIS-PRIMARY-RESULT.json), `infeval_01a082a36f5f78d3a8f4a2b84fb22a54` | 2/2 | 13,915 / 1,679 | 6.274 seconds |
| [Kimi control](ASTRA-M04-D675159-TEST-DIAGNOSIS-CONTROL-RESULT.json), `infeval_01a082a6774176c1a9d61344c1b84aa0` | 2/2 | 18,656 / 2,691 | 59.779 seconds |

Both identify the actual failed assertion and recommend changing the source constant to the value explicitly required by the task, preserving the test. Both classify the passing case as unknown/no evidenced failure and propose no repair. This differs from the Planner's unrelated `/legacy` conflict: here the stated requirement resolves the intended behavior. The control provides more detailed source references; the primary is much shorter. Both satisfy these two cases.

The [primary execution receipt](ASTRA-M04-D675159-TEST-DIAGNOSIS-PRIMARY-EXECUTION.json) and [control receipt](ASTRA-M04-D675159-TEST-DIAGNOSIS-CONTROL-EXECUTION.json) retain the actual Job/Pod and released evaluator identities. The [primary input review](ASTRA-M04-D675159-TEST-DIAGNOSIS-PRIMARY-INPUT-REVIEW.json) precedes matched control dispatch. Each native result remains infrastructure-valid and diagnostic-only with no qualification ID. Case times exclude startup and protocol work and do not establish general throughput or cost.

The [old response timeout](ASTRA-M04-E508F43-DIAGNOSIS-PREFLIGHT.md) remains retained evidence, not a semantic failure and not a current blocker. Passing once does not establish that its intermittent cause is permanently resolved. There is no evidence from this pair that replacing the Diagnosis default is necessary. A model interpreting known deterministic results must still demonstrate useful behavior in M04E's actual failure-to-repair handoff and satisfy the complete stage qualification gate.

The declared d675159 Planner, Verifier and Diagnosis pairs are now terminal. Their results justify the [Planner decision correction](https://github.com/lward27/pharness/pull/395) and a further qualified Verifier-candidate comparison. They do not close M04, authorize Finance deployment, or replace the two full frozen coding qualifications.
