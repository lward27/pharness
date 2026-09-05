# ASTRA M08: Separate deployments from the build they promote

Status: implemented and locally validated. Not deployed; M08 and
production promotion remain unaccepted.

Date: 2026-09-05. Source base: `44f4d9d092f54b1a7c485204403c278f5b8dd5a0`.
Branch: `codex/astra-staging-delivery`. Live runtime remains `48c77b7`; its verified
database is schema 53. Hosted creation remains disabled. This change does not
submit Finance work, build an image, update GitOps or grant production authority.

## Problem and resulting contract

The existing schema allowed only one DeploymentIntent and one GitOpsChangeSet per
PipelineIntent. That would force staging and production to overwrite each other,
or require a second build record without a second real build. Neither satisfies
the accepted source-to-runtime evidence contract.

Schema 54 adds a finite `delivery_stage` to DeploymentIntent: `legacy`, `staging`,
or `production`. One PipelineIntent may have one record per hosted environment;
each deployment owns its own GitOps change, Release and registry evidence. Both
environments refer to the same build. Existing records default to `legacy`; their
IDs, field values, decisions, artifacts and audit history are retained.

GitOpsChangeSet now permits an absent coding-run ID. Hosted build records already
use workflow/stage identities and need not have a coding run. Their required
WorkItem, WorkPlan, source ChangeSet, PipelineIntent, DeploymentIntent, session and
update-plan artifact remain linked. The database rejects a new legacy GitOps
change without coding-run provenance. Nullable coding provenance does not remove
source/build/effect evidence requirements from the controller.

Historical singular getters deliberately read only `legacy` records. Hosted
callers use the explicit deployment stage, deployment ID, or existing bounded
lists. Readers must not pick whichever environment happened to be inserted first.
The API returns `delivery_stage` on deployment records and a nullable `run_id` on
GitOps records. Legacy artifact projections return no legacy flow for hosted
records without a coding run; they do not borrow another run's artifacts.

Legacy deployment/GitOps creation, transition, evidence attachment, authority,
dispatch and writer-context routes reject hosted work. The new staging controller
and production approval path must supply their own bound workflow operations.
Creating a proposed record alone cannot count as build, deployment, approval,
runtime verification, or a successful release outcome.

## Migration and compatibility

This includes one departure from strictly additive SQL: removing SQLite's embedded
`UNIQUE(pipeline_intent_id)` constraint requires replacing `gitops_change_sets`
inside the migration transaction. The replacement copies every existing column
and row and changes uniqueness to `deployment_intent_id`. It does not delete
history or introduce a competing GitOps evidence table. Deployment stage itself
is an added column with a legacy default and a finite check constraint.

SQLx runs migration 54 atomically on the existing dedicated migration connection,
where foreign keys are disabled for historical table migrations. The runtime pool
enables foreign keys. The new migration checks the entire foreign-key graph before
committing and fails without discarding data if dangling lineage exists. An old
binary's default deployment insertion is rejected for a hosted WorkItem.

Before live deployment:

1. Record the exact source revision, current schema, Finance generation, and
   pre-migration database/archive identity. Preserve the archive and audit history.
2. Test the merged candidate against an independent copy of the current live
   schema-53 database, comparing existing values, integrity and foreign keys.
3. Deploy the compatible reader with hosted creation disabled. Observe the actual
   schema, live image identity and historical reads before enabling any new writes.
4. Record the immutable release containing migration 54 as the rollback floor
   when that schema is deployed, even before hosted writes. SQLx rejects an
   applied migration absent from the older binary's migration list. After hosted
   staging/production or nullable GitOps run references are written, old readers
   also lack the required semantics. Do not override migration-version checking.
   The onboarding v1alpha2 reader from PR #351 is included in this source base.

On failed migration, keep the original archive and failed evidence, diagnose the
specific failure, and leave hosted writes disabled. Do not force a migration
version, remove the foreign-key check, or erase offending rows. After schema 54
deployment, recovery must use a compatible reader; restoring an old database would
lose accepted history and is not an automatic rollback procedure.

## Validation and limits

[Validation receipt](ASTRA-M08-DELIVERY-RECORD-VALIDATION.json) records the exact
source and log hashes. The final combined run passed 337 tests: 276 API, one
administrative-tool and 60 store tests. The migration-floor assertion added during
review also passed its focused rerun. Clippy for both affected crates and all
targets, architecture boundaries, five dependency-parser checks, formatting and
47 relative documentation links passed. No UI code changed.

The initial checks caught an omitted public type export and two old callers still
assuming a non-null coding run; both were corrected. An initial API command used
`--lib` even though the API is a binary and ran no tests; the recorded 277 API/admin
results come from the corrected package command. Clippy then found one redundant
borrow, which was removed. Failed attempts are retained in the validation receipt;
none is counted as passing evidence.

The migration tests exercise a populated schema-53 graph, all original column
values across twelve related tables, repeated opening, rejection by the previous
migration list, interrupted transaction rollback/retry, and a dangling-reference
failure. Storage tests cover two distinct
environments on one build, independent GitOps/release/registry reads, duplicates,
immutable target/lineage identity, legacy/hosted separation and absent coding-run
provenance. API checks exercise readable records and rejected legacy mutations.

These are local contract checks with synthetic records. They are not real source,
build, GitOps, Argo or runtime evidence. This slice does not implement automatic
staging, production approval, promotion by digest, telemetry adjudication or
rollback. Those M08/M09 gates remain open. The live database-copy exercise is also
pending; a passing fixture migration does not replace it.
