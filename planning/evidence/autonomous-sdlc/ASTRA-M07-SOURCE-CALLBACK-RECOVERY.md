# ASTRA M07: Source callback identity and closed pull requests

Observed 2026-09-05. Base: `5b7942fc5e5aaf3aec6bb9ca6c9442adabc9dbe6`.

The normal writer callback exposed a defect in the newly merged source controller: after recording `writer_dispatched`, the controller attempted to replace an immutable operation-reference value with `pull_request_open`. The store correctly refused that identity change, but the WorkItem then became blocked. This code was not in the deployed `48c77b7` image and no hosted WorkItems had been enabled.

The controller now retains only stable source-intent identity and its original deadline in operation references. Changing source status remains in the existing source-intent record. The store's immutable identity constraint is unchanged. A confirmed closed, unmerged pull request also terminates its source operation and releases the repository lock while leaving the WorkItem blocked; it authorizes no build.

## Evidence

The new regression invokes the actual writer callback, reproduces the identity conflict before the fix, and then confirms the same operation identity continues into observation. The termination coverage includes closed, unmerged pull requests. The API suite passed 260 tests. A final assertion-only test edit was checked with the focused regression, followed by Clippy with warnings denied, formatting, architecture checks, and five dependency-parser tests. Checksums and exact local logs are recorded in [the validation record](ASTRA-M07-SOURCE-CALLBACK-VALIDATION.json).

This is tested source implementation. Deployment and autonomous Finance acceptance remain separate gates. A terminal writer without a callback still retains its identity and lock for investigation; this change does not claim provider-side recovery for that case.
