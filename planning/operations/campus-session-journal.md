# Campus lab session journal

Status: driven by the Campus lab API and its in-process reconciler
([`campus-lab-admission.md`](campus-lab-admission.md), LUC-126). No real
execution provider is wired yet, so in production nothing is launched.
Migration `0058` adds the tables, so a release that contains it also changes
the database schema (see [Restore procedure](#restore-procedure)).

The journal is the durable local state machine for Campus lab sessions: one
student workspace, launched under a Campus profile, in one lab slot. It is
separate from SDLC runs and WorkItems and from Campus browser sessions
(`campus_sessions`, LUC-117). It lives in the existing SQLite database
(ADR 10, single writer).

| Piece | Location |
| --- | --- |
| States, transition table, provider trait, fake provider | `crates/pharness-workspaces/src/campus_lab_session.rs` |
| Records and typed journal errors | `crates/pharness-store/src/campus_journal.rs` |
| SQL and fencing | `crates/pharness-store/src/sqlite/campus_journal.rs` |
| One-step driver | `crates/pharness-store/src/campus_driver.rs` |
| Schema | `crates/pharness-store/migrations/0058_campus_session_journal.sql` |

## States

| State | Holds slot or resources | Terminal | Next states |
| --- | --- | --- | --- |
| `requested` | no | no | `authorizing`, `terminated` |
| `authorizing` | no | no | `provisioning`, `failed`, `terminated` |
| `provisioning` | slot; resources may exist | no | `ready`, `stopping` |
| `ready` | slot and resources | no | `stopping` |
| `stopping` | slot and resources; access revoked | no | `cleaning` |
| `cleaning` | slot; resources may remain | no | `terminated`, `failed`, `cleanup_blocked` |
| `cleanup_blocked` | slot; resources may remain; quarantined | no | `cleaning` |
| `terminated` | no | yes | none |
| `failed` | no | yes | none |

- Only `ready` grants student access.
- Every way out of a resource-holding state goes through `stopping` (revoke
  first, ADR 12) and `cleaning` (delete and confirm every owned resource).
  A terminal state therefore always means the provider confirmed that
  nothing remains.
- `failed` means the launch did not succeed: it was denied in
  `authorizing`, the provider rejected it, or the workspace was lost. A
  session that failed after taking a slot is still cleaned up first.
- The desired state only moves from `ready` to `terminated` (a stop request
  or absolute expiry). The observed state is the provider's last report.

The table is `CampusLabSessionState::successors`. Migration `0058` seeds the
same pairs into `campus_session_transitions`, and a trigger rejects any
other state change, including raw SQL. A test keeps the two identical.

## Tables

- `campus_sessions_journal`: one row per lab session. It holds the identity
  tuple `(tenant_id, issuer, subject)` (a foreign key to
  `campus_identities`), the course, the profile id, version and hash, the
  state, desired and observed state, `generation`, the slot and slot
  generation, the workspace id, failure and blocked reasons, revocation,
  quarantine and termination times, and the absolute expiry.
- `campus_session_operations`: one row per provider side effect
  (`provision` or `delete`). `op_key` is the primary key and the provider
  idempotency key. Each row records its kind, the session and slot
  generations, its status, attempt count and lease (owner, expiry, fence).
  At most one operation per session is pending.
- `campus_session_events`: the append-only audit log and outbox. `sequence`
  orders it, and `published_at` is the only column that may change, once.
- `campus_lab_slots`: one row per lab slot, with its current occupant and
  `occupant_generation`.

## Fencing

- **Session generation.** Every state change advances `generation` by
  exactly one, and every write compares and sets on `(session_id,
  generation)`. Observations and stop requests do not advance it, so an
  in-flight operation can still record what it did before the stop is acted
  on.
- **Slot generation.** A free slot that gets an occupant advances
  `occupant_generation` by one. A slot can only change occupant by being
  freed first. Provider resources carry `(session_id, slot_id,
  slot_generation)`, and completing an operation or freeing the slot
  requires that exact occupancy. A worker holding an earlier occupancy
  therefore cannot change a later occupant, in the journal or at the
  provider.
- **Lease fence.** Leasing an operation advances `lease_fence` and
  `attempt_count`. Completing it requires the latest fence and an unexpired
  lease. An expired holder cannot write even if nobody has taken over.
- Every journal write runs under `BEGIN IMMEDIATE`, so concurrent workers
  serialize on SQLite's write lock instead of failing a lock upgrade.

## Crash recovery

`advance_campus_lab_session` makes at most one provider call per step:

1. Load the session from the journal.
2. Either commit the transition, its new leased operation and its audit
   event in one transaction, or lease the existing pending operation.
3. Call the provider with `op_key`.
4. Commit the result under the lease fence, or release the lease if the
   result is inconclusive.

| Crash or fault | Journal afterwards | What resumes it |
| --- | --- | --- |
| Before step 2 commits | unchanged | any worker; the step repeats |
| After step 2, before the provider call | pending operation, live lease | any worker after the lease expires; same `op_key` |
| After the provider side effect, before step 4 | pending operation; the workspace exists but is not recorded | any worker after the lease expires; the provider returns the same workspace for the same `op_key` |
| Part way through a delete | `cleaning`, pending delete | any worker after the lease expires; delete is idempotent and is confirmed with `list_owned` |
| A slow worker finishes after its lease expired | already advanced by another worker | nothing; its write fails with `Fenced` |
| Delete unreachable or blocked, or deletion cannot be confirmed | `cleanup_blocked`; slot kept | `retry_campus_lab_cleanup` (operator or later automation) |

Duplicate deliveries are harmless. Creating a session is idempotent on
`session_id` with identical content. A second worker sees `LeaseHeld` while a
lease is live, or is fenced if the session already moved. Restarted
processes must use a new worker id; they wait for the old lease to expire.

Connectivity loss (ADR 18): with `central_authority = false` the driver
denies launches in `authorizing` (`failed`, reason
`central authority is unavailable; new launches are denied`). Sessions that
already hold a slot keep running and being observed, and an unreachable
provider is only recorded as observed `unreachable`. At `expires_at` the
driver requests a stop and tears the session down, with or without central
authority.

Teardown (ADR 12): `stopping` records `revoked_at` before any delete. A
`cleanup_blocked` session stays listed by
`list_quarantined_campus_lab_sessions`, keeps its slot, and is never
reported as terminated while its resources cannot be confirmed gone.
`find_campus_lab_orphans` compares a provider's `list_owned` inventory with
the journal and reports resources of unknown sessions, terminal sessions and
stale slot occupancies.

## Provider contract

`CampusExecutionProvider` has `provision(op_key, spec)`, `observe(workload)`,
`delete(op_key, workload)` and `list_owned()`. An implementation must:

- treat `op_key` as an idempotency key: the same key and spec return the
  same workspace, and a reused key with a different spec is rejected;
- label every resource with the full workload `(session_id, slot_id,
  slot_generation)` and `op_key`, and act only on exact label matches;
- never create anything for a workload after `delete` was called for it;
- return `Deleted` only once nothing labelled with the workload remains,
  and report an unreachable node as `Unreachable`, never as deleted.

`FakeCampusProvider` implements this in memory, with one-shot faults at
`BeforeProvision`, `AfterProvision`, `BeforeDelete`, `MidDelete`, `Observe`
and `ListOwned` (pause, unreachable, blocked, reject, in progress). Aborting
a paused call simulates a crash at that point. Releasing it simulates a
slow, stale worker.

## Restore procedure

Migration `0058` only creates tables, indexes and triggers. It does not
alter, rebuild or copy any existing table. The migration test opens a
schema-57 database with SDLC and Campus identity rows, applies `0058`, and
checks that every pre-existing table, row and schema object is unchanged and
that `foreign_key_check` and `integrity_check` are clean.

As with every migrating release, the previous runtime cannot open a
migrated database: SQLx reports `VersionMissing(58)`. Rollback is therefore
a restore, never an in-place downgrade.

1. `scripts/pharness-release.sh` archives the live database before the pin
   PR whenever the release adds a file under
   `crates/pharness-store/migrations`. A release containing `0058` triggers
   it. Keep the `ARCHIVE.json` it prints (archive path and digests).
2. To roll back, restore that archive into a new claim with the previous
   runtime, following
   [`runbooks/sqlite-backup-and-restore.md`](runbooks/sqlite-backup-and-restore.md)
   section 3. The archive is at schema 57 and contains every SDLC row as of
   the archive time.
3. Rolling forward again applies `0058` to the restored database; it is the
   same additive upgrade.

Campus lab sessions created after the archive are lost on restore. Before
rolling back, list sessions in a resource-holding state and, if the
provider is real, tear them down or record them for manual cleanup. Their
resources would otherwise become orphans that the restored database does not
know about. While nothing calls the journal this list is empty.

## Residual risks

- The API, quotas and reconciler (LUC-126) call the journal, but the real
  provider (LUC-125) does not exist yet, so production launches are refused
  and the journal is exercised end to end only by tests.
- A provider call can outlive its lease. Journal writes are fenced, but the
  call itself is not; the real provider must enforce the
  no-create-after-delete rule and exact slot-generation labels
  server-side (for example through admission control on the slot), or a
  late call could create an orphan. `find_campus_lab_orphans` detects such
  resources but nothing deletes them yet.
- Fencing relies on unique worker ids and on clocks that agree within the
  lease margin; `now` is supplied by the caller.
- `cleanup_blocked` holds its slot indefinitely. Capacity is lost until an
  operator retries, which is deliberate but needs an alert once a real
  provider exists.
- The outbox has no publisher. `published_at` is set only by
  `mark_campus_lab_events_published`.
- Journal and event rows are never deleted, and a trigger blocks event
  deletion. A future retention policy needs its own migration.
- Absolute expiry is enforced when a session is stepped. The LUC-126
  reconciler steps every live session every few seconds, and the
  journal-backed capability authority treats an expired session as revoked
  regardless; LUC-133 owns the remaining revocation work.
