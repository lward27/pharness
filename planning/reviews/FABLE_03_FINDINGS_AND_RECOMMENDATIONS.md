# FABLE 03: Findings and Recommendations

Reviewed 2026-10-17 at `dd55eec`. Findings are ordered by impact on reaching a fully autonomous SDLC. Each carries a severity, the evidence, and a concrete recommendation. Recommendations respect the ASTRA boundaries (single-writer SQLite, GitOps-only delivery, human production approval, no premature adapter framework) unless a finding argues for revisiting one.

Severity scale: **Blocker** (prevents autonomous operation), **High** (undermines safety or trust claims), **Medium** (operational or structural risk), **Low** (hygiene).

---

## Part 1: Findings

### FB-01 · Blocker · The hosted controller stops at the staging GitOps commit

**Evidence.** `crates/pharness-api/src/app/hosted_controller/progression.rs:36-95` selects candidates in order: repo-mode actions → `build::candidate` → `staging::candidate` → `continue_stage` → `waiting`. There is no production, release, or observe candidate. `staging.rs:134` and `staging/callbacks/outcomes.rs:145,199` write `"deployment_verification":"pending","runtime_verification":"pending"`; a grep over the crate shows no other writer of those fields. `repo_mode/state.rs:52-54` returns early for hosted items, so `release` and `observe` outcomes are never sealed. The 0052 completion trigger requires eight succeeded outcomes, so a hosted WorkItem can never complete.

**Consequence.** Even with coding qualified and V2 enabled, the first autonomous WorkItem would deliver a staging digest commit and then sit in `running` for 60 minutes before `wait_expired`.

**Recommendation.** Land M08/M09 as controller candidates following the existing pattern:
1. `verify_staging_deployment` candidate: requires `stage_verified_build` succeeded; dispatches a read-only observer Job (or runs in-process `ReadOnlyClusterTools`) that polls `ArgoGetApp` for `Synced/Healthy` at the committed GitOps revision, checks rollout, and asserts pod `imageID` ends with the built digest. Resolve `deployment_verification`.
2. `verify_staging_runtime` candidate: 5-minute window with bounded Prometheus/Loki/Tempo reads plus the contract's `health_path` probe; outcome `succeeded | failed | inconclusive`. Resolve `runtime_verification`. Seal `release.staging`.
3. `request_production_approval` candidate: materialize approval material (digest, GitOps diff preview, staging evidence IDs, target, current healthy production imageID) into an `approval_gates` row of kind `production_deployment`; condition `awaiting_human`.
4. `promote_production` candidate: requires a current approval whose material hash still matches; re-validate immediately before dispatching the GitOps writer (same `validate_binding` discipline as `source_merge.rs:322-440`); observe Argo + imageID; 10-minute runtime window; seal `release.production`.
5. `observe` candidate: post-release observation window; seal `observe`; WorkItem completes.
6. Expose rollback as a hosted candidate (`rollback_production`) reusing `work_items/rollback.rs` logic, gated on a confirmed regression classification and the single-rollback permission in the policy snapshot.

The drafts #363 (851 checks) and #366 (216 checks) reportedly cover much of 1–4; the work is integration into `progression.rs`, not greenfield.

### FB-02 · Blocker · The platform has no CI for itself

**Evidence.** `.github/workflows/` contains only `secret-scan.yml`. `README.md:38-40` lists `cargo fmt/clippy/test` as developer commands. `scripts/check-app-module-boundaries.sh`, `scripts/tests/*`, vitest, Playwright, `helm lint`, and `pharness-eval run --provider replay` are never executed automatically. Playwright baselines are all `-darwin.png` (92 files), so they cannot run on Linux. `tests/helm.rs` in the gateway crate is `#[ignore]`d.

**Consequence.** The system that enforces "no merge without passing required checks" on target repositories merges its own changes without any required check. `source_merge.rs:168` even hard-codes a required check named `"Source integrity"` for *target* repos. The project's own M03/F11 architecture guardrails are honor-system.

**Recommendation.**
- Add `ci.yml`: `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace` (with `python3`, `node`, `git` installed so replay suites run), `scripts/check-app-module-boundaries.sh`, `scripts/tests/*.sh` and `*.py`, `helm lint` + `helm template` + `tests/helm.rs` un-ignored, `npm ci && npm test` in `ui/`.
- Regenerate Playwright baselines on a Linux runner (or run in the `mcr.microsoft.com/playwright` container) and store them as `-linux.png`; keep darwin baselines for local use only.
- Make `ci.yml` a required branch-protection check on `main`, alongside the existing secret scan.
- Add `cargo audit` (or `cargo deny check advisories licenses`) and `npm audit --audit-level=high` as advisory jobs first, then required.

### FB-03 · High · Production approval identity is client-asserted

**Evidence.** `approvals.rs` contains no reference to `OperatorIdentity`. `decide_approval_gate` (`:529,561`), `batch_decide_approval_gates`, `approve_approval` (`:998`), `deny_approval` (`:1015`) take `decided_by` from the request body. Audit actor defaults to `"api"` (`:856,1155`). `auth.rs:49-79` already inserts `OperatorIdentity(name)` when tokens are configured, so the authenticated principal is available and ignored. With no tokens configured (loopback dev), everything is anonymous.

**Consequence.** The central safety claim ("a real human approved this exact digest for production") cannot be proven from the audit trail; any holder of any operator token can record any name.

**Recommendation.**
- Make `OperatorIdentity` mandatory (not `Option<Extension<_>>`) on all approval, gate, grant-creation, and control (`pause/resume/cancel`) handlers. Derive `decided_by` from it; reject or warn on a body `decided_by` that disagrees.
- Record token name, request ID, and material hash on every decision; include them in `audit_events`.
- Longer term (SECURITY_REVIEW H1 ties in): put the API behind OIDC (oauth2-proxy or ingress-nginx `auth-url`) so identity is a verified subject rather than a shared bearer name. Keep bearer tokens for worker/internal routes.

### FB-04 · High · Coding reliability is not yet qualified on the serving runtime

**Evidence.** `coding_reliability_v2.enabled=false`; Test Diagnosis stopped at `gateway protocol verification timed out` (intermittent, root cause unknown; Cilium/Hubble absent so flow evidence unavailable); Builder/Repair rows are from `48c77b7`, not the current runtime; M04E connected loop and M04F frozen qualification are open. Transport diagnostics were added (PR #404) but the images were not deployed at review time.

**Consequence.** No WorkItem can legally enter `implement` under hosted policy; FB-01 is moot until this passes.

**Recommendation.** This is on the project's critical path already and the documented next step (deploy `72fe079`, re-read hashes, one Test Diagnosis preflight) is correct. Two additions:
- Treat the intermittent gateway→proxy timeout as a reliability bug, not only a qualification blocker: add a gateway `/metrics` histogram for upstream connect/first-byte/idle (see FB-07) so the next occurrence has data.
- Make the replay suites (`coding_v2::tests::replay_patches_pass_public_and_hidden_checks_without_scope_drift`, `every_stage_qualification_replay_fixture_passes`) run in CI (FB-02) so harness regressions are caught before a live run burns budget.

### FB-05 · High · Operational resilience is below unattended-operation requirements

**Evidence.** `api.yaml`: 1 replica, `Recreate`, SQLite on `local-path` RWO PVC; worker PVCs pinned to `ubuntu-lucas-engineering`, eval runner to `ubuntu-lucas-engineering-2`; no PDB; `pharness-data-archive-job.sh` is manual with no restore procedure; no `/metrics`, ServiceMonitor, or PrometheusRule; `CONTROLLER_WAIT_MAX_CHECKS = 240` expiry is silent. Gateway nonce cache is in-memory (`main.rs:697-709`).

**Consequence.** M12 requires 24 hours unattended with interruption recovery. Today a stalled controller, a full PVC, a node drain, or an expired operation produces no alert. Restore from backup has never been rehearsed.

**Recommendation.** Within the single-writer constraint:
- **Alerting first:** expose `/metrics` from the API (controller tick age, due-but-unclaimed WorkItems, operations by condition, Job creation failures, SQLite file size/free space) and the gateway (upstream latency, failure class counts, grant rejections). Add a `ServiceMonitor` and a `PrometheusRule` for: no controller tick in 2 min, any operation `wait_expired`, any `blocked` condition older than N minutes, PVC > 80%. Route to whatever the cluster already uses (Alertmanager → Slack/email).
- **Backup:** add a `CronJob` running the existing archive command nightly to the archive PVC, plus an off-node copy (object storage or a second node). Write and rehearse a restore runbook under `planning/operations/runbooks/`.
- **Storage:** move the SQLite PVC off `local-path` to a replicated or network-backed StorageClass if the cluster has one (Longhorn, NFS, etc.) so the API can reschedule; add a PDB (`minAvailable: 0`, `maxUnavailable: 1`) to make intent explicit.
- **Nonce durability:** persist gateway nonces to SQLite via the API or accept the risk explicitly and bound grant expiry tighter (document it).
- **Revisit the SQLite-forever decision after M12.** The `postgres` sqlx feature is already compiled in. A Postgres backend would allow 2+ API replicas with the existing `claim_due_workflow` leases, remove node pinning, and simplify backup. This is a post-acceptance change, not a current one.

### FB-06 · High · No supply-chain attestation for anything the platform ships or builds

**Evidence.** No cosign, syft, SBOM, or SLSA provenance in `deploy/`, `scripts/`, or `.github/`. Evidence JSON records `sbom_verified: false`. `Dockerfile.runtime:32-34` fetches `kubectl` without a checksum. No `cargo audit`/`cargo deny`; `npm audit` disabled in `Dockerfile.ui`. Target-application builds (yfinance, frontend) via Tekton also produce only a digest.

**Consequence.** "Digest identifies an artifact" is true, but an autonomous system that merges and deploys unattended should be able to prove what is inside the artifact and that the build was not tampered with.

**Recommendation.**
- Add `syft` SBOM generation and `cosign` keyless signing (Fulcio/Rekor, or a cluster KMS key) as Tekton tasks in `clone-build-push` for both PHarness and target application pipelines. Record SBOM digest and signature in `registry_evidence`.
- Make `verify_release` / staging verification check the signature before accepting a digest.
- Pin and checksum `kubectl`. Add `cargo deny` (advisories, licenses, bans) and `npm audit` to CI (FB-02).

### FB-07 · Medium · PHarness is a bespoke installation, not a product

**Evidence.** Compile-time constants: `hosted_workflow/delivery.rs:14-38` (two Finance repos, images, pipelines, Argo apps, chart paths, ports, health paths), `system.rs:32-43` (protected target), `source_merge.rs:168-169` (GitHub App ID, check name), `releases.rs:627` (`app=yfinance-wrapper`), `progression.rs:129` (`gitops:lucas_engineering:main`), `staging_gitops/github.rs:10-11`, `tempo.rs:17,29`, `pipeline/lucas.rs`. `values.yaml` carries hostnames, node names, context name, product/contract IDs.

**Consequence.** Onboarding a third application requires code changes and a release. The ASTRA decision to avoid a premature adapter framework is sound for *proving the loop*; it should be time-boxed.

**Recommendation.** Do not build an adapter framework now. Instead, as a low-risk refactor after FB-01 lands: move the per-application tuple in `delivery.rs` and the protected-target constants into the `LucasDeliveryBinding` that already exists in `hosted_sdlc.rs` and is already snapshotted per WorkItem. Validation stays strict (same-application coherence checks), but the data comes from configuration, not `match` arms. This keeps the boundary and removes the "two repos only" rule without widening authority.

### FB-08 · Medium · Verification is narrower than "SDLC" implies

**Evidence.** The verify stage is an independent agent reading requirements, changes, and test evidence. There is no platform-level SAST, dependency audit, license check, or content secret scan of the produced diff (only secret-shaped *path* rejection). Test quality is not measured; flaky tests have only an `identical_failures: 2` budget.

**Recommendation.** Add two deterministic, model-free stage steps alongside `test`, both configurable in `.pharness/repository.yaml` so they remain repo-owned: (a) `audit` running the stack's dependency audit tool against the lockfile, and (b) `scan` running Gitleaks on the diff and an optional SAST command. Record results as typed evidence validators; fail closed on high-severity findings. This fits the existing `execute_deterministic_test` pattern.

### FB-09 · Medium · Observe stage and closed loop are absent

**Evidence.** `observations`, `incidents`, `remediation_plans` exist and are read-only/draft. `observe` is never sealed for hosted items. Incident → WorkItem is explicitly deferred. There is no notification path.

**Recommendation.** Within the current boundary: implement `observe` as a sealed post-release window (FB-01 step 5) and emit a notification (webhook/Slack) on `awaiting_human`, `failed`, `blocked`, `wait_expired`, and `incident created`. Notifications are not "incident initiation"; they are the minimum for a human to supervise exceptions without polling. Defer incident-initiated WorkItems as planned, but design the `Incident` → proposed `WorkItem` hook now so it is a feature flag later, not a redesign.

### FB-10 · Medium · Two controllers with different semantics

**Evidence.** Timer-driven `hosted_controller/*` vs API-triggered `work_items/{reconcile,waits}.rs` (`POST /api/controller-waits/reconcile-due` needs an external caller). The legacy controller owns rollback and Argo sync; the hosted controller cannot reach them.

**Recommendation.** After FB-01 lands, either (a) fold the legacy controller's reachable effects (Argo sync, rollback) into hosted candidates and retire `controller_waits`, or (b) have the hosted scheduler tick call `reconcile_due` so there is one clock. Prefer (a); it also removes one of the triplicated callback handler sets.

### FB-11 · Medium · Structural debt that will slow the remaining milestones

**Evidence.** `dispatch.rs` 5.4k, `worker.rs` 4.5k, `worker/src/main.rs` 5.8k (16 modes by env string), `store/src/sqlite.rs` 9.9k, `cli/src/main.rs` 9.6k; `run_loop` ~1,230 lines with a nine-times-repeated block; triplicated git/gitops callback plumbing; `internal.rs:57-152` prefix dispatch on `rollback_`; `OperationalMode::from_env()` per request; feature flags read in the API rather than `pharness-config`; `fireworks` crate's legacy path dead for `ModelProvider` consumers but still tested; V1/V2 prompt, profile, context, and UI duplication.

**Recommendation.** Do not pause delivery for a refactor. Apply these opportunistically:
- Split `worker/src/main.rs` by execution kind into modules under `worker/src/modes/`, each exposing `run(ctx)`; `main` becomes a 30-line dispatch table.
- Extract the repeated protocol-correction block in `run_loop` into one function; the nine call sites become one.
- Unify the three git/gitops callback handler families behind one `DeliveryCallbacks` trait keyed by intent kind instead of string prefix.
- Move all `PHARNESS_*` behavior flags into `ApiRuntimeConfig` and read `OperationalMode` from `AppState` (refresh on SIGHUP or a config endpoint if live toggling is needed).
- Delete the dead `pharness-fireworks` streaming path once nothing imports it; keep the crate as a thin alias or merge into `openai-compatible`.
- Lower the architecture guardrail limits (`check-app-module-boundaries.sh`: 1500/3500/4000) once CI enforces them, and add the worker and store crates to the check.

### FB-12 · Low · Documentation volume obscures the status signal

**Evidence.** `planning/` is 922 tracked files, ~790 of them evidence artifacts; `ASTRA-00-PROGRAM.md` status table cells are multi-sentence paragraphs; the checkpoint, addendum, master, and M04 doc all restate overlapping state. The content is honest and consistent; it is simply expensive to read.

**Recommendation.** Keep the evidence discipline. Add one machine-readable `planning/STATUS.yaml` (milestone → status enum, served image digests, schema version, flags) regenerated by the release-pin script, and render a short table from it at the top of the README. Move prose status into the milestone docs only.

### FB-13 · Low · Hygiene

- Untracked `tutorial-site/` duplicates runbook content with no provenance; decide to commit under `planning/` with a generation note or delete.
- `.gitleaks.toml` allowlists a commit ID string; fine, but document why in the file.
- SECURITY_REVIEW C1 (historical token in git history) still lacks rotation evidence.
- Chart `version: 0.1.0` never changes; bump with each release pin so Argo history is meaningful.
- `TrustedWrites` ignores `require_approval_for_writes` in both match arms (`safety_policy.rs:281-290`); confirm intentional and comment, or fix.
- `unwrap()` on `workflow_policy` in hosted paths (`progression.rs:163` and others); convert to `ApiError` for defense in depth.

---

## Part 2: What *not* to change

These are deliberate and correct; the review recommends preserving them.

- **Single-writer API with all effects as isolated Jobs.** It is the reason retries are safe and credentials never reach coding workers. Keep it until Postgres is justified post-acceptance (FB-05).
- **Fetch-context → single-use admission → one mutation → observe → outcome.** Preserve for every new candidate in FB-01.
- **Agent output is a claim; controller seals.** Do not let the verifier or any model seal its own outcome, including for the new runtime-verification steps.
- **Approval before production GitOps merge.** Required because Argo auto-syncs. Never add a "merge then approve" path.
- **Content-hashed inference bindings and exact-runtime qualification.** Expensive but correct; it is what makes "qualified" a meaningful word.
- **Frozen suite with hidden tests and policy violation detection.** Keep thresholds in code with a pinned hash.
- **Honest documentation separating implemented / deployed / qualified / accepted.**

---

## Part 3: Recommended sequence

This respects the ASTRA milestone order while pulling forward the platform work that M11/M12 will require anyway.

| Step | Scope | Unblocks | Finding |
| --- | --- | --- | --- |
| 1 | Add `ci.yml` (fmt, clippy, test incl. replay, guardrails, helm, UI, script tests); Linux Playwright baselines; make required | Every later step is protected | FB-02 |
| 2 | Bind approvals/controls to `OperatorIdentity`; audit token name + request ID | Human production approval becomes provable | FB-03 |
| 3 | `/metrics` on API + gateway, ServiceMonitor, PrometheusRule, nightly archive CronJob, restore runbook | Diagnoses FB-04 timeouts; prerequisite for M12 soak | FB-05 |
| 4 | Deploy `72fe079`, finish M04E Slice 3/4 and M04F (project's current path) | V2 enabled; first connected WorkItem | FB-04 |
| 5 | Integrate M08 (#363) and M09 (#366) as hosted candidates; add `observe` sealing; hosted rollback candidate; notifications | Hosted WorkItem can complete; M11 possible | FB-01, FB-09 |
| 6 | SBOM + cosign in Tekton for PHarness and target apps; signature check in verification | Supply-chain claim matches autonomy claim | FB-06 |
| 7 | Deterministic `audit`/`scan` steps from repository contract | Verification breadth | FB-08 |
| 8 | M11 (two real Finance WorkItems, human approvals), M12 (24h soak) | Program acceptance | — |
| 9 | Post-acceptance: move Finance tuple into `LucasDeliveryBinding` config; retire legacy controller; worker/`run_loop`/callback refactors; evaluate Postgres | Third application without a code change; two replicas | FB-07, FB-10, FB-11 |

Steps 1–3 are small (days each), independent of the qualification blocker, and remove the three findings most likely to embarrass the system once it operates unattended. Step 5 is the largest and is where the "autonomous" word is earned.

---

## Part 4: Answer to the original question

**Are all the right pieces present for a fully autonomous SDLC?**

The *architecture* has the right pieces, and several of them (evidence sealing, effect isolation, inference governance, approval envelopes) are better than typical. The *implementation* is missing the last third of the loop (staging verification → production approval execution → release sealing → observe → hosted rollback), the *platform* is missing the self-protection an unattended system needs (CI, alerting, backup/restore, HA path, attestation), and the *trust model* has one gap that must be closed before any human approval is relied upon (authenticated approver identity). None of these requires an architectural reset; all fit the existing patterns. The project's own ledger already identifies most of them; this review adds the platform-self-operation and identity items, which the ASTRA milestones treat as implicit rather than gated.
