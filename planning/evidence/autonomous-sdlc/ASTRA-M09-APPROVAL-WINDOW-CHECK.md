# ASTRA M09: Deterministic production approval-window check

The combined M07 source run passed 258 API tests and failed one approval-window assertion. The test calculated a deadline one millisecond beyond its sampled 30-minute limit, then the validator read the clock again. Once a millisecond elapsed, that deadline was legitimately within the validator's window. The failure was in the test's decision-time assumption.

The production wrapper continues to use the real clock and the same 30-minute comparison. The decision function now accepts an explicit instant so the regression test checks exact boundaries deterministically. Missing and malformed values, already-expired values, expiry at the decision instant, and 30 minutes plus one millisecond are rejected. Exactly 30 minutes and five minutes remain valid.

After this change, all 259 API tests passed. Clippy with warnings denied, formatting, module boundaries and five dependency-parser tests passed. [Validation record](ASTRA-M09-APPROVAL-WINDOW-VALIDATION.json) preserves the initial failure and final tested source/log hashes.

This does not implement or accept the hosted production-approval, promotion or rollback path. Those M09 gates remain open. No live approval or deployment was performed by these tests.
