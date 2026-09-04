# PHarness Lamina Operator Console Redesign

Status: source merged and deterministic acceptance complete; evaluator blocked by local AMD64 emulation

Approved: 2026-09-04. Re-characterized baseline:
`1770ad823a4970a8bc8a48d093f1cacc7c5bb001`.

Implementation and a live-data repository-label correction were merged in PRs
#319 and #320. The final source is `2d99156a410830aa0015995c779e6c3603fdab95`.
The redesign flag remains disabled. See the
[release progress record](../evidence/assessments/pharness-lamina-release-progress-2026-09-04.md)
for current tests, verified partial image builds, the chunked-upload workaround,
approved older-cache cleanup, and the local Rust/AMD64 emulation blocker. The earlier
[local acceptance and infrastructure hold](../evidence/assessments/pharness-lamina-local-acceptance-2026-09-04.md)
is retained as historical evidence.
The milestone is not operationally accepted until the seven-image release,
native bundle, disabled/enabled GitOps rollout, and live Finance checks pass.

## Summary and locked decisions

Rebuild the complete operator console around the supplied
`planning/pharness-ui-design-overhaul/Pharness Console.dc.html` visual reference.
Preserve that user-owned prototype unchanged; port its composition into React,
not its generated runtime. Existing Product/API/controller contracts remain
authoritative over fictional prototype data.

- Floating top navigation: Overview, Products, Repositories, WorkItems, Agents,
  Releases, Insights, Settings.
- Default WorkItem Overview: horizontal laminae activity lanes, not dots/orbit.
- Close visual fidelity: glass, background patterns, glows, typography, spacing,
  and restrained motion. Self-host fonts/assets. Keep dark default and saved
  light/dark preference; reduced-motion and readable contrast are mandatory.
- Complete console coverage; deepest interactions are onboarding, WorkItems,
  and active Runs. Phone navigation is a drawer with one content column.
- Develop, test, and build locally on the Mac, including Docker release builds
  explicitly targeting Linux/AMD64. No dependency on lucas-desktop returning.

## Implementation sequence

1. Capture baseline APIs/screenshots; add
   `features.repoModeV1.designOverhaulEnabled=false` and scoped visual primitives.
   The existing `uiEnabled` flag retains its meaning and current console remains
   the fallback.
2. Floating shell, typed hash routes, search, read-only Overview, Products.
3. Repository registration, onboarding, readiness, and reviewed topology.
4. WorkItem laminae, owning-resource actions, Current Stage, Stage Outcomes,
   source-only Delivery, Evidence, and History.
5. Agents, Releases, Insights, Settings, and contextual legacy compatibility.
6. Complete deterministic and browser acceptance before enabling the redesign.

Use Finance's actual multi-repository composition in acceptance fixtures.
Products retain current work, Services/scoped bindings, Agents, Releases,
evidence/audit, and History. Repository screens separate registration, contract
readiness, coding readiness, capabilities, trust, and authorization. Onboarding
is discovery → proposal → review → PR → manual merge → readiness with one next
action. Preserve real release/legacy delivery behavior without inventing health.

## Laminae, evidence, and API contracts

Render Discover, Plan, Implement, Test, Verify, Source Delivery on a common time
axis fitted to recorded WorkItem history. Repeated/repair executions have
separate labeled intervals and correction lineage. Current/effective work is
primary; historical/superseded records remain identifiable. Clicking an interval
opens a read-only inspector with execution, Run, outcome, timing, and evidence.
Controller outcomes may be instantaneous markers. Missing timestamps are
unavailable, never estimated. Elapsed intervals include pauses; recorded active
model time is separate. Release/Observe are explicitly inapplicable outside the
active timeline. Do not implement alternate prototype visualizations.

Extend only the existing Repo Mode flow read model with `lifecycle_timeline`:
observation timestamp; stage/execution/resource references; recorded start/end
or markers; timing basis; status/origin; effective/current flags; correction
linkage. Derive it from existing records, including the SourceDeliveryIntent
interval (the closing StageExecution alone hides PR wait). Do not include the
observation clock in action hashes. No database migrations or new mutations.

Presentation must use effective outcomes rather than stage position; never
substitute the last historical Run for absent current execution. Preserve
server action ordering/blockers, durable closure classification, and complete
server counts. Label page counts and distinguish unavailable values from zero.
Retain exact host/quota blockers, deterministic Test, and correction lineage.

Share the narrow organization summary between shell and Overview. Keep
route-owned loading, bounded visibility-aware polling, active-Run SSE, request
cancellation, and stale-data labels. Prevent old route/search responses from
overwriting current results. Navigation, refresh, search, and inspection never
dispatch work.

Actions remain on their owner. Separate safe advance, model execution,
authorization, and external effects. Keep tool approvals inside Runs. Preserve
actor/reason/state hash/exact target/effect confirmation. All failures clear
pending state; stale previews require fresh review without automatic retry.
Manual merge and legacy production controls remain unchanged. Facts, claims,
acceptance, risks, contradictions, provenance, and raw evidence stay distinct.

## Acceptance

- Rust fmt, workspace tests, all-target Clippy with warnings denied; UI build,
  Vitest, full Playwright compatibility, accessibility, Helm/schema validation.
- Deterministic timeline tests: repeated/repair executions, missing timestamps,
  delivery wait, source closure, read-only GETs, stable action hashes.
- UI tests: routing, request races/stale state, status honesty, failed/stale
  actions, keyboard timeline inspection, current/history separation.
- Desktop/tablet/phone, dark/light, reduced motion, visible focus and modal
  trapping/restoration, no page overflow or fabricated progress.
- Regressions: completed Repo Mode, legacy full SDLC, paused Runs, unavailable
  hosts, quota pauses, stale previews, compacted history, no five-stage legacy
  delivery warning for Repo Mode.
- Real API + temporary SQLite + provider/worker adapters, no browser API
  interception: Product → onboarding → readiness → WorkItem → stage chain →
  reviewed source delivery → observed manual merge → closure.
- Paid model evaluations and unfinished Codex qualification are not dependencies.

## AMD64 release and rollback

Remove hard-coded lucas-desktop enforcement in build wrapper, image builder,
and native-bundle packager. Require explicit normalized builder selection; use
`rancher-desktop` here, never silently select another builder/platform. Preserve
immutable source checks, registry targets, Linux/AMD64, OCI labels and digests.
Prove actual AMD64 execution, not only advertised platform support. Future
Minisforum adoption changes builder selection, not code.

After acceptance: merge implementation; build all seven existing images and
native bundle from one merged SHA; inspect AMD64 artifacts; separate release-pin
commit; rendered immutable-image scan and Kubernetes server-side dry-run;
deploy through PHarness Argo with redesign disabled; verify revision/image IDs
and fallback behavior; enable through GitOps; verify every route and Finance
records. Preserve characterization and release evidence. Rollback disables the
redesign flag only; no database rollback or evidence deletion.

No host retirement, credential migration, Codex policy promotion, Finance coding
campaign resumption, or execution/approval semantic changes are included.
