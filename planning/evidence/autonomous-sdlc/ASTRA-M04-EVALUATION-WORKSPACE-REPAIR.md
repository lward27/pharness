# ASTRA M04: Evaluation workspace retention repair

Status: implementation tested; live coding qualification remains unaccepted.
Observed 2026-09-05 in `lucas_engineering`. Source base:
`97f65557870154c9b02ba5a71d83a006ad6e638e`.

## Observed failure

The authenticated Kimi K2.7 Code protocol preflight passed 30/30. The subsequent
frozen two-run coding evaluation `infeval_01a070d9cc217571ae42e952c28c69c7`
executed on runtime `83a2689c877a3f48688d1d457c34e83474698c46`.
Its Job `pharness-inference-eval-f54b7c034ff1` failed at 09:51:08 UTC.

The Pod's explicit eviction reason was:
`Usage of EmptyDir volume "tmp" exceeds the limit "1Gi".`
The container exited 137. Its Job deadline was 7,215 seconds and had not expired.
The Pod had the existing two-CPU, 2-GiB memory, and 4-GiB ephemeral-storage limits.
This is storage exhaustion, not evidence of a provider-account failure or a
failed model-quality threshold. No final qualification report was produced;
partial logs cannot establish an aggregate score or a qualifying run.

## Implementation

The Coding Reliability V2 evaluator retained every fixture's complete temporary
workspace after persisting its separate evidence copy. Rust build directories and
Python virtual environments therefore accumulated across cases and independent
runs. The repair gives each fixture a uniquely owned scratch directory, releases
it after evidence capture and scoring, and stops if explicit cleanup fails.
Preparation failures and abandoned futures also release that owned directory.
It never removes an existing directory to begin a new fixture.

The existing shared V2 fixture path gets this lifecycle correction; no coding
backend, provider, prompt, test, score, retry policy, or execution limit changes.
The native-host path is not expanded or qualified by this change.

## Validation

All 20 evaluator tests passed on the Mac. The frozen replay test now executes
both independent 24-case runs and checks that their scratch directories are gone.
All 48 replay cases passed public and hidden checks without safety violations.
Seeded semantic defects remain rejected. An error-path test verifies that one
abandoned fixture cannot delete another fixture's workspace.

The 48 separate source-evidence directories remain present. Inspection confirmed
that they contain the expected source and documentation, without Git internals,
compiler targets, virtual environments, Python caches, or Node dependencies.
The frozen fixture revision remains `coding-reliability-v2.1`; its suite hash is
unchanged:
`sha256:4bf3fce21f86369794ac6e57816436ff331e7dd607eb303baaf720c885583767`.

Formatting, Clippy with warnings denied, and the existing architecture checks
passed, including all five dependency-parser regressions. The first Clippy pass found redundant borrows introduced
by the ownership change; those were corrected without changing test behavior.

## Deployment and acceptance boundary

Build and release the required immutable PHarness image set from the merged source,
including the evaluation runner and existing native bundle. Record source labels,
digests, exact Argo revision, and live identities. Preserve schema 0051 in this
repair release; the separate uncommitted M05 migration is not part of this fix.

Run fresh protocol and stage qualification against the deployed execution profile.
M04 still requires two real qualifying coding runs, the companion repair and
stage suites, all per-stack thresholds, and no hidden-test false passes or policy
violations. These deterministic tests prove the retention correction, not live
coding quality. Do not enlarge the temporary volume or any model budget to avoid
the acceptance gate. The failed evaluation remains retained as failed evidence.
