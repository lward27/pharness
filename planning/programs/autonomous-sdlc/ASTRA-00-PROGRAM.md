# ASTRA: Autonomous SDLC in lucas_engineering

Status: approved implementation program; M01 accepted; M02 and M03 active. Approved by the owner on 2026-09-04.
Baseline: PHarness main `c36b46aceb72f3d7097bc0bdee74810c745f7c0c`; GitOps main `fa27225c4c33b710ce24708e17fd39ac05ab6aeb`.
Current compiled PHarness release: `83a2689c877a3f48688d1d457c34e83474698c46`.
These are starting observations, not permanent latest-version claims.

## Product promise and authority

Retire Repo Mode as a separate product experience while preserving its discovery,
coding, evidence, and source-delivery implementation. A bounded user request should
progress through tested source, immutable build, staging, human production approval,
and verified runtime behavior. This direction is approved; complete hosted autonomy
is not implemented or accepted at this baseline.

The owner authorizes implementation, GitOps work, kubectl on `lucas_engineering`,
local Docker builds, and necessary bounded validation. That implementation authority
does not replace the human production-approval event required in runtime acceptance.

- User requests initiate WorkItems. LGTM verifies and recovers the same work; it does
  not initiate incident campaigns.
- Discovery, planning, coding, tests, source merge, builds, and staging are automatic
  under a recorded, bounded authorization.
- Human approval binds digest, GitOps diff, staging evidence, target, and healthy
  rollback baseline **before production GitOps merge**. Argo already auto-syncs.
- One bounded safe rollback may follow an approved release. Incompatible/destructive
  changes stop. Missing telemetry alone is not proof that rollback is appropriate.
- One mutable application repository per WorkItem; separately authorized GitOps
  updates are delivery effects. Related application work is sequential and pins context.
- Use and qualify the existing gateway/Coding Reliability V2 path. GPT-6 Astra Max
  is the program implementation agent, not a newly mandated runtime provider.
- Preserve existing token/turn/deadline/retry limits and one bounded correction.
  Record usage; never increase budgets or switch providers silently.
- Keep SQLite, the single-writer API, current workers/effect boundaries, and Lamina.
  Defer generic adapters, multi-repo orchestration, incident initiation, new navigation,
  workflow builders, and native Codex-host expansion until this program is accepted.

## Reading and execution

Read the [baseline addendum](../../evidence/assessments/ASTRA-CURRENT-BASELINE-ADDENDUM.md),
[product vision](../../design/product-vision-and-boundaries.md), then the next eligible
milestone below. The original [review](../../evidence/assessments/ASTRA-REVIEW-OVERVIEW.md)
is dated evidence for `12d36e9`, not a current defect list.

Use a fresh `codex/` worktree from verified main; preserve existing saved checkouts
and untracked design/review files. Before each slice, record HEAD, remote main, status,
relevant deployment identity, and evidence freshness. If upstream moves, revalidate
affected assumptions rather than replaying stale changes.

Each milestone is the implementation plan for its bounded goal. Update its status
and this table after validation. Keep accepted numbered documents in place; link
from active/implemented indexes instead of duplicating them. Evidence belongs under
`planning/evidence/autonomous-sdlc/` with an `ASTRA-MNN-` prefix. All new program,
assessment, and acceptance Markdown uses `ASTRA-`.

| Milestone | Document | Status | Dependencies |
| --- | --- | --- | --- |
| M01 | [Current baseline and authoritative documentation](ASTRA-01-BASELINE-AND-DOCUMENTATION.md) | accepted | None. This is the first milestone. |
| M02 | [Finance platform readiness](ASTRA-02-FINANCE-PLATFORM-READINESS.md) | active | M01. May proceed independently of M03. |
| M03 | [Evidence and code integrity](ASTRA-03-EVIDENCE-AND-CODE-INTEGRITY.md) | active | M01. May proceed independently of M02. |
| M04 | [Coding reliability qualification](ASTRA-04-CODING-RELIABILITY-QUALIFICATION.md) | planned | M03. An external qualification blocker does not stop independent M02/M05 preparation. |
| M05 | [Unified hosted SDLC contract](ASTRA-05-UNIFIED-SDLC-CONTRACT.md) | planned | M02 bindings and M03 integrity. Code preparation may proceed while an unrelated TLS prerequisite is blocked; acceptance still requires usable bindings. |
| M06 | [Durable autonomous controller](ASTRA-06-DURABLE-AUTONOMOUS-CONTROLLER.md) | planned | M05. |
| M07 | [Exact-source delivery and real builds](ASTRA-07-SOURCE-DELIVERY-AND-BUILDS.md) | planned | M04 and M06. |
| M08 | [Staging and runtime verification](ASTRA-08-STAGING-AND-RUNTIME-VERIFICATION.md) | planned | M07 and usable M02 staging bindings. |
| M09 | [Production approval and bounded rollback](ASTRA-09-PRODUCTION-PROMOTION-AND-ROLLBACK.md) | planned | M08. |
| M10 | [Console convergence and polish](ASTRA-10-CONSOLE-CONVERGENCE-AND-POLISH.md) | planned | May begin after M05; closes against M09 behavior. |
| M11 | [Finance end-to-end acceptance](ASTRA-11-FINANCE-END-TO-END-ACCEPTANCE.md) | planned | M09 and M10, with all earlier gates satisfied. |
| M12 | [Operations and program closeout](ASTRA-12-OPERATIONS-AND-PROGRAM-CLOSEOUT.md) | planned | M11 and all earlier acceptance gates. |

Next eligible: M02 and M03 (independent). M01 is accepted; see its committed evidence.
A real external blocker may suspend its dependent work but never waive its gate.
Continue eligible independent work and ask only for missing authority/credentials
or a decision with material downstream consequences.

## Lifecycle and interface invariants

Canonical lifecycle: `discover -> plan -> implement -> test -> verify ->
source_delivery -> release -> observe`. Release exposes build, staging, and
production evidence independently. Code verification and runtime verification
must not share an ambiguous success label.

Use existing WorkItem and stage/effect resources. Add a versioned hosted workflow
policy snapshot to creation/readiness, not another workflow root. Bind exact
delivery configuration, allowed automatic actions, budgets, mutable source,
read-only dependencies, and rollback permission. Existing operator projections
explain current state and one useful action. GET and navigation never dispatch work.

Use additive migrations from the verified current schema (0051 at this baseline).
Preserve the Finance generation and retention/audit history. Compatible readers
ship before hosted writes; record the minimum compatible rollback release.
Legacy work finishes under its pinned source-only contract. Preserve source-only
success history and inapplicable stages. New hosted work cannot close at source merge.

The durable API controller owns scheduling, claims, and retry-safe dispatch.
One coding run and per-repository/environment delivery serialization are the initial
limits. Pause stops new development/promotions while observation and already-authorized
release recovery continue. Do not replace this runtime with browser clicks or Codex tasks.

## Acceptance contract

A successful hosted WorkItem connects acceptance -> tested source -> merge commit ->
Tekton run -> image digest -> staging GitOps/reconciliation/verification -> human
production approval -> production GitOps/reconciliation/imageID -> runtime verification.
The same digest moves between environments without rebuilding. Recovered service means
failed work with successful recovery, not successful delivery.

Verification defaults are five-minute baseline/staging and ten-minute production
windows with bounded read-only probes, exact identity, fresh app-scoped metrics/logs,
and traces for instrumented applications. Preserve stricter existing requirements.
No data is inconclusive; cluster inventory is not service verification. Record latency
without inventing an unsupported SLO. Do not fabricate frontend traces.

Coding qualification preserves the frozen 24-task suite: two independent runs, each
>=21/24 first pass and >=23/24 after one correction, each stack >=6/8 and >=7/8,
all existing protocol/stage gates, and zero hidden-test false passes/policy violations.
A provider or account failure cannot be converted into a relaxed acceptance criterion.

M11 uses two meaningful Finance maintenance requests: nonblocking yfinance upstream
work and frontend deployment-time runtime configuration. Already-implemented market
features and noops do not count. The implementation agent cannot supply their patches
or manually tick normal transitions. Production approvals are real human decisions.
Failure injection belongs in tests/staging, not production.

M12 requires 24-hour unattended evidence, interrupted-workflow recovery, accurate
operator documentation, owner review, and all required immutable release identities.
A healthy deployment, elapsed time, or passing unit suite alone cannot close the program.

## Review coverage ledger

Initial dispositions must be read with the baseline addendum. Closure requires
current evidence, including for fixes that landed before this program.

| Finding | Initial disposition | Owner | Required closure |
| --- | --- | --- | --- |
| F01 duplicate source closure | Fix landed upstream; revalidate | M03, M06 | Normal/repeated closure and retry-safe completion pass |
| F02 verifier caveats | Still applicable | M03, M10 | Risks retained; contradictions cannot normalize to unconditional success |
| F03 unavailable looks empty | New resource states improved; retiring fallback remains | M10 | Failed-load and stale-state UI tests across reachable surfaces |
| F04 scope disagreement | Legacy exposure requires validation | M10 | Scope, filters, counts, cancellation, and data agree |
| F05 incoherent cockpit example | Prototype concern; validate implementation | M10 | One coherent current state/action, no fictional fixture state |
| F06 lifecycle dominates decision | Subjective hierarchy concern | M10 | Decision-first desktop/phone walkthrough |
| F07 narrow/keyboard interaction | Prototype concern; current implementation partly improved | M10 | Responsive/native keyboard interaction evidence |
| F08 documentation drift | Confirmed; indexes partly repaired upstream | M01, M12 | Current reality/direction/history have unambiguous entry points |
| F09 competing lifecycle/navigation | Direction superseded by approved program | M05, M10 | Canonical lifecycle and established navigation agree |
| F10 demo defaults | Legacy creation exposure | M10 | Validated context only, no implicit demo input |
| F11 failed architecture guardrails | Still applicable | M03 | Parser and existing boundary/size checks pass |
| F12 tracked generated workspace | Still applicable | M03 | Disposable output untracked; intentional evidence retained |
| F13 complete-journey proof | Not accepted | M04, M07–M09, M11–M12 | Qualified coding plus two autonomous real releases and operational proof |
| F14 read-only readiness wording | Still applicable | M05, M10 | Actual Job/persistence effects accurately explained |
| F15 competing destinations/filters | Lamina improved; remaining routes need convergence | M10 | One operational path with honest scoped history |
| F16 missing ordinary states/decorative controls | Prototype concern; partly implemented upstream | M10 | Complete state matrix and no unsupported controls |

## Goal-mode execution prompt

Read this document and the next eligible numbered milestone. Inspect current source,
remote main, worktree state, and relevant release evidence. Execute the approved slice,
run meaningful checks, record exact results and limitations, commit implementation and
evidence, then update milestone/finding status. Continue eligible work until acceptance
or a concrete external dependency requires input. Never weaken a gate, expand production
authority, erase history, or count a manually completed step as autonomous success.

