# ASTRA M04D: Verifier primary and matched control

Status: complete valid diagnostics on d675159. GLM-5.3 primary2/3; Kimi K3 control3/3. No qualification, activation or model-default change.

## Objective results

Both runs use source `d675159cb205d222616d253d311706be1889bb91`, Verifier v2.4, one attempt, identical retained inputs and unchanged per-role limits. Their separate exact-policy protocol prerequisites pass30/30. The [independent comparison](ASTRA-M04-D675159-VERIFIER-COMPARISON.json) verifies native report hashes, matching actual source/workspace identities, complete unredacted public inputs and initial contexts, and equal tool/prompt/context/budget configuration.

| Case | GLM-5.3 primary | Kimi K3 control |
| --- | --- | --- |
| Wrong endpoint path | Correctly rejected; passing tests target the wrong endpoint | Correctly rejected |
| Valid implementation | Correctly approved against exact-source test receipts and requirements | Correctly approved |
| Frontend semantic mismatch | No accepted verdict: malformed terminal action, missing `verification` field | Correctly rejected the price conversion and weakened test coverage |

The [primary](ASTRA-M04-D675159-VERIFY-PRIMARY-RESULT.json), `infeval_01a082949c567da0949805d60ce85565`, used 110,844 prompt and 14,843 completion tokens over 321.189 seconds of case execution. The [control](ASTRA-M04-D675159-VERIFY-CONTROL-RESULT.json), `infeval_01a0829cf8417f3292422ddd61bb386e`, used 113,108 prompt and 12,767 completion tokens over 270.117 seconds. These times exclude Job startup and protocol checks; they are not cost or throughput benchmarks.

Both evaluations are infrastructure-valid, terminal and diagnostic-only. Both have no qualification ID and false `gate_passed`. The [primary Job/Pod receipt](ASTRA-M04-D675159-VERIFY-PRIMARY-EXECUTION.json) and [control receipt](ASTRA-M04-D675159-VERIFY-CONTROL-EXECUTION.json) record execution under the released evaluator digest. Each result retains source, inputs, events, token usage and failure detail. The [primary input review](ASTRA-M04-D675159-VERIFY-PRIMARY-INPUT-REVIEW.json) preceded control dispatch; no uncertain POST was repeated.

## Interpretation and limits

The GLM failure is a provider/tool-protocol submission failure, not an observed incorrect semantic verdict. The retained diagnostic reports `MalformedArguments`, missing the required `verification` field, and no accepted typed submission. This record does not independently establish whether generation or an upstream formatting layer caused the malformed payload. It was correctly treated as failure, without inventing a verdict or making the case green.

Kimi's frontend rejection identifies concrete evidence: multiplying the price by100 renders12.5 as1250.00, while the requirement specifies currency units; replacing four behavioral tests with one type assertion removes required coverage. Its residual-risk prose also includes an unproven floating-point speculation. That speculation is not a verified fact and is not needed to support the rejection. A correct native verdict does not validate every sentence in an agent's explanation.

These three cases favor Kimi as a candidate for subsequent qualification under the same limits. They do not establish general superiority, reliability thresholds, independent-run repeatability, or suitability for Finance activation. Preserve the original GLM failure; do not loosen the tool schema, manually fill in its missing verdict, or silently switch registered policies. The earlier e508 native3/3 used invalid fixtures and remains historical; it is not added to this result.

The [Planner comparison](ASTRA-M04-D675159-PLANNER-COMPARISON.md) separately requires an explicit decision boundary before automatic implementation. Finish the declared Diagnosis prerequisite, then select the next bounded qualification work against those findings. M04E's connected loop, M04F's complete stage/coding/repair qualification and M11's human production approvals remain open.
