# ASTRA M04: Preserve the original authorization at the repair handoff

Status: implemented, validated locally and merged in PR397 as `67f53039a9485a4cd57af2c7dbaea14c4f7f9a44`; not deployed. [Merge and unchanged validated-source hashes](ASTRA-M04-REPAIR-AUTHORIZATION-MERGE.json) are retained. This is a local M04E/M06 integrity correction, not connected-loop or runtime acceptance.

## Observed defect

On source `291e007cedcb2440a328f5e24710fb3423cb0ded`, Test and Verifier checked the active stage-chain authorization against the current plan revision, Product snapshot, repository, source and expiry. Automatic repair loaded an active chain and called the Builder entry without those checks. The shared Builder entry also omitted them. A revised approved plan or expired authorization could therefore reach later preparation/execution checks instead of stopping at the authority boundary.

The new regression invokes the real Builder/Repair entry with native in-memory store records and a disabled worker. Before the fix, a changed plan revision produced “model execution worker is unavailable” rather than a stale-authorization error. This demonstrates the missing early check; no real worker, model call, application patch or cluster effect was dispatched by the test.

## Change

One private validation function in the existing stage executor now checks the original authorization before Builder/Repair preparation, before recording that automatic repair has started, and before Test/Verifier continuation. It retains the existing expiry and pinned-state rules and additionally rejects revoked authority, a different WorkItem, or a plan that is no longer approved. The repair count, budgets, model choices, qualification checks, workspace reuse and approval authority are unchanged.

A valid historical source-only plan still reaches its original worker boundary; it is not retroactively required to contain the new Planner readiness field. This change adds no database migration, public API, workflow mode or execution backend.

## Validation and limits

The regression covers plan revision, expired/malformed expiry, source, Product snapshot, revoked state and WorkItem mismatch for both Builder and Repair, plus the unchanged valid historical path. The original failure is retained. The complete API/admin suite passed 301 checks. A temporary placement of the helper in the authorization module produced an import cycle; keeping it private in the stage executor removed the cycle. The final focused regression, warnings-denied Clippy for all API targets, formatting and five architecture checks pass on the final layout. [Validation and source hashes](ASTRA-M04-REPAIR-AUTHORIZATION-VALIDATION.json) distinguish the full component run from the final focused check.

This test does not prove that a live invalid repair was dispatched, nor that the complete failure → diagnosis → repair → Test → Verifier sequence works. M04E still requires that connected proof, and M04F still requires full qualification on the final frozen runtime. No qualification or Finance acceptance gate is closed here.

## Integration and recovery

This correction is separate from the already-frozen source291e007 Planner readiness release. Retain its evidence and include this merged correction when the connected-loop candidate is frozen. Do not modify in-progress release artifacts or claim this code is included in them. Keep hosted activation off until the program gates pass.

The preceding compatible reader remains data-compatible, but lacks this authorization recheck. Pause development and retain in-flight history before any rollback to it; do not use a rollback to continue an expired or superseded chain. The compatible-reader floor and Finance production approval rules in the master program remain unchanged.
