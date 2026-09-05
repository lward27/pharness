# ASTRA M05: Unified hosted SDLC contract

Status: planned.
Authority: [approved program](ASTRA-00-PROGRAM.md).
Dependencies: M02 bindings and M03 integrity. Code preparation may proceed while an unrelated TLS prerequisite is blocked; acceptance still requires usable bindings.

## Objective and scope

Expose one hosted workflow while preserving the meaning of existing source-only work.

The program's locked authority, budget, source identity, compatibility, and evidence
rules apply. Work on current verified main in an isolated `codex/` worktree.
Do not mark this milestone accepted because a dependency appears healthy.

## Implementation

1. Reuse WorkItem, StageExecution, StageOutcome, source delivery, pipeline, release, approval, and observation records. Keep discover -> plan -> implement -> test -> verify -> source_delivery -> release -> observe.

2. Extend product-scoped creation/readiness with a versioned workflow policy snapshot: binding revision, authorized automatic actions, current execution limits, one-application-repository scope, pinned context, and bounded rollback permission.

3. Represent build, staging, and production as inspectable release steps using existing effect identities. New hosted work cannot close at source merge.

4. Resolve validated product/repository configuration server-side. Readiness explains temporary Jobs and persistence; GET/navigation never triggers it.

5. Preserve pinned contracts for in-flight legacy work and readable source-only history. Cut over new creation to hosted work; obsolete creation routes return a clear retirement response after cutover.

6. Use additive migrations from the verified current schema. Install compatible readers before enabling new writes; preserve Finance generation and retention.

## Interfaces and compatibility

Versioned policy snapshot on existing WorkItem creation/readiness; enriched existing operator/action projections. Reuse current storage abstractions before adding persistence. No second workflow root or tool-agnostic DSL.

Preserve immutable historical evidence, existing Finance data generation, and additive migration compatibility. Record any minimum compatible rollback version before enabling new writes.

## Tests and acceptance

- [ ] One definition of hosted success requires release and observation evidence; source merge alone remains nonterminal.
- [ ] Legacy records retain their original result and inapplicable stages; no bulk relabeling or reopening.
- [ ] Policy/profile edits cannot retroactively rebind an existing WorkItem; stale inputs invalidate the affected authorization/evidence.
- [ ] API and migration tests cover new, legacy, paused, partially completed, and incompatible-reader cases.
- [ ] Minimum compatible rollback version is recorded before enabling hosted creation.

Fresh and upgrade database tests against current migrations, request/response tests, legacy fixture compatibility, and source-closure regression checks.

## Deployment and recovery

Record exact changed resources, source revisions, validation, before/after identities,
and external effects. Keep managed cluster changes in authoritative GitOps sources.
Production runtime acceptance still requires a concrete human approval.

Disable new hosted creation without rewriting active records; only roll back to a reader that understands the new contract. No down migration or data-generation reset.

## Evidence and closeout

Write ASTRA-M05-UNIFIED-SDLC-CONTRACT.md with interface/migration compatibility and cutover validation.
Use `planning/evidence/autonomous-sdlc/` for milestone execution evidence unless an
existing assessment location is explicitly named. Include date, revisions, commands
without secrets, observed results, failures, limitations, and commit/release identities.
A test result and a deployed result are separate claims.

Review coverage: F09, F14 and product-direction findings.
Update the master ledger and this document only after its checks are evidenced.
Unmet criteria remain unchecked with a concrete reason and next action.

## Goal-mode execution prompt

Read ASTRA-00-PROGRAM.md and this milestone. Verify dependencies against current
evidence, inspect the affected implementation, execute the bounded changes above,
and run the specified meaningful checks. Preserve user work and all safety/identity
boundaries. Record results, commit the implementation and evidence, update the master
and finding ledger, then continue the next eligible milestone. If an external input is
missing, explain the exact blocker and continue independent work. Do not weaken a gate,
silently switch provider/budget, or claim unexecuted deployment or autonomous acceptance.

