# ASTRA M06: Durable autonomous controller

Status: planned.
Authority: [approved program](ASTRA-00-PROGRAM.md).
Dependencies: M05.

## Objective and scope

Progress authorized work inside the deployment without browser clicks or an external assistant driving the state machine.

The program's locked authority, budget, source identity, compatibility, and evidence
rules apply. Work on current verified main in an isolated `codex/` worktree.
Do not mark this milestone accepted because a dependency appears healthy.

## Implementation

1. Add continuous reconciliation to the existing single-writer API. Persist due times, claims/leases, and external operation identities; reuse workers, observers, dispatchers, and effect intents.

2. Start with one coding run globally and serialized delivery per repository/environment. Reconcile a known external operation before dispatching another.

3. Use deterministic operation names and transactional state transitions. Do not promise exactly-once network delivery; prove retry-safe effect handling.

4. Implement pause/resume/cancel without discarding history. Pause stops new development/promotions while observations and already-authorized release recovery continue.

5. Keep GET and navigation read-only. Do not use browser polling, CLI ticks, cron, or Codex automations as the runtime controller.

## Interfaces and compatibility

Durable scheduling/claim state and existing action previews/projections; no new service, event bus, or database engine.

Preserve immutable historical evidence, existing Finance data generation, and additive migration compatibility. Record any minimum compatible rollback version before enabling new writes.

## Tests and acceptance

- [ ] API restart during active work resumes from durable state after readiness.
- [ ] Duplicate callbacks and reconciliation do not duplicate source writes, PipelineRuns, GitOps changes, or completion outcomes.
- [ ] Expired claims and lost dispatch acknowledgments reconcile existing external resources before retrying.
- [ ] Pause/resume/cancel behavior matches API/UI explanations and preserves observation/recovery.
- [ ] Idle or overdue work cannot create infinite retries, expand budgets, or generate a new incident WorkItem.

Deterministic state-machine tests with temporary real SQLite plus adapter fixtures; bounded live restart proof belongs to M11/M12.

## Deployment and recovery

Record exact changed resources, source revisions, validation, before/after identities,
and external effects. Keep managed cluster changes in authoritative GitOps sources.
Production runtime acceptance still requires a concrete human approval.

Disable new dispatch while allowing bounded observation/recovery. Retain intents and claims for a compatible controller to resume; do not delete Jobs or records to reset progress.

## Evidence and closeout

Write ASTRA-M06-DURABLE-AUTONOMOUS-CONTROLLER.md with restart, duplicate-effect, lease, interruption, and pause scenarios.
Use `planning/evidence/autonomous-sdlc/` for milestone execution evidence unless an
existing assessment location is explicitly named. Include date, revisions, commands
without secrets, observed results, failures, limitations, and commit/release identities.
A test result and a deployed result are separate claims.

Review coverage: F01 and F13.
Update the master ledger and this document only after its checks are evidenced.
Unmet criteria remain unchecked with a concrete reason and next action.

## Goal-mode execution prompt

Read ASTRA-00-PROGRAM.md and this milestone. Verify dependencies against current
evidence, inspect the affected implementation, execute the bounded changes above,
and run the specified meaningful checks. Preserve user work and all safety/identity
boundaries. Record results, commit the implementation and evidence, update the master
and finding ledger, then continue the next eligible milestone. If an external input is
missing, explain the exact blocker and continue independent work. Do not weaken a gate,
silently switch provider/budget, or claim unexecuted deployment or autonomous acceptance.

