# ASTRA M07: Exact-source delivery and real builds

Status: finite pipelines and packaging merged; both real builds verified; source publication and observation validated in PR 338; automatic merge, source-to-build integration and milestone acceptance open.
Authority: [approved program](ASTRA-00-PROGRAM.md).
Dependencies: M04 and M06.

The [execution evidence](../../evidence/autonomous-sdlc/ASTRA-M07-SOURCE-DELIVERY-AND-BUILDS.md)
records the GitOps and application revisions. This preparation proceeds independently
while qualification and delivery integration remain open. It cannot satisfy their
gates or either M11 autonomous change.

## Objective and scope

Connect verified source to an automatically merged commit and a genuine immutable Tekton build.

The program's locked authority, budget, source identity, compatibility, and evidence
rules apply. Work on current verified main in an isolated `codex/` worktree.
Do not mark this milestone accepted because a dependency appears healthy.

## Implementation

1. Reuse source writer/observer boundaries for PR creation, exact-head required-check evaluation, and authorized automatic merge. Preserve branch protections.

2. Invalidate/revalidate evidence when head or base changes alter the tested tree. Never merge untested source or silently bypass a failed required check.

3. Use the existing yfinance pipeline and a finite frontend counterpart with remote BuildKit. Require SOURCE_COMMIT, IMAGE_URL, IMAGE_DIGEST and verify their relationship to the actual merged commit.

4. Make application release images use committed locks and the tested Python/Node environments. Pin AMD64 bases and remove unbounded yfinance dependency/bootstrap resolution.

5. Keep synthetic build-output/noop fixtures out of live acceptance. Record registry resolution independently from the pipeline's claim.

## Interfaces and compatibility

Existing SourceDeliveryIntent/PipelineIntent and native pipeline result contract. No general source-provider or build-provider abstraction.

Preserve immutable historical evidence, existing Finance data generation, and additive migration compatibility. Record any minimum compatible rollback version before enabling new writes.

## Tests and acceptance

- [ ] Real PR/check/merge evidence binds to the built source and resulting registry digest.
- [ ] Stale head/base, failed checks, merge conflict, build failure, missing output, and mismatched digest stop the workflow.
- [ ] Build inputs use declared locks and aligned runtime versions; no staging-to-production rebuild.
- [ ] Required credentials remain scoped to their writer/effect boundary and absent from coding workers.
- [x] Real backend and frontend Tekton builds passed, with independent registry source/digest verification. These program-operated runs do not satisfy autonomous source delivery or M11.

Unit/adapter negative cases, pipeline rendering/server dry-run, then an authorized real immutable build. Application packaging prerequisites are implementation work, not M11 autonomous proof.

## Deployment and recovery

Record exact changed resources, source revisions, validation, before/after identities,
and external effects. Keep managed cluster changes in authoritative GitOps sources.
Production runtime acceptance still requires a concrete human approval.

Stop before subsequent promotion on failure. Preserve PR, PipelineRun, and digest evidence; never force-push or weaken protection to recover.

## Evidence and closeout

Write ASTRA-M07-SOURCE-DELIVERY-AND-BUILDS.md with source-to-registry identity map and failures.
Use `planning/evidence/autonomous-sdlc/` for milestone execution evidence unless an
existing assessment location is explicitly named. Include date, revisions, commands
without secrets, observed results, failures, limitations, and commit/release identities.
A test result and a deployed result are separate claims.

Review coverage: F13.
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
