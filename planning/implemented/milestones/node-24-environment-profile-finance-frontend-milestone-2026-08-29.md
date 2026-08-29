# PHarness Node 24 Environment Profile and Finance Frontend Source Smoke

Status: implemented and accepted on 2026-08-29

Implementation baseline: `2b4c59ff0273942bc0b06f04764c61744061d2a7`

Accepted compiled source: `4bb2d96b650687021e04b51c4c8f7ada07c5b59a`

Accepted release pin: `39521a27a917aca79e5a097e09f6e3edb2f4b7de`

Completion evidence: [Node 24 finance-frontend source smoke](../../evidence/smoke-results/node-24-finance-frontend-source-smoke-2026-08-29.md)

## Objective

Add a first-class, immutable `node-24` EnvironmentProfile backed by
deterministic `npm ci` preparation, then prove it through finance-frontend
onboarding, coding readiness, and one supervised Repo Mode source-delivery
WorkItem.

## Locked boundaries

- Preserve all Python behavior and historical Runs.
- Support npm `package-lock.json` versions 2 and 3 only.
- Deny npm lifecycle scripts, mutable dependencies, workspaces, local links,
  unapproved registries, coding-phase package installation, and
  agent-initiated network access.
- Require a real source root and test root discovered at the pinned commit.
- Keep the incompatible frontend onboarding proposal as immutable history;
  create a fresh onboarding at the prerequisite merge SHA.
- Mutate only `finance-frontend` during the acceptance WorkItem. Source
  delivery ends at an observed manual merge; Release and Observe remain
  inapplicable.

## Implemented behavior

1. Generalized RepositoryContract dependency-lock and EnvironmentSnapshot
   runtime fields without breaking historical Python records.
2. Added strict npm-lock validation and profile/lock compatibility enforcement
   across onboarding, readiness, and WorkItem boundaries.
3. Added deterministic Node preparation and offline acceptance execution with
   lifecycle scripts denied.
4. Added the digest-pinned `node-24` runner profile, image, service account,
   proxy policy, local and Tekton build targets, and four-image release
   pinning.
5. Hardened onboarding context/actions and operator error handling for
   incompatible proposals and stale or blocked actions.
6. Landed the finance-frontend Node test prerequisite and canonical PHarness
   RepositoryContract.
7. Fixed correction preparation, executable-mode preservation, Tester
   workspace copying, and zero-turn follow-up-stage startup recovery exposed by
   the supervised smoke.
8. Completed Planner, Builder, Tester, Verifier, exact ChangeSet delivery,
   provider observation, manual merge, WorkItem closure, and a 90-day evidence
   hold.

## Acceptance boundary

- Node/npm runtime and immutable image provenance are visible through
  readiness without Python-specific placeholders.
- `npm ci --ignore-scripts --no-audit --no-fund` runs only during preparation
  and cannot create a lifecycle-script marker.
- Python and Node profile preflights pass against exact allowlisted
  repositories.
- The finance-frontend contract selects `node-24`, pins the exact lock hash,
  declares only existing roots and approved acceptance commands, and reaches
  coding-ready.
- The supervised WorkItem closes only after exact source provenance,
  acceptance evidence, provider checks, and manual merge are observed.
- No Pipeline, GitOps, Argo, deployment, or runtime-health action is performed
  for the frontend WorkItem.

## Residual follow-ups

- Add a server-derived, state-hashed `cancel_work_item` action so the Repo Mode
  UI can cancel nonterminal WorkItems without using the compatibility cancel
  endpoint directly.
- Continue the Finance reliability campaign before adding another runner or
  broadening into Connected Mode.
