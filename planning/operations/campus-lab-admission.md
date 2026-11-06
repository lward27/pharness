# Campus lab admission and session API

Status: API, admission and reconciler are in place and tested against
`FakeCampusProvider` and the local journal. No real execution provider is
wired yet (LUC-125), so production deployments can list, inspect and stop
lab sessions but answer every launch with `503 labs_unavailable`.
Migration `0059` adds the admission tables (see
[Restore procedure](#restore-procedure)).

| Piece | Location |
| --- | --- |
| Lab catalog (static assignment seed) | `crates/pharness-api/src/app/campus/lab_catalog.rs` |
| Admission requests and reconciler | `crates/pharness-api/src/app/campus/labs.rs` |
| HTTP handlers | `crates/pharness-api/src/app/campus/lab_handlers.rs` |
| Admission records | `crates/pharness-store/src/campus_admission.rs` |
| Admission SQL | `crates/pharness-store/src/sqlite/campus_admission.rs` |
| Journal-backed capability authority | `crates/pharness-store/src/campus_authority.rs` |
| Schema | `crates/pharness-store/migrations/0059_campus_lab_admission.sql` |

## Scope decisions (M1)

- **One live lab session per student across the tenant**, not per course or
  per assignment. A second launch for any assignment is refused with
  `409 active_session_exists` and the id of the live session.
- **No queue.** When the tenant's slots or a course's limit are full, the
  launch is refused (`503 no_slots_available` or
  `503 course_capacity_reached`) and nothing is recorded.
- **Two-hour cap.** A session's lifetime is the effective policy TTL, capped
  at `max_session_seconds` (at most 7200). Expiry is enforced by the
  reconciler and, independently, by the capability authority, which treats
  an expired session as revoked on the next call.
- **Unlimited resets.** A reset tears the session down and only then admits
  the next generation of the same lab. Every generation keeps the lab's
  `lab_id` and `budget_id`.
- **Five slots by default** (`lab_slots`), registered at startup as
  `<tenant>-lab-slot-NN`, or under the ids listed in `lab_slot_ids` (which
  sets `lab_slots` to its length). The Kubernetes provider maps these ids
  to slot namespaces; the Campus PoC lists `campus-lab-slot-1..5`. Startup
  retires registered ids the catalog no longer lists and restores listed
  ones that were retired ([campus-teardown.md](campus-teardown.md#slot-ids)).
- **Static catalog.** Assignments come from a JSON file; there is no
  instructor UI, LMS sync or billing.

## Configuration

| Variable | Meaning |
| --- | --- |
| `PHARNESS_CAMPUS_LABS_FILE` | Path to the lab catalog. Unset disables the lab routes (`503 labs_unavailable`). |
| `PHARNESS_CAMPUS_LAB_PROVIDER` | `none` (default), `fake` or `kubernetes`. `fake` is accepted only with `PHARNESS_CAMPUS_DEV_ALLOW_INSECURE_LOOPBACK_HTTP=true` and launches nothing real. `kubernetes` runs session Jobs in the lab slots ([campus-session-jobs.md](campus-session-jobs.md)). |
| `PHARNESS_CAMPUS_LAB_PROVIDER_CONFIG_FILE` | Required with, and only with, `kubernetes`: JSON `{"kubectl", "global_args", "provider"}`, where `global_args` selects the controller identity (for example `--kubeconfig=...`) and `provider` is a `KubernetesCampusProviderConfig`. Startup fails unless `provider.slots` maps exactly the catalog's slot ids and every assignment's effective policy fits a slot. Example: `deploy/helm/pharness-campus/files/lab-provider.json`. |

Catalog (`pharness.dev/campus-lab-catalog/v1alpha1`, at most 1 MiB, unknown
fields rejected):

```json
{
  "schema_version": "pharness.dev/campus-lab-catalog/v1alpha1",
  "lab_slots": 5,
  "max_active_per_course": 5,
  "max_session_seconds": 7200,
  "recorded_qualifications": [],
  "profiles": [ { "...": "a full campus profile document" } ],
  "policy_layers": [ { "authority": "platform", "...": "" }, { "authority": "institution", "...": "" } ],
  "assignments": [
    { "course_id": "cs101", "assignment_id": "lab-1", "title": "Intro lab",
      "profile_id": "campus-coding", "assignment_layer": null }
  ]
}
```

Every profile and layer is validated and every assignment's effective policy
is composed at startup; any error stops the process. `lab_slots` is 1-64,
`max_active_per_course` defaults to `lab_slots` and cannot exceed it,
`max_session_seconds` is 300-7200. A gated profile (security) composes only
when its qualification is listed in `recorded_qualifications`.

## Routes

All routes need a Campus session cookie; `POST` also needs the CSRF header
and a same-origin `Origin`. Bodies accept only the fields shown; any other
field (image, namespace, credentials, profile, tenant, subject, expiry,
resources) is refused with `400 invalid_request` before anything is read
or written.

| Method and path | Who | Body | Success |
| --- | --- | --- | --- |
| `GET /api/campus/courses/:course_id/assignments` | course members, tenant admins | none | `200 { course_id, assignments: [{ assignment_id, title, profile {id, version, kind}, session_seconds, launchable, session }] }` |
| `POST /api/campus/courses/:course_id/assignments/:assignment_id/launch` | course members (student or instructor) | `{ "idempotency_key"? }` or `Idempotency-Key` header | `201 { admission: "created", session }`, `200` with `"existing"` (live session returned) or `"replayed"` (key seen before) |
| `GET /api/campus/lab-sessions` | any session | none | `200 { sessions }`, the caller's own, newest first |
| `GET /api/campus/lab-sessions/:session_id` | owner, course instructors, tenant admins | none | `200 { session }` |
| `POST /api/campus/lab-sessions/:session_id/stop` | owner, course instructors, tenant admins | `{}` or empty | `202 { session }`, `200` if already terminal |
| `POST /api/campus/lab-sessions/:session_id/reset` | owner, while still a course member | `{}` or empty | `202 { reset: "pending", session }` while teardown runs, `201 { reset: "created", session }` for a terminal session, `200 { reset: "replayed" }` for a reset already answered |
| `GET /api/campus/courses/:course_id/lab-sessions` | course instructors, tenant admins | none | `200 { sessions }`, live sessions with `owner {issuer, subject}` |

A session view contains `session_id, course_id, assignment_id, lab_id,
generation, status, state, stop_requested, current, reset_pending,
reset_from_session_id, profile {id, version}, error, created_at, updated_at,
expires_at, terminated_at`. `status` is one of `provisioning`, `ready`,
`stopping`, `cleanup_blocked`, `stopped`, `failed`. `error` is `null` or a
fixed `{code, message}`: `provision_failed`, `workspace_lost`,
`launch_denied`, `lab_failed`, `cleanup_blocked`. Provider reasons, workspace
ids, slots, operation keys and budget ids are never returned; they stay in
the journal and logs.

Errors are `{ "error": message, "code": code, "session_id"? }`:

| Status | Code | When |
| --- | --- | --- |
| 400 | `invalid_request` | unknown field or malformed body |
| 400 | `invalid_idempotency_key` | key not 1-128 of `[A-Za-z0-9._:-]`, or header and body differ |
| 403 | none | not a member of the course (launch, reset) or not staff (course sessions) |
| 404 | `assignment_not_found` | not in the catalog, or withdrawn (reset) |
| 404 | `session_not_found` | unknown session, or one the caller may not see |
| 409 | `active_session_exists` | the student already has a live session (`session_id` names it) |
| 409 | `idempotency_key_reused` | key first used for another assignment |
| 409 | `reset_superseded` | reset of a session that is no longer the lab's current one |
| 409 | `session_changed` | a concurrent change won; retry |
| 503 | `no_slots_available` | every tenant slot is in use |
| 503 | `course_capacity_reached` | the course limit is reached while the tenant has room |
| 503 | `labs_unavailable` | no catalog, or no provider for launch and reset |

## Admission

`SqliteStore::admit_campus_lab_launch` runs in one `BEGIN IMMEDIATE`
transaction:

1. A known idempotency key `(tenant, issuer, subject, key)` answers with the
   session it first produced, or `idempotency_key_reused` for another
   assignment.
2. The lab `(tenant, course, assignment, issuer, subject)` is found or
   created with a permanent `budget_id`.
3. A live current session of the lab is returned instead of a new one.
4. Limits are counted from non-terminal journal rows plus labs whose reset
   waits for admission: the identity (1), then the tenant
   (`min(lab_slots, registered slots)`), then the course.
5. The journal session, its generation row, the lab's new current session,
   the key and a `campus.lab.admitted` event are written.

A rejection writes nothing. SQLite admits one writer at a time and every
check reads inside the writing transaction, so parallel or retried launches
cannot exceed a limit or give a lab two live sessions.

Handlers never call the provider. They write the journal and wake the
reconciler (`CampusLabs::reconcile`, every 5 s and on wake), which steps each
live session with `advance_campus_lab_session` and admits the next
generation for labs whose reset finished tearing down. A reset that can no
longer be admitted (assignment withdrawn, membership removed, capacity
reduced) is cleared instead of holding capacity.

`JournalCampusAuthoritySource` answers the capability broker from the
journal: `Active` only while the session is `ready`, its desired state is
`ready`, it is not revoked and not expired, and the assignment's current
effective policy hash and profile equal the ones the session launched with.
Under the same conditions a session that is still `provisioning` reports
`Provisioning` (LUC-133): the Kubernetes provider may launch its workload,
while the capability broker, which grants only `Active`, still refuses it.

## Restore procedure

Migration `0059` only creates tables, indexes and triggers; it alters no
existing table. The migration tests open a schema-58 database with journal
rows, apply `0059`, and check that every existing row and schema object is
unchanged; an interrupted `0059` rolls back and can be retried. The previous
runtime refuses a migrated database with `VersionMissing(59)`, so rollback is
a restore of the pre-release archive as described in
[`campus-session-journal.md`](campus-session-journal.md#restore-procedure).
Labs, generations and launch keys created after the archive are lost.

## Residual risks

- No real provider: launches are refused in production until LUC-125 plugs
  a `CampusExecutionProvider` into `CampusLabs::new`.
- The reconciler runs inside the single API process. A second API replica
  would be safe (leases and fencing) but would duplicate observation calls.
- `cleanup_blocked` keeps its slot and, if a reset is pending, keeps the
  reset waiting until an operator runs `retry_campus_lab_cleanup`. There is
  no alert yet.
- The catalog is read at startup only. Changing an assignment's profile or
  layers needs a restart and revokes the authority of sessions launched
  under the old policy hash.
- Staff can stop any session of their course. Tenant admins who are not
  course members can see and stop sessions but cannot launch.
- Idempotency keys and labs are kept forever; retention needs a migration.
