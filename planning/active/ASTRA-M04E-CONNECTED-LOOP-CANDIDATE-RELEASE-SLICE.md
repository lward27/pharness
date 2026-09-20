# ASTRA M04E Slice 1: Connected-loop candidate release and dispatch readiness

Status: complete 2026-09-20, re-baselined onto upstream `49ecdc5`. Authority: [ASTRA master](../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md) and [M04](../programs/autonomous-sdlc/ASTRA-04-CODING-RELIABILITY-QUALIFICATION.md). Date: 2026-09-19.
Result: [release and dispatch-readiness evidence](../evidence/autonomous-sdlc/ASTRA-M04E-49ECDC5-CONNECTED-LOOP-CANDIDATE-RELEASE.md).

## Purpose

Make the M04E connected loop dispatchable on one verified exact runtime. The repair-authorization correction (PR #397, `67f5303`) is merged in main but not deployed, and the 2026-09-18 readiness snapshot shows all twelve capability verifications stale. This slice ships the combined candidate release and restores dispatch readiness; it proves nothing about model capability and dispatches no connected work.

## Current state (record before starting)

- main HEAD `bc59f30`, in sync with origin/main; untracked `tutorial-site/` is preserved, not touched.
- Deployed runtime: compiled `291e007` (Planner readiness release, pin `90ab524`) under GitOps `50590c3` (local GPT-OSS config-only release, 2026-09-18).
- `67f5303` adds the original-stage-authorization recheck at the Builder/Repair handoff; it is a prerequisite of the connected-loop candidate and is absent from the serving images.
- Readiness (2026-09-18): model_provider, source_workspace, source_reader/writer/observer, gitops_writer/observer, tekton, argo, observability, and the python-3.11 / node-24 environment profiles are all stale or configured-unverified.
- Local GPT-OSS 120B diagnostic is terminal: 30/30 protocol calibration, Planner 1/2 (retained failure), diagnostic-only, not qualified.
- No active evaluations; Finance on schema 55, generation `dbgen_finance_20260827`, 14 WorkItems / 82 ordinary Runs (last verified counts); hosted creation and Coding Reliability V2 disabled.

## Slice steps

1. **Baseline record**: HEAD, origin/main, worktree status, serving identities, Argo revision, and a fresh readiness snapshot in the evidence dir.
2. **Refresh the twelve isolated capability preflights** against the current runtime. Verification records only; no cluster mutation, no budget or policy change.
3. **Build one combined immutable release** from fresh `codex/` worktree main: seven service images plus the native bundle, Mac BuildKit route, cached evaluator toolchain steps reused. Record build duration and any transport warnings.
4. **Deploy through a GitOps pin**; Argo auto-sync; observe exact serving identities (API/UI report the new compiled source; gateway registry hash matches the build).
5. **Run the full ten-minute service window** and a pre/post live read-only comparison of the Finance database (generation, WorkItems, Runs, stage outcomes, audit records, holds).
6. **Record the rollback floor**: rollback to `291e007`/`90ab524` lacks the repair recheck, so pausing new Planner work is required before or during any rollback to it.
7. **Write the evidence record** and update the M04 gate table and master "Current execution" only against that evidence.

## Acceptance

- [x] Single merged source; all seven images verified at exact digests (pinned upstream by PR #403; serving identities re-observed by this slice).
- [x] Argo `Synced/Healthy` without manual sync or rollout restart; exact serving identities observed.
- [x] Full ten-minute service window passes (20 samples, 600 s).
- [x] Live pre/post comparison shows every Finance record preserved (schema 55, 14 WorkItems, 82 Runs, audit and holds intact; +12 capability_verifications rows from this session's preflights only).
- [x] All twelve capability preflights valid against the new runtime identity.
- [x] Rollback floor recorded; hosted creation and Coding Reliability V2 remain disabled; no qualification is claimed.
- [x] Evidence committed under `planning/evidence/autonomous-sdlc/` with an `ASTRA-` prefix; M04 and the master updated from evidence.

## Boundaries and non-goals

- No connected-loop dispatch and no protocol or semantic qualification in this slice.
- No model, default, budget, limit, or suite change; the local GPT-OSS policy stays diagnostic-only and its failed result is not rescored.
- No schema migration, no Finance production change, no activation of hosted creation.
- Frozen 24-task coding suite and all M04 thresholds remain untouched.

## Failure behavior

- Intermittent registry transport (known issue): retain the failed phase evidence and retry only that phase; one successful build does not establish a permanent transport repair.
- Service-window or preflight failure: retain the first useful failure, identify the failing boundary, fix only the demonstrated issue, repeat the affected check.
- A real external blocker may suspend dependent work but never waives a gate; record the concrete blocker and continue independent work.

## Next slice

M04E Slice 2: first connected-loop attempt on this exact runtime — plan → implement → deterministic Test → actual failure diagnosis → one repair → Test → independently scoped verification — with at least one real failed implementation repaired through the actual handoff, no manually supplied patch, and an unresolved plan stopping before implementation.
