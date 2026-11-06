# Campus teardown, expiry and orphan quarantine

LUC-133. Every Campus lab session ends at an absolute expiry. A stop or an
expiry revokes access first and removes resources afterwards. A cleanup that
cannot be confirmed is quarantined, never forced. A janitor reconciles what
the execution provider owns against the journal. Migration `0061` adds slot
holds and teardown hook records (see [Restore procedure](#restore-procedure)).

| Piece | Location |
| --- | --- |
| Teardown hook trait | `crates/pharness-core/src/campus_lab_session.rs` (`CampusTeardownHook`) |
| Delete confirmation and hooks in the driver | `crates/pharness-store/src/campus_driver.rs` (`confirm_teardown`) |
| Backoff, slot holds, janitor | `crates/pharness-store/src/campus_teardown.rs` |
| Slot sync, hold, metrics SQL | `crates/pharness-store/src/sqlite/campus_teardown.rs` |
| Schema | `crates/pharness-store/migrations/0061_campus_teardown.sql` |
| Reconciler order, janitor interval | `crates/pharness-api/src/app/campus/labs.rs` |
| Metrics | `crates/pharness-api/src/app/metrics.rs` (`render_campus`) |
| Alert rules | `deploy/helm/pharness/files/prometheus-rules.yaml` (group `pharness-campus`) |
| Cluster qualification | `scripts/campus-teardown-cluster-test.sh`, `crates/pharness-api/src/app/campus/tests/teardown_live.rs` |

## Absolute expiry

`expires_at` is fixed at admission (launch time plus the assignment's
session lifetime, at most 7200 s) and nothing changes it. Restarts, duplicate
stops, retried provisions and resets keep it; the fake 100-cycle test and the
live cycles assert it after every teardown. Expiry takes effect in three
places, independently:

1. At `expires_at` the journal authority source reports `Revoked` and the
   broker refuses every token of the session (`SessionExpired`), with no
   reconciler involved.
2. The Kubernetes Job has `activeDeadlineSeconds` set to the remaining
   lifetime, so the kubelet stops the Pod even if PHarness is down.
3. Each reconciler sweep (every 5 s) steps expired sessions first, before
   any other work, recording the stop and revoking the session's tokens in
   the journal.

## Revoke first, then remove

A stop request revokes the session's broker tokens in its own transaction,
before any reconciler runs; tokens issued later are impossible because
issuance takes the same write lock. A sweep that moves any session wakes
open browser terminals, which re-check their session at once
([campus-browser-terminal.md](campus-browser-terminal.md)). Only then does
the reconciler delete the workload.

The delete counts as done only when the provider reports it deleted **and**
its owned-resource inventory no longer lists anything of that workload.
Then every configured teardown hook runs (`CampusTeardownHook`, for
anything a session holds outside the provider, such as an external volume
or an attachment). Each hook that succeeds is recorded in
`campus_teardown_hook_runs`, so a replayed or retried cleanup never calls it
again; a crash between a hook's effect and its record repeats the call, so
hooks must be idempotent per workload. Only after the last hook does the
session become `terminated` and its slot free.

## Blocked cleanup and backoff

A delete the provider cannot confirm leaves the session `cleanup_blocked`
with its reason, still holding its slot. Causes include:

- a Pod on an unreachable or lost node that has not confirmed termination
  within `stuck_termination_seconds` (120 s), reported `unreachable`;
- a Job or Pod held by a finalizer, reported `blocked`;
- an inventory the API server will not return;
- a teardown hook that reports its backend unreachable.

The controller never force-deletes, never sets a zero grace period and never
strips a finalizer. The reconciler retries a blocked cleanup after 30 s,
then 60, 120, 240, 480 and 600 s (capped). Each retry repeats the same
idempotent delete. A retry that still blocks stays quarantined with the next
wait, and one that succeeds finishes the teardown and frees the slot.

## Orphan janitor and slot quarantine

Every 30 s the reconciler lists every resource the provider owns (Jobs,
Pods, Secrets, tombstones in the configured slots) and classifies each
workload against the journal. A workload no live occupancy accounts for
(another or a lost database, a session the journal already finished, a
stale generation) is an orphan:

1. Its slot gets a `quarantined` hold at once, so nothing new is placed next
   to it and it does not count toward admission capacity.
2. After `CAMPUS_ORPHAN_GRACE_SECONDS` (60 s), measured by the janitor of
   this process, the janitor deletes exactly that workload. The delete key
   is deterministic per workload (`campus-janitor/<session>/<slot>/<gen>`),
   and the delete is fenced on session, slot and generation, so it cannot
   touch a live occupant.
3. Once the slot's inventory comes back clean the janitor releases its own
   hold. A provider that cannot list its inventory releases nothing.

Workloads in a slot this tenant does not own are counted as `foreign` and
never touched. Holds are append-only (released once, never deleted), so
`campus_lab_slot_holds` is also the audit trail. The janitor's first
sightings are kept in memory, so a restarted process waits a fresh grace
period; that only delays a delete.

## Slot ids

The journal's slot ids come from the catalog: `lab_slot_ids`, or by
default `<tenant>-lab-slot-NN` (LUC-137). The Kubernetes provider maps each
id to a slot namespace, and admission requires every object's slot label to
equal the namespace's `pharness.dev/campus-slot` label, so the Campus PoC
catalog lists the M1 namespaces `campus-lab-slot-1..5`
([campus-poc-deployment.md](campus-poc-deployment.md)). At startup, and on
every janitor pass, `sync_campus_lab_slots` makes the registry match the
catalog. Listed ids that are missing are registered. A registered id the
catalog no longer lists gets a `retired` hold: an occupant keeps running
until it ends, and the slot is never reused or counted as capacity. A
retired id that is listed again is restored. A database that ran with the
default ids before switching to the M1 ids therefore keeps its old rows,
retired, with no further effect. The API logs what the sync changed at
startup.

## Provisioning authority

The journal authority source now reports `Provisioning` for a session that
is being launched (state `provisioning`, desired `ready`, not revoked, not
expired). The Kubernetes provider launches such a session, while the
capability broker grants only `Active` (state `ready`). Before this change
the provider refused every session the reconciler handed it ("provision
rejected: the session authority is revoked"), because the authority was
`Revoked` until the session was `ready`. Earlier live tests had not shown
this, because they drove sessions to `ready` with the fake provider and
called the Kubernetes provider directly. The Campus PoC deployment
(LUC-137) builds the Kubernetes provider with this authority source, so it
depends on this change.

## Metrics and alerts

Exported by `/metrics` from the journal on every scrape:

| Metric | Meaning |
| --- | --- |
| `pharness_campus_lab_sessions{tenant,state}` | non-terminal sessions by state, including `cleanup_blocked` |
| `pharness_campus_lab_slots{tenant,condition}` | slots by condition: `free`, `occupied`, `cleanup_blocked`, `quarantined`, `retired` |
| `pharness_campus_lab_oldest_quarantine_age_seconds{tenant}` | age of the oldest blocked cleanup or quarantined slot; NaN when none |
| `pharness_campus_lab_sessions_expired_live{tenant}` | sessions past `expires_at` that are neither terminal nor quarantined |
| `pharness_campus_lab_janitor_last_pass_timestamp_seconds` | last janitor pass of this process; 0 before the first or without a provider |
| `pharness_campus_lab_janitor_orphaned_workloads` | orphans at the last pass |
| `pharness_campus_lab_janitor_errors_total` | janitor inventory, delete and journal errors |

Alert rules (group `pharness-campus` in
`deploy/helm/pharness/files/prometheus-rules.yaml`):

| Alert | Expression | For | Severity |
| --- | --- | --- | --- |
| `PharnessCampusCleanupBlocked` | `pharness_campus_lab_slots{condition="cleanup_blocked"} > 0` | 5m | warning |
| `PharnessCampusSlotQuarantined` | `pharness_campus_lab_slots{condition="quarantined"} > 0` | 10m | warning |
| `PharnessCampusQuarantineAging` | `pharness_campus_lab_oldest_quarantine_age_seconds > 3600` | 0m | critical |
| `PharnessCampusExpiredSessionsLive` | `pharness_campus_lab_sessions_expired_live > 0` | 5m | critical |
| `PharnessCampusJanitorStalled` | last pass timestamp set and older than 300 s | 5m | warning |
| `PharnessCampusJanitorErrors` | `increase(pharness_campus_lab_janitor_errors_total[15m]) > 3` | 0m | warning |

A PrometheusRule object is not needed on this cluster. It runs the community
Prometheus chart without the operator, so the same groups are mirrored into
`charts/prometheus/values.yaml` (`serverFiles.alerting_rules.yml`) of the
GitOps repository. The pharness chart's `monitoring.yaml` renders a
PrometheusRule from the same file for clusters that run the operator.
`PharnessCampusJanitorStalled` cannot tell a janitor that never started from
a process without a provider; the timestamp stays 0 in both cases.

## Runbook

**`cleanup_blocked`** (alert `PharnessCampusCleanupBlocked`). The student
already has no access, because tokens were revoked at stop or expiry. Read
the reason from the journal (read-only:
`SELECT session_id, slot_id, blocked_reason, quarantined_at FROM
campus_sessions_journal WHERE state = 'cleanup_blocked'`). Then fix the
cause in the cluster:

- for an unreachable node, recover the node, or drain and delete the Node
  object, so the Pod is garbage-collected;
- for a finalizer, find the controller that owns it and let it finish, or
  remove that finalizer only as a deliberate operator decision;
- for an inventory error, restore the controller's list permission.

Do not edit the journal. The next retry (at most 10 minutes later) confirms
the deletion and frees the slot.

**Quarantined slot** (`PharnessCampusSlotQuarantined`). List the slot's
controller objects with
`kubectl -n <slot> get jobs.batch,pods,secrets,configmaps -l pharness.dev/campus-controller`
and read the hold reason in `campus_lab_slot_holds`. A slot stays
quarantined while the janitor's delete is blocked, for the same causes as
above. Once its inventory is clean, the next pass releases it.

**Expired sessions live** (`PharnessCampusExpiredSessionsLive`). The
reconciler is not running or every step fails. Check the API logs for
`campus lab` warnings and `pharness_campus_lab_janitor_last_pass_timestamp_seconds`.
Access is already refused by the broker and the authority source, and the
Job deadline still stops the Pod.

## Cluster qualification

```
scripts/campus-teardown-cluster-test.sh --chart <lucas_engineering>/charts/campus-lab \
  [--cycles 100] [--out <dir>] [--skip-stuck] [--skip-cycles]
```

It needs cluster-admin on `lucas_engineering` and impersonates the
controller (`system:serviceaccount:campus-poc:pharness-campus-controller`).
It requires the five slots to hold no controller objects and no Campus API
Pod (`app=pharness-campus-api` in `campus-poc`) to be running, because a
deployed API's janitor and the test's would each treat the other's sessions
as orphans. Then:

1. **Stuck finalizers and orphans.** In a probe namespace
   `campus-teardown-probe-1` (chart probe mode, labelled as slot
   `campus-lab-slot-1`), a session of a second journal is found by the first
   journal's janitor, which quarantines the slot at once and removes the
   session after the grace period. Then a session's Pod, and then its Job,
   is given an operator finalizer (`pharness.dev/luc-133-probe`) before the
   stop. The cleanup becomes `cleanup_blocked` with the expected reason and
   the finalizer stays in place. A backoff retry stays blocked. After the
   operator removes the finalizer, the next retry finishes and frees the
   slot.
2. **Cycles.** Batches of five sessions in `campus-lab-slot-1..5`, with both
   approved runner images, end in turn by stop, expiry, stop while still
   provisioning, and a duplicate stop followed by a reconciler restart
   (lease takeover). Each batch reads the broker token from inside the Pod,
   then measures when the broker refuses it and when the session is
   terminal with no Job, Pod or Secret left. At the end a janitor pass must
   find no orphan, no hold, no live session and five free slots.

Everything the run creates is deleted on exit, including its tombstones and
the probe namespace and bindings, and leftovers are printed.

## Results

Run on 2026-11-05 against `lucas_engineering` (Argo `campus-lab`
Synced/Healthy at `fa05e31`), with evidence in `/tmp/luc133-live-5`:
15 checks passed, 0 failed, and no probe namespace, probe binding or
controller object was left in any slot.

**Stuck finalizers and orphans** (311 s):

| Case | Result |
| --- | --- |
| Session of another journal in the slot | quarantined on the first janitor pass and not deleted; removed and the slot released 6 s after the grace period |
| Pod held by a finalizer | `cleanup_blocked` after 125 s: "Pod ... on node ubuntu-lucas-engineering-2 has not confirmed termination" (the node-loss signal); the 30 s retry stayed blocked; the finalizer was untouched; finished 6 s after the operator removed it |
| Job held by a finalizer | `cleanup_blocked` after 125 s: "Job ... is held by finalizers [\"pharness.dev/luc-133-probe\"]"; the retry stayed blocked; finished 6 s after removal |

**100 cycles** (20 batches of 5, both runner images, 10.4 minutes, 0 sweep
errors): every session ended `terminated` with its original `expires_at`
and every broker token revoked. The final janitor pass found no orphan, no
hold and no live session, and all five slots were free.

| Ending (5 batches each) | Ready | Broker refuses the token, max | Resources removed, max |
| --- | --- | --- | --- |
| Stop | 15-19 s | 6 ms | 14 s |
| Expire | 18-20 s | 1 ms | 14 s |
| Stop while provisioning | stopped after the first sweep, 5-6 s | 5 ms | 13 s |
| Duplicate stop, then restart | 19-20 s | 3171 ms | 13 s |

Over all 100 sessions, revocation took p50 4 ms, p95 2999 ms and max
3171 ms (limit 60 s), and removal took p50 13 s, p95 14 s and max 14 s
(limit 300 s). The restart batches measure revocation after one sweep that
the first reconciler runs before it is replaced. The tokens were already
revoked in the stop transaction, as the other batches show.

## Restore procedure

Migration `0061` only creates two tables, one index and four triggers; it
alters no existing table. The migration tests open a schema-60 database with
registered slots and an admitted session, apply `0061`, and check that every
existing row and schema object is unchanged, that only the new objects were
added, and that a hold on a pre-existing slot takes it out of admission.
An interrupted `0061` rolls back and can be retried. The previous runtime
refuses a migrated database with `VersionMissing(61)`, so rollback is a
restore of the pre-release archive as described in
[`campus-session-journal.md`](campus-session-journal.md#restore-procedure).
Slot holds and hook records written after the archive are lost. The janitor
re-quarantines any slot that still holds orphans on its first pass, and a
lost hook record repeats an idempotent hook.

## Residual risks

- The live tests run the journal, reconciler and broker in the test
  process with the controller identity impersonated, not the deployed
  Campus API in `campus-poc` (LUC-137), which builds the same provider from
  its configuration file. The deployed API needs this branch's
  `Provisioning` authority change before it can launch any session.
- The janitor's grace clock and its metrics are per process. A restart
  delays orphan deletion by one grace period, and two replicas would each
  report their own gauges.
- Revocation of browser terminals depends on the terminal registry's
  re-check (measured in LUC-134); this run measured broker refusal only.
- Node loss was simulated with a Pod whose termination never completes
  (a finalizer, past `stuck_termination_seconds`). No node was powered off.
