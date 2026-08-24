# PHarness product vision and decision boundaries

Status: living design

Last decision round: 2026-08-24

## Purpose

This document is the upstream product-definition entry point for PHarness. It
records decisions that are settled enough to constrain architecture and future
milestones, while keeping unresolved questions explicit. It is not an
implementation plan and does not authorize work from the open-question
sections.

PHarness is an autonomous software-stewardship control plane. It understands a
software product across source, services, environments, delivery, evidence,
and operational behavior. It should begin producing value from a repository,
then gain a richer feedback loop and carefully broader autonomy as more of the
engineering environment becomes connected.

The product is opinionated about engineering capabilities, evidence, trust,
and safety semantics. It should not require customers to adopt the reference
Kubernetes, Tekton, Argo CD, and LGTM implementations merely to receive value.

## Canonical hierarchy and vocabulary

Use this hierarchy in product models, APIs, navigation, and future planning:

```text
Organization
  -> Product
    -> Service
      -> Repository
    -> Environment
```

| Term | Meaning |
| --- | --- |
| Organization | Administrative, identity, policy, billing, and fleet boundary |
| Product | The durable software system PHarness reasons about and improves |
| Service | A deployable or operationally meaningful component of a Product |
| Repository | A source-control system of record associated with one or more Services |
| Environment | A named runtime or delivery target such as development, staging, or production |
| WorkItem | A bounded engineering outcome owned by a Product and targeting explicit Services, Repositories, and Environments |
| Release | An immutable promoted outcome for a Product and Environment |
| AgentProfile | A versioned role definition including model, prompt, skills, and capability policy |
| AgentRun | One execution of an AgentProfile for a bounded objective |
| ExecutionStrategy | The orchestration shape for AgentRuns: single, sequential chain, or parallel swarm |

Use **Product**, not **Project**, for the primary unit of reasoning. Do not
introduce a separate Project resource unless a future decision gives it a
distinct, time-bounded meaning.

## Locked product decisions

### Repo Mode is the first product mode

The first commercially useful mode starts with repository access and does not
require runtime integration. It must support isolated workspaces, bounded code
changes, test and verification evidence, durable auditability, and normal
source-control delivery.

Connected Mode, a customer-side satellite, managed capabilities, and private
deployment remain later product modes. Do not make their enterprise security
requirements prerequisites for proving Repo Mode.

### Central-first implementation

Near-term development should optimize for achieving useful autonomy with low
onboarding friction through the central PHarness deployment. The satellite is
expected to become important later, but is not a near-term prerequisite.

Security and data-boundary choices must be recorded as they are made so they
can mature without being mistaken for accidental guarantees. This direction
does not remove the current provenance, audit, isolation, explicit-effect, or
credential-handling safeguards; the minimum central-mode safety baseline still
needs an explicit decision.

### The WorkItem owns the engineering lifecycle

The canonical lifecycle is:

```text
Discover -> Plan -> Implement -> Test -> Verify -> Release -> Observe
```

Each WorkItem advances through the stages applicable to its intended outcome
and connected capabilities. A Product does not have one mutable lifecycle
stage: it may contain many concurrent WorkItems in different stages. Product
screens aggregate WorkItems and show deployed Release state separately.

Unavailable downstream stages are not failures. Repo Mode must make its
supported terminal boundary explicit rather than pretending Release and
Observe occurred without runtime connectivity.

### Capability availability, trust policy, and authorization are separate

Do not collapse these concepts:

1. **Capability availability** states whether a configured provider can
   perform an engineering operation against a particular target.
2. **Trust policy** states whether the Organization permits that class of
   operation under explicit scope and conditions.
3. **Authorization** is a time- and execution-bounded grant or human approval
   for an exact action at an exact lifecycle boundary.

A UI must not describe a capability as authorized merely because it is
configured, or describe an unavailable capability as a failed execution.

### Agent identity and orchestration are explicit

AgentProfile, AgentRun, and ExecutionStrategy are distinct concepts. Genuine
multi-agent coordination is part of the long-term vision, but it is a later
phase rather than a precondition for useful Repo Mode.

Full autonomy should not be claimed while the feedback loop is incomplete.
Connected runtime and operational evidence are expected to be important before
PHarness can safely act as an autonomous steward of a running Product.

### Dashboards are primarily read-oriented

Organization and Product dashboards exist for comprehension, triage, and
navigation. Exact external mutations belong in the owning WorkItem or Release
boundary, where PHarness can show the target, evidence, authorization,
external-effect summary, and current state.

The visual concept labeled **Autonomous Success Rate** is rejected. Any future
headline outcome metric must have a defensible denominator and expose human
intervention, verification failure, safety rejection, and rollback rather than
rewarding autonomy for its own sake.

## Evidence and agent handoff direction

PHarness already has a strong durable chain of events, artifacts, approvals,
provenance, test results, delivery evidence, and release verification. Raw
durability alone does not make that evidence usable by another agent.

The future execution model must answer how one stage closes, how its facts and
decisions are sealed, and how the next AgentRun receives a bounded, trustworthy
context without replaying an entire transcript or accepting an earlier agent's
interpretation as fact.

A working hypothesis for the next decision round is a controller-sealed stage
outcome containing:

- Objective and stage identity.
- Immutable input and product-model snapshot references.
- Facts and evidence references with provenance and freshness.
- Decisions made and authorizations consumed.
- Verified outputs and acceptance results.
- Unresolved risks, contradictions, and blocked capabilities.
- Recommended next action, clearly separated from verified fact.
- A content hash binding the handoff to the exact WorkItem state.

This is a hypothesis, not a locked contract. The deterministic controller must
remain the authority for lifecycle transitions and policy enforcement even if
a future control agent proposes assignments, replans, or coordination.

## Open decisions, in required order

Resolve these before writing a Repo Mode implementation milestone:

### Decision round 2: Repo Mode and evidence flow

1. Define the Repo Mode completion boundary: local verification, pull-request
   creation, pull-request merge observation, customer CI, or another outcome.
2. Decide whether a Product may register multiple Repositories immediately and
   whether one Repo Mode WorkItem may mutate more than one Repository.
3. Define low-friction repository onboarding and the role of the committed
   `.pharness` contract versus a centrally stored discovered contract.
4. Define the authoritative stage-outcome and evidence-handoff contract.
5. Define whether a control agent is persistent per Product, ephemeral per
   WorkItem, or deferred behind deterministic orchestration.
6. State the minimum central-mode security and data-handling baseline without
   designing the future satellite prematurely.

### Decision round 3: Operator experience

1. Select the first primary user persona and organization roles.
2. Define the Organization, Product, Repository, WorkItem, AgentRun, and Release
   navigation model.
3. Define the safe purpose of **Ask PHarness**.
4. Define honest portfolio and Product health measures.
5. Reconcile WorkItem lifecycle, Release state, approvals, and agent activity
   so each has one canonical source of truth.

### Implementation planning boundary

Only after the preceding decisions are recorded should plan mode create the
first active Repo Mode milestone. That milestone must identify behavior that
already exists, behavior that needs adaptation, and genuinely new capability;
it must not rewrite the proven reference stack merely to introduce provider
abstractions.

## Plan-mode reading order

For product-level planning, read in this order:

1. [`../README.md`](../README.md) for documentation lifecycle and authority.
2. This document for the product hierarchy, locked decisions, and open
   boundaries.
3. [`trusted-autonomy.md`](trusted-autonomy.md) for the existing trust model.
4. [`../architecture/README.md`](../architecture/README.md) for current system
   structure and accepted ADRs.
5. [`../active/README.md`](../active/README.md) to determine whether an approved
   implementation milestone exists.

Do not convert an open decision in this document into an implementation
assumption. Return to product discovery instead.
