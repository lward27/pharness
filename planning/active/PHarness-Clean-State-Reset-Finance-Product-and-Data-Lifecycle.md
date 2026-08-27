# PHarness Clean-State Reset, Finance Product Model, and Data Lifecycle Foundation

Status: approved for implementation

Baseline: `57a4d0410b944320419a55333c085042e7977bec`

## Objective

Replace the accumulated test database with a clean, independently rollbackable
SQLite generation, establish safe retention and purge controls, and correct the
Product model using Finance as the proving Product. Never clean the live
database with ad hoc row deletion and never delete the previous database or its
archive automatically.

## Locked implementation contract

- Identify every database with a durable generation ID and require the mounted
  generation to match the Helm expectation.
- Support `normal`, `draining`, and `read_only` operation. Disable creation of
  legacy `mode=null` WorkItems while preserving their read compatibility.
- Make the PHarness data claim configurable and retained. Archive the existing
  SQLite database before and after drain, initialize a separate claim from
  zero, and keep both old-data copies for 14 days behind an explicit deletion
  confirmation.
- Add offline archive and verification tooling that uses a consistent SQLite
  backup, integrity checking, checksums, table counts, release provenance, and
  a sanitized yfinance characterization export.
- Introduce immutable typed RepositoryBinding scopes with roles `source`,
  `delivery`, `automation`, `product_integration`, and `documentation`, plus an
  optional Product-owned Service.
- Introduce `pharness.dev/product-model/v1alpha2`; snapshots embed Services,
  Repositories, active binding revisions, and typed scopes in canonical order.
  Existing `v1alpha1` snapshots and legacy scope arrays remain readable.
- Separate Product-model revisions from executable Repository onboarding and
  require actor, reason, state hash, and preflight hash for model changes.
- Add RetentionHold, RetentionPreview, RetentionReceipt, ArchiveRecord,
  RunSummary, and typed EvidenceValidationReference records.
- Retain disposable workspaces and Jobs for seven days, raw Run payloads for 30
  days after WorkItem closure, and sealed evidence indefinitely. Cleanup is
  previewed, state-hashed, aggregate-aware, hold-aware, idempotent, and limited
  to server-derived rows and PHarness-labeled Kubernetes resources.
- Add the data lifecycle APIs and Settings surface, typed Product topology
  editor, and clear compacted-history states.

## Finance proving Product

Model one Finance Product with Services for web, database API, market-data API,
market-data ingestion, messaging, and the data store. Register these application
Repositories at immutable SHAs:

- `lward27/finance_app_database_service`
- `lward27/finance-frontend`
- `lward27/yfinance_wrapper`
- `lward27/scraper_manager`

Register `lward27/lucas_engineering` as a shared Repository. Bind only the exact
Finance chart, root-app integration, and Tekton automation paths to the Finance
Product; registration does not authorize source, GitOps, Argo, or cluster
mutation.

Progressive readiness is required. The Python repositories use the existing
Python 3.11 profile. The frontend remains honestly blocked until a later Node
runner milestone. Complete two supervised single-Repository WorkItems: ticker
normalization in the database API and startup configuration validation in
scraper-manager. Other Finance repositories are pinned read-only context.

## Acceptance and rollout

- Pass empty-database and production-copy migrations, product-model
  determinism and immutability tests, retention safety tests, API/UI/browser
  coverage, Rust/Clippy/UI/Helm/Kubernetes gates, and offline backup/restore.
- Deploy a safety release on the old claim, drain, take the second archive, then
  deploy the clean generation on a new retained claim through Argo.
- Recreate Finance exclusively through supported Product and Repository flows,
  onboard the Python proving path, complete both WorkItems through observed
  manual merge, and place their raw evidence under a 90-day hold.
- Validate retention in preview-only mode before enabling scheduled policy
  execution through GitOps. Archive-PVC deletion remains a separate explicit
  operation after the 14-day eligibility date.

## Boundaries

SQLite remains single-replica. This milestone does not add multi-Repository
mutation, DeliveryPlan, Connected Mode deployment, a Service dependency graph,
a Node runner, migration squashing, or physical legacy-table removal. Physical
schema convergence is the immediate follow-up milestone.
