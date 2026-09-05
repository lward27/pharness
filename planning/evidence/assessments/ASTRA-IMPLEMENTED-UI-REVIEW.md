# Review of the implemented PHarness UI

Date: 2026-09-04. Source revision: `12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d`. [Overview](ASTRA-REVIEW-OVERVIEW.md) · [Selected current design review](ASTRA-CURRENT-UI-DESIGN-REVIEW.md) · [Evidence](ASTRA-FINDINGS-AND-EVIDENCE.md)

## Scope: this is the older console

The owner correctly identified the existing React console as the old UI. This report examines its implemented behavior because it remains relevant to the code review and because adopting a new visual design will not automatically correct its data handling. It is not used as the primary reference for visual recommendations.

Evidence combines inspection of `ui/src`, direct execution of current API-loading functions with stubbed responses, and a browser walkthrough of the pre-existing `ui/dist` bundle against a local fixture server. The bundle was not rebuilt; source-level findings are identified independently. No live service was exercised.

## F03 — An unavailable queue can look like a healthy empty queue

**Confirmed source behavior:** `loadDashboardData` loads the dashboard through one `Promise.all`. A failure from an unrelated required endpoint rejects the entire load. A direct source probe returned a healthy `/health` response but made `/api/incidents` return 500; the complete dashboard load rejected.

`TriageView` treats missing dashboard data as an empty set, renders zero counts, and chooses “Nothing needs attention.” It has no distinct loading or error branch. The local browser fixture corroborated this combination: the attention area showed empty reassurance while the shell indicated a connectivity problem.

Evidence: [dashboard loading](../../../ui/src/api/dashboard.js#L19), [Triage rendering](../../../ui/src/views/TriageView.tsx#L12).

**Why it matters:** this is an operational truth problem. An operator could reasonably read the largest content area as “there is nothing to do” when the system cannot establish that fact.

**Smallest useful correction:** render a clear unavailable/loading state before rendering the empty state. Preserve previously loaded data as explicitly stale if that behavior is used. Isolate unrelated read failures where practical within the existing loading structure. This does not require a new backend, a new client framework, or a universal caching layer.

## F04 — Global scope does not mean the same thing across the screen

A direct probe supplied environment, namespace, repository, branch, and production-impact filters. Requests carried different subsets:

| Data | Scope actually sent by the shared loader |
| --- | --- |
| Runs, run summary, tool approvals, audit | Namespace, repository, branch, production-impact flag |
| Gates, WorkPlans, ChangeSets, incidents, remediation plans, observations | Namespace only |
| Dashboard WorkItems | Environment only |
| Triage and Triage summary | No scope parameters |
| Automatically chosen Flow root | First ChangeSet or WorkPlan, unscoped |

The WorkItems list also has its own default filters and independently loads `/api/work-items?include=operator_state&limit=25&offset=0` without inheriting the global selection.

Evidence: [shared loader](../../../ui/src/api/dashboard.js#L19), [default Flow lookup](../../../ui/src/api/dashboard.js#L10), [WorkItems list](../../../ui/src/views/WorkItemsListView.tsx), [WorkItems client](../../../ui/src/api/workItems.js).

**Interpretation:** a prominently displayed global scope can imply a stronger selection boundary than the data obeys. This is not evidence of an authorization bypass; it is inconsistent filtering and potentially misleading context.

**Simplification:** either apply a supported scope consistently to the relevant views or label the selector's actual scope locally. Do not display a global selection that silently ceases to apply. Load an explicitly selected Flow rather than fetching an arbitrary unrelated root during every dashboard refresh.

The source probe issued 18 requests for an empty dashboard, including two default-Flow lookups. That establishes avoidable coupling and work. It does not establish a measured latency problem.

## F10 — Creation asks the user to start from someone else's task

`WorkItemNewView` prepopulates a yfinance-specific repository, title, acceptance content, and a fixed production target. Several scope fields are read-only. The first step requires a full source SHA before the user has explained the desired change. Submission uses the older WorkItem endpoint, not the new product-scoped Repo Mode flow.

Evidence: [new WorkItem state and form](../../../ui/src/views/WorkItemNewView.tsx#L8), [client submission](../../../ui/src/api/workItems.js).

**Judgment:** this is effective demonstration scaffolding but a poor default product experience. It asks a new user to recognize and undo the author's example. The full SHA is a legitimate execution input; making it the first task is an approachability choice, not a safety requirement.

**Simplification:** keep sample content in explicit examples, start ordinary intent fields empty, and reuse already selected validated source context where it exists. Keep an exact revision review before execution. Do not implement the entire future onboarding product merely to remove hard-coded demonstration defaults.

## F14 — “Read-only preflight” understates the action

The creation UI offers “Run read-only preflight.” Its capability checks call verification endpoints that can create short-lived Kubernetes Jobs, poll them, delete them, and persist verification results. The reviewed code includes repository/runtime capability checks and controlled network activity.

Evidence: [preflight handler](../../../ui/src/views/WorkItemNewView.tsx#L72), [verification endpoint](../../../crates/pharness-api/src/app/system.rs#L411), [verification dispatch](../../../crates/pharness-api/src/dispatch.rs#L787).

This does not mean the check performs an unbounded product mutation. It means “read-only” is ambiguous about infrastructure effects.

**Correction:** use “Check readiness” and briefly state that PHarness runs temporary checks in the selected environment. Preserve exact effects in the preview/details. Better labeling is enough; another approval mechanism is not implied by this finding.

## F15 — Existing information is spread across competing routes

The shell exposes ten destinations grouped under Operate, Govern, Investigate, and Platform Status. Flow and WorkPlan routes also remain in the code. A WorkItem can lead toward its overview, active attempt, delivery details, gates, run artifacts, or audit records, each with overlapping summaries.

The direction toward WorkItem-centered triage is sound. The problem is that the user must still infer which view is authoritative for the current question. The default WorkItem list mixes current and terminal records and exposes a status menu that omits some newer backend states, including awaiting approval, waiting external, and failed.

Evidence: [shell](../../../ui/src/App.jsx), [WorkItem list](../../../ui/src/views/WorkItemsListView.tsx), [WorkItem detail](../../../ui/src/views/WorkItemDetailView.tsx), [workspace routing](../../../ui/src/lib/runWorkspace.ts).

**Simplification:** make the current WorkItem explanation the ordinary destination; retain Runs and Audit for investigation. Use existing filters to distinguish current work from history and align labels with actual server states. Do not add another summary surface to reconcile the existing summaries.

## Secondary concerns worth carrying into the redesign

- `WorkItemDetailView` converts some Flow/rollback loading failures to `null`. Without an unavailable explanation, missing information can appear indistinguishable from inapplicable information.
- `LifecycleReviewDrawer` declares a modal dialog and handles Escape, but does not implement the complete focus entry, containment, and restoration behavior expected of a modal. This is a source finding, not a completed accessibility audit.
- The interface repeats engineering language where the user needs the effect of an action. Improve labels without flattening tool authority and lifecycle decisions into one generic “Approve.”
- The existing React Triage rows are native buttons. Preserve useful semantics like these when translating the prototype, whose rows are clickable `div` elements.

Evidence: [detail loading](../../../ui/src/views/WorkItemDetailView.tsx#L128), [review drawer](../../../ui/src/views/LifecycleReviewDrawer.tsx), [Triage buttons](../../../ui/src/views/TriageView.tsx#L34).

## What this means for the new design

A visual replacement can improve hierarchy while inheriting the same unavailable-data and scope bugs. These should be explicit acceptance conditions for any targeted UI cleanup: unavailable must not mean empty, scope must match the data, and primary actions must have a clear owner and effect.

The UI test suite was attempted but could not run because `vitest` was unavailable in the existing local dependency installation. No dependencies were installed, and no fresh build or full Playwright run was claimed. The behavioral findings above rest on the cited source and isolated probes, with browser corroboration limited to the existing bundle.
