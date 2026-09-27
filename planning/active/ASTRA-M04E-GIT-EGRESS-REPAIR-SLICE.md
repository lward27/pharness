# ASTRA M04E Slice 2: Repair the worker git egress regression, re-release, restore readiness

Status: **complete (2026-09-22).** Authority: [ASTRA master](../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md), [M04](../programs/autonomous-sdlc/ASTRA-04-CODING-RELIABILITY-QUALIFICATION.md), and the [dispatch-path audit](../evidence/autonomous-sdlc/ASTRA-M04E-DISPATCH-PATH-AUDIT.md) (2026-09-21). Date: 2026-09-21.
Corrects: slice 1's "first connected attempt is the next slice" — the audit proved the loop is **not** dispatchable and found the blocking defect.

## Why this slice is the thin one

The audit established, with deterministic in-cluster A/B evidence, that `crates/pharness-worker/src/main.rs` `repository_git_output` (hardened by **`bc59f30`**, present in the deployed `49ecdc5` lineage) calls `.env_clear()` without re-adding the preparation proxy env. Runner-preparation egress is NetworkPolicy-restricted to the egress proxy, so every source `git fetch` through that helper fails (`github.com:443 … Couldn't connect to server`, rc=128); with the proxy env inherited the identical fetch of the exact pinned commit succeeds (rc=0). This blocks: every readiness assessment (3/3 `git_fetch_failed` on `finance-frontend` @ `28e55351`), and — same helper — Builder/Repair workspace checkout, source-reader validation, onboarding patch/contract validation. The connected loop is therefore stopped **before any stage dispatch**, independent of the V2 flag and the V2 policy-qualification gaps (also recorded in the audit). The fix is small and local; the re-release reuses the slice-1 pipeline.

## Current state (recorded 2026-09-21, runtime `49ecdc5`)

- Readiness assessment for `repo_01a04369f1057e13a13c24233d4e278a` @ `28e55351…`: `contract_status: ready`, `coding_status: blocked`, blocker `git_fetch_failed` (preps `sprep_01a0c241…`, `sprep_01a0c24c…`, 3rd retry same result).
- Capability preflights pass when re-run (source_reader/writer/workspace, model_provider, node-24 profile all `available` on 21 Sep); evidence is 15-minute-expiring, not the blocker.
- V2 policy qualifications: `builder-kimi-k2p7-code-v2` and `repair-kimi-k3-v2` have passing rows whose hashes still match the live registry; `planner-kimi-k3-v2`, `test-diagnosis-nemotron-v2`, `verifier-glm-5p3-v2` latest=failed. `PHARNESS_CODING_RELIABILITY_V2_ENABLED=false` (deployed).
- Rollback floor from slice 1 still applies: rolling back to `291e007`/`90ab524` requires pausing Planner work.

## Slice steps

1. **Fix** `repository_git_output` (and confirm no sibling helper needs the same): re-add `HTTPS_PROXY`/`https_proxy`/`NO_PROXY`/`no_proxy` from the inherited environment into the allowlisted env (fallback to no proxy only when unset), preserving `bc59f30` hardening (hooks disabled, global/system config disabled, token via askpass only). Commit to pharness main with a focused message citing the audit.
2. **Local checks**: `cargo build` + the worker crate tests; add/adjust a deterministic unit test asserting the proxy env is propagated (and that unrelated env is still cleared).
3. **Build + re-pin**: rebuild the affected runner images (node-runner; python-runner if it shares the worker) via the slice-1 release pipeline from the new main SHA; push; update the GitOps pin (env-profile image digests + revision).
4. **Deploy and verify identities**: Argo auto-sync `Synced/Healthy`; API reports the new compiled revision; runner images at exact digests; no rollout restart.
5. **Restore readiness**: re-run capability preflights (source_reader + node-24 profile fresh) and create the readiness assessment for `28e55351…`; it must reach `coding_status: ready` with declared acceptance executed.
6. **Service window + data preservation**: full ten-minute window; pre/post read-only Finance DB comparison (generation, WorkItems, Runs, holds).
7. **Evidence + status**: write the ASTRA evidence record (fix SHA, digests, readiness result, window, DB comparison, rollback floor) and update the M04 gate table / master "Current execution" against it only.

## Acceptance

- [x] Fix merged in main; worker unit tests pass locally; proxy env present in the git invocation, hardening preserved.
- [x] New runner image(s) built at exact digests; GitOps pin updated; Argo `Synced/Healthy` without manual intervention.
- [x] Fresh readiness assessment for `28e55351…` reaches `coding_status: ready` (declared `test`/`lint`/`build` executed, no `git_fetch_failed`).
- [x] Ten-minute service window passes; Finance DB fully preserved (only new verification rows).
- [x] Rollback floor re-stated for the new runtime; V2 flag and all policies unchanged; no model run dispatched.
- [x] Evidence committed under `planning/evidence/autonomous-sdlc/` with `ASTRA-` prefix; M04/master updated from evidence.

## Boundaries and non-goals

- No connected-loop dispatch, no V2 flag change, no policy re-qualification in this slice (those are the following slices — see "Next").
- No changes to the egress proxy, NetworkPolicies, or allowlists; the proxy already allows `github.com`.
- No schema migration, no Finance production change, no hosted-creation activation.

## Failure behavior

- If the fixed readiness assessment still fails `git_fetch_failed`: capture the prep Job's real git stderr (pod logs before TTL) before any retry; the A/B probe (variant A vs B) is the reference for distinguishing helper-env from network causes.
- Intermittent registry transport (known): retain the failed phase evidence, retry only that phase.
- A real external blocker may suspend dependent work but never waives a gate.

## Next (after this slice, in order)

1. **M04E Slice 3** — re-qualify the three failing V2 stage policies (planner, test-diagnosis, verifier) on the new runtime via live inference evaluations (builder/repair retain their passing rows).
2. **M04E Slice 4** — enable `codingReliabilityV2`, re-establish readiness, and run the **first connected attempt**: plan → implement → deterministic Test → actual failure diagnosis → one repair → Test → independently scoped verification, with at least one real failed implementation repaired through the actual handoff and no manually supplied patch.
