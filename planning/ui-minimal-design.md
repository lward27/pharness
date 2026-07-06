# Minimal UI Design

## Decisions

- Treat the sibling UI as an operations console for the Pharness control plane, not as the primary agent chat surface.
  - The mock already aligns with this direction: SDLC flow, evidence, policy evaluation, approvals, and audit are first-class.
  - Hermes or any desktop worker UI should remain separate and integrate through a clean adapter.

- First UI slice should be read-heavy and API-backed.
  - Wire health, config, runs, run events, approvals, approval gates, audit events, SDLC resources, readiness, and registry evidence.
  - Avoid synthetic polish that suggests backend capability we do not yet expose.
  - Use `GET /api/work-plans/:work_plan_id/flow` as the initial Flow view read model before a ChangeSet exists.
  - Use `GET /api/change-sets/:change_set_id/flow` once source changes exist, instead of stitching readiness, incidents, remediation plans, approval gates, and audit events client-side.

- Keep the visual grammar from the mock.
  - Dark operational console.
  - Scope selector across environment, namespace, repository, and branch.
  - Left navigation around control-plane resource types.
  - Main Flow view for SDLC state.
  - Right detail panel for selected resource, policy evaluation, blast radius, gates, and tool events.

- Defer write workflows in the UI until the backend API exposes deterministic create surfaces for the full SDLC root chain.
  - Approval decisions and run cancellation are acceptable first mutations.
  - Creating WorkPlans and ChangeSets from existing roots is acceptable once the list/detail views are wired.

- Start the live UI adapter as a read-only Vite client over the machine-facing API.
  - Use same-origin `/health` and `/api/...` calls in the React app.
  - Let the Vite dev server proxy those calls to the local Rust API, avoiding a CORS dependency for the prototype.
  - Prefer ChangeSet flow when a ChangeSet exists; otherwise fall back to WorkPlan flow.
  - Preserve the static mock as an empty/offline fallback instead of blanking the operator console.

- Compact live resource identifiers in dense UI surfaces.
  - Do not place raw WorkPlan, ChangeSet, PipelineIntent, DeploymentIntent, or RegistryEvidence ids into topbar values, flow card footers, or timeline cards without truncation.
  - Preserve full values through hover titles and detail panels.
  - Long tokens must be clipped, ellipsized, or line-clamped inside their own component; they must not paint into adjacent cards or controls.

- Wire Queue, Tool Approvals, and Approval Gates to live API data after Flow.
  - Queue reads `/api/runs` and `/api/runs/summary`.
  - Queue submits runs through `POST /api/runs` with task, cwd, and max turns.
  - Queue cancels non-terminal runs through `POST /api/runs/:id/cancel`.
  - Queue must read `/api/config/effective` and disable submit when `worker.enabled` is false, because the API can persist queued runs even when no worker can execute them.
  - Tool Approvals reads `/api/approvals` and uses explicit approve/deny buttons for `/api/approvals/:id/{approve,deny}`.
  - Approval Gates reads `/api/approval-gates` and uses explicit satisfy/waive/reject buttons for `/api/approval-gates/:id/{satisfy,waive,reject}`.
  - Empty API lists should render honest empty states instead of seeded prototype cards.

- Remove seeded prototype data from the runtime UI.
  - The Flow view now renders only API-backed WorkPlan/ChangeSet flow data or an explicit empty/offline state.
  - The UI includes a compact implementation strip that separates live API-backed surfaces from planned-only surfaces.
  - Planned navigation items are disabled and labelled `planned` instead of behaving like unfinished live screens.
  - The right inspector is read-only for SDLC resources. Real mutations stay in Queue, Tool Approvals, and Approval Gates where they call the API.
  - Flow and timeline cards use constrained dimensions and truncation rules so long control-plane ids do not overlap adjacent UI.

- Add a live Run Detail view as the first drill-down surface.
  - Queue `Open` selects a run and opens Run Detail instead of bouncing back to Flow.
  - Run Detail composes `GET /api/runs/:id`, `/events`, `/diff`, and `/artifacts`.
  - The view shows final result summary, run status/scope/turns, durable events, persisted file diffs, and artifacts.
  - Missing diffs and artifacts render as explicit empty states because read-only runs can complete without either.
  - Run Detail can cancel the selected non-terminal run through `POST /api/runs/:id/cancel`; approvals remain in Tool Approvals or Approval Gates.
  - Other SDLC resource detail surfaces stay read-only until deterministic mutation semantics exist.

- Wire Run Detail to the existing SSE event stream.
  - The UI subscribes to `/api/runs/:id/events/stream` with `EventSource`.
  - Because Pharness emits named SSE events, the client registers known event names instead of relying on `onmessage`.
  - The client dedupes events by `event_id` or `seq/type` because a fresh `EventSource` replays persisted events from the beginning.
  - Terminal events close the client stream explicitly so a successful completed run does not surface as a later disconnect.
  - Already-terminal runs do not open an SSE connection after initial detail fetch because there are no future events to watch.
  - Tool completion, approvals, and terminal events trigger a detail reload so run result, diff, and artifacts converge back to API truth.

- Add an SSE cursor query for browser clients.
  - `GET /api/runs/:id/events/stream?after_seq=N` starts replay after a durable event sequence.
  - The query cursor takes precedence over `Last-Event-ID`.
  - Run Detail uses the highest sequence from the initial `/events` fetch to avoid replaying the entire run on first stream connection.

- Make Run Detail stream state explicit and API-backed.
  - The Run Detail SSE subscription effect depends on both the selected run and the loaded event cursor; otherwise a non-terminal run can set a cursor after initial load without opening a stream.
  - The view now shows source, replay cursor, durable event count, and run state in a compact stream status panel.
  - Terminal or approval-paused runs are presented as durable snapshots instead of implying an active live stream.

- Keep Queue controls executable, not decorative.
  - Queue Refresh calls the shared dashboard API refresh instead of being a static button.
  - Successful run submission selects the created run and opens Run Detail so the operator immediately sees durable events and stream state.
  - Run Detail cancellation refreshes the shared dashboard data so Queue summaries converge with the selected run state.

- Keep approval controls executable and locally observable.
  - Tool Approvals and Approval Gates render their own action notices instead of relying on the Flow inspector to show mutation feedback.
  - Tool Approvals can open the associated run in Run Detail for event, diff, artifact, and result review.
  - Approval and gate views expose real Refresh buttons wired to the shared dashboard API refresh.
  - Placeholder-only approval/gate buttons are removed until a deterministic API-backed detail surface exists.

- Promote Audit from planned-only to a live read-only surface.
  - The UI now fetches `GET /api/audit-events?limit=50` as part of the dashboard read model.
  - The Audit view renders event kind, resource, actor, run id, payload summary, and timestamp.
  - Audit remains read-only; search and resource-specific filtering stay in the backlog until the UI exposes explicit filter controls.

- Promote WorkPlans from planned-only to a live read-only surface.
  - The UI now fetches `GET /api/work-plans?limit=50` as part of the dashboard read model.
  - The WorkPlans view lists real plans, status, risk, summary, and revision.
  - Selecting a plan loads `GET /api/work-plans/:id/flow` to show readiness, blockers, warnings, and downstream ChangeSet/PipelineIntent/DeploymentIntent/Release/RegistryEvidence state.
  - WorkPlan mutations remain out of the UI until the operator flow for plan revision, approval staleness, and trusted envelopes is deterministic.

## Backlog

- Continue the typed API client in the UI:
  - `GET /health`
  - `GET /api/config/effective`
  - `GET /api/runs`
  - `GET /api/runs/:id`
  - `GET /api/runs/:id/events`
  - `GET /api/runs/:id/events/stream`
  - `GET /api/approvals`
  - `POST /api/approvals/:id/approve`
  - `POST /api/approvals/:id/deny`
  - `GET /api/approval-gates`
  - `GET /api/change-sets`
  - `GET /api/pipeline-intents`
  - `GET /api/deployment-intents`
  - `GET /api/releases`
  - `GET /api/registry-evidence`
  - `GET /api/change-sets/:id/flow`
  - readiness endpoints for WorkPlans and ChangeSets.

- Move inspector actions onto the same live mutation adapter once duplicate decision paths are removed.

- Replace mock topology data with API-derived status:
  - WorkPlan lifecycle.
  - ChangeSet lifecycle and material revision status.
  - PipelineIntent status.
  - DeploymentIntent status.
  - Release status.
  - RegistryEvidence status and supply-chain warning state.

- Add UI states for missing backend data:
  - Not configured.
  - No resources yet.
  - Capability unavailable.
  - Backend unhealthy.
  - Stale approvals after material changes.

- Add event streaming for selected runs with Server-Sent Events.

- Add a small local proxy or config mechanism for the UI API base URL.

- Add visual regression checks once the first real API-backed screens exist.

- Add real list/detail surfaces for the currently planned navigation items:
  - Observations.
  - Incidents.
  - Remediation Plans.
  - Capabilities.

- Add UI actions only when they call deterministic API endpoints.
  - Do not reintroduce local state-only inspector actions.
  - Direct SDLC resource mutations should remain read-only until backend execution semantics exist.
