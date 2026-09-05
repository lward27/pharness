# PHarness Repo Mode V1 Product and Control-Plane Milestone

> Current status (2026-09-04): **Historical source-only implementation**.
> Replacement/authority: [ASTRA](../programs/autonomous-sdlc/ASTRA-05-UNIFIED-SDLC-CONTRACT.md).
> Preserve its implemented behavior for legacy work. New hosted convergence is governed by ASTRA M05–M10.

## Summary

Implement the approved Repo Mode V1 contract against baseline `d4502359cc63b2d64a43ac8ca62627e633ac8408`, re-characterizing `main` before execution. The milestone ends when PHarness can register and onboard a GitHub repository, run a fixed Planner → Builder → Tester → Verifier chain, deliver one reviewed source PR, observe its manual merge and provider checks, and close the WorkItem with controller-sealed evidence.

This is the Product/control-plane milestone only. It exposes the durable resources and read models required by the separate Screen Plan, but does not redesign navigation, layouts, or visual interactions.

### Current-state map

| Classification | Current PHarness behavior |
| --- | --- |
| Preserve as authoritative | WorkItems, WorkPlans, ChangeSets, Runs, Sessions, events, artifacts, observations, audits, approval gates, permission grants, state-hashed actions, resumable budgets, durable workspaces/PVCs, EnvironmentProfiles, environment preparation, typed acceptance execution, Git writer/observer Jobs, immutable release workflow |
| Adapt | `.pharness/project.yaml`, WorkItem creation and reconciliation, WorkItem-scoped workspaces/preparations, one universal agent prompt/tool set, ChangeSet-specific Git delivery, Git observer behavior, operator summaries |
| Add | Bootstrap Organization, Products, Repositories, Services, versioned RepositoryBindings, Product-model snapshots, deterministic discovery, onboarding lifecycle, derived readiness, AgentProfiles, StageExecutions, StageOutcomes, evidence validation, context packs, annotations, chain authorization, generic source delivery, required provider-check observations |
| Resolve explicitly | Repo Mode must not enter Pipeline/GitOps/Release delivery; `failed` source delivery after merge is terminal; stage success cannot be self-declared by an agent; new hashed records require deterministic canonical serialization |

Record the approved plan under `planning/active/` before implementation. Preserve all existing untracked files and unrelated work.

## Implementation Changes

### 1. Compatibility foundation and product registry

- Add `features.repoModeV1.enabled`, disabled until rollout, plus a single configured bootstrap Organization. V1 has no organization CRUD, memberships, billing, or multi-tenant authorization.
- Introduce prefixed UUIDv7 identifiers for new resources. Existing timestamp-style identifiers and historical records remain unchanged.
- Add persisted Products, global Repositories, optional Services, RepositoryBindings and immutable binding revisions. A Repository may bind to several Products; Services belong to one Product.
- Every accepted Product, Service, or binding change creates an immutable `ProductModelSnapshot` containing normalized Product structure and a canonical SHA-256 content hash. WorkItems pin its exact ID and hash.
- Repository registration accepts a GitHub HTTPS URL and full 40-character commit SHA. The server canonicalizes the URL, rejects credentials, SSH URLs, query strings and fragments, resolves the provider default branch, verifies the SHA exactly, and creates:
  - The Repository, or reuses its global provider identity.
  - A reviewed whole-repository binding with no manufactured Service.
  - A Product-model snapshot.
  - An initial onboarding record when the exact revision is not already canonically ready.
- Add a versioned canonical JSON serializer for new hashed resources. Do not change existing `material_hash` behavior retroactively.
- Keep legacy WorkItems in `mode=null`; route them through the existing full-SDLC reconciler unchanged. New Product-scoped submissions use `mode=repo`.
- Add nullable WorkItem fields for Product, mutable Repository, Product-model snapshot, contract version, selected acceptance names, pinned context repositories, current stage execution, state version, `closed_at`, and closure reason.
- Add `waiting_external` as a nonterminal WorkItem status. `completed`, terminal `failed`, `cancelled`, and successful source merge set `closed_at`.
- Add compatibility tests from a migration-0039 database before adding new migrations. Existing APIs and DTO fields remain readable.

### 2. Canonical Repository contract and repository-owned configuration

- Rename the core concept to `RepositoryContract` while retaining deprecated Rust aliases and existing legacy response fields where required.
- Make `.pharness/repository.yaml` canonical while retaining `.pharness/project.yaml` as a read-only compatibility alias:
  - Canonical only: active.
  - Alias only: readable to legacy execution but blocked for new Repo Mode readiness.
  - Both byte-identical: canonical is active and a deprecation warning is recorded.
  - Both different: readiness is blocked.
  - New source changes never create or amend the alias.
- Preserve the existing strict `pharness.dev/v1alpha1` serialized shape and 32 KiB limit.
- Validate immutable dependency-lock hashes, allowed profile, named acceptance commands, roots, writable globs, traversal and symlink escape, secret-shaped paths, network denial, and preparation-only package installation.
- Onboarding may propose a contract when the immutable dependency lock is missing, but source delivery remains blocked with `immutable_dependency_lock_missing`. V1 does not generate or amend dependency locks; the Repository owner must commit one and restart onboarding from the new SHA.
- Permit an onboarding source diff to modify only:
  - `.pharness/repository.yaml`
  - `.pharness/instructions.md`
  - Removal of `.pharness/project.yaml`
- Instructions are bounded to 32 KiB and cannot override executable contract fields.

### 3. Isolated discovery, onboarding and readiness

- Generalize `workspaces` and `environment_preparations` to subject-scoped resources supporting WorkItems, Repository onboardings and Repository readiness assessments. Migrate existing rows as `subject_kind=work_item`; preserve legacy WorkItem queries and DTOs.
- Add an isolated source-reader capability separate from source writer and observer:
  - Public repositories may use anonymous access.
  - Private access uses a reader-only credential mounted only into discovery/preparation Jobs.
  - Credentials never enter the API environment, model context, durable snapshot or logs.
  - Readiness reports sanitized state only.
- Implement deterministic discovery as a worker execution with no model, writer, GitOps, Tekton or Argo credentials:
  - Fetch only the exact commit into a detached checkout and reject SHA mismatch.
  - Do not initialize submodules or follow symlinks.
  - Enforce a 600-second deadline, 2 GiB workspace limit, 20,000-entry limit, 32 MiB inspected-text limit and 256 KiB per inspected text file.
  - Emit sorted `pharness.dev/repository-discovery/v1alpha1` evidence with repository identity, resolved SHA, inventory, symlinks, submodules, contract files, language/build indicators, dependency and lock candidates, command candidates with source locations, roots, automation references, conflicts, limits and content hash.
- Add the server-owned `repository-onboarding-proposer` AgentProfile. It receives the discovery record and bounded read-only repository access, then submits `pharness.dev/repository-onboarding-proposal/v1alpha1` through a typed tool.
- The proposal contains the exact discovery ID/hash, candidate contract, bounded instructions, optional Service/binding proposals, assumptions, conflicts, blockers and readiness forecast. The controller validates it; model claims never activate configuration.
- Permit operator edits as versioned proposal revisions before approval. Approval binds the exact proposal revision and may create reviewed Service/binding revisions and a new Product-model snapshot.
- Generalize Git delivery into `SourceDeliveryIntent`, bound to either an approved WorkItem ChangeSet or approved onboarding proposal. It owns the exact repository, base ref/SHA, branch, patch artifact/hash, actor, gates and execution identities.
- Adapt existing ChangeSet Git routes as compatibility adapters over SourceDeliveryIntent. Writer and observer Jobs accept only a server-issued intent ID and never client-supplied arbitrary targets.
- After manual onboarding merge, load the canonical contract from the exact merge revision, validate it, persist an immutable `RepositoryContractVersion`, and derive readiness.
- Represent readiness as immutable assessments keyed by repository SHA, contract version/hash, lock hash, EnvironmentProfile revision/digest, validation-policy version and capability evidence:
  - Contract readiness verifies merged provenance and contract inputs.
  - Coding readiness additionally proves exact checkout, preparation, executables, workspace construction and typed acceptance execution.
  - A nonzero baseline acceptance result is recorded as a warning unless the command cannot execute structurally; it does not prevent PHarness from fixing an already-failing repository.
  - Reader, writer and observer capability states remain separate from contract readiness, trust policy and authorization.
- Readiness invalidates when any keyed input changes. A WorkItem preflight is read-only and requires a current matching assessment; readiness refresh is an explicit idempotent action.

### 4. Stage execution, evidence and handoff contracts

- Add append-only `StageExecution` and immutable `StageOutcome` persistence, plus a controller-maintained effective-outcome pointer per WorkItem and stage.
- Use SQL triggers to reject StageOutcome update or deletion. Effective-pointer changes produce audit events and preserve supersession relationships.
- Support `discover`, `plan`, `implement`, `test`, `verify`, `source_delivery`, `release` and `observe` keys. Source delivery is the Repo Mode delivery segment; Release and Observe receive controller-only `inapplicable` outcomes.
- Terminal statuses are `succeeded`, `failed`, `blocked`, `cancelled` and `inapplicable`. Approval waits, external waits, budget pauses and resumable failures remain nonterminal.
- A StageOutcome uses `pharness.dev/stage-outcome/v1alpha1` and contains:
  - WorkItem, execution and objective identity.
  - Pinned inputs and Product/Repository snapshots.
  - Verified facts with evidence-validation references.
  - Agent claims separately labeled.
  - Outputs, acceptance, decisions and authorizations.
  - Contradictions, risks, unavailable capabilities and recommendations.
  - Exact stop reason, sealed state version and canonical content hash.
- Add immutable `pharness.dev/evidence-validation/v1alpha1` records referencing existing artifacts and observations rather than copying raw evidence. Initial validators cover checkout SHA, contract, EnvironmentSnapshot, diff and changed paths, declared acceptance, pull-request head, required provider checks and merge provenance.
- Add immutable `pharness.dev/agent-context/v1alpha1` context packs. They contain current intent, pinned Product/repository state, effective upstream outcomes, remaining budgets, policies, grants, contradictions, risks, operator decisions and an allowlisted evidence catalog.
- Exclude raw transcripts and full diffs from default context. Agents retrieve deeper evidence through a typed `get_evidence` tool; every retrieval records actor, Run, stage, evidence version and returned hash.
- Cap assembled context at 16,000 estimated tokens. Mandatory intent, acceptance, contradictions, failures, operator decisions and provenance cannot be compacted away. Block before model invocation if mandatory context cannot fit.
- Add append-only operator annotations with target, statement, evidence refs, requested effect, actor, reason and state hash. Controller decisions may add context, mark evidence stale or require a stage repeat/replan, but cannot override facts or rewrite sealed outcomes.
- Snapshot approved WorkPlan and ChangeSet revisions into immutable evidence before sealing their outcomes.

### 5. Fixed sequential stage-agent execution

Add read-only `GET /api/agent-profiles`; V1 ships five compiled, versioned and hash-pinned profiles with no general profile-management API:

| Profile | Default soft/hard budget | Allowed behavior |
| --- | --- | --- |
| `repository-onboarding-proposer` | 16/24 turns, 100k/200k tokens, 600s | Discovery and bounded repository reads; typed proposal submission |
| `repo-planner` | 16/24 turns, 100k/200k tokens, 600s | Read-only context/evidence; typed WorkPlan submission |
| `repo-builder` | Existing WorkItem-selected budget, default 48/100 turns and 400k/1m tokens, 3600s | Existing reliable coding loop, exact writable grant, typed acceptance, diff/status |
| `repo-tester` | 8/12 turns, 80k/160k tokens, 900s | Declared acceptance commands and evidence only |
| `repo-verifier` | 12/20 turns, 120k/240k tokens, 900s | Read-only repository/diff/evidence inspection and typed verification submission |

- All profiles initially use the configured Fireworks provider/model. The profile hash binds prompt version, model identifier, tool schemas, tool allowlist and budget policy, but never credentials.
- Extend Run execution targets with subject, StageExecution, profile ID/version/hash and context-pack ID.
- Filter native tool schemas per profile and enforce the same allowlist in the executor. Prompt instructions alone are not an authorization boundary.
- WorkItem execution proceeds:
  1. Controller seals Discover from pinned readiness evidence.
  2. Operator explicitly starts the Planner AgentRun.
  3. Planner submits a proposed WorkPlan.
  4. Operator approves the exact WorkPlan.
  5. One `authorize_stage_chain` action atomically creates the Builder’s four-hour workspace grant and a state-hashed Builder → Tester → Verifier chain authorization.
  6. Builder starts and, on controller-validated success, Tester and Verifier dispatch automatically within the authorized envelope.
- The chain authorization binds WorkItem, WorkPlan revision, Product snapshot, mutable Repository, source SHA, workspace, writable paths, profiles, budgets and state hash. It is checked again before each stage dispatch.
- The Builder grant permits only `write_file`, `patch_file` and safe directory creation inside contract-declared writable paths. It never authorizes Git, provider, pipeline, deployment or other external effects.
- Planner, Tester and Verifier cannot modify the durable source. Tester uses the same workspace identity but runs acceptance against a source-hash-verified ephemeral copy so commands such as `compileall` cannot alter the Builder workspace. Verifier mounts the durable source read-only.
- Budget or tool-approval pauses preserve Run identity and the PVC. Expired chain/grant authorization requires renewal before the next dispatch.
- A terminal Builder, Test or Verify failure preserves the workspace and outcome, blocks the WorkItem, and offers a state-hashed correction action. Correction creates a new Builder StageExecution, context pack, grant and chain authorization on the same workspace. A full replan creates a new workspace and Plan execution.
- No stage automatically retries after terminal failure. Changing intent, mutable Repository or required acceptance creates a linked WorkItem.

### 6. Repo Mode controller, provider checks and closure

- Dispatch `mode=repo` WorkItems to a new dedicated Repo Mode controller module. Keep the existing full-SDLC reconciler untouched for legacy WorkItems.
- Derive actions deterministically and apply them transactionally. Reads and navigation never reconcile or dispatch work.
- Repo Mode creates only applicable source/coding gates. It must never create Pipeline, GitOps, cluster, deployment or rollback gates.
- A verified WorkItem produces a ChangeSet from the exact Builder workspace and effective Implement/Test/Verify outcomes. ChangeSet provenance binds workspace, Run, StageExecution, source SHA, diff hash and WorkPlan revision.
- Source PR creation requires ChangeSet approval, source-mutation authorization and Git writer authorization. Manual merge remains mandatory.
- Extend the GitHub observer to produce immutable `ProviderCheckSetObservation` records:
  - Resolve active required checks from the branch’s applicable rules/rulesets and classic branch protection.
  - Fetch check runs and commit statuses for the exact PR head SHA.
  - Bind expected GitHub App identity when the provider rule does.
  - Treat `success`, `skipped` and `neutral` as passing, and require both a check run and commit status when the same required name exists in both systems.
  - An unreadable rule set is unavailable, never an inferred empty set.
  - An empty required set is valid only after authoritative rule queries succeed.
  - Use a 15-minute PHarness observation freshness window.
  
  These rules follow GitHub’s [active branch-rules API](https://docs.github.com/en/rest/repos/rules), [protected-branch contract](https://docs.github.com/en/rest/branches/branch-protection), and [required-check semantics](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks).
- A PR head, required-set hash, result or expected provider-app change invalidates readiness.
- Merge success requires a fresh passing pre-merge observation and a merge observation confirming the same head and required-set/result state. Because GitHub does not supply historical branch-rule state, missing fresh pre-merge evidence is a failed PHarness delivery rather than inferred success.
- If an unapproved head change occurs before merge, block source delivery and require the PR to be closed before correction/replan. Never adopt external commits silently.
- If that head is already merged, or a merge occurs with missing, stale or failed required checks, record immutable merge provenance, seal Source Delivery as `failed`, set the WorkItem to terminal `failed`, and set `closed_at`. It is not replan-eligible.
- A merge with exact approved provenance and current checks seals Source Delivery as `succeeded`, sets the WorkItem `completed`, and closes it.
- Release and Observe remain `inapplicable`; source merge never manufactures a Release.

## Public Interfaces and Screen-Plan Dependencies

### New and extended routes

- Organization and Product:
  - `GET /api/organization`
  - `GET /api/organization/overview`
  - `GET|POST /api/products`
  - `GET|PATCH /api/products/:product_id`
  - `GET /api/products/:product_id/model-snapshots/:snapshot_id`
  - `GET /api/products/:product_id/services`
- Repository:
  - `POST /api/products/:product_id/repositories/preflight`
  - `GET|POST /api/products/:product_id/repositories`
  - `GET /api/repositories/:repository_id`
  - `GET /api/repositories/:repository_id/readiness?source_commit=<sha>`
  - `POST /api/repositories/:repository_id/readiness-assessments`
  - `POST /api/repositories/:repository_id/onboardings`
  - `GET /api/repository-onboardings/:onboarding_id`
  - `GET /api/repository-onboardings/:onboarding_id/flow`
  - `PUT /api/repository-onboardings/:onboarding_id/proposal`
  - `POST /api/repository-onboardings/:onboarding_id/actions/:action_id/execute`
- WorkItem and execution:
  - `POST /api/products/:product_id/work-items/preflight`
  - `POST /api/products/:product_id/work-items`
  - `GET /api/agent-profiles`
  - `GET /api/work-items/:work_item_id/stage-executions`
  - `GET /api/stage-executions/:stage_execution_id`
  - `GET /api/stage-executions/:stage_execution_id/outcome`
  - `GET /api/stage-executions/:stage_execution_id/context-pack`
  - `POST /api/work-items/:work_item_id/annotations`
- Existing action, approval, Run, WorkPlan, ChangeSet, diff, artifact, audit and source-delivery routes remain compatible.

All mutation requests require actor and reason. Revisions and lifecycle actions additionally require the latest state hash. Creation after preflight requires the exact preflight hash.

### Repo WorkItem request

The Product-scoped request accepts:

- Title and bounded intent.
- One registered mutable `repository_id`.
- Full immutable `source_commit`.
- A nonempty list of acceptance-command names from the active contract.
- Up to four read-only context repositories, each with registered `repository_id` and full commit SHA.
- Builder budgets and attempt limit, defaulting to two and capped at three.

The server derives repository URL, base branch, contract version, exact command strings, EnvironmentProfile and writable paths. Clients cannot provide runner images, arbitrary commands, provider targets or writable paths.

Context repositories require an active Product binding and deterministic discovery at the exact revision. They are exposed through typed bounded reads, not writable mounts.

### Required read-model fields

The separate Screen Plan may rely on API-backed:

- Product-model snapshot and repository binding state.
- Contract and coding readiness with exact checks/blockers.
- Capability availability, trust policy and current authorization as separate axes.
- Current stage, StageExecution, effective StageOutcome and history.
- Verified facts, outputs, acceptance, claims, contradictions, risks, freshness and provenance.
- Current AgentRun/profile, budgets, context-pack hash and stop reason.
- Current external wait, action rail, attention reason and exact corrective action.
- Source-delivery intent, PR head, required-check set/results and merge provenance.
- Aggregate per-stage turns, tokens, active time, retries and approval wait time.

No client may infer success, origin, readiness, attention counts or current execution from titles, actor names or visible-page subsets.

## Test and Acceptance Plan

### Deterministic gates

- Contract tests cover canonical/alias/missing/conflict behavior, schema/path security, missing immutable lock and migration.
- Store tests cover Product uniqueness, shared Repository bindings, immutable snapshots, generalized workspaces/preparations, StageOutcome SQL immutability, effective pointers and migration-0039 compatibility.
- Discovery tests cover stable ordering and hashes, size limits, symlink escape, submodules, binary metadata, conflicting indicators and exact SHA rejection.
- Onboarding tests prove discovery/proposal jobs have no writer capability, proposal revisioning, allowed diff paths, missing-lock blocking, exact merge validation and contract amendment behavior.
- Readiness tests cover every input-tuple invalidation and capability/trust/authorization separation.
- Agent tests cover immutable profile hashes, role-specific tool-schema filtering, executor enforcement, structured submission tools and context bounds.
- Controller tests cover Planner approval, chain authorization, automatic bounded stage dispatch, expiry, budget pauses, same-workspace correction, fresh-workspace replan, legacy reconciliation and Repo Mode’s lack of downstream delivery gates.
- Evidence tests cover validator scoping, claim/fact separation, immutable outcomes, annotations, deterministic context assembly and audited evidence retrieval.
- GitHub adapter tests cover rulesets, classic protection, empty sets, app-bound checks, checks/statuses with duplicate names, passing/pending/failed states, stale observations, changed heads, changed required sets and merge without passing evidence.
- End-to-end fake-provider tests cover:
  - Complete registration → onboarding → readiness → WorkItem → stage chain → source PR → checks → merge → closure.
  - Merged PR with failed/missing checks producing closed failed delivery.
  - Pre-merge head drift producing a blocked correction boundary.
- Preserve all existing workspace tests, legacy production-chain tests and API response characterization fixtures.

### Reliability and release gates

- Run `cargo fmt --check`, workspace/all-target tests, Clippy with warnings denied, UI production build, Vitest and full Playwright compatibility suite.
- Run SQLite upgrade tests from a migration-0039 fixture and an old-release read/rollback test against a migrated database copy.
- Run Helm lint/template/schema, server-side Kubernetes dry-run, Linux/AMD64 OCI inspection and rendered-manifest scans proving no `:latest`.
- Run deterministic runner/discovery/preparation smokes without Fireworks.
- Run the existing 16-case matched Fireworks coding evaluation against the deployed baseline and candidate Builder profile. Require no fewer passes, no safety regression, no new environment-discovery loop and no increase in context-limit failures.
- Add fixed Planner, Tester and Verifier fixtures proving structured outputs and evidence handoff; these do not replace the matched Builder evaluation.
- Fireworks and live provider runs remain explicitly authorized external validation.

### Release and supervised acceptance

1. Merge implementation to `main` only after deterministic gates pass; record the exact source SHA.
2. Build API runtime, UI and Python runner from that one SHA, capture all digests and revision/platform labels, and create a separate digest-pinning release commit.
3. Back up SQLite, deploy only through the PHarness Argo Application, and verify the exact release revision, migrations, pod image IDs, profile hashes and readiness.
4. Enable Repo Mode only after source-reader and GitHub observer verification passes. Keep legacy flow enabled.
5. In a clean yfinance worktree, create a Product and register `lward27/yfinance_wrapper` at its then-current immutable `main` SHA.
6. Confirm discovery identifies the existing compatibility alias, hashed lock, Python profile, acceptance commands and no required provider checks unless provider configuration has changed.
7. Review and deliver an onboarding PR that renames `.pharness/project.yaml` to `.pharness/repository.yaml` and updates the instructions reference only. Merge manually and validate the exact merge SHA.
8. Require contract and coding readiness before creating the WorkItem.
9. Submit this bounded task:
   - Add a pure normalized-period validator for yfinance’s supported history periods.
   - Make period mutually exclusive with explicit start/end dates.
   - Convert input-validation failures at `/history` into stable HTTP 422 responses before any upstream call.
   - Add standard-library unit tests and update `readme.md`.
   - Acceptance: `python -m unittest discover -s tests -v` and `python -m compileall -q src tests`.
10. Assert the task is not already present before submission; if it is, stop and revise the acceptance task rather than silently changing the live test.
11. Start Planner, approve its exact WorkPlan, then authorize the bounded Builder → Tester → Verifier chain.
12. Require preparation before turn zero, no environment/network/package discovery, durable source/test/docs changes, both declared commands passing and controller-sealed stage outcomes.
13. Review and approve the ChangeSet and source-delivery authorization; create the PR.
14. Observe the exact head and authoritative required-check set. For the current yfinance repository an explicitly observed empty set is acceptable; deterministic tests prove nonempty and failure behavior.
15. Merge manually, observe exact merge provenance, close the WorkItem successfully, and confirm Release/Observe are inapplicable.
16. Preserve discovery, proposal revisions, actions, context packs, outcomes, provider observations, API responses and Run events as characterization evidence.
17. Keep workspace PVCs for seven days by default and evidence indefinitely; place the live smoke under a retention hold until milestone acceptance.
18. Verify the prior release commit/digests remain a runnable GitOps rollback. Do not down-migrate or delete Repo Mode evidence.
19. Tag the accepted release-pin commit `v4-repo-mode-v1`, recording the build SHA and three image digests in the annotated tag.

## Assumptions and Boundaries

- Organization is one configured bootstrap resource in V1.
- GitHub.com HTTPS is the only registration provider; public contracts remain provider-neutral.
- Repo Mode is source-only and mutates one Repository per WorkItem.
- Manual merge is mandatory for onboarding and WorkItem PRs.
- The fixed stage chain has no direct agent-to-agent messaging, generic bus, swarm or parallel scheduling.
- Existing Fireworks provider/model configuration is used for all V1 AgentProfiles.
- No Service or Environment is manufactured merely to register a Repository.
- No dependency-lock generation, deployment, runtime health, Release, Observe, Product Steward, satellite, multi-Repository mutation or DeliveryPlan is included.
- The existing full supervised SDLC path remains available to legacy WorkItems.
- The following Screen Plan owns all visual redesign after these APIs and semantics are fixed.
