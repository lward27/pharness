# PHarness stage outcomes and evidence handoffs

Status: living design

Last decision round: 2026-08-24

Upstream authority:
[`product-vision-and-boundaries.md`](product-vision-and-boundaries.md)

## Purpose

PHarness already persists detailed events, tool results, artifacts, approvals,
provenance, tests, delivery evidence, and release verification. The missing
product contract is how those records become a trustworthy stage conclusion,
an understandable operator rollup, and bounded context for the next AgentRun.

Raw history is evidence, not a handoff. An earlier agent's summary is a claim,
not automatically a fact. The deterministic controller must adjudicate and
seal the effective stage outcome before the WorkItem advances.

## WorkItem is the correlation root

All execution and evidence for one engineering intent must be navigable from
its WorkItem:

```text
WorkItem
  -> intent and current scope
  -> lifecycle stages
    -> stage executions
      -> AgentRuns
      -> tool executions
      -> evidence and approvals
      -> sealed StageOutcome
  -> source and delivery resources
  -> current effective state
  -> prior history
```

Product, Repository, AgentRun, approval, and Release views may provide
cross-WorkItem lenses, but they must retain the WorkItem correlation and must
not mix unrelated records into one apparent execution.

## WorkItem and stage-execution identity

A WorkItem represents one bounded engineering intent. A replan remains within
the same WorkItem when the desired outcome, mutable Repository, and required
acceptance boundary remain materially the same. A materially changed outcome,
Repository, delivery target, or acceptance boundary creates a new linked
WorkItem rather than rewriting the original intent.

Execution identity is equally explicit:

- Provider transport retries and recoverable tool retries remain within the
  same StageExecution.
- Resuming after an approval wait or budget extension remains within the same
  StageExecution.
- A full replan or deliberate repeat of a lifecycle stage creates a new
  StageExecution.
- Terminal outcomes are immutable. The controller may select a later outcome
  as effective without altering or hiding the earlier one.
- Prior executions and outcomes appear under History with their exact stop
  reasons and relationships.

Typed WorkItem relationships such as `supersedes`, `follows_up`, and
`discovered_from` retain continuity when an intent change requires new work.

## StageOutcome contract direction

A StageOutcome is an immutable, controller-sealed conclusion for one stage
execution. It should contain:

- WorkItem, stage, and stage-execution identity.
- Objective and terminal status.
- Immutable inputs and Product/Repository model snapshot references.
- Verified facts with evidence references, provenance, scope, and freshness.
- Agent claims that could not be independently verified.
- Decisions made and authorizations consumed.
- Outputs, changed resources, and acceptance results.
- Unresolved risks, contradictions, and unavailable capabilities.
- Recommendations clearly separated from facts and decisions.
- Exact stop reason.
- A content and state hash binding the outcome to its inputs and WorkItem state.

Sealed outcomes are never rewritten. A retry, resume, or replan creates new
execution history and may produce a later effective outcome; the earlier
outcome remains inspectable.

## Controller sealing flow

```mermaid
flowchart LR
    A["AgentRun finishes"] --> B["Proposed summary and recommendations"]
    C["Durable events and artifacts"] --> D["Controller validation"]
    B --> D
    E["Policy, approvals, and WorkItem state"] --> D
    D --> F["Sealed StageOutcome"]
    F --> G["Stage transition"]
    F --> H["Operator rollup"]
    F --> I["Next-stage context assembler"]
```

The controller owns:

1. Selecting records within the exact WorkItem and stage-execution scope.
2. Verifying objective claims against typed evidence and acceptance rules.
3. Separating verified fact, unverified claim, decision, and recommendation.
4. Recording contradictions and stale evidence instead of smoothing them over.
5. Binding the result to current state and rejecting stale sealing attempts.
6. Determining whether the lifecycle may advance.

An agent may propose a summary, but it cannot seal its own success or advance
the lifecycle directly.

## Context for the next AgentRun

The next AgentRun should receive a bounded context pack rather than every prior
transcript. The pack should include:

- Current WorkItem intent, scope, and acceptance criteria.
- Relevant Product, Service, Repository, and Environment snapshot.
- Effective upstream StageOutcomes.
- Remaining budgets, policies, grants, and unavailable capabilities.
- Exact evidence references needed for the next objective.
- Explicit contradictions, unresolved risks, and operator decisions.
- Typed tools for retrieving deeper evidence on demand.

The context pack records which outcome and evidence versions it used. Context
selection must be deterministic enough to reproduce and audit, while remaining
bounded enough to avoid exhausting model context with raw history.

## Initial control model

The first control model remains deliberately simple:

- The deterministic controller owns state, policy, evidence sealing, and stage
  transitions.
- An ephemeral Planner AgentRun may propose a WorkPlan or replan.
- Stage-specific AgentProfiles perform bounded work.
- The controller assembles context and dispatches the next AgentRun.
- No persistent Product-level control agent is required for Repo Mode.

A persistent Product Steward becomes meaningful later, when Connected Mode can
feed runtime and Observe evidence back into task generation. Even then, it may
propose WorkItems and coordination; it does not become the source of truth for
state or policy.

## Operator rollups and history separation

The default WorkItem experience should answer these questions without exposing
an undifferentiated event stream:

- What intent is PHarness pursuing?
- What is the current stage and exact boundary?
- What did each completed stage establish?
- Which facts are verified and which remain claims or risks?
- What changed, what was tested, and what evidence supports the result?
- What is waiting on a human or external system?
- What happened in previous attempts, and why are they no longer current?

The UI should present:

1. **Intent and scope** as the WorkItem header.
2. **Current execution** as the primary operational surface.
3. **Stage rollups** from sealed StageOutcomes.
4. **Current evidence and delivery state** attached to the owning stage.
5. **History** as a separate chronological view of prior executions and
   superseded outcomes.
6. **Raw events and artifacts** as drill-down evidence, not the default mental
   model.

Portfolio and Product views aggregate WorkItem rollups. They must not merge
events from several WorkItems into a synthetic execution or place old and
current AgentRuns beside each other without status and ownership context.

## Open decisions before implementation planning

1. Define StageOutcome status and supersession semantics, including failed,
   blocked, cancelled, inapplicable, and stale outcomes.
2. Define the first typed evidence validators and what remains an agent claim.
3. Define context-pack selection, token budgeting, compaction, and retrieval
   audit records.
4. Define how an operator correction or annotation affects a sealed outcome
   without rewriting history.
5. Map the proposed concepts to existing PHarness events and resources before
   adding a new database entity.

Do not implement a generic multi-agent message bus. StageOutcome and evidence
retrieval are the first handoff contracts; orchestration builds on them later.
