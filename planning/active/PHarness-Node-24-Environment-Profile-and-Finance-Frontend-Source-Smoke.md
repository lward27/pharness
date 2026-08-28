# PHarness Node 24 Environment Profile and Finance Frontend Source Smoke

Status: active  
Approved: 2026-08-28  
Implementation baseline: `2b4c59ff0273942bc0b06f04764c61744061d2a7`

## Objective

Add a first-class, immutable `node-24` EnvironmentProfile backed by deterministic `npm ci` preparation, then prove it through finance-frontend onboarding, coding readiness, and one supervised Repo Mode source-delivery WorkItem.

## Locked boundaries

- Preserve all Python behavior and historical Runs.
- Support npm `package-lock.json` versions 2 and 3 only.
- Deny npm lifecycle scripts, mutable dependencies, workspaces, local links, unapproved registries, coding-phase package installation, and agent-initiated network access.
- Require a real source root and test root discovered at the pinned commit.
- Keep the current incompatible frontend onboarding proposal as immutable history; create a fresh onboarding at the prerequisite merge SHA.
- Mutate only `finance-frontend` during the acceptance WorkItem. Source delivery ends at an observed manual merge; Release and Observe remain inapplicable.

## Implementation sequence

1. Generalize RepositoryContract dependency-lock and EnvironmentSnapshot runtime fields.
2. Add strict npm-lock validation and profile/lock compatibility enforcement at every onboarding, readiness, and WorkItem boundary.
3. Add deterministic Node preparation and offline acceptance execution.
4. Add the digest-pinned `node-24` runner profile, image, service account, proxy policy, build target, and four-image release pinning.
5. Harden onboarding context/actions and operator error handling for incompatible proposals and stale or blocked actions.
6. Land and merge the finance-frontend Node test prerequisite.
7. Run deterministic Rust, UI, Helm, Kubernetes-rendering, image-platform, and runner-smoke gates.
8. Merge the implementation, build all four artifacts from one SHA, pin their digests in a separate release commit, and deploy through Argo.
9. Complete fresh frontend onboarding, coding readiness, Planner → Builder → Tester → Verifier, source PR, provider observation, manual merge, closure, and a 90-day evidence hold.

## Acceptance evidence

- Node/npm runtime and immutable image provenance are visible through readiness and Settings without Python-specific placeholders.
- `npm ci --ignore-scripts --no-audit --no-fund` runs only during preparation and cannot create a lifecycle-script marker.
- Both Python and Node profile preflights pass against exact allowlisted repositories.
- The finance-frontend contract selects `node-24`, pins the exact lock hash, declares only existing roots and approved acceptance commands, and reaches coding-ready.
- The supervised WorkItem closes only after exact source provenance, acceptance evidence, provider checks, and manual merge are observed.
- No Pipeline, GitOps, Argo, deployment, or runtime-health action is performed for the frontend smoke.
