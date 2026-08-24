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
    -> Services
    -> Environments
    -> RepositoryBindings -> Repositories
    -> WorkItems
    -> Releases
```

This is an ownership and navigation hierarchy, not a strict containment tree.
Service and Repository relationships are many-to-many through explicit
bindings.

| Term | Meaning |
| --- | --- |
| Organization | Administrative, identity, policy, billing, and fleet boundary |
| Product | The durable software system PHarness reasons about and improves |
| Service | A deployable or operationally meaningful component of a Product |
| Repository | A source-control system of record associated with one or more Services |
| RepositoryBinding | Explicit relationship among a Product, Repository, and optional Services |
| Environment | A named runtime or delivery target such as development, staging, or production |
| WorkItem | One bounded engineering intent owned by a Product and targeting explicit Services, Repositories, and Environments |
| StageExecution | One execution identity for an applicable WorkItem lifecycle stage |
| StageOutcome | Immutable controller-sealed conclusion for one StageExecution |
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
pull request containing the reviewed `.pharness/repository.yaml` contract; the
Repository becomes ready only after PHarness observes its merge and validates
the contract at that revision.

Future multi-Repository mutation requires an explicit delivery dependency
graph that surfaces merge and release order to the operator. The complete
boundary and remaining onboarding questions are recorded in
[`repo-mode-operating-model.md`](repo-mode-operating-model.md).

The relational ownership model, WorkItem identity boundary, and execution
identity rules are recorded in [`product-model.md`](product-model.md).

## Central Repo Mode safety decision

Central-first Repo Mode retains immutable provenance, disposable isolated
workspaces, reviewed writable roots, declared acceptance, explicit source
mutation authorization, credential exclusion from model context, bounded
network/provider access, durable audit, and manual pull-request merge. Future
satellite, air-gap, and enterprise data-residency work is deferred.

## Decision record and planning boundary

Resolve these before writing a Repo Mode implementation milestone:

### Decision round 2: Repo Mode and evidence flow - resolved 2026-08-24

The decisions are summarized above and specified in the two supporting design
documents. Their explicitly open contract questions remain prerequisites for
implementation planning.

### Decision round 3: Product identity and operator entry - resolved 2026-08-24

The engineering/platform lead is the first primary persona. Product ownership
supplies the main navigation hierarchy while explicit many-to-many bindings
preserve real Service and Repository relationships. One WorkItem owns one
engineering intent, with new StageExecutions for full replans and new linked
WorkItems for materially changed outcomes.

Organization and Product dashboards remain read-oriented. **Ask PHarness** may
answer read-only questions, explain state, and draft a WorkItem for review; it
never mutates a target directly. The detailed view hierarchy and its remaining
metric and interaction questions are recorded in
[`operator-information-architecture.md`](operator-information-architecture.md).

### Decision round 4: Onboarding, status, and navigation - resolved 2026-08-24

Repository onboarding begins with deterministic discovery, followed by an
agent-assisted proposal, operator review, one authorized onboarding pull
request, manual merge, and exact-revision validation. Executable Repository
configuration remains Git-owned; central annotations are display-only.
Autonomous coding requires immutable dependency input, deterministic
acceptance, a valid runner profile, and fresh preparation evidence.

Global navigation uses Overview, Products, Repositories, WorkItems, Agents,
Releases, Insights, and Settings. Provider-specific systems remain nested, and
the Environment selector filters read state without becoming a mutation
target. Product health remains dimensioned across work flow, release/runtime,
evidence freshness, and capability readiness. Global approval queues navigate
to exact owning actions; acknowledgment never grants authorization.

StageOutcome uses `succeeded`, `failed`, `blocked`, `cancelled`, and
`inapplicable` terminal statuses. Staleness and supersession remain explicit
properties or relationships. The detailed contracts are recorded in
[`repository-onboarding-and-readiness.md`](repository-onboarding-and-readiness.md),
[`stage-outcomes-and-evidence-handoffs.md`](stage-outcomes-and-evidence-handoffs.md),
and
[`operator-information-architecture.md`](operator-information-architecture.md).

### Decision round 5: Identity, checks, and evidence - resolved 2026-08-24

Product, Service, and RepositoryBinding use stable central identity. Repo Mode
requires a Product and Repository but does not manufacture a Service or
Environment. Bindings are reviewed and versioned, and existing WorkItems retain
their pinned Product-model snapshot.

Deterministic Repository discovery emits a versioned, content-hashed fact
inventory. The canonical contract begins at `pharness.dev/v1alpha1`; the
deprecated filename is read-only, conflicts block readiness, and removal waits
for complete migration plus a documented deprecation release.

Required provider checks bind to the exact pull-request head SHA. Head, check
set, or result changes invalidate readiness. A manual merge ends the external
wait, but source delivery seals as failed when required checks were not current
and passing.

Initial typed validators cover source, contract, environment preparation,
diff, acceptance, pull-request head, provider checks, and merge provenance.
Next-stage context uses effective outcomes and typed evidence retrieval rather
than raw transcript replay. Operator corrections are append-only annotations
that may trigger new controller decisions but never rewrite sealed history.

### Decision round 6: Repo Mode V1 screens and plan split - resolved 2026-08-24

The first screen milestone covers Organization Overview, Product detail,
Repository onboarding and readiness, WorkItem creation and detail, and active
AgentRun drill-down. WorkItem Overview owns the current intent and action;
Current Stage, StageOutcomes, Delivery, Evidence, and History remain distinct
sections beneath it.

StageOutcome cards distinguish verified facts, outputs, acceptance, agent
claims, contradictions, risks, freshness, and provenance. The supplied visual
concepts remain the dark, dense Product-oriented north star, but the UI uses
progressive disclosure and only API-backed state.

Planning is intentionally split into two independent entry points. The Product
Plan is created first and owns control-plane semantics and API contracts. The
Screen Plan is created second and owns operator presentation and interaction;
it records missing API fields as dependencies rather than redefining backend
behavior.

### Implementation planning boundary

The foundational Repo Mode and initial screen decisions are recorded. Plan Mode
may now use the approved entry points in this exact order:

1. [`repo-mode-v1-product-contract.md`](repo-mode-v1-product-contract.md)
2. [`repo-mode-v1-screen-contract.md`](repo-mode-v1-screen-contract.md)

Create the plans one at a time and keep them separate. Connected Mode topology,
Product graph, advanced AgentProfile management, and multi-Repository
DeliveryPlan do not block Repo Mode V1.

The milestone must identify behavior that already exists, behavior that needs
adaptation, and genuinely new capability. It must not rewrite the proven
reference stack merely to introduce provider abstractions.

## Plan-mode reading order

For product-level planning, read in this order:

1. [`../README.md`](../README.md) for documentation lifecycle and authority.
2. This document for the product hierarchy, locked decisions, and open
   boundaries.
3. [`product-model.md`](product-model.md) for ownership, relationships,
   WorkItem identity, and execution identity.
4. [`repo-mode-operating-model.md`](repo-mode-operating-model.md) for the first
   product mode and source-delivery boundary.
5. [`repository-onboarding-and-readiness.md`](repository-onboarding-and-readiness.md)
   for discovery, contract authority, onboarding, amendment, and readiness.
6. [`stage-outcomes-and-evidence-handoffs.md`](stage-outcomes-and-evidence-handoffs.md)
   for evidence rollups, context, and future agent coordination.
7. [`operator-information-architecture.md`](operator-information-architecture.md)
   for view ownership, current/history separation, and operator interaction.
8. [`repo-mode-v1-product-contract.md`](repo-mode-v1-product-contract.md) for
   the first, control-plane-focused Plan Mode task.
9. [`repo-mode-v1-screen-contract.md`](repo-mode-v1-screen-contract.md) for the
   separate, second operator-UI Plan Mode task.
10. [`trusted-autonomy.md`](trusted-autonomy.md) for the existing trust model.
11. [`../architecture/README.md`](../architecture/README.md) for current system
   structure and accepted ADRs.
12. [`../active/README.md`](../active/README.md) to determine whether an approved
   implementation milestone exists.

Do not combine the two Plan Mode entry points into one unbounded milestone.
Neither entry point authorizes implementation until its resulting plan is
reviewed and approved under [`../active/`](../active/README.md).
