# ASTRA: Autonomous SDLC in lucas_engineering

Status: approved implementation program; M01–M03 accepted; M04 active; M05/M06 compatible reader deployed; hosted creation and end-to-end gates remain open. Approved by the owner on 2026-09-04.
Baseline: PHarness main `c36b46aceb72f3d7097bc0bdee74810c745f7c0c`; GitOps main `fa27225c4c33b710ce24708e17fd39ac05ab6aeb`.
Current compiled PHarness release: `48c77b7b4438d621ff9563b913857bcf771f1800`, observed
2026-09-05 at 16:19 UTC through release commit `0bc84048e0d8817c6451e6f83dfcf250a17ab3b5`.
Live schema 0053 and unchanged Finance history were independently verified at 16:20 UTC.
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
  rollback baseline **before production GitOps merge**. Frontend and PHarness already auto-sync;
  yfinance is currently manual-sync and must be aligned in M09 before hosted promotion.
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
| M02 | [Finance platform readiness](ASTRA-02-FINANCE-PLATFORM-READINESS.md) | accepted | M01. May proceed independently of M03. |
| M03 | [Evidence and code integrity](ASTRA-03-EVIDENCE-AND-CODE-INTEGRITY.md) | accepted | M01. May proceed independently of M02. |
| M04 | [Coding reliability qualification](ASTRA-04-CODING-RELIABILITY-QUALIFICATION.md) | Builder passed; onboarding, Planner and Test Diagnosis failed; harness contract corrections and remaining stages open | M03. Qualification blockers do not stop independent implementation. |
| M05 | [Unified hosted SDLC contract](ASTRA-05-UNIFIED-SDLC-CONTRACT.md) | compatible reader deployed; creation and delivery gates open | M02 bindings and M03 integrity. Code preparation may proceed while an unrelated TLS prerequisite is blocked; acceptance still requires usable bindings. |
| M06 | [Durable autonomous controller](ASTRA-06-DURABLE-AUTONOMOUS-CONTROLLER.md) | engineering controller deployed; delivery integration and acceptance gates open | M05. |
| M07 | [Exact-source delivery and real builds](ASTRA-07-SOURCE-DELIVERY-AND-BUILDS.md) | both real Finance builds verified; automatic source delivery and acceptance open | M04 and M06. |
| M08 | [Staging and runtime verification](ASTRA-08-STAGING-AND-RUNTIME-VERIFICATION.md) | bounded Tempo reader locally validated; deployment, staging progression and acceptance open | M07 and usable M02 staging bindings. |
| M09 | [Production approval and bounded rollback](ASTRA-09-PRODUCTION-PROMOTION-AND-ROLLBACK.md) | planned | M08. |
| M10 | [Console convergence and polish](ASTRA-10-CONSOLE-CONVERGENCE-AND-POLISH.md) | initial corrections deployed; list consistency validated in PR 337; remaining deployment and acceptance gates open | May begin after M05; closes against M09 behavior. |
| M11 | [Finance end-to-end acceptance](ASTRA-11-FINANCE-END-TO-END-ACCEPTANCE.md) | planned | M09 and M10, with all earlier gates satisfied. |
| M12 | [Operations and program closeout](ASTRA-12-OPERATIONS-AND-PROGRAM-CLOSEOUT.md) | planned | M11 and all earlier acceptance gates. |

Next eligible: finish M04 qualification on immutable runtime `48c77b7` and continue
M08 staging integration and M10 refinement. After live history preservation passed at 16:20 UTC, the gateway protocol checks
passed 30/30 and coding evaluation `infeval_01a0725f855b7c038234cd6af3830594` started
with two frozen attempts. Both finished 24/24 on the first pass, with every stack
8/8 and no reported hidden-test false passes or policy violations.
[Builder qualification](../../evidence/autonomous-sdlc/ASTRA-M04-48C77B7-BUILDER-QUALIFICATION.md)
is passed. [Onboarding failed 0/12 in both attempts](../../evidence/autonomous-sdlc/ASTRA-M04-48C77B7-ONBOARDING-FAILURE.md)
despite passing 30 protocol checks. Planner subsequently scored 4/12 and 7/12;
Test Diagnosis scored 0/12 in both attempts. Inspection found a Test Diagnosis
scorer/tool-schema mismatch and an overbroad Planner substring check. These require
contract-aligned regression fixes and fresh qualification, not reclassification of
the recorded results as passes. The [scoring correction](../../evidence/autonomous-sdlc/ASTRA-M04-STAGE-SCORING-CONTRACT-CORRECTION.md)
preserves the twelve-case scenarios and thresholds, changes the two affected
suite revisions to V2.1, and adds bounded contract diagnostics. Onboarding's
fixture/context concern remains open. Verifier and Repair remain open. The diagnostic correction retains concrete tool
validation errors without changing fixtures, gates, profiles or limits.
Keep qualification Jobs serial. The frozen coding suite, thresholds, profiles and
execution limits remain in force. Scorer repairs require explicit suite revisions
and fresh evidence. The exact-runtime creation gate also requires
matching qualification on any subsequent release before activation. The earlier fd74092 runs failed; they remain
[recorded evidence](../../evidence/autonomous-sdlc/ASTRA-M04-FD740-QUALIFICATION-FAILURE-ANALYSIS.md),
not superseded passes.

The source-publication controller merged through [PR 338](https://github.com/lward27/pharness/pull/338),
with its normal-callback identity regression fixed in [PR 342](https://github.com/lward27/pharness/pull/342).
The finite build-dispatch restriction merged through [PR 336](https://github.com/lward27/pharness/pull/336),
and the latest list polish merged through [PR 337](https://github.com/lward27/pharness/pull/337).
Guarded source merge merged through [PR 344](https://github.com/lward27/pharness/pull/344)
at `2bbc7a77152d4104651702e84bac3b1893739fc3`; its
[validation](../../evidence/autonomous-sdlc/ASTRA-M07-GUARDED-SOURCE-MERGE.md)
includes persisted merge admission, exact source/base checks and independent provider observation.
The [verified build handoff](../../evidence/autonomous-sdlc/ASTRA-M07-HOSTED-BUILD-HANDOFF.md)
merged through [PR 345](https://github.com/lward27/pharness/pull/345) at
`252030cdd2e457e4658ed7489c7e6a833add2f28`; 464 API/core/worker checks passed.
It binds finite build authority to sealed source and retains declared Tekton outputs
and conflicts. The [durable build controller](../../evidence/autonomous-sdlc/ASTRA-M07-DURABLE-BUILD-CONTROLLER.md)
now records one build admission, original Job and PipelineRun identities, one read-only
recovery observer, bounded grants and duplicate-safe terminal receipts. Its implementation
passed 672 workspace tests and merged in [PR 347](https://github.com/lward27/pharness/pull/347)
at `94e81f89cfa6922224c23193520346a0884ced75`. Deployment and the actual autonomous build/registry chain
remain open.
These changes are not included in the deployed 48c77b7 artifacts. Source-to-build
progression and live source-merge acceptance remain open. The proposed required
Finance source checks passed in both application PRs, but applying main-branch
protections remains [a pending owner decision](../../evidence/autonomous-sdlc/ASTRA-M07-SOURCE-MERGE-DECISION.md).
The owner updated the source writer's Administration read permission; both required
reads are now [verified as authorized](../../evidence/autonomous-sdlc/ASTRA-M07-SOURCE-CREDENTIAL-VERIFIED.json).
Main branches remain unprotected pending the separate owner decision.
That dependent gate cannot be waived; independent implementation can continue.

Current evidence entry points:

- [M02 platform acceptance](../../evidence/autonomous-sdlc/ASTRA-M02-FINANCE-PLATFORM-READINESS.md): supported certificate controller, trusted TLS, isolated staging and the owner-authorized Mac BuildKit path.
- [M03 integrity acceptance](../../evidence/autonomous-sdlc/ASTRA-M03-EVIDENCE-AND-CODE-INTEGRITY.md): evidence normalization and architecture checks.
- [M06 compatible release and recovery floor](../../evidence/autonomous-sdlc/ASTRA-M06-COMPATIBLE-CONTROLLER-RELEASE.md): seven verified images and native bundle, exact Argo revision, schema 53 and preserved Finance history. Hosted creation and Coding Reliability V2 remain disabled.
- [M07 real build evidence](../../evidence/autonomous-sdlc/ASTRA-M07-SOURCE-DELIVERY-AND-BUILDS.md): both actual Finance Tekton builds and registry identities; these program-operated builds do not count as autonomous WorkItems.
- [M08 native Tempo reader](../../evidence/autonomous-sdlc/ASTRA-M08-BOUNDED-TEMPO-READER.md): bounded collection and a real staging trace sample; the reader is not deployed or integrated into staging progression.
- [M10 visual and interaction evidence](../../evidence/autonomous-sdlc/ASTRA-M10-LIST-CONSISTENCY.md): 94 unit checks and 116 distinct browser checks across the documented runs; deployment, delivery-dependent states and owner walkthrough remain open.

[Execution history](ASTRA-PROGRAM-EXECUTION-HISTORY.md) retains prior source
references, checks and superseded status descriptions. Each milestone keeps its
own implementation and acceptance evidence. Healthy infrastructure, merged code
and isolated fixtures never replace the remaining end-to-end gates.

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

Use additive migrations from the verified current schema (0053 after the M06 compatible-controller release).
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
| F01 duplicate source closure | Revalidated in M03; controller recovery remains | M03, M06 | Normal/repeated closure and retry-safe completion pass |
| F02 verifier caveats | Semantics fixed/tested in M03; UI remains | M03, M10 | Risks retained; contradictions cannot normalize to unconditional success |
| F03 unavailable looks empty | New resource states improved; retiring fallback remains | M10 | Failed-load and stale-state UI tests across reachable surfaces |
| F04 scope disagreement | List counts, paging and query focus corrected/tested; wider scope and deployment validation remain | M10 | Scope, filters, counts, cancellation, and data agree |
| F05 incoherent cockpit example | Prototype concern; validate implementation | M10 | One coherent current state/action, no fictional fixture state |
| F06 lifecycle dominates decision | Subjective hierarchy concern | M10 | Decision-first desktop/phone walkthrough |
| F07 narrow/keyboard interaction | Prototype concern; current implementation partly improved | M10 | Responsive/native keyboard interaction evidence |
| F08 documentation drift | Confirmed; indexes partly repaired upstream | M01, M12 | Current reality/direction/history have unambiguous entry points |
| F09 competing lifecycle/navigation | Direction superseded by approved program | M05, M10 | Canonical lifecycle and established navigation agree |
| F10 demo defaults | Legacy creation exposure | M10 | Validated context only, no implicit demo input |
| F11 failed architecture guardrails | Closed with M03 checks | M03 | Parser and existing boundary/size checks pass |
| F12 tracked generated workspace | Closed with M03 inventory | M03 | Disposable output untracked; intentional evidence retained |
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
