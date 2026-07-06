# Prototype Instructions

Run the local server yourself and open the preview in the in-app browser. Do not give the user server-start instructions when you can run it.

Before making substantial visual changes, use the Product Design plugin's `get-context` skill when the visual source is unclear or no longer matches the current goal. When the user gives durable prototype-specific design feedback, preferences, or decisions, record them in `AGENTS.md`.

When implementing from a selected generated mock, treat that image as the source of truth for layout, component anatomy, density, spacing, color, typography, visible content, and hierarchy.

## PHarness Prototype Direction

- Primary lens is the SDLC topology flow.
- Keep Queue, Approvals, and Approval Gates as switchable operator lenses over the same resources.
- Prefer dark mode visually, but keep theme switching available.
- Keep tool approvals visually and behaviorally distinct from approval gates.
- Use production-impacting semantics in UI copy.
- Show pipeline status, policy status, and gate status as separate axes.
- Mark Registry, Database, RAG, and Release as future-backed until wired.
- Integrate policy evaluation, blast radius, approval gates, and tool events into the flow inspector.
- Do not use a double-left-sidebar layout.
- Keep dense trust data collapsible so the main topology remains readable.
- Keep chat/assistant affordances secondary to runs, evidence, policy, and audit.
