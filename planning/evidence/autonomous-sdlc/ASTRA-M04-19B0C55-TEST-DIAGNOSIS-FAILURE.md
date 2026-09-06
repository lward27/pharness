# ASTRA M04: Source-19 Test Diagnosis qualification failed

Status: **not qualified**. The two complete attempts scored **1/12 each**, after
30/30 protocol checks. All 24 typed diagnoses were retained. The runtime was
`19b0c55c48e3614d0b4507d56df3029a52475618`.

The report records 21 evidence-reference failures and ten classification failures;
these overlap and are not 31 separate failed cases. Some diagnoses cite the
controller-provided evidence hash instead of its catalog ID. Others cite source
paths, or the catalog ID together with paths. The scorer accepts only an array of
`fixture_evidence` IDs. The tool schema currently accepts arbitrary nonempty strings.
This contract mismatch requires an explicit rule shared with actual stage validation.
An arbitrary hash or filename cannot silently become trusted evidence.

Several summaries also offer unsupported explanations, such as treating a test
class or file name as a cause of compilation failure. That is an agent-quality
problem even when the reported failure category matches. The current fixtures
contribute confusing evidence: every case exposes the same passing Python files,
with short synthetic failure text and an already-supplied category. The categories
`preexisting_failure`, `localized_repair` and `coherent_repair` fall through the
scorer's type mapping to `unknown`, despite conveying different concepts from
the tool's failure-kind enum. The model cannot infer missing actual failure output.

Correct and validate these controller/evaluator contracts before fresh qualification.
Retain all twelve scenarios, negative controls, existing thresholds and limits.
Do not convert the current results to passes or treat protocol success as diagnostic
reliability. Unsupported causal claims remain a concern requiring fresh evidence.

The local observer disconnected after dispatch. A read of the persisted evaluation
on 2026-09-06 recovered its terminal result; no duplicate evaluation was submitted.
The completed Kubernetes Job had already expired under its existing retention policy.

[Raw result](ASTRA-M04-19B0C55-TEST-DIAGNOSIS-LIVE-RESULT.json) and
[analysis](ASTRA-M04-19B0C55-TEST-DIAGNOSIS-ANALYSIS.json) retain identities,
report hash and measured usage. M04 and autonomous Finance acceptance remain open.
