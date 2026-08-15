# UI V3: Operator Console for the Autonomous Loop

## Implementation outcome (2026-08-15)

Status: complete for P0-P3 and the stated non-goals.

- P0: auto-refresh is stateful and persisted; Flow resource detail closes and
  retains its collapsed state; navigation badges come from actionable triage
  and run summaries; operational timestamps are age-first with absolute hover
  text; build/fixture material lives under Status; the sidebar is the only
  primary navigation.
- P1: WorkItems have a server-filtered, exactly counted list; blocked items sort
  first; detail renders read-only reconcile preview, explicit reasoned apply,
  authorization checks, active wait timing, advisory attempt history, workspace
  provenance, and the durable Source/Build/GitOps/Deploy/Verify chain.
- P2: Triage aggregates the five attention classes; repeated operator records
  use complete-result server grouping; gates support pending-group selection and
  reasoned batch or single decisions; actor/origin filters cover gates,
  approvals, runs, and audit; run execution and governance counts are separate;
  identifiers lead with human text and expose the full value for copy; scope
  options come from the server.
- P3: `App.jsx` is 461 lines, resource APIs and views are split by concern,
  polling is surface-specific (Triage 10s, Runs 15s, WorkItems 30s, active wait
  5s), and Audit does not auto-poll. Desktop/mobile visual baselines cover the
  stable P0/P1 layouts.

Validation: `cargo test --workspace`, Clippy with `-D warnings`, UI production
build, seven Vitest checks, 26 desktop/mobile Playwright scenarios, visual
inspection in the in-app browser at 1280px, and `git diff --check` all pass.

## Context

This plan reviews the deployed console (v0.12.0) against the current backend
position in [autonomous-sdlc-roadmap.md](autonomous-sdlc-roadmap.md) and
[autonomous-sdlc-alpha.md](autonomous-sdlc-alpha.md). It supersedes the open
items in [ui-v2-improvements.md](ui-v2-improvements.md); the honest-empty-state
and no-decorative-controls rules from
[ui-minimal-design.md](ui-minimal-design.md) still govern.

Evidence used: four live screenshots of Flow, WorkPlans, Runs, and Approval
Gates against real smoke data, plus [App.jsx](../ui/src/App.jsx) (2,836 lines)
and [pharnessApi.js](../ui/src/pharnessApi.js).

The V2 plan's stated goals were met: surfaces are API-backed, deep links work,
Incidents/RemediationPlans/Observations shipped, gates group under plans, audit
has server-side filters. The problems now are different in kind.

## Diagnosis

Three structural problems, in priority order.

### 1. The console models the old root. The backend moved to WorkItem.

The console's spine is `ChangeSet -> ... -> RegistryEvidence`: a seven-card
ribbon with `WorkPlan | ChangeSet | PipelineIntent | PipelineRunAnalysis |
DeploymentIntent | Release | RegistryEvidence` (screenshot 1). That was the
right spine when a ChangeSet was the durable root.

The backend's durable root is now a WorkItem, and the real chain is roughly:

```
WorkItem -> WorkPlan -> Workspace(pin) -> ChangeSet
  -> git_delivery_plan -> grant -> preflight -> writer result -> PR -> merge
  -> PipelineIntent -> Tekton execution -> analysis -> build_output
  -> DeploymentIntent -> GitOpsChangeSet -> plan -> writer -> PR -> merge
  -> Argo sync -> Release -> post-sync verification -> complete
```

Nothing between `ChangeSet` and `PipelineIntent`, and nothing in the GitOps
segment, has any UI. Neither do `WorkItem`, `Workspace`, `PermissionGrant`,
`controller_wait`, `PipelineContract`, `DeploymentContract`, or
`GitOpsChangeSet`. `pharnessApi.js` contains no `/api/work-items`,
`/api/workspaces`, or `/api/controller-waits` call. Phases 1-6 — the entire
current body of work — are CLI-and-curl only.

This is the single highest-value gap. Everything else on this list is polish by
comparison.

### 2. The console shows inventory, not decisions.

The screenshots show sixteen WorkPlans titled `WorkPlan: Execute inert Tekton
fixture`, eleven pending gates all titled `Approve production impact` /
`Approve cluster mutation` on `PrometheusInventory/smoke-alert`, and a Flow
ribbon that is four-sevenths `Not created`. Every surface is a flat list of
near-identical rows whose only distinguishing feature is a middle-truncated id
(`wplan_1785...24511`, `agate_rpla...mpact`).

An operator opening this cannot answer the two questions that matter: *what is
waiting on me*, and *what is stuck*. Those answers exist in the data — pending
gates, pending approvals, `blocked` WorkItems, expired `controller_wait`s — and
are spread across four surfaces with no aggregation.

Contributing causes:

- Nav badges are page lengths, not workload. `badgeForNav`
  ([App.jsx:389](../ui/src/App.jsx:389)) returns `data.auditEvents.length`
  (50 — the request limit, [pharnessApi.js:150](../ui/src/pharnessApi.js:150)),
  `observations.length` (49), `incidents.length` (21). "Audit 50" is a
  pagination artifact rendered as an alert count. Only Approvals and Approval
  Gates badge something actionable.
- Smoke traffic and operator traffic are indistinguishable. The V2 backlog item
  "actor chips/filters to separate smoke traffic from operator traffic" never
  shipped, and the screenshots are the consequence: real state is buried in
  fixture repetition.
- Only Audit has search. WorkPlans, gates, and Runs have none.

### 3. Dead and stale controls have crept back.

The no-decorative-controls rule is being violated by three specific elements:

- The Auto-refresh toggle is an icon with no handler
  ([App.jsx:593](../ui/src/App.jsx:593)). Refresh is an unconditional 15s
  `setInterval` ([App.jsx:130](../ui/src/App.jsx:130)). It renders as an
  enabled switch in the on position and does nothing.
- The Inspector's close button has no `onClick`
  ([App.jsx:2500](../ui/src/App.jsx:2500)); `IconButton` accepts one
  ([App.jsx:472](../ui/src/App.jsx:472)). The X in all four screenshots is
  inert.
- The Inspector itself is pinned to Flow. It resolves from `topologyNodes` and
  falls back to `topologyNodes[0]`
  ([App.jsx:2470](../ui/src/App.jsx:2470)), so `PipelineRunAnalysis` detail
  renders identically in Runs, WorkPlans, and Approval Gates — a fifth of the
  viewport showing a resource unrelated to the current view. In screenshot 4
  it reports `Approval Gates (this flow) 0 pending` roughly 300px from a list
  header reading `11 pending`. The label is technically honest and still reads
  as a contradiction.

Two further honesty problems:

- Timestamps are time-only (`1:57:01 PM`, `8:32:22 AM`). Runs in screenshot 2
  clearly span more than one day, and gate `Requested 1:57:01 PM` gives no age.
  A control plane needs age, and needs it to be the primary form.
- `Delivery Test` is a test fixture holding a top-level nav slot, and the
  Implementation Strip — a build-status artifact — occupies prime vertical
  space on every view, truncated mid-word (`Incidents / R…`) in every
  screenshot.

## P0 — Honesty pass (small, causes located)

- Make the Auto-refresh toggle real: hold `autoRefresh` state, gate the
  interval on it, or delete the control and label the interval.
- Wire the Inspector close button; persist collapsed state.
- Scope the Inspector to the active view or hide it outside Flow and Run
  Detail. Do not render Flow-node detail beside an unrelated list.
- Replace page-length nav badges with actionable counts only: pending
  approvals, pending gates, running runs, blocked WorkItems, expired waits.
  Drop badges from WorkPlans, Audit, Observations, Incidents, and Remediation
  Plans unless a real "needs attention" predicate exists. A count that means
  "the page limit" is worse than no count.
- Render timestamps as relative-primary with absolute in `title`
  (`3d ago` / `Aug 3, 1:57:01 PM`). Apply to gate requested/decided, run
  submitted/finished, audit rows, and timeline cards.
- Move the Implementation Strip behind an About/Status panel or a dev-mode
  flag. Its content is release history, not operator context, and it is
  truncated everywhere it renders.
- Move `Delivery Test` out of primary nav into that same status/dev surface.
- Collapse the duplicate navigation. The sidebar and the mode-bar tab strip
  ([App.jsx:566](../ui/src/App.jsx:566)) overlap, disagree on labels
  (sidebar `Runs`, tab `Queue`), and the tab strip omits Observations,
  Incidents, and Remediation Plans, so navigating between those three is
  inconsistent with navigating the other five. Keep the sidebar; use the
  mode-bar for view-local context and filters.

## P1 — WorkItems as the console spine

This is the substance of V3. Three new surfaces, in this order.

### WorkItems list

`GET /api/work-items`. Columns: intent summary, status, environment, target
repo/ref, current boundary, attempt budget remaining, age, active wait. Filter
by status and by actor/origin. Group or badge `blocked` first — a blocked
WorkItem with a draft RemediationPlan is the highest-value row in the system.

### WorkItem detail — the reconcile surface

This is the most important screen the console does not have. The controller's
central concept is *next action, previewed by default, applied explicitly, with
bounded waits*, and it maps almost directly onto UI:

- **Next action panel.** Render `POST /api/work-items/:id/reconcile` with
  `apply=false` as a first-class preview: the action name, the boundary it
  stops at, and the exact reason it cannot proceed (missing gate, missing
  grant, absent writer configuration, external wait).
- **Apply as an explicit, typed operation.** One button, labelled with the
  action, disabled with the specific unmet precondition as its reason, and a
  confirmation that restates the external effect (`dispatch one isolated
  branch-and-PR writer against lward27/yfinance_wrapper`). The preview/apply
  split is a safety property; make it visible rather than hiding it behind a
  generic Refresh-shaped button.
- **Authorization row.** Gate / PermissionGrant / trusted envelope / allowlist
  as four explicit states. Today an operator has to read reconcile JSON to
  learn that `dispatch_ready=false` because a writer is not deployed rather
  than because a gate is unsatisfied. Those are different problems and should
  not look the same.
- **Active wait.** Next observation time, deadline, checks used of budget, and
  a visible countdown to expiry. Expired waits block a WorkItem; that should
  be predictable from the UI, not a surprise.
- **Attempt history** with the `work_item.attempt_finished` classification and
  its recommended next action, marked advisory.
- **Workspace provenance**: repository, requested ref, resolved immutable
  commit, attempt branch, retention state. The commit pin is the trust anchor
  of the whole chain and appears nowhere in the UI today.

### Delivery chain view (replaces the Flow ribbon)

The seven-card ribbon should become a segmented chain with the Git and GitOps
segments present:

```
Source        Build          GitOps         Deploy         Verify
workspace     intent         changeset      intent         release
changeset     execution      plan           argo sync      prometheus
plan/grant    analysis       writer/PR      health         complete
writer/PR     build_output   merge
merge
```

Each cell resolves to one durable artifact with a status, and each artifact is
clickable. Render `Not created` cells as unreached rather than as failures —
four `Not created` pills of equal visual weight (screenshot 1) read as damage
when they mean "not yet". Show which segment the item is currently stopped in.

Keep `GET /api/change-sets/:id/flow` as the read model for ChangeSet-rooted
history; add a WorkItem-rooted read model rather than stitching artifacts
client-side.

## P2 — Triage and density

- **Add a Triage/Inbox landing view.** One list, ordered by what needs a human:
  pending gates, pending tool approvals, blocked WorkItems, expired waits,
  RemediationPlans awaiting `proposed -> approved`. This replaces "open Flow
  and hope" as the entry point and is the natural home for the actionable
  counts P0 keeps.
- **Collapse repetition.** Group identical WorkPlans, gates, and runs by
  (title, resource, status) with a count and an expander. Sixteen rows of
  `Execute inert Tekton fixture` is one row that says `16`.
- **Multi-select gate decisions.** Eleven identical smoke gates require eleven
  round trips through a single detail panel. Support select-all-in-group with
  one reason, and require an explicit reason for waive/reject.
- **Actor and origin filters** across gates, approvals, runs, and audit — the
  outstanding V2 item, now the dominant readability problem.
- **Compact the detail panels.** Screenshot 4 spends six bordered boxes and
  roughly 300px of height on six short values (`Pending`, `High`,
  `production_impact`, `4`, one resource, one time). Use a two-column
  definition list; reserve boxes for values that carry weight (risk, blockers,
  the gate payload).
- **Fix identifier rendering.** Middle-truncated ids (`rplan_inc_...ddcfb`) are
  unreadable and un-greppable as primary card content. Lead with the human
  label plus a short suffix (`gate · production_impact · …ddcfb`), keep the full
  id copyable, and never make a truncated id the only distinguishing text in a
  card title or group header.
- **Separate governance from execution in the Runs summary.** The Runs view's
  four stat cards mix run state (running, completed) with governance state
  (`Approval gates 11`), duplicating the nav badge in a view that cannot act
  on it.
- **Make scope selectors server-derived.** Options come from loaded rows
  (`scopeOptions`, [App.jsx:698](../ui/src/App.jsx:698)), so a namespace with
  no rows in the current page is unselectable. Add distinct-value endpoints or
  derive from config.

## P3 — Structure

- Split `App.jsx` before P1 lands, not after. 2,836 lines with the WorkItem
  surfaces added is unreviewable; the split is behavior-neutral and cheap now.
- Extract the typed API client per resource; add `work-items`, `workspaces`,
  `controller-waits`, and the delivery-artifact reads.
- Replace the 15s whole-dashboard poll with per-surface freshness. The
  triage counts and an active wait countdown want SSE or a short poll; the
  audit page does not need to refetch 50 rows every 15 seconds.
- Add visual regression checks (outstanding since ui-minimal-design) once the
  P0/P1 layout settles.

## Decisions

- The console's information architecture follows the durable root. WorkItem is
  now that root; the ChangeSet-rooted Flow ribbon becomes one segment of a
  larger chain rather than the whole spine.
- Reconcile preview/apply is a UI-visible safety boundary, not an
  implementation detail. Every apply states the external effect before it
  dispatches, and no view infers or triggers a dispatch as a side effect of
  navigation or refresh.
- Nav badges mean "needs attention". A count that reflects a page limit or a
  total inventory is removed rather than relabelled.
- Timestamps in an operational console are ages first, wall-clock second.
- Fixtures and build status are not product surfaces. `Delivery Test` and the
  Implementation Strip move to a dev/status surface.
- Identical rows are collapsed with counts. Smoke repetition must not be able
  to hide real state.

## Non-Goals

- No chat/assistant surface. Runs, evidence, policy, and audit stay primary.
- No direct SDLC resource mutation from the Inspector. WorkItem apply actions
  live on the WorkItem detail surface and go through `reconcile`.
- No UI-initiated retry, merge, sync, or rollback. The UI exposes the
  controller's bounded actions; it does not add new authority.
- Keep the dark operational console visual grammar.

## Sequencing

1. P0 honesty pass — one small, self-contained commit.
2. Split `App.jsx` — behavior-neutral, before new surfaces.
3. WorkItems list and detail, read-only preview first. No apply.
4. Apply actions on WorkItem detail, one boundary at a time, starting with the
   boundaries already exercised by the in-process full-chain test.
5. Delivery chain view replacing the Flow ribbon.
6. Triage view, collapse/multi-select, actor filters.

## Backlog

- Add a WorkItem-rooted flow read model so the delivery chain view does not
  stitch artifacts client-side.
- Surface `PipelineContract` and `DeploymentContract` bindings read-only; a
  drifted or retired contract is currently invisible until preflight rejects
  it.
- Show `PermissionGrant` scope and expiry as a first-class read surface.
- Show worker Job identity and link running rows to their Kubernetes Job
  (outstanding V2 item).
- Consider a per-WorkItem live event stream reusing the Run Detail SSE client
  once WorkItem events are streamable.
