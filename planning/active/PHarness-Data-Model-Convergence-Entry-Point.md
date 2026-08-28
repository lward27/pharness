# PHarness Data Model Convergence — Planning Entry Point

## Purpose

Plan the behavior-preserving convergence of PHarness's physical SQLite model after the clean Finance generation has been accepted. The work should reduce duplicate lifecycle representations and compatibility-only storage without weakening immutable evidence, Repo Mode semantics, legacy readability, retention safety, or rollback.

This document is an entry point for a dedicated plan-mode session. It is not authorization to perform the convergence.

## Accepted baseline

- Database generation: `dbgen_finance_20260827`
- Schema version: `0049`
- Product model: one Finance Product, six Services, five registered Repositories, and reviewed repository-relative binding scopes
- Repo Mode acceptance: database-service and scraper-manager WorkItems completed through observed source merge
- Historical residue: superseded attempts are terminal history; no current Finance WorkItems or AgentRuns remain
- Evidence: both accepted WorkItems have 90-day raw-evidence holds; sealed evidence remains indefinite
- Archives: the prior database and its verified archive remain retained and independently recoverable
- Retention: daily seven-/30-day policy execution is enabled; archive deletion remains manual

Before planning, re-characterize these facts against the then-current `origin/main`, live Argo revision, current database generation, migration inventory, table counts, foreign keys, triggers, indexes, and API response fixtures.

## Problems to resolve

1. WorkItem lifecycle state is represented across legacy status fields, Repo Mode metadata, stage executions/outcomes, controller waits, and closure backfills.
2. Product and Repository binding responses still carry deprecated JSON compatibility fields beside typed relational scopes.
3. Workspaces and environment preparations retain legacy WorkItem-specific columns after becoming subject-scoped resources.
4. Source delivery has compatibility adapters around the canonical `SourceDeliveryIntent` model.
5. Evidence retention still depends on a mixture of typed references and older JSON payloads.
6. Read models and controllers must not require broad joins over unrelated legacy delivery resources for Repo Mode.
7. Migrations are append-only and numerous; the project needs an explicit long-term migration and archive-read strategy rather than ad hoc table removal.

## Required planning outcomes

The plan must produce:

- A table-by-table ownership and lifecycle map, including every writer, reader, foreign key, trigger, index, retention class, and immutable-record rule.
- A canonical target model for WorkItems, stages, Runs, workspaces, preparations, Product snapshots/bindings, source delivery, evidence references, approvals, and retention.
- A compatibility matrix for the legacy full-SDLC controller, Repo Mode, archived databases, CLI/API clients, operator UI, and existing characterization fixtures.
- A staged migration strategy with forward migration, verification, rollback mount, and mixed-version behavior defined for every step.
- Deterministic invariants and SQL checks that prove no WorkItem, StageOutcome, approval, audit event, provider observation, merge provenance, hold, preview, receipt, or archive record is lost or rewritten.
- A removal/deprecation ledger distinguishing fields that can be stopped on new writes, fields that remain readable, and tables that cannot be removed yet.
- Query and controller boundaries that keep Repo Mode isolated from legacy Pipeline/GitOps/Release state.
- Measured acceptance criteria for database size, query count/latency, migration time, integrity, and API compatibility.

## Recommended sequence

1. Freeze and characterize the current schema and durable API fixtures.
2. Document canonical aggregates and invariants before proposing SQL.
3. Stop compatibility-only writes where a canonical source already exists, while retaining reads.
4. Add typed backfills and shadow consistency checks; do not delete old representations.
5. Switch one aggregate/read model at a time behind compatibility tests.
6. Run retention and archive/restore validation against both pre- and post-convergence copies.
7. Remove physical structures only in a later, separately approved milestone after an observation window.

## Non-goals

- No multi-repository mutation, dependency graph, DeliveryPlan, Connected Mode, Node runner, satellite service, database-engine migration, or multi-replica controller.
- No migration squashing, archive deletion, evidence rewriting, or down-migration of the accepted clean generation.
- No UI redesign beyond changes required to preserve existing read models and truthful lifecycle state.

## Plan-mode prompt

Create a granular, behavior-preserving PHarness Data Model Convergence milestone from this entry point. Inspect the actual schema, migrations, store traits, controller modules, API/UI read models, retention implementation, and live clean-generation inventory first. Preserve legacy readability and immutable evidence. Separate characterization, additive canonicalization, shadow verification, cutover, observation, and eventual removal into independently reversible phases. End with exact tests, migration/restore drills, GitOps rollout, rollback criteria, and the evidence needed before any old physical structure can be removed.
