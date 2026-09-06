# ASTRA M04B: Explicit context and inspectable recovery

Date: 2026-09-06. Source: `a06eb27bb7c09593d002de877f2e2bfcc2cb3b85`. Base: `ae4eed39be1260dc2cc9f9fbeb1f8586d66fb5fd`.
Status: implemented and locally validated; live provider canary and deployment pending.
Authority: [approved M04 refactor](../../programs/autonomous-sdlc/ASTRA-04-CODING-RELIABILITY-QUALIFICATION.md).

## Result

New V2 bindings select one `pharness.dev/agent-context-envelope/v1` initial system
message, followed by the original user request. It separates controller base/stage/
profile instructions from repository guidance, environment facts, repository map
and AgentContext. The model no longer depends on delivery of six independent initial
system sections. Existing per-turn budgets, execution ledgers, recovery messages and
provider-specific serialization remain in the shared runtime.

API planning, qualification binding and both evaluators now use one context-policy
hash constructor. The envelope version is part of that immutable hash. Saved legacy
hashes select the original message format; unsupported hashes stop with a compatible
worker requirement. This does not infer a Run's contract from current defaults.

Approval and budget resumes validate the original envelope before executing a saved
action or calling a model. Checks bind Run/session identity, original request and
AgentContext, stage prompt/content, controller instructions, environment and the
complete payload hash. Removing or modifying required context stops execution.
Even a recomputed checksum cannot replace controller instructions or pinned stage
content. Repository files are not reread to reconstruct the starting context on resume.
The checksum provides local integrity, not a cryptographic claim about provider behavior.

## Diagnostic evidence

V2 RunStarted events retain the initial native messages and their content hash.
Every ModelRequestStarted event records `input_context_sha256` for the exact native
messages handed to the provider, including transient controller instructions.
Gateway/replay stage evaluation reports retain a bounded, redacted starting-context
record alongside submission diagnostics. The 128 KiB limit and redaction/omission
status are explicit; missing context never becomes an invented input record.

This makes a small failed canary inspectable. Native context, provider wire format
and actual upstream ingestion remain separate boundaries. Builder/Repair benchmark
reports retain their existing shape; the shared runtime emits the new starting-context
record, but this slice does not add complete transcript retention to their reports.

## Objective validation

**554 distinct component tests pass**, with one existing live-only core test ignored.
The five affected packages pass all-target Clippy, formatting and the five architecture
checks. The [validation record](ASTRA-M04-CONTEXT-ENVELOPE-VALIDATION.json) and raw logs
record exact revisions and counts. Nested failing public/hidden fixtures in the raw
log are intentional evaluator tests; every outer test target finished successfully.

New coverage exercises:

- The complete initial envelope and selection of the legacy six-section format.
- Missing, modified, foreign and unsupported saved context, including recomputed checksums.
- Compaction that retains mandatory context; too-small budgets fail without dropping it.
- A real shared-runner recoverable tool error followed by a valid native submission.
- Budget pause, Run/transcript serialization, reconstructed execution and retained failure evidence.
- Approval resume, including rejection before the saved action runs when context is missing.
- Actual native requests through the shared provider serializer, with complete initial
  content and non-system transcript identity retained for the MiniMax, Kimi and GLM paths.
- Initial-input retention through the complete stage replay/report path, plus bounded
  redaction and omission cases.

These are local tests with deterministic providers and isolated temporary repositories.
They do not claim live model qualification, a process-kill recovery demonstration,
production approval, or the connected engineering loop required by M04E.

## Compatibility, deployment and remaining gates

There is no database migration or new runtime dependency. A test-only reference to
the already-present shared transport crate lets runner tests exercise its actual
serializer; the lockfile adds only that workspace dependency edge. Stage fixtures,
scorers, numerical thresholds, registered models, profile budgets, repair limits and
hosted activation rules are unchanged by this slice. Coding/repair task data is
unchanged; its profile hash changes through the explicit context-policy version.

Deploy the common immutable API/worker/evaluator artifact set before starting new
bindings. Active envelope Runs require a worker containing this implementation.
Legacy readers can read the additive event data, but are not executable rollback
candidates for active envelope Runs. On incompatibility, retain the Run and use the
original compatible worker; do not rewrite its context to fit a new default.

No new provider request, deployment, Finance source patch or production action was
made for this slice. Hosted creation and Coding Reliability V2 remain disabled.
M04C must replace invalid stage measurements before the small live canaries in M04D.
Actual provider delivery, blind verification, the connected repair loop and full
exact-runtime qualification remain open.

## Design judgment

The useful simplification is a single context contract and one shared construction
path. Keeping explicit legacy handling costs some code, but avoids changing the
meaning of saved Runs during the transition. Prompt wording and model choice can
now be evaluated against an identifiable input; this still does not prove that the
models will use it correctly.
