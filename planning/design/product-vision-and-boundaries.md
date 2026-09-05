# PHarness product vision and boundaries

Status: living direction, approved 2026-09-04.
Implementation authority: [ASTRA program](../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md).
Current implementation: [baseline addendum](../evidence/assessments/ASTRA-CURRENT-BASELINE-ADDENDUM.md).

## Product promise

PHarness embeds a bounded autonomous SDLC in the environment that hosts a Product.
An operator requests a change, supervises exceptions, approves production, and receives
traceable evidence that the running result satisfies the request. The initial target
is `lucas_engineering`: Tekton, the registry, GitOps/Argo, and LGTM.

The existing source-only workflow is useful implementation and history. It is not
the long-term separate product mode. New work will use one hosted workflow after
M05/M06 implement its contract and M04 qualifies its coding path. Until then, do not
describe the complete hosted promise as an accepted capability.

## Operator model

Product is the software system. Repository supplies source. WorkItem owns one bounded
intent, current state, authority, attempts, delivery, and evidence. Release describes
a promoted artifact and observed environment. The operator should not need to learn
the entire persistence model before submitting or understanding work.

Lead with what change is requested, what is happening, why input is needed, what the
next action will do, and whether the result met acceptance. Technical evidence and
history remain accessible below that reading path.

## Locked boundaries

- A user request initiates work. Observability verifies and recovers that same work;
  autonomous incident campaigns are deferred.
- One application repository is mutable per WorkItem. Other source context is pinned
  and read-only. Separately authorized GitOps writes deliver that application's artifact.
- Discovery, planning, implementation, deterministic tests, code verification, source
  delivery, build, and staging progress within a recorded authorization and finite limits.
- Production approval precedes production GitOps merge because Argo auto-syncs. It binds
  exact digest, diff, staging evidence, target, and preceding healthy deployment.
- One compatible rollback is authorized by that approval. Unknown health is not a
  confirmed regression; incompatible or destructive recovery requires intervention.
- Source merge, deployed release, and verified runtime acceptance are different facts.
  Hosted success requires all applicable evidence. A recovered failed release remains
  failed work, with successful recovery recorded separately.
- Keep single-writer SQLite, current worker/effect isolation, the existing gateway, and
  bounded execution. A gateway configuration is qualified only after its frozen gates pass.

## Lifecycle and presentation

Canonical lifecycle: `discover -> plan -> implement -> test -> verify -> source_delivery
-> release -> observe`. Release exposes build, staging, and production steps. UI labels
must distinguish code verification and runtime verification.

The shipped Lamina console and selected `Pharness Console.dc.html` are the visual baseline.
Keep Overview, Products, Repositories, WorkItems, Agents, Releases, Insights, and Settings.
There is one operational path beneath a WorkItem, with explicit History for prior work.
Do not add customization controls, a competing rail/navigation model, or invented progress.

Tool approvals authorize effects; lifecycle gates adjudicate evidence/authority. Keep
that distinction visible. GET/navigation never dispatches work. The durable controller
progresses previously authorized work independently of browser activity.

## Configuration and evidence

Committed repository contracts own portable execution facts. Product/environment
bindings own exact native hosting targets. Snapshot both for work; later configuration
edits cannot change old evidence or widen authority. Capabilities, policy, and current
authorization remain distinct.

Runtime verification combines exact artifact/deployment identity, functional behavior,
and fresh application-scoped metrics/logs/traces as applicable. Missing data is inconclusive.
Preserve immutable outcome history, Finance data generation, and additive migration
compatibility. Source-only historical success never becomes a retroactive deployment claim.

## Deferred until the native program is accepted

Generic build/observability/deployment interfaces, new coding backends, cross-repository
DeliveryPlan orchestration, autonomous incident initiation, satellites, commercial tenancy,
workflow builders, and additional navigation are outside this program. Existing internal
boundaries should remain tidy, but do not design a universal adapter framework prematurely.

## Read next

[Product model](product-model.md), [stage outcomes](stage-outcomes-and-evidence-handoffs.md),
[onboarding/readiness](repository-onboarding-and-readiness.md),
[architecture](../architecture/README.md), and the
[numbered implementation program](../programs/autonomous-sdlc/ASTRA-00-PROGRAM.md).
The older Repo Mode contracts are legacy behavior references, not future-direction authority.
