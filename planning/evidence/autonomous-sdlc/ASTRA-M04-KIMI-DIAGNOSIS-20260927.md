# ASTRA M04: Kimi Test Diagnosis and gateway-contract release, 2026-09-27

This is the single evidence record for the follow-up to the [2026-09-25 handoff](../../handoffs/LOCAL-PREP-HANDOFF-20260925.md). Full JSON receipts are stored privately on the operator machine under `pharness-release-artifacts/cd1b28393a3df5084a9b6d9f6d5a15b7097c075f/`. Times are UTC.

## Changes

| Decision | Change | Result |
| --- | --- | --- |
| Switch Test Diagnosis from Nemotron to Kimi | #424: target `fireworks-kimi-k3@v2` (v1 plus the `test` stage only) and policy `test-diagnosis-kimi-k3-v2@v1` (Nemotron limits; only the target differs), now the `repo-test-diagnoser` V2 default | Released; Planner and Repair policy hashes unchanged |
| Stop tying qualification to releases | #424: `INFERENCE_GATEWAY_CONTRACT` (`pharness.dev/inference-gateway/v1`); receipts bind registry hash + contract; evaluations and qualifications store the contract (migration 0056); hosted admission checks the contract, not `runtime_revision` | Released; readiness `gateway_contract_aligned=true` |
| Consolidate drafts | #366 retargeted to `main` and merged with it (docs-only conflicts), 936 tests passing; #356–#363 closed | #366 remains a draft |
| Fix doctor drift | lucas_engineering #70: `buildkit_endpoint` checks the in-cluster `k3s-buildkit` Pod on an AMD64 `workload=build` node; `lucas-ops` reinstalled (`29353df2…`) | `doctor --credentials` → `passed` |
| Enable V2 and hosted creation (staging binding) | **Not applied.** The values edit was refused by the operator permission guard | Awaiting Lucas |

## Release `cd1b283`

- Source `cd1b28393a3df5084a9b6d9f6d5a15b7097c075f`; built 12:02–12:44 with the in-repo `pharness-build.sh`, which ran unmodified after #422; verified 7 of 7 images with `lucas-ops release verify`.
- Digests: runtime `sha256:502f7cf5f5032c91456691a34385a579992adde822de18ef13ff5ee72b41e4c0`, ui `sha256:948b0560cf2fa8c199e389533240675ffafbbd515b7e233b1e695e1f3b6b1bde`, python-runner `sha256:b50eacf9d4a77fce389ea6f3a37a599741566ea996743efa5e994c9f1a0e62a8`, node-runner `sha256:8798a9f69266454c8f13fe50cb7cae67f0af43347d2ca9e137cefe597c9fb482`, model-gateway `sha256:0f69fcf87c527f805df7957e1dd83bb91d59fc337352d035c7113c95d8b33dfe`, eval-runner `sha256:4d90561f318dfc3c30dd323ff5f0ce42a3b5999d47fae5c9b7799dc3605fd450`, codex-host `sha256:a41e5550d63228db63ebc5dde0178c3d14f05d68edc996ce2be1dbc00bd95a22`, native bundle `sha256:0bdb70f55cdeb73d122199591c8811327e6b6c22888a7309ff86acf36d039fa3`.
- Archive `pre-release-cd1b283-20260927` (manifest `sha256:ff4d3e85…4e85`, database `sha256:b7a54b8e…d303`) was independently verified: hash matches, integrity ok, migrations 1–55, 14 WorkItems, 82 Runs, 0 hosted rows or leases.
- Pin #425 was merged as `514f0cb`. Server dry-run passed for 53 of 53 objects, and the model-gateway env was unchanged. Observed at 12:51: Argo Synced/Healthy at `514f0cb`, every Pod imageID equal to its pin with 0 restarts, API and UI at `cd1b283`, registry `sha256:797966ef8976e5fc2af2c1176361912979e864388006cd41ecac665febb10575` and contract aligned, 14 WorkItems, 82 Runs.
- **Rollback note:** earlier runtimes refuse a database that carries the unknown migration 0056 (sqlx `VersionMissing`). A rollback requires reverting the pin and restoring the archive above.

## Test Diagnosis qualification

1. Protocol `inferverify_01a0e2ec36427c71a99e8d80f0ffba4a` (12:52–12:56): **passed 30/30**, bound to `test-diagnosis-kimi-k3-v2@v1` (`sha256:0ea7cb85…1cce`), target `fireworks-kimi-k3@v2`, runtime `cd1b283`, contract v1.
2. Full qualification `infeval_01a0e2f08d277e01a3f883aa0114995c` (op `m04-qualify-test-diagnosis-kimi-cd1b283`, 2 attempts, 12:56–13:35), qualification `inferqual_01a0e31427bc71919abc66ccb2f92cbe`: **not qualified**. Results: 21/24 passed, infrastructure valid, 0 false approvals, 0 false rejections, `candidate_safe=false`.
   - `wrong-test-selection` failed on both attempts (`stage_judgment`, `test_failure_misclassified`). Kimi reported `failure_kind: unknown` because the only receipt (`py_compile`) exited 0. It did not recognize that the selected acceptance command does not exercise the behavior.
   - `timeout` failed on attempt 2: it classified a 5 s `time.sleep` against the 100 ms harness budget as `semantic_test` instead of `structural_environment`. Attempt 1 passed.
3. Stop rule applied: no Verifier, Builder, Repair or Planner protocol check or qualification was dispatched. At 13:37 readiness was normal and available, and no evaluation or Job was active.

## What this means

The failure is model judgment on a systematic case, not transport or protocol. Unlike Nemotron, Kimi is protocol-reliable here. Hosted admission needs all five stages qualified under contract v1, so hosted creation would stay blocked even with the flags enabled. The suite and thresholds were not changed.

## Prompt fix and requalification (same day)

- #427 changed the `repo-test-diagnoser-v2` prompt (revision `2026-09-27.1`) to state evidence admissibility: receipts must match the selected acceptance names and the current source; `timed_out`, `spawn_failed` and `refused` receipts are `structural_environment`; `unknown` only for a fully passing, admissible result. The suite, fixtures and thresholds are unchanged.
- Release `9e495188bfa9eb0f4d5b6813b42f7e9225704dbe`: pin #428 → `73e1e41`; archive `pre-release-9e49518-20260927` verified (migration 56, 14 WorkItems, 82 Runs); rollout Synced/Healthy with Pod imageIDs equal to the pins (runtime `sha256:1b7994f7…`, gateway `sha256:5963288d…`, UI `sha256:72361c7a…`).
- The rollout coincided with **DiskPressure on `ubuntu-lucas-engineering`**, which evicted Pods in several namespaces, including the registry. It recovered on its own (about 25 GB free on a 196 GB disk; images account for only 2.6 GB; the rest is local-path volume data). The preparation egress proxy recovered after the registry returned. Pruning was prepared as an explicit script but not run.
- Protocol `inferverify_01a0e35aaa0673c3951d184674ce7a2b`: passed 30/30.
- Qualification `infeval_01a0e35e08a57461a95ce01731af77b3` → `inferqual_01a0e377c359766295b4071e31ed1fea`: **not qualified, 21/24**. `wrong-test-selection` and `timeout` now pass on both attempts. The three failures are the last three cases of attempt 2 (`single-localized-failure`, `multiple-related-failures`, `passing-control`): `provider_or_protocol_failure` with no typed submission. The gateway logged Fireworks `412 PRECONDITION_FAILED` at 15:24:30–31, returned in 10–18 ms with zero tokens. That points to an account-level rejection (spend or credit limit, or key state), not model judgment. On every case that reached the model, the diagnoser's judgment was correct.
- Stopped: no retry and no other stage. Before any further model-backed operation, the Fireworks account state must be checked.
