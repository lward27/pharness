# ASTRA M06: Durable controller implementation evidence

Status: continuous engineering progression integrated and locally tested; delivery integration, live recovery, and milestone acceptance open.
Observed 2026-09-05. Worktree `pharness-astra-controller`, branch
`codex/astra-autonomous-controller`, created from current main `db84b797` and
merged with tested M05 preparation at `e197497`, then current main `2249950` at
controller merge `3a2e369`. The deployed API is on source `2249950`, schema 0052
as of the 14:08 UTC live check. M06 is merged through source `48c77b7`; its new
runtime passed the isolated migration check below but remains undeployed.

## Implemented boundary

Migration 0053 adds scheduling records owned by existing hosted WorkItems. Legacy
source-only WorkItems are not enrolled. New hosted inserts and existing hosted rows
receive due times without any browser or GET request causing registration.

Atomic claims have persisted expiry and a monotonically increasing fence. An old
API claim cannot record dispatch or completion after expiry, replacement, pause,
resume or cancellation. Control changes are version-bound and audited. Cancellation
cannot be reversed into a new authorization.

Operation records preserve one action/input identity and references to existing
Runs or delivery intents. They do not replace those resources or copy their evidence.
A repeated operation returns its existing identity. References can be enriched but
cannot be rebound, and a completed result cannot be rewritten or reopened.

Resource locks support one coding operation and serialization by repository and
environment. Locks remain held when an API claim expires or an operation's outcome
is unknown; only a reconciled terminal operation releases them after dispatch. A pending
operation that has never entered its executor may relinquish capacity while paused
or cancelled; its immutable key set must be reacquired before it can execute. This prevents a
restart from being interpreted as permission to dispatch another external effect.

Hosted worker dispatch now records a hash of the intended Job manifest and checks
the exact deterministic name before creation. Existing Jobs must match every
requested value; Kubernetes defaults and status are permitted, while changed
images, credentials, command arguments, added containers/environment entries, and
terminating Jobs require intervention. A lost create acknowledgement triggers a
bounded re-read of that same Job. An uncertain hosted dispatch no longer seals its
Run as failed. Existing source-only dispatch retains its previous behavior.

## Validation

The full store suite passed **51 tests**, including five new tests covering
exclusive/expired claims, future due times, persistent locks, duplicate identity,
immutable results, pause/cancellation, and reopening a real on-disk SQLite database.
The five affected tests passed again after the final control-fencing correction.
Store Clippy passed for all targets with warnings denied. See
[validation evidence](ASTRA-M06-PERSISTENCE-VALIDATION.json) for exact source hashes,
log hashes, and the validation boundary. These counts overlap and are not additive.

The complete API/admin suite also passed **240 tests** after the worker-dispatch
change, including three new adapter regressions. The 32-test dispatch subset is
part of that total. With the store results this is **291 distinct tests**. API
Clippy, formatting and architecture checks pass. See
[dispatch recovery evidence](ASTRA-M06-DISPATCH-RECOVERY-VALIDATION.json). The
adapter uses a local fake Kubernetes executable with no cluster credentials.

The existing action endpoint now provides versioned pause, resume, and cancellation
for hosted work. It rejects routine manual advancement, preserves legacy actions,
and keeps reads effect-free. The public state explains that already-dispatched work
may finish while observation and authorized recovery continue. A process admission
lock also closes a race between simultaneous legacy and hosted dispatches; one API
replica alone did not serialize asynchronous capacity checks.

After these changes the complete API/admin suite passed **244 tests**. With the
unchanged 51 store tests, **295 distinct tests** passed. A 16-test hosted subset
is included in the API total. Clippy, formatting, and architecture checks passed.
Two unused-result warnings in test code were corrected before final Clippy. See
[control and admission evidence](ASTRA-M06-CONTROLS-AND-ADMISSION-VALIDATION.json).

## Continuous controller integration

The API now reconciles persisted hosted due times every 15 seconds in normal
operating mode. It keeps the existing 240-check unchanged-wait bound. Each pass
has a 60-second claim and a 45-second execution ceiling; a timeout retains any
uncertain operation. Disabling creation does not silently revoke existing work.
Maintenance operating modes do not start the scheduler.

The controller uses existing Planner, stage-chain, deterministic Test, one Repair,
and Verifier executors. It records the operation and pre-dispatch Run set first,
then reconciles the exact resulting Run and stage/context identity. It can adopt
an existing coherent Run after losing the dispatch acknowledgement, and only a
zero-consumption queued Run is eligible for the exact-Job retry path. Running or
partially initialized work is not blindly restarted. Hosted outcome callbacks
bring due times forward; they no longer dispatch the next stage themselves.

Automatic plan approval checks the exact sealed Planner output and revision.
ChangeSet approval checks current successful stage evidence and material hashes.
Changed revisions and unresolved contradictions stop automatic approval. Tool
approvals, budget increases, broad replanning, and production promotion do not
become automatic side effects of the old action rail.

All **301 distinct tests** passed: API 248, admin 1, store 52. The eight controller
and six store-controller checks are included in those totals. Final Clippy, format,
and architecture checks passed. The architecture check initially caught an import
cycle introduced during extraction; shared state now has its own ownership module,
and the final API suite was rerun after that correction. No checker exception or
size-limit increase was introduced. See
[integration validation](ASTRA-M06-CONTROLLER-INTEGRATION-VALIDATION.json).

## Preparation interruption recovery

The next implementation slice reconciles preparation Jobs using the same exact
manifest identity as coding Jobs. It preserves the existing Run, workspace,
preparation and bounded authorization after a lost dispatch acknowledgement.
A completed Job without a validated signed callback remains a failure; a Job's
status is not an environment verification result.

After signature, source, contract and runner validation, a successful hosted
callback records the preparation, Run environment and WorkItem snapshot pointer
in one SQLite transaction. An injected persistence failure proves those changes
roll back together. A late dispatch acknowledgement or failure callback cannot
reopen or overwrite an accepted result. The callback wakes the controller rather
than dispatching the next worker directly. Paused work may observe an existing Job
but cannot recreate an absent one. Historical partial state does not receive a
reconstructed signature.

All **304 distinct tests** passed after this slice: API 250, admin 1, store 53.
Clippy with warnings denied, formatting and architecture checks passed. The
three additional tests cover signed/duplicate callbacks, exact Job recovery,
and transactional rollback with late acknowledgements. These are local tests;
no M06 code or migration has been deployed. See
[preparation recovery validation](ASTRA-M06-PREPARATION-RECOVERY-VALIDATION.json).

## Terminal evidence recovery

Hosted terminal results now retain the original validated attempt and its recorded
consumption. The controller can finish local normalization after a restart without
another model call, Run, workspace or budget. Missing or inconsistent original
reports block recovery. Callback and reaper finalization share a local serialization
boundary, so a stale completion or failure cannot replace a terminal result.

A SQLite trigger injects interruption after Run completion and before stage sealing.
A newly connected store resumes the original Planner result without another plan
revision. Repeated normalization leaves the sealed outcome, Run, consumption and
WorkItem projection unchanged. Interrupted Verify finalization derives the same
ChangeSet and preserves later approval. Validation records use stable identities
bound to their complete facts and references. Immutable outcome protections remain
enabled throughout the tests.

The final API suite passes **252 tests**, plus one admin test. The unchanged store
retains its 53 passing tests from the preparation slice: **306 distinct tests**.
Clippy with warnings denied, formatting and architecture checks pass. See
[terminal normalization validation](ASTRA-M06-TERMINAL-NORMALIZATION-VALIDATION.json).
This remains undeployed controller implementation, not live recovery acceptance.

## Source publication dispatch boundary

Initial publication records its writer identity in the immutable authorization
and persists the current execution before creating a Job. The existing explicit
retry follows that same ordering. Dispatch acknowledgement reads the latest intent
so a fast callback is not overwritten. Hosted dispatch uncertainty retains its
writer or observer identity for reconciliation; it does not become permission to
allocate another operation.

Hosted source writer/observer Jobs use exact manifest hashes and the existing
lost-acknowledgement adapter. Deterministic tests prove repeated dispatch reuses
one Job, rejects a conflicting Job, and preserves the publication intent when
observation is unavailable. These tests exercise the control-plane boundary;
they do not prove GitHub source effects or automatic source merge.

All **254 API tests** and one admin test pass, with 53 unchanged passing store tests
(**308 distinct tests**). Clippy, formatting and architecture checks pass. See
[source dispatch validation](ASTRA-M06-SOURCE-DISPATCH-VALIDATION.json).

## Remaining implementation and acceptance

Incomplete multi-record pre-dispatch startup still needs interruption recovery. The source writer/observer dispatch boundary is prepared, but controller source
progression, remote-effect recovery and delivery adapters are not integrated into
this scheduler. Production approvals, rollback,
and terminal cancellation must close against those actual operations. A real
active-workflow restart and duplicate source/pipeline/deployment acceptance remain
open. The local fixtures are not a live autonomous Finance result.

Before applying 0053, publish and record a compatible rollback release that knows
both 0052 and 0053. This unpublished migration now preserves the intended lock set
independently from held locks. An older SQLx migrator rejects an unknown applied
migration. Preserve the Finance data generation, audit history and retention
policy; do not use a down migration or a database reset for recovery. The final
acceptance gates remain in
[M06](../../programs/autonomous-sdlc/ASTRA-06-DURABLE-AUTONOMOUS-CONTROLLER.md).

## Combined source validation (2026-09-05 14:18 UTC)

Source `f370505` includes the merged M05 compatible-reader release and M04 prompt
clarification through main `4c40b10c0b2f71ab92d464528145e178222a3368`. The complete
workspace passed **641 distinct top-level Rust tests**, workspace/all-target
Clippy with warnings denied, formatting, and architecture checks including five
parser regressions. Nested language-fixture output is excluded from the count.
[Validation manifest](ASTRA-M06-COMBINED-WORKSPACE-VALIDATION.json).

The live Finance database is now on schema 0052 under source `2249950`. Migration
0053 and this engineering controller remain undeployed. Source-publication
continuation, build/deployment integration, terminal cancellation and live recovery
acceptance remain open; source validation does not waive those gates. The next
release must retain a schema-53 compatible recovery image before applying 0053.

## Source integration and migration boundary

PR 332 merged on 2026-09-05 at 14:31 UTC as `ba8ce03e4dfd3df5815c897a69276858b53aacb2`.
Schema 0053 remains undeployed. The verified [pre-0053 snapshot](ASTRA-M06-DATABASE-ARCHIVE-VERIFIED.json)
contains the existing Finance generation on schema 0052 with 14 WorkItems and
82 Runs. Validate an isolated copy with the immutable release image and record
its compatible rollback floor before deploying. The controller has no live hosted
work to advance while creation remains disabled; that idle fact is not restart
or autonomous-delivery acceptance.

## Immutable-image migration proof

Runtime `48c77b7` at digest
`sha256:42d7acbc2e425c76c7b22be58251aa2bb45f5a94b25634421f30f9d7c4dabf6d`
successfully migrated an isolated copy of the verified pre-0053 archive. The
migration container had neither the live data volume nor credentials. SQLite
integrity and the runtime health check passed; all 14 WorkItems, 82 Runs, stage,
evidence, audit, product/repository and generation records were preserved. Existing
WorkItem fields were compared, and all three new hosted controller tables remain
empty. Legacy work was not enrolled. The original archive remains schema 0052.
[Job](ASTRA-M06-CLONE-MIGRATION-JOB.json) and
[verified result](ASTRA-M06-CLONE-MIGRATION-VERIFIED.json).

The complete seven-image/native release is still being assembled. A Node runner
registry transfer timed out; its completed local OCI artifact is retained for an
identity-preserving retry. No partial release pins or live database migration were
applied. Before deployment, record the complete schema-53-compatible rollback set;
source `2249950` cannot read schema 0053 and must not be used afterward.
