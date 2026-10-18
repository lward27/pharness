# FABLE 02: Autonomous SDLC Capability Matrix

Reviewed 2026-10-17 at `dd55eec`. Each capability is graded on **what the code in this tree does**, then cross-checked against the project's own status claims in `planning/`. Where the two disagree, the code wins and the disagreement is noted.

**Legend**

| Grade | Meaning |
| --- | --- |
| **Present** | Implemented end to end in code, with tests, reachable from the hosted path |
| **Present, unqualified** | Implemented, but the project's own acceptance gate has not been passed on the current runtime |
| **Partial** | Meaningful implementation exists but a required step is missing, stubbed, or not reachable from the hosted controller |
| **Missing** | Not implemented |
| **Deferred** | Explicitly out of scope by ASTRA decision; noted so the boundary is visible |

The reference model below is a generic one for an autonomous SDLC: a system that can take a request, deliver verified running software, and keep it healthy, with humans supervising exceptions rather than driving steps.

---

## A. Intake and intent

| # | Capability | Grade | Evidence | Gap |
| --- | --- | --- | --- | --- |
| A1 | Request capture as a durable, bounded intent | **Present** | `work_items` (0023), product-scoped creation `POST /api/products/:id/work-items` with preflight/readiness hash; `workflow_policy` snapshot (0052) binds repo, GitOps repo, targets, limits, rollback permission at creation | — |
| A2 | Product / Repository / Service model | **Present** | `organizations/products/repositories/services/repository_bindings/product_model_snapshots` (0040); registration validates HTTPS GitHub URL and resolves commit | Service bindings proposed by agent, applied behind `PHARNESS_LEGACY_ONBOARDING_PRODUCT_MODEL_APPLY_ENABLED` |
| A3 | Repository onboarding (discovery → proposal → PR → merge observation → readiness) | **Present** | `repository_discoveries/onboardings/proposals/contract_versions/readiness_assessments` (0041); `repository_discovery`, `onboarding_patch`, `onboarding_contract_validate`, `repository_readiness` worker modes; `.pharness/repository.yaml` contract | Onboarding PR merge is manual by design |
| A4 | Readiness as derived, evidenced state | **Present** | `products/readiness.rs`; `discover` sealed from readiness at creation (`repo_mode/creation.rs:296-330`) | — |
| A5 | Intake from sources other than a human request (incidents, alerts, backlog systems, PR comments) | **Deferred** | `product-vision-and-boundaries.md`: "autonomous incident campaigns are deferred"; `mcp-boundary.md`: Jira/Slack via MCP is future | No inbound integration surface at all |
| A6 | Prioritization / scheduling across WorkItems | **Partial** | `claim_due_workflow` picks one due row per 15s tick; `coding` lock allows one coding run globally | No priority, no fairness, no queue visibility beyond list ordering |

## B. Planning

| # | Capability | Grade | Evidence | Gap |
| --- | --- | --- | --- | --- |
| B1 | Planner agent producing a structured WorkPlan | **Present** | `repo-planner` profile, `planner_submission.rs`, `submit_work_plan` tool, `work_plans` (0014) | — |
| B2 | Decision boundary (ready vs. needs human decision) | **Present** | `readiness: {status: ready\|needs_decision, blockers}` required; `needs_decision` seals `blocked`; blockers survive normalization (`stage-outcomes-and-evidence-handoffs.md`, PR #395) | — |
| B3 | Plan approval automated under recorded authority | **Present** | `approve_work_plan` automatic when `HostedAutomaticAction::Plan` in policy; `hosted_controller/approval.rs` validates sealed outcome | — |
| B4 | Replan / annotation loop | **Present** | `operator_annotations`, new StageExecution per replan, typed WorkItem relationships (`supersedes`, `follows_up`) | — |
| B5 | Multi-repo / dependency-ordered planning (DeliveryPlan) | **Deferred** | `product-model.md` "Future DeliveryPlan"; one mutable repo per WorkItem | — |

## C. Implementation

| # | Capability | Grade | Evidence | Gap |
| --- | --- | --- | --- | --- |
| C1 | Coding agent loop with tools, budgets, pause/resume | **Present** | `core/agent/runtime.rs`, `runhost/lib.rs`; 48/100 turns, 400k/1M tokens, 3600s, resumable on soft budget or approval | — |
| C2 | Isolated workspace per run | **Present** | per-run PVC, shallow clone to exact commit, `env_clear`, node-pinned | Node pinning limits scheduling |
| C3 | Write scoping to contract-declared paths | **Present** | `writable_paths` globs, secret-shaped path rejection, preimage-hashed `apply_patch` | — |
| C4 | Dependency preparation reproducibly | **Present** | `environment_prepare` mode, signed `EnvironmentSnapshot`, preparation egress proxy allowlist | — |
| C5 | Multi-language support | **Partial** | Python 3.11 and Node 24 runner images; Rust toolchain only in eval image | Any stack beyond Python/Node needs a new runner image and profile |
| C6 | Qualified coding reliability on the deployed runtime | **Present, unqualified** | Frozen 24-task suite; Builder/Repair passed on `48c77b7`; Planner 24/24 on `49ecdc5`; Test Diagnosis stopped at protocol timeout; `coding_reliability_v2.enabled=false` | M04E/M04F gates open; no connected WorkItem has run |
| C7 | Correction / repair loop | **Present** | `repo-repair` profile, one bounded correction, `test-diagnosis` classification, ≤2 protocol corrections | Connected handoff proof (M04E) not yet demonstrated |
| C8 | Alternative execution backends | **Deferred** | `pharness-codex-host` exists and is packaged; expansion deferred | — |

## D. Test

| # | Capability | Grade | Evidence | Gap |
| --- | --- | --- | --- | --- |
| D1 | Deterministic acceptance execution without model access | **Present** | `execute_deterministic_test`, `/bin/sh -c` declared commands, `env_clear`, 600s cap | — |
| D2 | Hidden-test false-pass detection | **Present (eval only)** | `run_hidden_test` in `coding_v2.rs` | Only in qualification; production WorkItems rely on repo-declared acceptance |
| D3 | Test generation / coverage reasoning | **Missing** | No profile or tool for generating or assessing tests | Agents may add tests as part of a plan, but nothing requires or measures it |
| D4 | Flaky-test handling | **Missing** | `identical_failures: 2` budget only | No retry-with-classification or quarantine |

## E. Verification (code)

| # | Capability | Grade | Evidence | Gap |
| --- | --- | --- | --- | --- |
| E1 | Independent verifier agent, blind to expected verdicts | **Present** | `repo-verifier` profile, verification reserve turns, `submit_verification`; verifier-v2 gate zero false approvals | Qualification pending on current runtime |
| E2 | Controller-sealed outcomes from typed validators | **Present** | `evidence_validations`, `stage_outcomes`, contradictions/risks retained | — |
| E3 | Static analysis / lint in the loop | **Partial** | Only if the repo's declared acceptance commands include it | No platform-level SAST or lint stage |
| E4 | Dependency vulnerability audit of the *target* change | **Missing** | No `cargo audit`/`npm audit`/`pip-audit` capability or stage | — |
| E5 | Secret scanning of the agent's diff | **Partial** | Secret-shaped *path* rejection; no content scan of produced diff | Gitleaks runs on PHarness's own repo, not on target repos |
| E6 | Human code review as an optional gate | **Partial** | `approve_change_set` can be manual or automatic; no PR review request integration | — |

## F. Source delivery

| # | Capability | Grade | Evidence | Gap |
| --- | --- | --- | --- | --- |
| F1 | Branch + PR creation with scoped writer identity | **Present** | `git_delivery` mode, askpass 0700, `PHARNESS_GIT_WRITER_TOKEN`, `GIT_CONFIG_GLOBAL=/dev/null`, hooks disabled | — |
| F2 | Required-check observation bound to exact head SHA | **Present** | `provider_check_set_observations`, head drift detection, `authoritative_rules_succeeded` | Check name/App ID hard-coded (`source_merge.rs:168`) |
| F3 | Guarded single-attempt automatic merge | **Present** | `HostedSourceMergeAuthority`, merge-attempt admission, `PUT /pulls/{n}/merge` never retried, `merge_parent_shas == [base, head]` proof | — |
| F4 | Branch protection preserved / verified | **Present** | `source_merge.rs:173-181` validates protection before merge; M07 protection evidence | — |
| F5 | Non-GitHub providers | **Missing** | `github_api_url == https://api.github.com` enforced | — |

## G. Build

| # | Capability | Grade | Evidence | Gap |
| --- | --- | --- | --- | --- |
| G1 | Immutable build from exact merged source | **Present** | `build_verified_source` → Tekton PipelineRun via executor Job; `SOURCE_COMMIT`/`IMAGE_URL`/`IMAGE_DIGEST` results validated against merge commit (`staging/evidence.rs:108-142`) | — |
| G2 | Independent registry verification of digest | **Present** | `registry_inspect_image` (manifest HEAD/GET, digest check), `registry_evidence` | — |
| G3 | SBOM / signing / provenance (SLSA) | **Missing** | Evidence JSON records `sbom_verified: false`; no cosign/syft anywhere | — |
| G4 | Build-system portability | **Deferred** | Tekton + BuildKit only; `pipeline/lucas.rs` constants | — |
| G5 | Build caching / performance | **Partial** | PR #394 caching evidence (34 min build, 20 min push) | Not an autonomy blocker |

## H. Staging deployment

| # | Capability | Grade | Evidence | Gap |
| --- | --- | --- | --- | --- |
| H1 | Automatic staging GitOps update | **Present** | `stage_verified_build` → `hosted_staging_gitops` GraphQL `createCommitOnBranch` on GitOps `main`; one-file, one-parent, blob-changed validation | — |
| H2 | Observe Argo reconciliation to the committed revision | **Partial** | `ArgoGetApp` reader exists; `argo_sync` executor exists (legacy path) | **Not invoked by hosted controller**; `deployment_verification: "pending"` never resolved |
| H3 | Verify running imageID equals built digest | **Partial** | `verify_release` does this for the protected production target (`releases.rs:620-652`, hard-coded label) | Not invoked for staging; not generalized |
| H4 | Functional probes against staging | **Partial** | Contract carries `service_port`/`health_path` (`delivery.rs:52-56`) | No code performs the probe in the hosted path |
| H5 | Staging environment isolation | **Present (infra)** | M02 evidence: 13/13 isolation checks | Platform fact, not PHarness code |

**This is where the autonomous chain stops today.** `hosted_controller/progression.rs` has no candidate after `staging::candidate`. The staging operation remains `running` until 240 unchanged checks (60 min) and then `wait_expired`.

## I. Runtime verification

| # | Capability | Grade | Evidence | Gap |
| --- | --- | --- | --- | --- |
| I1 | Metrics read (Prometheus/Mimir) | **Present (reader)** | `prometheus_query`, `prometheus_inventory`; release observability evidence | Not wired to hosted stages |
| I2 | Logs read (Loki) | **Present (reader)** | `loki_log_summary` with redaction and window clamps | Same |
| I3 | Traces read (Tempo) | **Present (reader)** | `cluster/tempo.rs`, bounded TraceQL, one service/namespace; endpoint `""` in values | Hard-bound to `yfinance-wrapper`; endpoint unconfigured |
| I4 | Decision model: pass / fail / inconclusive with windows | **Partial** | Design specifies 5 min staging / 10 min production, missing data inconclusive; M08 draft #363 has 851 checks | **Draft is unmerged**; nothing in tree executes a verification window |
| I5 | Verification bound to exact artifact + deployment identity | **Partial** | `verify_release` checks Argo Synced/Healthy, rollout, imageID | Operator-triggered only (`POST /api/releases/:id/verify`) |
| I6 | SLO / latency recording | **Missing** | Design says "record latency without inventing an SLO"; no implementation | — |

## J. Production promotion

| # | Capability | Grade | Evidence | Gap |
| --- | --- | --- | --- | --- |
| J1 | Approval material binds digest + GitOps diff + staging evidence + target + healthy baseline | **Partial** | Production grant scope requires `source_merge_shas`, `gitops_merge_shas`, `image_digests` (`deployment/intents.rs:318-389`); M09 draft #366 (216 checks) | Draft unmerged; hosted controller has no production candidate |
| J2 | Approval-before-GitOps-merge ordering (Argo auto-sync) | **Present (design + legacy path)** | `approval_gates` `production_deployment`; `deployment/execution.rs` requires grant before Argo executor | Not reachable from hosted work |
| J3 | Authenticated human approver identity | **Missing** | `approvals.rs` never reads `OperatorIdentity`; `decided_by` from request body (`:529,561,998,1015`); audit actor defaults to `"api"` | **Critical for the central safety claim** |
| J4 | Re-validate bound state immediately before merge | **Partial** | Pattern exists for source merge (`validate_binding`); M09 specifies it for production | Not implemented for production |
| J5 | Same digest staging → production, no rebuild | **Present (schema)** | 0054: one PipelineIntent, two DeploymentIntents | Execution not reachable |
| J6 | yfinance Argo app auto-sync alignment | **Missing (infra)** | M09 baseline correction notes yfinance is manual-sync | GitOps repo change |

## K. Rollback and recovery

| # | Capability | Grade | Evidence | Gap |
| --- | --- | --- | --- | --- |
| K1 | One bounded rollback to recorded healthy baseline | **Partial** | `work_items/rollback.rs` (2.2k LOC): prepare/approve/preflight/execute/observe, GitOps re-pin + Argo sync, fresh capability baselines | Legacy path only; not reachable from hosted controller; hard-coded protected target |
| K2 | Rollback leaves WorkItem failed, recovery recorded separately | **Present (design)** | `product-vision-and-boundaries.md`, `incidents.md` | Not exercised for hosted |
| K3 | Stop on incompatible migration / destructive change | **Partial** | `prune=false`, `force=false` enforced in contracts; no migration-compat analysis | — |
| K4 | Controller restart recovery | **Present** | Persisted claims/operations, exact Job manifest re-observation, `PlannerStartupIdentity` (commit `af6db7d`) | M06 live restart proof open |
| K5 | Duplicate-callback / lost-ack safety | **Present** | Single-use admissions, never-retry mutations, observer reconciliation | — |

## L. Observe and closed loop

| # | Capability | Grade | Evidence | Gap |
| --- | --- | --- | --- | --- |
| L1 | Durable observations from cluster/LGTM reads | **Present** | `observations` (0008), runless control-plane observations | — |
| L2 | Incident candidates from risky evidence | **Present** | `incidents` (0010); deterministic by release+observation; from Tekton analysis, release evidence, delivery failure | Read-only candidates |
| L3 | Draft remediation plans with gates | **Present** | `remediation_plans` (0011); `automatic_retry=false`, `mutation_performed=false` markers | Never dispatched |
| L4 | Incident → new WorkItem (closed loop) | **Deferred** | Explicit ASTRA boundary: "LGTM verifies and recovers the same work; it does not initiate incident campaigns" | The "observe" stage has no hosted implementation at all |
| L5 | Post-deploy observation window as a lifecycle stage | **Missing** | `observe` stage exists as a label; never sealed for hosted items | — |
| L6 | Feedback into planning (learned constraints, flakiness, hot files) | **Missing** | No memory beyond per-WorkItem context packs | RAG config section exists, disabled |

## M. Governance, policy, audit

| # | Capability | Grade | Evidence | Gap |
| --- | --- | --- | --- | --- |
| M1 | Capability taxonomy + SafetyPolicy modes | **Present** | `CapabilityKind`, 5 policy modes, risk classes | Shell classifier heuristic; `TrustedWrites` ignores `require_approval_for_writes` |
| M2 | Trusted envelopes (PermissionGrant) scoped by resource | **Present** | `permission_grants`, WorkPlan/ChangeSet scoping, stale on material change | File writes only by design |
| M3 | Approval gates per effect class | **Present** | 7 gate kinds, lifecycle readiness checks | Identity gap (J3) |
| M4 | Immutable audit trail with origin | **Present** | `audit_events` (0006, 0036 origins), operator origins (0034) | Actor can be self-asserted |
| M5 | Budgets and circuit breakers | **Present** | Run budgets, 60-min operation expiry, no retry inflation | No cost budget per WorkItem/Product; no cross-run spend tracking |
| M6 | Operational modes (normal / draining / read-only) | **Present** | `PHARNESS_OPERATIONAL_MODE` | Read from env per request |
| M7 | Data retention, archive, holds | **Present** | 0049, retention scheduler, `pharness-admin data archive` | Restore undocumented |
| M8 | Secrets never in evidence/logs | **Present** | Redaction, secret-shaped guards, sanitized gateway errors | — |

## N. Model and agent management

| # | Capability | Grade | Evidence | Gap |
| --- | --- | --- | --- | --- |
| N1 | Inference gateway with grant-bound requests | **Present** | HMAC grants, body hash, nonce, exact policy enforcement | Nonce cache in-memory (single replica) |
| N2 | Content-addressed targets and stage policies | **Present** | `InferenceTargetRevision.config_hash`, `StageInferencePolicyRevision.policy_hash` | — |
| N3 | Multi-provider incl. local models | **Present** | Fireworks, OpenRouter, LM Studio, llama.cpp, OpenAI-compatible | Selection hard-wired in worker `main` |
| N4 | Qualification harness with frozen suites and hidden tests | **Present** | `pharness-eval`; thresholds in code; suite hash pinned | Not in CI |
| N5 | Qualification bound to exact deployed runtime | **Present** | `api_revision` binding; exact-policy protocol receipts | — |
| N6 | Agent profiles versioned (prompt + tools + model) | **Present** | compiled profiles, `StagePromptRevision` hashes, tool schema hash | V1/V2 duplication |
| N7 | Model comparison / A-B on identical inputs | **Present** | M04D diagnostics; `compare --kind regression\|policy\|transport` | — |
| N8 | Cost/usage accounting | **Partial** | Token/cost per eval result; run usage recorded | No roll-up per WorkItem/Product |

## O. Platform self-operation

| # | Capability | Grade | Evidence | Gap |
| --- | --- | --- | --- | --- |
| O1 | CI for PHarness itself (fmt/clippy/test/UI/e2e/guardrails/replay) | **Missing** | Only `secret-scan.yml` | **The platform does not meet its own merge standard** |
| O2 | Supply-chain controls for PHarness images | **Partial** | Digest-pinned bases, non-root, revision labels, pinned Codex | No SBOM/signing/provenance/audit |
| O3 | High availability | **Missing** | 1 replica, `Recreate`, SQLite RWO `local-path`, node-pinned | Any node loss is an outage |
| O4 | Backup / restore | **Partial** | Manual archive Job; no CronJob, no restore runbook | — |
| O5 | Metrics / alerting / SLOs for PHarness | **Missing** | No `/metrics`, no ServiceMonitor, no PrometheusRule | A stalled controller is silent |
| O6 | Structured logging / tracing | **Partial** | `tracing` compact fmt; gateway correlation IDs | No OTel export |
| O7 | Release of PHarness itself through its own pipeline | **Partial** | In-cluster Tekton build + release-pin script; verification scripts | Pin written to app repo; chart version static; manual |
| O8 | Config portability | **Missing** | Tenant constants in `values.yaml` and library code | — |
| O9 | Operator documentation | **Present** | `planning/operations/`, runbooks, checkpoint | No backup/restore/DR runbook; untracked `tutorial-site/` |

## P. Human interface

| # | Capability | Grade | Evidence | Gap |
| --- | --- | --- | --- | --- |
| P1 | Operator console with decision-first WorkItem view | **Present** | Lamina console, 8 sections, StageOutcome presentation rules in `ui/AGENTS.md` | M10 delivery-dependent states open; two UI generations |
| P2 | CLI | **Present** | `pharness` with ~25 command groups | 9.6k-line single file |
| P3 | Notifications (Slack/email/pager) for approvals and failures | **Missing** | No outbound notification code | An operator must poll the console |
| P4 | Approval ergonomics (batch, envelopes, fatigue control) | **Present** | `batch_decide_approval_gates`, trusted envelopes, `trusted-autonomy.md` decisions | — |

---

## Summary by phase of the canonical lifecycle

```
discover ──▶ plan ──▶ implement ──▶ test ──▶ verify ──▶ source_delivery ──▶ release ──▶ observe
Present     Present   Present*     Present   Present    Present            ┌──────────┐  Missing
                      (*unqualified)                                       │ build  ✓ │
                                                                           │ staging ◐│ ← stops at GitOps commit
                                                                           │ prod   ✗ │
                                                                           └──────────┘
```

**Pieces present and connected:** 6 of 8 stages for hosted work, with the first real autonomous WorkItem still blocked on coding qualification (`coding_reliability_v2.enabled=false`).
**Pieces present but not connected:** Argo observation, imageID verification, LGTM readers, Tempo reader, production grants, rollback intent, incidents/remediation.
**Pieces missing:** staging/production verification windows in the controller, release sealing, observe stage, authenticated approver identity, notifications, CI for the platform, HA/backup/alerting, SBOM/signing, cross-WorkItem cost budgets.
**Pieces deliberately deferred:** incident-initiated work, multi-repo plans, generic adapters, non-GitHub providers, additional coding backends, multi-tenancy.
