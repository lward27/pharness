# ASTRA M04: Source-19 Verifier terminal result

Observed 2026-09-06 at 11:47 UTC. Runtime `19b0c55c48e3614d0b4507d56df3029a52475618`.
Evaluation `infeval_01a07635d61a78b0825b6ee3c8ee3578` completed; its **qualification failed**.

Both attempts passed **1/24**. Of 48 cases, 47 recorded an accepted typed submission;
all 47 matched the disclosed expected verdict and marker. Only two used the exact
`fixture_evidence` reference. The other 45 failed reference matching; one produced
no accepted typed submission. Infrastructure validity passed. The raw result
records 1,236,927 input and 266,486 output tokens; no cost is inferred from them.

The [unchanged result](ASTRA-M04-19B0C55-VERIFIER-LIVE-RESULT.json) and
[derived counts](ASTRA-M04-19B0C55-VERIFIER-ANALYSIS.json) preserve the failed gate.
The first retained submission correctly identifies an empty diff, placeholder
`VALUE = 1` source and tests that always pass. This supports the harness-validity
concern: there was no endpoint implementation for it to verify. It does not prove
that this model can independently find the real defects in the intended cases.

Expected verdicts, answer markers and defect-bearing case identifiers were visible
to the model. The historical zero false-approval and zero false-rejection counters
therefore do not establish semantic safety. Do not rescore this report under the
new contract or present its matching verdicts as blind verification.

The approved M04C work replaces those measurements and preserves their scenario
mapping. M04D uses small real-gateway canaries before another full qualification.
No further old-suite run was dispatched. Hosted activation and M04 acceptance
remain blocked on the new exact-runtime evidence.
