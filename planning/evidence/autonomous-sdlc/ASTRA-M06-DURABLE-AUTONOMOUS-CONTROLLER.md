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

## Validation

The full store suite passed **51 tests**, including five new tests covering
exclusive/expired claims, future due times, persistent locks, duplicate identity,
immutable results, pause/cancellation, and reopening a real on-disk SQLite database.
The five affected tests passed again after the final control-fencing correction.
Store Clippy passed for all targets with warnings denied. See
[validation evidence](ASTRA-M06-PERSISTENCE-VALIDATION.json) for exact source hashes,
log hashes, and the validation boundary. These counts overlap and are not additive.

## Remaining implementation and acceptance

The API loop, routine progression, dispatch recovery, operator control routes,
callback integration and their adapter tests remain to be implemented. No autonomous
workflow or cluster recovery is claimed by the persistence tests. M05 compatible
readers and qualification remain dependencies; this independent preparation does
not waive those gates.

Before applying 0053, publish and record a compatible rollback release that knows
both 0052 and 0053. An older SQLx migrator rejects an unknown applied migration.
Preserve the Finance data generation, audit history and retention policy; do not use
a down migration or a database reset for recovery. The final acceptance gates remain
in [M06](../../programs/autonomous-sdlc/ASTRA-06-DURABLE-AUTONOMOUS-CONTROLLER.md).
