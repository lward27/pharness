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

A controller-sealed stage outcome will contain:

- Objective and stage identity.
- Immutable input and product-model snapshot references.
- Facts and evidence references with provenance and freshness.
- Decisions made and authorizations consumed.
- Verified outputs and acceptance results.
- Unresolved risks, contradictions, and blocked capabilities.
- Recommended next action, clearly separated from verified fact.
- A content hash binding the handoff to the exact WorkItem state.

The deterministic controller remains the authority for lifecycle transitions
and policy enforcement even if a future control agent proposes assignments,
replans, or coordination. The detailed direction and remaining questions are
recorded in
[`stage-outcomes-and-evidence-handoffs.md`](stage-outcomes-and-evidence-handoffs.md).

## Repo Mode operating decisions

Repo Mode ends with a verified source pull request and an external wait. The
WorkItem closes only after PHarness observes the manual merge and records
immutable merge provenance. Release and Observe remain unavailable without a
connected runtime.

A Product may register multiple Repositories, but an initial Repo Mode WorkItem
may mutate only one. Other registered, pinned Repositories may provide
read-only context. Repository onboarding occurs through a dedicated PHarness
pull request containing the reviewed `.pharness` contract; the Repository
becomes ready only after PHarness observes its merge and validates the contract
at that revision.

Future multi-Repository mutation requires an explicit delivery dependency
graph that surfaces merge and release order to the operator. The complete
boundary and remaining onboarding questions are recorded in
[`repo-mode-operating-model.md`](repo-mode-operating-model.md).

## Central Repo Mode safety decision

Central-first Repo Mode retains immutable provenance, disposable isolated
workspaces, reviewed writable roots, declared acceptance, explicit source
mutation authorization, credential exclusion from model context, bounded
network/provider access, durable audit, and manual pull-request merge. Future
satellite, air-gap, and enterprise data-residency work is deferred.

## Open decisions, in required order

Resolve these before writing a Repo Mode implementation milestone:

### Decision round 2: Repo Mode and evidence flow - resolved 2026-08-24

The decisions are summarized above and specified in the two supporting design
documents. Their explicitly open contract questions remain prerequisites for
implementation planning.

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
3. [`repo-mode-operating-model.md`](repo-mode-operating-model.md) for the first
   product mode and source-delivery boundary.
4. [`stage-outcomes-and-evidence-handoffs.md`](stage-outcomes-and-evidence-handoffs.md)
   for evidence rollups, context, and future agent coordination.
5. [`trusted-autonomy.md`](trusted-autonomy.md) for the existing trust model.
6. [`../architecture/README.md`](../architecture/README.md) for current system
   structure and accepted ADRs.
7. [`../active/README.md`](../active/README.md) to determine whether an approved
   implementation milestone exists.

Do not convert an open decision in this document into an implementation
assumption. Return to product discovery instead.
