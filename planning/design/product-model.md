# PHarness product model

Status: living design

Last decision round: 2026-09-04

Upstream authority:
[`product-vision-and-boundaries.md`](product-vision-and-boundaries.md)

## Current direction and compatibility

The [ASTRA program](../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md) owns new hosted
workflow implementation. The entities below describe semantics, not a promise that
every listed concept has a database table or is exposed by the current UI. Source-only
references describe legacy records; they do not authorize new source-only product modes.

## Purpose

This document defines the durable product entities that PHarness uses to
organize software, engineering intent, execution, evidence, and releases. It
is a semantic model, not a proposed database schema or implementation
milestone.

The model must make the operator experience understandable without forcing
real software systems into a false tree. Product ownership supplies the main
navigation hierarchy; explicit bindings preserve shared repositories,
monorepos, and cross-Product dependencies.

## Canonical ownership hierarchy

```text
Organization
  -> Product
    -> Services
    -> Environments
    -> RepositoryBindings -> Repositories
    -> WorkItems
    -> Releases
```

The hierarchy defines primary ownership and navigation. It does not imply
that every entity is physically contained by one parent or that every
relationship is one-to-many.

| Entity | Product meaning |
| --- | --- |
| Organization | Administrative, identity, policy, billing, and fleet boundary |
| Product | Durable software system PHarness understands and improves |
| Service | Deployable or operationally meaningful component of a Product |
| Repository | Source-control system of record registered with PHarness |
| RepositoryBinding | Explicit relationship between a Product, Repository, and optional Services |
| Environment | Named runtime or delivery target owned by a Product |
| WorkItem | One bounded engineering intent and its complete lifecycle record |
| StageExecution | One execution identity for an applicable WorkItem lifecycle stage |
| StageOutcome | Immutable, controller-sealed conclusion for one StageExecution |
| Release | Immutable promoted outcome for one Product and Environment |
| AgentProfile | Versioned role definition including model, prompt, skills, and capability policy |
| AgentRun | One execution of an AgentProfile for a bounded stage objective |
| ExecutionStrategy | Orchestration shape for AgentRuns: single, sequential chain, or parallel swarm |
| DeliveryPlan | Future dependency graph coordinating ordered changes and promotions |

Use **Product**, not **Project**, throughout the product and operator
experience. A future Project resource requires a separate, time-bounded
meaning; it must not become an alias for Product.

## Product, Service, and Repository relationships

A Product owns its Services, Environments, WorkItems, and Releases. It
registers Repositories through explicit RepositoryBindings.

RepositoryBindings are many-to-many:

- A Product may bind multiple Repositories.
- One Repository may implement multiple Services.
- One Service may span multiple Repositories.
- A shared Repository may bind to multiple Products only through explicit
  bindings in each Product.
- Monorepos use scoped bindings rather than duplicating the Repository.
- Cross-Product dependencies remain explicit relationships, not implicit
  access inherited from Organization membership.

The initial central product model owns Product-to-Service and
Product-to-Repository mappings. The committed Repository contract owns the
portable execution facts that must travel with source. A future discovery
process may propose mappings, but it does not silently make them authoritative.

Product structure used during execution must be snapshotted and bound to the
WorkItem or StageExecution. Later edits to a Service or RepositoryBinding must
not retroactively change the meaning of prior evidence.

## Product and Repository registration

An operator explicitly creates or selects the Product before registering a
Repository. PHarness does not silently create Product identity from a
repository name or agent inference.

The initial Product registration contains:

- A PHarness-generated stable opaque identifier.
- An Organization-unique display name.
- A human-readable description.
- An accountable owner.

Legacy source-only WorkItems required a registered Repository without a hosting target.
New hosted WorkItems require a validated Repository plus versioned native delivery and
verification bindings. Missing readiness blocks creation/progression; it does not silently
fall back to source-only completion.

Deterministic Repository discovery records source facts. An AgentRun may then
propose Services and RepositoryBindings for operator review. Approved mappings
become central Product-model state and retain the discovery and approval
evidence that produced them. The committed Repository contract separately owns
portable execution configuration.

The detailed onboarding boundary is recorded in
[`repository-onboarding-and-readiness.md`](repository-onboarding-and-readiness.md).

## Service and RepositoryBinding identity

A Service has a PHarness-generated stable opaque identifier and initially
belongs to exactly one Product. Services are optional in Repo Mode; discovery
may propose them when the Repository contains a meaningful component boundary.
A shared Repository does not create a shared Service implicitly.

RepositoryBindings are versioned, reviewed relationships. A binding records
the exact Product, Repository, optional Services, and any repository-relative
scope needed to explain the relationship. Shared Repositories receive explicit
bindings in every Product that uses them.

Changing a Service or RepositoryBinding creates a new Product-model version.
Already-pinned WorkItems retain their original snapshot. A binding describes
product structure and context eligibility; it does not grant reader, writer,
or mutation authorization.

## WorkItem identity and revision boundary

A WorkItem is the correlation root for exactly one engineering intent. It
owns applicable lifecycle stages, plans, executions, approvals, evidence,
source delivery, and any connected release work for that intent.

A changed plan remains in the same WorkItem when the desired outcome, mutable
Repository, and required acceptance criteria remain materially the same. The
new plan produces new stage execution history and does not rewrite the
previous attempt.

A new, explicitly linked WorkItem is required when the operator materially
changes any of these:

- Desired engineering outcome.
- Mutable Repository or delivery target.
- Required acceptance boundary.
- Product scope in a way that creates a distinct deliverable.

The new WorkItem should record a typed relationship such as `supersedes`,
`follows_up`, or `discovered_from`. This preserves why the work changed without
turning one WorkItem into an indefinitely mutable container.

Plan detail, model choice, AgentProfile, execution budget, recovery strategy,
or tool selection may change within the same WorkItem when its intent boundary
does not change.

## Stage execution and retry identity

StageExecution separates current work from prior attempts:

- Provider transport retries remain within the same StageExecution.
- Recoverable tool retries remain within the same StageExecution.
- Resuming after an approval wait or budget extension remains within the same
  StageExecution.
- A full replan or deliberate repeat of a lifecycle stage creates a new
  StageExecution.
- Every terminal StageExecution produces an immutable StageOutcome.
- The WorkItem points to one effective outcome per applicable stage while all
  previous executions and outcomes remain under History.

The effective pointer is a controller decision, not a mutation of the sealed
outcome. Supersession and staleness are relationships between outcomes, not
ways to rewrite them.

## Application mutation boundary

The Product may supply context from several registered Repositories, but an
hosted WorkItem mutates exactly one application Repository. The mutable
Repository and pinned source revision are immutable WorkItem inputs. Other
registered Repositories may be pinned as read-only context.

Separately authorized GitOps updates are delivery effects, not permission to edit a
second application Repository. Changing the mutable Repository creates a new linked
WorkItem. Future
multi-Repository mutation requires a DeliveryPlan that makes merge,
compatibility, promotion, failure, and recovery order explicit.

## Releases and environments

A Release is an immutable promoted outcome for one Product and Environment.
It binds the promoted source and artifact provenance, applicable WorkItems,
deployment evidence, verification result, and rollback relationship.

A source pull request or merge is not a Release. Legacy source-only WorkItems retain
their observed-merge completion and unavailable Release. New hosted WorkItems remain
nonterminal until the approved target has deployed and runtime acceptance is verified.
Production GitOps merge requires human approval bound to the exact artifact and evidence.
Successful rollback records recovered service while the requested WorkItem remains failed.

Product dashboards may summarize currently deployed Releases, but release
state never replaces WorkItem lifecycle state.

## Capability, trust, and authorization scope

Capability availability, trust policy, and authorization remain separate and
attach to exact modeled targets:

- Capability availability names the Product, Service, Repository, or
  Environment operation that a configured provider can perform.
- Trust policy names the permitted operation, scope, and conditions.
- Authorization binds a time- and state-bounded action to the exact WorkItem,
  target, and evidence boundary.

Organization membership or Repository registration alone never implies
authorization to mutate a target.

## Systems of record

PHarness should make source-of-truth boundaries visible:

| Concern | System of record |
| --- | --- |
| Portable Repository execution contract | Committed `.pharness/repository.yaml` at an immutable revision |
| Product, Service, Environment, and RepositoryBinding relationships | PHarness control plane initially |
| WorkItem intent, stage state, approvals, and sealed outcomes | PHarness control plane |
| Source commits, pull requests, checks, and merges | Source provider, observed and snapshotted by PHarness |
| Deployment and runtime state | Connected delivery/runtime providers, observed and snapshotted by PHarness |
| Agent event and artifact history | PHarness durable evidence store |

An observed external fact is stored as evidence with provenance and freshness;
the PHarness copy does not become the external provider's new authority.

## Future DeliveryPlan

DeliveryPlan is reserved for coordinated work whose safe order cannot be
represented by one WorkItem. It will eventually describe dependencies among
WorkItems, source merges, builds, Releases, environment promotions, and
verification steps.

It is not required for initial Repo Mode and must not be implemented as an
informal list. Its future contract needs dependency validation,
partial-completion semantics, compatibility evidence, operator overrides, and
recovery ownership.

## Implementation mapping

M05 maps the hosted contract onto existing WorkItem/stage/effect resources before adding
persistence. Preserve compatible readers and additive migrations. Environment promotion
and observation for the two Finance applications are in scope now; generic topology,
DeliveryPlan, and cross-Product mutation remain deferred. Historical in-flight work keeps
its pinned contract and all prior outcomes remain immutable.
