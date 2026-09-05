# ASTRA: Program execution history

This file preserves superseded status descriptions and source references. They are dated implementation history, not current operating instructions. Use [the master program](ASTRA-00-PROGRAM.md) for current milestones, authority, dependencies and next work. Individual evidence files remain the authoritative records for their checks.

## Status record through 2026-09-05, before the schema 53 release was verified

Next eligible: address the measured M04 coding failures and qualify the resulting
immutable runtime while continuing M06, M07 and M10 preparation. Source `48c77b7`
is being built as one immutable controller/console release; live PHarness remains
on `2249950`. The isolated schema-53 migration passed against the Finance snapshot;
live migration and its compatible rollback floor remain pending.
The fd74092 live evaluation completed at 12:34 UTC and failed: 22/24 and 20/24,
two hidden-test false passes, and one blocked write outside permitted paths.
There were no provider or infrastructure failures in this run. See the
[failure analysis](../../evidence/autonomous-sdlc/ASTRA-M04-FD740-QUALIFICATION-FAILURE-ANALYSIS.md).
All 216 packaged offline checks passed; those fixtures do not qualify the model.
The original backend artifact is restored; both staging deployments are Healthy;
all 13 isolation checks passed. Cert-manager 1.20.3 preserves 31 ready certificates
and 78 retained requests. The owner-authorized Mac serves Tekton's existing
BuildKit endpoint; uncached AMD64 execution, a 112 MiB private TLS push, and
exact-digest pull/run passed. Worker capability checks passed after the GitOps
writer credential was rotated. Runtime contract declarations remain M05 and the
real frontend pipeline is deployed through GitOps `491f081`; sequential backend
and frontend Tekton builds passed with independently verified registry identities.
Automatic source delivery and dispatch remain M07, with the repository-protection
decision pending owner input. The tested M04 scratch cleanup is merged at
`fd740927110366a983de6bb0d3bc6c576577708b`; its [release evidence](../../evidence/autonomous-sdlc/ASTRA-M04-CODING-RELIABILITY-QUALIFICATION.md)
does not replace live model qualification. M05 now enforces saved stage profiles, gateway choices and limits in
merged code; [live acceptance remains open](../../evidence/autonomous-sdlc/ASTRA-M05-UNIFIED-SDLC-CONTRACT.md).
Fresh gateway calibration passed for Builder and Planner. MiniMax's malformed
history rejection exposed a protocol compatibility defect; that repair and complete
stage-report guards merged in PR 330 at `db84b797f1bbc833ba86844874d1d041bc33ab72`.
The completed coding evaluation used fd74092. New runtime qualification remains
required; keep qualification jobs serial. M05 compatible-reader source merged in
PR 328 at `2249950d225a4632b24235c2b6f2d8469a774243`. Its complete seven-image
AMD64 release and native bundle were verified and deployed through PR 333, merge
`8ca88f32e3d50f8430cf5a486912ebe6d00a392d`. Argo and all five long-running
Deployment image identities matched; hosted creation and Coding Reliability V2
remain disabled. See [reader release and rollback floor](../../evidence/autonomous-sdlc/ASTRA-M05-COMPATIBLE-READER-RELEASE.md).
M04 contract clarification merged through [PR 331](https://github.com/lward27/pharness/pull/331)
at `4c40b10c0b2f71ab92d464528145e178222a3368`. Its 28 runhost tests and Clippy pass. Its live qualification follows a new immutable release; the current reader
release does not include that clarification. Neither source merge nor provider
diagnostics close M04.

M06 engineering progression is integrated at `e1709a2` on
`codex/astra-autonomous-controller`, following persistence `9d52c9e`, dispatch
recovery `bded5a1`, and controls/admission `51485ba`. Atomic preparation recovery follows at `f755915`, with 304 distinct API/admin/store
tests passing. Terminal normalization recovery follows at `1dc8c97`, with 306
distinct passing API/admin/store tests. Source delivery dispatch ordering follows at `deb7ebf`, with 308
distinct passing tests. These controller changes merged through [PR 332](https://github.com/lward27/pharness/pull/332)
at `ba8ce03e4dfd3df5815c897a69276858b53aacb2`. Full combined validation passed
641 workspace tests, Clippy, formatting and architecture checks; see the
[combined validation record](../../evidence/autonomous-sdlc/ASTRA-M06-COMBINED-WORKSPACE-VALIDATION.json).
Controller delivery integration, terminal cancellation and live acceptance remain open. These independent preparations do
not waive M05 or M04 gates. See
[controller evidence](../../evidence/autonomous-sdlc/ASTRA-M06-DURABLE-AUTONOMOUS-CONTROLLER.md).
M10 initial console corrections merged through [PR 334](https://github.com/lward27/pharness/pull/334)
at `ca98fa7c7474902d206e130ca14eddddec8d82a7`. All 79 UI unit checks, the production build,
and the real API journey with both console flags passed against combined M04/M06 source.
See the [console evidence and subjective review](../../evidence/autonomous-sdlc/ASTRA-M10-CONSOLE-CONVERGENCE-AND-POLISH.md).
The documented remaining concerns and delivery dependencies keep M10 open.

The next console slice is [PR 337](https://github.com/lward27/pharness/pull/337),
source `1b52f2520c38dd185f02c82760939d5c037b9642`, based on `48c77b7`.
It preserves focused searches, repairs disappearing pagination, independently pages
legacy records, and makes failures readable. The UI build, 94 unit checks and
116 distinct browser checks passed; this source is not yet deployed.
See [list evidence and visual judgment](../../evidence/autonomous-sdlc/ASTRA-M10-LIST-CONSISTENCY.md).

Schema 0052 is applied to the live Finance database; 0053 remains undeployed.
A new 21,213,184-byte pre-0053 snapshot preserves the same 14 WorkItems and 82 Runs;
its [verified manifest](../../evidence/autonomous-sdlc/ASTRA-M06-DATABASE-ARCHIVE-VERIFIED.json)
is retained. Clone migration and a compatible immutable schema-53 rollback floor
are required before the controller release. Neither snapshot is an independent
disaster-recovery backup.
A verified 21,204,992-byte pre-0052 snapshot is retained on its existing PVC.
The immutable 2249950 reader successfully migrated an isolated copy to 0052 while
preserving its 14 WorkItems, 82 Runs, evidence, audit records and four holds; see
[isolated migration proof](../../evidence/autonomous-sdlc/ASTRA-M05-CLONE-MIGRATION-VERIFIED.json).
The subsequent live read-only comparison verified all original WorkItems, Runs,
stage outcomes, audit records and holds unchanged; see [live preservation evidence](../../evidence/autonomous-sdlc/ASTRA-M05-LIVE-DATABASE-VERIFIED.json).
See [M02 evidence](../../evidence/autonomous-sdlc/ASTRA-M02-FINANCE-PLATFORM-READINESS.md).
M03 implementation is `b354c2b534fb4f518a439e92bb6770c8287fd4fd`; see [its acceptance evidence](../../evidence/autonomous-sdlc/ASTRA-M03-EVIDENCE-AND-CODE-INTEGRITY.md).
A real external blocker may suspend its dependent work but never waive its gate.
Continue eligible independent work and ask only for missing authority/credentials
or a decision with material downstream consequences.

