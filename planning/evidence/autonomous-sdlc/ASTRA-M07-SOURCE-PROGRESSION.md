# ASTRA M07: Durable source publication and observation

Status: implemented and locally verified against merged source `48c77b7b4438d621ff9563b913857bcf771f1800`; release and end-to-end acceptance remain open.

The hosted controller now selects the existing source-publication action after an approved ChangeSet. It checks the saved SourceDelivery permission and holds the repository lock across publication and observation. It does not acquire a coding slot for a Git operation. Source-only work keeps its original execution path.

The intent binds the WorkItem, repository, original source commit and saved policy hash. Its originally planned writer identity survives an interruption between intent creation and dispatch. Recovery first observes the exact hash-bound Job and recreates only a missing eligible Job with that same identity. Pausing prevents a missing writer from being created; read-only observation can continue.

The existing one-hour external-wait allowance is an absolute deadline from the operation's creation. New callbacks or observer identities cannot reset it. Source observation runs at most once a minute under the recorded Observe permission. Expiry stops further dispatch while retaining valid late callbacks. Failed and merged source outcomes release the operation boundary, but neither closes the hosted WorkItem. Head drift retains the exception and repository lock.

## What the tests establish

The full API suite passed 257 tests. After adding one final regression test, the seven source-focused tests passed; together these runs cover 258 distinct API tests. The added test exercises six late callback states and verifies that none resets the wait or closes the WorkItem. Clippy with warnings denied, formatting, module boundaries and the five dependency-parser tests passed. [Validation record](ASTRA-M07-SOURCE-PROGRESSION-VALIDATION.json) binds the tested files and logs.

Tests cover automatic publication, repeated reconciliation without duplicate Jobs, unchanged execution budgets, original-identity recovery, pause/cancel/expiry, writer and observer identity rejection, terminal Job handling, and late failed/merged/drift/waiting results. They use temporary real SQLite and deterministic dispatcher fixtures.

## Remaining limitations

This is source publication and observation, not automatic source merge. GitHub merge authority, protection/readiness checks, and the source-to-Tekton transition still require implementation. The proposed Finance branch protections remain an owner decision recorded in the program evidence artifact `ASTRA-M07-SOURCE-MERGE-DECISION.md`.

A terminal source Job whose callback is missing stays blocked with its identity and repository lock retained. This prevents a blind repeat of a source effect, but automatic reconstruction of that missing remote outcome is not yet proven. It remains an M06/M07 recovery gap.

No actual GitHub change, PipelineRun, application deployment, qualification, hosted request, or Finance acceptance is represented by these fixtures. M04, M06 and M07 gates remain open. Deploy only in a subsequent complete immutable PHarness release; keep hosted creation disabled until its dependencies pass. Recovery requires the schema-53-compatible release floor.
