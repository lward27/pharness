# ASTRA M06: Durable controller implementation evidence

Status: continuous engineering progression integrated and locally tested; delivery integration, live recovery, and milestone acceptance open.
Observed 2026-09-05. Worktree `pharness-astra-controller`, branch
`codex/astra-autonomous-controller`, created from current main `db84b797` and
merged with tested M05 preparation at `e197497`, then current main `2249950` at
controller merge `3a2e369`. The deployed API remains on
`fd740927`; no M06 code or schema has been deployed.

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

## Remaining implementation and acceptance

Terminal Run-to-stage normalization and incomplete multi-record startup
still need interruption recovery. Source writer/observer persistence ordering and the delivery
adapters are not integrated into this scheduler. Production approvals, rollback,
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
