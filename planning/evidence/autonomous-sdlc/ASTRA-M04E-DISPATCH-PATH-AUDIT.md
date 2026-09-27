# ASTRA M04E: Connected-loop dispatch-path audit on 49ecdc5

Reviewed: 2026-09-21. Runtime: `49ecdc5cc4eaf86197707dede6244bc55c888732` (deployed, from
slice-1 [candidate release record](ASTRA-M04E-49ECDC5-CONNECTED-LOOP-CANDIDATE-RELEASE.md)).
Purpose: before dispatching M04E's first connected attempt, trace the *exact* code path the
connected loop runs and verify every gate is open on the live runtime. This is a
read/inspect audit plus the sanctioned non-production-mutating capability preflights; it
created **no** WorkItem, dispatched **no** model Run, and changed **no** config.

## Verdict

**The M04E connected loop is NOT dispatchable on the current `49ecdc5` deployment.**
Slice-1's "dispatch-ready / first connected attempt is the next slice" framing conflated
*service health* (which slice 1 proved) with *connected-loop dispatchability* (which it
did not). The audit below shows the connected loop's required repair handoff is hard-gated
behind `coding_reliability_v2_enabled`, which is **off**, and the non-failure path is
independently blocked by stale stage-policy qualifications. This audit corrects the slice
plan; it initiates no model run and flips no flag.

## The connected loop's mechanism (code-traced)

The repo-mode stage chain auto-advances on each sealed Run outcome via
`continue_repo_stage_chain` (`crates/pharness-api/src/app/repo_mode/stages.rs:1002`).
The M04E-relevant branch (a failed deterministic Test or a rejected Verify) reaches the
repair handoff **only** through:

```
continue_repo_stage_chain
  └─ if outcome.status != "succeeded":
       if state.repo_mode.coding_reliability_v2_enabled        // <-- HARD GATE, line 1052
           && matches!(stage, "test" | "verify")
           && repairable_repo_stage_failure(...)?             // one implement, real command failure
           if stage == "test" && inference.enabled
              && latest_planned_selection_for_profile(..., "repo-test-diagnoser").is_some()
              => start_repo_followup_stage(..., "test", Some(outcome))   // test_diagnosis
           else
              => start_repo_automatic_repair(...)
       return Ok(None)                                       // <-- V2 OFF: chain STOPS here
```

- `start_repo_automatic_repair` (`stages.rs:1155`) is the **only** caller of the
  diagnose→repair→retest handoff after a real failed implementation. It is reached **only**
  under `coding_reliability_v2_enabled` (line 1052) or via the `test_diagnosis` path
  (line 1030), which is itself only reachable when a `repo-test-diagnoser` selection exists —
  which is only created under V2 (`stages.rs:1305`, `deterministic_test =
  coding_reliability_v2_enabled && stage=="test" && !test_diagnosis`).
- **Deployed flag:** `PHARNESS_CODING_RELIABILITY_V2_ENABLED=false`
  (`kubectl -n pharness exec pharness-api-... -- env | grep CODING_RELIABILITY_V2`).
  Helm: `deploy/helm/pharness/values.yaml` `features.codingReliabilityV2.enabled: false`.
- **Consequence:** with V2 off, a failed Test → `return Ok(None)` → the chain stops. There is
  **no** diagnosis and **no** repair. M04E's core requirement — *"at least one real failed
  implementation repaired through the actual handoff"* — **cannot occur** in the current config.

The operator `correct_stage_chain` action (`repo_mode/actions.rs:683`) re-opens a fresh
builder chain on the preserved workspace; it is a *re-implementation* lever, not the
diagnose→bounded-repair handoff, and does not substitute for the M04E proof.

## Dispatchability matrix (resolved against the LIVE registry + store)

`policy_reference` (`inference.rs:596`) selects the V2 default per profile when
`coding_reliability_v2_enabled`, else the v1 registry defaults. `resolve_binding`
(`inference.rs:619`) then requires — for any policy **except** `fireworks-legacy-v1` — a
passing qualification row whose `policy_hash` **and** `target_hash` match the **current**
registry (runtime not required here). Verified by computing the live API's policy hashes
(`/api/inference-policies`) and comparing to `inference_policy_qualifications`.

| Stage | V2-OFF (current) policy | Qual gate | V2-ON default policy | Qual gate |
| --- | --- | --- | --- | --- |
| plan | `fireworks-legacy-v1` | **exempt** (skips qual) | `planner-kimi-k3-v2` | **latest=failed** (rt 19b0c55) |
| implement | `fireworks-legacy-v1` | **exempt** | `builder-kimi-k2p7-code-v2` | passed, but rt `48c77b7` |
| test | `tester-kimi-k2p6-low-v1` | **BLOCKED** (passed row only on rt `4cedb59ed6`, `ph_match=False th_match=False`) | deterministic controller (no model qual) | n/a |
| test_diagnosis | `repo-test-diagnoser` (V2-only) | `test-diagnosis-nemotron-v2` **latest=failed** | `test-diagnosis-nemotron-v2` | **latest=failed** |
| verify | `verifier-kimi-k2p6-high-v1` | **BLOCKED** (passed row only on rt `4cedb59ed6`, `ph_match=False th_match=False`) | `verifier-glm-5p3-v2` | **latest=failed** (rt 19b0c55) |
| repair | `repo-repair` (V2-only) | `repair-kimi-k3-v2` passed (rt `48c77b7`) | `repair-kimi-k3-v2` | passed, rt `48c77b7` |

Notes:
- The v1 test/verify "passed" rows are on old runtime `4cedb59ed6` with hashes that no
  longer match the **current** registry — the registry has since changed
  (`git log -- deploy/helm/pharness/files/inference-registry.json`: `50590c3` #400,
  `54a9bfd` Planner readiness, `4a6e511` protocol fixtures, …). So **even the non-failure
  happy path (plan→implement→test→verify) blocks at the test stage** in the current config.
- With V2 **on**, plan/test-diagnosis/verify resolve to V2 policies whose latest verdicts are
  **failed** → those stages block; implement/repair would resolve (passed on the old
  `48c77b7` runtime, hashes still match). So flipping V2 on does **not** make the loop
  dispatchable either — it moves the blockers to planner/diagnosis/verifier.

## Readiness (the one fixable class) — still not current

The read-only WorkItem preflight
(`POST /api/products/:id/work-items/preflight`, `repo_mode/creation.rs:118`) returned, on the
onboarded+ready `finance-frontend` repo (`repo_01a04369f1057e13a13c24233d4e278a`, commit
`28e55351…`):

```
BLOCKERS: [repository_readiness_not_current]
  mismatches: environment_profile_tuple_changed,
              source_reader_evidence_stale,
              runner_profile_evidence_stale,
              readiness_input_hash_mismatch
planner_execution: null   // => chain dispatches via the INFERENCE path (not codex)
```

- The capability verifications have a 15-minute expiry. Slice-1's ran ~2026-09-20 02:22 UTC
  and lapsed. Re-running the isolated preflights this session returned `available` for
  `source_reader`, `source_workspace`, `source_writer`, `model_provider` — **and `node-24`
  runner verification is now FAILING**:
  `pf2-node24-fresh.json` → `status: unavailable`,
  `summary: "Isolated identity did not verify runner_revision_platform_executables_venv_and_preparation_egress
  for https://github.com/lward27/finance-frontend.git"`. An earlier node-24 run at
  23:20 UTC had returned `available`. **Update:** a retry at 04:12 UTC returned
  `available` — the isolated runner Job is **flaky**, not stably broken. The job script
  (`dispatch.rs:1036` `verify_environment_profile`) runs revision/executable checks,
  `git ls-remote`, and `npm ping --registry=https://registry.npmjs.org/` through the
  preparation egress proxy; the 135 s failure duration = polling to the deadline,
  consistent with the egress `npm ping` phase intermittently failing. See "Open question".
- Creating a fresh readiness assessment
  (`POST /api/repositories/:id/readiness-assessments`, `products/readiness.rs:82`) requires
  **both** a fresh passing source-reader **and** a fresh passing runner-profile verification.
  It therefore currently fails with `409 "repository readiness requires a fresh passing
  runner-profile verification"`. The stored assessment `rready_01a05337…` also still binds the
  pre-`49ecdc5` node-24 profile revision (`117feed8…`) → `environment_profile_tuple_changed`.

## What "dispatch-ready" actually covered (correction)

Slice-1's "dispatch-ready" = deployed `49ecdc5`, 12/12 capabilities, ten-minute service window
passed, Finance DB preserved. That is **service/infrastructure readiness**, which is a real and
necessary precondition. It was **not** evidence that the connected loop's *stage-policy
qualifications and V2 repair gate* are open. Those were never checked, and this audit shows
they are not. No prior record was wrong; the term was doing more work than it should.

## Root cause of the readiness `git_fetch_failed` (deterministic, reproduced)

The readiness preparation Job fails its `fetch` step with `git_fetch_failed` (3/3 attempts:
`sprep_01a0c241…`, `sprep_01a0c24c…`, `sprep_…050`), while the `node-24` profile verification
(job runs `git ls-remote` with the proxy env **inherited**) succeeds on retry. A/B probe Job
in the cluster (same `pharness-node-runner` SA, same deployed node-runner image
`sha256:e1f3056e…`, `app=pharness-runner` + `phase=preparation` labels, preparation egress
proxy, identical readiness fetch args `--depth 1 --no-tags --filter=blob:none --no-recurse-submodules`):

- **VARIANT A — proxy env removed** (mimics the worker's git helper):
  `fatal: unable to access …: Failed to connect to github.com port 443 after 37 ms` → `rc=128`.
- **VARIANT B — proxy env inherited**: fetch of the exact pinned commit `28e55351…` → `rc=0`.

The runner preparation NetworkPolicy (`pharness-runner-preparation-egress`) permits egress
only to DNS, `pharness-api:4777`, and `pharness-preparation-egress-proxy:8080` — direct
`github.com:443` is blocked, so git **must** use the proxy.

**The defect:** `crates/pharness-worker/src/main.rs` `repository_git_output` (used by
`repository_git_command`/`repository_git_stdout`, and by `checkout_exact_repository` at
`main.rs:992`) was changed by **`bc59f30`** ("updates", 2026-09-19 — **present in the deployed
`49ecdc5` lineage**, verified with `git merge-base --is-ancestor`) to add
`.env_clear()` + allowlisted env (`PATH`, `HOME`, `LANG`, `PHARNESS_SOURCE_READER_TOKEN`,
`GIT_ASKPASS`, `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_NOSYSTEM`) for hardening. It does **not**
re-add `HTTPS_PROXY`/`https_proxy`/`NO_PROXY`/`no_proxy`, which the Job container env sets
(`dispatch.rs` `repository_readiness_job_manifest`). So every source `fetch` through that
helper now dies on the blocked direct route.

**Blast radius:** every readiness assessment (3/3 failed), and — by the same helper —
source-reader/discovery validation, onboarding patch/contract validation, and the
Builder/Repair workspace checkout (`checkout_exact_repository` call sites at `main.rs:525`,
`:767`, `:884`) on `49ecdc5`. The connected loop is blocked at readiness *before* any stage
dispatch, independent of the V2 flag and policy-qualification gaps above.

**Fix (next slice):** re-add the proxy env (or an equivalent explicit
`-c http.proxy=…` on the git invocation) to `repository_git_output` — preserving the
`bc59f30` hardening intent (hooks disabled, global config disabled, token only via askpass)
— then rebuild/re-pin and re-run the readiness assessment to `coding_status: ready`.
The egress proxy host allowlist already covers `github.com`.

## Prior open question (resolved)

The `node-24` isolated runner verification is **not** flaky in the proxy itself: it passes
whenever it runs (4/4 on 21 Sep) because its job inherits the proxy env. The proxy
`pharness-preparation-egress-proxy` (pod `…-v2r7s`, endpoint `10.42.0.26:8080`, 0 restarts,
healthy) is stable; the constant log line "proxy client closed before sending CONNECT" is
kubelet readiness-probe noise (probes close before sending CONNECT). The failures were all
the worker-helper proxy regression above. (An earlier probe using label `app=pharness-diag`
was rejected by the proxy NetworkPolicy — self-inflicted and discarded; only the
`app=pharness-runner` probe is valid evidence.)

## Consequence for the next slice

The connected attempt is not the next thin slice. The next thin slice is to **make the
connected loop dispatchable**, in this order:

1. **Fix the worker git-helper proxy regression** (`bc59f30`, `repository_git_output`):
   re-add the preparation proxy env to the hardened git invocation; local checks; rebuild +
   re-pin; re-run the readiness assessment to `coding_status: ready` on the exact commit.
   This unblocks readiness *and* the Builder/Repair workspace checkout that the loop needs.
2. **Re-qualify the V2 stage policies on the new runtime** (planner, test-diagnosis,
   verifier; builder/repair already have passing rows that still match current hashes).
3. **Enable `codingReliabilityV2`** and re-establish current readiness (15-min
   capability evidence).
4. **Only then** dispatch the first connected attempt
   (plan → implement → deterministic Test → actual failure diagnosis → one repair → Test →
   independently scoped verification).

Steps 2–3 are themselves substantial (live model evaluations and a config/deployment change),
so they are scoped in the corrected slice plan rather than assumed away. No model run was
started and no flag was flipped by this audit.
