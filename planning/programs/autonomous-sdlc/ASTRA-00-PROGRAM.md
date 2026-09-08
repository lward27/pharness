# ASTRA: Autonomous SDLC in lucas_engineering

Status: approved implementation program; M01–M03 accepted; M04 active; M05/M06 compatible reader deployed; hosted creation and end-to-end gates remain open. Approved by the owner on 2026-09-04.
Baseline: PHarness main `c36b46aceb72f3d7097bc0bdee74810c745f7c0c`; GitOps main `fa27225c4c33b710ce24708e17fd39ac05ab6aeb`.
Current compiled PHarness release: `d675159cb205d222616d253d311706be1889bb91`, observed through release pin `ce98a26065cd48323b58b8905fcbbfc6c76452cd` on 2026-09-08. [Exact live identities and the full ten-minute service window](../../evidence/autonomous-sdlc/ASTRA-M04-POLICY-PROTOCOL-RELEASE.md) pass. Schema55, Finance history, the compatible-reader floor (`92f8f1b` or later compatible readers), model defaults and execution limits are unchanged. Hosted creation and Coding Reliability V2 remain disabled; the runtime is not qualified for autonomous Finance work.

Current operations: [cluster operator checkpoint](../../evidence/autonomous-sdlc/ASTRA-CLUSTER-OPERATOR-CHECKPOINT.md). The d675159 diagnostic pairs are terminal; a validated Planner decision correction is the next release boundary. [Execution history](ASTRA-PROGRAM-EXECUTION-HISTORY.md) preserves the older source38/source80/sourcee508 diagnostics and their original verdicts.

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
| M04 | [Coding reliability qualification](ASTRA-04-CODING-RELIABILITY-QUALIFICATION.md) | Corrected measurements and exact-policy gate released; fresh M04D canaries and M04E–F remain open | M03. Qualification blockers do not stop independent implementation. |
| M05 | [Unified hosted SDLC contract](ASTRA-05-UNIFIED-SDLC-CONTRACT.md) | compatible reader deployed; creation and delivery gates open | M02 bindings and M03 integrity. Code preparation may proceed while an unrelated TLS prerequisite is blocked; acceptance still requires usable bindings. |
| M06 | [Durable autonomous controller](ASTRA-06-DURABLE-AUTONOMOUS-CONTROLLER.md) | engineering controller deployed; delivery integration and acceptance gates open | M05. |
| M07 | [Exact-source delivery and real builds](ASTRA-07-SOURCE-DELIVERY-AND-BUILDS.md) | both real Finance builds verified; approved source protections applied; automatic source delivery and acceptance open | M04 and M06. |
| M08 | [Staging and runtime verification](ASTRA-08-STAGING-AND-RUNTIME-VERIFICATION.md) | Basic Tempo reader, schema-54 records and staging handoff deployed; new native runtime integration and autonomous staging acceptance open | M07 and usable M02 staging bindings. |
| M09 | [Production approval and bounded rollback](ASTRA-09-PRODUCTION-PROMOTION-AND-ROLLBACK.md) | initial decision-material contract validated in draft; integration and acceptance open | M08. |
| M10 | [Console convergence and polish](ASTRA-10-CONSOLE-CONVERGENCE-AND-POLISH.md) | initial corrections and PR 337 list consistency deployed; delivery-dependent states and acceptance gates open | May begin after M05; closes against M09 behavior. |
| M11 | [Finance end-to-end acceptance](ASTRA-11-FINANCE-END-TO-END-ACCEPTANCE.md) | planned | M09 and M10, with all earlier gates satisfied. |
| M12 | [Operations and program closeout](ASTRA-12-OPERATIONS-AND-PROGRAM-CLOSEOUT.md) | planned | M11 and all earlier acceptance gates. |

### Current execution — 2026-09-08

The owner approved the [M04 process reassessment](../../evidence/autonomous-sdlc/ASTRA-M04-PROCESS-REASSESSMENT.md)
and requested implementation. [M04](ASTRA-04-CODING-RELIABILITY-QUALIFICATION.md)
now owns six ordered gates: trusted submissions; context delivery; valid stage
measurements; small live canaries and controlled model comparison; connected coding
and repair; then frozen-release qualification. This targeted refactor retains the
shared gateway/runner, durable controller, deterministic tests and execution limits.
Independent verification and production approval remain separate boundaries.

[M04A trusted submissions](../../evidence/autonomous-sdlc/ASTRA-M04-TRUSTED-SUBMISSIONS.md), [M04B context delivery](../../evidence/autonomous-sdlc/ASTRA-M04-CONTEXT-ENVELOPE.md), and [M04C grounded measurements](../../evidence/autonomous-sdlc/ASTRA-M04-STAGE-MEASUREMENTS.md) have deterministic validation. The latest release adds the [complete fixture correction](../../evidence/autonomous-sdlc/ASTRA-M04-SEMANTIC-FIXTURE-VALIDITY.md), the Planner selection/completeness repair, and [exact-policy protocol selection/admission](../../evidence/autonomous-sdlc/ASTRA-M04-POLICY-BOUND-PROTOCOL.md). The protocol correction passes 297 API Rust checks, three UI tests, 16 browser scenarios and five architecture regressions. All 29 live policies now require their own current-runtime check; historical target-only receipts cannot satisfy that gate.

**Next eligible: integrate validated Planner decision PR395 and layer-reuse PR394, freeze one source, release it and run the affected Planner v2.6 checks before the connected loop or broad qualification.** Follow the [release and declared diagnostic sequence](../../evidence/autonomous-sdlc/ASTRA-M04-POLICY-PROTOCOL-RELEASE.md). Check each exact policy, inspect the terminal primary and its complete retained inputs, then run its matched control. Revalidate Diagnosis's prerequisite without changing limits. M04E's connected failure-to-repair loop and M04F's full frozen qualification remain open.

The [d675159 Planner pair](../../evidence/autonomous-sdlc/ASTRA-M04-D675159-PLANNER-COMPARISON.md) is complete with native2/2 for both same-model runs on identical valid inputs. The control nevertheless recommends weakening an existing regression while leaving the intended behavior unresolved. The current controller can lose that blocker when sealing the plan. M04C must preserve an explicit decision boundary; M04E must prove that it stops unsafe progression without blocking settled authorized work. [Verifier](../../evidence/autonomous-sdlc/ASTRA-M04-D675159-VERIFIER-COMPARISON.md) is native2/3 for GLM (one malformed terminal submission) versus3/3 for Kimi. [Diagnosis](../../evidence/autonomous-sdlc/ASTRA-M04-D675159-DIAGNOSIS-COMPARISON.md) is2/2 for both Nemotron and Kimi; the previous protocol timeout did not recur. All declared d675159 diagnostics are terminal. [Planner decision PR395](https://github.com/lward27/pharness/pull/395) has local validation and remains undeployed. No model default or qualification has changed.

The prior [sourcee508 Onboarding comparison](../../evidence/autonomous-sdlc/ASTRA-M04-E508F43-ONBOARDING-COMPARISON.md) is useful diagnostic evidence: primary1/2 and control2/2 on identical inputs. [Planner native1/2](../../evidence/autonomous-sdlc/ASTRA-M04-E508F43-PLANNER-ANALYSIS.md) and [Verifier native3/3](../../evidence/autonomous-sdlc/ASTRA-M04-E508F43-VERIFIER-ANALYSIS.md) had invalid measurements; both old-suite controls remain undispatched and the native outcomes unchanged. [Diagnosis preflight](../../evidence/autonomous-sdlc/ASTRA-M04-E508F43-DIAGNOSIS-PREFLIGHT.md) timed out before a semantic run. The new Planner v2.5 and Verifier v2.4 correct those inputs, not those historical scores. Source24790cc was only preflighted; [source5a40945 built complete artifacts](../../evidence/autonomous-sdlc/ASTRA-M04-5A40945-ARTIFACT-BUILD.md) but was held for the combined d675159 release.

The [Mac Tekton route](../../evidence/autonomous-sdlc/ASTRA-M02-MAC-BUILDKIT-RETURN.md) remains selected while lucas-desktop is off. The Windows Minisforum guide is available; no local endpoint has been supplied or activated. The [current registry transport investigation](../../evidence/autonomous-sdlc/ASTRA-M04-D675159-REGISTRY-TRANSPORT.md) retains intermittent TLS and internal upstream connection timeouts. The original build completed and subsequent probes passed; the underlying cause is not declared repaired. [Draft PR394](https://github.com/lward27/pharness/pull/394) isolates evaluator toolchain layers from release metadata; three local cache-only builds pass, but it is not included in this runtime.

M07 source/build foundations and both real Finance builds are evidenced; the autonomous chain remains open. [M08 draft363](https://github.com/lward27/pharness/pull/363) is refreshed against5a40945 with [851 checks](../../evidence/autonomous-sdlc/ASTRA-M08-5A40945-INTEGRATION.md), including two explicit Helm tests. [M09 draft366](https://github.com/lward27/pharness/pull/366) has [216 core/integration checks](../../evidence/autonomous-sdlc/ASTRA-M09-5A40945-INTEGRATION.md). Both remain unreleased and unaccepted. M09's pure production-material contract still needs authenticated decisions, persistence, execution and bounded recovery. The prepared Finance production-baseline change is unapproved; implementation authority never substitutes for M11's human approval.

Current evidence entry points:

- [M02 platform acceptance](../../evidence/autonomous-sdlc/ASTRA-M02-FINANCE-PLATFORM-READINESS.md): supported certificate controller, trusted TLS and isolated staging; the [Mac BuildKit route](../../evidence/autonomous-sdlc/ASTRA-M02-MAC-BUILDKIT-RETURN.md) is verified while lucas-desktop is off.
- [M03 integrity acceptance](../../evidence/autonomous-sdlc/ASTRA-M03-EVIDENCE-AND-CODE-INTEGRITY.md): evidence normalization and architecture checks.
- [M06 compatible release and recovery floor](../../evidence/autonomous-sdlc/ASTRA-M06-COMPATIBLE-CONTROLLER-RELEASE.md): seven verified images and native bundle, exact Argo revision, schema 53 and preserved Finance history. Hosted creation and Coding Reliability V2 remain disabled.
- [M07 real build evidence](../../evidence/autonomous-sdlc/ASTRA-M07-SOURCE-DELIVERY-AND-BUILDS.md): both actual Finance Tekton builds and registry identities; these program-operated builds do not count as autonomous WorkItems.
- [M08 native Tempo reader](../../evidence/autonomous-sdlc/ASTRA-M08-BOUNDED-TEMPO-READER.md): bounded collection and a real staging trace sample; the basic reader is deployed but its Tempo endpoint is not yet configured or integrated into staging progression.
- [M08 delivery-record compatibility](../../evidence/autonomous-sdlc/ASTRA-M08-DELIVERY-RECORD-COMPATIBILITY.md): separate staging/production records on the same build, preserved legacy history, and rejected legacy delivery actions. The later schema55 live-copy check and compatible-reader deployment are recorded under M04; hosted delivery writes and autonomous deployment acceptance remain open.
- [M08 durable staging GitOps handoff](../../evidence/autonomous-sdlc/ASTRA-M08-DURABLE-STAGING-GITOPS.md): the controller records one staging authority and admission, preserves its original writer and reader Jobs, and retains uncertain or contradictory outcomes. Local tests cover automatic handoff and recovery. A committed digest remains distinct from Argo and runtime verification; no real staging release or production authorization is claimed.
- [M10 visual and interaction evidence](../../evidence/autonomous-sdlc/ASTRA-M10-LIST-CONSISTENCY.md): 94 unit checks and 116 distinct browser checks across the documented runs; the initial list corrections are deployed; delivery-dependent states and owner walkthrough remain open.

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

Use additive migrations from the verified current schema (0055 after the observed source-92f8f1b release).
Preserve the Finance generation and retention/audit history. Compatible readers
ship before hosted writes; record the minimum compatible rollback release.
The M08 cardinality extension records one explicit SQL exception: SQLite requires
an atomic, data-preserving replacement of the GitOps table to remove the embedded
one-change-per-build constraint. Its history/foreign-key/rollback tests do not
waive the pre-deployment check against a copy of the current live database.
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
| F02 verifier caveats | Verifier semantics fixed/tested in M03; d675159 diagnostics expose the same concern at Planner sealing; UI remains | M03, M04, M06, M10 | Risks and planning blockers retained; unresolved decisions cannot normalize to unconditional success or grant automatic execution |
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
