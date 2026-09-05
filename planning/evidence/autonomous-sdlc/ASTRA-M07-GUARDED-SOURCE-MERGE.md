# ASTRA M07: Guarded autonomous source merge

Status: implementation validated in the source worktree; release and live acceptance remain open. Base source: `103959f1715b54dfc21a50628c965a37964c57b8`, 2026-09-05. The deployed runtime remains the previously observed `48c77b7` release.

## What changed

The hosted source operation can now progress from a verified, checked pull request to a guarded merge attempt using the existing isolated Git writer. No new service, database, coding backend or general provider adapter is introduced. The implementation is limited to the yfinance and Finance frontend repositories on `main`; it cannot authorize a GitOps merge.

The original publication authorization still grants only branch, commit and pull-request creation. A separate immutable merge authority binds the WorkItem, operation, execution, source intent, policy hash, approved ChangeSet hash, original base commit, exact PR head/branch/number, GitHub Actions `Source integrity` identity and original one-hour source deadline.

The worker checks the exact PR and base, administrator-enforced strict branch protection, required checks and actual successful Source integrity execution. It revalidates the saved authority immediately before requesting durable admission for one merge attempt. Admission is persisted before GitHub is called. Competing requests cannot both acquire admission. A lost admission response does not cause a provider write, and a lost merge response never causes another merge attempt.

The request uses GitHub's exact-head merge argument. The observed merge commit must have exactly the approved base and head as its two parents, with a valid tree identity. Strict branch protection supplies GitHub's up-to-date-base enforcement; the API has no independent expected-base argument. Protections must remain enforced while work runs. Unexpected ancestry stops delivery even if a merge already occurred.

## Recovery and evidence semantics

Pause, cancellation, draining, expired authority, failed/stale checks or changed source withhold new merge admission. A missing Job may be reconstructed only before its attempt is admitted, using the original execution identity. After admission, recovery observes existing provider state. A completed Job is never recreated as a fresh merge attempt.

Receipts are immutable artifacts. An identical callback returns the original artifact; a conflicting callback is rejected. A late receipt remains recordable after pause, cancellation, expiry or changed current source, but cannot grant another action. Worker receipts remain claims until a separate read-only observer checks GitHub. Missing receipts can be reconciled from the admitted operation plus exact provider ancestry; the evidence identifies that recovery rather than claiming a known HTTP acknowledgement.

A successful source outcome records the admission, original authority, merge-parent/tree observation, receipt references when available, and provider-check evidence. It leaves the hosted WorkItem open for build, deployment and runtime verification. Incompatible or unadmitted merges terminate as failed; they do not fabricate completed or inapplicable delivery stages. Legacy source-only records retain their original behavior.

## Validation

The workspace suite passed 661 top-level tests, including 266 API tests, 37 worker tests and the frozen evaluation harness checks. This includes concurrent admission, repeated read-only context access, missing Job recovery after admission, pause/cancel, failed and expired checks, stale source, late/conflicting receipts, exact merge ancestry, provider-observation recovery without a writer callback, failure termination, and preserved downstream gates. Worker guards reject missing protections, skipped/neutral Source integrity runs, stale heads/bases, forks, draft or unresolved mergeability, and invalid merge provenance.

The first full API run identified an intentionally changed route inventory and a stale test assumption that failed work remains open. Those expectations were corrected without changing the success or failure gates. The earlier integration assertion also distinguished successful hosted continuation from existing terminal failure semantics. Clippy with warnings denied, the architecture checks and formatting passed after using syntax compatible with the declared Rust version and grouping the source conclusion into one argument. The complete API suite was rechecked after those changes. Final check results, exact logs and content hashes belong in the [validation record](ASTRA-M07-GUARDED-SOURCE-MERGE-VALIDATION.json). Evaluation-harness logs include deliberate failing seed programs; those are negative fixtures, not failed top-level tests.

## Remaining acceptance gates

No live GitHub source merge was performed by this implementation. The [Finance branch-protection decision](ASTRA-M07-SOURCE-MERGE-DECISION.md) remains pending. The two proposed CI checks passed, but protected-main enforcement and actual writer/observer credential access must be revalidated before activation. The current readiness path still needs to report these merge-specific prerequisites before expensive coding begins.

The source-to-Tekton controller transition remains separate work. Both real Finance build paths have been verified through program-operated runs; they do not count as autonomous WorkItems. M04 still requires all remaining qualifications. M08–M12 and production approval are unchanged. M10 must expose the new pending admission/receipt/exception details clearly in the console; this slice does not claim that visual acceptance.

## Deployment and recovery procedure

Release the complete required PHarness image set and native bundle from one merged source revision after active qualification and ordinary Runs reach a safe boundary. Keep hosted creation disabled until all enabling gates pass. The API, worker and observer must understand the new merge execution and evidence fields before any new hosted writes occur.

There is no SQL migration in this slice; it uses the schema-53 operation and artifact records. Nevertheless, before enabling source-merge writes, record this compatible behavior in the minimum rollback release. The currently retained schema-53 floor does not prove that an older worker understands new execution kinds. Never remove an admission record to retry a merge. Preserve the operation, inspect its provider outcome, and recover forward if the prior reader cannot understand the recorded work.
