# UI V2 Improvements Plan

## Context

The operator console now serves live at pharness.lucas.engineering against
the V2 cluster runtime, and real data accumulates: smoke chains, worker-Job
runs, decided approvals, and sixteen pending gates. Live operation surfaced
issues the empty prototype never hit. This plan extends
[ui-minimal-design.md](ui-minimal-design.md); the honest-empty-state and
no-decorative-controls rules there still govern.

## P0 — Correctness and trust (one small pass, causes located)

- Fix run summary stat cards that always render 0.
  - `statusCount` matches `bucket.label`, but the API serializes
    `CountBucket { value, count }` (`ui/src/App.jsx:350`,
    `pharness-store/src/models.rs:134`). Running and Completed are constant
    zero while the run list visibly contains completed runs. An operator who
    notices stops trusting every other number on the page.
- Remove the literal `unknown` column in Queue run rows.
  - `run.result?.risk_level` does not exist on run results
    (`ui/src/App.jsx:904`); every row renders the fallback. Replace with a
    real field (turns, scope namespace) or drop the cell.
- Rework the Tool Approvals "pending" pill.
  - It renders `toolApprovalState`, a local last-action string initialized to
    "pending", styled like a filter chip (`ui/src/App.jsx:1417`). Replace
    with real status filters (pending / decided) now that decided approvals
    accumulate, and show decided_by / decided_at / reason on decided items.
    Approve/Deny are already correctly disabled for non-pending items; give
    the disabled state a visibly muted style.
- Fill and fix the Approval Gates detail panel.
  - The panel renders a title, one floating sentence, and Status/Risk pushed
    below the fold with dead space between. Show what the API already
    returns: gate kind and order, remediation plan and incident ids,
    resource scope, requested/decided metadata; fix the vertical layout.
- Reconcile right-rail counts with the main views.
  - The rail shows "Approval Gates 0 pending" while the view lists sixteen,
    and "Tool Approvals 2 pending" while the Queue card says zero. Audit the
    rail's data sources; derive every count from the shared dashboard read
    model or label the rail's scope explicitly (for example "selected flow
    root only").
- Disable Cancel on terminal Queue rows.
- Remove count badges from disabled planned nav items.
  - Incidents (2) and Remediation Plans (2) show badge counts but cannot be
    opened. Ship the views (P1) or drop the badges; a numbered dead door is
    worse than a plain one.
- Update stale copy.
  - The implementation strip still says "Fireworks worker"; the sidebar
    footer says "Worker Enabled". Show the dispatch mode from
    `/api/config/effective` (`worker.mode`: local / kubernetes_job), which
    the API already exposes.

## P1 — Navigation and drill-down

- Add a Flow root picker.
  - `loadFlow` pins the Flow view to the first ChangeSet returned with
    `limit=1`; with accumulating data the operator cannot choose what they
    are looking at. Offer recent ChangeSets/WorkPlans (by update time),
    remember the selection, and make WorkPlans selection navigate to Flow
    with that root.
- Add hash-based deep links.
  - `#/runs/:id`, `#/approvals/:id`, `#/gates/:id`, `#/flow/:kind/:id`.
    Refresh currently loses all context and nothing is shareable. No router
    dependency needed; sync the existing `activeView`/selection state to the
    hash.
- Make resource ids clickable everywhere they appear.
  - Evidence & Signals rows, Control-Plane Timeline cards, and Audit rows
    show ids that look like links but navigate nowhere. Route them to the
    owning surface once deep links exist.
- Ship Incidents and Remediation Plans as live read-only surfaces.
  - List endpoints exist and nav badges already show counts. Group Approval
    Gates by remediation plan with incident context; sixteen flat cards of
    four repeating kinds hides the structure the control plane already has.
- Ship a live Observations list.
  - Source/kind/resource filters map to the existing list endpoint; link
    flow evidence rows into it.

## P2 — Scope, filtering, and cluster-mode affordances

Status: scope filtering and audit search shipped locally on 2026-07-10; cluster
rollout remains pending. Queue worker-Job affordances and actor presets remain.

- Wire the topbar scope selector to real API filters.
  - Runs, approvals, gates, and audit endpoints accept namespace, repo,
    branch, and production_impacting filters. Until wired, demote the
    selector to a display-only label per the no-decorative-controls rule.
- Add Audit search and filters.
  - Kind, resource kind/id, actor, and run id are all supported server-side;
    add payload row expansion instead of mid-word truncation.
- Make Queue honest about kubernetes_job mode.
  - Hide or annotate CWD (the server overrides it to the Job workspace),
    show worker mode and image from effective config, and link a running row
    to its worker Job via the `pharness.lucas.engineering/run-id` label.
- Add actor chips/filters to separate smoke traffic from operator traffic.

The shipped scope/search slice includes:

- Topbar namespace, repository, branch, and production-impact selectors that
  refetch runs, run summaries, approvals, approval gates, SDLC lists, and audit
  rows through server-side query parameters.
- Global audit search that deep-links to `#/audit/<search>`.
- Audit filters for kind, resource kind, actor, run id, and free text.
- Expandable audit payload JSON instead of irreversible row truncation.
- Store-side namespace resolution through referenced SDLC resources when older
  audit payloads do not contain an embedded run scope.

## P3 — Structure and polish

- Split `App.jsx` (~2,000 lines) into view modules once P0/P1 churn settles.
- Revisit approvals/gates freshness (SSE or tighter polling) after real
  usage; the 15s poll is acceptable for alpha.
- Existing design-qa follow-ups: inspector width resize, compact timeline
  below 720px, saved operator presets.
- Add visual regression checks (already in the ui-minimal-design backlog)
  once the P0 pass lands.

## Non-Goals

- No direct SDLC resource mutations from the inspector; mutations stay in
  Queue, Tool Approvals, and Approval Gates.
- No chat/assistant surface; runs, evidence, policy, and audit stay primary.
- Keep the dark operational console visual grammar.

## Verification

- The P1 pass shipped on 2026-07-08 and was verified against the deployed
  API's live data before shipping and on the deployed console after
  rollout (bundle hash matched the local build exactly).
  - Hash deep links restore every surface across a hard reload
    (`#/incidents`, `#/observations`, `#/gates/<id>`, ...).
  - The Flow root picker lists recent ChangeSets and WorkPlans, switches
    the flow read model, and encodes the root in the URL
    (`#/flow/change_set/<id>`); WorkPlans open into Flow.
  - Incidents (8), Remediation Plans (8), and Observations (15) render
    live smoke-generated data with metrics, source filters, and detail
    panels; the incident -> plan -> gate link graph navigates end to end,
    and plan detail showed live gate statuses (two satisfied by the
    operator, two pending).
  - Approval gates group under remediation-plan headers that link to the
    owning plan; audit rows and timeline cards navigate to their owning
    surfaces.
  - `scripts/pharness-build.sh` (bash, literal image references) shipped
    the image; the deployed bundle hash matched the local build,
    confirming the mistagged-push failure mode is closed.

- The P0 pass shipped on 2026-07-08 and was verified twice: against the
  deployed API's live data through the dev proxy before shipping (queue
  summary reads real counts, run rows show turns, both approval surfaces
  filter pending/decided with real count pills, the gate panel renders
  order/requested/plan/incident, the inspector's hardcoded count is gone,
  planned nav items lost their badges, worker mode reads kubernetes_job),
  and again on the deployed console through the tunnel after rollout.
  The approvals API now returns requested/decided metadata, shown for the
  finance-app approval decided by `lucas`.

- Deployment note: the first attempt to ship these images silently landed
  in `pharness-uiatest` / `pharness-runtimeatest` registry repositories.
  Cause: an ad-hoc zsh heredoc expanded `$IMG:latest` as the `${IMG:l}`
  lowercase modifier plus `atest`. Kaniko reported success because the
  push genuinely succeeded, at the wrong name; rollouts then re-pulled the
  old `latest`. Recovered with an in-cluster skopeo copy to the correct
  repositories. Keep image references literal or brace-quoted in shells,
  and prefer `scripts/trigger-build.sh` (bash, unaffected). The two junk
  repositories remain in the registry until a garbage-collection pass.

## Sequencing

1. P0 is a single focused pass; every item has a located cause.
2. Deep links and the Flow root picker land before the new P1 surfaces so
   Incidents/RemediationPlans/Observations are linkable from day one.
3. Each stage ships through the existing pharness-ui Tekton build and is
   verified against the deployed console, not just the dev server.

## Decisions

- Scope controls are execution filters, not decorative context labels. The
  environment remains a fixed `homelab` display until environment becomes a
  first-class list filter across the API.
- Audit filtering belongs in SQLite/API query contracts. Client-only filtering
  would silently search only the latest loaded page and produce false results.
- Namespace-scoped audit reads resolve scope from durable resources when event
  payloads predate embedded `run_scope` fields.

## Backlog

- Deploy this P2 scope/search slice through `scripts/pharness-build.sh ui` and
  verify it against the cluster console.
- Show worker Job identity and link running rows to their Kubernetes Job.
- Add saved actor presets after real operator identities exceed the current
  smoke/operator split.
- Split `App.jsx` after the P2 cluster rollout; keep the split behavior-neutral.
