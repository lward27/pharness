# SQLite backup and restore

PHarness keeps all control-plane state in one SQLite database on the
`pharness-api` PersistentVolumeClaim. This runbook covers the scheduled
backups the chart can produce, how to verify them, and how to bring a
replacement API up from one. It complements
`clean-database-generation-cutover.md`, which handles planned generation
changes rather than recovery.

## Boundaries

- Resolve the `lucas_engineering` context and `pharness` namespace before
  every command.
- Never copy `pharness.db`, `pharness.db-wal`, or `pharness.db-shm` with `cp`,
  `kubectl cp`, or a volume snapshot while the API is running. Only
  `pharness-admin data archive` produces a consistent copy (`VACUUM INTO` from
  a read-only connection with an integrity check on both sides).
- Never restore over the live claim. Restore into a new claim and point a new
  release at it so the previous claim remains available for comparison.
- A restore is a generation change: the restored database carries the
  generation ID it was archived with. The release that mounts it must either
  keep that ID with `adoptExisting: true`, or go through the cutover runbook.

## What the chart provides

`backup.enabled: true` renders:

- a retained `PersistentVolumeClaim` (`backup.persistence.claimName`) labelled
  with the active `api.databaseGeneration.id`;
- a `CronJob/pharness-backup` that runs
  `pharness-admin data archive --database /data/pharness.db --output-dir
  /archive/<prefix>/<UTC stamp>` from the exact runtime image digest the API
  runs. Each run writes `pharness.db` and `manifest.json` into a new
  directory and removes directories older than `backup.retainDays`.

The CronJob mounts the active claim writable because WAL readers need the
`-shm` file. The archive connection itself is read-only and never writes to
the source database. With `local-path` storage the Job is scheduled onto the
node that holds the volume.

Alerting for the backup Jobs (`PharnessBackupJobFailed`,
`PharnessBackupMissing`) and for the control plane lives in
`deploy/helm/pharness/files/prometheus-rules.yaml`. Render it as a
`PrometheusRule` with `api.metrics.prometheusRule.enabled: true`, or load the
same file into a Prometheus that does not use the operator.

## 1. Confirm backups are being produced

```bash
kubectl --context lucas_engineering -n pharness get cronjob pharness-backup
kubectl --context lucas_engineering -n pharness get jobs \
  -l app.kubernetes.io/component=data-archive --sort-by=.status.startTime
kubectl --context lucas_engineering -n pharness logs job/<latest pharness-backup job>
```

The log line is JSON with `database`, `manifest`, `database_sha256`, and
`manifest_sha256`. Keep the two digests with the incident or change record
when a backup is later used.

To run an archive outside the schedule:

```bash
kubectl --context lucas_engineering -n pharness create job \
  --from=cronjob/pharness-backup pharness-backup-manual-$(date -u +%Y%m%d%H%M)
```

## 2. Inspect an archive without touching the API

Start a read-only inspection Pod from the runtime image that mounts only the
backup claim:

```bash
kubectl --context lucas_engineering -n pharness run pharness-backup-inspect \
  --rm -it --restart=Never \
  --image=registry.lucas.engineering/pharness-runtime@sha256:<api digest> \
  --overrides='{
    "spec": {
      "automountServiceAccountToken": false,
      "securityContext": {"runAsNonRoot": true, "runAsUser": 65532, "runAsGroup": 65532, "fsGroup": 65532},
      "containers": [{
        "name": "inspect",
        "image": "registry.lucas.engineering/pharness-runtime@sha256:<api digest>",
        "command": ["/bin/sh"], "stdin": true, "tty": true,
        "volumeMounts": [{"name": "archive", "mountPath": "/archive", "readOnly": true}]
      }],
      "volumes": [{"name": "archive", "persistentVolumeClaim": {"claimName": "pharness-api-backups-finance-20260827", "readOnly": true}}]
    }
  }'
```

Inside the Pod:

```sh
ls -1 /archive/nightly
cat /archive/nightly/<stamp>/manifest.json
sha256sum /archive/nightly/<stamp>/pharness.db   # must match archive_database_sha256
```

`manifest.json` records the database generation, the applied migrations, and
per-table row counts. Compare the generation ID with the live release before
planning a restore.

## 3. Restore into a new claim

1. Put the live API into `read_only` (or `draining` if runs must finish) so
   no further writes land on the claim you are replacing:

   ```yaml
   api:
     operationalMode: read_only
   ```

   Sync and confirm `/health` reports the new mode.

2. Create the replacement claim with the chart by setting a new
   `api.persistence.claimName` in a branch, but do not merge yet. Instead
   create the claim out-of-band so it exists before the restore Job:

   ```bash
   kubectl --context lucas_engineering -n pharness apply -f - <<'EOF_PVC'
   apiVersion: v1
   kind: PersistentVolumeClaim
   metadata:
     name: pharness-api-data-restore-<date>
     labels:
       app.kubernetes.io/part-of: pharness
       pharness.dev/data-generation: <generation id from manifest>
     annotations:
       helm.sh/resource-policy: keep
       argocd.argoproj.io/sync-options: Prune=false
   spec:
     accessModes: [ReadWriteOnce]
     storageClassName: local-path
     resources:
       requests:
         storage: 2Gi
   EOF_PVC
   ```

3. Copy the archived database into the new claim with a one-off Job. The
   archive file is a closed, vacuumed database, so a plain copy is correct
   here; only the live file must never be copied.

   ```bash
   kubectl --context lucas_engineering -n pharness create -f - <<'EOF_JOB'
   apiVersion: batch/v1
   kind: Job
   metadata:
     name: pharness-restore-<date>
   spec:
     backoffLimit: 0
     template:
       spec:
         automountServiceAccountToken: false
         restartPolicy: Never
         securityContext:
           runAsNonRoot: true
           runAsUser: 65532
           runAsGroup: 65532
           fsGroup: 65532
           seccompProfile: {type: RuntimeDefault}
         containers:
           - name: restore
             image: registry.lucas.engineering/pharness-runtime@sha256:<api digest>
             command: ["/bin/sh", "-euc"]
             args:
               - |
                 src=/archive/nightly/<stamp>
                 test ! -e /data/pharness.db
                 cp "$src/pharness.db" /data/pharness.db
                 echo "$(sed -n 's/.*"archive_database_sha256": *"sha256:\([0-9a-f]*\)".*/\1/p' "$src/manifest.json")  /data/pharness.db" | sha256sum -c -
             securityContext:
               allowPrivilegeEscalation: false
               readOnlyRootFilesystem: true
               capabilities: {drop: ["ALL"]}
             volumeMounts:
               - {name: archive, mountPath: /archive, readOnly: true}
               - {name: data, mountPath: /data}
         volumes:
           - name: archive
             persistentVolumeClaim: {claimName: pharness-api-backups-finance-20260827, readOnly: true}
           - name: data
             persistentVolumeClaim: {claimName: pharness-api-data-restore-<date>}
   EOF_JOB
   kubectl --context lucas_engineering -n pharness wait --for=condition=complete --timeout=10m job/pharness-restore-<date>
   kubectl --context lucas_engineering -n pharness logs job/pharness-restore-<date>
   ```

   The Job fails if the destination already holds a database or if the
   checksum does not match the manifest.

4. Point the release at the restored claim and keep the archived generation:

   ```yaml
   api:
     operationalMode: normal
     databaseGeneration:
       id: <generation id from manifest>
       adoptExisting: true
     persistence:
       create: false
       claimName: ""
       existingClaim: pharness-api-data-restore-<date>
   backup:
     persistence:
       claimName: pharness-api-backups-restore-<date>
   ```

   Giving the backup claim a new name keeps the pre-restore archives intact
   on the old claim. Merge, sync, and verify Argo revision, Pod image IDs,
   `/health`, and `/api/system/readiness`.

5. Confirm the scrape on the metrics listener resumes
   (`pharness_controller_last_tick_age_seconds` falls below 30s) and that
   `pharness_hosted_reconciliations{condition="blocked"}` matches what the
   manifest's `table_counts` lead you to expect.

## 4. Work lost between the archive and the incident

Everything written after the archive stamp is absent from the restored
database: WorkItems, approvals, stage executions, and controller claims. Use
the previous claim (still retained) to enumerate what happened after the
stamp before deciding whether to replay any action. Hosted operations are
idempotent by `(work_item_id, action, input_hash)`, so re-issuing a
reconcile on a restored WorkItem is safe; approvals are not replayed
automatically and need a fresh operator decision.

## 5. Rehearsal

Rehearse the restore quarterly against a disposable namespace:

1. Install the chart with `backup.enabled: true`, a short schedule, and a
   throwaway database claim.
2. Create a WorkItem, wait for one archive, then delete the API Deployment.
3. Follow section 3 into a new claim and confirm the WorkItem is present and
   `pharness_build_info` reports the expected revision.

Record the digests, generation ID, and elapsed time in a note under
`planning/operations/rehearsals/` (first one:
[`sqlite-restore-20261018.md`](../rehearsals/sqlite-restore-20261018.md)).
