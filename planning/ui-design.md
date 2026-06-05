# Decisions

- Start PHarness UI design with static visual concepts before implementation.
- Treat the primary surface as an operator control plane, not a chatbot.
- Center the first screen on runs, approvals, live events, diffs, artifacts, observations, incidents, remediation plans, typed capabilities, and audit evidence.
- Use desktop dashboard dimensions for the first exploration because the target workflow is dense SDLC operations.
- Vary the three concepts by information architecture and operating model, not by decorative styling alone.
- Prioritize the SDLC topology view for the first product direction because PHarness starts in a homelab but is aiming at fully automated end-to-end SDLC.
- Use the dark visual language from the run-control concept as the preferred initial theme, while preserving light/dark theme switching as a product requirement for the real UI.
- Incorporate gate-review evidence into the topology-first surface: policy evaluation, blast radius, tool events, approvals, and audit provenance should be visible without forcing a separate review-only screen.
- Avoid double-left-sidebar navigation. Switchable operator views should share one primary navigation system.
- Build the first interactive prototype as a self-contained React/Vite app outside the Rust workspace at sibling directory `pharness-ui/` so prototype churn does not disturb the PHarness repo.
- Implement Flow, Queue, Approvals, and Approval Gates as switchable lenses over shared seeded PHarness data.
- Keep inspector density manageable with collapsible policy, blast-radius, approval-gate, and tool-event sections.
- Separate tool approvals from approval gates in both labels and actions. Tool approvals are execution decisions that resume or block run actions; approval gates are governance/release state with satisfy, waive, and reject lifecycle actions.
- Use `production_impacting` or "Production-impacting" language instead of blunt "Production true" copy.
- Keep pipeline status, policy status, and gate status as separate state axes in the resource detail panel.
- Treat Registry, Database, RAG, and Release as future-backed states until the corresponding capabilities are implemented; the UI should degrade gracefully rather than implying live support.
- Keep assistant/chat affordances secondary so the product center of gravity remains runs, evidence, policy, and audit.

# Backlog

- Decide whether the first implementation should use live API data, seeded mock data, or a hybrid contract fixture.
- Define mobile and tablet behavior after the desktop operator workflow is coherent.
- Create reusable visual language for risk, policy state, resource identity, and verification evidence.
- Revisit whether PHarness needs a chat-like prompt entry surface once the run and approval workbench is usable.
- Explore role/risk-profile view presets: topology-first for SDLC operators, queue-first for active run triage, and gate-first for high-risk or production environments.
- Add a compact timeline mode that keeps recent tool events visible on shorter desktop viewports.
- Decide whether the right inspector should become resizable once real payloads are connected.
- Add a dedicated WorkPlans screen or decide whether WorkPlans belong under a broader planning/remediation navigation group.
- Connect resource-specific action buttons to the eventual live API: approve, deny, waive, satisfy, reject, cancel, view diff, and view artifact.
