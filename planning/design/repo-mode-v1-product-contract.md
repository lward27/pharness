# PHarness Repo Mode V1 product contract

> Historical source-only contract. Retained for the meaning of shipped and in-flight
> legacy work. New direction is the [ASTRA program](../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md);
> do not start a competing Repo Mode implementation from this document.

Status: approved Plan Mode entry point

Approved: 2026-08-24

Planning order: **first**

Companion entry point:
[`repo-mode-v1-screen-contract.md`](repo-mode-v1-screen-contract.md)

## Purpose

This document is the authoritative entry point for planning the Repo Mode V1
product milestone. It consolidates the approved product decisions into one
bounded outcome without replacing the detailed upstream designs.

Use this document to produce an implementation plan, not to begin
implementation. The plan must characterize current PHarness behavior before it
proposes new resources, migrations, APIs, or controllers.

The separate screen-contract plan should be created only after this product
plan is complete. This plan owns product and control-plane semantics; it does
not own the broad operator-console redesign.

## Product promise

Starting from repository access alone, PHarness can onboard a Repository into
a Product, establish an immutable and reviewable execution contract, carry one
bounded engineering intent through verified code change and source pull
request, observe its manual merge, and close the WorkItem with durable,
controller-sealed evidence.

Repo Mode does not claim deployment, runtime health, Release, or Observe
without connected runtime capabilities.

## End-to-end V1 journey

```mermaid
flowchart LR
    A["Create Product"] --> B["Register Repository"]
    B --> C["Deterministic discovery"]
    C --> D["Agent-assisted onboarding proposal"]
    D --> E["Operator review"]
    E --> F["Onboarding PR"]
    F --> G["Manual merge and validation"]
    G --> H["Repository coding ready"]
    H --> I["Create WorkItem"]
    I --> J["Plan and execute"]
    J --> K["Test and verify"]
    K --> L["Source PR"]
    L --> M["Required provider checks"]
    M --> N["Manual merge observation"]
    N --> O["Seal outcome and close WorkItem"]
```

Every external mutation and manual wait remains explicit. Safe internal
controller work may advance only within the existing action, trust, policy,
state-hash, and authorization boundaries.

## V1 product scope

### Product registration

- An operator explicitly creates or selects a Product.
- Product identity uses a PHarness-generated stable opaque identifier.
- Initial fields are Organization-unique display name, description, and owner.
- Repo Mode requires at least one registered Repository before work begins.
- Service and Environment are not required merely to obtain repository-only
  value.

### Repository and Service model

- A Repository is registered at an immutable source revision.
- A Service has a stable central identifier and initially belongs to one
  Product.
- Services are optional in Repo Mode and may be proposed during onboarding.
- RepositoryBindings are explicit, versioned, reviewed Product-model
  relationships.
- Shared Repositories use explicit bindings for every Product that uses them.
- Product-model snapshots are pinned to WorkItems and never change
  retroactively.
- Registration or binding never implies reader, writer, or mutation
  authorization.

### Repository onboarding

- Discovery is deterministic, isolated, read-only, versioned, and
  content-hashed.
- The discovery inventory contains facts and conflicts, not semantic choices.
- An AgentRun may propose Services, RepositoryBindings, the Repository
  contract, and bounded guidance while labeling assumptions.
- An operator reviews the proposal and exact diff before authorizing one
  onboarding pull request.
- A Repository owner merges manually.
- PHarness observes the merge and validates the contract at the exact merged
  revision before deriving readiness.

### Git-owned execution contract

- The canonical path is `.pharness/repository.yaml`.
- The initial API version is `pharness.dev/v1alpha1` with strict validation.
- `.pharness/project.yaml` is a deprecated, read-only compatibility alias.
- New and amended contracts use only the canonical path.
- Conflicting files block readiness.
- Alias removal requires complete registered-Repository migration and a
  documented deprecation release.
- Commands, dependency inputs, profiles, writable roots, network and package
  policy, and acceptance remain Git-owned executable configuration.
- Central annotations may supply display-only metadata and never override
  executable behavior.

### Derived readiness

Contract readiness requires observed merge provenance, a valid canonical
contract at the exact revision, an active EnvironmentProfile, immutable
dependency input, deterministic acceptance commands, and bounded path policy.

Coding readiness additionally requires fresh proof of exact source checkout,
environment preparation, required executables, isolated workspace creation,
and typed acceptance-command execution.

Reader, writer, observer, and provider capabilities remain independently
reported. Capability availability is not trust policy, and neither is an
authorization grant.

### WorkItem identity

- One WorkItem owns one bounded engineering intent.
- The mutable Repository, pinned source revision, desired outcome, and required
  acceptance boundary define its identity.
- Planning, model, budget, or tool changes may remain within the same WorkItem.
- A materially changed outcome, mutable Repository, delivery target, or
  acceptance boundary creates a linked WorkItem.
- Relationships such as `supersedes`, `follows_up`, and `discovered_from`
  preserve continuity without rewriting intent.

### StageExecution and StageOutcome

- Transport retries, recoverable tool retries, approval resumes, and budget
  resumes remain within one StageExecution.
- A full replan or deliberate stage repeat creates a new StageExecution.
- Every terminal execution produces an immutable, controller-sealed
  StageOutcome.
- Terminal statuses are `succeeded`, `failed`, `blocked`, `cancelled`, and
  `inapplicable`.
- Waiting and recoverable states are not terminal.
- Staleness is an evidence property; supersession is a relationship.
- The controller selects one effective outcome per applicable stage while all
  prior history remains inspectable.

### Evidence and next-stage context

Initial typed validators cover:

- Source revision and checkout identity.
- Repository contract version, hash, and validation.
- EnvironmentSnapshot and preparation result.
- Changed paths and full diff hash.
- Exact declared acceptance command and result.
- Source pull request and head SHA.
- Required provider-check set, results, freshness, and head binding.
- Observed merge and immutable provider provenance.

The next AgentRun receives current intent, pinned Product and Repository
context, effective upstream outcomes, budgets, policy, grants, contradictions,
risks, operator decisions, and exact evidence references. Raw transcript replay
is excluded by default; typed evidence retrieval is audited.

Operator corrections are append-only annotations. They may add context,
invalidate freshness, or trigger replan and later sealing, but never modify a
sealed outcome or external observation.

### Source delivery and closure

- Initial Repo Mode mutates exactly one Repository per WorkItem.
- Other registered and pinned Repositories may provide read-only context.
- Local declared acceptance is required.
- Configured provider checks bind to the exact pull-request head SHA.
- Any head, required-check-set, or result change invalidates pull-request
  readiness.
- Pull-request merge remains manual.
- The WorkItem closes only after PHarness observes the merge and records
  immutable provenance.
- If a manual merge occurs without current passing required checks, the
  external wait ends but source delivery seals as failed.
- Source merge is not a Release. Release and Observe are inapplicable in Repo
  Mode.

## Capability, trust, and authorization boundary

The implementation must continue to model these separately:

1. Capability availability: whether an operation can be performed against an
   exact target.
2. Trust policy: whether the Organization permits that operation under stated
   conditions.
3. Authorization: a time-, state-, action-, and target-bound approval or grant.

Repository discovery has no writer credential. Onboarding PR authorization
does not authorize later WorkItem mutation. Coding workspace authorization
does not authorize commit, push, pull request, provider, GitOps, deployment, or
rollback effects.

## Compatibility requirements

- Existing WorkItems and Runs remain readable.
- Existing coding-loop reliability, resumable budgets, environment preparation,
  approval, immutable provenance, and source-delivery behavior are preserved.
- The deprecated contract alias remains readable during the documented
  migration window.
- Current reference providers continue to work while provider-specific nouns
  remain below product-level contracts.
- New persistence is nullable or migratable without rewriting historical
  evidence.
- Current external-effect confirmation and state-hash protections remain in
  force.

## Explicit V1 exclusions

- Connected runtime onboarding and customer-side satellite.
- Deployment, Release, Observe, and runtime health claims.
- Multi-Repository mutation and DeliveryPlan.
- Product Graph editing.
- Persistent autonomous Product Steward.
- Generic multi-agent message bus or autonomous swarm.
- Broad AgentProfile management.
- Automatic source merge, deployment, retry of whole attempts, or rollback.
- The operator-console visual redesign governed by the companion screen
  contract.

## Product acceptance contract

Repo Mode V1 is product-complete only when a deterministic test and one
supervised live Repository prove this sequence:

1. Create or select a Product and register a Repository at an immutable
   revision.
2. Produce a versioned deterministic discovery inventory without writer
   capability.
3. Produce and review an agent-assisted onboarding proposal.
4. Authorize one onboarding pull request, observe manual merge, and validate
   the canonical contract at the merge revision.
5. Derive contract and coding readiness from durable evidence.
6. Create one WorkItem against the ready Repository and pinned Product-model
   snapshot.
7. Prepare the environment before model turn zero.
8. Plan, execute, test, and verify one bounded code change.
9. Seal applicable StageOutcomes and construct the next-stage context from
   effective outcomes rather than transcript replay.
10. Authorize one source pull request and bind required checks to its exact
    head.
11. Observe manual merge and close the WorkItem with immutable delivery
    provenance.
12. Demonstrate honest failed delivery when a merged PR lacks current passing
    required checks.
13. Preserve prior executions, annotations, evidence, and compatibility state.

A healthy API, passing unit tests, or an isolated UI fixture is not sufficient
proof of this product contract.

## Required Plan Mode reading order

Read these sources before inspecting implementation details:

1. [`product-vision-and-boundaries.md`](product-vision-and-boundaries.md)
2. [`product-model.md`](product-model.md)
3. [`repo-mode-operating-model.md`](repo-mode-operating-model.md)
4. [`repository-onboarding-and-readiness.md`](repository-onboarding-and-readiness.md)
5. [`stage-outcomes-and-evidence-handoffs.md`](stage-outcomes-and-evidence-handoffs.md)
6. [`trusted-autonomy.md`](trusted-autonomy.md)
7. [`../architecture/README.md`](../architecture/README.md)
8. [`../implemented/README.md`](../implemented/README.md)
9. [`../active/README.md`](../active/README.md)

Then inspect the current code, tests, schemas, migrations, API models,
controllers, UI data contracts, fixtures, Helm configuration, and release
workflow at the exact Git revision.

## Product Plan Mode output requirements

Create one product implementation plan that:

1. Begins with a current-state map of what exists, what needs adaptation, and
   what is genuinely new.
2. Names exact existing resources and APIs that remain authoritative.
3. Defines compatibility and migration boundaries before new persistence.
4. Splits work into independently verifiable vertical slices.
5. Includes deterministic backend, controller, migration, provider, and
   end-to-end acceptance tests.
6. Preserves coding-loop reliability and all external-effect controls.
7. Includes release, rollback, observability, and evidence-retention gates.
8. Ends with the complete supervised Repo Mode acceptance journey.
9. Excludes broad operator visual redesign and instead lists the API/read-model
   dependencies the separate screen plan will consume.
10. Creates no implementation changes while still in Plan Mode.

If current behavior conflicts with this contract, call out the conflict and
propose a migration. Do not silently reinterpret the product decision from the
existing implementation.
