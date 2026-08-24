# PHarness operator information architecture

Status: living design

Last decision round: 2026-08-24

Upstream authorities:

- [`product-vision-and-boundaries.md`](product-vision-and-boundaries.md)
- [`product-model.md`](product-model.md)
- [`stage-outcomes-and-evidence-handoffs.md`](stage-outcomes-and-evidence-handoffs.md)

## Purpose

This document defines how operators should navigate and understand PHarness.
It translates the product model into view ownership and information hierarchy;
it is not a visual specification or an implementation milestone.

The supplied dashboard concepts are a useful north star for composition,
density, lifecycle visibility, and Product-level rollups. They are not a
literal feature contract. Labels, scores, apparent autonomous behaviors, and
security claims shown in a concept must be backed by the real PHarness model
before they appear in the product.

## Primary persona

The initial primary persona is an engineering or platform lead who needs to
understand work across Products, repositories, agents, delivery systems, and
environments without reading every event.

This operator needs to:

- See what is active, waiting, blocked, or complete.
- Understand how an intent relates to all execution and evidence beneath it.
- Review exact human and external-effect boundaries.
- Distinguish current execution from prior attempts and historical WorkItems.
- Inspect capability availability separately from trust and authorization.
- Navigate from portfolio rollups to the owning resource before acting.

Detailed developer, security reviewer, release manager, and executive
personas may be added later. The initial UI should not fragment into
role-specific applications before the core model is comprehensible.

## Canonical view hierarchy

```text
Organization overview
  -> Product
    -> WorkItem
      -> Current StageExecution
      -> StageOutcomes
      -> Delivery and Release links
      -> Evidence
      -> History
    -> Service
    -> Repository
    -> Environment
    -> Release

Cross-cutting lenses
  -> Repositories
  -> WorkItems
  -> AgentProfiles and AgentRuns
  -> Releases
  -> Insights
```

The ownership hierarchy answers where an action belongs. Cross-cutting lenses
support discovery and comparison, but always link back to the owning Product,
WorkItem, stage, or Release.

## Organization overview

The Organization overview is primarily read-oriented. It should summarize:

- Products and their current delivery or runtime posture.
- Active WorkItems grouped by current lifecycle boundary.
- Exact human waits, external waits, and actionable blockers.
- Current Releases and environments with verified state.
- Running AgentRuns with owning Product, WorkItem, and stage.
- Capability gaps or stale provider observations.
- Recent high-signal decisions and outcomes.

An Organization dashboard must not become a generic action console. An
approval or requested action navigates to the exact owning WorkItem or Release
where scope, evidence, state hash, and effect can be reviewed.

Do not show **Autonomous Success Rate**. A single autonomy score hides the
denominator and encourages the wrong behavior. Product health should remain
dimensioned until honest measures are defined.

## Product view

The Product is the principal rollup for one software system. Its header should
establish identity, Services, registered Repositories, Environments, current
Release posture, and connected capability state.

The primary Product surface should make concurrent WorkItems legible. A
recommended information grouping is:

- **WorkItems** — current intent rollups first; completed and superseded work
  separated into History.
- **Services and Repositories** — explicit mappings, onboarding readiness, and
  immutable contract revisions.
- **Releases** — promoted revisions by Environment and their verification
  state.
- **Agents** — current AgentRuns, AgentProfiles, and execution ownership.
- **Evidence and Audit** — sealed outcomes, external observations, approvals,
  and raw drill-down.
- **Graph** — future Product relationships and DeliveryPlan dependencies once
  their semantics are defined.

Product health must not be inferred from the most advanced WorkItem stage.
The Product can have several concurrent WorkItems and separately deployed
Releases.

## WorkItem view

The WorkItem is the canonical operational surface for one engineering intent.
Its header should continuously answer:

- What outcome is being pursued?
- Which Product, Services, mutable Repository, and Environments are in scope?
- What acceptance boundary defines completion?
- What is the current lifecycle stage and exact stop boundary?
- What is waiting on a person or external system?

Recommended sections are:

1. **Overview** — intent, scope, acceptance, lifecycle, current blockers, and
   the one recommended next action.
2. **Current Stage** — active StageExecution, AgentRuns, tools, budgets,
   approvals, and live evidence.
3. **Stage Outcomes** — controller-sealed rollups of what each completed stage
   established.
4. **Delivery** — source PR, checks, merge observation, and connected build,
   deployment, Release, and rollback resources when available.
5. **Evidence** — artifacts and typed records grouped by their claims and
   owning stage.
6. **History** — prior StageExecutions, superseded outcomes, replans, and
   related WorkItems.

Future approval gates may be previewed in lifecycle order, but they are not
actionable before their owning boundary. Safe internal progression and an
external mutation must never be presented as equivalent buttons.

## Repository view

The Repository view explains registration and source readiness:

- Product and Service bindings.
- Onboarding state and onboarding pull request.
- Canonical Repository contract revision and validation state.
- Mutable and read-only use by active WorkItems.
- Recent source pull requests and observed merges created through PHarness.
- Capability availability for reading, writing, and observing that exact
  Repository.

Repository-level history must retain WorkItem links. It is a lens over work,
not an alternative execution root.

## Agent view

AgentProfile and AgentRun must not be collapsed into one generic Agent object.

- **AgentProfiles** explain the versioned role, model, prompt, skills, and
  policy used for future dispatch.
- **Running AgentRuns** show current Product, WorkItem, stage, objective,
  duration, budget, and status.
- **Run history** is separate and grouped by WorkItem and StageExecution.

An AgentRun detail may show streaming events and tool activity, but it should
not force the operator to reconstruct WorkItem state from raw execution data.

## Release view

The Release view is owned by a Product and Environment. It shows immutable
promotion provenance, contributing WorkItems, artifact revisions, deployment
evidence, verification, observation freshness, and rollback relationships.

Source merge alone does not create a Release. Repo Mode should visibly end at
its verified source boundary instead of displaying an invented deployment
state.

## Current state and history

Current state is the default at every level:

- Product lists emphasize active WorkItems and current Releases.
- WorkItem shows one current StageExecution and effective StageOutcome per
  completed stage.
- Agent surfaces separate active runs from historical runs.
- Previous plans, attempts, and sealed outcomes remain immutable under
  History.
- Completed or related WorkItems roll up beneath the same Product but are not
  mixed with the active execution of another intent.

Cross-WorkItem views may aggregate counts and statuses. They must not merge
events from different WorkItems into a synthetic timeline.

## Approval and notification routing

Global approval and notification queues are navigation aids. Each item should
show Product, WorkItem or Release, lifecycle stage, exact target, effect class,
age, and blocker. Review takes place on the owning resource with its evidence
and current state.

PHarness must not encourage operators to batch-satisfy unrelated future gates.
Capability availability, trust policy, and one-time authorization require
different visual language.

## Ask PHarness

The initial **Ask PHarness** surface has a deliberately bounded purpose:

- Answer read-only questions from durable Product, WorkItem, Release, and
  evidence state.
- Explain blockers, unavailable capabilities, provenance, and why an action
  is or is not eligible.
- Summarize current and historical work without mixing their identity.
- Draft a new WorkItem for explicit operator review and submission.

Ask PHarness never performs a mutation directly. Conversational text is not an
authorization channel. Any proposed external effect becomes a typed WorkItem
or exact lifecycle action and follows the normal review, state-hash, trust,
and authorization path.

## Responsive and density principles

The dashboard concepts use substantial density effectively, but hierarchy
must survive smaller screens:

- Preserve identity, current boundary, blocker, and next action before
  secondary metrics.
- Collapse rollups into drill-down cards rather than horizontally compressing
  every column.
- Use lifecycle visualization to orient, not to imply that unavailable stages
  failed or every Product has one stage.
- Prefer controller-sealed summaries over raw event volume.
- Never use color alone to distinguish unavailable, waiting, failed, and
  complete.

## Open decisions before implementation planning

1. Confirm the exact global navigation and which provider-specific concepts,
   such as Pipelines, remain nested instead of top-level.
2. Define Environment selector semantics. The recommendation is a read/filter
   context only; mutation targets remain explicit on the owning action.
3. Define honest Product health dimensions and portfolio measures without a
   synthetic autonomy score.
4. Define approval-queue and notification behavior, including urgency and
   acknowledgment semantics.
5. Define the visual contract for StageOutcomes, facts, claims,
   contradictions, and evidence freshness.
6. Define the useful first Product graph and whether any relationship may be
   edited from that view.
7. Define the first AgentProfile and AgentRun management surfaces.
8. Identify accessibility and workflow needs of secondary personas after the
   engineering/platform lead experience is coherent.

Do not treat the supplied screenshots as authorization to invent aggregate
metrics, agent swarms, connected capabilities, or Product health claims.
