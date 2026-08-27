# Clean database generation cutover

This runbook is the operational companion to the clean-state milestone. It
archives and replaces the active SQLite generation without copying operational
rows or deleting the former database.

## Non-negotiable boundaries

- Resolve the `lucas_engineering` context and `pharness` namespace before every
  command.
- Build the runtime, UI, and Python runner from one merged SHA and use only
  digest-pinned images.
- Never copy a live SQLite database file or its WAL. Use `pharness-admin data
  archive`, which opens the source read-only, performs `PRAGMA integrity_check`,
  and creates a consistent SQLite backup.
- Never reuse, format, resize down, or delete `pharness-api-data`.
- Keep both the former database claim and archive claim for at least 14 days.
- Archive deletion is a separate state-hashed API action after the clean
  generation is accepted; it is never part of this cutover.

## 1. Safety release on the former claim

Deploy the merged safety release with:

```yaml
api:
  operationalMode: normal
  databaseGeneration:
    id: dbgen_legacy_20260826
    purpose: legacy-safety-generation
    adoptExisting: true
    accepted: false
  persistence:
    create: true
    claimName: pharness-api-data
    existingClaim: ""
    retain: true
archivePersistence:
  enabled: true
  claimName: pharness-data-archive-legacy-20260826
  archivedGenerationId: dbgen_legacy_20260826
  retain: true
```

Verify Argo revision, Pod image IDs, `/health`, system readiness, database
generation, migration 49, and the new retained archive claim.

## 2. Pre-drain archive

Run the immutable runtime image from the safety release. Supply the accepted
historical WorkItem ID so its controller actions, outcomes, evidence,
observations, checks, and merge provenance are exported into the manifest.

```bash
scripts/pharness-data-archive-job.sh \
  registry.lucas.engineering/pharness-runtime@sha256:REPLACE \
  pharness-api-data \
  pharness-data-archive-legacy-20260826 \
  pre-drain \
  witem_REPLACE
```

Retain the Job and its single JSON log line as checksum evidence.

## 3. Drain and seal

Commit `api.operationalMode: draining` through GitOps. Confirm no new Products,
registrations, onboardings, WorkItems, stage starts, approvals, or external
effects are accepted. Allow already-dispatched callbacks to settle or cancel
them explicitly. Verify zero active PHarness Jobs, active Runs, and mounted
workspace claims. Then commit `api.operationalMode: read_only`.

Run a second archive to a distinct directory:

```bash
scripts/pharness-data-archive-job.sh \
  registry.lucas.engineering/pharness-runtime@sha256:REPLACE \
  pharness-api-data \
  pharness-data-archive-legacy-20260826 \
  post-drain \
  witem_REPLACE
```

Compare both manifest checksums, integrity status, migrations, generation, and
table counts. Restore the post-drain archive into a temporary claim and start a
read-only verification Pod before proceeding.

## 4. Clean retained generation

Create a new generation ID and claim in the release values:

```yaml
api:
  operationalMode: normal
  databaseGeneration:
    id: dbgen_REPLACE
    purpose: clean-finance-product-generation
    adoptExisting: false
    accepted: false
  persistence:
    create: true
    claimName: pharness-api-data-REPLACE
    existingClaim: ""
    retain: true
```

Keep `archivePersistence.archivedGenerationId` fixed to the former generation.
Deploy through Argo with the API `Recreate` strategy. Readiness must report the
new generation and refuse any accidental former-generation mount.

Verify that only the bootstrap Organization exists and that historical
WorkItems, Runs, approvals, and gates are absent. Recreate Product structure
through supported APIs/UI, never SQL.

## 5. Accept and record archives

After Finance topology, onboarding, and both acceptance WorkItems pass, set
`api.databaseGeneration.accepted: true` through GitOps. Create an ArchiveRecord
in the clean generation using the verified post-drain database and manifest
checksums, exact former database claim, exact archive claim, and deletion
eligibility at least 14 days after creation.

Generate a retention preview and confirm both 90-day WorkItem holds exclude the
accepted evidence. Scheduled execution stays preview-only until this check
passes.

At 14-day eligibility, inspect the server-authored archive deletion action. Do
not execute it during milestone acceptance.
