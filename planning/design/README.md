# Living design

This directory is for durable design intent that continues to constrain new
work. It is not a backlog and does not prove implementation.

## Approved Plan Mode entry points

Create these plans one at a time and keep their ownership separate:

1. [`repo-mode-v1-product-contract.md`](repo-mode-v1-product-contract.md) — plan
   first; owns Product and control-plane semantics, resources, APIs, onboarding,
   evidence handoff, and source delivery.
2. [`repo-mode-v1-screen-contract.md`](repo-mode-v1-screen-contract.md) — plan
   second; consumes the Product Plan and owns navigation, presentation,
   interactions, responsive behavior, and UI acceptance.

The entry points are approved design authority, not active implementation
milestones. Put a reviewed plan under [`../active/`](../active/README.md) before
implementation begins. Do not merge the two planning tasks or let the Screen
Plan redefine backend semantics.

## Current principles

- [`product-vision-and-boundaries.md`](product-vision-and-boundaries.md) is the
  upstream product-definition entry point. Read it before creating a new
  implementation milestone; its open decisions are not implementation
  authorization.
- [`product-model.md`](product-model.md) defines the ownership hierarchy,
  relational bindings, WorkItem identity boundary, execution identity, and
  systems of record.
- [`repo-mode-operating-model.md`](repo-mode-operating-model.md) defines the
  first product mode, onboarding PR, source-delivery boundary, and future
  merge-order direction.
- [`repository-onboarding-and-readiness.md`](repository-onboarding-and-readiness.md)
  defines deterministic discovery, agent-assisted proposals, Git-owned
  execution contracts, amendments, and derived readiness.
- [`stage-outcomes-and-evidence-handoffs.md`](stage-outcomes-and-evidence-handoffs.md)
  defines WorkItem-scoped evidence rollups, controller sealing, next-agent
  context, and the initial control model.
- [`operator-information-architecture.md`](operator-information-architecture.md)
  defines the primary persona, view ownership, current/history separation,
  approval routing, and the safe initial purpose of Ask PHarness.
- [`repo-mode-v1-product-contract.md`](repo-mode-v1-product-contract.md) and
  [`repo-mode-v1-screen-contract.md`](repo-mode-v1-screen-contract.md) are the
  approved, separately sequenced V1 planning contracts.
- [`trusted-autonomy.md`](trusted-autonomy.md) defines the trust-envelope and
  supervised-autonomy principles.
- [`../../ui/AGENTS.md`](../../ui/AGENTS.md) defines current operator-console
  hierarchy, honesty, navigation, and mutation-surface rules.
- [`../../ui/design-qa.md`](../../ui/design-qa.md) describes the code-adjacent
  UI comparison assets.
- [`../../crates/pharness-runhost/src/prompt.rs`](../../crates/pharness-runhost/src/prompt.rs)
  is the actual runtime prompt source. The original planning prompt is retained
  under [`../archive/foundations/`](../archive/foundations/) for history only.

The implemented control-plane and UI designs are indexed in
[`../implemented/README.md`](../implemented/README.md). Superseded V1/V2 UI
concepts are under [`../archive/ui/`](../archive/ui/) and must not be used as
current visual direction.

## Plan-mode sequence

For product-level work, read the product vision, product model, Repo Mode
operating model, repository onboarding and readiness, stage-outcome design,
operator information architecture, and trusted autonomy. Then use exactly one
approved V1 entry point, inspect current architecture and implementation, and
finally check the active-milestone index. The Product Plan precedes the Screen
Plan.
