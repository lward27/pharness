# Prototype Instructions

## Current implementation authority

Read the approved [ASTRA program](../planning/programs/autonomous-sdlc/ASTRA-00-PROGRAM.md),
its M05 contract and M10 console milestone, and the living
[product vision](../planning/design/product-vision-and-boundaries.md). The older Repo Mode
product/screen contracts describe legacy behavior, not competing new implementation plans.
A missing API field remains a dependency; never synthesize product state in the client.

Run the local server yourself and open the preview in the in-app browser. Do not give the user server-start instructions when you can run it.

Before making substantial visual changes, use the Product Design plugin's `get-context` skill when the visual source is unclear or no longer matches the current goal. When the user gives durable prototype-specific design feedback, preferences, or decisions, record them in `AGENTS.md`.

When implementing from a selected generated mock, treat that image as the source of truth for layout, component anatomy, density, spacing, color, typography, visible content, and hierarchy.

## PHarness Prototype Direction

- Approved 2026-09-04: the Lamina prototype is the visual source for the new
  flagged console. Use floating top navigation with the eight existing sections,
  glass/pattern/glow styling, and default horizontal laminae WorkItem intervals.
  Do not ship the generated prototype runtime or fictional lifecycle/data.
- Lamina intervals represent recorded elapsed time including waits; active
  model time is separate. Unknown timing is unavailable. Preserve correction
  lineage and read-only inspection. Local Mac release builds target linux/amd64.

- The V1 global order is Overview, Products, Repositories, WorkItems, Agents, Releases, Insights, and Settings. Existing Triage, Queue, Approval, and Flow lenses must be adapted under this hierarchy rather than preserved as a competing navigation system. A WorkItem remains the durable intent root; ChangeSet Flow is one delivery segment, not the console spine.
- WorkItem Overview is the default operational surface. Current Stage, StageOutcomes, Delivery, Evidence, and History remain distinct sections under the same intent.
- Keep current WorkItems and AgentRuns primary. Prior attempts, superseded outcomes, completed WorkItems, and historical Runs belong in explicit History surfaces.
- StageOutcome presentation must distinguish verified facts, outputs, acceptance, agent claims, contradictions, risks, freshness, and provenance. Raw events are drill-down evidence, not the default state model.
- Navigation and refresh must never dispatch controller work. The durable backend controller advances previously authorized hosted work without ritual clicks. New authority and production promotion retain explicit, state-bound previews/approval; production approval must precede GitOps merge.
- A view may show only API-backed durable state. Empty, unreached, disabled, and unavailable states must be named plainly; do not synthesize inventory or failure treatment.
- Origin is operational data: operator, controller, worker, smoke, system, and legacy records must be filterable without deleting their audit trail.
- Audit origin is durable provenance returned by the API, not an origin guessed from actor names, titles, or client-side links.
- Run actor filters use the durable submission or reconcile actor recorded by the API; never infer a person from a run title, branch, or origin label.
- Repeated-record counts come from the complete API-filtered result set, never only the currently visible list page.
- Keep preview and apply separated for authority-bearing actions. Creation, readiness, pause/resume/cancel, and approval use documented mutation APIs. Explain that readiness can create temporary Jobs and persist evidence. Other controls are read-only, navigational, or refresh-only.
- Maintain desktop and phone-width visual baselines for empty and blocked operator states whenever shared layout changes.
- Navigation badges mean actionable attention only. Show ages first with the absolute time available on hover or assistive text.
- Keep the Inspector scoped to relevant resource detail. Do not show Flow-node detail beside unrelated inventory views.
- Put fixture and implementation-status material on the Status surface, not in the primary operator path.
- Use Flow as a focused delivery-evidence view when a WorkItem or resource needs investigation; it is not the console's default landing surface.
- Keep Queue, Approvals, and Approval Gates as switchable operator lenses over the same resources.
- Prefer dark mode visually, but keep theme switching available.
- Keep tool approvals visually and behaviorally distinct from approval gates.
- Use production-impacting semantics in UI copy.
- Show pipeline status, policy status, and gate status as separate axes.
- Describe each capability from API-backed evidence. Do not keep an implemented Release marked future-backed or imply unimplemented Registry/Database/RAG behavior exists.
- Integrate policy evaluation, blast radius, approval gates, and tool events into the flow inspector.
- Do not use a double-left-sidebar layout.
- Keep dense trust data collapsible so the main topology remains readable.
- Keep chat/assistant affordances secondary to runs, evidence, policy, and audit.
