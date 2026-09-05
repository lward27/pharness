# ASTRA M03: Evidence and code integrity acceptance

Accepted as committed implementation on 2026-09-04 (2026-09-05 UTC).
Implementation revision: `b354c2b534fb4f518a439e92bb6770c8287fd4fd`; parent `4d7cb34`;
main baseline `c36b46aceb72f3d7097bc0bdee74810c745f7c0c`.
This is source/test acceptance. The live PHarness deployment has not changed.

## Result

Verifier submissions retain their raw agent claims, risks, and contradictions.
An approved decision with unresolved contradictions or malformed caveat fields
seals a failed verification outcome and cannot create an approved ChangeSet.
Absent optional caveat fields remain compatible with older submissions.
Controller facts describe validated decisions, caveat shape/count, and upstream
outcomes; submitted caveats are not relabeled as independently verified facts.
Repeated synchronization preserves the same sealed outcome and validation hash.

The upstream source-closure fix was retained. The existing regression exercises
normal prior-state closure and repeat notification without duplicate Release or
Observe records. No historical sealed outcomes were rewritten.

The dependency checker now masks Rust literals and comments before reading imports,
including raw strings, escaped strings/chars, lifetimes, and nested comments. Inline
module scope is respected. This removes false imports and false cycles while still
rejecting a real two-module dependency cycle.

The previous 5,502-line product and 8,152-line repository modules are now explicit
routing facades with owned implementation modules. Product responsibilities are
model changes, registration, readiness, onboarding actions, execution callbacks,
policy, and state projection. Repository responsibilities are creation, projection,
actions, stages, source delivery, evidence, and state. Shared readiness validation
has one owner. Public route behavior and request shapes are preserved.
The largest resulting implementation module has 1,941 lines; the existing 3,500-line
limit was not relaxed. All wildcard imports under the app module were removed.

Forty tracked generated evaluation-workspace files were removed and their specific
target directory ignored. Intentional evaluation source, reports, and frozen task
fixtures remain tracked. The exact removed inventory is in
[the check record](ASTRA-M03-CHECKS.json).

## Validation

| Check | Result |
| --- | --- |
| API regression suite | 223 passed, 0 failed |
| Administrative binary tests | 1 passed, 0 failed |
| Final caveat regression after MSRV-compatible cleanup | 1 passed |
| Lexer/import regression suite | 5 passed |
| Existing module size/import/dependency guard | Pass; includes lexer tests |
| Strict Clippy, all pharness-api targets | Pass, warnings denied |
| Workspace Rust formatting and Git whitespace checks | Pass |

Commands used: `cargo test -p pharness-api -- --nocapture`,
`cargo test -p pharness-api verifier_caveats_are_sealed_without_false_approval`,
`cargo clippy -p pharness-api --all-targets -- -D warnings`,
`cargo fmt --all --check`, and `scripts/check-app-module-boundaries.sh`.
Cargo caches were outside the checkout. The final implementation uses the declared
Rust 1.80-compatible optional-value check; an initial Clippy incompatibility was
corrected. No acceptance threshold or lint gate was lowered.

## Finding disposition and limits

- F01: upstream closure fix revalidated; controller crash/restart proof remains M06.
- F02: normalized evidence semantics implemented and tested; visual presentation remains M10.
- F11: existing architecture guardrails pass.
- F12: generated workspace removal and ignore rule are complete.

These deterministic tests do not qualify the coding provider or prove an autonomous
application release. Those are M04 and M11 gates. No image was built or deployment
promoted for M03. No database migration was needed. Rollback of this source change
requires a normal immutable release and must not rewrite stored evidence. The later
hosted-contract migration will define its own minimum compatible rollback release.
