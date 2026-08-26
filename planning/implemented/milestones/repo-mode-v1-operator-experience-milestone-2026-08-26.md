# PHarness Repo Mode V1 Operator Experience Milestone

Status: completed and deployed

Baseline: `448c65259ed01de509336d2fa83382678a4be737`

Deployed source at planning time: `a6c7c39e250ebfdb6104f9050de3abc4ba4e7c93`

Governing contract:
[`../../design/repo-mode-v1-screen-contract.md`](../../design/repo-mode-v1-screen-contract.md)

## Outcome

Replace the legacy delivery-centric console with the approved Product-oriented
Repo Mode V1 operator experience. An operator must be able to create or select
a Product, register and onboard a Repository, create and follow a WorkItem,
review controller-sealed StageOutcomes, perform the exact current action, and
observe source delivery and closure without reconstructing state from unrelated
pages or raw events.

The primary navigation is Overview, Products, Repositories, WorkItems, Agents,
Releases, Insights, and Settings. Repo Mode presents Discover, Plan, Implement,
Test, Verify, and Source Delivery. Release and Observe remain explicitly
inapplicable and no runtime state is manufactured.

## Scope and invariants

- Add backward-compatible portfolio, Product, Repository, WorkItem, Run,
  evidence, search, and action read models using existing durable records.
- Add no database migration and do not change controller, trust,
  authorization, or lifecycle semantics.
- Add `features.repoModeV1.uiEnabled`, default false, and retain the current
  console as the rollback surface until the complete journey passes.
- Preserve legacy `mode=null` WorkItems, the full-SDLC controller, preview/apply
  separation, state hashes, actor/reason attribution, manual merge, and every
  external-effect boundary.
- Move Triage, Flow, global approvals, incidents, and remediation out of
  primary navigation while preserving contextual and historical deep links.
- Keep current state primary and put completed, superseded, replanned, and
  prior execution records under explicit History.
- Reuse the existing live Run console and lifecycle review confirmation rather
  than weakening their execution and approval protections.
- Every visible state is API-backed. Never synthesize Product health, autonomy,
  agent swarms, capability availability, Release, deployment, or runtime state.
- Defer Ask PHarness, Connected Mode expansion, Product Graph, broad
  AgentProfile editing, and new mutation APIs.
- Preserve user-owned untracked planning diagrams, screenshots, `temp.md`, and
  unrelated work.

## Implementation sequence

1. Characterize the exact release routes, API payloads, screen states, tests,
   and known completed Repo Mode delivery defect.
2. Add read-only aggregation and evidence retrieval contracts, enriched
   server-derived action descriptions, and safe-advance eligibility.
3. Add the flagged typed hash router, route-owned data loading, search,
   responsive shell, and standard state presentation.
4. Implement Organization Overview and Product list/detail.
5. Implement Repository list/detail, registration, onboarding, readiness, and
   exact onboarding action review.
6. Implement Product-scoped WorkItem creation plus mode-aware WorkItem
   Overview, Current Stage, Stage Outcomes, Delivery, Evidence, and History.
7. Implement Agents, Releases, Insights, Settings, and contextual legacy route
   compatibility.
8. Add deterministic API, component, desktop/phone, accessibility, visual, and
   real server-backed browser acceptance.
9. Run all release gates, merge, build API/UI/runner from one SHA, pin immutable
   digests, deploy with the flag off, then enable and verify the cutover.

## Acceptance gates

- Organization and Product rollups use complete server-filtered data and state
  explicit denominators and evidence freshness.
- Repository registration, discovery, proposal, exact diff, source PR,
  manual-merge wait, validation, readiness, and capability axes are usable in
  one comprehensible UI path.
- Product-scoped WorkItem creation accepts only registered Repository IDs,
  immutable SHAs, contract-declared acceptance names, and bounded budgets.
- WorkItem detail exposes one recommended next action and separates Current
  Stage, effective StageOutcomes, source Delivery, typed Evidence, and History.
- A completed Repo Mode WorkItem shows successful Source Delivery and
  inapplicable Release/Observe without the legacy `0/5` warning.
- Legacy full-SDLC WorkItems retain their existing delivery behavior and
  protections.
- Empty, loading, unavailable, stale, waiting, blocked, failed, completed, and
  historical states have desktop and phone-width coverage with accessible
  labels independent of color.
- The real PHarness API and controller, using a temporary SQLite store and test
  provider/worker adapters, support the complete browser journey without
  intercepting browser requests with fixture JSON.
- `cargo fmt --check`, workspace tests, all-target Clippy with warnings denied,
  UI production build, Vitest, full Playwright, accessibility checks, Helm
  lint/template/schema, changed-resource server dry-run, immutable-manifest
  inspection, and rendered `:latest` scans pass.

## Rollout and completion evidence

- Build API, UI, and Python runner from the same merged implementation SHA.
- Record source revision, three image digests, OCI revision/platform metadata,
  release-pin commit, Argo revision, live Pod image IDs, and readiness.
- Deploy first with the new UI flag disabled and verify the legacy fallback.
- Enable the new shell only through the GitOps release values and verify every
  top-level route, the completed yfinance Product/onboarding/WorkItem evidence,
  responsive behavior, and absence of the false legacy delivery warning.
- Preserve before/after API responses, screenshots, route behavior, and action
  transitions as characterization evidence.
- Rollback consists of disabling the UI flag through GitOps; no evidence
  deletion or database down-migration is permitted.

## Completion evidence

- Implementation PR: `#161`; implementation commit
  `f4127e1d73d6d0b04760e8e36279a5ec9464975d`; merged source revision
  `069ce56078da1081c01570844e792bda8a95c9ee`.
- Flag-off release PR: `#162`; release commit
  `cbfccd079ef082a9425546399f1ac1cc7d109d0e`.
- Flag-enable PR: `#163`; deployed GitOps revision
  `7c27b6e29905a17c8aeb7eb63ee386646738fc04`.
- Runtime digest:
  `sha256:1bb00ca024d097e0c1f03c08d0dc9c8e43902a7204926fb6550aa44ea9f931e4`.
- UI digest:
  `sha256:57ea5473989cc158faf82a0e7dbbb66d2bf58d6552ed5e5e6dfda420c792eec2`.
- Python runner digest:
  `sha256:e641183ef11d76ffc67074142a945a546d1f110153c8920ca985753c9ebaecdf`.
- All three OCI artifacts report `linux/amd64`, the merged source revision,
  and the PHarness source URL.
- The online SQLite backup
  `/data/backups/pharness-before-069ce560-20260826T0335Z.db` completed with
  `PRAGMA integrity_check = ok` before rollout.
- Argo reached `Synced/Healthy` at the flag-off and flag-enabled revisions.
  Live API, proxy, and UI Pod image IDs matched their declared digests.
- API and UI both reported source revision `069ce560...` with platform version
  alignment. The flag-off browser check rendered the legacy shell; the
  flag-enabled check rendered the approved eight-item Repo Mode hierarchy.
- The complete yfinance Repo Mode WorkItem
  `witem_01a03a99600479a095e3533c5f61c7b6` renders Source Delivery as
  `succeeded` and Release/Observe as controller-recorded `inapplicable` with no
  legacy delivery warning.
- All eight top-level routes loaded without alerts. Phone-width acceptance
  showed one content column, a hidden slide-over navigation, and no horizontal
  overflow.
- Deterministic gates passed: Rust formatting, all workspace tests, all-target
  Clippy with warnings denied, UI production build, 36 Vitest tests, 81
  Playwright tests with one intentional mobile skip, accessibility assertions,
  Helm lint/template/schema, rendered immutable-image scans, Kubernetes
  server-side dry-run, and OCI platform/revision inspection.
- Detailed release evidence is recorded in
  [`../../evidence/smoke-results/repo-mode-v1-operator-experience-release-2026-08-26.md`](../../evidence/smoke-results/repo-mode-v1-operator-experience-release-2026-08-26.md).

## Corrective completion release

A contract-level audit after the first cutover found that the durable read
models and visual shell existed, but several approved operator projections
were still incomplete. Corrective implementation PR `#165` added complete
WorkItem outcome history and ownership, Product binding revisions, evidence
and audit rollups, genuine connected Release data, actionable onboarding
attention, server-authored authorization boundaries, structured evidence
presentation, route-owned AgentRun pagination, and deterministic real-server
screenshots. Its merged source revision was
`c187cc58ff9f19ad0e9a0695779266706a555ccf`.

The first corrective flag-off/enable rollout (`#166` and `#167`) proved the
new projections live, but the live Organization Overview exposed a horizontal
overflow caused by long durable owner and action text. That rollout was not
accepted as the stable completion. PR `#168` added an explicit desktop/phone
overflow regression and clipped only presentation text while preserving the
durable values. The accepted source revision is
`c3638afa6d14819adca82006630858935c616172`.

- Final flag-off release PR/revision: `#169` /
  `8662e27e1d1ad944dcc93af75d37b398adc03d95`.
- Final flag-enable release PR/revision: `#170` /
  `05f0c079c43c8f3491ba76c451356d1d55324767`.
- Runtime digest:
  `sha256:5a304073220f370433f615e73ea914a36e306e4cbd61a64a55562ff349135c7f`.
- UI digest:
  `sha256:78930c2bff88497e72927c788ffbdad23f15d6c38126bc9296028b043fb1e3f6`.
- Python runner digest:
  `sha256:62474fc976a185b26fac6bd3439d58f6fe88f0c8bb4e5a76ee06f06c2a29a921`.
- All three artifacts report `linux/amd64`, the accepted source revision, and
  `https://github.com/lward27/pharness` as their OCI source.
- Pre-rollout online backup:
  `/data/backups/pharness-before-c187cc5-20260826T230516Z.db`, 7,221,248 bytes,
  integrity `ok`. The responsive correction added no migration or persistence
  change, so no database rollback is required.
- Full Rust formatting, workspace tests, and all-target Clippy passed on the
  corrective implementation. The accepted source additionally passed the UI
  production build, 43 Vitest tests, and the complete Playwright suite: 85
  passed with one intentional mobile-only skip.
- Final live browser acceptance loaded all eight routes at desktop and phone
  widths with no alert, page exception, or horizontal overflow. The completed
  yfinance WorkItem still rendered successful Source Delivery, two
  controller-recorded inapplicable downstream stages, and no legacy warning.
- Final platform readiness reported matching API/UI revisions and immutable
  digests. Expired isolated capability evidence remains honestly `stale` and
  continues to gate new work until refreshed.
