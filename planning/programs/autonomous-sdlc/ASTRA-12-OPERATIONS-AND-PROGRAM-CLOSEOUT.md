# ASTRA M12: Operations and program closeout

Status: planned.
Authority: [approved program](ASTRA-00-PROGRAM.md).
Dependencies: M11 and all earlier acceptance gates.

## Objective and scope

Finish a polished, accurately documented, operationally proven native SDLC system.

The program's locked authority, budget, source identity, compatibility, and evidence
rules apply. Work on current verified main in an isolated `codex/` worktree.
Do not mark this milestone accepted because a dependency appears healthy.

## Implementation

1. Complete 24 hours of unattended observation including demonstrated recovery from an interrupted active workflow. Confirm idle/expired waits cannot generate duplicate or unrequested work.

2. Finish operator guidance for onboarding, requests, approvals, pauses, failures, recovery, and evidence inspection, including actual limits and minimum compatible rollback release.

3. Reconcile README, product contracts, active indexes, examples, and diagrams against the accepted source and deployment. Preserve historical proof and superseded plans as history.

4. Release PHarness using one merged source revision, all seven currently required image artifacts and native bundle, a separate digest-pin commit, and observed Argo/imageID verification.

5. Close F01–F16 with current evidence and distinguish objective checks from subjective usability assessment. Obtain owner review of the polished workflow.

6. Leave generic build/observability/deployment adapters, incident initiation, multi-repo orchestration, workflow builders, and new coding backends to a separate future program.

## Interfaces and compatibility

Finalize the accepted native interfaces and operation contracts; no adapter abstraction expansion.

Preserve immutable historical evidence, existing Finance data generation, and additive migration compatibility. Record any minimum compatible rollback version before enabling new writes.

## Tests and acceptance

- [ ] 24-hour evidence and interruption recovery exist; no duplicate work or silent budget/authority expansion.
- [ ] All twelve milestones and every finding have a passing evidence-backed disposition, with no waived acceptance gate.
- [ ] Documentation links, examples, and any rendered diagrams validate against the release.
- [ ] Final source, image digests, GitOps revision, Argo observation, live imageIDs, compatibility, and known limitations agree.
- [ ] Owner review is recorded. A healthy release or elapsed time alone cannot complete the program.

Required release checks, bounded live observation, meaningful failure scenarios, documentation validation, and owner walkthrough.

## Deployment and recovery

Record exact changed resources, source revisions, validation, before/after identities,
and external effects. Keep managed cluster changes in authoritative GitOps sources.
Production runtime acceptance still requires a concrete human approval.

Use the recorded minimum-compatible immutable PHarness release; preserve current schema, Finance generation, and all evidence. Never down-migrate or delete data to make rollback possible.

## Evidence and closeout

Write ASTRA-M12-OPERATIONS-AND-PROGRAM-CLOSEOUT.md with soak, release, owner review, and final findings ledger.
Use `planning/evidence/autonomous-sdlc/` for milestone execution evidence unless an
existing assessment location is explicitly named. Include date, revisions, commands
without secrets, observed results, failures, limitations, and commit/release identities.
A test result and a deployed result are separate claims.

Review coverage: Final verification of F01–F16.
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

