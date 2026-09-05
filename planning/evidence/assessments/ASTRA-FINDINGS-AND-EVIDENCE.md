# PHarness findings and evidence record

Review date: 2026-09-04. [Review overview](ASTRA-REVIEW-OVERVIEW.md)

This record preserves the basis and limits of the review. Findings concern the inspected checkout and local design artifacts. They are not claims about the current live cluster or an independently verified deployed release.

## Provenance

| Input | Reviewed identity |
| --- | --- |
| Repository | `/Users/wardl/Personal/apps/pharness` |
| Branch | `codex/repo-mode-v1-product` |
| Tracked source | `12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d` |
| Primary design, selected by owner | `planning/pharness-ui-design-overhaul/Pharness Console.dc.html` |
| Design SHA-256 | `a24a8a442d08034d36165568f80cb141f6112d2b1f2fd2632a410e6664e9869c` |
| Design runtime | Adjacent `support.js`; SHA-256 `8fe7df74405f3c55f49b7249c74ea1397e65d07dea2b1bd3b4a489bec2e28cbe` |
| Secondary visual references | `ui/Pharness-project-view-UI-design-08-23-26.PNG` and `ui/Pharness-top-level-UI-design-08-23-26.PNG` |
| Implemented UI | Current `ui/src`; pre-existing `ui/dist` used only for a bounded fixture walkthrough, without a fresh build |
| Documentation | README, planning indexes, architecture and design contracts, active/implemented work, operational and evaluation evidence, UI guidance/QA, and untracked `living-docs` |

Tracked files were initially clean. Existing untracked inputs included `living-docs/`, the UI overhaul directory, the two design PNGs, four Mermaid SVGs, `temp.md`, and `scripts/__pycache__/`. They were preserved. The selected design is a local untracked artifact, so its identity is recorded separately from the Git revision.

The old console was initially opened and then identified by the owner as the old UI. It was removed as the visual authority; the explicitly selected HTML is the basis of the current-design report. This correction is reflected throughout the artifacts.

## Evidence labels and priority

- **Confirmed:** a current-source probe or isolated diagnostic reproduced the stated behavior.
- **Source:** the relevant control/data path is directly supported by inspected source; runtime consequences may remain inferred.
- **Design observation:** behavior or layout observed in the selected fixture prototype, not a deployed product.
- **Judgment:** a reasoned usability, product, or maintainability assessment; not a measured user-study result.
- **Limitation:** missing proof or an unexecuted check, not automatically a defect.

High priority means the issue threatens truthful decisions, ordinary workflow completion, or understanding of the product's supported boundary. Medium means material usability or maintainability friction. Low means bounded housekeeping. These are review priorities, not CVSS scores, release blockers for an uninspected deployment, or a sequenced implementation plan.

## Findings register

| ID | Finding | Priority | Evidence | Smallest useful response |
| --- | --- | --- | --- | --- |
| F01 | Source merge closure duplicates already-sealed Release/Observe executions | High | Confirmed isolated diagnostic plus source chain | Reuse one idempotent tail-sealing path; cover normal prior state |
| F02 | Verification success normalization does not reflect submitted risks/contradictions | High | Source; downstream operator effect inferred | Validate existing submission semantics and preserve caveats in normalized fields |
| F03 | Failed dashboard load can render “Nothing needs attention” | High | Current-source fetch probe and rendering logic; old-bundle fixture corroboration | Separate unavailable/loading from empty success |
| F04 | Global scope applies inconsistently across data and views | High | Current-source request capture | Make selection scope honest and consistent |
| F05 | Current cockpit example combines incompatible workflow states | High for design clarity | Selected prototype and embedded fixtures | Use one coherent current state and one useful action |
| F06 | Lifecycle illustration outranks the operator's next decision | Medium | Design observation and judgment | Prefer the existing compact treatment; move decision content first |
| F07 | Current prototype compresses primary content at narrow widths and uses non-semantic click rows | Medium | DOM measurements, visual observation, source | Stack existing fields and use native interaction elements |
| F08 | README/indexes/design artifacts disagree about current product and work | High for approachability | Documentation comparison against current source | Establish one current entry point with revision and scope |
| F09 | Lifecycle/navigation models conflict across selected design and contracts | Medium | Design and documentation comparison | Choose the applicable existing authority; reconcile names |
| F10 | Old UI creation defaults to a specific demo repository/task | Medium | Source and old-bundle walkthrough | Remove implicit example defaults; reuse selected validated context |
| F11 | Architecture boundary checker fails and dependency parsing aborts | Medium | Executed checker | Repair the existing guardrail; resolve explicit rule violations |
| F12 | Generated nested evaluation workspaces are tracked | Low | Tracked-file inventory | Preserve intentional evidence, ignore disposable outputs |
| F13 | Passing tests and historical coding eval do not prove the complete intended journey | High as a proof gap | Executed tests, fixture inspection, dated evaluation | Validate the bounded existing journey before expanding claims |
| F14 | “Read-only preflight” obscures temporary Job and persistence effects | Medium | UI, API, dispatch source | Name it readiness checking and describe actual effects |
| F15 | Existing UI exposes competing destinations and incomplete state filtering | Medium | Source and judgment | Center the current WorkItem; align existing filters and labels |
| F16 | Selected prototype does not cover unavailable/stale/empty states and includes decorative controls | High for design completeness | Prototype source and interaction | Specify truthful ordinary states for existing screens |

F05–F07, F09, and F16 are detailed in the [current design review](ASTRA-CURRENT-UI-DESIGN-REVIEW.md). F03–F04, F10, F14–F15 are in the [implemented UI review](ASTRA-IMPLEMENTED-UI-REVIEW.md). F01–F02 and F11–F13 are in the [code review](ASTRA-CODE-AND-RELIABILITY-REVIEW.md). F08 is developed in the [documentation review](ASTRA-DOCUMENTATION-REVIEW.md).

## Executed checks

| Check | Result | Meaning and limit |
| --- | --- | --- |
| Rust workspace tests, offline | **Passed: 408 top-level tests** | Tested source behavior passed; does not cover every journey or live provider/model behavior |
| All SQLite migrations applied to a fresh in-memory database | **Passed: 47 migrations, 57 application tables** | Fresh schema construction works; not a live upgrade or populated-database migration rehearsal |
| API module boundary check | **Failed** | Two size violations, one test wildcard import, then dependency-parser failure |
| Current UI dashboard source probe | **Confirmed failure coupling and inconsistent scope** | Stubbed HTTP responses; no live latency or authorization conclusion |
| Existing UI test command | **Could not run: `vitest: command not found`** | Local dependencies incomplete/unavailable; not a failing product test result |
| Existing UI bundle with local fixtures | **Bounded walkthrough completed** | Corroborates old-console presentation; bundle was not rebuilt from the reviewed source |
| Selected HTML prototype | **Rendered and inspected** | Fixture design only; no real external effects or backend state |
| Repo closure seam diagnostic in temporary source copy | **Failed as predicted: one test ran** | Reproduced uniqueness error after seeding the normal preexisting tail |

Rust command used:

```sh
CARGO_HOME=/private/tmp/pharness-cargo-home cargo test --offline --workspace --no-fail-fast
```

Per-target results: API 163; CLI 26; config 12; core 121; core-types integration 6; eval 2; Fireworks 13; runhost 15; store 28; worker 22. Nested fixture subprocess output is excluded from the 408 count. Doc-test targets contained no tests. No live Fireworks run was performed.

Boundary command and relevant output:

```sh
bash scripts/check-app-module-boundaries.sh
```

```text
module size limit exceeded: products.rs has 3959 lines (limit 3500)
module size limit exceeded: repo_mode.rs has 4777 lines (limit 3500)
repo_mode.rs:4443: use super::*;
wildcard imports and re-exports are forbidden under crates/pharness-api/src/app
ValueError: unbalanced use tree: a new ChangeSet and SourceDeliveryIntent.
```

The final error comes from prose inside a Rust string being captured by the import parser. The actual wildcard is inside tests. A current acyclic dependency-graph result was not obtained.

## F01 reproduction: normal prior state breaks the existing success fixture

### Source chain

1. [Writer dispatch](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-api/src/app/repo_mode.rs#L1239) calls `append_repo_audit`.
2. [The audit helper](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-api/src/app/repo_mode.rs#L2362) calls `seal_repo_inapplicable_tail`, which creates Release and Observe at sequence 1 if they have no effective outcomes.
3. [Merge observation](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-api/src/app/repo_mode.rs#L2047) calls `seal_source_delivery_closure` before updating terminal intent/WorkItem status.
4. [Closure](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-api/src/app/repo_mode.rs#L2225) checks only whether a source-delivery outcome already exists. On its first path it seals that outcome and then unconditionally creates the Release/Observe tail again.
5. [Store insertion](https://github.com/lward27/pharness/blob/12d36e97e7e31d01b3fcb7f1aedbb158d5f95c2d/crates/pharness-store/src/sqlite/repo_mode.rs#L257) uses an ordinary insert. Migration 0042 enforces uniqueness of `(work_item_id, stage_key, sequence)`.

### Diagnostic modification, isolated from the user's checkout

A `git archive HEAD` copy was created under a temporary directory. Only that copy was modified. The existing helper's visibility was widened so the existing integration-style test could call it:

```diff
-async fn seal_repo_inapplicable_tail(
+pub(in crate::app) async fn seal_repo_inapplicable_tail(
```

Immediately after `repo_delivery_fixture("success")` in the existing test, the real helper seeded the state produced earlier by the ordinary path:

```rust
let fixture = repo_delivery_fixture("success").await;
crate::app::repo_mode::seal_repo_inapplicable_tail(
    &fixture.state.store,
    &fixture.work_item_id,
)
.await
.unwrap();
```

The test's normal pre-merge and merge observations then ran unchanged. The diagnostic did not manufacture a conflicting raw database row or alter production behavior; it invoked the existing production helper to account for a stage the fixture had skipped.

Equivalent invocation from that temporary copy:

```sh
CARGO_HOME=/private/tmp/pharness-cargo-home \
cargo test --offline -p pharness-api \
  repo_mode_fake_provider_closes_only_after_fresh_checks_and_exact_merge \
  -- --nocapture
```

Observed result:

```text
running 1 test
called Result::unwrap() on an Err value:
ApiError { status: 500, message: "sqlite error: error returned from database:
(code: 2067) UNIQUE constraint failed: stage_executions.work_item_id,
stage_executions.stage_key, stage_executions.sequence" }
test result: FAILED. 0 passed; 1 failed; 162 filtered out
```

An earlier attempt used an incomplete exact test name and ran zero tests. That invocation is not counted as validation; the one-test failing run above is the reproduction.

This establishes a first-closure failure with already-sealed tail state. It does not establish permanent unrecoverability: the source-delivery outcome is persisted before the duplicate insertion, and a repeated callback can enter a different path. The original source remained unchanged.

## F03/F04 source probe

The probe imported current `http.js`, `dashboard.js`, and `workItems.js` through local data-URL modules. It replaced the operator-name setter with a no-op and stubbed `fetch`, leaving URL construction and dashboard error handling unchanged. It supplied this explicit selection:

```json
{
  "environment": "staging",
  "namespace": "apps-staging",
  "repo": "https://example.test/repo.git",
  "branch": "review",
  "productionImpacting": "false"
}
```

With empty successful responses, it captured 18 calls. Sixteen were direct reads; two were unscoped fallback Flow-root lookups. Relevant examples:

```text
/api/runs?limit=25&namespace=apps-staging&repo=https%3A%2F%2Fexample.test%2Frepo.git&branch=review&production_impacting=false
/api/approval-gates?limit=50&resource_namespace=apps-staging
/api/work-items?limit=100&include=operator_state&target_environment=staging
/api/triage
/api/triage/summary
/api/change-sets?limit=1
/api/work-plans?limit=1
```

A second call changed only the incident endpoint to return 500. The complete dashboard promise rejected with `500 Internal Server Error: {"error":"fixture incident endpoint unavailable"}`. All other stubbed responses, including health, remained successful. This verifies client failure coupling; the source rendering inspection establishes why absent dashboard data becomes an empty attention state.

The independently loaded WorkItems list's default query was:

```text
/api/work-items?include=operator_state&limit=25&offset=0
```

No claim is made about cross-tenant access, API authorization, real service latency, or the number of requests for every nonempty scenario.

## Design observation record

The selected HTML was served with its adjacent runtime on a loopback-only static server. Default light theme and dark theme were inspected. Triage, WorkItems/cockpit, lifecycle alternatives, Runs, Gates, Tools content, and Audit informed the review through rendered and source inspection.

| Observation | Evidence |
| --- | --- |
| Default lifecycle variant | `data-props` selects `laminae`; the fallback state alone would misleadingly suggest `beads` |
| Next decision below chart | At the ordinary 883 × 1019 CSS-pixel viewport, the next-step card begins near the bottom; compact linear variant brings it upward |
| Narrow-layout failure | At measured 433 × 938 CSS pixels, primary Triage title column measured approximately 2.27 pixels; page scroll width still equaled viewport width |
| Cockpit tabs | Clicking Attempt changed active styling but retained the same cockpit headings/content; no tool/model stream appeared |
| Keyboard semantics | Primary click rows are `div` elements, without native button/link roles or focus behavior |
| State contradiction | Delivery “not started” coexists with active Tekton observation; stage-three staging gate blocks earlier ChangeSet capture |
| Permanent status | “Live” is fixture markup rather than a backend-derived condition |
| Scope of accessibility conclusion | Layout and semantic issues found; no formal contrast, full keyboard, or screen-reader conformance result |

The browser's viewport override initially produced CSS dimensions different from the requested values. Reported sizes above are measured `window.innerWidth/innerHeight`, not assumed device sizes. The viewport override was reset after inspection.

## Coverage and remaining limits

The review traced composition, attempt execution, stage evidence, source delivery, lifecycle closure, client data loading, current visual design, and documentation authority. It sampled the broader typed delivery paths and deployment configuration to assess boundaries. It was not a line-by-line security audit of every tool or a performance benchmark.

No application source or deployment configuration was edited. No dependencies were installed. There was no live cluster access, provider mutation, deployment, automatic merge, paid model evaluation, or external communication. Diagnostics used temporary/in-memory state; ordinary build/test output remained outside tracked source changes.

The following remain unverified: current deployed behavior and release provenance; realistic live coding success rates for this revision; fresh UI build and complete UI automated tests; populated-database upgrade behavior; continuous end-to-end autonomy through deployed product verification; and usability with representative new users.

Those limits do not invalidate the confirmed findings. They prevent a local review from being presented as broader acceptance than it actually performed.
