# Local preparation handoff - 2026-09-25

Written by the local preparation agent (Claude Code on Lucas's machine) for the cloud development agent, which cannot reach the cluster. The evidence is in [ASTRA-M04E-LOCAL-PREP-20260925](../evidence/autonomous-sdlc/ASTRA-M04E-LOCAL-PREP-20260925.md). Times are UTC.

## Identities

| Item | Value |
| --- | --- |
| pharness `origin/main` at start | `3eaa3129b72b8f21de3fe6713119f3bccdbb9dda` |
| pharness `origin/main` before this handoff PR | `a0d748b32ddfbf0f08833e842e100c464412f1af` (#420 fmt, #421 pin, #422 script fixes) |
| lucas_engineering `origin/main` | `9de1e2d236b87a1bdb96a0278ddfb7bddfd33a00` (unchanged) |
| Built source SHA | `97c7e338630088ee8da70e0b7f554666dce8664b` |
| runtime (API + both egress proxies) | `sha256:ae83610075a9f976f87ac4fb7b988c28a9570d8f8acb67125b3930898a9c8121` |
| ui | `sha256:9aee6a25d337e41ebf7a9c9e1c550cc71c617450d81557b791fc60acfbb9f4fa` |
| python-runner | `sha256:6335c271c8a841fd2ee7cd90ffe85fca16be3312dc0614d6d23ba20b2aa8b3d4` |
| node-runner | `sha256:1de54a7fea1126c43b1f72a3536d29eab36645306aec253fd093d8c0756f55e7` |
| model-gateway | `sha256:69289e1a7de9aeb6679afeb25d893f2c859fb0ca07d40e273b1bb448cf272da3` |
| eval-runner | `sha256:2953bccbaeb64f074d9770ea9b6f95f5259781262077a2c6b17aa9cfd3c588fb` |
| codex-host | `sha256:319e48299878712de6ec876a926aed2a1b368f373036637602fd9a4538b16ed5` |
| native bundle | image `sha256:553c5744…05f8`; tar.gz `sha256:77ffa324facd33430dc678cd5bc33811c2cc847b5594a190fc7d7d252c09fe76` |
| GitOps pin | pharness commit `5155d48` merged as `c3395e8907be16109a7cd9161498076de82d1a6a` via [#421](https://github.com/lward27/pharness/pull/421). The pin lives in pharness `deploy/helm/pharness/values.yaml`, not in lucas_engineering. |
| Argo `pharness` revision | `c3395e8` Synced/Healthy at 14:34:09. It auto-syncs from pharness `HEAD`, so later docs-only merges advance the revision without changing workloads. |
| Observation timestamps | baseline 13:17–13:23; build 13:38–14:28; archive 13:39; rollout 14:34:09–14:34:55; protocol preflight 14:35:36–14:35:51; post-check 14:37:12 |

## Live flags and configuration

- Operational mode `normal`; inference `available`; `direct_fireworks_enabled=true`.
- Inference registry hash is `sha256:a2850180384d782b9aa8fa0bc4aa811983c834281a0d263231ea4d3d18c49e3a` on both API and gateway (aligned). The release did not change it.
- `coding_reliability_v2.enabled=false` (env `PHARNESS_CODING_RELIABILITY_V2_ENABLED=false`). The readiness endpoint does not report this flag or the hosted workflow.
- Hosted workflow is **disabled**. `PHARNESS_HOSTED_WORKFLOW*` is rendered only when enabled, so it is absent. Chart binding: `finance-yfinance@v1` (product `prod_01a043699d65721193e7e75d38654a2d`, repository `repo_01a04369f6387e6095aee4047ed2224f`, pipeline contract `pcontract_1788605350565822945`; staging `dcontract_1788605351039034558` → `charts/finance-staging/yfinance/kustomization.yaml`; production `dcontract_1788605351512248801` → `charts/yfinance-wrapper/kustomization.yaml`; `rollback_permitted=true`).
- Repo Mode V1, V1 UI and design overhaul are enabled; legacy WorkItem creation is disabled.
- All ten capability verifications and both runner profiles (`python-3.11`, `node-24`, now pinned to the `97c7e33` images) are **stale**, with expired TTLs. Run fresh capability and runner preflights before any connected WorkItem.

## Data

- Schema: migrations 1–55 applied; latest is `0055_inference_evaluation_scope`, and main adds no new migration. The readiness `database_generation.schema_version` field shows `0049`, the generation's *initializing* version, not the current schema.
- Counts (unchanged across the release): 14 WorkItems, 82 Runs, 0 hosted reconciliations, 0 hosted operations, 0 hosted operation locks, 0 agent leases or active claims, 69 inference evaluations, 4 retention holds.
- Archive: `pre-release-97c7e33-20260925` on PVC `pharness-data-archive-legacy-20260826`. Manifest `sha256:747cbd3acb0a9b750e279d6bd0932d052343e7a626e4c0e6b15be38141ab5445`, database `sha256:4b10bacc84c0c2f5c1c5431a9fc52e8005c8f63be10776e2ed18aaa9ad70d4ee`. Independently verified: hash matches, `integrity_check=ok`, all migrations successful.

## Validation

| Check (at `3eaa312`) | Result |
| --- | --- |
| `cargo fmt --all -- --check` | **failed** (`pharness-worker/src/main.rs`); fixed by [#420](https://github.com/lward27/pharness/pull/420) |
| `cargo clippy --workspace --all-targets -D warnings` | pass |
| `cargo test --workspace` | exit 0; 875 passed (8 `hidden_semantics` lines are seeded fixtures) |
| `check-app-module-boundaries.sh`, `test_app_module_dependencies.py` | pass |
| UI `npm ci` / `vitest` / `vite build` | pass / 97 of 97 / pass |
| Playwright | 136 passed, 1 skipped; the real-server spec timed out on the cargo lock, then passed when rerun alone |
| CI | [#420](https://github.com/lward27/pharness/pull/420), [#421](https://github.com/lward27/pharness/pull/421), [#422](https://github.com/lward27/pharness/pull/422) merged with the secret scan passing. The secret scan is the only GitHub workflow; fmt, clippy and test run only locally. |

## Qualification

| Stage | Policy | Protocol | Qualification | IDs | Failure class |
| --- | --- | --- | --- | --- | --- |
| Test Diagnosis | `test-diagnosis-nemotron-v2@v1` (`sha256:ebdd7edb…44f1`) → `fireworks-nemotron-lightning-3p5-30b-a3b@v1` | **failed** | not dispatched | op `astra-localprep-protocol-test-diagnosis-20260925`; receipt `inferverify_01a0d8fe43c0787190b720c22848a461` | model protocol behavior: `single_tool_call` attempt 1 did not return exactly one native tool call |
| Verifier | `verifier-glm-5p3-v2@v1` | not run (stop rule) | not run | none | none |
| Builder | `builder-kimi-k2p7-code-v2@v1` (qualification `passed`, protocol not ready) | not run | none | none | none |
| Repair | `repair-kimi-k3-v2@v1` (qualification `passed`, protocol not ready) | not run | none | none | none |

Planner (`planner-kimi-k3-v2@v1`) is still qualified: it passed 24/24 on `49ecdc5` (`inferqual_01a0cfba2d307d52b5fefec2aa20106c`). All policy hashes were unchanged by the release.

## Test Diagnosis timeout

**It did not recur.** The preflight failed after 14.1 s with a response-shape failure, not the 300 s timeout. The gateway logged a single `upstream request started` (correlation `verify_inferverify_01a0d8fe43c0787190b720c22848a461_1_1`, attempt 1 of 3, first-response timeout 300 s) and no warn- or error-level transport events. The coding proxy logged nothing in the window, and the API returned 200. No hop stalled; the provider answered and the calibration rejected the answer. PR #404's success-path events (`upstream response headers received`, `stream first chunk/completed`) are `tracing::debug!`, and the gateway's default filter is `pharness_model_gateway=info`, so a successful attempt shows only its start line. **Code task:** promote the terminal success events (headers with status, stream completed) to `info` so that every attempt has a visible outcome. Also consider a sanitized per-case reason in the protocol receipt (tool-call count or finish reason, no content).

## Open PR inventory

All six drafts form one stack: #356 → #357 → #358 → #361 → #363 → #366. Each PR's base branch is the previous PR's branch; #356 and #363 target `main`. #366 contains every commit from the others. `git cherry` finds none of their commits on main by patch-id. Conflicts were measured by merging `a0d748b` into each head in a scratch worktree, then discarding the merge. All conflicts are in docs; no code files conflict. The merged code was **not** compiled or tested.

| PR | Head | Base (merge-base) | Behind main | Conflicting files | Already on main another way | Recommendation |
| --- | --- | --- | --- | --- | --- | --- |
| [#356](https://github.com/lward27/pharness/pull/356) runtime evidence binding | `4d225e4` | `19b0c55` | 148 | 2 (`ASTRA-00-PROGRAM.md`, `ASTRA-08-…VERIFICATION.md`) | ~7 of 40 sampled new functions already exist on main by name, so there is partial overlap | Close as superseded by a rebased #366, or fold into it |
| [#357](https://github.com/lward27/pharness/pull/357) probe traces | `34c7f81` | `19b0c55` | 148 | 2 (same) | ~6 of 24 | Same as #356 |
| [#358](https://github.com/lward27/pharness/pull/358) observer access | `ccde01f` | `19b0c55` | 148 | 2 (same) | 0 of 4 | Same as #356 |
| [#361](https://github.com/lward27/pharness/pull/361) staging baseline admission | `c9febec` | `19b0c55` | 148 | 2 (`README.md`, `ASTRA-00-PROGRAM.md`) | ~6 of 36 | Same as #356 |
| [#363](https://github.com/lward27/pharness/pull/363) staging runtime verification | `12d3a06` | `5a40945` | 73 | 3 (`README.md`, `ASTRA-M08-5A40945-INTEGRATION.md`, `ASTRA-00-PROGRAM.md`) | High name overlap, because it merged main at `5a40945` | Rebase or merge main into #366 only |
| [#366](https://github.com/lward27/pharness/pull/366) production authority | `02e4c0e` | `5a40945` | 73 | 3 (same as #363) | ~9 of 17 | **Keep as the single carrier.** Retarget to `main`, merge main, resolve the 3 docs conflicts, run the full suite, and split into reviewable PRs (observation/evidence, staging admission, production authority) if it's too large (~211 files) |

## Environment facts the cloud agent cannot see

- **Cluster:** context `lucas_engineering`, API `https://192.168.20.192:6443`, k3s on 3 nodes: `ubuntu-lucas-engineering` (control plane; holds the `local-path` API data PV), `ubuntu-lucas-engineering-2` (evaluation Jobs, `PHARNESS_INFERENCE_EVALUATION_NODE_HOSTNAME`), and `ubuntu-lucas-engineering-build` (label and taint `workload=build`, which hosts the `k3s-buildkit` Pod).
- **Namespaces:** `pharness`, `apps-staging` (Finance staging: `finance-frontend`, `yfinance-wrapper`), `apps-prod` (production Finance and other apps), `argocd`, `tekton-pipelines`, `monitoring`.
- **Argo applications:** `pharness` (source pharness repo `deploy/helm/pharness`, `HEAD`, value file `values-yfinance-production.yaml`, automated prune and selfHeal); `finance-frontend`, `yfinance-wrapper`, `finance-app-database-service` (lucas_engineering `charts/*`, `HEAD`); `finance-frontend-staging`, `yfinance-staging` (lucas_engineering `charts/finance-staging/{frontend,yfinance}`, `main`); plus `root-app`, `tekton-ci`, `loki`, `mimir`, `tempo`, `prometheus`, `opentelemetry-collector`.
- **Tekton pipelines** in `tekton-pipelines`: `clone-build-push` (PHarness images; SA `tekton-ci-build`; in-cluster BuildKit service `k3s-buildkit:12340`), `pharness-yfinance-build`, `pharness-finance-frontend-build`, `pharness-e2e-build-output`, `pharness-e2e-noop`. Deterministic run names follow `pharness-<component>-<sha12>`.
- **Registry:** `registry.lucas.engineering` (in-cluster alias `docker-registry.registry.svc.cluster.local:5000`).
- **Telemetry services** (all in `monitoring`): `tempo:3200` (OTLP 4317/4318), `loki:3100` (also `loki-gateway:80`), `mimir-gateway:80`, `prometheus-server:80`, `opentelemetry-collector:4317/4318`. The API uses `PHARNESS_PROMETHEUS_URL=http://prometheus-server.monitoring.svc.cluster.local:80` and `PHARNESS_LOKI_URL=http://loki.monitoring.svc.cluster.local:3100`; no Tempo URL is configured. Loki, Mimir and Tempo `/ready` passed via the API-server proxy. CNI and Cilium/Hubble were not discovered, so there is no flow evidence.
- **Live-only Helm values that matter for code:** `values-yfinance-production.yaml` enables `gitOpsWriter` and `gitOpsObserver` for `lucas_engineering` only, and restricts `argoExecutor.allowedApplications` to **`yfinance-wrapper` only**. Staging Argo apps (`yfinance-staging`, `finance-frontend-staging`) are not in the executor allowlist. The protected target is `apps-prod/yfinance-wrapper`. `PHARNESS_WORKER_K8S_MAX_CONCURRENT_RUN_JOBS=1`. The workspace node is `ubuntu-lucas-engineering`. `PHARNESS_FIREWORKS_MODEL=accounts/fireworks/models/kimi-k2p6`.
- **Credentials exist and are readable** by the operator identity (checked by name only): `pharness-operator-token`, `pharness-git-writer-token`, `pharness-gitops-writer-token`, and the `pharness-gitops-observer-token` and `pharness-git-observer-token` observers. PHarness and GitHub authentication checks passed via `lucas-ops doctor --credentials`.
- **Data PVCs:** live `pharness-api-data-finance-20260827`; archive `pharness-data-archive-legacy-20260826`. 53 old `pharness-run-*` and `pharness-ws-*` workspace PVCs (25–31 days) remain; that cleanup is outside this handoff's authority.

## Codex skills summary

The shared skills are in `~/.agents/skills/` (git-tracked). Three of them are also maintained in lucas_engineering `tools/cluster-ops/skills`. `~/.codex/skills/` holds only generic system skills, and `~/.codex/AGENTS.md` is empty (the policy text is in `agents.md.bak`).

| Skill / tool | What it does |
| --- | --- |
| `lucas-ops` CLI (`~/.local/bin/lucas-ops`, source `tools/cluster-ops`) | `doctor` (read-only readiness with `--credentials` and `--builder`); `builder preflight` (uncached AMD64 build on the Mac builder); `evaluation status\|watch\|start` (durable evaluation reads, and one-POST qualification dispatch with an exclusive record); `release verify\|pin` (registry and OCI verification of 7 images plus the bundle; delegates to `pharness-release-pin.sh`). It has **no** protocol-preflight command. |
| `immutable-build-release` | Source SHA → Tekton → digest → pin → Argo → Pod imageID chain, with stop conditions. Includes `verify_remote_revision.sh`. |
| `k8s-rollout-observe` | Timed rollout observation against stated success and failure criteria. |
| `k8s-cluster-health` | Bounded whole-cluster health (`collect-health.sh`); use `lucas-ops doctor` for PHarness readiness. |
| `k8s-workload-triage` | Diagnose one unhealthy workload. |
| `tekton-run-triage` | Diagnose one failed or stuck PipelineRun without leaking credentials. |
| `tekton-pipeline-create` | Author or run a Tekton pipeline. |
| `k8s-change-review` | Pre-merge review of Helm, Kustomize, Argo or Tekton changes. |
| `k8s-app-deploy` | Deploy or update an app via the repository's GitOps convention. |
| `argocd-app-onboard` | Register an app in the Argo hierarchy. |
| `k8s-namespace-provision` | Governed namespace creation. |
| `k8s-incident-snapshot` | Sanitized incident evidence bundle. |
| `cilium-connectivity-triage` | Network and policy triage (Cilium is not installed on this cluster). |
| `agent-loop-deploy` | Bounded agent workload deployment. |
| `_shared` | Cluster profile, safety policy (Class A/B/C), redaction and context-guard scripts. |

Work the local agent can execute for you later: builds and pins (about 60 min for all 8 PipelineRuns), archive Jobs, capability and runner preflights, protocol preflights, qualifications (`lucas-ops evaluation start`), and rollout observation.

## Decisions needed from Lucas

1. **Test Diagnosis model:** Nemotron has now failed 3 of 4 exact-policy protocol checks (two timeouts, one tool-call-shape failure). Recommended: add or switch the `test-diagnosis` V2 policy to the Kimi target, which scored 2/2 as the control on `d675159`. That needs a reviewed inference-registry change (new policy and registry hash), a release, and then protocol plus qualification. The alternative is to authorize exactly one more Nemotron preflight.
2. **Qualification coupling:** stop tying stage qualification to unrelated runtime releases. Receipts bind `runtime_revision`, so every image release invalidates protocol readiness. Consider binding to the gateway and inference-registry identity only, which is a code change the cloud agent can make.
3. **Staging-first activation:** enable V2 and hosted creation for the `finance-yfinance` **staging** binding only, after Test Diagnosis, Verifier, Builder and Repair are protocol-ready. The Argo executor allowlist would also need `yfinance-staging`.
4. **Draft stack:** approve consolidating #356–#363 into #366 (then close the rest), or explicitly retire the stack.
5. **Doctor profile drift:** update lucas_engineering `tools/cluster-ops/profiles/lucas_engineering.json` so the builder check targets the in-cluster `k3s-buildkit` endpoint instead of the retired Mac route. Your local lucas_engineering checkout has related uncommitted Tekton build-scheduling edits.

## Surprises

- `~/.codex/skills/` has no ops skills and `~/.codex/AGENTS.md` is empty. The real skills are in `~/.agents/skills/`.
- `lucas-ops doctor` reports `blocked` only because its profile still expects the Mac BuildKit route. The checkpoint also says `lucas-ops` is absent and cites a `/home/wardl` path, but it is installed at `~/.local/bin/lucas-ops`.
- The release pin lives in the **pharness** repo, not lucas_engineering. Argo tracks pharness `HEAD`, so any merge to main is a deploy trigger, although digests only change with a pin.
- `origin/main` was not fmt-clean. GitHub CI only runs the secret scan, so fmt, clippy and test regressions can land unnoticed.
- `pharness-build.sh` could not complete on macOS: a jq 1.7 object-literal syntax error, plus a missing `--arg build_target` in the reuse validator. `pharness-data-archive-job.sh` failed under bash 3.2 with no WorkItem IDs. All three are fixed in #422.
- `pharness-verify-build-revision.sh` verifies the *source revision*, not image digests. Digest verification is `lucas-ops release verify`.
- The Test Diagnosis failure mode changed from a transport timeout to model tool-call behavior. The gateway's success-path transport logs are debug-level, so the PR #404 diagnostics are invisible for a successful attempt at the default filter.
- Readiness does not report V2, hosted workflow, or the true schema version (it shows the generation's initializing `0049`).
- The auto-mode guard blocked a read-only bulk copy of the live SQLite file. Schema and counts came from the API inventory and the verified archive instead; no live database query was run.
