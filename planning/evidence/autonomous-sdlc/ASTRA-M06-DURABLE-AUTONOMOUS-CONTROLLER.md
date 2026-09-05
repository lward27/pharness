# ASTRA M06: Durable controller implementation evidence

Status: persistence preparation tested; controller integration and milestone acceptance open.
Observed 2026-09-05. Worktree `pharness-astra-controller`, branch
`codex/astra-autonomous-controller`, created from current main `db84b797` and
merged with tested M05 preparation at `e197497`. The deployed API remains on
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
is unknown; only a reconciled terminal operation releases them. This prevents a
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

## Remaining implementation and acceptance

The API loop, routine progression, scheduling of dispatch recovery, operator control routes,
callback integration and their adapter tests remain to be implemented. No autonomous
workflow or cluster recovery is claimed by the persistence tests. M05 compatible
readers and qualification remain dependencies; this independent preparation does
not waive those gates.

Before applying 0053, publish and record a compatible rollback release that knows
both 0052 and 0053. An older SQLx migrator rejects an unknown applied migration.
Preserve the Finance data generation, audit history and retention policy; do not use
a down migration or a database reset for recovery. The final acceptance gates remain
in [M06](../../programs/autonomous-sdlc/ASTRA-06-DURABLE-AUTONOMOUS-CONTROLLER.md).
