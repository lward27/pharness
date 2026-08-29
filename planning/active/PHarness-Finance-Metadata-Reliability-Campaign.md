# PHarness Finance Metadata Reliability Campaign

Status: active

Started: 2026-08-29

PHarness compiled baseline: `78fb3eb77cf082a9385a0bca1c4c2b06ed618f18`

PHarness release baseline: `bbe5c613958881b6237c5173d0dd7458eed7669c`

Product: `Finance` (`prod_01a043699d65721193e7e75d38654a2d`)

## Objective

Use six real, single-repository Repo Mode WorkItems to add Market, Sector, and
Industry metadata capabilities across the Finance product while producing
reliable characterization evidence for PHarness.

The campaign proves repeated clean execution after the Node and correction
repairs, exercises all four coding-ready application repositories, and records
one explicit backend-to-frontend merge order for a future DeliveryPlan design.
It is source-only: Pipeline, GitOps, Argo, deployment, and runtime-health claims
remain outside each Repo Mode WorkItem.

The upstream feature references are the yfinance public APIs for
[`Market`](https://ranaroussi.github.io/yfinance/reference/yfinance.market.html)
and
[`Sector` / `Industry`](https://ranaroussi.github.io/yfinance/reference/yfinance.sector_industry.html).
The initial scope does not include Screener, Lookup, Search, Calendars, or live
streaming.

## Starting repository state

| Repository | PHarness Repository | Initial `main` SHA | Initial readiness |
| --- | --- | --- | --- |
| `lward27/yfinance_wrapper` | `repo_01a04369f6387e6095aee4047ed2224f` | `56af994611396d886d1abefaa9745757981df834` | Contract/coding ready; refresh required for current runner tuple |
| `lward27/finance-frontend` | `repo_01a04369f1057e13a13c24233d4e278a` | `28e553510b1e0b82e3105c754a7019145829ab55` | Contract/coding ready at prior revision; refresh exact current SHA before use |
| `lward27/finance_app_database_service` | `repo_01a04369c7e37c63abd5adcad419ffc5` | `015f720176154921533b88af1526cbd5230e706a` | Contract/coding ready at prior revision; refresh exact current SHA before use |
| `lward27/scraper_manager` | `repo_01a04369fb627ac0bfd5a492ae348ca2` | `311d26be08f6ac54649b248b6b3378f06ada6e05` | Contract/coding ready at prior revision; refresh exact current SHA before use |

Every WorkItem must use the then-current full merge SHA, a current matching
readiness assessment, and fresh isolated capability verification. A readiness
claim from an older runner digest or repository SHA is not reusable.

## Product contract for this campaign

- Every WorkItem mutates exactly one Repository.
- Every source merge remains reviewed and manual at the provider boundary.
- A downstream WorkItem is created only after PHarness observes the upstream
  merge and the exact merge SHA is available.
- Context Repositories are read-only, registered Product bindings pinned to
  full SHAs, and must have deterministic discovery at those exact revisions.
- Agents may not change dependency locks or RepositoryContracts during this
  campaign. Any required dependency change stops for a separate reviewed
  prerequisite PR.
- Acceptance uses only commands declared by the active RepositoryContract.
- Do not manufacture a correction or budget pause. The substantial WorkItem is
  allowed one in-place budget extension or one controller-derived correction
  when the real execution evidence requires it.
- Record every preflight, action, Run, context pack, StageOutcome, correction,
  budget extension, ChangeSet, provider observation, merge provenance, and
  closure response.

## WorkItem sequence

### FRC-1 — Validated Market summary endpoint

Mutable Repository: `yfinance_wrapper`

Initial source: `56af994611396d886d1abefaa9745757981df834`

Intent:

- Add a pure, case-insensitive validator for the eight yfinance Market keys:
  `US`, `GB`, `ASIA`, `EUROPE`, `RATES`, `COMMODITIES`, `CURRENCIES`, and
  `CRYPTOCURRENCIES`.
- Add `GET /markets/{market_name}` returning the stable envelope
  `{market, summary, status}`.
- Instantiate `yf.Market` only after validation. Invalid names return stable
  HTTP 422 without an upstream call; upstream failures return a generic HTTP
  502 without leaking exception details.
- Preserve yfinance's documented limitation by allowing `status` to be null
  outside `US`.
- Add standard-library unit tests using mocks; do not use live network data.
- Document the endpoint and response contract in `readme.md`.

Acceptance: `unit-tests`, `compile-check`.

Purpose: the first clean Python reliability run in the new database generation
and the smallest independently useful slice of the requested feature.

### FRC-2 — Clean Market overview UI

Mutable Repository: `finance-frontend` at its then-current merge SHA.

Required context: the exact FRC-1 `yfinance_wrapper` merge SHA.

Intent:

- Add a typed API helper for `GET /markets/US`.
- Add a bounded Market Overview panel that renders useful summary fields,
  loading, empty, and error states without synthetic data.
- Keep the response adapter pure and test it with `node:test` fixtures.
- Do not add persistence, deployment configuration, or new dependencies.

Acceptance: `test`, `lint`, `build`.

Purpose: a clean Node run after the correction/workspace fixes exposed by the
previous frontend smoke.

### FRC-3 — Sector and Industry wrapper resources

Mutable Repository: `yfinance_wrapper` at the FRC-1 merge SHA.

Intent:

- Add validated, read-only Sector and Industry endpoints with explicit region
  handling and stable JSON envelopes.
- Normalize pandas/yfinance values through a bounded, deterministic serializer.
- Include overview, company lists, research reports, sector industries, and
  the documented top-performing/top-growth Industry fields when present.
- Distinguish invalid keys (422), no upstream resource (404), and sanitized
  upstream failure (502).
- Add exhaustive mocked endpoint and serialization tests plus documentation.

Acceptance: `unit-tests`, `compile-check`.

Purpose: the deliberately substantial exercise. Use the standard 48-turn,
400,000-token soft budget with the existing 100-turn/1,000,000-token hard caps.
If PHarness reaches an honest budget boundary or Verifier finds a material
defect, exercise one in-place extension or one same-workspace correction and
preserve the evidence.

### FRC-4 — Persisted finance metadata API

Mutable Repository: `finance_app_database_service` at its then-current merge
SHA.

Required context: the exact FRC-3 wrapper merge SHA.

Intent:

- Add relational models for market snapshots, sectors, industries, and their
  parent relationship with stable natural keys and update timestamps.
- Add idempotent batch upsert endpoints for scraper-manager.
- Add read endpoints for market summary, sector lists/details, and industry
  lists/details with bounded pagination and stable response models.
- Keep database access behind existing session patterns and test schema/API
  behavior without requiring a live production database.
- Document the source contract and explicitly identify any deployment-time
  migration prerequisite; Repo Mode does not execute it.

Acceptance: `unit-tests`, `compileall`.

Purpose: backend half of the explicit backend-to-frontend exercise. Its merge
SHA is a hard prerequisite for FRC-6.

### FRC-5 — Metadata ingestion orchestration

Mutable Repository: `scraper_manager` at its then-current merge SHA.

Required context:

- Exact FRC-3 `yfinance_wrapper` merge SHA.
- Exact FRC-4 database-service merge SHA.

Intent:

- Add HTTP-client methods for the approved wrapper and database endpoints.
- Add a bounded metadata refresh operation that retrieves Market, Sector, and
  Industry data and writes idempotent batches to the database service.
- Preserve retry, timeout, concurrency, error-sanitization, metrics, and
  shadow-mode conventions.
- Make partial failures observable and retryable without corrupting completed
  metadata.
- Add deterministic async tests with fake HTTP responses and update operator
  documentation.

Acceptance: `pytest`, `compileall`.

### FRC-6 — Persisted Sector and Industry experience

Mutable Repository: `finance-frontend` at the FRC-2 merge SHA.

Required context and merge order:

1. FRC-4 database-service merge SHA — mandatory backend contract.
2. FRC-3 wrapper merge SHA — source semantics.
3. FRC-5 scraper-manager merge SHA — ingestion behavior.

Intent:

- Add database API adapters for the merged sector/industry read contract.
- Add browsable sector and industry views with loading, empty, stale, and
  error states.
- Show the persisted relationship and available company/ranking metadata
  without inventing absent fields.
- Add pure response adapters and Node tests; preserve responsive behavior.

Acceptance: `test`, `lint`, `build`.

Purpose: frontend half of the explicit backend-to-frontend exercise. Creating
this WorkItem before PHarness observes FRC-4's merge is a campaign failure.

## Merge and context graph

```text
FRC-1 wrapper Market
  ├──> FRC-2 frontend Market overview
  └──> FRC-3 wrapper Sector/Industry
          └──> FRC-4 database metadata API
                 ├──> FRC-5 scraper ingestion
                 └──> FRC-6 frontend persisted metadata

FRC-6 source starts from the FRC-2 frontend merge and pins FRC-3, FRC-4,
and FRC-5 as read-only context. FRC-4 must merge before FRC-6 is created.
```

This graph is characterization evidence for a future DeliveryPlan. PHarness V1
must not infer or schedule it automatically.

## Per-WorkItem reliability scorecard

Record facts, not a synthetic score:

- Preparation duration and result before turn zero.
- Environment-discovery turns; target zero.
- Planner, Builder, Tester, and Verifier turns/tokens/active time.
- Tool failures, identical failures, recoveries, and approval wait time.
- Corrections, replans, and budget extensions with exact causes.
- Changed paths and acceptance-command results.
- ChangeSet-to-PR patch hash equality.
- Required-check observation and exact merge provenance.
- Final closure state and retained evidence identifiers.

After all six close, summarize distributions and recurring failure modes. Do
not claim a reliability rate from six hand-selected runs; use the evidence to
choose the next deterministic benchmark and product-control improvement.

## Stop conditions

Pause the campaign instead of working around PHarness when:

- A current readiness assessment cannot be produced for the exact SHA.
- A RepositoryContract or dependency lock must change.
- Source writer/observer identity verification fails.
- An agent requests coding-phase network or package installation.
- A context Repository lacks deterministic discovery at the exact merge SHA.
- Provenance, patch hash, provider checks, or merge observations disagree.
- A WorkItem enters a terminal failed state or a correction would silently
  change its intent.

## Completion evidence

Store a dated campaign report under `planning/evidence/smoke-results/` with:

- The six WorkItem and repository identities.
- Every source and context SHA.
- StageExecution, Run, ChangeSet, source-delivery, PR, check, and merge IDs.
- Budget/correction facts and acceptance results.
- Defects fixed during the campaign, separated from product changes.
- The observed merge-order/context handoff and implications for DeliveryPlan.

The campaign is complete only when all six source deliveries close, or when a
documented PHarness blocker terminates it honestly.
