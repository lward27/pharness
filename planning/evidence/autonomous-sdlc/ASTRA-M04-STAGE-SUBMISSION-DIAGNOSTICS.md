# ASTRA M04: Preserve submitted stage evidence

Status: local diagnostic implementation; not deployed and not a new live qualification result.

Source base: `5d35fad3bbcb9cbd0b7d3397829b5496e73094bc`. Branch: `codex/astra-verifier-evidence-diagnostics`. This slice changes evaluation reporting only; it preserves stage fixtures, suite hashes, prompts, models, tool schemas, budgets, existing failure labels and gate decisions.

The [48c77b7 Verifier run](ASTRA-M04-48C77B7-VERIFIER-FAILURE.md) passed 3/48 cases. Its 43 `verification_evidence_mismatch` failures combine two independent checks: the literal `fixture_evidence` reference and the expected marker. The report omitted the accepted typed documents, so it cannot show which check failed in each case. A generic failure label is insufficient to choose a defensible prompt or harness correction.

Stage evaluation results now include optional `stage_submission` evidence for both passed and failed cases. It records whether an accepted typed submission existed, the original canonical document hash and byte count, and up to 16 KiB of retained structured content. Credential-shaped fields and text are redacted. Oversized content, redaction expansion beyond the cap and excessive nesting are explicitly marked; omitted or redacted content is never described as complete. This diagnostic cap does not alter what the tool accepts or how the original document is scored.

Verifier results additionally retain the three unchanged predicate results separately: decision match, required evidence-reference presence and expected-marker presence. They state that the fixture discloses its expected decision and marker. A passing result therefore demonstrates compliance with that contract; it does not independently demonstrate discovery of an undisclosed implementation defect.

The existing scorer remains unchanged. Original results are not rescored, and prior reports deserialize with no invented submission evidence. Coding and Repair reports omit the new optional field. The runtime gate still requires fresh matching qualification before autonomous activation.

All **35 evaluation unit tests** passed, including five submission-retention tests. All-target Clippy with warnings denied, formatting and architecture checks passed. A 48-case Verifier replay passed and retained all 48 complete documents with the original suite hash. These are local checks, not live qualification. See [the validation receipt](ASTRA-M04-STAGE-SUBMISSION-DIAGNOSTICS-VALIDATION.json) for source hashes and checks. Tests cover every combination of the Verifier's three predicates in both current and historical suites, exact document retention and hashing, missing submissions, size and depth bounds, redaction, and reading all 48 historical Verifier results without changing their three recorded passes. A deterministic replay can prove report wiring; it cannot count as live gateway qualification.

Next: include the diagnostic reader with the already merged onboarding and scoring corrections in a compatible immutable release, retain the schema-54 rollback floor, and run fresh qualification against the unchanged registered candidates. Use the newly retained evidence to distinguish model behavior from a contract defect before changing either. M04 remains open, and no historical failure is upgraded by this change.
