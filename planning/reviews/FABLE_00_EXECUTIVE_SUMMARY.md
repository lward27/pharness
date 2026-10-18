# FABLE Architecture Review: PHarness — Executive Summary

**Reviewed:** 2026-10-17
**Repository:** `lward27/pharness`, working tree at `dd55eec` (branch `codex/incluster-bundle-packaging-20260924`; `main` at `3d4ec02`)
**Method:** Static review of all 12 Rust crates (~195k LOC incl. tests), 55 SQL migrations, Helm chart, Dockerfiles, scripts, CI, React console, and the `planning/` corpus (922 tracked files). Workspace compiles (`cargo check --workspace --all-targets` passed). No cluster access was used; no live state was inspected.
**Question asked:** Are all the right pieces present for a fully autonomous SDLC?

Companion documents:

| File | Contents |
| --- | --- |
| [FABLE_01_SYSTEM_ARCHITECTURE.md](FABLE_01_SYSTEM_ARCHITECTURE.md) | What actually exists: components, control loop, data model, deployment topology, security posture |
| [FABLE_02_AUTONOMOUS_SDLC_CAPABILITY_MATRIX.md](FABLE_02_AUTONOMOUS_SDLC_CAPABILITY_MATRIX.md) | Capability-by-capability scorecard against a reference model for autonomous SDLC |
| [FABLE_03_FINDINGS_AND_RECOMMENDATIONS.md](FABLE_03_FINDINGS_AND_RECOMMENDATIONS.md) | Prioritized findings, risks, and a recommended sequence |

---

## Short answer

**Most of the right pieces are present as code. The loop is not closed.** PHarness has an unusually complete *skeleton* for an autonomous SDLC control plane: durable WorkItem lifecycle, controller-sealed stage outcomes, a timer-driven reconciler with leases and idempotent operations, isolated worker Jobs for every external effect, HMAC-bound model inference through a gateway, a frozen qualification harness, typed approvals/grants, and a GitOps-only delivery boundary. The design discipline (single writer, evidence over claims, approval-before-production-merge, one bounded rollback) is sound and consistently applied.

But as of this revision the autonomous chain **terminates at "staging GitOps commit written"**. Nothing in the code completes staging deployment verification, runtime verification, production promotion, release sealing, or the observe stage for hosted work. The repo's own documentation says this truthfully (ASTRA M04 open, M08/M09 drafts unmerged, M11/M12 planned). The gap between "pieces exist" and "autonomous SDLC exists" is therefore real and well understood by the project, and it is concentrated in a few specific places rather than spread everywhere.

## Scorecard

Grades reflect code in this tree, not planning intent. See the matrix for evidence.

| Area | Grade | One-line reason |
| --- | --- | --- |
| Intent capture & onboarding | **Present** | Product/Repository/WorkItem model, deterministic discovery, contract-based readiness, onboarding PR flow |
| Planning & readiness decisions | **Present** | Planner submission contract with `ready`/`needs_decision`, blockers survive sealing |
| Implementation (coding agent) | **Present, unqualified** | Full tool loop, budgets, preimage-hashed patches, profile-restricted tools; frozen 24-task gate not yet passed on current runtime |
| Deterministic test | **Present** | Model-free acceptance execution, env-cleared, 600s cap |
| Verification (code) | **Partial** | Independent verifier agent exists; no SAST, dependency audit, or secret scan inside the coding loop |
| Source delivery | **Present** | PR open → required-check observation → guarded single-attempt merge → parent-SHA proof |
| Build | **Present** | Tekton via executor Job, digest/source binding verified; no SBOM/signing/provenance |
| Staging deploy | **Partial (stops here)** | GitOps digest commit only; `deployment_verification`/`runtime_verification` set to `pending` and never resolved |
| Runtime verification | **Partial** | Argo/rollout/imageID/Prometheus/Loki/Tempo readers exist as operator-triggered `verify_release`; not wired into the hosted controller |
| Production promotion | **Missing (planned M09)** | Approval binding exists in draft PR #366; controller has no production candidate; approval identity is client-asserted |
| Rollback & recovery | **Partial** | Rollback intent with GitOps re-pin + Argo sync exists for the legacy path; not reachable from hosted work |
| Observe / closed loop | **Missing by design** | Observations → Incidents → draft RemediationPlans exist; incident-initiated WorkItems explicitly deferred |
| Governance (policy, audit, grants) | **Present** | SafetyPolicy modes, PermissionGrants, approval gates, audit events, budgets |
| Model/agent management | **Present** | Gateway, content-hashed targets/policies, grants, qualification evidence, eval harness |
| Platform self-operation (CI, HA, backup, metrics) | **Weak** | No CI beyond secret scan; single-replica SQLite; manual backup; no metrics export for PHarness itself |
| Portability / multi-tenancy | **Missing by design** | Two Finance repos, one GitOps repo, one cluster hard-coded in library code |

## The five things that matter most

1. **The hosted lifecycle ends at the staging commit.** `hosted_controller/progression.rs` has candidates for plan → implement → test → verify → source delivery → build → staging commit, and nothing after. `staging.rs:134` and `staging/callbacks/outcomes.rs:145,199` set verification fields to `"pending"` and no code path ever updates them; the operation waits 60 minutes and expires. Release/observe stage outcomes are never written for hosted items. **This is the single largest gap** and it is where M08/M09 drafts (#363, #366) need to land.

2. **PHarness does not apply its own standards to itself.** The product premise is "no merge without tests, no deploy without verification," but `.github/workflows/` contains only a Gitleaks scan. `cargo test`, `clippy`, vitest, Playwright, architecture guardrails, Helm lint, deterministic eval replay, and the `scripts/tests/*` suite all depend on a human or agent remembering to run them. Playwright baselines are macOS-only (`*-darwin.png`), so they cannot run on a Linux runner today.

3. **Production approval has no authenticated decision model.** `approvals.rs` never reads `OperatorIdentity`; `decided_by` comes from the request body (`approvals.rs:529,561,998,1015`). With bearer tokens configured the token *name* is available but unused. For a system whose central safety claim is "human production approval binds digest + diff + evidence," the approver identity must come from the authenticated principal, not the client.

4. **Operational resilience is below what unattended operation requires.** Single API replica with `Recreate` strategy, SQLite on `local-path` RWO storage pinned to one node, worker PVCs node-pinned, no PodDisruptionBudget, manual backup with no restore runbook, no `/metrics`, no alert rules, no SLOs. M12 demands 24-hour unattended evidence; the platform cannot currently alert anyone if it stalls.

5. **The system is a bespoke installation, not a product.** Finance repo URLs, image names, chart paths, Argo application names, GitHub App ID, pod label selectors, namespaces, and the GitOps lock key are compile-time constants (`hosted_workflow/delivery.rs`, `system.rs:32-43`, `source_merge.rs:168`, `releases.rs:627`, `progression.rs:129`, `staging_gitops/github.rs:10`, `tempo.rs:17`). This is an explicit ASTRA decision ("do not design a universal adapter framework prematurely"), and it is the right call for proving the loop once. It means "autonomous SDLC for `lucas_engineering` Finance apps," not "autonomous SDLC."

## What is genuinely strong

- **Evidence discipline.** Agent output is a claim; the controller seals outcomes from typed validators; contradictions and blockers survive normalization; `stale` and `superseded` are relationships, not overwrites. This is rarer than it should be in agentic systems.
- **Effect isolation.** Every external mutation runs in a short-lived Job with its own ServiceAccount and token identity, env-cleared, through phase-specific egress proxies, and follows a fetch-context → single-use admission → one mutation (never retried) → observe → outcome protocol. Lost acknowledgements lead to observation, not re-mutation.
- **Inference governance.** Content-hashed target/policy revisions, per-request HMAC grants bound to a body hash and nonce, exact-policy protocol checks, and qualification evidence bound to the exact compiled API revision.
- **Honesty of the documentation.** The README, program ledger, and operator checkpoint consistently refuse to conflate "implemented," "deployed," "qualified," and "accepted." That is the correct posture and should be preserved.

## Bottom line

If the question is "could this become a fully autonomous SDLC without an architectural rewrite?", the answer is **yes**: the missing pieces (staging verification integration, production approval execution, release sealing, hosted rollback reachability, incident → WorkItem) all fit the existing operation/candidate pattern and the existing typed readers. If the question is "is it one today?", the answer is **no**, and the most valuable next steps are: close the staging→production→observe loop in the controller, put the control plane under its own CI, bind approvals to authenticated identity, and raise platform resilience to the level the 24-hour soak test requires. Details and sequencing are in `FABLE_03`.
