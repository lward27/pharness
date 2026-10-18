# FABLE 01: PHarness System Architecture (as built)

Reviewed 2026-10-17 at `dd55eec`. This document describes what the code does, with file references. Planning intent is noted only where it explains a boundary. See [FABLE_00](FABLE_00_EXECUTIVE_SUMMARY.md) for the summary and [FABLE_02](FABLE_02_AUTONOMOUS_SDLC_CAPABILITY_MATRIX.md) for the capability grading.

## 1. Shape of the system

PHarness is a Rust workspace of 12 crates plus a React/Vite console, deployed by one Helm chart into a Kubernetes namespace alongside a target cluster's Tekton, Argo CD, private registry, and LGTM stack.

```
                    ┌──────────────────────────────────────────────────────────────┐
  Operator ──────▶  │ pharness-ui (nginx, injects bearer)  ──▶  pharness-api        │
  (Basic Auth)      │                                           (axum, 1 replica)   │
                    │                                           ├─ SQLite /data PVC │
                    │   hosted controller (15s tick) ───────────┤   single writer   │
                    │                                           ├─ kubectl ─────────┼──▶ K8s Jobs (one per effect)
                    │                                           └─ reqwest ─────────┼──▶ model-gateway
                    └──────────────────────────────────────────────────────────────┘
                              │                                        │
       ┌──────────────────────┼─────────────────────┐                  ▼
       ▼                      ▼                     ▼           pharness-model-gateway
 runner Jobs            writer/observer Jobs   executor Jobs    (HMAC grant verify, policy
 (python/node image,    (git PR/merge, gitops  (tekton_trigger,  enforce, SSE proxy)
  pharness-worker →      commit, observe)       argo_sync)             │
  pharness-runhost)          │                     │           ┌───────┴────────┐
       │                     ▼                     ▼           ▼                ▼
  coding egress proxy    GitHub API        Tekton / Argo CD   Fireworks /    local llama.cpp
  (CONNECT allowlist)                      (via kubectl)      OpenRouter     (Minisforum)
```

### Crates

| Crate | LOC | Role |
| --- | --- | --- |
| `pharness-api` | 98k | Control plane: HTTP API, hosted controller, Job dispatcher, stage sealing, approvals, releases, inference registry, data lifecycle |
| `pharness-store` | 25k | SQLite persistence; 55 additive migrations, ~100 tables, trigger-encoded invariants |
| `pharness-core` | 19k | Domain types: agent runtime loop, SafetyPolicy, tool executors (fs/shell/cluster), inference registry + HMAC grants, hosted SDLC authority documents, repository contract |
| `pharness-eval` | 10k | Qualification harness: frozen coding suite (24 tasks), stage suites, replay providers, gate thresholds |
| `pharness-cli` | 9.6k | Thin HTTP client CLI over the API (one file) |
| `pharness-worker` | 7.6k | One-shot Job binary with 16 execution modes selected by `PHARNESS_EXECUTION_KIND` |
| `pharness-runhost` | 6.4k | Shared agent-loop host (prompts, context envelope, tool stack, patch application, acceptance commands) |
| `pharness-codex-host` | 4.7k | Portable alternative execution backend driving OpenAI Codex `app-server` (expansion deferred) |
| `pharness-config` | 2.7k | TOML + ~130 `PHARNESS_*` env overrides |
| `pharness-openai-compatible` | 2.1k | OpenAI-compatible client, gateway client, MiniMax quirk |
| `pharness-model-gateway` | 1.8k | Inference reverse proxy |
| `pharness-fireworks` | 1.3k | Legacy Fireworks client; `ModelProvider` impl delegates to openai-compatible |

Compile status: `cargo check --workspace --all-targets` passes (18s incremental). 836 `#[test]`/`#[tokio::test]` functions across crates.

## 2. Domain model

The semantic model (`planning/design/product-model.md`) is mostly realized in the schema:

- **Organization → Product → Repository (via RepositoryBinding) → WorkItem → StageExecution → StageOutcome** (migrations 0040, 0042). `effective_stage_outcomes` is the controller-selected pointer; sealed outcomes are immutable.
- **Lifecycle:** `discover → plan → implement → test → verify → source_delivery → release → observe` (`pharness-core/src/repo_mode.rs` `RepoStageKey`). Hosted completion requires eight succeeded outcomes (trigger in `0052_hosted_workflow_policy.sql:53-64`).
- **Effect resources:** `WorkPlan`, `ChangeSet`, `SourceDeliveryIntent`, `PipelineIntent`, `DeploymentIntent` (staging and production per build since 0054), `GitOpsChangeSet`, `Release`, `RegistryEvidence`, `RollbackIntent`.
- **Governance:** `approvals` (per tool call), `approval_gates` (per WorkItem: `source_mutation`, `git_mutation`, `pipeline_mutation`, `gitops_mutation`, `cluster_mutation`, `production_impact`, `production_deployment`), `permission_grants` (trusted envelopes), `audit_events`.
- **Observability:** `observations` (read-only cluster/LGTM facts), `incidents` (candidates), `remediation_plans` (drafts).
- **Inference:** `stage_inference_selections`, `inference_target_verifications`, `inference_policy_qualifications`, `model_grant_issuances`, `inference_evaluations` (0050, 0055).
- **Hosted workflow:** `workflow_policy` snapshot on `work_items` (0052), `hosted_reconciliations` / `hosted_operations` / `hosted_operation_locks` (0053).
- **Data lifecycle:** `database_generations`, retention policies/holds, `archive_records`, `sealed_run_summaries` (0049).

`sqlx` is compiled with the `postgres` feature but no Postgres code exists. SQLite WAL, pool of 5, migrations on a dedicated connection (`store/src/sqlite.rs:45-105`).

## 3. The control loop

### 3.1 Hosted controller (the autonomous driver)

- **Trigger:** one tokio task spawned at router construction in `Normal` mode (`hosted_controller/scheduler.rs:9-24`), ticking every `CONTROLLER_WAIT_INTERVAL_MS = 15s`. Worker callbacks persist evidence only; the next tick acts on it.
- **Claim:** process-wide `DISPATCH_BOUNDARY` mutex → `claim_due_workflow(owner, now, 60_000)` (60s lease, one WorkItem per tick) → load `Snapshot` (metadata, stage executions, runs, derived action list) → hash → `progression::advance` → `finish_workflow_reconciliation`. Whole pass under a 45s timeout; on timeout the claim and operation are left for the next owner (`scheduler.rs:67-73`).
- **Advance precedence** (`progression.rs:10-156`):
  1. Active operation exists → reconcile it (even when paused).
  2. Closed → `closed`. `control != active` → named condition. 240 unchanged checks (60 min) → `wait_expired`.
  3. First `ready` repo-mode action among `start_planner | approve_work_plan | authorize_stage_chain | approve_change_set | authorize_source_delivery`; else `build::candidate`; else `staging::candidate`; else `continue_stage`; else `waiting`.
  4. Each candidate requires the saved `workflow_policy.automatic_actions` to contain the matching `HostedAutomaticAction`. Approvals validate sealed stage outcomes, not DB flags (`hosted_controller/approval.rs`).
  5. Operation ID = `workflowop_<sha256([work_item_id, action, input_hash])>`; a `succeeded` op blocks re-dispatch. Resource locks: `coding`, `repository:<id>`, `delivery:<id>:staging`, `gitops:lucas_engineering:main`.
- **Recovery:** never "create another one." Recovery re-observes the exact persisted Job manifest (`dispatch/recovery.rs`) and only creates it if authority still holds and no admission was ever recorded.
- **Pause:** stops new dispatch; existing operation still reconciled; observers may still run, writers/builders do not.

**Stages actually driven for hosted items:** discover (sealed synchronously at creation from readiness) → plan → implement → test → verify → source_delivery (PR → checks → guarded merge → parent-SHA proof) → `build_verified_source` (Tekton) → `stage_verified_build` (one-file kustomization digest commit to staging). **Nothing after.** `deployment_verification` and `runtime_verification` are set to `"pending"` in three places (`staging.rs:134`, `staging/callbacks/outcomes.rs:145,199`) and never updated. No production, release, or observe candidate exists in `progression.rs`. Release/observe stage outcomes are never written for hosted items (`repo_mode/state.rs:52-54` returns early).

### 3.2 Legacy WorkItem controller

`work_items/{reconcile,waits}.rs` is an earlier, **API-triggered** controller for the pipeline → deployment → release chain (`POST /api/work-items/:id/reconcile`, `POST /api/controller-waits/reconcile-due`). It requires an external caller; there is no timer. It owns the rollback path (`work_items/rollback.rs`) and the Argo executor path (`deployment/execution.rs`). The hosted controller does not call into it for production.

### 3.3 Effect execution protocol

Every external mutation is a short-lived Job created by the API via `kubectl create -f -` (`dispatch.rs:4175`), with a mode-specific ServiceAccount and token identity:

```
fetch context (GET /api/internal/...)  →  validate authority hash locally
→ POST single-use admission            →  perform exactly one mutation (never retried)
→ observe                              →  POST outcome
```

Explicit in code: "Never retry this PUT after an uncertain response" (`worker/src/source_merge.rs:172`), "Do not retry admission" (`hosted_build.rs:47`), "Never retry the mutation" (`staging_gitops.rs:137`). Lost acknowledgements are resolved by an observer Job reading provider state, not by re-mutation.

Writers: `git_delivery` (branch + PR via REST), `git_delivery_merge` (`PUT /pulls/{n}/merge` after branch-protection and required-check validation), `gitops_delivery` (branch + PR), `hosted_staging_gitops` (GraphQL `createCommitOnBranch` directly on `main` of the GitOps repo), `tekton_trigger` (`kubectl create` PipelineRun), `argo_sync` (`kubectl patch` with `prune=false`). All cluster interaction is `kubectl` subprocess; there is no typed Kubernetes client.

## 4. Agent execution

### 4.1 Runner

`pharness-runhost::execute_attempt` (`lib.rs:828-1038`) runs in the worker Job (python or node runner image), in-process in the API (`LocalAttemptBackend`), and in the evaluator, via the `AttemptBackend` trait.

- **Tool stack:** `ProjectTools` (contract-aware writes, acceptance, workspace commands, evidence, typed submissions) + read-only fs + read-only cluster + shell, wrapped in `ProfileRestrictedTools` (per-AgentProfile allowlist). V2 profiles remove `run_shell` from the builder.
- **Writes:** gated by `.pharness/repository.yaml` `writable_paths` globs and secret-shaped path rejection. `apply_patch`: ≤256 KiB, 1–200 paths, no renames, `sha256:` preimage for every existing path re-hashed immediately before `git apply --check` then `git apply`.
- **Acceptance commands:** `/bin/sh -c <declared>` with `env_clear`, isolated HOME, PATH from signed runtime snapshot, 600s timeout. `run_workspace_command`: argv-only, 7 allowed executables, inline programs rejected.
- **Budgets** (`core/src/project.rs:201-225`): 48 soft / 100 hard turns, 400k soft / 1M hard tokens, 3600s active, 4 recoverable tool errors, 2 identical failures, 8-turn verification reserve, ≤2 protocol corrections per run. Soft exhaustion pauses resumably; hard exhaustion fails.
- **Context:** V2 runs receive one hashed JSON context envelope (`pharness.dev/agent-context-envelope/v1`); resume validates envelope hash, run ID, AgentContext, and prompt revision hashes against the saved inference binding.
- **Stage prompts:** onboarding-proposer, planner, builder, repair, test-diagnoser, verifier, each a `StagePromptRevision` with content hash bound at run start. Planner must return `readiness: ready | needs_decision` with blockers; `needs_decision` seals as `blocked`.

### 4.2 Agent runtime loop

`core/src/agent/runtime.rs::run_loop` (~1,230 lines, one function): budget checks → compaction → inject budget/ledger/reserve messages → `provider.complete_action` → `SafetyPolicy::evaluate_action_in_scope` → Allow / Ask (pause with resumable transcript) / Deny (fail) → execute tool → update completion state. Builder cannot submit until acceptance passed and git status/diff inspected after the last mutation.

### 4.3 SafetyPolicy

Modes `Default | TrustedWrites | SupervisedAutonomy | Plan | DenyAllWrites`. Reads, typed submissions, acceptance/workspace commands, git reads: always Allow. Writes: Plan/DenyAllWrites → Deny; TrustedWrites → Allow; Default/Supervised → Ask when configured. Shell: heuristic `classify_command` (prefix/substring); `EnvironmentSetup`, `Privileged`, `SecretAccessing` → Deny; `Network`, `Destructive`, `Unknown` → Ask. Cluster reads deny if any field looks secret-shaped. `PermissionGrant` converts Ask → Allow only for file writes under `TrustedWrites`, scoped by subject/environment/run scope/paths/risk.

### 4.4 Inference path

- **Gateway** (`model-gateway/src/main.rs`): `POST /v1/chat/completions` → 4 MiB cap → HMAC `ModelGrant` verify (run, stage execution, selection, target/policy hashes, `request_sequence`, `request_body_hash`, nonce, expiry) → body hash must match → nonce replay check (in-memory) → registry binding resolution (target `config_hash`, policy `policy_hash`) → protocol enforcement (model alias, exact `max_tokens`/`temperature`, `stream: true`, native tools non-empty, `parallel_tool_calls: false`) → backend translation (OpenRouter pinned provider, Fireworks reasoning fields, MiniMax folding) → retrying SSE forward with first-byte and idle timeouts, correlation-ID logging, sanitized error bodies.
- **Backends:** `Fireworks | Openrouter | LmStudio | LlamaCpp | OpenaiCompatible`. Registry ships Kimi/Qwen/DeepSeek/GLM/Nemotron/MiniMax on Fireworks, an OpenRouter GPT target, and a local llama.cpp GPT-OSS-120B target on a LAN host.
- **Provider trait:** `ModelProvider::complete_action(ModelRequest) -> ModelTurn` (single non-streaming method). Selection (gateway vs direct Fireworks) is hard-wired in `worker/src/main.rs:124-156`.
- **Network:** runner pods get `HTTPS_PROXY` to the coding egress proxy; NetworkPolicies select gateway-mode vs direct-Fireworks routes per pod label; gateway egress is limited to DNS, coding proxy, and private CIDRs derived from the registry.

### 4.5 Codex host (alternative backend)

`pharness-codex-host` enrolls with the API, long-polls leases, checks out exact source, and drives `codex app-server` over JSON-RPC with `approvalPolicy: never`, a stage permission profile, and an output schema; runs under rootless Podman (`--read-only --cap-drop=ALL --network=none` for Test) or in-cluster. Not routed through the gateway. Planning marks expansion deferred; code, Helm StatefulSet (disabled), systemd packaging, and a qualification runner remain.

## 5. Evaluation and qualification

`pharness-eval`:

- **Frozen coding suite** `coding-reliability-v2.1`: 3 stacks × 8 cases = 24 fixtures generated programmatically; suite hash pinned in a test. Hidden tests run after the agent finishes; `hidden_test_false_pass`, `protected_path_modified`, `git_metadata_mutated`, out-of-scope paths, `environment_rediscovery_loop` are violations.
- **Gates:** per attempt ≥21/24 first pass, ≥6/8 per stack; repair ≥23/24, ≥7/8 per stack, every result `correction_used`; planner-v2 ≥11/12; verifier-v2 zero false approvals, ≤1 false rejection; protocol 30/30.
- **Stage suites:** onboarding, planner, tester, test-diagnosis, verifier (v1/v2), fixture JSON with expected answers kept out of agent input.
- **Replay:** every suite has a `Replay` provider; full 48-result coding replay and all 8 stage suites run as ordinary `cargo test` (needs cargo/python3/node/git on PATH). **Nothing runs them in CI.**
- **In-cluster:** `execute-qualification` fetches context, validates runtime revision and `selection_id`, runs via the gateway client, posts outcome with qualification evidence. Hosted work requires `-v2` policies with passing qualifications bound to `state.build.api_revision`.

Current status per `planning/`: Planner passed 30/30 protocol and 24/24 qualification on `49ecdc5`; Test Diagnosis stopped at an intermittent gateway protocol timeout; Builder/Repair have passing rows from `48c77b7` but lack current exact-policy protocol receipts; `coding_reliability_v2.enabled=false`; no connected WorkItem authorized.

## 6. Deployment topology (Helm `deploy/helm/pharness`)

| Workload | Replicas | Notes |
| --- | --- | --- |
| `pharness-api` | 1, `Recreate` | SQLite PVC `local-path` RWO 2Gi; `PHARNESS_WORKER_MODE=kubernetes_job`; uid 65532, read-only root, drop ALL, seccomp |
| `pharness-ui` | 1 | nginx-unprivileged; injects operator bearer from Secret; no read-only root |
| `pharness-model-gateway` | 1 | credentials mounted as files; no SA token |
| `pharness-preparation-egress-proxy` / `pharness-coding-egress-proxy` | 1 each | worker binary in `egress_proxy` mode; CONNECT-only; host allowlists in-process |
| `pharness-codex-host-*` StatefulSets | disabled | 20Gi PVC per profile |
| Tekton fixtures `pharness-e2e-noop`, `pharness-e2e-build-output` | — | inert smoke pipelines |

Not present: Jobs/CronJobs (other than API-spawned), ServiceMonitor/PodMonitor/PrometheusRule, PDB, HPA, OTel config, metrics port.

**RBAC:** API SA can create/get/list/watch/delete `batch/jobs` and PVCs in-namespace, plus read one protected Deployment/pods in `apps-prod` and one Argo Application. Worker SA has a cluster-wide read-only ClusterRole (pods, logs, configmaps, apps, batch, Argo, Tekton). Tekton runner: create PipelineRuns in allowlisted namespaces. Argo runner (disabled): get/patch named Applications. Writer/observer SAs have no RoleBindings; authority is a mounted GitHub token.

**Network:** NetworkPolicies for API ingress, UI ingress, phase-labelled runner egress, gateway ingress/egress, proxy egress `0.0.0.0/0:443`. No namespace default-deny; API and UI have no Egress policies.

**Ingress:** console host behind nginx Basic Auth; `/api/internal/agent-hosts` on the same host without Basic Auth; `pharness-api.lucas.engineering` exposes the full API with bearer-only auth and no rate limiting (SECURITY_REVIEW H1, still open).

**Build & release of PHarness itself:** `scripts/pharness-build-incluster.sh` → Tekton `clone-build-push` on a dedicated amd64 BuildKit node, pinned to a verified SHA (`pharness-verify-build-revision.sh` checks clean tree, HEAD == origin/main == requested). Seven images, all base images digest-pinned, all non-root. `pharness-release-pin.sh` rewrites `values.yaml` digests in this repo (not a separate GitOps repo), lints/templates, rejects `:latest`, prints diff, does not commit. Chart version stays `0.1.0`. No SBOM, signing, provenance, `cargo audit`, or `cargo deny`; `kubectl` fetched without checksum.

**CI:** `.github/workflows/secret-scan.yml` only (Gitleaks, pinned, scans introduced commits).

## 7. Console (`ui/`)

React 19 / Vite 6 / vitest 4 / Playwright 1.62. Two generations behind a runtime flag: `RepoModeApp` (Lamina: Overview, Products, Repositories, WorkItems, Agents, Releases, Insights, Settings; hash routing) when `features.repo_mode_v1.ui_enabled`, else `LegacyApp` (Triage, Queue, Flow, ApprovalGates, ToolApprovals, WorkPlans, Status, Audit). Same-origin `fetch` with no browser token; nginx injects the bearer. 19 vitest files; 8 Playwright specs with 92 macOS-only (`-darwin`) PNG baselines, desktop + 390px mobile, axe WCAG 2A/2AA checks.

## 8. Security posture (from `SECURITY_REVIEW.md`, verified against code where cited)

Fixed: constant-time token compare; fail-closed non-loopback bind without tokens; worker checkout env/hook hardening. Open: historical operator token in reachable git history (rotation unverified); public bearer-only API ingress. Strong: egress proxy exact-host allowlists; HMAC grants with body hash + nonce; per-host/lease hashed credentials; secret-shaped path guards; env-cleared acceptance; preimage-bound patches; HTTPS-GitHub-only repo URL validation; all Services ClusterIP; tokens via `secretKeyRef`.

## 9. Documentation corpus

`planning/` is 922 tracked files (~25 MB), of which ~790 are evidence artifacts (JSON receipts, logs, screenshots). Structure: `programs/autonomous-sdlc/` (ASTRA-00 master + 12 milestones + 41 KB execution history), `design/` (living contracts), `evidence/` (dated proof), `implemented/capabilities/` (decision records per capability), `operations/runbooks/`, `archive/`. The corpus is internally consistent about status and explicitly separates implemented / deployed / qualified / accepted. The risk is volume: the master program document alone requires reading several hundred lines of nested status prose to answer "what works right now."

## 10. Notable structural debt

| Item | Evidence |
| --- | --- |
| God files | `dispatch.rs` 5.4k, `worker.rs` 4.5k, `worker/src/main.rs` 5.8k (16 modes by env string), `store/src/sqlite.rs` 9.9k, `cli/src/main.rs` 9.6k, `dto.rs` 3.1k, `reconcile.rs` 2.7k |
| `run_loop` monolith | ~1,230 lines; protocol-correction block repeated ≥9 times (`runtime.rs:455-1275`) |
| Triplicated callback plumbing | git/gitops context/outcome/observation handlers in `source/git_delivery.rs`, `repo_mode/source_delivery.rs`, `work_items/rollback.rs`; `internal.rs:57-152` string-prefix dispatch on `rollback_` |
| Two controllers | timer-driven hosted controller vs API-triggered `work_items` controller with different wait semantics |
| Legacy/V2 duplication | prompts, context assembly, agent profiles, UI generations, provider clients (`fireworks` vs `openai-compatible` ~183-line diff, legacy path dead for `ModelProvider` consumers) |
| Env reads outside config | `OperationalMode::from_env()` per request/tick; `PHARNESS_CODING_RELIABILITY_V2_ENABLED` etc. read in API not `pharness-config` |
| Hard-coded tenant | see FABLE_00 §"five things", item 5 |
| `unwrap()` on policy presence | `progression.rs:163`, `hosted_controller/source.rs:30`, `hosted_workflow/stages.rs:84-85,194,248` |
| Postgres compiled, unused | `Cargo.toml:37` |
