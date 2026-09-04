# PHarness Repo Mode V1 screen contract

Status: approved Plan Mode entry point

Approved: 2026-08-24

Planning order: **second, after the product plan**

Upstream product entry point:
[`repo-mode-v1-product-contract.md`](repo-mode-v1-product-contract.md)

## Purpose

This document is the authoritative entry point for planning the Repo Mode V1
operator experience. It defines page ownership, information hierarchy,
interaction semantics, state honesty, responsive behavior, and the first
screen acceptance boundary.

Use this document to produce a separate UI implementation plan, not to begin
implementation. The plan consumes the approved product contract and the
completed product plan. It may identify missing API or read-model dependencies,
but it must not redefine backend state, trust, authorization, or lifecycle
semantics.

## Operator promise

An engineering or platform lead can understand one Product, onboard a
Repository, initiate and follow one WorkItem, review what each stage actually
established, perform the exact current human action, and distinguish current
execution from all prior history without reconstructing state from raw events
or visiting unrelated resource pages.

## V1 screen scope

The first screen milestone owns:

- Global application shell and navigation hierarchy.
- Read-oriented Organization Overview.
- Product list and Product detail.
- Repository list, detail, onboarding, and readiness.
- WorkItem list, creation, and detail.
- Current AgentRun list and AgentRun drill-down.
- StageOutcome, evidence, approval, wait, and history presentation.
- Desktop and phone-width behavior for the complete Repo Mode journey.

Releases, Insights, and Settings remain in global navigation and preserve
honest existing behavior, but this milestone does not expand them into
Connected Mode. Product Graph, broad AgentProfile management, multi-agent
swarm visualization, and secondary-persona specialization are deferred.

## Visual direction

### Approved Lamina update — 2026-09-04

The Lamina Operator Console milestone supersedes the earlier shell styling:
use the supplied glass/pattern/glow prototype, floating top navigation in the
same eight-section order, and horizontal laminae as WorkItem Overview's default.
Intervals are server-projected recorded elapsed time, not fabricated progress
or active model time. Repeated/repair executions remain distinct; Source
Delivery includes recorded PR wait. Missing times are explicitly unavailable.
Release/Observe remain inapplicable for Repo Mode. Phone navigation uses a
drawer and one column; timeline scrolling is contained and keyboard accessible.
Keep dark/light themes, self-hosted assets, reduced motion, and the current
console fallback behind `features.repoModeV1.designOverhaulEnabled=false`.
See [the approved implementation plan](../active/PHarness-Lamina-Operator-Console-Redesign.md).

The supplied Product and organization dashboard concepts are the visual north
star for dark, polished, information-dense, Product-oriented composition. The
implementation should preserve their clarity, hierarchy, lifecycle visibility,
compact typography, and restrained status color without copying fictional
data or behaviors.

Use progressive disclosure:

- Identity, current boundary, blocker, and next action remain primary.
- Dense evidence and trust detail is collapsible.
- Raw events are drill-down material.
- Large-screen secondary rails appear only when their content is relevant.
- Do not introduce a double-left-sidebar layout.
- Dark mode is preferred while theme switching remains available.
- On narrow screens, use one prioritized content column rather than compressing
  desktop tables and rails.

Every visible state is API-backed. Do not invent Products, agent swarms,
capabilities, Releases, health, approvals, or activity to make a screen look
complete.

## Global shell

The initial navigation order is:

1. **Overview**
2. **Products**
3. **Repositories**
4. **WorkItems**
5. **Agents**
6. **Releases**
7. **Insights**
8. **Settings**

Pipelines, GitOps Applications, build providers, and observability systems are
nested under their owning Product, WorkItem, Release, or capability settings.

The global Environment selector is read/filter context only. It may narrow
rollups and lists but never supplies an implicit mutation target. Navigation,
search, filtering, and refresh never dispatch controller work.

Navigation badges indicate actionable attention, not general record counts.
Search and cross-cutting lists always retain links to the owning Product and
WorkItem.

## Organization Overview

Overview is read-oriented and answers:

- Which Products exist and which need attention?
- Which WorkItems are active, waiting, blocked, failed, or recently complete?
- Which exact human or external waits are actionable?
- Which AgentRuns are currently active, and what owns them?
- Which repository capabilities are unavailable or stale?

The initial dashboard may show exact counts, rates with stated denominators,
and age distributions. It does not show Autonomous Success Rate or a synthetic
Product-health number.

Product health remains separate across:

- Work flow.
- Release and runtime observation when genuinely connected.
- Evidence freshness.
- Capability readiness.

An approval or attention item navigates to its owning WorkItem or resource. It
is not approved from a context-free dashboard card.

## Product list and detail

The Product list prioritizes identity, owner, registered Repository count,
active WorkItems, actionable waits, and capability posture. A Product may exist
before its first Repository is ready.

Product detail establishes:

- Product identity and owner.
- Services and versioned RepositoryBindings.
- Registered Repositories and onboarding readiness.
- Active WorkItems grouped by current boundary.
- Current AgentRuns with WorkItem and stage ownership.
- Connected Release posture only when real Release data exists.
- Evidence freshness and capability readiness as separate dimensions.

The default Product surface emphasizes current WorkItems. Completed and
superseded work is available as History and is never mixed into a current
execution.

Recommended Product sections are:

1. **WorkItems**
2. **Services and Repositories**
3. **Agents**
4. **Releases**
5. **Evidence and Audit**
6. **History**

Product Graph is deferred and no editable relationship graph is required for
Repo Mode V1.

## Repository list, detail, and onboarding

The Repository surface shows registration separately from readiness. Its
primary states include:

- Not registered.
- Registered and awaiting discovery.
- Discovery active.
- Proposal ready for review.
- Onboarding PR awaiting manual merge.
- Validating merged contract.
- Contract ready but coding blocked.
- Coding ready.
- Readiness stale or invalid, with exact reason.

The onboarding experience is one comprehensible flow:

1. Select or create Product and register Repository at an immutable revision.
2. Show deterministic discovery progress and facts.
3. Show agent proposal, assumptions, unsupported inferences, and proposed diff.
4. Review Product or Service mappings separately from executable contract
   changes.
5. Confirm the exact onboarding pull-request effect.
6. Show pull-request identity and manual-merge wait.
7. Observe merge and exact-revision validation.
8. Show contract readiness, coding readiness, capability gaps, and corrective
   action.

The screen must distinguish:

- Contract facts from capability checks.
- Capability availability from trust policy.
- Trust policy from one-time authorization.
- An invalid contract from an unavailable writer.

WorkItem creation is disabled when coding readiness fails. The disabled state
names the exact blocker and routes the operator to the corrective Repository
surface instead of offering a generic retry.

## WorkItem list and creation

The WorkItem list groups current work by Product and lifecycle boundary. It
shows intent, mutable Repository, current stage, exact wait or blocker,
effective outcome, current AgentRun, and age. Current work is the default;
completed and superseded WorkItems are a separate history lens.

Creation begins from a Product and a coding-ready Repository. It captures one
bounded intent, immutable source revision, acceptance boundary, applicable
Service context, execution profile, and budget. Preflight explains any blocker
before durable creation.

The final confirmation states what PHarness may mutate and which later actions
still require separate authorization.

## WorkItem detail

WorkItem Overview is the default operational screen. Its persistent header
shows:

- Intent and immutable scope.
- Product, optional Service, mutable Repository, and pinned revision.
- Required acceptance boundary.
- Lifecycle position and current StageExecution.
- Exact current wait, blocker, or stop reason.
- One recommended next action.

The primary sections are:

1. **Overview** — lifecycle, current boundary, next action, compact current
   execution, and sealed outcome rollups.
2. **Current Stage** — active StageExecution, AgentRuns, environment, budgets,
   activity, recoveries, and current approvals.
3. **Stage Outcomes** — controller-sealed conclusions for completed and
   inapplicable stages.
4. **Delivery** — source PR, head SHA, checks, merge wait, and final provenance.
5. **Evidence** — typed evidence grouped by the exact claim it supports.
6. **History** — prior plans, StageExecutions, outcomes, annotations, replans,
   and linked WorkItems.

The UI never requires the operator to visit Triage, Flow, approvals, approval
gates, and resource pages in an undocumented sequence merely to advance one
WorkItem.

## Current Stage and AgentRun

Current Stage shows exactly one active StageExecution as the primary execution
surface. It includes:

- AgentProfile and AgentRun identity.
- Current objective and stage.
- EnvironmentSnapshot, runner identity, and unavailable tools.
- Configured, consumed, and remaining turn, token, time, and recovery budgets.
- Live model and tool activity.
- Current approval or budget-extension request.
- Changed paths, acceptance progress, recoveries, and stop condition.

Raw model/tool events remain available in a collapsed evidence stream. They do
not replace the controller-derived current state.

The Agents top-level surface initially separates active AgentRuns from Run
history. AgentRun drill-down retains Product, WorkItem, StageExecution, and
AgentProfile ownership. Broad AgentProfile editing is deferred.

## StageOutcome visual contract

Every StageOutcome rollup shows:

- Lifecycle stage and StageExecution identity.
- Terminal status.
- Exact stop reason.
- Effective, superseded, and freshness state.
- Verified facts.
- Verified outputs and changed resources.
- Acceptance results.
- Unresolved risks and contradictions.
- Agent claims that remain unverified.
- Decisions and authorizations consumed.
- Evidence links beside the claim they support.

Verified facts, agent claims, operator annotations, contradictions, and
recommendations use distinct labels and visual treatment. Color alone is not
sufficient.

The default card is a concise conclusion. Expanding it reveals evidence and
provenance. Raw events and artifacts remain a deeper drill-down.

If evidence becomes stale, the UI preserves the original terminal result while
showing that the outcome is no longer sufficient for a current action. If a
later outcome supersedes it, the effective outcome remains primary and the
older outcome remains under History.

## Action and approval contract

The current owning resource presents one recommended next action. Other actions
remain visibly secondary and explain their effect class.

Keep these interactions distinct:

- Read, navigate, filter, refresh, and expand evidence.
- Advance safe internal controller steps.
- Resume a paused execution.
- Approve a budget extension.
- Replan within the WorkItem identity boundary.
- Authorize an attempt-scoped workspace grant.
- Authorize source mutation or pull-request creation.
- Observe an external check or manual merge.

Future lifecycle gates may be visible but are not actionable early. A disabled
action names the reason and corrective action. Acknowledging a notification
never grants authorization.

Every effectful confirmation names the exact Product, WorkItem, Repository,
revision, external target, state hash, actor, reason, and expected effect. The
UI never constructs arbitrary effect targets.

Preview and apply remain separate. Safe internal advance is never styled as
equivalent to an external mutation.

## Current state and history

- Current state is the default at every level.
- One current StageExecution is primary for a WorkItem.
- One effective StageOutcome is primary for each applicable completed stage.
- Previous executions, replans, annotations, and superseded outcomes are
  immutable History.
- Product and portfolio rollups aggregate WorkItems but never synthesize one
  execution from unrelated records.
- Origin and audit provenance come from durable API fields, never actor-name or
  title inference.

## Responsive and accessibility contract

Desktop may use a contextual secondary rail for approvals, readiness, or
evidence when it belongs to the current resource. Phone-width layouts use one
ordered column:

1. Identity and intent.
2. Current boundary and blocker.
3. Recommended next action.
4. Current execution or readiness.
5. Stage outcomes and delivery.
6. Evidence and history.

The implementation must:

- Support keyboard navigation and visible focus.
- Use semantic headings, controls, dialogs, and status announcements.
- Provide labels beyond color for every state.
- Preserve exact confirmation context at narrow widths.
- Avoid horizontal-only lifecycle meaning.
- Keep absolute time available while showing operator-friendly age.
- Maintain desktop and phone-width fixtures for empty, onboarding, active,
  waiting, blocked, failed, completed, and historical states.

## Screen acceptance contract

The screen milestone is complete only when an operator can perform and
understand this journey entirely through the UI:

1. Create or select a Product.
2. Register and onboard a Repository.
3. Review discovery facts, proposal assumptions, and onboarding diff.
4. Confirm the onboarding pull request and follow its manual-merge wait.
5. Understand contract and coding readiness plus exact blockers.
6. Create one WorkItem against a ready Repository.
7. Review its intent, plan boundary, current StageExecution, environment, and
   budgets.
8. Perform only the currently eligible approval or action without searching an
   unrelated page.
9. Follow active AgentRun progress and inspect raw events only when needed.
10. Review sealed StageOutcomes, verified facts, claims, acceptance, risks, and
    evidence.
11. Follow source PR, required checks, manual merge, and final closure.
12. Distinguish current execution from prior attempts and historical WorkItems.
13. Complete the same comprehension path at desktop and phone widths.

Passing component tests or reproducing the visual concept with fixture-only
data is not sufficient. The acceptance path uses real server-backed state.

## Required Plan Mode reading order

Read these sources before inspecting UI implementation details:

1. [`repo-mode-v1-product-contract.md`](repo-mode-v1-product-contract.md)
2. The approved product implementation plan produced from that entry point.
3. [`operator-information-architecture.md`](operator-information-architecture.md)
4. [`stage-outcomes-and-evidence-handoffs.md`](stage-outcomes-and-evidence-handoffs.md)
5. [`repository-onboarding-and-readiness.md`](repository-onboarding-and-readiness.md)
6. [`trusted-autonomy.md`](trusted-autonomy.md)
7. [`../../ui/AGENTS.md`](../../ui/AGENTS.md)
8. [`../../ui/design-qa.md`](../../ui/design-qa.md)
9. [`../architecture/README.md`](../architecture/README.md)
10. [`../implemented/README.md`](../implemented/README.md)
11. [`../active/README.md`](../active/README.md)

Then inspect the current routes, components, state hooks, API clients, design
tokens, responsive behavior, accessibility, Vitest coverage, Playwright
fixtures, screenshots, and real API payloads at the exact Git revision.

## Screen Plan Mode output requirements

Create one UI implementation plan that:

1. Begins with a route, component, state-source, and test characterization of
   the current console.
2. Maps every proposed screen field and action to an existing or product-plan
   API contract.
3. Lists missing read models or actions as explicit dependencies without
   redefining product semantics.
4. Sequences reusable shell, Product, Repository, WorkItem, StageOutcome, and
   AgentRun slices without a second competing navigation system.
5. Preserves current external-effect, preview/apply, state-hash, and approval
   protections.
6. Defines desktop and phone-width fixtures plus empty, loading, unavailable,
   stale, waiting, blocked, failed, completed, and historical states.
7. Includes accessibility checks, production build, Vitest, and full
   Playwright acceptance.
8. Includes a real API-backed end-to-end Repo Mode operator journey.
9. Names legacy screens or concepts to preserve, adapt, redirect, or retire.
10. Does not create backend migrations or change product semantics; those are
    dependencies on the product plan.
11. Creates no implementation changes while still in Plan Mode.

If the current API cannot support an honest screen, record the exact missing
contract and dependency. Do not synthesize state in the client.
