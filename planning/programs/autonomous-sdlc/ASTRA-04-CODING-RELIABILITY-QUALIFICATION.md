# ASTRA M04: Qualify the connected coding process

Status: active, not accepted. On 2026-09-06 the owner approved the
[process reassessment](../../evidence/autonomous-sdlc/ASTRA-M04-PROCESS-REASSESSMENT.md)
and instructed implementation. This document replaces the previous sequence of
broad stage runs and incremental prompt/scorer exceptions. It does not lower any
coding threshold or authorize a production release.

Authority: [master program](ASTRA-00-PROGRAM.md). Dependency: M03 accepted.
Starting main: `04dbaeab51d9de939f0743d587c3aa3d10c21a8f`.
Deployed runtime: `92f8f1b8e98dd45d0a01e030aeb99ef9bcf95267`, unqualified; [live release and preservation evidence](../../evidence/autonomous-sdlc/ASTRA-M04-DIAGNOSTIC-LIVE-RELEASE.md).
Hosted creation and Coding Reliability V2 remain disabled until the exact-runtime gates pass.

Operator workflow: [cluster operator checkpoint](../../evidence/autonomous-sdlc/ASTRA-CLUSTER-OPERATOR-CHECKPOINT.md). Latest preparation and resume point: [local gateway readiness and Onboarding scope correction](../../evidence/autonomous-sdlc/ASTRA-M04-LOCAL-MODEL-READINESS.md). The first diagnostic remains failed and unqualified; the shared prompt now distinguishes onboarding configuration permissions from future development scope. The [complete metered gateway release](../../evidence/autonomous-sdlc/ASTRA-M04-METERED-GATEWAY-RELEASE.md) is deployed and observed. Its canary failed on the [shared writable-path contract gap](../../evidence/autonomous-sdlc/ASTRA-M04-WRITABLE-SCOPE-CONTRACT-GAP.md), which must be corrected before further comparisons. Retired temporary helpers are historical only.

## Objective and implementation boundaries

Prove that the existing gateway path can complete bounded engineering work with
truthful evidence, useful failures and one correction. Remove unnecessary model
bookkeeping and invalid evaluation assumptions before evaluating model capability.
Keep the shared runner, controlled stage Runs, deterministic discovery and Test,
checkpoint machinery, SQLite, durable source/effect records, independent verification,
and the existing execution limits. Native Codex-host expansion is deferred.

The controller owns original run, discovery and source identity. The model owns its
proposal, reasoning, evidence selections and code. Native code attaches trusted
metadata from the original Run, never the latest mutable state. Preserve explicit
rejection of conflicting legacy identity claims and all downstream stale-source,
preimage, authorization and immutable-evidence checks. Semantic claims remain claims.

No provider/model switch, budget increase, automatic acceptance or new execution
backend is implied. Candidate comparisons are labelled diagnostics on identical
valid inputs and budgets; existing policies and defaults stay unchanged. Explicit diagnostic control policies
may be added, without activation or a silent fallback.
Do not build a new evaluation service or qualification-cache architecture.

## Ordered implementation and acceptance gates

Each gate closes with committed implementation, meaningful deterministic checks,
source hashes and an ASTRA evidence record. An implementation gate is distinct from
live qualification. M04A and M04B can be prepared independently; later gates depend
on the stated evidence. Run live evaluations serially.

| Gate | Work and affected contracts | Required evidence | Status |
| --- | --- | --- | --- |
| M04A — Trusted submission boundary | Version the agent-facing submission separately from durable controller records. Bind onboarding discovery in native code; use catalog evidence IDs for selections. Validate required fields and reject conflicting supplied IDs with field-specific diagnostics. Preserve historical V1 reads and originally bound resumed Runs. | Valid, blocked, missing-context, stale/conflicting identity, extra-field and replay/resume tests. Schema and prompt describe the same accepted tool contract. Full durable proposal still carries its original identity and hash. | native adapter and deterministic replay validated; saved-context checks validated in B |
| M04B — Context delivery | Assemble a versioned, inspectable context envelope with distinct instructions/data. Reuse the current runtime. Exercise initial, replay, checkpoint, budget and recovery serialization, including actual provider-facing messages. Retain the demonstrated MiniMax transport correction. | All required sections survive; missing required context fails before dispatch. Tool-call/result order and persisted replay remain valid. Small provider canary exercises the real envelope, not a repeated marker in a user message. | native envelope, replay and provider serialization validated; live canary pending |
| M04C — Valid stage measurements | Validate plans by structured paths, named commands and acceptance coverage; retire English negation parsing as an execution gate. Ground Planner/Diagnosis cases in matching workspaces and actual receipts. Give Verifier requirements, source changes and test evidence while keeping expected verdicts and answer markers private. | Explicit old-to-new scenario mapping and suite revisions, adversarial leakage checks, known correct/incorrect submissions, real command receipts, and distinct protocol versus semantic failure reports. Old protocol fixtures remain labelled history/regression. | source-backed measurements and 48-case replay validated; live judgment remains open |
| M04D — Small live canaries and model control | Through the real gateway/worker, run valid and blocked onboarding, a grounded plan, actual failure diagnosis, and blind verification. Diagnose the first boundary failure before another batch. Compare registered candidates to an explicitly recorded common control on identical inputs, tools and limits before optimizing roles. | Retained request/context/schema hashes, outputs, tool errors, usage and failure class. No silent model fallback. Passing canaries do not count as benchmark acceptance. | native diagnostic/control path deployed; live preservation and service checks pass; first small canary failed; scope clarification prepared; release and fresh canary pending |
| M04E — Connected loop | Exercise plan → implement → deterministic Test → actual failure diagnosis → one repair → Test → independently scoped verification. Preserve the same workspace and original evidence lineage at each handoff. Deterministic facts should not require an extra model to restate them. | At least one real failed implementation repaired through the actual handoff, plus a correct change and a rejected semantic defect. No manually supplied application patch, synthetic success receipt or extra repair. Restart/replay retain the same authority. | open; requires D |
| M04F — Frozen release qualification | Freeze one release candidate, pin all profiles/prompts/tool and suite hashes/environment images/limits, release immutably, and run complete protocol, revised stage, frozen coding and repair qualification. Keep the exact-runtime activation guard. | Two independent qualifying runs meeting every threshold below, complete stage evidence, immutable release identity, explicit failure behavior and measured usage. A later runtime requires matching qualification. | open; requires A–E |

Changes to stage fixtures/scorers are authorized as part of the approved reassessment.
Document why each change improves validity, its scenario mapping and new revision.
Do not edit historical failed results, secretly reduce the case set, expose hidden
answers, or count a replay as a live pass. Keep the frozen 24 coding tasks untouched.

## Acceptance checks

- [x] M04A–M04C deterministic contract and measurement checks pass before new broad runs.
- [ ] M04D canaries preserve the actual context/tool/evidence boundary and isolate model quality from harness failures.
- [ ] M04E proves the connected failure-to-repair handoff and independent verification.
- [ ] Two independent frozen coding runs each achieve **at least 21/24 first pass and 23/24 after one correction**.
- [ ] Every language stack achieves **at least 6/8 first pass and 7/8 after correction** in both runs.
- [ ] Existing protocol qualification passes **30/30** (ten cases repeated three times), alongside the stronger context-envelope checks.
- [ ] All stage-specific qualification gates pass on the explicitly revised, valid suites; seeded Repair qualification is retained in addition to the connected-loop proof.
- [ ] No hidden-test false passes, policy violations, increased budgets or unrecorded interventions.
- [ ] Exact source, model/profile/policy, tools, prompts, tasks, images, configuration, usage, wall time, intervention and failure classes are retained.
- [ ] Hosted activation refers to the exact qualified deployed runtime. Credential/provider failures remain concrete blockers; no fallback silently replaces the selected profile.

## Current facts and historical evidence

[M04A trusted submissions](../../evidence/autonomous-sdlc/ASTRA-M04-TRUSTED-SUBMISSIONS.md)
are implemented and locally validated (530 component checks; one live-only test
ignored). The combined implementation is now deployed; live qualification remains
open. Its saved-context replay checks are covered by M04B. [M04B context delivery](../../evidence/autonomous-sdlc/ASTRA-M04-CONTEXT-ENVELOPE.md)
is now implemented and locally validated (554 component checks; one live-only test
ignored), including actual runner resume paths and bounded context retention.
[M04C valid measurements](../../evidence/autonomous-sdlc/ASTRA-M04-STAGE-MEASUREMENTS.md)
are locally validated, with all 48 original scenarios mapped and source/receipt inputs retained.
[M04D diagnostic scope and exact-input controls](../../evidence/autonomous-sdlc/ASTRA-M04-DIAGNOSTIC-CANARIES.md)
are locally validated on `f0c7afd` (627 passing component tests; one live-only ignored).
Migration 0055 is additive; old schema-54 readers are proven incompatible with the
new migration set. A current [schema-54 snapshot](../../evidence/autonomous-sdlc/ASTRA-M04-PRE0055-ARCHIVE-VERIFIED.json)
is verified, and the [actual schema-55 clone migration](../../evidence/autonomous-sdlc/ASTRA-M04-DIAGNOSTIC-CLONE-MIGRATION-VERIFIED.json) preserves every original column in all 81 tables. [All seven images, the native bundle and release pins](../../evidence/autonomous-sdlc/ASTRA-M04-DIAGNOSTIC-COMPATIBLE-RELEASE.md) are verified. The [live compatible reader, preserved data and full service window](../../evidence/autonomous-sdlc/ASTRA-M04-DIAGNOSTIC-LIVE-RELEASE.md) now pass. **Next: implement the [shared writable-scope contract correction](../../evidence/autonomous-sdlc/ASTRA-M04-WRITABLE-SCOPE-CONTRACT-GAP.md), with deterministic authority and feedback checks before a fresh immutable release and one new MiniMax diagnostic**, then the connected loop. No new broad qualification
or model switch is justified by these local passes.


Builder and seeded Repair on `48c77b7` each passed both 24-case attempts, 8/8 per
stack. This is meaningful bounded editing evidence; it neither proves the connected
handoff nor qualifies a later runtime. On `19b0c55`, Onboarding failed 0/12 twice,
Planner scored 6/12 and 7/12, and Test Diagnosis scored 1/12 twice. Its [Verifier result](../../evidence/autonomous-sdlc/ASTRA-M04-19B0C55-VERIFIER-ANALYSIS.md)
is now terminal: 1/24 in each attempt, gate failed. Retain that unchanged result
without launching another old-suite batch. It tests protocol conventions and must
not be presented as blind semantic verification.

The merged [context transport correction](../../evidence/autonomous-sdlc/ASTRA-M04-SYSTEM-CONTEXT-DELIVERY.md)
and [stage evidence correction](../../evidence/autonomous-sdlc/ASTRA-M04-STAGE-EVIDENCE-CONTRACT.md)
are deployed in `92f8f1b` but not qualified. M04C has retired the latter's interim Planner language
predicate and extended its grounded Diagnosis and bounded evidence references. Full diagnosis and earlier evidence are
linked in the reassessment. The [execution history](ASTRA-PROGRAM-EXECUTION-HISTORY.md)
preserves the superseded sequence and all original outcomes.

## Deployment, compatibility and recovery

Use fresh `codex/` worktrees from verified main. Ship compatible readers before new
writes. Prefer a versioned worker-bound submission adapter that preserves the full
durable proposal and historical schemas, without a database migration. Explicitly
bind new versus legacy behavior to the Run's saved execution contract; do not guess
from a model's payload or reinterpret an old Run using new defaults.

Before deployment, finish or retain the already-dispatched evaluation. Record the
minimum compatible rollback release, build the full required immutable artifact
set from one merged source, pin GitOps digests, and observe exact runtime identities.
Keep hosted creation disabled on failure. A diagnostic binary or local replay does
not qualify the deployed runtime. Preserve all Finance records and audit history.
This milestone authorizes no production Finance merge or acceptance approval.

If a canary fails, retain its first useful failure, identify the failing boundary,
fix only the demonstrated issue, and repeat the affected canary. Do not run the full
matrix to debug a known invalid harness. If the corrected connected loop still
fails representative work under existing limits with a recorded capable control,
bring that evidence to the owner before a larger architectural replacement.

## Evidence and closeout

Write bounded `ASTRA-M04-…` records under `planning/evidence/autonomous-sdlc/` with
objective checks separated from design judgment. Record source revisions, source
hashes, exact checks, fixtures and limitations. Update this gate table, the master
program and F13 only against evidence. M04 remains open until every gate above
passes; M07–M12 and the two Finance acceptance WorkItems retain their own gates.

## Goal-mode execution prompt

Read ASTRA-00-PROGRAM.md, this document and ASTRA-M04-PROCESS-REASSESSMENT.md.
Verify current main, deployed identity and the last retained qualification result.
Implement the next eligible M04 gate in a fresh codex/ worktree. The source-81
checkpoint owns the shared writable-scope contract gap: correct its description
and early validation without broadening authority, prove deterministic regression
checks, release, renew the bounded protocol verification if expired, and repeat the
affected diagnostic. Do not restart already evidenced submission/context work.
Then run common-input controls, prove the connected loop, and freeze the exact
release before full qualification. Preserve the 24 coding tasks, all thresholds,
existing execution budgets, historical failed evidence, independent verification
and the human production-approval boundary. Do not launch a broad run to diagnose
an already-known harness defect. Commit implementation and evidence, update the gate
and program status, and continue eligible work. Ask only for a material missing
decision or external prerequisite; do not treat elapsed time or a local pass as
runtime qualification or autonomous Finance acceptance.
